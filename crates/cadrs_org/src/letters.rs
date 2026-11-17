//! What clients and shops send each other: requests for quotes, quotes, declines,
//! acceptances, messages and order news.
//!
//! A [`Letter`] is signed by the sending device and carries the device's certificate, so the
//! receiver knows which account it is from. It travels straight to one of the recipient's devices
//! (an iroh connection, encrypted to that device's key) or, later, sealed through a mailbox.
//!
//! The shop's side turns letters into operations on its organization document
//! ([`shop_op`]); the client's side keeps a [`ClientBook`] of the requests it sent.

use std::collections::BTreeMap;

use cadrs_social::contact::Card;
use cadrs_social::keys::{AccountId, DeviceCert, SecretKey, Signed, Statement};
use cadrs_social::receipts::{CoSigned, HalfSigned};
use serde::{Deserialize, Serialize};

use crate::capability::PartSummary;
use crate::{Id, Op, Org, Quote, Stage};

/// One message between a client and a shop. (A request is much bigger than the rest; messages
/// are made and sent one at a time, never kept in bulk, so it isn't boxed.)
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Message {
    /// Client → shop.
    Request {
        id: Id,
        parts: Vec<(PartSummary, u32)>,
        notes: String,
        /// Milliseconds since the Unix epoch.
        deadline: Option<u64>,
        /// The frozen parts: a packed document, which the shop can open.
        #[serde(with = "serde_bytes")]
        snapshot: Vec<u8>,
        /// How to reach the client back.
        client: Signed<Card>,
    },
    /// Shop → client.
    Quote { request: Id, quote: Quote },
    /// Shop → client.
    Decline { request: Id, reason: String },
    /// Client → shop: go ahead.
    Accept { request: Id },
    /// Shop → client: the order moved on.
    Stage { request: Id, stage: Stage },
    /// Either way: a message about a request.
    Chat { request: Id, text: String },
    /// Shop → client: delivered; the receipt the shop signed, for the client to countersign.
    Delivered { request: Id, receipt: HalfSigned },
    /// Client → shop: the receipt both signed.
    Received { request: Id, receipt: CoSigned },
    /// Client → shop: a review of a completed order.
    Review {
        request: Id,
        stars: u8,
        text: String,
    },
}

impl Message {
    pub fn request(&self) -> Id {
        match self {
            Message::Request { id, .. } => *id,
            Message::Quote { request, .. }
            | Message::Decline { request, .. }
            | Message::Accept { request }
            | Message::Stage { request, .. }
            | Message::Chat { request, .. }
            | Message::Delivered { request, .. }
            | Message::Received { request, .. }
            | Message::Review { request, .. } => *request,
        }
    }
}

/// A message, who it is for, and the sending device's certificate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Letter {
    pub to: AccountId,
    pub from: Signed<DeviceCert>,
    /// Milliseconds since the Unix epoch.
    pub sent: u64,
    pub message: Message,
}

impl Statement for Letter {
    const CONTEXT: &'static str = "cadrs letter 1";
}

/// Why a letter was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not a letter, or its signatures don't hold.
    Forged,
    /// For another account.
    NotForUs,
    /// It doesn't fit what we know (a quote for a request we never sent, an acceptance from
    /// someone other than the client).
    Unexpected(String),
}

/// Signs a letter from the device with `key` and certificate `cert`.
pub fn write(
    key: &SecretKey,
    cert: &Signed<DeviceCert>,
    to: AccountId,
    message: Message,
    now: u64,
) -> Vec<u8> {
    let letter = Signed::new(
        key,
        Letter {
            to,
            from: cert.clone(),
            sent: now,
            message,
        },
    );
    postcard::to_allocvec(&letter).expect("letters encode")
}

/// A letter ready to go: the post for the recipient's devices, and the same post sealed for
/// each of them, in envelopes for their mailboxes in case none is online.
pub struct Composed {
    pub post: Vec<u8>,
    pub envelopes: Vec<Signed<cadrs_social::mailbox::Envelope>>,
}

/// How long a mailbox keeps a letter for someone offline.
pub const KEEP_FOR: u64 = 14 * 24 * 60 * 60 * 1000;

