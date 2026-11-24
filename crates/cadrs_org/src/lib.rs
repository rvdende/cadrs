//! Organization documents: a shop (or any organization) as a signed log of operations.
//!
//! Every shop is an organization from the start; a one-person shop is an organization with one
//! member, so growing means adding people, never moving history. The document is a
//! [`cadrs_social::log::Log`] whose entries carry [`Op`]s; [`Org`] is what replaying them in the
//! log's order gives: members and their devices, machines, materials and stock, the rate card,
//! clients, requests for quotes and the orders they become, jobs, parts, reviews and quality
//! issues, its public [`Profile`] and its [`Location`]s. Each module of the organization's
//! screen shows one part of it.
//!
//! [`Org`] is the log's reducer: it checks each operation against the state before it (is the
//! author a member, may their role do this, is there stock left) and answers
//! [`Outcome::Conflicted`] when not, the same way on every peer.
//!
//! Operations are written with their names ([`cadrs_social::codec`]), so the document can grow:
//! see that module for the rules. An operation a newer cadrs wrote is counted
//! ([`Org::newer_ops`]) and otherwise left alone, so an organization whose members run different
//! versions keeps working; the README has the map of what is here and what is to come.

pub mod blob;
pub mod capability;
pub mod country;
pub mod directory;
pub mod file;
pub mod letters;
pub mod profile;
pub mod quality;
pub mod quote;
pub mod refusal;
pub mod session;

use std::collections::BTreeMap;

use cadrs_social::keys::{AccountId, DeviceCert, DeviceId, Devices, Revocation, Signed};
use cadrs_social::log::{Entry, EntryId, Outcome, Reducer, Witnesses};
use cadrs_social::receipts::{CoSigned, Receipt};
use cadrs_social::reputation::{Reputation, witness_weight};
use serde::{Deserialize, Serialize};

use capability::{MachineCaps, PartSummary};
pub use profile::{Address, BlobRef, Location, Logo, Offers, Profile};
use quality::{Disposition, Issue, IssueAbout, IssueStatus};
use quote::{Money, RateCard};

/// The longest message, note or description, in characters.
pub const MAX_TEXT: usize = 4000;
/// The longest name of something the organization keeps (a machine, a material, a client).
pub const MAX_LABEL: usize = 120;
/// The largest operation, as written, in bytes. Whatever an operation holds, and whatever is
/// added to one later, an entry of the log is never larger: the log is kept whole on every
/// member's device, and strangers' requests are written into it.
pub const MAX_OP: usize = 128 << 10;
/// The largest request for a quote, as written (its parts, notes and card; its document is a
/// blob beside the log).
pub const MAX_REQUEST: usize = 48 << 10;
/// The most messages either side writes about one request, and the most their text weighs
/// together, in bytes: a conversation about an order, not an archive. Each side has its own
/// share, so one can't write the other out.
pub const MAX_NOTES: usize = 100;
pub const MAX_NOTE_BYTES: usize = 64 << 10;
/// What a client may write about a request before it is an order: questions, not a flood.
pub const MAX_NOTES_BEFORE_ORDER: usize = 10;
pub const MAX_NOTE_BYTES_BEFORE_ORDER: usize = 8 << 10;
/// The most requests taken in a day: from one client, and from all the people the organization
/// doesn't know yet together ([`Org::knows`]). What waits has a ceiling of its own; this one is
/// on how fast declining and asking again can make the log grow.
pub const MAX_REQUESTS_A_DAY_PER_CLIENT: usize = 20;
pub const MAX_REQUESTS_A_DAY: usize = 300;
const DAY: u64 = 24 * 60 * 60 * 1000;
/// The largest document a request for a quote carries, in bytes.
pub const MAX_SNAPSHOT: u64 = 32 << 20;
/// The most different parts one request asks for.
pub const MAX_REQUEST_PARTS: usize = 100;
/// The most requests that wait for an answer (neither quoted nor declined yet): from one
/// client, and from all the people the organization doesn't know yet together. Anyone with the
/// organization's link can ask, so what strangers can make it keep has a ceiling; declining a
/// request makes room.
pub const MAX_WAITING_PER_CLIENT: usize = 5;
pub const MAX_WAITING: usize = 200;
/// The most the documents of the waiting requests weigh together, in bytes.
pub const MAX_WAITING_BYTES: u64 = 512 << 20;
/// How long after a quote ran out an acceptance sent in time may still arrive (it may have
/// waited at a mailbox), in milliseconds. The client's word for when it was sent is taken only
/// within this.
pub const ACCEPT_GRACE: u64 = 3 * 24 * 60 * 60 * 1000;

