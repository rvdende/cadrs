//! A bank: the books, the key that signs their receipts, and the file every effect is written
//! to before it counts. [`Bank::handle`] takes an ask as it arrived and gives the answer to
//! send back, with what to tell the devices of whoever else it concerns.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cadrs_social::codec;
use cadrs_social::keys::{SecretKey, SignedBytes};

use crate::book::{Book, Decision, Effect, Level};
use crate::refusal as why;
use crate::{AccountId, Answer, Ask, Asked, CLOCK_SLACK, DeviceId, MAX_ASKED, Movement, Receipt};

/// How many times a device may ask in a minute.
pub const ASKS_A_MINUTE: u32 = 240;
/// How many devices are told of a wallet's movements, and for how long after each last asked
/// about it (milliseconds): a week.
pub const MAX_WATCHERS: usize = 16;
pub const WATCH_FOR: u64 = 7 * 24 * 60 * 60 * 1000;
/// How many devices' asking is counted at once (the oldest counts are dropped first).
const MAX_COUNTED: usize = 100_000;

/// How a journal starts: so that a file that isn't one is never read as one, and a later
/// format is told from this.
const MAGIC: &[u8; 8] = b"cadrsJ1\n";
/// The most bytes one record may be (an effect is a few hundred).
const MAX_RECORD: usize = 64 << 10;
/// What goes before a record: its length (four bytes, little end first) and the first eight
/// bytes of its BLAKE3 hash.
const HEAD: usize = 12;

fn check(record: &[u8]) -> [u8; 8] {
    let hash = blake3::hash(record);
    let mut sum = [0; 8];
    sum.copy_from_slice(&hash.as_bytes()[..8]);
    sum
}

/// Whether a whole record (a length, its checksum, and that many bytes that match it) starts
/// anywhere in `tail` after its first byte.
fn holds_a_record(tail: &[u8]) -> bool {
    (1..tail.len().saturating_sub(HEAD)).any(|at| {
        let rest = &tail[at..];
        let len = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        // (No effect is this short: a few zero bytes aren't taken for an empty record.)
        (8..=MAX_RECORD).contains(&len)
            && rest
                .get(HEAD..HEAD + len)
                .is_some_and(|record| rest[4..HEAD] == check(record))
    })
}

fn damaged(what: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, what)
}

