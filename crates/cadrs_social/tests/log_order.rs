//! The log's promises, over simulated peers: the same entries give the same order whatever
//! order they arrive in, a late branch can't reorder what is final, and every peer finds the same
//! conflicts.

use std::collections::{BTreeMap, BTreeSet};

use cadrs_social::keys::SecretKey;
use cadrs_social::log::{
    Anyone, Entry, EntryId, Inserted, Kind, Log, LogId, Outcome, Reducer, Witnesses,
};
use proptest::prelude::*;

/// Peers that each keep a copy of one log and sync with each other when told.
struct Net {
    keys: Vec<SecretKey>,
    logs: Vec<Log>,
    wall: u64,
}

impl Net {
    fn new(peers: usize) -> Self {
        let id = LogId([9; 32]);
        Self {
            keys: (0..peers)
                .map(|i| SecretKey::from_bytes(&[i as u8 + 1; 32]))
                .collect(),
            logs: (0..peers).map(|_| Log::new(id)).collect(),
            wall: 1_000_000,
        }
    }

    fn write(&mut self, peer: usize, data: &[u8]) -> Entry {
        self.wall += 7;
        self.logs[peer].append(&self.keys[peer], Kind::Data(data.to_vec()), self.wall)
    }

    /// `peer` writes at its own (possibly skewed) wall time.
    fn write_at(&mut self, peer: usize, data: &[u8], wall: u64) -> Entry {
        self.logs[peer].append(&self.keys[peer], Kind::Data(data.to_vec()), wall)
    }

    fn ack(&mut self, peer: usize) -> Entry {
        self.wall += 7;
        self.logs[peer].append(&self.keys[peer], Kind::Ack, self.wall)
    }

    /// `to` gets everything `from` has that it lacks.
    fn sync(&mut self, from: usize, to: usize) {
        let send = self.logs[from].since(self.logs[to].heads());
        for entry in send {
            self.logs[to].insert(entry, &Anyone, self.wall).unwrap();
        }
    }

    fn sync_all(&mut self) {
        for from in 0..self.logs.len() {
            for to in 0..self.logs.len() {
                if from != to {
                    self.sync(from, to);
                }
            }
        }
    }

    fn witnesses(&self, peers: &[usize]) -> Witnesses {
        peers.iter().map(|&p| (self.keys[p].public(), 1)).collect()
    }
}

fn ids(order: &[cadrs_social::log::Ordered]) -> Vec<EntryId> {
    order.iter().map(|o| o.id).collect()
}

proptest! {
    /// Peers write and sync in a random pattern; then every entry is handed to fresh peers in
    /// random orders. All end with the same heads and the same order, and that order puts every
    /// entry after its parents.
    #[test]
    fn same_entries_same_order(
        steps in prop::collection::vec((0usize..4, 0usize..4, any::<bool>()), 1..60),
        shuffles in prop::collection::vec(any::<prop::sample::Index>(), 3),
        witness_count in 0usize..4,
    ) {
        let mut net = Net::new(4);
        for (i, (peer, other, sync)) in steps.iter().enumerate() {
            if *sync && peer != other {
                net.sync(*other, *peer);
            } else {
                net.write(*peer, &[i as u8]);
            }
        }
        net.sync_all();
        let witnesses = net.witnesses(&(0..witness_count).collect::<Vec<_>>());
        let reference = net.logs[0].order(&witnesses);
        for log in &net.logs {
            prop_assert_eq!(log.heads(), net.logs[0].heads());
            prop_assert_eq!(&log.order(&witnesses), &reference);
        }

        // Every entry, delivered in a shuffled order to a fresh peer.
        let all: Vec<Entry> = net.logs[0].since(&BTreeSet::new());
        for shuffle in shuffles {
            let mut entries = all.clone();
            let mut seed = shuffle.index(usize::MAX);
            for i in (1..entries.len()).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                entries.swap(i, seed % (i + 1));
            }
            let mut fresh = Log::new(net.logs[0].id());
            for entry in entries {
                fresh.insert(entry, &Anyone, net.wall).unwrap();
            }
            prop_assert_eq!(fresh.len(), all.len());
            prop_assert_eq!(&fresh.order(&witnesses), &reference);
        }

        // Parents come first.
        let position: BTreeMap<EntryId, usize> =
            ids(&reference).into_iter().enumerate().map(|(i, id)| (id, i)).collect();
        for (id, i) in &position {
            for parent in &net.logs[0].get(id).unwrap().body.parents {
                prop_assert!(position[parent] < *i);
            }
        }
    }
}

