//! Whatever people who aren't members send, what a device keeps for it has a ceiling.
//!
//! The limits are checked one by one where they are made (`shop_life.rs`, `directory.rs`); a
//! limit that was forgotten is found here instead: letters and listings are made at random,
//! each field as large as it will go, and sent through the same doors the app uses. Afterwards
//! what was kept is weighed: the organization's log, a client's book of requests, the
//! marketplace's directory. Nothing in these tests knows which field is the heavy one.

use cadrs_org::capability::{PartSummary, Process};
use cadrs_org::directory::{Directory, Intake, Offer, ShopListing};
use cadrs_org::letters::{self, ClientBook, Message, Sent};
use cadrs_org::profile::ProfileEdit;
use cadrs_org::quote::{Line, Money};
use cadrs_org::session::Session;
use cadrs_org::{Offers, Op, Quote, Stage};
use cadrs_social::contact::{Card, Kind};
use cadrs_social::keys::{Account, DeviceCert, SecretKey, Signed, SignedBytes};

/// A small generator of the same numbers every run (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// Usually a sensible length, sometimes an absurd one.
    fn length(&mut self, sensible: usize, absurd: usize) -> usize {
        match self.below(10) {
            0 => self.below(absurd + 1),
            _ => self.below(sensible + 1),
        }
    }

    fn text(&mut self, sensible: usize, absurd: usize) -> String {
        let n = self.length(sensible, absurd);
        // Mostly one byte a character, sometimes four.
        let c = if self.below(5) == 0 { '𝕏' } else { 'x' };
        std::iter::repeat_n(c, n).collect()
    }
}

struct Stranger {
    account: Account,
    key: SecretKey,
    cert: Signed<DeviceCert>,
}

impl Stranger {
    fn new() -> Self {
        let account = Account::generate();
        let key = SecretKey::generate();
        let cert = account.certify(key.public(), "device", 1);
        Self { account, key, cert }
    }

    fn card(&self, rng: &mut Rng) -> Signed<Card> {
        Card {
            account: self.account.id(),
            kind: Kind::Person,
            name: rng.text(40, 150_000),
            devices: (0..rng.length(3, 20_000))
                .map(|_| self.key.public())
                .collect(),
            mailboxes: (0..rng.length(1, 5_000))
                .map(|_| self.key.public())
                .collect(),
            issued: 1,
        }
        .sign(self.account.key())
    }
}

fn part(rng: &mut Rng) -> PartSummary {
    PartSummary {
        name: rng.text(30, 50_000),
        flat_thickness: Some(10.0),
        size: [200.0, 80.0, 10.0],
        min_hole: Some(8.5),
        min_inside_radius: Some(3.0),
        cut_length: 900.0,
        pierces: 3,
        area: 12_000.0,
        material: Some(rng.text(12, 50_000)),
        tolerance: None,
    }
}

fn quote(rng: &mut Rng) -> Quote {
    Quote {
        id: rng.next() as u128,
        total: Money::from_major(100.0),
        currency: rng.text(3, 10_000),
        lines: (0..rng.length(4, 3_000))
            .map(|_| Line {
                part: rng.text(20, 20_000),
                quantity: 1,
                machine: rng.text(20, 20_000),
                minutes: 1.0,
                machine_cost: Money::from_major(1.0),
                material_cost: Money::from_major(1.0),
                total: Money::from_major(2.0),
            })
            .collect(),
        valid_until: u64::MAX,
        note: rng.text(100, 150_000),
    }
}

/// A message of any kind about request `id` (or a new request), every field at random.
fn message(rng: &mut Rng, from: &Stranger, id: u128) -> Message {
    match rng.below(7) {
        0 | 1 => Message::Request {
            id: rng.next() as u128,
            parts: (0..rng.length(3, 2_000)).map(|_| (part(rng), 1)).collect(),
            notes: rng.text(200, 150_000),
            deadline: None,
            snapshot: vec![7; rng.length(2_000, 150_000)],
            client: from.card(rng),
        },
        2 => Message::Chat {
            request: id,
            text: rng.text(200, 150_000),
        },
        3 => Message::Quote {
            request: id,
            quote: quote(rng),
        },
        4 => Message::Decline {
            request: id,
            reason: rng.text(100, 150_000),
        },
        5 => Message::Review {
            request: id,
            stars: 5,
            text: rng.text(100, 150_000),
        },
        _ => Message::Accept {
            request: id,
            quote: 0,
        },
    }
}

