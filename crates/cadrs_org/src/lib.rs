//! Organization documents: a shop (or any organization) as a signed log of operations.
//!
//! Every shop is an organization from the start; a one-person shop is an organization with one
//! member, so growing means adding people, never moving history. The document is a
//! [`cadrs_social::log::Log`] whose entries carry [`Op`]s; [`Org`] is what replaying them in the
//! log's order gives: members and their devices, machines, materials and stock, the rate card,
//! clients, requests for quotes and the orders they become, jobs, parts and reviews. Each tab of
//! the document shows one part of it.
//!
//! [`Org`] is the log's reducer: it checks each operation against the state before it (is the
//! author a member, may their role do this, is there stock left) and answers
//! [`Outcome::Conflicted`] when not, the same way on every peer.

pub mod capability;
pub mod directory;
pub mod file;
pub mod letters;
pub mod quote;
pub mod session;

use std::collections::BTreeMap;

use cadrs_social::keys::{AccountId, DeviceCert, DeviceId, Devices, Revocation, Signed};
use cadrs_social::log::{Entry, EntryId, Outcome, Reducer, Witnesses};
use cadrs_social::place::Place;
use cadrs_social::receipts::{CoSigned, Receipt};
use cadrs_social::reputation::{Reputation, witness_weight};
use serde::{Deserialize, Serialize};

use capability::{MachineCaps, PartSummary};
use quote::{Money, RateCard};

/// An id for something in the document (a machine, an order), made where it was created.
pub type Id = u128;

/// A new random [`Id`].
pub fn new_id() -> Id {
    u128::from_le_bytes(cadrs_social::random_bytes())
}

/// What a member may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Role {
    /// Runs the shop's work: requests, quotes, orders, jobs, stock, clients.
    Member,
    /// Also the shop's set-up: machines, materials, prices, profile, members.
    Admin,
    /// Also other owners.
    Owner,
}

impl Role {
    /// A witness's weight before reputation.
    pub fn witness_weight(self) -> u64 {
        match self {
            Role::Member => 1,
            Role::Admin => 2,
            Role::Owner => 3,
        }
    }
}

/// The shop's public face.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub about: String,
    pub place: Option<Place>,
    /// How precisely the place is shown publicly, in metres (contacts see it all).
    pub public_accuracy_m: f64,
    pub website: Option<String>,
    /// Takes requests for quotes from anyone, not just contacts.
    pub open_for_quotes: bool,
}

/// A stock material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// "mild steel".
    pub name: String,
    pub thickness: f64,
    /// The sheet's size, mm.
    pub sheet: [f64; 2],
}

/// A client of the shop.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Client {
    pub name: String,
    /// Their cadrs account, if they have one.
    pub account: Option<AccountId>,
    pub email: String,
    pub phone: String,
    pub notes: String,
}

/// Where a request for a quote is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stage {
    Requested,
    Quoted,
    Declined,
    /// The client accepted the quote: an order.
    Accepted,
    InProduction,
    Ready,
    /// Delivered, with a receipt both signed.
    Completed,
}

/// A request for a quote, and the order it may become.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub client: AccountId,
    pub parts: Vec<(PartSummary, u32)>,
    pub notes: String,
    /// The frozen snapshot of the parts (a blob's hash).
    pub snapshot: [u8; 32],
    /// Milliseconds since the Unix epoch.
    pub deadline: Option<u64>,
    pub stage: Stage,
    pub quote: Option<Quote>,
    pub decline_reason: Option<String>,
    pub receipt: Option<CoSigned>,
    /// Milliseconds since the Unix epoch, when it arrived.
    pub received: u64,
    /// The client's card (how to reach them).
    #[serde(default)]
    pub card: Option<Signed<cadrs_social::contact::Card>>,
    /// The conversation about it, oldest first: (from the client, text, when).
    #[serde(default)]
    pub chat: Vec<(bool, String, u64)>,
}

