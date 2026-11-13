//! An append-only log of signed entries that every peer puts in the same order.
//!
//! Each [`Entry`] names the entries it builds on (its parents: the log's heads when it was
//! written), carries a [`Timestamp`] later than all of them, and is signed by the device that
//! wrote it. Its id is the hash of all that, so an entry proves its own place in history. Peers
//! that were apart write side by side; when they meet, they exchange entries and the branches
//! simply merge. Nothing is ever deleted.
//!
//! **One order for everyone.** An entry's key is (time, author, id). Because an entry's time is
//! later than its parents', sorting by key already puts every entry after the ones it builds on,
//! so two peers holding the same entries compute the same order with no vote.
//!
//! **Finality.** A peer that comes back from a long time apart can bring entries whose keys sort
//! before what everyone else has already acted on (a sheet of steel that was reserved twice).
//! So some devices are **witnesses**, each with a weight (an org gives it from roles and
//! reputation). A witness acknowledges what it has seen just by writing an entry (an
//! [`Kind::Ack`] if it has nothing else to say), since an entry builds on everything its author
//! had. An entry is **final** once witnesses with more than half the total weight have built on
//! it. Final entries are ordered first, in rounds by the final entry that first covered them,
//! and everything not yet final comes after, by key: so a late branch lands after what is
//! final instead of rewriting it, and every peer still agrees, because finality is itself
//! computed from the entries.
//!
//! **Conflicts** are the [`Reducer`]'s to find: it replays entries in order and may answer
//! [`Outcome::Conflicted`] (the stock is gone); every peer finds the same conflicts.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::hlc::{Clock, Timestamp};
use crate::keys::{self, DeviceId, SecretKey, Signature};

const CONTEXT: &str = "cadrs log entry 1";

/// Which log an entry belongs to (an organization document, a chat), so an entry can't be
/// replayed into another.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LogId(pub [u8; 32]);

impl LogId {
    pub fn generate() -> Self {
        Self(crate::random32())
    }
}

impl fmt::Debug for LogId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "log {}…", &crate::hex(&self.0)[..8])
    }
}

/// An entry's id: the hash of the signed entry.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntryId(pub [u8; 32]);

impl fmt::Debug for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}…", &crate::hex(&self.0)[..8])
    }
}

/// What an entry says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Something for the log's reducer (encoded by whoever owns the log, encrypted or not).
    Data(Vec<u8>),
    /// Nothing: a witness saying it has seen the entries this one builds on.
    Ack,
}

/// The signed part of an entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    pub log: LogId,
    /// The entries this one builds on, sorted, without repeats.
    pub parents: Vec<EntryId>,
    pub time: Timestamp,
    pub author: DeviceId,
    pub kind: Kind,
}

/// A signed entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub body: Body,
    pub signature: Signature,
}

impl Entry {
    pub fn sign(key: &SecretKey, mut body: Body) -> Self {
        body.author = key.public();
        body.parents.sort();
        body.parents.dedup();
        let bytes = postcard::to_allocvec(&body).expect("entries encode");
        let signature = key.sign(CONTEXT, &bytes);
        Self { body, signature }
    }

    pub fn id(&self) -> EntryId {
        let bytes = postcard::to_allocvec(self).expect("entries encode");
        EntryId(*blake3::hash(&bytes).as_bytes())
    }

    fn verify(&self) -> Result<(), keys::Error> {
        let bytes = postcard::to_allocvec(&self.body).map_err(|_| keys::Error::Encoding)?;
        keys::verify(&self.body.author, CONTEXT, &bytes, &self.signature)
    }

    /// The order key: (time, author, id).
    fn key(&self, id: EntryId) -> Key {
        (self.body.time, self.body.author, id)
    }

    pub fn data(&self) -> Option<&[u8]> {
        match &self.body.kind {
            Kind::Data(bytes) => Some(bytes),
            Kind::Ack => None,
        }
    }
}

type Key = (Timestamp, DeviceId, EntryId);

/// Why an entry was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    WrongLog,
    BadSignature,
    /// Its parents are not sorted or repeat one.
    BadParents,
    /// Its time is not later than all its parents'.
    NotAfterParents,
    /// Its time is too far ahead of our clock.
    TooFarAhead,
    /// Its author may not write to this log (then).
    NotAllowed,
}