fn manufacturer() -> (Session, Account) {
    let owner = Account::generate();
    let key = SecretKey::generate();
    let (mut shop, org) = Session::found(
        key.clone(),
        owner.certify(key.public(), "laptop", 0),
        "Cape Waterjet",
        1_000,
    );
    shop.write(
        Op::EditProfile(ProfileEdit {
            offers: Some(Offers {
                manufacturing: true,
                designs: false,
            }),
            ..Default::default()
        }),
        1_500,
    )
    .unwrap();
    (shop, org)
}

fn log_bytes(shop: &Session) -> usize {
    let dir = std::env::temp_dir().join(format!(
        "cadrs-org-hostile-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let path = cadrs_org::file::path(&dir);
    cadrs_org::file::save(&path, shop.log()).unwrap();
    let len = std::fs::metadata(&path).unwrap().len() as usize;
    let _ = std::fs::remove_dir_all(&dir);
    len
}

/// Strangers write to a shop, anything at all, as fast as they like, and nobody at the shop
/// does anything about it: the organization's log stops growing at what may wait, every entry
/// is an entry's size, and every card it kept is a card.
#[test]
fn strangers_cannot_grow_a_shops_log_without_end() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut shop, org) = manufacturer();
    let strangers: Vec<Stranger> = (0..45).map(|_| Stranger::new()).collect();
    let mut theirs: Vec<Vec<u128>> = vec![Vec::new(); strangers.len()];
    let (mut sent, mut taken) = (0, 0);
    let mut now = 2_000;
    for _ in 0..900 {
        let who = rng.below(strangers.len());
        let stranger = &strangers[who];
        // About one of their own requests, when they have one.
        let about = match theirs[who].len() {
            0 => rng.next() as u128,
            n => theirs[who][rng.below(n)],
        };
        let message = message(&mut rng, stranger, about);
        now += 1;
        sent += 1;
        let bytes = letters::write(&stranger.key, &stranger.cert, org.id(), message, now);
        // The doors the app takes a letter through: read, turned into an operation, written.
        let Ok((letter, from)) = letters::read(&bytes, &[org.id()]) else {
            continue;
        };
        let Ok(op) = letters::shop_op(shop.org(), from, &letter.message, letter.sent) else {
            continue;
        };
        if shop.write(op, now).is_ok() {
            taken += 1;
            if let Message::Request { id, .. } = letter.message {
                theirs[who].push(id);
            }
        }
    }
    assert!(
        taken > 100,
        "the test sent nothing a shop takes ({taken} of {sent})"
    );
    let org_state = shop.org();
    // What waits has its ceilings, per client and in all.
    let waiting = org_state
        .requests
        .values()
        .filter(|r| r.stage == Stage::Requested)
        .count();
    assert!(waiting <= cadrs_org::MAX_WAITING);
    for stranger in &strangers {
        let own = org_state
            .requests
            .values()
            .filter(|r| r.client == stranger.account.id())
            .count();
        assert!(own <= cadrs_org::MAX_WAITING_PER_CLIENT);
    }
    // Every entry is an entry's size, whatever it holds.
    for entry in shop.log().since(&Default::default()) {
        if let cadrs_social::log::Kind::Data(data) = &entry.body.kind {
            assert!(data.len() <= cadrs_org::MAX_OP, "{} bytes", data.len());
        }
    }
    // Every card kept is one to answer: a name, and devices one could dial.
    for request in org_state.requests.values() {
        let card = &request.card.as_ref().unwrap().value;
        assert!(card.valid() && card.account == request.client);
        let said: usize = request.chat.iter().map(|(_, text, _)| text.len()).sum();
        assert!(request.chat.len() <= cadrs_org::MAX_NOTES_BEFORE_ORDER);
        assert!(said <= cadrs_org::MAX_NOTE_BYTES_BEFORE_ORDER);
    }
    // What a stranger's request costs the log at the most: the request as written, the
    // questions a client may ask before it is an order, and the entries' own envelopes.
    let each = cadrs_org::MAX_REQUEST + cadrs_org::MAX_NOTE_BYTES_BEFORE_ORDER + (8 << 10);
    let weighs = log_bytes(&shop);
    assert!(
        weighs <= cadrs_org::MAX_WAITING * each + (1 << 20),
        "{weighs} bytes of log with {waiting} waiting"
    );

    // (Declining makes room, and strangers fill it again: how much a day of that can add is
    // `shop_life.rs`'s `a_day_of_floods_adds_a_days_share`.)
}