/// Writes `message` to the account on card `to`, from the device with `key` and `cert`.
pub fn compose(
    key: &SecretKey,
    cert: &Signed<DeviceCert>,
    to: &Card,
    message: Message,
    now: u64,
) -> Composed {
    let letter = write(key, cert, to.account, message, now);
    let post = cadrs_social::post::Post::Letter(letter).encode();
    let envelopes = to
        .devices
        .iter()
        .filter_map(|d| cadrs_social::seal::seal(d, &post).ok())
        .map(|sealed| cadrs_social::mailbox::envelope(key, to.account, sealed, now, KEEP_FOR))
        .collect();
    Composed { post, envelopes }
}

/// Reads a letter: checks both signatures, and that it is for one of `ours`. Returns the
/// letter and the account it is from.
pub fn read(bytes: &[u8], ours: &[AccountId]) -> Result<(Letter, AccountId), Refused> {
    let letter: Signed<Letter> = postcard::from_bytes(bytes).map_err(|_| Refused::Forged)?;
    let value = letter.verify().map_err(|_| Refused::Forged)?;
    let cert = value.from.verify().map_err(|_| Refused::Forged)?;
    if value.from.signer != cert.account || cert.device != letter.signer {
        return Err(Refused::Forged);
    }
    if !ours.contains(&value.to) {
        return Err(Refused::NotForUs);
    }
    Ok((value.clone(), cert.account))
}

/// The operation a letter to a shop makes on its document, given the document as it is.
pub fn shop_op(org: &Org, from: AccountId, message: &Message) -> Result<Op, Refused> {
    let unexpected = |why: &str| Err(Refused::Unexpected(why.into()));
    match message {
        Message::Request {
            id,
            parts,
            notes,
            deadline,
            snapshot,
            client,
        } => {
            if client.value.account != from {
                return unexpected("the card is not the sender's");
            }
            if org.requests.contains_key(id) {
                return unexpected("that request arrived already");
            }
            Ok(Op::Request {
                id: *id,
                client: from,
                parts: parts.clone(),
                notes: notes.clone(),
                snapshot: *blake3::hash(snapshot).as_bytes(),
                deadline: *deadline,
                card: Some(client.clone()),
            })
        }
        Message::Accept { request } => match org.requests.get(request) {
            Some(r) if r.client == from => Ok(Op::Accept { request: *request }),
            Some(_) => unexpected("only the client accepts a quote"),
            None => unexpected("no such request"),
        },
        Message::Chat { request, text } => match org.requests.get(request) {
            Some(r) if r.client == from => Ok(Op::Note {
                request: *request,
                from_client: true,
                text: text.clone(),
            }),
            Some(_) => unexpected("only the client writes about a request"),
            None => unexpected("no such request"),
        },
        Message::Received { request, receipt } => match org.requests.get(request) {
            Some(r) if r.client == from => Ok(Op::Complete {
                request: *request,
                receipt: receipt.clone(),
            }),
            Some(_) => unexpected("only the client signs for delivery"),
            None => unexpected("no such request"),
        },
        Message::Review {
            request,
            stars,
            text,
        } => match org.requests.get(request) {
            Some(r) if r.client == from => match &r.receipt {
                Some(receipt) => Ok(Op::Review {
                    id: *request,
                    review: crate::Review {
                        by: from,
                        stars: *stars,
                        text: text.clone(),
                        request: *request,
                    },
                    receipt: receipt.clone(),
                }),
                None => unexpected("only a completed order is reviewed"),
            },
            Some(_) => unexpected("only the client reviews an order"),
            None => unexpected("no such request"),
        },
        Message::Quote { .. }
        | Message::Decline { .. }
        | Message::Stage { .. }
        | Message::Delivered { .. } => unexpected("a shop's message, sent to a shop"),
    }
}

/// A request a client sent, as the client sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sent {
    pub shop: AccountId,
    pub shop_name: String,
    pub parts: Vec<(PartSummary, u32)>,
    pub notes: String,
    pub stage: Stage,
    pub quote: Option<Quote>,
    pub decline_reason: Option<String>,
    /// The conversation, oldest first: (from the shop, text, when).
    pub chat: Vec<(bool, String, u64)>,
    /// Milliseconds since the Unix epoch.
    pub sent: u64,
    /// The delivery receipt: the shop's half, until the client countersigns it.
    #[serde(default)]
    pub delivery: Option<Delivery>,
    /// The client's review, once given.
    #[serde(default)]
    pub review: Option<(u8, String)>,
}