fn longer(text: &str, max: usize) -> bool {
    text.chars().count() > max
}

/// An id for something in the document (a machine, an order), made where it was created.
pub type Id = u128;

/// Whether `card`'s signature holds. Each card is checked once a run: an organization's
/// document is read again from its first entry whenever something is written to it, and a
/// signature costs more than everything else about a request together, so a long history of
/// requests would make every click slow. (Whether a signature holds never changes, so
/// remembering that it did is safe; only those that held are remembered.)
fn card_verifies(card: &Signed<cadrs_social::contact::Card>) -> bool {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    /// How many cards are remembered before starting over.
    const REMEMBERED: usize = 100_000;
    static HELD: OnceLock<Mutex<HashSet<[u8; 32]>>> = OnceLock::new();
    let held = HELD.get_or_init(Default::default);
    let key = *blake3::hash(&cadrs_social::codec::encode(card)).as_bytes();
    if held.lock().is_ok_and(|held| held.contains(&key)) {
        return true;
    }
    if card.verify().is_err() {
        return false;
    }
    if let Ok(mut held) = held.lock() {
        if held.len() >= REMEMBERED {
            held.clear();
        }
        held.insert(key);
    }
    true
}

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
    /// Its number in the organization, from 1, in the order the requests arrived.
    #[serde(default)]
    pub number: u32,
    pub client: AccountId,
    pub parts: Vec<(PartSummary, u32)>,
    pub notes: String,
    /// The frozen document of the parts: a blob's hash ([`blob`]), and its size.
    pub snapshot: [u8; 32],
    #[serde(default)]
    pub snapshot_len: u64,
    /// Milliseconds since the Unix epoch.
    pub deadline: Option<u64>,
    pub stage: Stage,
    pub quote: Option<Quote>,
    pub decline_reason: Option<String>,
    /// When it was declined, in milliseconds since the Unix epoch: its document is kept a day
    /// longer ([`blob::sweep`]).
    #[serde(default)]
    pub declined_at: Option<u64>,
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

impl Request {
    /// The blob holding the document the client sent, if it sent one.
    pub fn snapshot_blob(&self) -> Option<BlobRef> {
        (self.snapshot_len > 0).then_some(BlobRef {
            hash: self.snapshot,
            len: self.snapshot_len,
        })
    }
}

/// A shop's price for a request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    /// Which quote this is: an acceptance names it, so a quote the shop replaced meanwhile
    /// isn't accepted by mistake.
    #[serde(default)]
    pub id: Id,
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
    /// Sets the whole profile: how an organization starts.
    SetProfile(Profile),
    /// Changes the parts of the profile it names and leaves the rest as it is, so two people
    /// changing different things at once (one the tagline, one the listing) both keep theirs.
    EditProfile(profile::ProfileEdit),
    /// Sets a location; `None` removes it (not while machines stand there).
    SetLocation {
        id: Id,
        location: Option<Location>,
    },
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
        #[serde(default)]
        snapshot_len: u64,
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
    /// The client accepted the quote `quote` (0: whichever stands) at `at` (milliseconds since
    /// the Unix epoch, 0: when this was written). Refused when that quote was replaced or had
    /// run out by then.
    Accept {
        request: Id,
        #[serde(default)]
        quote: Id,
        #[serde(default)]
        at: u64,
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
    /// Opens a quality issue ([`quality`]).
    OpenIssue {
        id: Id,
        title: String,
        details: String,
        about: Option<IssueAbout>,
        owner: Option<AccountId>,
    },
    /// Moves an issue on: its status, owner and what was decided. Closing needs a disposition
    /// and the reasoning behind it.
    UpdateIssue {
        id: Id,
        status: IssueStatus,
        owner: Option<AccountId>,
        disposition: Option<Disposition>,
        rationale: String,
    },
    /// Stops taking requests for quotes from an account, or takes them again. What it sent
    /// before stays as it is (decline it if it isn't wanted).
    SetBlocked {
        account: AccountId,
        blocked: bool,
    },
}