/// A shop's price for a request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub total: Money,
    pub currency: String,
    pub lines: Vec<quote::Line>,
    /// Milliseconds since the Unix epoch.
    pub valid_until: u64,
    pub note: String,
}

/// A job on a machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub request: Id,
    pub machine: Id,
    /// Milliseconds since the Unix epoch.
    pub start: u64,
    pub minutes: f64,
    pub state: JobState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Scheduled,
    Running,
    Done,
}

/// A part the shop makes (often for a client), and what goes into it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub name: String,
    pub revision: String,
    pub bom: Vec<BomLine>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BomLine {
    Material { material: Id, sheets: f64 },
    Part { part: Id, quantity: u32 },
    Bought { name: String, quantity: u32 },
}

/// A client's review of a completed order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub by: AccountId,
    /// 1 to 5.
    pub stars: u8,
    pub text: String,
    pub request: Id,
}

/// Stock taken out for an order but not used yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reservation {
    pub material: Id,
    pub sheets: u32,
    pub request: Id,
}

/// One change to an organization document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op {
    /// The first entry: the organization, its founder and the founder's device (the author).
    Found {
        name: String,
        org: AccountId,
        founder: Signed<DeviceCert>,
    },
    AddDevice(Signed<DeviceCert>),
    RevokeDevice(Signed<Revocation>),
    /// Sets someone's role; `None` removes them.
    SetMember {
        account: AccountId,
        role: Option<Role>,
    },
    /// Makes a device a witness (an always-on shop box, an owner's laptop) or not.
    SetWitness {
        device: DeviceId,
        on: bool,
    },
    SetProfile(Profile),
    SetMachine {
        id: Id,
        machine: Option<MachineCaps>,
    },
    SetMaterial {
        id: Id,
        material: Option<Material>,
    },
    /// Sheets in (positive) or out (negative), outside of reservations: a delivery, a count.
    Stock {
        material: Id,
        sheets: i64,
    },
    Reserve {
        id: Id,
        reservation: Reservation,
    },
    /// A reservation's sheets go back to stock (`used: false`) or are used up.
    Release {
        id: Id,
        used: bool,
    },
    SetRates(RateCard),
    SetClient {
        id: Id,
        client: Option<Client>,
    },
    /// A request for a quote arrived.
    Request {
        id: Id,
        client: AccountId,
        parts: Vec<(PartSummary, u32)>,
        notes: String,
        snapshot: [u8; 32],
        deadline: Option<u64>,
        /// How to reach the client: any member's device can answer.
        card: Option<Signed<cadrs_social::contact::Card>>,
    },
    Quote {
        request: Id,
        quote: Quote,
    },
    Decline {
        request: Id,
        reason: String,
    },
    /// The client accepted the quote.
    Accept {
        request: Id,
    },
    /// The order moves on (in production, ready).
    Advance {
        request: Id,
        stage: Stage,
    },
    /// Delivered: the receipt both sides signed.
    Complete {
        request: Id,
        receipt: CoSigned,
    },
    SetJob {
        id: Id,
        job: Option<Job>,
    },
    SetPart {
        id: Id,
        part: Option<Part>,
    },
    Review {
        id: Id,
        review: Review,
        receipt: CoSigned,
    },
    /// A message about a request, from the client or from the shop.
    Note {
        request: Id,
        from_client: bool,
        text: String,
    },
    /// A message among the members, in a channel ("general", "night shift").
    Say {
        channel: String,
        text: String,
    },
}

impl Op {
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("ops encode")
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }

    /// The least role that may do this.
    fn needs(&self) -> Role {
        match self {
            Op::SetMember { .. }
            | Op::SetWitness { .. }
            | Op::SetProfile(_)
            | Op::SetMachine { .. }
            | Op::SetMaterial { .. }
            | Op::SetRates(_) => Role::Admin,
            _ => Role::Member,
        }
    }
}

