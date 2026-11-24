//! One device's open organization document: its log, the state replaying it gives, and writing
//! to it. Both apps drive this; the desktop through its tabs, the phone through its screens.
//!
//! [`Session::write`] tries an operation on a copy first and only appends it when it applies:
//! entries are never deleted, so a mistake caught here (no stock left, a member changing
//! prices) never reaches the log. Conflicts can still arise later, when other devices' entries
//! arrive and are ordered before ours; [`Session::conflicts`] lists them.

use std::collections::BTreeSet;

use cadrs_social::keys::{Account, DeviceCert, SecretKey, Signed};
use cadrs_social::log::{Entry, EntryId, Inserted, Kind, Log, LogId, Outcome, Refused, Witnesses};

use crate::{Op, Org};

/// An open organization document.
#[derive(Clone, Debug)]
pub struct Session {
    log: Log,
    key: SecretKey,
    org: Org,
    conflicts: Vec<(EntryId, String)>,
    finals: BTreeSet<EntryId>,
}

impl Session {
    /// Founds a new organization called `name`, from the device with `key` and its `cert`.
    /// Returns the session and the organization's own account (its key goes in the keystore).
    pub fn found(
        key: SecretKey,
        cert: Signed<DeviceCert>,
        name: &str,
        now: u64,
    ) -> (Self, Account) {
        let org = Account::generate();
        let mut session = Self::open(Log::new(LogId::generate()), key);
        session
            .write(
                Op::Found {
                    name: name.to_owned(),
                    org: org.id(),
                    founder: cert,
                },
                now,
            )
            .expect("founding applies to an empty log");
        (session, org)
    }