impl Op {
    pub fn encode(&self) -> Vec<u8> {
        cadrs_social::codec::encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, cadrs_social::codec::Error> {
        cadrs_social::codec::decode(bytes)
    }

    /// The least role that may do this.
    fn needs(&self) -> Role {
        match self {
            Op::SetMember { .. }
            | Op::SetWitness { .. }
            | Op::SetProfile(_)
            | Op::EditProfile(_)
            | Op::SetLocation { .. }
            | Op::SetMachine { .. }
            | Op::SetMaterial { .. }
            | Op::SetRates(_) => Role::Admin,
            _ => Role::Member,
        }
    }
}

/// What taking a request needs to know of the requests before it, kept up to date as operations
/// apply: a check then reads today's requests and the waiting ones, not the organization's whole
/// history, however long that has grown. (All of it follows from the operations; none of it is
/// written anywhere.)
#[derive(Clone, Debug, Default)]
struct Intake {
    /// The requests by when they arrived: (received, id).
    by_time: std::collections::BTreeSet<(u64, Id)>,
    /// The requests that wait for an answer.
    waiting: std::collections::BTreeSet<Id>,
    /// How many of an account's requests stand quoted (or further along): one makes it known.
    quoted: BTreeMap<AccountId, u32>,
    /// How many of the organization's client records name an account: one makes it known.
    kept: BTreeMap<AccountId, u32>,
    /// The last request's number, and the last issue's.
    numbered: u32,
    issues: u32,
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
    pub locations: BTreeMap<Id, Location>,
    pub machines: BTreeMap<Id, MachineCaps>,
    pub materials: BTreeMap<Id, Material>,
    /// Sheets on hand (reserved ones included), by material.
    pub stock: BTreeMap<Id, i64>,
    pub reservations: BTreeMap<Id, Reservation>,
    pub rates: Option<RateCard>,
    pub clients: BTreeMap<Id, Client>,
    pub requests: BTreeMap<Id, Request>,
    /// The accounts whose requests aren't taken ([`Op::SetBlocked`]).
    pub blocked: std::collections::BTreeSet<AccountId>,
    /// What the checks on a new request count, kept as operations apply.
    intake: Intake,
    pub jobs: BTreeMap<Id, Job>,
    pub parts: BTreeMap<Id, Part>,
    pub reviews: BTreeMap<Id, Review>,
    /// The members' channels: (who, what, when), oldest first.
    pub channels: BTreeMap<String, Vec<(AccountId, String, u64)>>,
    pub issues: BTreeMap<Id, Issue>,
    /// Operations a newer cadrs wrote, which this one left alone: the organization's screen
    /// says to update when there are any.
    pub newer_ops: usize,
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
        if !(profile::MIN_NAME..=profile::MAX_NAME).contains(&name.trim().chars().count()) {
            return conflict("a name is 2 to 60 characters");
        }
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
            return conflict(format!("a {role:?} {}", refusal::ROLE));
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
                // The last owner stays one: neither removed nor made less.
                let owners = self.members.values().filter(|r| **r == Role::Owner).count();
                if self.members.get(&account) == Some(&Role::Owner)
                    && new != Some(Role::Owner)
                    && owners == 1
                {
                    return conflict(refusal::LAST_OWNER);
                }
                match new {
                    Some(new) => {
                        self.members.insert(account, new);
                    }
                    None => {
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
                if let Some(why) = profile.problem() {
                    return conflict(why);
                }
                self.name = profile.name.trim().to_owned();
                self.profile = Profile {
                    name: self.name.clone(),
                    ..profile
                };
                Outcome::Applied
            }
            Op::EditProfile(edit) => {
                let mut profile = self.profile.clone();
                edit.apply(&mut profile);
                if let Some(why) = profile.problem() {
                    return conflict(why);
                }
                self.name = profile.name.trim().to_owned();
                self.profile = Profile {
                    name: self.name.clone(),
                    ..profile
                };
                Outcome::Applied
            }
            Op::SetLocation { id, location } => {
                match &location {
                    Some(location) => {
                        if let Some(why) = location.problem() {
                            return conflict(why);
                        }
                        if !self.locations.contains_key(&id)
                            && self.locations.len() >= profile::MAX_LOCATIONS
                        {
                            return conflict("an organization keeps at most 200 locations");
                        }
                    }
                    None => {
                        if self.machines.values().any(|m| m.location == Some(id)) {
                            return conflict(refusal::LOCATION_IN_USE);
                        }
                    }
                }
                set(&mut self.locations, id, location);
                Outcome::Applied
            }
            Op::SetMachine { id, machine } => {
                if let Some(machine) = &machine {
                    if machine.name.trim().is_empty() || longer(&machine.name, MAX_LABEL) {
                        return conflict("a machine's name is 1 to 120 characters");
                    }
                    if let Some(location) = machine.location
                        && !self.locations.contains_key(&location)
                    {
                        return conflict("no such location");
                    }
                } else if self
                    .jobs
                    .values()
                    .any(|j| j.machine == id && j.state != JobState::Done)
                {
                    return conflict(refusal::MACHINE_IN_USE);
                }
                set(&mut self.machines, id, machine);
                Outcome::Applied
            }
            Op::SetMaterial { id, material } => {
                if let Some(material) = &material
                    && (material.name.trim().is_empty() || longer(&material.name, MAX_LABEL))
                {
                    return conflict("a material's name is 1 to 120 characters");
                }
                set(&mut self.materials, id, material);
                Outcome::Applied
            }
            Op::Stock { material, sheets } => {
                if !self.materials.contains_key(&material) {
                    return conflict("no such material");
                }
                if self.available(material) + sheets < 0 {
                    return conflict(format!(
                        "only {} {}",
                        self.available(material),
                        refusal::NO_STOCK
                    ));
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
                        "only {free} {}; {} were asked for",
                        refusal::NO_STOCK,
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
                if let Some(client) = &client
                    && (client.name.trim().is_empty()
                        || longer(&client.name, MAX_LABEL)
                        || longer(&client.email, profile::MAX_CONTACT)
                        || longer(&client.phone, MAX_LABEL)
                        || longer(&client.notes, MAX_TEXT))
                {
                    return conflict("a client has a name, and its lines aren't endless");
                }
                // Which accounts the organization keeps as clients, counted as records change.
                let before = self.clients.get(&id).and_then(|c| c.account);
                let after = client.as_ref().and_then(|c| c.account);
                if before != after {
                    if let Some(account) = before
                        && let Some(n) = self.intake.kept.get_mut(&account)
                    {
                        *n = n.saturating_sub(1);
                    }
                    if let Some(account) = after {
                        *self.intake.kept.entry(account).or_default() += 1;
                    }
                }
                set(&mut self.clients, id, client);
                Outcome::Applied
            }
            Op::Request {
                id,
                client,
                parts,
                notes,
                snapshot,
                snapshot_len,
                deadline,
                card,
            } => {
                if self.requests.contains_key(&id) {
                    return conflict("that request exists");
                }
                if longer(&notes, MAX_TEXT)
                    || snapshot_len > MAX_SNAPSHOT
                    || parts.is_empty()
                    || parts.len() > MAX_REQUEST_PARTS
                    || parts.iter().any(|(part, _)| {
                        longer(&part.name, MAX_LABEL)
                            || part.material.as_ref().is_some_and(|m| longer(m, MAX_LABEL))
                    })
                {
                    return conflict("the request is too large, or asks for nothing");
                }
                // The card to answer with is the client's own, and says no more than a card
                // may: replies go to the devices it names.
                if let Some(card) = &card
                    && !(card_verifies(card)
                        && card.signer == client
                        && card.value.account == client
                        && card.value.valid())
                {
                    return conflict("the card with a request is the client's own");
                }
                if !self.profile.offers.manufacturing {
                    return conflict(refusal::NOT_A_MANUFACTURER);
                }
                if self.blocked.contains(&client) {
                    return conflict(refusal::BLOCKED);
                }
                // Counted over today's requests and the waiting ones only ([`Intake`]): what a
                // request costs to check doesn't grow with the organization's history.
                let since = time.saturating_sub(DAY);
                let from = if time >= DAY { since + 1 } else { 0 };
                let today = || {
                    self.intake
                        .by_time
                        .range((from, 0)..)
                        .filter_map(|(_, id)| self.requests.get(id))
                };
                let waiting = || {
                    self.intake
                        .waiting
                        .iter()
                        .filter_map(|id| self.requests.get(id))
                };
                // One client has their own share, of what a day takes and of what waits.
                let theirs = |r: &&Request| r.client == client;
                if today().filter(theirs).count() >= MAX_REQUESTS_A_DAY_PER_CLIENT {
                    return conflict(refusal::TOO_MANY_TODAY);
                }
                if waiting().filter(theirs).count() >= MAX_WAITING_PER_CLIENT {
                    return conflict(refusal::TOO_MANY_OF_YOURS);
                }
                // The rest is shared by the strangers: the people the organization doesn't
                // know yet. Someone it knows (kept among its clients, or quoted before) isn't
                // shut out by a list strangers filled, nor by a day's share strangers used up;
                // and what a blocked account sent counts against nobody.
                if !self.knows(&client) {
                    let stranger =
                        |r: &&Request| !self.blocked.contains(&r.client) && !self.knows(&r.client);
                    if today().filter(stranger).count() >= MAX_REQUESTS_A_DAY {
                        return conflict(refusal::TOO_MANY_TODAY);
                    }
                    if waiting().filter(stranger).count() >= MAX_WAITING
                        || waiting()
                            .filter(stranger)
                            .map(|r| r.snapshot_len)
                            .sum::<u64>()
                            + snapshot_len
                            > MAX_WAITING_BYTES
                    {
                        return conflict(refusal::TOO_MANY_WAITING);
                    }
                }
                self.intake.by_time.insert((time, id));
                self.intake.waiting.insert(id);
                self.intake.numbered += 1;
                let number = self.intake.numbered;
                self.requests.insert(
                    id,
                    Request {
                        number,
                        client,
                        parts,
                        notes,
                        snapshot,
                        snapshot_len,
                        deadline,
                        stage: Stage::Requested,
                        quote: None,
                        decline_reason: None,
                        declined_at: None,
                        receipt: None,
                        received: time,
                        chat: Vec::new(),
                        card,
                    },
                );
                Outcome::Applied
            }
            Op::SetBlocked { account, blocked } => {
                if blocked {
                    self.blocked.insert(account);
                } else {
                    self.blocked.remove(&account);
                }
                Outcome::Applied
            }
            Op::Quote { request, quote } => {
                self.advance(request, &[Stage::Requested, Stage::Quoted], |r| {
                    r.quote = Some(quote);
                    r.stage = Stage::Quoted;
                })
            }
            Op::Decline { request, reason } => {
                if longer(&reason, MAX_TEXT) {
                    return conflict("the reason is too long");
                }
                self.advance(request, &[Stage::Requested, Stage::Quoted], |r| {
                    r.decline_reason = Some(reason);
                    r.declined_at = Some(time);
                    r.stage = Stage::Declined;
                })
            }
            Op::Accept { request, quote, at } => {
                if let Some(standing) = self.requests.get(&request).and_then(|r| r.quote.as_ref()) {
                    if quote != 0 && standing.id != quote {
                        return conflict(refusal::QUOTE_REPLACED);
                    }
                    // When the client accepted, as far as it can be known: never later than
                    // this was written, and the client's own word for it only while a letter
                    // sent in time could still be on its way.
                    let at = if at == 0 { time } else { at.min(time) };
                    if at > standing.valid_until
                        || time > standing.valid_until.saturating_add(ACCEPT_GRACE)
                    {
                        return conflict(refusal::QUOTE_EXPIRED);
                    }
                }
                self.advance(request, &[Stage::Quoted], |r| {
                    r.stage = Stage::Accepted;
                })
            }
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
                if text.trim().is_empty() || longer(&text, MAX_TEXT) {
                    return conflict("a message has some words, and at most 4000 characters");
                }
                // Each side's share of the conversation; a client's is small until the request
                // is an order.
                let theirs = || {
                    r.chat
                        .iter()
                        .filter(|(client, _, _)| *client == from_client)
                };
                let said: usize = theirs().map(|(_, text, _)| text.len()).sum();
                let ordered = matches!(
                    r.stage,
                    Stage::Accepted | Stage::InProduction | Stage::Ready | Stage::Completed
                );
                let (lines, bytes) = if from_client && !ordered {
                    (MAX_NOTES_BEFORE_ORDER, MAX_NOTE_BYTES_BEFORE_ORDER)
                } else {
                    (MAX_NOTES, MAX_NOTE_BYTES)
                };
                if theirs().count() >= lines || said + text.len() > bytes {
                    return conflict("that many messages about this request are enough");
                }
                r.chat.push((from_client, text, time));
                Outcome::Applied
            }
            Op::Say { channel, text } => {
                let channel = channel.trim().trim_start_matches('#').to_lowercase();
                if channel.is_empty()
                    || text.trim().is_empty()
                    || longer(&channel, MAX_LABEL)
                    || longer(&text, MAX_TEXT)
                {
                    return conflict("a message needs a channel and some words (at most 4000)");
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
                if review.by != r.client
                    || !(1..=5).contains(&review.stars)
                    || longer(&review.text, MAX_TEXT)
                {
                    return conflict("a review is the client's, of 1 to 5 stars");
                }
                if self.reviews.values().any(|v| v.request == review.request) {
                    return conflict("that order was reviewed already");
                }
                self.reviews.insert(id, review);
                Outcome::Applied
            }
            Op::OpenIssue {
                id,
                title,
                details,
                about,
                owner,
            } => {
                if self.issues.contains_key(&id) {
                    return conflict("that issue exists");
                }
                let title = title.trim().to_owned();
                if title.is_empty() || longer(&title, quality::MAX_TITLE) {
                    return conflict("an issue's title is 1 to 120 characters");
                }
                if longer(&details, MAX_TEXT) {
                    return conflict("the details are too long");
                }
                if let Some(why) = self.issue_refs(about, owner) {
                    return conflict(why);
                }
                self.intake.issues += 1;
                let number = self.intake.issues;
                self.issues.insert(
                    id,
                    Issue {
                        number,
                        title,
                        details,
                        about,
                        status: IssueStatus::Open,
                        owner,
                        disposition: None,
                        rationale: String::new(),
                        opened_by: author,
                        opened: time,
                        closed: None,
                    },
                );
                Outcome::Applied
            }
            Op::UpdateIssue {
                id,
                status,
                owner,
                disposition,
                rationale,
            } => {
                let Some(before) = self.issues.get(&id).map(|i| i.owner) else {
                    return conflict("no such issue");
                };
                if longer(&rationale, MAX_TEXT) {
                    return conflict("the reasoning is too long");
                }
                if status == IssueStatus::Closed
                    && (disposition.is_none() || rationale.trim().is_empty())
                {
                    return conflict(refusal::ISSUE_CLOSE);
                }
                // What it is about was checked when it was opened, and may be gone since (a
                // machine sold, a part dropped): the issue is still there to be closed. A new
                // owner is a member; the one it had may have left.
                if owner != before
                    && let Some(why) = self.issue_refs(None, owner)
                {
                    return conflict(why);
                }
                let issue = self.issues.get_mut(&id).expect("looked up above");
                issue.closed = match (status, issue.closed) {
                    (IssueStatus::Closed, None) => Some(time),
                    (IssueStatus::Closed, closed) => closed,
                    _ => None,
                };
                issue.status = status;
                issue.owner = owner;
                issue.disposition = disposition;
                issue.rationale = rationale;
                Outcome::Applied
            }
        }
    }