/// An organization, as its document's operations so far make it.
#[derive(Clone, Debug, Default)]
pub struct Org {
    pub org: Option<AccountId>,
    pub name: String,
    pub devices: Devices,
    pub members: BTreeMap<AccountId, Role>,
    pub witnesses: BTreeMap<DeviceId, bool>,
    pub profile: Profile,
    pub machines: BTreeMap<Id, MachineCaps>,
    pub materials: BTreeMap<Id, Material>,
    /// Sheets on hand (reserved ones included), by material.
    pub stock: BTreeMap<Id, i64>,
    pub reservations: BTreeMap<Id, Reservation>,
    pub rates: Option<RateCard>,
    pub clients: BTreeMap<Id, Client>,
    pub requests: BTreeMap<Id, Request>,
    pub jobs: BTreeMap<Id, Job>,
    pub parts: BTreeMap<Id, Part>,
    pub reviews: BTreeMap<Id, Review>,
    /// The members' channels: (who, what, when), oldest first.
    pub channels: BTreeMap<String, Vec<(AccountId, String, u64)>>,
}

fn conflict(why: impl Into<String>) -> Outcome {
    Outcome::Conflicted(why.into())
}

impl Org {
    /// Sheets of `material` not reserved for an order.
    pub fn available(&self, material: Id) -> i64 {
        let reserved: i64 = self
            .reservations
            .values()
            .filter(|r| r.material == material)
            .map(|r| r.sheets as i64)
            .sum();
        self.stock.get(&material).copied().unwrap_or(0) - reserved
    }

    /// The account a device signs for in this org at `time`, if it's a member's.
    pub fn member_of(&self, device: &DeviceId, time: u64) -> Option<(AccountId, Role)> {
        let account = self.devices.account_of(device, time)?;
        Some((account, *self.members.get(&account)?))
    }

    /// The org's witnesses and their weights, each device's role weight times its reputation's
    /// multiplier (`reputation` looks a device's up).
    pub fn witnesses(&self, reputation: impl Fn(&DeviceId) -> Reputation) -> Witnesses {
        self.witnesses
            .iter()
            .filter(|(_, on)| **on)
            .filter_map(|(device, _)| {
                let account = self.devices.account_of(device, u64::MAX - 1)?;
                let role = self.members.get(&account)?;
                Some((
                    *device,
                    witness_weight(role.witness_weight(), &reputation(device)),
                ))
            })
            .collect()
    }

    /// Average stars, and how many reviews.
    pub fn rating(&self) -> Option<(f64, usize)> {
        let n = self.reviews.len();
        (n > 0).then(|| {
            let sum: u32 = self.reviews.values().map(|r| r.stars as u32).sum();
            (sum as f64 / n as f64, n)
        })
    }

    fn found(&mut self, entry: &Entry, op: &Op) -> Outcome {
        let Op::Found { name, org, founder } = op else {
            return conflict("an organization document starts by founding it");
        };
        let Ok(cert) = founder.verify() else {
            return conflict("the founder's certificate doesn't verify");
        };
        if cert.device != entry.body.author || founder.signer != cert.account {
            return conflict("the founder's certificate is not for the device that wrote it");
        }
        self.devices.add_cert(founder).expect("verified");
        self.org = Some(*org);
        self.name = name.clone();
        self.profile.name = name.clone();
        self.members.insert(cert.account, Role::Owner);
        self.witnesses.insert(cert.device, true);
        Outcome::Applied
    }