/// What became of an entry handed to [`Log::insert`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inserted {
    /// It (and the entries that were waiting for it, after it) joined the log.
    Added(Vec<EntryId>),
    /// The log already has it.
    Known,
    /// It waits for parents the log doesn't have yet: ask a peer for these.
    Waiting(Vec<EntryId>),
}

/// Who may write to a log, and when: an org's members, a chat's people.
pub trait Writers {
    fn may_write(&self, author: &DeviceId, time: Timestamp) -> bool;
}

/// Anyone may write (tests, public rooms).
pub struct Anyone;

impl Writers for Anyone {
    fn may_write(&self, _: &DeviceId, _: Timestamp) -> bool {
        true
    }
}

impl<F: Fn(&DeviceId, Timestamp) -> bool> Writers for F {
    fn may_write(&self, author: &DeviceId, time: Timestamp) -> bool {
        self(author, time)
    }
}

/// The witnesses of a log and their weights.
pub type Witnesses = BTreeMap<DeviceId, u64>;

/// One entry in the log's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ordered {
    pub id: EntryId,
    /// Witnesses with more than half the weight have built on it.
    pub is_final: bool,
}

/// What a reducer made of one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    /// The entry can't apply after what came before it (the stock it reserves is gone); it is
    /// kept, and its author is told why.
    Conflicted(String),
}

/// Builds a state from a log's entries, in order.
pub trait Reducer {
    fn apply(&mut self, id: EntryId, entry: &Entry, data: &[u8]) -> Outcome;
}

/// A peer's copy of one log.
#[derive(Clone, Debug)]
pub struct Log {
    id: LogId,
    entries: HashMap<EntryId, Entry>,
    heads: BTreeSet<EntryId>,
    /// Entries whose parents haven't all arrived, by id.
    waiting: HashMap<EntryId, Entry>,
    clock: Clock,
}

impl Log {
    pub fn new(id: LogId) -> Self {
        Self {
            id,
            entries: HashMap::new(),
            heads: BTreeSet::new(),
            waiting: HashMap::new(),
            clock: Clock::default(),
        }
    }

    pub fn id(&self) -> LogId {
        self.id
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: &EntryId) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// The entries nothing builds on yet.
    pub fn heads(&self) -> &BTreeSet<EntryId> {
        &self.heads
    }

    /// Parents that entries are waiting for and nobody has handed us yet.
    pub fn missing(&self) -> BTreeSet<EntryId> {
        self.waiting
            .values()
            .flat_map(|entry| &entry.body.parents)
            .filter(|id| !self.entries.contains_key(id) && !self.waiting.contains_key(id))
            .copied()
            .collect()
    }

    /// Writes a new entry on top of everything this peer has, at wall time `wall`.
    pub fn append(&mut self, key: &SecretKey, kind: Kind, wall: u64) -> Entry {
        let entry = Entry::sign(
            key,
            Body {
                log: self.id,
                parents: self.heads.iter().copied().collect(),
                time: self.clock.stamp(wall),
                author: key.public(),
                kind,
            },
        );
        self.add(entry.id(), entry.clone());
        entry
    }

    /// Takes in an entry from a peer.
    pub fn insert(
        &mut self,
        entry: Entry,
        writers: &dyn Writers,
        wall: u64,
    ) -> Result<Inserted, Refused> {
        let id = entry.id();
        if self.entries.contains_key(&id) || self.waiting.contains_key(&id) {
            return Ok(Inserted::Known);
        }
        let body = &entry.body;
        if body.log != self.id {
            return Err(Refused::WrongLog);
        }
        if !body.parents.windows(2).all(|w| w[0] < w[1]) {
            return Err(Refused::BadParents);
        }
        entry.verify().map_err(|_| Refused::BadSignature)?;
        if !writers.may_write(&body.author, body.time) {
            return Err(Refused::NotAllowed);
        }
        self.clock
            .check(body.time, wall)
            .map_err(|_| Refused::TooFarAhead)?;
        let missing: Vec<EntryId> = body
            .parents
            .iter()
            .filter(|parent| !self.entries.contains_key(parent))
            .copied()
            .collect();
        if !missing.is_empty() {
            self.waiting.insert(id, entry);
            return Ok(Inserted::Waiting(missing));
        }
        self.check_after_parents(&entry)?;
        let mut added = vec![id];
        self.add(id, entry);
        // Entries that were waiting for this one, and those waiting for them.
        let mut i = 0;
        while i < added.len() {
            let ready: Vec<EntryId> = self
                .waiting
                .iter()
                .filter(|(_, waiting)| {
                    waiting.body.parents.contains(&added[i])
                        && waiting
                            .body
                            .parents
                            .iter()
                            .all(|parent| self.entries.contains_key(parent))
                })
                .map(|(id, _)| *id)
                .collect();
            for ready in ready {
                let entry = self.waiting.remove(&ready).expect("waiting");
                if self.check_after_parents(&entry).is_ok() {
                    self.add(ready, entry);
                    added.push(ready);
                }
            }
            i += 1;
        }
        Ok(Inserted::Added(added))
    }