/// A delivery's receipt, as the client holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Delivery {
    /// Signed by the shop, waiting for the client.
    ToSign(HalfSigned),
    /// Signed by both.
    Signed(CoSigned),
}

/// The requests a client has sent, and what became of them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientBook {
    pub sent: BTreeMap<Id, Sent>,
}

/// What a letter from a shop changed, for a notification.
#[derive(Clone, Debug, PartialEq)]
pub enum News {
    Quoted { shop: String, total: String },
    Declined { shop: String },
    Stage { shop: String, stage: Stage },
    Chat { shop: String, text: String },
    Delivered { shop: String },
}

impl ClientBook {
    /// Records a request just sent.
    pub fn record(&mut self, id: Id, sent: Sent) {
        self.sent.insert(id, sent);
    }

    /// Merges another device's copy of the same account's book: requests it alone knows are
    /// added; for the others the furthest stage wins, the conversations combine (in time order),
    /// and a quote, receipt or review either has is kept. Returns whether anything changed.
    pub fn merge(&mut self, other: &ClientBook) -> bool {
        let mut changed = false;
        for (id, theirs) in &other.sent {
            let Some(mine) = self.sent.get_mut(id) else {
                self.sent.insert(*id, theirs.clone());
                changed = true;
                continue;
            };
            let before = mine.clone();
            if theirs.stage > mine.stage {
                mine.stage = theirs.stage;
            }
            if mine.quote.is_none() {
                mine.quote.clone_from(&theirs.quote);
            }
            if mine.decline_reason.is_none() {
                mine.decline_reason.clone_from(&theirs.decline_reason);
            }
            if mine.parts.is_empty() {
                mine.parts.clone_from(&theirs.parts);
            }
            if mine.notes.is_empty() {
                mine.notes.clone_from(&theirs.notes);
            }
            match (&mine.delivery, &theirs.delivery) {
                (_, Some(Delivery::Signed(r))) => mine.delivery = Some(Delivery::Signed(r.clone())),
                (None, Some(d)) => mine.delivery = Some(d.clone()),
                _ => {}
            }
            if mine.review.is_none() {
                mine.review.clone_from(&theirs.review);
            }
            for line in &theirs.chat {
                if !mine.chat.contains(line) {
                    mine.chat.push(line.clone());
                }
            }
            mine.chat.sort_by_key(|(_, _, when)| *when);
            changed |= *mine != before;
        }
        changed
    }

    /// Takes in a letter from a shop. A letter about a request another of the account's
    /// devices sent (this device didn't) teaches this one the request, so the user sees the
    /// shop's answer here too; `shop_name` names it.
    pub fn receive_learning(
        &mut self,
        from: AccountId,
        shop_name: &str,
        message: &Message,
        now: u64,
    ) -> Result<News, Refused> {
        let id = message.request();
        let learns = matches!(
            message,
            Message::Quote { .. } | Message::Decline { .. } | Message::Stage { .. }
        );
        if learns && !self.sent.contains_key(&id) {
            let parts = match message {
                Message::Quote { quote, .. } => quote
                    .lines
                    .iter()
                    .map(|l| {
                        let part = PartSummary {
                            name: l.part.clone(),
                            ..Default::default()
                        };
                        (part, l.quantity)
                    })
                    .collect(),
                _ => Vec::new(),
            };
            self.sent.insert(
                id,
                Sent {
                    shop: from,
                    shop_name: shop_name.to_owned(),
                    parts,
                    notes: String::new(),
                    stage: Stage::Requested,
                    quote: None,
                    decline_reason: None,
                    chat: Vec::new(),
                    sent: now,
                    delivery: None,
                    review: None,
                },
            );
        }
        self.receive(from, message, now)
    }