    fn apply_op(&mut self, author: AccountId, role: Role, time: u64, op: Op) -> Outcome {
        if role < op.needs() {
            return conflict(format!("a {role:?} may not do this"));
        }
        match op {
            Op::Found { .. } => conflict("this organization was already founded"),
            Op::AddDevice(cert) => {
                let Ok(value) = cert.verify() else {
                    return conflict("the certificate doesn't verify");
                };
                if value.account != author && role < Role::Admin {
                    return conflict("only an admin adds devices of other accounts");
                }
                if !self.members.contains_key(&value.account) {
                    return conflict("the device's account is not a member");
                }
                self.devices.add_cert(&cert).expect("verified");
                Outcome::Applied
            }
            Op::RevokeDevice(revocation) => match self.devices.add_revocation(&revocation) {
                Ok(()) => Outcome::Applied,
                Err(_) => conflict("the revocation doesn't verify"),
            },
            Op::SetMember { account, role: new } => {
                let touches_owner =
                    new == Some(Role::Owner) || self.members.get(&account) == Some(&Role::Owner);
                if touches_owner && role < Role::Owner {
                    return conflict("only an owner makes or removes owners");
                }
                match new {
                    Some(new) => {
                        self.members.insert(account, new);
                    }
                    None => {
                        let owners = self.members.values().filter(|r| **r == Role::Owner).count();
                        if self.members.get(&account) == Some(&Role::Owner) && owners == 1 {
                            return conflict("an organization keeps at least one owner");
                        }
                        self.members.remove(&account);
                    }
                }
                Outcome::Applied
            }
            Op::SetWitness { device, on } => {
                if self.member_of(&device, time).is_none() {
                    return conflict("the device is not a member's");
                }
                self.witnesses.insert(device, on);
                Outcome::Applied
            }
            Op::SetProfile(profile) => {
                self.profile = profile;
                Outcome::Applied
            }
            Op::SetMachine { id, machine } => {
                set(&mut self.machines, id, machine);
                Outcome::Applied
            }
            Op::SetMaterial { id, material } => {
                set(&mut self.materials, id, material);
                Outcome::Applied
            }
            Op::Stock { material, sheets } => {
                if !self.materials.contains_key(&material) {
                    return conflict("no such material");
                }
                if self.available(material) + sheets < 0 {
                    return conflict(format!("only {} sheets are free", self.available(material)));
                }
                *self.stock.entry(material).or_default() += sheets;
                Outcome::Applied
            }
            Op::Reserve { id, reservation } => {
                if self.reservations.contains_key(&id) {
                    return conflict("that reservation exists");
                }
                if !self.requests.contains_key(&reservation.request) {
                    return conflict("no such order");
                }
                let free = self.available(reservation.material);
                if (reservation.sheets as i64) > free {
                    return conflict(format!(
                        "only {free} sheets are free; {} were asked for",
                        reservation.sheets
                    ));
                }
                self.reservations.insert(id, reservation);
                Outcome::Applied
            }
            Op::Release { id, used } => {
                let Some(reservation) = self.reservations.remove(&id) else {
                    return conflict("no such reservation");
                };
                if used {
                    *self.stock.entry(reservation.material).or_default() -=
                        reservation.sheets as i64;
                }
                Outcome::Applied
            }
            Op::SetRates(rates) => {
                self.rates = Some(rates);
                Outcome::Applied
            }
            Op::SetClient { id, client } => {
                set(&mut self.clients, id, client);
                Outcome::Applied
            }
            Op::Request {
                id,
                client,
                parts,
                notes,
                snapshot,
                deadline,
                card,
            } => {
                if self.requests.contains_key(&id) {
                    return conflict("that request exists");
                }
                self.requests.insert(
                    id,
                    Request {
                        client,
                        parts,
                        notes,
                        snapshot,
                        deadline,
                        stage: Stage::Requested,
                        quote: None,
                        decline_reason: None,
                        receipt: None,
                        received: time,
                        chat: Vec::new(),
                        card,
                    },
                );
                Outcome::Applied
            }
            Op::Quote { request, quote } => {
                self.advance(request, &[Stage::Requested, Stage::Quoted], |r| {
                    r.quote = Some(quote);
                    r.stage = Stage::Quoted;
                })
            }
            Op::Decline { request, reason } => {
                self.advance(request, &[Stage::Requested, Stage::Quoted], |r| {
                    r.decline_reason = Some(reason);
                    r.stage = Stage::Declined;
                })
            }
            Op::Accept { request } => self.advance(request, &[Stage::Quoted], |r| {
                r.stage = Stage::Accepted;
            }),
            Op::Advance { request, stage } => {
                let from: &[Stage] = match stage {
                    Stage::InProduction => &[Stage::Accepted],
                    Stage::Ready => &[Stage::Accepted, Stage::InProduction],
                    _ => return conflict("use Quote, Decline, Accept or Complete for that"),
                };
                self.advance(request, from, |r| r.stage = stage)
            }
            Op::Complete { request, receipt } => {
                let Some(r) = self.requests.get(&request) else {
                    return conflict("no such request");
                };
                match receipt.verify() {
                    Ok(Receipt::OrderCompleted {
                        shop,
                        client,
                        order,
                        ..
                    }) if Some(*shop) == self.org
                        && *client == r.client
                        && *order == order_ref(request) => {}
                    _ => return conflict("the receipt is not for this order"),
                }
                self.advance(
                    request,
                    &[Stage::Accepted, Stage::InProduction, Stage::Ready],
                    |r| {
                        r.receipt = Some(receipt);
                        r.stage = Stage::Completed;
                    },
                )
            }
            Op::SetJob { id, job } => {
                if let Some(job) = &job
                    && (!self.requests.contains_key(&job.request)
                        || !self.machines.contains_key(&job.machine))
                {
                    return conflict("the job's order or machine doesn't exist");
                }
                set(&mut self.jobs, id, job);
                Outcome::Applied
            }
            Op::Note {
                request,
                from_client,
                text,
            } => {
                let Some(r) = self.requests.get_mut(&request) else {
                    return conflict("no such request");
                };
                if text.trim().is_empty() {
                    return conflict("an empty message");
                }
                r.chat.push((from_client, text, time));
                Outcome::Applied
            }
            Op::Say { channel, text } => {
                let channel = channel.trim().trim_start_matches('#').to_lowercase();
                if channel.is_empty() || text.trim().is_empty() {
                    return conflict("a message needs a channel and some words");
                }
                self.channels
                    .entry(channel)
                    .or_default()
                    .push((author, text, time));
                Outcome::Applied
            }
            Op::SetPart { id, part } => {
                set(&mut self.parts, id, part);
                Outcome::Applied
            }
            Op::Review {
                id,
                review,
                receipt,
            } => {
                let Some(r) = self.requests.get(&review.request) else {
                    return conflict("no such order");
                };
                if r.stage != Stage::Completed || r.receipt.as_ref() != Some(&receipt) {
                    return conflict("only a completed order is reviewed");
                }
                if review.by != r.client || !(1..=5).contains(&review.stars) {
                    return conflict("a review is the client's, of 1 to 5 stars");
                }
                if self.reviews.values().any(|v| v.request == review.request) {
                    return conflict("that order was reviewed already");
                }
                self.reviews.insert(id, review);
                Outcome::Applied
            }
        }
    }