    fn check_after_parents(&self, entry: &Entry) -> Result<(), Refused> {
        let after = entry
            .body
            .parents
            .iter()
            .all(|parent| self.entries[parent].body.time < entry.body.time);
        if after {
            Ok(())
        } else {
            Err(Refused::NotAfterParents)
        }
    }

    fn add(&mut self, id: EntryId, entry: Entry) {
        for parent in &entry.body.parents {
            self.heads.remove(parent);
        }
        self.heads.insert(id);
        self.clock.observe(entry.body.time, u64::MAX).ok();
        self.entries.insert(id, entry);
    }

    /// `ids` and everything they build on.
    pub fn ancestry(&self, ids: impl IntoIterator<Item = EntryId>) -> BTreeSet<EntryId> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<EntryId> = ids.into_iter().collect();
        while let Some(id) = stack.pop() {
            if let Some(entry) = self.entries.get(&id)
                && seen.insert(id)
            {
                stack.extend(&entry.body.parents);
            }
        }
        seen
    }

    /// The entries a peer whose heads are `theirs` lacks, parents first: what to send it.
    pub fn since(&self, theirs: &BTreeSet<EntryId>) -> Vec<Entry> {
        let have = self.ancestry(theirs.iter().copied());
        let mut send: Vec<(Key, &Entry)> = self
            .entries
            .iter()
            .filter(|(id, _)| !have.contains(id))
            .map(|(id, entry)| (entry.key(*id), entry))
            .collect();
        send.sort_by_key(|(key, _)| *key);
        send.into_iter().map(|(_, entry)| entry.clone()).collect()
    }

    /// The final entries: those witnesses with more than half the weight have built on.
    pub fn finals(&self, witnesses: &Witnesses) -> BTreeSet<EntryId> {
        let total: u64 = witnesses.values().sum();
        if total == 0 {
            return BTreeSet::new();
        }
        let mut weight: HashMap<EntryId, u64> = HashMap::new();
        for (witness, &w) in witnesses {
            if w == 0 {
                continue;
            }
            let own = self
                .entries
                .iter()
                .filter(|(_, entry)| &entry.body.author == witness)
                .map(|(id, _)| *id);
            for id in self.ancestry(own) {
                *weight.entry(id).or_default() += w;
            }
        }
        weight
            .into_iter()
            .filter(|(_, w)| 2 * w > total)
            .map(|(id, _)| id)
            .collect()
    }

    /// Every entry in the one order all peers with the same entries agree on.
    pub fn order(&self, witnesses: &Witnesses) -> Vec<Ordered> {
        let finals = self.finals(witnesses);
        // Each entry's round: the key of the first final entry (by key) that covers it.
        let mut by_key: Vec<(Key, EntryId)> = finals
            .iter()
            .map(|id| (self.entries[id].key(*id), *id))
            .collect();
        by_key.sort();
        let mut round: HashMap<EntryId, Key> = HashMap::new();
        for (key, id) in by_key {
            let mut stack = vec![id];
            while let Some(id) = stack.pop() {
                if round.contains_key(&id) {
                    continue;
                }
                round.insert(id, key);
                stack.extend(&self.entries[&id].body.parents);
            }
        }
        let mut order: Vec<(Option<Key>, Key, EntryId)> = self
            .entries
            .iter()
            .map(|(id, entry)| (round.get(id).copied(), entry.key(*id), *id))
            .collect();
        // Rounds first (`None`, not yet final, last), then by key.
        order.sort_by_key(|a| (a.0.is_none(), a.0, a.1));
        order
            .into_iter()
            .map(|(_, _, id)| Ordered {
                id,
                is_final: finals.contains(&id),
            })
            .collect()
    }

    /// Replays the log's data entries into `reducer`, in order; what became of each.
    pub fn replay(
        &self,
        witnesses: &Witnesses,
        reducer: &mut dyn Reducer,
    ) -> Vec<(Ordered, Outcome)> {
        self.order(witnesses)
            .into_iter()
            .filter_map(|ordered| {
                let entry = &self.entries[&ordered.id];
                let data = entry.data()?;
                Some((ordered, reducer.apply(ordered.id, entry, data)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(bytes: &[u8]) -> Kind {
        Kind::Data(bytes.to_vec())
    }

    #[test]
    fn entries_build_on_the_heads() {
        let key = SecretKey::generate();
        let mut log = Log::new(LogId::generate());
        let a = log.append(&key, data(b"a"), 1_000);
        let b = log.append(&key, data(b"b"), 1_000);
        assert!(a.body.parents.is_empty());
        assert_eq!(b.body.parents, vec![a.id()]);
        assert!(b.body.time > a.body.time);
        assert_eq!(
            log.heads().iter().copied().collect::<Vec<_>>(),
            vec![b.id()]
        );
    }

    #[test]
    fn refusals() {
        let key = SecretKey::generate();
        let id = LogId::generate();
        let mut theirs = Log::new(id);
        let mut ours = Log::new(id);
        let a = theirs.append(&key, data(b"a"), 1_000);

        let mut other_log = a.clone();
        other_log.body.log = LogId::generate();
        assert_eq!(
            ours.insert(other_log, &Anyone, 1_000),
            Err(Refused::WrongLog)
        );

        let mut tampered = a.clone();
        tampered.body.kind = data(b"b");
        assert_eq!(
            ours.insert(tampered, &Anyone, 1_000),
            Err(Refused::BadSignature)
        );

        let nobody = |_: &DeviceId, _: Timestamp| false;
        assert_eq!(
            ours.insert(a.clone(), &nobody, 1_000),
            Err(Refused::NotAllowed)
        );

        let future = Entry::sign(
            &key,
            Body {
                log: id,
                parents: vec![],
                time: Timestamp::new(1_000_000, 0),
                author: key.public(),
                kind: data(b"later"),
            },
        );
        assert_eq!(
            ours.insert(future, &Anyone, 1_000),
            Err(Refused::TooFarAhead)
        );

        ours.insert(a.clone(), &Anyone, 1_000).unwrap();
        let before_parent = Entry::sign(
            &key,
            Body {
                log: id,
                parents: vec![a.id()],
                time: a.body.time,
                author: key.public(),
                kind: data(b"b"),
            },
        );
        assert_eq!(
            ours.insert(before_parent, &Anyone, 1_000),
            Err(Refused::NotAfterParents)
        );
        assert_eq!(ours.insert(a, &Anyone, 1_000), Ok(Inserted::Known));
    }

    #[test]
    fn entries_wait_for_their_parents() {
        let key = SecretKey::generate();
        let id = LogId::generate();
        let mut theirs = Log::new(id);
        let a = theirs.append(&key, data(b"a"), 1_000);
        let b = theirs.append(&key, data(b"b"), 1_001);
        let c = theirs.append(&key, data(b"c"), 1_002);

        let mut ours = Log::new(id);
        assert_eq!(
            ours.insert(c.clone(), &Anyone, 2_000),
            Ok(Inserted::Waiting(vec![b.id()]))
        );
        assert_eq!(
            ours.insert(b.clone(), &Anyone, 2_000),
            Ok(Inserted::Waiting(vec![a.id()]))
        );
        assert!(ours.is_empty());
        assert_eq!(ours.missing(), BTreeSet::from([a.id()]));
        assert_eq!(
            ours.insert(a.clone(), &Anyone, 2_000),
            Ok(Inserted::Added(vec![a.id(), b.id(), c.id()]))
        );
        assert_eq!(ours.heads(), theirs.heads());
    }

    #[test]
    fn since_sends_what_the_other_lacks_parents_first() {
        let key = SecretKey::generate();
        let id = LogId::generate();
        let mut theirs = Log::new(id);
        let a = theirs.append(&key, data(b"a"), 1_000);
        let mut ours = Log::new(id);
        ours.insert(a, &Anyone, 1_000).unwrap();
        let b = theirs.append(&key, data(b"b"), 1_001);
        let c = theirs.append(&key, data(b"c"), 1_002);
        let send = theirs.since(ours.heads());
        assert_eq!(send, vec![b, c]);
        for entry in send {
            ours.insert(entry, &Anyone, 2_000).unwrap();
        }
        assert_eq!(ours.heads(), theirs.heads());
    }
}