    /// Takes in a letter from a shop.
    pub fn receive(
        &mut self,
        from: AccountId,
        message: &Message,
        now: u64,
    ) -> Result<News, Refused> {
        let id = message.request();
        let Some(sent) = self.sent.get_mut(&id) else {
            return Err(Refused::Unexpected("no such request".into()));
        };
        if sent.shop != from {
            return Err(Refused::Unexpected("not from the request's shop".into()));
        }
        let shop = sent.shop_name.clone();
        match message {
            Message::Quote { quote, .. } => {
                if !matches!(sent.stage, Stage::Requested | Stage::Quoted) {
                    return Err(Refused::Unexpected("quoted too late".into()));
                }
                sent.stage = Stage::Quoted;
                sent.quote = Some(quote.clone());
                Ok(News::Quoted {
                    shop,
                    total: format!("{} {}", quote.currency, quote.total.format()),
                })
            }
            Message::Decline { reason, .. } => {
                sent.stage = Stage::Declined;
                sent.decline_reason = Some(reason.clone());
                Ok(News::Declined { shop })
            }
            Message::Stage { stage, .. } => {
                sent.stage = *stage;
                Ok(News::Stage {
                    shop,
                    stage: *stage,
                })
            }
            Message::Chat { text, .. } => {
                sent.chat.push((true, text.clone(), now));
                Ok(News::Chat {
                    shop,
                    text: text.clone(),
                })
            }
            Message::Delivered { receipt, .. } => {
                // It must be the receipt for this order, between this shop and its client.
                let ok = matches!(&receipt.receipt, cadrs_social::receipts::Receipt::OrderCompleted {
                    shop, order, ..
                } if *shop == sent.shop && *order == crate::order_ref(id));
                if !ok {
                    return Err(Refused::Unexpected("not this order's receipt".into()));
                }
                sent.stage = Stage::Ready;
                sent.delivery = Some(Delivery::ToSign(receipt.clone()));
                Ok(News::Delivered { shop })
            }
            Message::Request { .. }
            | Message::Accept { .. }
            | Message::Received { .. }
            | Message::Review { .. } => Err(Refused::Unexpected(
                "a client's message, sent to a client".into(),
            )),
        }
    }

    /// The client writes about a request: keeps it, and returns the letter's message.
    pub fn say(&mut self, id: Id, text: &str, now: u64) -> Result<Message, Refused> {
        let sent = self
            .sent
            .get_mut(&id)
            .ok_or_else(|| Refused::Unexpected("no such request".into()))?;
        if text.trim().is_empty() {
            return Err(Refused::Unexpected("an empty message".into()));
        }
        sent.chat.push((false, text.to_owned(), now));
        Ok(Message::Chat {
            request: id,
            text: text.to_owned(),
        })
    }

    /// The client signs for delivery with its account key: the receipt both signed, and the
    /// letter's message.
    pub fn sign_delivery(
        &mut self,
        id: Id,
        account: &cadrs_social::keys::SecretKey,
    ) -> Result<Message, Refused> {
        let sent = self
            .sent
            .get_mut(&id)
            .ok_or_else(|| Refused::Unexpected("no such request".into()))?;
        let Some(Delivery::ToSign(half)) = sent.delivery.clone() else {
            return Err(Refused::Unexpected("nothing to sign for".into()));
        };
        let receipt = half
            .countersign(account)
            .map_err(|e| Refused::Unexpected(format!("{e:?}")))?;
        sent.delivery = Some(Delivery::Signed(receipt.clone()));
        sent.stage = Stage::Completed;
        Ok(Message::Received {
            request: id,
            receipt,
        })
    }

    /// The client reviews a completed order: keeps it, and returns the letter's message.
    pub fn review(&mut self, id: Id, stars: u8, text: &str) -> Result<Message, Refused> {
        let sent = self
            .sent
            .get_mut(&id)
            .ok_or_else(|| Refused::Unexpected("no such request".into()))?;
        if sent.stage != Stage::Completed || sent.review.is_some() || !(1..=5).contains(&stars) {
            return Err(Refused::Unexpected(
                "only a completed order is reviewed, once, 1 to 5 stars".into(),
            ));
        }
        sent.review = Some((stars, text.to_owned()));
        Ok(Message::Review {
            request: id,
            stars,
            text: text.to_owned(),
        })
    }