#[test]
fn an_entry_is_final_once_most_witness_weight_builds_on_it() {
    let mut net = Net::new(4);
    let witnesses = net.witnesses(&[0, 1, 2]);
    let order = net.write(3, b"order 17");
    net.sync(3, 0);
    net.ack(0);
    net.sync_all();
    assert!(!net.logs[3].finals(&witnesses).contains(&order.id()));
    net.ack(1);
    net.sync_all();
    for log in &net.logs {
        assert!(log.finals(&witnesses).contains(&order.id()));
    }
}

/// The partition case: a member's laptop was offline with a clock running behind and wrote an
/// entry stamped earlier than one the rest of the org has since made final. When it comes back,
/// its entry lands after the final one, not before.
#[test]
fn a_late_branch_lands_after_what_is_final() {
    let mut net = Net::new(3);
    let witnesses = net.witnesses(&[0, 1]);
    let start = net.write(0, b"start");
    net.sync_all();

    // Peer 2 goes offline and writes, its clock behind everyone's.
    let late = net.write_at(2, b"late", start.body.time.millis + 1);

    // Meanwhile the others write and both witnesses build on it: final.
    let reserved = net.write(0, b"reserve the last sheet");
    net.sync(0, 1);
    net.ack(1);
    net.sync(1, 0);
    assert!(net.logs[0].finals(&witnesses).contains(&reserved.id()));
    assert!(
        late.body.time < reserved.body.time,
        "the late entry sorts earlier by key"
    );

    // Peer 2 comes back.
    net.sync_all();
    for log in &net.logs {
        let order = ids(&log.order(&witnesses));
        let at = |id: EntryId| order.iter().position(|o| *o == id).unwrap();
        assert!(at(reserved.id()) < at(late.id()));
    }
}

/// Stock: one sheet left, two members reserve it while apart. Every peer, whatever it heard
/// first, finds the same reservation conflicted.
#[test]
fn every_peer_finds_the_same_conflict() {
    #[derive(Default)]
    struct Stock {
        sheets: i64,
    }
    impl Reducer for Stock {
        fn apply(&mut self, _: EntryId, _: &Entry, data: &[u8]) -> Outcome {
            let change = data[0] as i64 - 100;
            if self.sheets + change < 0 {
                Outcome::Conflicted(format!("only {} left", self.sheets))
            } else {
                self.sheets += change;
                Outcome::Applied
            }
        }
    }

    let mut net = Net::new(3);
    net.write(0, &[101]); // +1 sheet
    net.sync_all();
    let a = net.write(1, &[99]); // -1
    let b = net.write(2, &[99]); // -1, while apart
    // They hear of each other in different orders.
    net.sync(1, 0);
    net.sync(2, 0);
    net.sync(2, 1);
    net.sync(1, 2);
    let witnesses = Witnesses::new();
    let mut verdicts = Vec::new();
    for log in &net.logs {
        let mut stock = Stock::default();
        let outcomes = log.replay(&witnesses, &mut stock);
        assert_eq!(stock.sheets, 0);
        let conflicted: Vec<EntryId> = outcomes
            .iter()
            .filter(|(_, o)| matches!(o, Outcome::Conflicted(_)))
            .map(|(o, _)| o.id)
            .collect();
        assert_eq!(conflicted.len(), 1);
        verdicts.push(conflicted[0]);
    }
    assert!(verdicts.iter().all(|v| *v == verdicts[0]));
    assert!([a.id(), b.id()].contains(&verdicts[0]));
}

#[test]
fn a_waiting_entry_is_added_when_its_parents_arrive_from_another_peer() {
    let mut net = Net::new(3);
    net.write(0, b"a");
    net.sync(0, 1);
    let b = net.write(1, b"b");
    // Peer 2 hears of b before a.
    assert!(matches!(
        net.logs[2].insert(b, &Anyone, net.wall),
        Ok(Inserted::Waiting(_))
    ));
    net.sync(0, 2);
    assert_eq!(net.logs[2].heads(), net.logs[1].heads());
}