/// The file a bank writes every effect to, one after another, each with its length and a
/// checksum. What was written is what the books are.
///
/// Reading it back is strict, because a wrong guess moves everyone's balances:
/// - A last record cut short by a crash (the file ends inside it, or its checksum fails and
///   nothing follows) is dropped: it never counted, since nobody was answered before it was
///   written whole. What was dropped is kept beside the journal (`credits.journal.cut`, then
///   `.cut.2`, …; none is written over), and [`Journal::open`] says how many bytes.
/// - Anything else that doesn't read (a record in the middle whose checksum fails, a length
///   that can't be, an effect a later cadrs wrote that this one doesn't know) stops the bank
///   from starting, with where and why. Nothing is cut and nothing is written: the file is as
///   it was, for a backup to be compared with or a newer cadrs to read.
///
/// It is read and written with `std::fs`, not `cadrs_fs`: a bank with a journal is the seed's,
/// which never runs in a page (a bank kept in the app is in memory).
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    /// Opens (or starts) the journal at `path` and reads what it holds: the effects, and how
    /// many bytes of a torn last record were cut off.
    pub fn open(path: &Path) -> std::io::Result<(Self, Vec<Effect>, usize)> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let journal = Self {
            path: path.to_owned(),
        };
        if bytes.is_empty() {
            std::fs::write(path, MAGIC)?;
            std::fs::File::open(path)?.sync_all()?;
            return Ok((journal, Vec::new(), 0));
        }
        if bytes.get(..MAGIC.len()) != Some(&MAGIC[..]) {
            return Err(damaged(format!(
                "{} is not a bank's journal (or is a later cadrs's): it doesn't start as one",
                path.display()
            )));
        }
        let mut effects = Vec::new();
        let mut at = MAGIC.len();
        while at < bytes.len() {
            let rest = &bytes[at..];
            let whole = |len: usize| rest.len() >= HEAD + len;
            let len = rest
                .get(..4)
                .map(|l| u32::from_le_bytes([l[0], l[1], l[2], l[3]]) as usize);
            // The file ends inside this record: the crash's torn write, if it is no more than
            // one record could be.
            let torn = match len {
                None => true,
                Some(len) if len > MAX_RECORD => {
                    return Err(damaged(format!(
                        "{} is damaged at byte {at} (record {}): a record of {len} bytes \
                         can't be. Nothing was changed; restore it from a backup.",
                        path.display(),
                        effects.len() + 1
                    )));
                }
                Some(len) => !whole(len),
            };
            if torn {
                break;
            }
            let len = len.unwrap_or(0);
            let (sum, record) = (&rest[4..HEAD], &rest[HEAD..HEAD + len]);
            let last = rest.len() == HEAD + len;
            if sum != check(record) {
                if last {
                    // Written in part when the machine stopped.
                    break;
                }
                return Err(damaged(format!(
                    "{} is damaged at byte {at} (record {}): its checksum fails and {} bytes \
                     follow it. Nothing was changed; restore it from a backup.",
                    path.display(),
                    effects.len() + 1,
                    rest.len() - HEAD - len
                )));
            }
            let effect = codec::decode::<Effect>(record).map_err(|e| {
                damaged(format!(
                    "{} holds, at byte {at} (record {}), {}. Nothing was changed.",
                    path.display(),
                    effects.len() + 1,
                    if e.is_newer() {
                        "something a later cadrs wrote: start the bank with that cadrs"
                    } else {
                        "a record that is whole and can't be read"
                    }
                ))
            })?;
            effects.push(effect);
            at += HEAD + len;
        }
        let cut = bytes.len() - at;
        if cut > 0 {
            // A torn write is the start of one record. A tail with a whole, checksummed record
            // somewhere in it is not that: something before that record is damaged (a length,
            // which made what follows look like one long torn record).
            if holds_a_record(&bytes[at..]) {
                return Err(damaged(format!(
                    "{} is damaged at byte {at} (record {}): what follows doesn't read, and \
                     whole records come after it. Nothing was changed; restore it from a backup.",
                    path.display(),
                    effects.len() + 1
                )));
            }
            if cut > HEAD + MAX_RECORD {
                return Err(damaged(format!(
                    "{} is damaged at byte {at}: {cut} bytes that are no record follow. \
                     Nothing was changed; restore it from a backup.",
                    path.display()
                )));
            }
            // What follows the last whole record is kept aside, never over an earlier one.
            let aside = (1..)
                .map(|n| {
                    let mut name = path.as_os_str().to_owned();
                    name.push(if n == 1 {
                        ".cut".to_owned()
                    } else {
                        format!(".cut.{n}")
                    });
                    PathBuf::from(name)
                })
                .find(|aside| !aside.exists())
                .expect("a free name");
            std::fs::write(&aside, &bytes[at..])?;
            let file = std::fs::OpenOptions::new().write(true).open(path)?;
            file.set_len(at as u64)?;
            file.sync_all()?;
        }
        Ok((journal, effects, cut))
    }

    fn append(&self, effect: &Effect) -> std::io::Result<()> {
        use std::io::Write;
        let record = codec::encode(effect);
        if record.len() > MAX_RECORD {
            return Err(damaged("an effect larger than a record may be".into()));
        }
        let mut bytes = (record.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&check(&record));
        bytes.extend_from_slice(&record);
        let mut file = std::fs::OpenOptions::new().append(true).open(&self.path)?;
        file.write_all(&bytes)?;
        file.sync_data()
    }
}