    /// The client accepts a quote: marks it, and returns the letter's message.
    pub fn accept(&mut self, id: Id) -> Result<Message, Refused> {
        let sent = self
            .sent
            .get_mut(&id)
            .ok_or_else(|| Refused::Unexpected("no such request".into()))?;
        if sent.stage != Stage::Quoted {
            return Err(Refused::Unexpected("there is no quote to accept".into()));
        }
        sent.stage = Stage::Accepted;
        Ok(Message::Accept { request: id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::tests::bracket;
    use crate::quote::Money;
    use crate::session::Session;
    use cadrs_social::contact::Kind;
    use cadrs_social::keys::Account;

    struct Side {
        account: Account,
        key: SecretKey,
        cert: Signed<DeviceCert>,
    }

    fn side() -> Side {
        let account = Account::generate();
        let key = SecretKey::generate();
        let cert = account.certify(key.public(), "device", 0);
        Side { account, key, cert }
    }

    #[test]
    fn a_request_and_its_quote_go_round() {
        let designer = side();
        let owner = side();
        let (mut shop, org_account) = Session::found(
            owner.key.clone(),
            owner.cert.clone(),
            "Cape Waterjet",
            1_000,
        );

        // The designer sends a request for a quote to the shop's account.
        let id = crate::new_id();
        let card = Card {
            account: designer.account.id(),
            kind: Kind::Person,
            name: "Alex".into(),
            devices: vec![designer.key.public()],
            mailboxes: vec![],
            issued: 1,
        }
        .sign(designer.account.key());
        let message = Message::Request {
            id,
            parts: vec![(bracket(), 6)],
            notes: "brushed".into(),
            deadline: None,
            snapshot: b"packed document".to_vec(),
            client: card,
        };
        let mut book = ClientBook::default();
        book.record(
            id,
            Sent {
                shop: org_account.id(),
                shop_name: "Cape Waterjet".into(),
                parts: vec![(bracket(), 6)],
                notes: "brushed".into(),
                stage: Stage::Requested,
                quote: None,
                decline_reason: None,
                chat: vec![],
                sent: 2_000,
                delivery: None,
                review: None,
            },
        );
        let bytes = write(
            &designer.key,
            &designer.cert,
            org_account.id(),
            message,
            2_000,
        );

        // The shop reads it and writes it into its document.
        let (letter, from) = read(&bytes, &[org_account.id()]).unwrap();
        assert_eq!(from, designer.account.id());
        assert_eq!(
            read(&bytes, &[designer.account.id()]),
            Err(Refused::NotForUs)
        );
        let op = shop_op(shop.org(), from, &letter.message).unwrap();
        shop.write(op, 3_000).unwrap();
        assert_eq!(shop.org().requests[&id].stage, Stage::Requested);
        // The same request again is refused.
        assert!(shop_op(shop.org(), from, &letter.message).is_err());

        // The shop quotes; the designer's book shows it.
        let quote = Quote {
            total: Money::from_major(724.0),
            currency: "ZAR".into(),
            lines: vec![],
            valid_until: 0,
            note: String::new(),
        };
        let reply = write(
            &owner.key,
            &owner.cert,
            designer.account.id(),
            Message::Quote { request: id, quote },
            4_000,
        );
        // The shop's devices sign for the org: the client knows the shop by its org account,
        // so the letter's sender must be the org. Here the owner's device signs as the owner,
        // which the book refuses.
        let (letter, from) = read(&reply, &[designer.account.id()]).unwrap();
        assert!(book.receive(from, &letter.message, 4_000).is_err());
        // Signed by a device of the org's own account, it is taken.
        let org_device = SecretKey::generate();
        let org_cert = org_account.certify(org_device.public(), "shop box", 0);
        let reply = write(
            &org_device,
            &org_cert,
            designer.account.id(),
            letter.message.clone(),
            4_000,
        );
        let (letter, from) = read(&reply, &[designer.account.id()]).unwrap();
        assert_eq!(
            book.receive(from, &letter.message, 4_000),
            Ok(News::Quoted {
                shop: "Cape Waterjet".into(),
                total: "ZAR 724.00".into()
            })
        );

        // The designer accepts; the shop's document takes it.
        let accept = book.accept(id).unwrap();
        shop.write(
            Op::Quote {
                request: id,
                quote: book.sent[&id].quote.clone().unwrap(),
            },
            5_000,
        )
        .unwrap();
        let bytes = write(
            &designer.key,
            &designer.cert,
            org_account.id(),
            accept,
            6_000,
        );
        let (letter, from) = read(&bytes, &[org_account.id()]).unwrap();
        let op = shop_op(shop.org(), from, &letter.message).unwrap();
        shop.write(op, 6_000).unwrap();
        assert_eq!(shop.org().requests[&id].stage, Stage::Accepted);

        // The designer asks something; the shop's document keeps it.
        let say = book.say(id, "Can you deburr them?", 7_000).unwrap();
        let bytes = write(&designer.key, &designer.cert, org_account.id(), say, 7_000);
        let (letter, from) = read(&bytes, &[org_account.id()]).unwrap();
        let op = shop_op(shop.org(), from, &letter.message).unwrap();
        shop.write(op, 7_000).unwrap();
        assert_eq!(
            shop.org().requests[&id].chat,
            vec![(true, "Can you deburr them?".to_string(), 7_000)]
        );
        assert_eq!(book.sent[&id].chat.len(), 1);

        // Delivered: the shop signs half the receipt with the organization's key, the client
        // countersigns, the shop's document completes the order; then the review.
        let half = cadrs_social::receipts::Receipt::OrderCompleted {
            shop: org_account.id(),
            client: designer.account.id(),
            order: crate::order_ref(id),
            at: 8_000,
        }
        .sign_first(org_account.key())
        .unwrap();
        let delivered = Message::Delivered {
            request: id,
            receipt: half,
        };
        assert_eq!(
            book.receive(org_account.id(), &delivered, 8_000),
            Ok(News::Delivered {
                shop: "Cape Waterjet".into()
            })
        );
        assert!(book.review(id, 5, "early").is_err(), "not signed for yet");
        let received = book.sign_delivery(id, designer.account.key()).unwrap();
        let op = shop_op(shop.org(), designer.account.id(), &received).unwrap();
        shop.write(op, 9_000).unwrap();
        assert_eq!(shop.org().requests[&id].stage, Stage::Completed);
        let review = book.review(id, 5, "Clean edges").unwrap();
        let op = shop_op(shop.org(), designer.account.id(), &review).unwrap();
        shop.write(op, 9_500).unwrap();
        assert_eq!(shop.org().rating(), Some((5.0, 1)));
        assert!(book.review(id, 4, "again").is_err(), "once");

        // Another device of the designer's account, which didn't send the request, learns it
        // from the shop's quote.
        let mut phone = ClientBook::default();
        let quote = Message::Quote {
            request: id,
            quote: shop.org().requests[&id].quote.clone().unwrap(),
        };
        assert!(phone.receive(org_account.id(), &quote, 10_000).is_err());
        assert!(
            phone
                .receive_learning(org_account.id(), "Cape Waterjet", &quote, 10_000)
                .is_ok()
        );
        assert_eq!(phone.sent[&id].stage, Stage::Quoted);
        assert!(phone.accept(id).is_ok());
        phone.say(id, "From the phone", 11_000).unwrap();

        // The two devices' books merge: furthest stage, both conversations.
        let mut laptop = book.clone();
        assert!(laptop.merge(&phone));
        assert_eq!(laptop.sent[&id].stage, Stage::Completed);
        assert!(
            laptop.sent[&id]
                .chat
                .iter()
                .any(|(_, t, _)| t == "From the phone")
        );
        assert!(phone.merge(&laptop));
        assert_eq!(phone.sent[&id].stage, Stage::Completed);
        assert_eq!(phone.sent[&id].review, Some((5, "Clean edges".into())));
        // Merging again changes nothing.
        assert!(!laptop.merge(&phone));
    }

    #[test]
    fn forged_letters_are_refused() {
        let a = side();
        let b = side();
        let bytes = write(
            &a.key,
            &b.cert,
            a.account.id(),
            Message::Accept { request: 1 },
            0,
        );
        assert_eq!(read(&bytes, &[a.account.id()]), Err(Refused::Forged));
        assert_eq!(read(b"junk", &[a.account.id()]), Err(Refused::Forged));
    }
}