    fn advance(
        &mut self,
        request: Id,
        from: &[Stage],
        change: impl FnOnce(&mut Request),
    ) -> Outcome {
        let Some(r) = self.requests.get_mut(&request) else {
            return conflict("no such request");
        };
        if !from.contains(&r.stage) {
            return conflict(format!("the request is {:?}", r.stage));
        }
        change(r);
        Outcome::Applied
    }
}

/// The receipt order id for a request id.
pub fn order_ref(request: Id) -> [u8; 32] {
    request
        .to_le_bytes()
        .repeat(2)
        .try_into()
        .expect("32 bytes")
}

fn set<T>(map: &mut BTreeMap<Id, T>, id: Id, value: Option<T>) {
    match value {
        Some(value) => {
            map.insert(id, value);
        }
        None => {
            map.remove(&id);
        }
    }
}

impl Reducer for Org {
    fn apply(&mut self, _: EntryId, entry: &Entry, data: &[u8]) -> Outcome {
        let Some(op) = Op::decode(data) else {
            return conflict("not an operation this version knows");
        };
        if self.org.is_none() {
            return self.found(entry, &op);
        }
        let time = entry.body.time.millis;
        let Some((author, role)) = self.member_of(&entry.body.author, time) else {
            return conflict("the author is not a member");
        };
        self.apply_op(author, role, time, op)
    }
}
