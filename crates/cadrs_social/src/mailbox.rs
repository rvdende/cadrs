//! Mailboxes: nodes that hold encrypted messages for people who are offline.
//!
//! A sender seals a message for an account and signs an [`Envelope`] around it; any node that
//! keeps a [`Mailbox`] (the seed peer, a shop's always-on box, anyone who opts in) takes it for an
//! agreed time, within its quota. When one of the account's devices comes online it proves who it
//! is with a signed [`Collect`] plus its device certificate, takes its envelopes, and signs a
//! storage receipt for each: the node's proof that it kept someone else's data, in byte-seconds,
//! confirmed by a third party (the recipient, not the node itself).
//!
//! The node can't read what it holds; the sealing is the caller's. A sender can also check the node
//! still holds what it took, without keeping the message, with [`Challenges`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::keys::{self, AccountId, DeviceCert, Devices, PublicKey, SecretKey, Signed, Statement};
use crate::receipts::{HalfSigned, Receipt};

/// The ALPN a mailbox answers on.
pub const ALPN: &[u8] = b"cadrs/mailbox/1";

/// The signed outside of a sealed message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub to: AccountId,
    /// The sealed message: only `to`'s devices can open it.
    #[serde(with = "serde_bytes")]
    pub sealed: Vec<u8>,
    /// Milliseconds since the Unix epoch.
    pub sent: u64,
    /// How long it may be kept, in milliseconds.
    pub keep_for: u64,
}

impl Statement for Envelope {
    const CONTEXT: &'static str = "cadrs envelope 1";
}

/// An envelope's id: the hash of the signed envelope.
pub fn envelope_id(envelope: &Signed<Envelope>) -> [u8; 32] {
    let bytes = postcard::to_allocvec(envelope).expect("envelopes encode");
    *blake3::hash(&bytes).as_bytes()
}

/// A device asks a mailbox for its account's envelopes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collect {
    /// The mailbox asked, so the request can't be replayed at another.
    pub mailbox: PublicKey,
    /// Milliseconds since the Unix epoch: requests older than a minute are refused.
    pub time: u64,
}

impl Statement for Collect {
    const CONTEXT: &'static str = "cadrs mailbox collect 1";
}

/// Why a mailbox refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    Signature(keys::Error),
    /// Longer than this mailbox keeps anything.
    TooLong,
    /// Over the quota, in total or for one account.
    Full,
    Expired,
    /// The collecting device is not certified by the account, or the request is stale or for
    /// another mailbox.
    NotYours,
}

/// What a mailbox node agrees to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terms {
    /// All envelopes together, in bytes.
    pub quota: u64,
    /// For one account, in bytes.
    pub per_account: u64,
    /// The longest it keeps an envelope, in milliseconds.
    pub max_keep: u64,
}

