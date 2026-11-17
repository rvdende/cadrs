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

    /// The entries another device with `heads` lacks.
    pub fn since(&self, heads: &BTreeSet<EntryId>) -> Vec<Entry> {
        self.log.since(heads)
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
}