/// A shop writes to someone who once asked it for a quote, anything at all: the client's book
/// keeps a quote, a reason and a conversation's worth for each request it sent, and nothing
/// for requests it never sent.
#[test]
fn a_shop_cannot_grow_a_clients_book_without_end() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let shop = Stranger::new();
    let client = Account::generate();
    let mut book = ClientBook::default();
    let asked: Vec<u128> = (1..=4).collect();
    for id in &asked {
        book.record(
            *id,
            Sent {
                shop: shop.account.id(),
                shop_name: "A Shop".into(),
                parts: vec![],
                notes: String::new(),
                stage: Stage::Requested,
                quote: None,
                decline_reason: None,
                chat: vec![],
                sent: 0,
                delivery: None,
                review: None,
            },
        );
    }
    let mut taken = 0;
    for now in 0..1_500u64 {
        // Mostly about a request the client did send, sometimes about one it never did.
        let about = match rng.below(5) {
            0 => rng.next() as u128,
            _ => asked[rng.below(asked.len())],
        };
        let message = message(&mut rng, &shop, about);
        let bytes = letters::write(&shop.key, &shop.cert, client.id(), message, now);
        let Ok((letter, from)) = letters::read(&bytes, &[client.id()]) else {
            continue;
        };
        taken += usize::from(book.receive(from, &letter.message, now).is_ok());
    }
    assert!(
        taken > 100,
        "the test sent nothing a client takes ({taken})"
    );
    assert_eq!(
        book.sent.len(),
        asked.len(),
        "a request it never sent was kept"
    );
    for sent in book.sent.values() {
        let said: usize = sent.chat.iter().map(|(_, text, _)| text.len()).sum();
        assert!(sent.chat.len() <= letters::MAX_CHAT_LINES);
        assert!(said <= cadrs_org::MAX_NOTE_BYTES);
    }
    // A quote and a reason are a letter's worth each, a conversation its own: a ceiling for
    // each request that doesn't depend on how much the shop wrote.
    let weighs = cadrs_social::codec::encode(&book).len();
    let ceiling = asked.len() * (2 * letters::MAX_LETTER + 2 * cadrs_org::MAX_NOTE_BYTES);
    assert!(weighs <= ceiling, "{weighs} bytes of book");
}

/// Anyone says listings in the public room, anything at all, properly signed or not: what the
/// directory keeps is so many shops, each a listing's size, each something a listing may say.
#[test]
fn listings_cannot_grow_the_directory_without_end() {
    const NOW: u64 = 1_800_000_000_000;
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let mut directory = Directory::default();
    let mut intake = Intake::default();
    let mut secs = 0.0;
    for round in 0..120 {
        let mut said = Vec::new();
        for _ in 0..20 {
            let shop = Stranger::new();
            let mut card = shop.card(&mut rng).value;
            card.kind = if rng.below(6) == 0 {
                Kind::Person
            } else {
                Kind::Organization
            };
            let mut listing = ShopListing::withdrawal(card.sign(shop.account.key()), NOW);
            listing.withdrawn = rng.below(10) == 0;
            listing.tagline = rng.text(60, 5_000);
            listing.about = rng.text(200, 50_000);
            listing.email = rng.text(20, 5_000);
            listing.website = Some(rng.text(20, 5_000));
            listing.rating = Some((5.0, usize::MAX));
            listing.offers = (0..rng.length(2, 200))
                .map(|_| Offer {
                    process: Process::Laser,
                    envelope: [
                        3000.0,
                        if rng.below(8) == 0 { f64::NAN } else { 1500.0 },
                        0.0,
                    ],
                    materials: (0..rng.length(2, 100))
                        .map(|_| (rng.text(10, 2_000), 10.0))
                        .collect(),
                })
                .collect();
            // Signed as it stands, cut to fit as the app would, or not signed at all.
            let bytes = match rng.below(3) {
                0 => cadrs_social::codec::encode(&SignedBytes::new(shop.account.key(), &listing)),
                1 => cadrs_org::directory::sign(shop.account.key(), &listing).unwrap_or_default(),
                _ => vec![rng.next() as u8; rng.length(500, 100_000)],
            };
            said.push(bytes);
        }
        intake.queue(said);
        secs += f64::from(round % 3);
        intake.take(&mut directory, secs, NOW);
        assert!(intake.waiting() <= cadrs_org::directory::MAX_HEARD_WAITING);
    }
    assert!(
        !directory.is_empty(),
        "the test said nothing a directory takes"
    );
    assert!(directory.len() <= cadrs_org::directory::MAX_SHOPS);
    for listing in directory.listings() {
        assert!(listing.valid());
        assert_eq!(listing.rating, None);
    }
    let kept = directory.len().max(1) + 120;
    let weighs = directory.save().len();
    assert!(
        weighs <= kept * (cadrs_org::directory::MAX_LISTING + 64),
        "{weighs} bytes of directory"
    );
}