/// Something to tell devices: a receipt, as bytes for `Post::Credited`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Push {
    pub to: Vec<DeviceId>,
    pub receipt: Vec<u8>,
}

/// What the bank does with an ask it took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handled {
    /// The answer for the asker ([`crate::read_answer`]).
    pub answer: Vec<u8>,
    pub pushes: Vec<Push>,
}

pub struct Bank {
    key: SecretKey,
    book: Book,
    journal: Option<Journal>,
    /// The devices that asked about each wallet lately, whose they are, and when: told as it
    /// changes, while their account may still look at it.
    watchers: BTreeMap<AccountId, Vec<(DeviceId, AccountId, u64)>>,
    /// How often each device asked this minute: (the minute, the count).
    asked: BTreeMap<DeviceId, (u64, u32)>,
    /// How many bytes of a torn last record were cut off the journal when it was read (kept
    /// beside it): worth a line in the log.
    pub cut: usize,
}

impl Bank {
    /// A bank that keeps its books in memory only (a test's, a scenario's).
    pub fn in_memory(key: SecretKey) -> Self {
        Self {
            key,
            book: Book::default(),
            journal: None,
            watchers: BTreeMap::new(),
            asked: BTreeMap::new(),
            cut: 0,
        }
    }

    /// A bank whose books are the journal in `dir` (`credits.journal`), read back now.
    pub fn open(key: SecretKey, dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let (journal, effects, cut) = Journal::open(&dir.join("credits.journal"))?;
        let mut bank = Self::in_memory(key);
        bank.cut = cut;
        for effect in &effects {
            bank.book.apply(effect);
        }
        bank.journal = Some(journal);
        Ok(bank)
    }

    /// The bank's id: the key its receipts are signed with (as a peer, its cadrs ID).
    pub fn id(&self) -> AccountId {
        self.key.public()
    }

    pub fn book(&self) -> &Book {
        &self.book
    }

    /// Takes an ask as it arrived from device `from` (none when it didn't come over the
    /// network: the app's own, of a bank it keeps itself) at `now`, the bank's clock.
    pub fn handle(
        &mut self,
        bytes: &[u8],
        from: Option<DeviceId>,
        now: u64,
    ) -> Result<Handled, String> {
        if bytes.len() > MAX_ASKED {
            return Err(why::TOO_LARGE.into());
        }
        if let Some(from) = from {
            self.count(from, now)?;
        }
        let asked: Asked = codec::decode(bytes).map_err(|e| {
            if e.is_newer() {
                "this bank's cadrs is older than yours: it can't take that yet".to_string()
            } else {
                "not something cadrs asks a bank".to_string()
            }
        })?;
        let signer = asked.request.signer;
        let request = asked.request.open().map_err(|_| why::SIGNATURE)?;
        if request.bank != self.id() {
            return Err(why::OTHER_BANK.into());
        }
        if request.time.abs_diff(now) > CLOCK_SLACK {
            return Err(why::CLOCK.into());
        }
        // Whose word it is: an account's own key, or a device its account vouches for.
        let (by, own_key) = match &asked.cert {
            None => (signer, true),
            Some(cert) => {
                let value = cert.verify().map_err(|_| why::SIGNATURE)?;
                if cert.signer != value.account || value.device != signer {
                    return Err(why::SIGNATURE.into());
                }
                // A device's ask comes from that device, not from one that overheard it.
                if from.is_some_and(|from| from != signer) {
                    return Err(why::SIGNATURE.into());
                }
                (value.account, false)
            }
        };
        let wallet = request.wallet;
        let decision = self
            .book
            .decide(&wallet, &by, own_key, &request.ask, request.time, now)?;
        let answer = |answer: &Answer| codec::encode(answer);
        match decision {
            Decision::Unchanged => Ok(Handled {
                answer: answer(&Answer::Done),
                pushes: Vec::new(),
            }),
            Decision::Statement(statement) => {
                if let Some(device) = from.or((!own_key).then_some(signer)) {
                    self.watch(wallet, device, by, now);
                }
                Ok(Handled {
                    answer: answer(&Answer::Statement(statement)),
                    pushes: Vec::new(),
                })
            }
            Decision::Already(movement) => Ok(Handled {
                answer: answer(&Answer::Receipt(self.receipt(movement))),
                pushes: Vec::new(),
            }),
            Decision::Effect(effect) => {
                // Written down first: what isn't on file didn't happen.
                if let Some(journal) = &self.journal {
                    journal.append(&effect).map_err(|_| why::JOURNAL)?;
                }
                self.book.apply(&effect);
                let Effect::Move(movement) = effect else {
                    return Ok(Handled {
                        answer: answer(&Answer::Done),
                        pushes: Vec::new(),
                    });
                };
                let asker = from.or((!own_key).then_some(signer));
                if let (Some(device), Ask::Faucet { .. }) = (asker, &request.ask) {
                    self.watch(wallet, device, by, now);
                }
                let receipt = self.receipt(movement.clone());
                let pushes = self.pushes(&movement, &receipt, asker, now);
                Ok(Handled {
                    answer: answer(&Answer::Receipt(receipt)),
                    pushes,
                })
            }
        }
    }

