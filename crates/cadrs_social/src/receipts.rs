//! Receipts: statements two parties sign, so either can later prove what happened.
//!
//! - [`Receipt::Storage`]: a node kept someone's encrypted data for a while. Its size times how
//!   long is the node's contribution, in byte-seconds.
//! - [`Receipt::Relay`]: a node passed data on for someone.
//! - [`Receipt::OrderCompleted`]: a shop and its client agree an order was delivered. Only a
//!   client holding one may review the shop.
//!
//! A receipt counts once both have signed it ([`CoSigned`]). They go into a local [`Ledger`];
//! settling them on a chain is for later.

use serde::{Deserialize, Serialize};

use crate::keys::{self, AccountId, PublicKey, SecretKey, Signature};

const CONTEXT: &str = "cadrs receipt 1";

/// Something two parties agree happened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Receipt {
    Storage {
        /// Who kept the data.
        holder: PublicKey,
        /// Whose data it was.
        owner: PublicKey,
        /// The BLAKE3 hash of what was kept.
        blob: [u8; 32],
        bytes: u64,
        /// Milliseconds since the Unix epoch.
        from: u64,
        until: u64,
    },
    Relay {
        relay: PublicKey,
        sender: PublicKey,
        bytes: u64,
        /// Milliseconds since the Unix epoch.
        at: u64,
    },
    OrderCompleted {
        shop: AccountId,
        client: AccountId,
        /// The order's id in the shop's organization document.
        order: [u8; 32],
        /// Milliseconds since the Unix epoch.
        at: u64,
    },
}

impl Receipt {
    /// The two parties, in the order they sign.
    pub fn parties(&self) -> [PublicKey; 2] {
        match self {
            Receipt::Storage { holder, owner, .. } => [*holder, *owner],
            Receipt::Relay { relay, sender, .. } => [*relay, *sender],
            Receipt::OrderCompleted { shop, client, .. } => [*shop, *client],
        }
    }

    /// The contribution in byte-seconds: stored bytes times seconds, or relayed bytes.
    pub fn byte_seconds(&self) -> u128 {
        match self {
            Receipt::Storage {
                bytes, from, until, ..
            } => *bytes as u128 * (until.saturating_sub(*from) / 1000) as u128,
            Receipt::Relay { .. } | Receipt::OrderCompleted { .. } => 0,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("receipts encode")
    }
}

/// A receipt one party has signed, waiting for the other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HalfSigned {
    pub receipt: Receipt,
    pub first: Signature,
}

/// A receipt both parties signed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoSigned {
    pub receipt: Receipt,
    pub signatures: [Signature; 2],
}

/// Why a receipt was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The key signing is not the party whose turn it is.
    NotTheParty,
    Signature(keys::Error),
}

impl Receipt {
    /// The first party signs.
    pub fn sign_first(self, key: &SecretKey) -> Result<HalfSigned, Error> {
        if key.public() != self.parties()[0] {
            return Err(Error::NotTheParty);
        }
        let first = key.sign(CONTEXT, &self.bytes());
        Ok(HalfSigned {
            receipt: self,
            first,
        })
    }
}

impl HalfSigned {
    /// The second party checks the first's signature and signs too.
    pub fn countersign(self, key: &SecretKey) -> Result<CoSigned, Error> {
        let [first, second] = self.receipt.parties();
        if key.public() != second {
            return Err(Error::NotTheParty);
        }
        let bytes = self.receipt.bytes();
        keys::verify(&first, CONTEXT, &bytes, &self.first).map_err(Error::Signature)?;
        let signature = key.sign(CONTEXT, &bytes);
        Ok(CoSigned {
            receipt: self.receipt,
            signatures: [self.first, signature],
        })
    }
}

impl CoSigned {
    pub fn verify(&self) -> Result<&Receipt, Error> {
        let bytes = self.receipt.bytes();
        for (party, signature) in self.receipt.parties().iter().zip(&self.signatures) {
            keys::verify(party, CONTEXT, &bytes, signature).map_err(Error::Signature)?;
        }
        Ok(&self.receipt)
    }
}

/// The receipts a peer holds.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    receipts: Vec<CoSigned>,
}

impl Ledger {
    /// Keeps a receipt if both signatures hold.
    pub fn add(&mut self, receipt: CoSigned) -> Result<(), Error> {
        receipt.verify()?;
        if !self.receipts.contains(&receipt) {
            self.receipts.push(receipt);
        }
        Ok(())
    }

    pub fn receipts(&self) -> &[CoSigned] {
        &self.receipts
    }

    /// What `node` contributed by storing others' data, in byte-seconds.
    pub fn stored_by(&self, node: &PublicKey) -> u128 {
        self.receipts
            .iter()
            .filter(|r| matches!(&r.receipt, Receipt::Storage { holder, .. } if holder == node))
            .map(|r| r.receipt.byte_seconds())
            .sum()
    }

    /// Whether `client` completed an order with `shop`, so may review it.
    pub fn did_business(&self, client: &AccountId, shop: &AccountId) -> bool {
        self.receipts.iter().any(|r| {
            matches!(&r.receipt, Receipt::OrderCompleted { shop: s, client: c, .. }
                if s == shop && c == client)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_parties_sign() {
        let shop = SecretKey::generate();
        let client = SecretKey::generate();
        let receipt = Receipt::OrderCompleted {
            shop: shop.public(),
            client: client.public(),
            order: [7; 32],
            at: 1_000,
        };
        assert_eq!(receipt.clone().sign_first(&client), Err(Error::NotTheParty));
        let half = receipt.sign_first(&shop).unwrap();
        assert_eq!(half.clone().countersign(&shop), Err(Error::NotTheParty));
        let both = half.countersign(&client).unwrap();
        let mut ledger = Ledger::default();
        ledger.add(both.clone()).unwrap();
        assert!(ledger.did_business(&client.public(), &shop.public()));
        assert!(!ledger.did_business(&shop.public(), &client.public()));

        let mut forged = both;
        forged.receipt = Receipt::OrderCompleted {
            shop: shop.public(),
            client: client.public(),
            order: [8; 32],
            at: 1_000,
        };
        assert!(ledger.add(forged).is_err());
    }

    #[test]
    fn storage_counts_in_byte_seconds() {
        let holder = SecretKey::generate();
        let owner = SecretKey::generate();
        let receipt = Receipt::Storage {
            holder: holder.public(),
            owner: owner.public(),
            blob: [1; 32],
            bytes: 1_000_000,
            from: 0,
            until: 3_600_000,
        };
        assert_eq!(receipt.byte_seconds(), 3_600_000_000);
        let both = receipt
            .sign_first(&holder)
            .unwrap()
            .countersign(&owner)
            .unwrap();
        let mut ledger = Ledger::default();
        ledger.add(both).unwrap();
        assert_eq!(ledger.stored_by(&holder.public()), 3_600_000_000);
        assert_eq!(ledger.stored_by(&owner.public()), 0);
    }
}