impl Default for Terms {
    fn default() -> Self {
        Self {
            quota: 256 << 20,
            per_account: 16 << 20,
            max_keep: 30 * 24 * 60 * 60 * 1000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Held {
    envelope: Signed<Envelope>,
    /// When it arrived, milliseconds since the Unix epoch.
    since: u64,
}

impl Held {
    fn size(&self) -> u64 {
        self.envelope.value.sealed.len() as u64
    }
}

/// A node's mailbox.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mailbox {
    pub terms: Terms,
    held: BTreeMap<[u8; 32], Held>,
}

impl Mailbox {
    pub fn new(terms: Terms) -> Self {
        Self {
            terms,
            held: BTreeMap::new(),
        }
    }

    /// How many envelopes it holds.
    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    pub fn bytes(&self) -> u64 {
        self.held.values().map(Held::size).sum()
    }

    fn bytes_for(&self, account: &AccountId) -> u64 {
        self.held
            .values()
            .filter(|held| &held.envelope.value.to == account)
            .map(Held::size)
            .sum()
    }

    /// Takes an envelope in; its id.
    pub fn deposit(&mut self, envelope: Signed<Envelope>, now: u64) -> Result<[u8; 32], Refused> {
        let value = envelope.verify().map_err(Refused::Signature)?;
        if value.keep_for > self.terms.max_keep {
            return Err(Refused::TooLong);
        }
        if value.sent + value.keep_for <= now {
            return Err(Refused::Expired);
        }
        let size = value.sealed.len() as u64;
        if self.bytes() + size > self.terms.quota
            || self.bytes_for(&value.to) + size > self.terms.per_account
        {
            return Err(Refused::Full);
        }
        let id = envelope_id(&envelope);
        self.held.entry(id).or_insert(Held {
            envelope,
            since: now,
        });
        Ok(id)
    }

    /// Drops what has been kept as long as its sender asked.
    pub fn expire(&mut self, now: u64) {
        self.held
            .retain(|_, held| held.envelope.value.sent + held.envelope.value.keep_for > now);
    }

    /// A device collects its account's envelopes. They leave the mailbox, each with a storage
    /// receipt this node has signed for the device to countersign.
    pub fn collect(
        &mut self,
        node: &SecretKey,
        request: &Signed<Collect>,
        cert: &Signed<DeviceCert>,
        now: u64,
    ) -> Result<Vec<(Signed<Envelope>, HalfSigned)>, Refused> {
        let asked = request.verify().map_err(Refused::Signature)?;
        if asked.mailbox != node.public() || asked.time + 60_000 < now || asked.time > now + 60_000
        {
            return Err(Refused::NotYours);
        }
        let mut devices = Devices::default();
        devices.add_cert(cert).map_err(Refused::Signature)?;
        let account = devices
            .account_of(&request.signer, now)
            .ok_or(Refused::NotYours)?;
        let ids: Vec<[u8; 32]> = self
            .held
            .iter()
            .filter(|(_, held)| held.envelope.value.to == account)
            .map(|(id, _)| *id)
            .collect();
        Ok(ids
            .into_iter()
            .map(|id| {
                let held = self.held.remove(&id).expect("held");
                let receipt = Receipt::Storage {
                    holder: node.public(),
                    owner: request.signer,
                    blob: id,
                    bytes: held.size(),
                    from: held.since,
                    until: now,
                }
                .sign_first(node)
                .expect("the node is the holder");
                (held.envelope, receipt)
            })
            .collect())
    }
}

/// A device's request to collect from the mailbox `mailbox`.
pub fn collect_request(device: &SecretKey, mailbox: PublicKey, now: u64) -> Signed<Collect> {
    Signed::new(device, Collect { mailbox, time: now })
}

/// Seals nothing: signs an envelope around bytes the caller already sealed.
pub fn envelope(
    sender: &SecretKey,
    to: AccountId,
    sealed: Vec<u8>,
    now: u64,
    keep_for: u64,
) -> Signed<Envelope> {
    Signed::new(
        sender,
        Envelope {
            to,
            sealed,
            sent: now,
            keep_for,
        },
    )
}

/// Questions a sender prepares before handing data to a node, to ask later whether the node still
/// has it. Each needs the whole data to answer and is asked once.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Challenges {
    pending: Vec<([u8; 32], [u8; 32])>,
}

impl Challenges {
    pub fn prepare(data: &[u8], count: usize) -> Self {
        Self {
            pending: (0..count)
                .map(|_| {
                    let nonce = crate::random32();
                    (nonce, answer(&nonce, data))
                })
                .collect(),
        }
    }

    pub fn left(&self) -> usize {
        self.pending.len()
    }