    /// Opens a log as the device with `key`.
    pub fn open(log: Log, key: SecretKey) -> Self {
        let mut session = Self {
            log,
            key,
            org: Org::default(),
            conflicts: Vec::new(),
            finals: BTreeSet::new(),
        };
        session.replay();
        session
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    pub fn org(&self) -> &Org {
        &self.org
    }

    /// Entries that didn't apply, and why, in the log's order.
    pub fn conflicts(&self) -> &[(EntryId, String)] {
        &self.conflicts
    }

    /// Whether an entry is final (witnessed).
    pub fn is_final(&self, id: &EntryId) -> bool {
        self.finals.contains(id)
    }

    fn replay(&mut self) {
        // Witnesses come from the state, which comes from the order, which depends on the
        // witnesses: settle it by replaying until the witnesses stop changing (twice, nearly
        // always).
        let mut witnesses = Witnesses::new();
        for _ in 0..4 {
            let mut org = Org::default();
            let outcomes = self.log.replay(&witnesses, &mut org);
            let next = org.witnesses(|_| Default::default());
            self.conflicts = outcomes
                .iter()
                .filter_map(|(o, outcome)| match outcome {
                    Outcome::Conflicted(why) => Some((o.id, why.clone())),
                    Outcome::Applied => None,
                })
                .collect();
            self.org = org;
            if next == witnesses {
                break;
            }
            witnesses = next;
        }
        self.finals = self.log.finals(&witnesses);
    }

    /// Writes `op`, if it applies after everything this device has; otherwise why not.
    pub fn write(&mut self, op: Op, now: u64) -> Result<Entry, String> {
        let mut trial = self.clone();
        let entry = trial.log.append(&trial.key, Kind::Data(op.encode()), now);
        trial.replay();
        let id = entry.id();
        if let Some((_, why)) = trial.conflicts.iter().find(|(c, _)| *c == id) {
            return Err(why.clone());
        }
        *self = trial;
        Ok(entry)
    }

    /// Acknowledges what this device has seen (a witness does this now and then).
    pub fn ack(&mut self, now: u64) -> Entry {
        let entry = self.log.append(&self.key, Kind::Ack, now);
        self.replay();
        entry
    }

    /// Takes in entries from another device.
    pub fn receive(
        &mut self,
        entries: impl IntoIterator<Item = Entry>,
        now: u64,
    ) -> Vec<Result<Inserted, Refused>> {
        let results = entries
            .into_iter()
            // Entries of devices not (yet) members are kept: the entry adding them may still be
            // on its way. The reducer refuses what they wrote if they never are.
            .map(|entry| self.log.insert(entry, &cadrs_social::log::Anyone, now))
            .collect();
        self.replay();
        results
    }

    /// The entries another device with `heads` lacks: the first of them, in the log's order, up
    /// to [`SYNC_BUDGET`] (see [`Session::since_within`]).
    pub fn since(&self, heads: &BTreeSet<EntryId>) -> Vec<Entry> {
        self.since_within(heads, SYNC_BUDGET)
    }

    /// The first entries another device with `heads` lacks, in the log's order, that weigh no
    /// more than `budget` bytes together (one at least, when it lacks any). A log is synced a
    /// page at a time: however large it has grown, a page fits what one message carries, the
    /// device that took one asks again with its new heads, and what a page leaves out comes
    /// with the next.
    pub fn since_within(&self, heads: &BTreeSet<EntryId>, budget: usize) -> Vec<Entry> {
        let mut weighs = 0usize;
        self.log
            .since(heads)
            .into_iter()
            .take_while(|entry| {
                let first = weighs == 0;
                weighs += entry_weight(entry);
                first || weighs <= budget
            })
            .collect()
    }
}

/// The most entries weigh in one page of a sync, in bytes: well under what one message carries.
pub const SYNC_BUDGET: usize = 8 << 20;

// A page is well under what one message carries.
const _: () = assert!(SYNC_BUDGET <= cadrs_social::post::MAX_POST / 4);

/// About what an entry weighs in a message: what it holds, and its envelope.
fn entry_weight(entry: &Entry) -> usize {
    let data = match &entry.body.kind {
        Kind::Data(data) => data.len(),
        _ => 0,
    };
    data + 512
}

impl Session {
    /// Whether this session's log has everything a device with `heads` has: nothing more to
    /// ask it for.
    pub fn has_all(&self, heads: &BTreeSet<EntryId>) -> bool {
        heads.iter().all(|id| self.log.get(id).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Material, Role};

    fn founded() -> (Session, Account) {
        let owner = Account::generate();
        let key = SecretKey::generate();
        let cert = owner.certify(key.public(), "laptop", 0);
        let (session, _) = Session::found(key, cert, "Shop", 1_000);
        (session, owner)
    }

    #[test]
    fn founding_makes_the_founder_owner_and_witness() {
        let (session, owner) = founded();
        assert_eq!(session.org().name, "Shop");
        assert_eq!(session.org().members.get(&owner.id()), Some(&Role::Owner));
        // The founder's device is the only witness, so its own entries are final at once.
        let first = *session.log().heads().iter().next().unwrap();
        assert!(session.is_final(&first));
    }

    #[test]
    fn a_write_that_would_conflict_never_reaches_the_log() {
        let (mut session, _) = founded();
        let before = session.log().len();
        let err = session
            .write(
                Op::Stock {
                    material: 42,
                    sheets: 1,
                },
                2_000,
            )
            .unwrap_err();
        assert_eq!(err, "no such material");
        assert_eq!(session.log().len(), before);

        session
            .write(
                Op::SetMaterial {
                    id: 42,
                    material: Some(Material {
                        name: "steel".into(),
                        thickness: 3.0,
                        sheet: [2500.0, 1250.0],
                    }),
                },
                2_000,
            )
            .unwrap();
        session
            .write(
                Op::Stock {
                    material: 42,
                    sheets: 5,
                },
                2_001,
            )
            .unwrap();
        assert_eq!(session.org().available(42), 5);
    }

    #[test]
    fn members_talk_in_channels() {
        let (mut session, owner) = founded();
        session
            .write(
                Op::Say {
                    channel: "#General".into(),
                    text: "Waterjet down for maintenance at 3".into(),
                },
                2_000,
            )
            .unwrap();
        assert_eq!(
            session.org().channels["general"],
            vec![(
                owner.id(),
                "Waterjet down for maintenance at 3".to_string(),
                2_000
            )]
        );
        assert!(
            session
                .write(
                    Op::Say {
                        channel: "general".into(),
                        text: "  ".into(),
                    },
                    2_001,
                )
                .is_err()
        );
    }

    #[test]
    fn two_devices_sync() {
        let (mut laptop, owner) = founded();
        let box_key = SecretKey::generate();
        laptop
            .write(
                Op::AddDevice(owner.certify(box_key.public(), "box", 0)),
                2_000,
            )
            .unwrap();
        let mut shop_box = Session::open(Log::new(laptop.log().id()), box_key);
        shop_box.receive(laptop.since(shop_box.log().heads()), 3_000);
        assert_eq!(shop_box.org().name, "Shop");
        shop_box
            .write(
                Op::SetClient {
                    id: 1,
                    client: Some(crate::Client {
                        name: "Designer".into(),
                        ..Default::default()
                    }),
                },
                3_000,
            )
            .unwrap();
        laptop.receive(shop_box.since(laptop.log().heads()), 4_000);
        assert_eq!(laptop.org().clients.len(), 1);
        assert!(laptop.conflicts().is_empty());
    }

    /// A log too large for one message is synced a page at a time: each answer is a page of
    /// what the asker lacks, the asker takes it and asks again with its new heads, and after
    /// as many rounds as it takes it has everything. A new device joins an organization of any
    /// size this way.
    #[test]
    fn a_large_log_syncs_a_page_at_a_time() {
        let (mut laptop, owner) = founded();
        let box_key = SecretKey::generate();
        laptop
            .write(
                Op::AddDevice(owner.certify(box_key.public(), "box", 0)),
                2_000,
            )
            .unwrap();
        // A lot said in the members' channel: 60 entries of some 4 KB each.
        for n in 0..60u64 {
            let say = Op::Say {
                channel: "general".into(),
                text: format!("{n} {}", "x".repeat(4_000 - 3)),
            };
            laptop.write(say, 3_000 + n).unwrap();
        }
        let whole: usize = laptop
            .log()
            .since(&Default::default())
            .iter()
            .map(entry_weight)
            .sum();
        // A page of 20 KB: the whole log is a dozen of them.
        let budget = 20_000;
        assert!(whole > 10 * budget);
        let mut shop_box = Session::open(Log::new(laptop.log().id()), box_key);
        let mut pages = 0;
        while !shop_box.has_all(laptop.log().heads()) {
            let page = laptop.since_within(shop_box.log().heads(), budget);
            assert!(
                !page.is_empty(),
                "nothing to send while it still lacks entries"
            );
            let weighs: usize = page.iter().map(entry_weight).sum();
            assert!(weighs <= budget, "a page of {weighs} bytes");
            let before = shop_box.log().len();
            shop_box.receive(page, 9_000);
            assert!(shop_box.log().len() > before, "a page that brought nothing");
            pages += 1;
            assert!(pages < 100);
        }
        assert!(pages > 10, "{pages} pages");
        assert_eq!(shop_box.log().len(), laptop.log().len());
        assert_eq!(shop_box.org().channels["general"].len(), 60);
        assert!(
            laptop
                .since_within(shop_box.log().heads(), budget)
                .is_empty()
        );
        // One entry heavier than the budget is still a page of its own: nothing is stuck.
        let heavy = Op::Say {
            channel: "general".into(),
            text: "y".repeat(4_000),
        };
        laptop.write(heavy, 10_000).unwrap();
        assert_eq!(laptop.since_within(shop_box.log().heads(), 100).len(), 1);
    }
}