    /// Whether the organization knows this account as a client: it keeps it among its clients,
    /// or has answered a request of its with a quote (which the client may have accepted, and
    /// the order been made and delivered). Only the organization's own doing makes someone
    /// known: asking, however often, doesn't.
    pub fn knows(&self, account: &AccountId) -> bool {
        let counted = |of: &BTreeMap<AccountId, u32>| of.get(account).is_some_and(|n| *n > 0);
        counted(&self.intake.kept) || counted(&self.intake.quoted)
    }

    /// Why an issue can't be about `about` or looked after by `owner`, if it can't.
    fn issue_refs(
        &self,
        about: Option<IssueAbout>,
        owner: Option<AccountId>,
    ) -> Option<&'static str> {
        let known = match about {
            None => true,
            Some(IssueAbout::Request(id)) => self.requests.contains_key(&id),
            Some(IssueAbout::Job(id)) => self.jobs.contains_key(&id),
            Some(IssueAbout::Part(id)) => self.parts.contains_key(&id),
            Some(IssueAbout::Machine(id)) => self.machines.contains_key(&id),
            Some(IssueAbout::Location(id)) => self.locations.contains_key(&id),
        };
        if !known {
            return Some("what the issue is about isn't in this organization");
        }
        if owner.is_some_and(|o| !self.members.contains_key(&o)) {
            return Some("an issue's owner is a member");
        }
        None
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
        // What the request checks count ([`Intake`]) follows every change of stage.
        let stands = |r: &Request| {
            r.quote.is_some() && !matches!(r.stage, Stage::Requested | Stage::Declined)
        };
        let (client, stood) = (r.client, stands(r));
        change(r);
        let (waits, stands) = (r.stage == Stage::Requested, stands(r));
        if !waits {
            self.intake.waiting.remove(&request);
        }
        if stands != stood {
            let quoted = self.intake.quoted.entry(client).or_default();
            *quoted = if stands {
                *quoted + 1
            } else {
                quoted.saturating_sub(1)
            };
        }
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
        // First of all, and for every operation there is or will be: no entry is larger than
        // this. (A session never writes an operation the reducer refuses, so one that is too
        // large doesn't reach the log.)
        if data.len() > MAX_OP {
            return conflict(refusal::TOO_LARGE);
        }
        let op = match Op::decode(data) {
            Ok(op) => op,
            Err(e) if e.is_newer() => {
                // Left alone, and said so: the rest of the document still works.
                self.newer_ops += 1;
                return conflict("written by a newer cadrs: update to see it");
            }
            Err(_) => return conflict("not an operation"),
        };
        if matches!(op, Op::Request { .. }) && data.len() > MAX_REQUEST {
            return conflict(refusal::TOO_LARGE);
        }
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
