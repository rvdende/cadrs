//! Reputation: how much a node has done for others, from the receipts it holds, and the witness
//! weight that earns it in an organization.
//!
//! Only co-signed receipts count, and only work for someone else: a node can't sign receipts
//! with itself. Two nodes run by one person *can* sign each other endless receipts, so what one
//! counterparty vouches for counts less and less (a logarithm per counterparty): a node trusted
//! by many is worth more than one a single friend says carried a petabyte.
//!
//! Within an organization the weight still starts from the member's role (the org decides who
//! may witness at all); reputation multiplies it, up to a cap, so a long-serving shop box counts
//! more than a laptop that joined yesterday, but no node can outweigh the org's owners alone.

use std::collections::BTreeMap;

use crate::keys::PublicKey;
use crate::receipts::{Ledger, Receipt};

/// Byte-seconds that make one unit of contribution: a megabyte kept for an hour.
const UNIT: u128 = 1_000_000 * 3_600;
/// The most reputation multiplies a role's weight by.
pub const MAX_MULTIPLIER: u64 = 4;

/// What a node has done for others.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reputation {
    /// Byte-seconds kept for others, in total.
    pub byte_seconds: u128,
    /// Orders completed, as a shop or a client.
    pub orders: u32,
    /// How many different others co-signed its receipts.
    pub counterparties: usize,
    /// The score: the sum over counterparties of log2(1 + units with them), plus completed
    /// orders.
    pub score: f64,
}

impl Reputation {
    /// `node`'s reputation from the receipts in `ledger`.
    pub fn of(ledger: &Ledger, node: &PublicKey) -> Self {
        let mut by_party: BTreeMap<PublicKey, u128> = BTreeMap::new();
        let mut reputation = Reputation::default();
        for receipt in ledger.receipts() {
            let receipt = &receipt.receipt;
            let [first, second] = receipt.parties();
            if first == second {
                continue;
            }
            let other = if &first == node {
                second
            } else if &second == node {
                first
            } else {
                continue;
            };
            match receipt {
                Receipt::Storage { holder, .. } if holder == node => {
                    reputation.byte_seconds += receipt.byte_seconds();
                    *by_party.entry(other).or_default() += receipt.byte_seconds();
                }
                Receipt::OrderCompleted { .. } => {
                    reputation.orders += 1;
                    by_party.entry(other).or_default();
                }
                _ => {}
            }
        }
        reputation.counterparties = by_party.len();
        reputation.score = by_party
            .values()
            .map(|&bs| (1.0 + (bs / UNIT) as f64).log2())
            .sum::<f64>()
            + reputation.orders as f64;
        reputation
    }

    /// How much this multiplies a role's witness weight: 1 for nobody, up to [`MAX_MULTIPLIER`].
    pub fn multiplier(&self) -> u64 {
        1 + ((self.score / 8.0).floor() as u64).min(MAX_MULTIPLIER - 1)
    }
}

/// A witness's weight: its role's weight times its reputation's multiplier.
pub fn witness_weight(role_weight: u64, reputation: &Reputation) -> u64 {
    role_weight * reputation.multiplier()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::SecretKey;

    fn stored(ledger: &mut Ledger, holder: &SecretKey, owner: &SecretKey, megabyte_hours: u64) {
        let receipt = Receipt::Storage {
            holder: holder.public(),
            owner: owner.public(),
            blob: crate::random32(),
            bytes: 1_000_000 * megabyte_hours,
            from: 0,
            until: 3_600_000,
        }
        .sign_first(holder)
        .unwrap()
        .countersign(owner)
        .unwrap();
        ledger.add(receipt).unwrap();
    }

    #[test]
    fn many_counterparties_count_more_than_one_friend() {
        let honest = SecretKey::generate();
        let sybil = SecretKey::generate();
        let friend = SecretKey::generate();
        let mut ledger = Ledger::default();
        // The sybil's friend vouches for a million megabyte-hours.
        stored(&mut ledger, &sybil, &friend, 1_000_000);
        // The honest node kept 100 MB-hours each for 20 different people.
        for _ in 0..20 {
            stored(&mut ledger, &honest, &SecretKey::generate(), 100);
        }
        let honest = Reputation::of(&ledger, &honest.public());
        let sybil = Reputation::of(&ledger, &sybil.public());
        assert!(sybil.byte_seconds > honest.byte_seconds);
        assert!(honest.score > sybil.score * 5.0, "{honest:?} {sybil:?}");
        assert_eq!(honest.counterparties, 20);
    }

    #[test]
    fn keeping_data_is_what_counts_not_having_it_kept() {
        let holder = SecretKey::generate();
        let owner = SecretKey::generate();
        let mut ledger = Ledger::default();
        stored(&mut ledger, &holder, &owner, 1_000);
        assert!(Reputation::of(&ledger, &holder.public()).score > 0.0);
        assert_eq!(Reputation::of(&ledger, &owner.public()).byte_seconds, 0);
    }

    #[test]
    fn weights_grow_with_reputation_up_to_a_cap() {
        let nobody = Reputation::default();
        assert_eq!(witness_weight(3, &nobody), 3);
        let veteran = Reputation {
            score: 1_000.0,
            ..Default::default()
        };
        assert_eq!(witness_weight(3, &veteran), 3 * MAX_MULTIPLIER);
        assert_eq!(witness_weight(0, &veteran), 0);
    }
}