    /// The next question, if any are left.
    pub fn ask(&mut self) -> Option<Challenge> {
        let (nonce, expected) = self.pending.pop()?;
        Some(Challenge { nonce, expected })
    }
}

/// One question; send `nonce`, keep the rest.
#[derive(Clone, Copy, Debug)]
pub struct Challenge {
    pub nonce: [u8; 32],
    expected: [u8; 32],
}

impl Challenge {
    pub fn check(&self, answer: &[u8; 32]) -> bool {
        &self.expected == answer
    }
}

/// The node's answer to `nonce` for data it holds.
pub fn answer(nonce: &[u8; 32], data: &[u8]) -> [u8; 32] {
    *blake3::keyed_hash(nonce, data).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Account;

    const DAY: u64 = 24 * 60 * 60 * 1000;

    struct World {
        node: SecretKey,
        mailbox: Mailbox,
        designer: SecretKey,
        shop: Account,
        shop_box: SecretKey,
        shop_cert: Signed<DeviceCert>,
    }

    fn world() -> World {
        let shop = Account::generate();
        let shop_box = SecretKey::generate();
        let shop_cert = shop.certify(shop_box.public(), "shop box", 0);
        World {
            node: SecretKey::generate(),
            mailbox: Mailbox::new(Terms::default()),
            designer: SecretKey::generate(),
            shop,
            shop_box,
            shop_cert,
        }
    }

    #[test]
    fn an_offline_shop_collects_its_request_later() {
        let mut w = world();
        let rfq = envelope(
            &w.designer,
            w.shop.id(),
            b"sealed rfq".to_vec(),
            1_000,
            7 * DAY,
        );
        w.mailbox.deposit(rfq.clone(), 1_000).unwrap();
        // Someone else's envelope stays.
        let other = envelope(&w.designer, Account::generate().id(), vec![1], 1_000, DAY);
        w.mailbox.deposit(other, 1_000).unwrap();

        // The shop's box comes online an hour later.
        let now = 1_000 + 3_600_000;
        let request = collect_request(&w.shop_box, w.node.public(), now);
        let got = w
            .mailbox
            .collect(&w.node, &request, &w.shop_cert, now)
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, rfq);
        assert_eq!(w.mailbox.held.len(), 1);

        // The box countersigns the node's storage receipt: an hour of 10 bytes.
        let receipt = got[0].1.clone().countersign(&w.shop_box).unwrap();
        assert_eq!(receipt.verify().unwrap().byte_seconds(), 10 * 3_600);

        // Nothing left for it.
        let again = collect_request(&w.shop_box, w.node.public(), now);
        assert!(
            w.mailbox
                .collect(&w.node, &again, &w.shop_cert, now)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn only_the_accounts_devices_collect() {
        let mut w = world();
        w.mailbox
            .deposit(envelope(&w.designer, w.shop.id(), vec![1], 0, DAY), 0)
            .unwrap();
        // A stranger with a certificate from its own account.
        let stranger = Account::generate();
        let device = SecretKey::generate();
        let cert = stranger.certify(device.public(), "x", 0);
        let request = collect_request(&device, w.node.public(), 10);
        assert!(
            w.mailbox
                .collect(&w.node, &request, &cert, 10)
                .unwrap()
                .is_empty()
        );
        // A device presenting the shop's certificate it doesn't hold the key of.
        assert_eq!(
            w.mailbox.collect(&w.node, &request, &w.shop_cert, 10),
            Err(Refused::NotYours)
        );
        // A request made for another mailbox, or a stale one.
        let elsewhere = collect_request(&w.shop_box, SecretKey::generate().public(), 10);
        assert_eq!(
            w.mailbox.collect(&w.node, &elsewhere, &w.shop_cert, 10),
            Err(Refused::NotYours)
        );
        let stale = collect_request(&w.shop_box, w.node.public(), 10);
        assert_eq!(
            w.mailbox
                .collect(&w.node, &stale, &w.shop_cert, 10 + 120_000),
            Err(Refused::NotYours)
        );
    }

    #[test]
    fn terms_are_kept() {
        let mut w = world();
        w.mailbox.terms = Terms {
            quota: 10,
            per_account: 6,
            max_keep: DAY,
        };
        let to = w.shop.id();
        assert_eq!(
            w.mailbox
                .deposit(envelope(&w.designer, to, vec![0; 4], 0, 2 * DAY), 0),
            Err(Refused::TooLong)
        );
        assert_eq!(
            w.mailbox
                .deposit(envelope(&w.designer, to, vec![0; 4], 0, 10), 20),
            Err(Refused::Expired)
        );
        w.mailbox
            .deposit(envelope(&w.designer, to, vec![0; 4], 0, DAY), 0)
            .unwrap();
        assert_eq!(
            w.mailbox
                .deposit(envelope(&w.designer, to, vec![1; 4], 0, DAY), 0),
            Err(Refused::Full)
        );
        let other = Account::generate().id();
        w.mailbox
            .deposit(envelope(&w.designer, other, vec![0; 6], 0, DAY), 0)
            .unwrap();
        assert_eq!(
            w.mailbox
                .deposit(envelope(&w.designer, other, vec![1; 1], 0, DAY), 0),
            Err(Refused::Full)
        );
        w.mailbox.expire(DAY);
        assert_eq!(w.mailbox.bytes(), 0);
    }

    #[test]
    fn a_tampered_envelope_is_refused() {
        let mut w = world();
        let mut rfq = envelope(&w.designer, w.shop.id(), vec![1, 2, 3], 0, DAY);
        rfq.value.to = Account::generate().id();
        assert!(matches!(
            w.mailbox.deposit(rfq, 0),
            Err(Refused::Signature(_))
        ));
    }

    #[test]
    fn challenges_need_the_data() {
        let data = b"a sealed request for a quote".to_vec();
        let mut challenges = Challenges::prepare(&data, 3);
        let challenge = challenges.ask().unwrap();
        assert!(challenge.check(&answer(&challenge.nonce, &data)));
        let challenge = challenges.ask().unwrap();
        assert!(!challenge.check(&answer(&challenge.nonce, b"something else")));
        assert_eq!(challenges.left(), 1);
    }
}