    fn receipt(&self, movement: Movement) -> SignedBytes<Receipt> {
        SignedBytes::new(
            &self.key,
            &Receipt {
                bank: self.id(),
                movement,
            },
        )
    }

    /// Who is told of `movement`: the devices watching either side, but the one that asked.
    fn pushes(
        &mut self,
        movement: &Movement,
        receipt: &SignedBytes<Receipt>,
        asker: Option<DeviceId>,
        now: u64,
    ) -> Vec<Push> {
        let mut to: Vec<DeviceId> = Vec::new();
        for wallet in movement.from.iter().chain([&movement.to]) {
            let Some(watching) = self.watchers.get_mut(wallet) else {
                continue;
            };
            // Whoever asked lately and may still look: a member let go since is told nothing.
            let book = &self.book;
            watching.retain(|(_, account, seen)| {
                seen.saturating_add(WATCH_FOR) > now && book.may(account, wallet) >= Level::Look
            });
            for (device, ..) in watching.iter() {
                if Some(*device) != asker && !to.contains(device) {
                    to.push(*device);
                }
            }
        }
        if to.is_empty() {
            return Vec::new();
        }
        vec![Push {
            to,
            receipt: codec::encode(receipt),
        }]
    }

    /// `device` asked about `wallet` and may look at it: it is told when the wallet changes.
    fn watch(&mut self, wallet: AccountId, device: DeviceId, account: AccountId, now: u64) {
        let watching = self.watchers.entry(wallet).or_default();
        watching.retain(|(d, _, seen)| d != &device && seen.saturating_add(WATCH_FOR) > now);
        watching.push((device, account, now));
        if watching.len() > MAX_WATCHERS {
            watching.remove(0);
        }
    }

    fn count(&mut self, device: DeviceId, now: u64) -> Result<(), String> {
        let minute = now / 60_000;
        if self.asked.len() >= MAX_COUNTED {
            // Earlier minutes' counts go. If this minute alone fills it, a device not yet
            // counted waits for the next: nobody's count is forgotten to make room.
            self.asked.retain(|_, (m, _)| *m == minute);
            if self.asked.len() >= MAX_COUNTED && !self.asked.contains_key(&device) {
                return Err(why::TOO_OFTEN.into());
            }
        }
        let (counted, count) = self.asked.entry(device).or_insert((minute, 0));
        if *counted != minute {
            (*counted, *count) = (minute, 0);
        }
        *count += 1;
        if *count > ASKS_A_MINUTE {
            return Err(why::TOO_OFTEN.into());
        }
        Ok(())
    }
}
