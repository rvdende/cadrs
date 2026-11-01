//! Public documents: documents anyone using cadrs can find, view and fork, with no server.
//!
//! - Every cadrs that is online (and takes part: the user's choice, [`PublicEnabled`]) is in one
//!   well-known room ([`TOPIC`]), joined through the peers it knows ([`PublicPeers`]: seed
//!   peers, contacts, peers met there before) and those the rendezvous names. Offline, nothing
//!   reaches the room or the rendezvous.
//! - An owner **publishes** a document as it is: its [`Snapshot`], packed, is a *revision*
//!   kept by its hash, and a [`Listing`] signed with the owner's key names it. Publishing
//!   again makes a new revision; a signed [`Withdrawal`] makes the document private again.
//!   Edits made after publishing stay private until it is published again.
//! - Everyone keeps the listings, withdrawals and stars they hear ([`Directory`]) and hands
//!   them to whoever asks ([`Ask::Catalog`]), so the list outlives its owners going offline: a
//!   newcomer asks the first peers it meets, and a peer alone in the room asks the peers it
//!   knows now and then. What a peer signs goes to the room and straight to its seeds
//!   ([`Ask::Tell`]), which say it in their room in turn: gossip alone may not join them.
//! - A revision comes from its owner or from anyone who fetched it (they keep it and serve it
//!   while online), checked against the signed hash.
//! - The room carries only small statements (gossip drops what is over 4 kB): a listing names
//!   its picture by hash, and the picture comes with a catalog or is asked for.
//! - A **star** is a signed statement by one cadrs ID; a document's count is the IDs whose
//!   latest statement stars it, as far as this peer has heard. Nothing stops someone making
//!   new IDs to star with.
//! - A seed peer (`cadrs --public-seed`: [`KeepAll`]) stays in the room and also fetches and
//!   keeps every revision it hears of, so documents stay reachable while their owners are not.
//! - The rendezvous ([`RENDEZVOUS`], `tools/peers`): while taking part, a peer checks in every
//!   few minutes, signed with its key, and hears of up to 20 others online, to join the room
//!   through. It keeps only cadrs IDs; without it, peers start from the seeds and those they
//!   know.
//!
//! [`Snapshot`]: cadrs_core::collab::Snapshot

use std::collections::{BTreeMap, HashMap, HashSet};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::Real;
use bevy_iroh::iroh::{
    Endpoint, EndpointAddr, EndpointId, PublicKey, SecretKey, Signature,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use bevy_iroh::message::MessageAppExt;
use bevy_iroh::prelude::{Members, NetSender, Peer, PeerJoined, Received, Room, RoomStatus};
use bevy_iroh::{Iroh, IrohTask, RoomTicket, TopicId};
use cadrs_core::DocumentId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Versioned, so a peer with another framing fails to connect rather than misreads.
pub const ALPN: &[u8] = b"cadrs/public/1";

/// The public room: the same for every cadrs.
pub const TOPIC: [u8; 32] = *b"cadrs public documents, room 1\0\0";

/// The room's name, before anyone has said anything.
const ROOM_NAME: &str = "Public documents";

/// Seed peers every cadrs starts from (their cadrs IDs), besides the user's own: cadrs's own
/// seed (`tools/seed`).
pub const DEFAULT_SEEDS: &[&str] =
    &["a560c0cfdeaac54cde945476f851486bbdf584adc280c4120616b06a6b10ba6f"];

/// How often an owner says again what it publishes, for peers that missed it.
const ANNOUNCE_EVERY: f64 = 600.0;
/// How often a peer asks someone in the room for their catalog again.
const RESYNC_EVERY: f64 = 1800.0;
/// How often what is missing is asked for again: pictures (and, on a seed, revisions) of the
/// listings here, from their owners.
const SWEEP_EVERY: f64 = 60.0;
/// How many of the peers met after joining are asked for their catalog.
const SYNC_FIRST: usize = 3;
/// How long after a failed join the room is tried again.
const RETRY_AFTER: f64 = 30.0;
/// How often a peer alone in the room asks the peers it knows for their catalogs (a seed that
/// was offline when it joined may be back). Gossip doesn't let it join them later: a peer that
/// leaves the room and joins again is often not heard any more.
const ALONE_ASK_EVERY: f64 = 30.0;
/// The most a peer may tell another at once (its statements, and a picture).
const MAX_TELL: usize = 1 << 20;
/// How long after leaving the room (taking part switched off) it is joined again: gossip loses
/// a peer that leaves and joins at once (the others take its goodbye after its new hello).
const REJOIN_GAP: f64 = 5.0;
/// The soonest an owner says again what it publishes because someone new arrived.
const ANNOUNCE_FOR_NEWCOMERS: f64 = 60.0;
/// How long after a change the directory is written out.
const SAVE_AFTER: f64 = 5.0;

/// Where peers check in and hear of others online (`tools/peers`, a Cloudflare Worker).
/// `CADRS_RENDEZVOUS` names another (a local `wrangler dev`), or `0` none.
pub const RENDEZVOUS: &str = "https://peers.cadrs.net";
/// How often a peer in the room checks in (the rendezvous forgets one after 15 minutes).
const CHECK_IN_EVERY: f64 = 600.0;
/// How long the first check-in may hold up joining the room.
const CHECK_IN_WAIT: f64 = 5.0;
/// Signed with a check-in, before the cadrs ID and the time.
const CHECK_IN_CONTEXT: &str = "cadrs rendezvous 1\0";

/// The longest name and description a listing may have, in bytes: a listing travels the room
/// whole, and gossip carries [`ROOM_MESSAGE`] at most.
const MAX_NAME: usize = 200;
const MAX_DESCRIPTION: usize = 1000;
/// The most a statement may take in the room, its envelope left room for (iroh-gossip's
/// default message is 4096 bytes, and bevy_iroh keeps it: a bigger one is dropped).
pub const ROOM_MESSAGE: usize = 3000;
/// The biggest picture of a listing (the documents page's thumbnail is about 10 kB). Pictures
/// travel by their hash, outside the room.
pub const MAX_PICTURE: usize = 64 * 1024;
/// The biggest revision there is: as big as a document may travel to someone it is shared with.
pub const MAX_REVISION: usize = 1 << 30;
/// The biggest catalog a peer may answer with.
const MAX_CATALOG: usize = 256 << 20;
/// A statement this far in the future (by its own clock) is refused: someone's clock is wrong,
/// or someone wants theirs to win forever.
const MAX_AHEAD: i64 = 24 * 3600;
/// A listing nobody has mentioned for this long is forgotten (its owner stopped saying it,
/// and so did everyone else); so are withdrawals and stars that old with nothing to apply to.
pub const FORGET_AFTER: i64 = 60 * 24 * 3600;
/// How many peers met in the room are remembered, to start from next time.
const REMEMBER_PEERS: usize = 32;
/// How long reaching a peer may take (as for documents shared by link).
const REACH_WITHIN: Duration = Duration::from_secs(15);

const DIRECTORY_FILE: &str = "directory.bin";
/// The owners' names ([`Profile`]), in a file of their own so the directory's stays as it was.
const PROFILES_FILE: &str = "profiles.bin";
const REVISIONS_DIR: &str = "revisions";

/// Seconds since the Unix epoch, by this machine's clock.
pub fn unix_now() -> i64 {
    cadrs_fs::SystemTime::now()
        .duration_since(cadrs_fs::SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

// ---------------------------------------------------------------------------------------------
// Statements

/// Which document: its owner's cadrs ID and its id. Two owners' documents never mix, whatever
/// ids they claim.
pub type Key = (String, DocumentId);

/// What an owner publishes about a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub document: DocumentId,
    /// The owner's cadrs ID: the key that signs it.
    pub owner: String,
    pub owner_name: String,
    pub name: String,
    pub description: String,
    /// 1 for the first time it is published, one more each time after.
    pub revision: u32,
    /// When this revision was published.
    pub published: i64,
    /// The revision's hash (BLAKE3 of its bytes), and its size.
    pub hash: [u8; 32],
    pub size: u64,
    /// The hash (BLAKE3) of the documents page's picture of it (a PNG), if any: the picture
    /// itself comes with a catalog, or is asked for ([`Directory::picture`]).
    pub picture: Option<[u8; 32]>,
    /// The public document it was forked from.
    pub forked_from: Option<Origin>,
}

impl Listing {
    pub fn key(&self) -> Key {
        (self.owner.clone(), self.document)
    }
}

/// The public document a fork was made from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub document: DocumentId,
    pub owner: String,
    pub owner_name: String,
    pub name: String,
}

/// An owner making a document private again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withdrawal {
    pub document: DocumentId,
    pub owner: String,
    pub time: i64,
    /// The revision it was at, so publishing again counts on from it.
    pub revision: u32,
}

/// Someone starring a document, or taking their star back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Star {
    pub document: DocumentId,
    /// The document's owner.
    pub owner: String,
    /// Who stars it: the key that signs it.
    pub by: String,
    pub starred: bool,
    pub time: i64,
}

/// Something one cadrs ID says, signed with its key.
pub trait Statement: Serialize + DeserializeOwned {
    /// Signed with the statement, so one kind of statement can't pass for another.
    const CONTEXT: &'static [u8];
    /// Whose key must have signed it.
    fn signer(&self) -> &str;
}

impl Statement for Listing {
    const CONTEXT: &'static [u8] = b"cadrs public listing 1\0";
    fn signer(&self) -> &str {
        &self.owner
    }
}

impl Statement for Withdrawal {
    const CONTEXT: &'static [u8] = b"cadrs public withdrawal 1\0";
    fn signer(&self) -> &str {
        &self.owner
    }
}

impl Statement for Star {
    const CONTEXT: &'static [u8] = b"cadrs public star 1\0";
    fn signer(&self) -> &str {
        &self.by
    }
}

/// What someone calls themselves now: shown for their documents, whatever name each listing was
/// published under, so a rename reaches everyone without publishing again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Their cadrs ID: the key that signs it.
    pub owner: String,
    pub name: String,
    pub time: i64,
}

impl Statement for Profile {
    const CONTEXT: &'static [u8] = b"cadrs public profile 1\0";
    fn signer(&self) -> &str {
        &self.owner
    }
}

/// A statement as it travels and is kept: its bytes and their signature, so anyone can pass it
/// on and anyone can check it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Signed<T> {
    body: Vec<u8>,
    signature: Vec<u8>,
    #[serde(skip)]
    of: PhantomData<fn() -> T>,
}

impl<T> Clone for Signed<T> {
    fn clone(&self) -> Self {
        Self {
            body: self.body.clone(),
            signature: self.signature.clone(),
            of: PhantomData,
        }
    }
}

impl<T> PartialEq for Signed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.body == other.body && self.signature == other.signature
    }
}

impl<T: Statement> Signed<T> {
    /// `statement`, signed with `key` (which must be its signer's).
    pub fn new(statement: &T, key: &SecretKey) -> Result<Self, String> {
        if statement.signer() != key.public().to_string() {
            return Err("Signed by someone else's key".into());
        }
        let body = postcard::to_stdvec(statement).map_err(|e| e.to_string())?;
        let signature = key.sign(&[T::CONTEXT, &body].concat()).to_bytes().to_vec();
        Ok(Self {
            body,
            signature,
            of: PhantomData,
        })
    }

    /// The statement, if its signer's key signed it.
    pub fn open(&self) -> Result<T, String> {
        let statement: T =
            postcard::from_bytes(&self.body).map_err(|_| "Not a statement".to_string())?;
        let key: PublicKey = statement
            .signer()
            .parse()
            .map_err(|_| "Not a cadrs ID".to_string())?;
        let signature: &[u8; Signature::LENGTH] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| "Not a signature".to_string())?;
        key.verify(
            &[T::CONTEXT, &self.body].concat(),
            &Signature::from_bytes(signature),
        )
        .map_err(|_| "The signature does not match".to_string())?;
        Ok(statement)
    }
}

/// The checks every listing passes besides its signature.
fn check_listing(l: &Listing, now: i64) -> Result<(), String> {
    if l.name.len() > MAX_NAME || l.description.len() > MAX_DESCRIPTION {
        return Err("The name or description is too long".into());
    }
    if l.owner_name.len() > MAX_NAME {
        return Err("The owner's name is too long".into());
    }
    if l.size as usize > MAX_REVISION {
        return Err("The document is too big".into());
    }
    if l.published > now + MAX_AHEAD {
        return Err("Published in the future".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The directory

/// A listing as kept: what it says, as signed, and when it was last heard of here.
#[derive(Debug, Clone)]
pub struct Held {
    pub listing: Listing,
    signed: Signed<Listing>,
    pub heard: i64,
}

/// Someone's latest word on a document's star.
#[derive(Debug, Clone)]
struct Starring {
    starred: bool,
    time: i64,
    signed: Signed<Star>,
}

/// Everything public this peer has heard: the listings, the withdrawals that outrank older
/// listings, and the stars.
#[derive(Debug, Clone, Default)]
pub struct Directory {
    listings: BTreeMap<Key, Held>,
    withdrawn: BTreeMap<Key, (Withdrawal, Signed<Withdrawal>)>,
    stars: BTreeMap<Key, BTreeMap<String, Starring>>,
    /// The owners' names, by their cadrs ID.
    profiles: BTreeMap<String, (Profile, Signed<Profile>)>,
    /// The listings' pictures, by their hash.
    pictures: HashMap<[u8; 32], Vec<u8>>,
}

/// A public document as the documents page lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicDoc {
    pub listing: Listing,
    /// Its picture, once here.
    pub picture: Option<Vec<u8>>,
    pub stars: usize,
    /// Starred by the cadrs ID asked about.
    pub starred: bool,
}

/// Everything a peer hands a newcomer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub listings: Vec<Signed<Listing>>,
    pub withdrawals: Vec<Signed<Withdrawal>>,
    pub stars: Vec<Signed<Star>>,
    /// The listings' pictures (each checked against a listing's hash).
    pub pictures: Vec<Vec<u8>>,
}

/// What [`Directory::merge`] took from a catalog.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Merged {
    /// Listings new here, or newer revisions.
    pub listings: Vec<Key>,
    /// Anything else that changed what the page shows.
    pub other: bool,
}

impl Merged {
    pub fn changed(&self) -> bool {
        !self.listings.is_empty() || self.other
    }
}

impl Directory {
    /// Takes a listing: true if it is new here or a newer revision. Hearing the one held
    /// again only notes that it was heard.
    pub fn add_listing(&mut self, signed: &Signed<Listing>, now: i64) -> Result<bool, String> {
        let listing = signed.open()?;
        check_listing(&listing, now)?;
        let key = listing.key();
        if self
            .withdrawn
            .get(&key)
            .is_some_and(|(w, _)| w.time >= listing.published)
        {
            return Ok(false);
        }
        if let Some(held) = self.listings.get_mut(&key) {
            // The later published wins (its owner publishes each later than the last).
            let order = |l: &Listing| (l.published, l.revision);
            if order(&held.listing) >= order(&listing) {
                held.heard = held.heard.max(now);
                return Ok(false);
            }
        }
        self.listings.insert(
            key,
            Held {
                listing,
                signed: signed.clone(),
                heard: now,
            },
        );
        Ok(true)
    }

    /// Takes a withdrawal: true if it made a listing private.
    pub fn add_withdrawal(
        &mut self,
        signed: &Signed<Withdrawal>,
        now: i64,
    ) -> Result<bool, String> {
        let w = signed.open()?;
        if w.time > now + MAX_AHEAD {
            return Err("Withdrawn in the future".into());
        }
        let key = (w.owner.clone(), w.document);
        if self
            .withdrawn
            .get(&key)
            .is_some_and(|(x, _)| x.time >= w.time)
        {
            return Ok(false);
        }
        let time = w.time;
        self.withdrawn.insert(key.clone(), (w, signed.clone()));
        let hides = self
            .listings
            .get(&key)
            .is_some_and(|h| h.listing.published <= time);
        if hides {
            self.listings.remove(&key);
        }
        Ok(hides)
    }

    /// Takes a star (or a star taken back): true if it is someone's latest word on it.
    pub fn add_star(&mut self, signed: &Signed<Star>, now: i64) -> Result<bool, String> {
        let s = signed.open()?;
        if s.time > now + MAX_AHEAD {
            return Err("Starred in the future".into());
        }
        let by = self.stars.entry((s.owner.clone(), s.document)).or_default();
        if by.get(&s.by).is_some_and(|x| x.time >= s.time) {
            return Ok(false);
        }
        by.insert(
            s.by.clone(),
            Starring {
                starred: s.starred,
                time: s.time,
                signed: signed.clone(),
            },
        );
        Ok(true)
    }

    /// Takes all of a catalog that checks out; the rest is skipped.
    pub fn merge(&mut self, catalog: &Catalog, now: i64) -> Merged {
        let mut merged = Merged::default();
        // Withdrawals first, so a listing they outrank is never taken.
        for w in &catalog.withdrawals {
            merged.other |= self.add_withdrawal(w, now).unwrap_or(false);
        }
        for l in &catalog.listings {
            if self.add_listing(l, now).unwrap_or(false)
                && let Ok(l) = l.open()
            {
                merged.listings.push(l.key());
            }
        }
        for s in &catalog.stars {
            merged.other |= self.add_star(s, now).unwrap_or(false);
        }
        for p in &catalog.pictures {
            merged.other |= self.add_picture(p.clone());
        }
        merged
    }

    /// Keeps a listing's picture: true if a listing names it and it wasn't here.
    pub fn add_picture(&mut self, bytes: Vec<u8>) -> bool {
        if bytes.len() > MAX_PICTURE {
            return false;
        }
        let hash = *blake3::hash(&bytes).as_bytes();
        if self.pictures.contains_key(&hash) || !self.pictured().contains(&hash) {
            return false;
        }
        self.pictures.insert(hash, bytes);
        true
    }

    /// A picture kept here, by its hash.
    pub fn picture(&self, hash: &[u8; 32]) -> Option<&Vec<u8>> {
        self.pictures.get(hash)
    }

    /// The pictures the listings name.
    fn pictured(&self) -> HashSet<[u8; 32]> {
        self.listings
            .values()
            .filter_map(|h| h.listing.picture)
            .collect()
    }

    /// The listings whose picture isn't here yet.
    pub fn missing_pictures(&self) -> Vec<Listing> {
        self.listings
            .values()
            .filter(|h| {
                h.listing
                    .picture
                    .is_some_and(|p| !self.pictures.contains_key(&p))
            })
            .map(|h| h.listing.clone())
            .collect()
    }

    /// Everything kept, to hand on.
    pub fn catalog(&self) -> Catalog {
        Catalog {
            listings: self.listings.values().map(|h| h.signed.clone()).collect(),
            withdrawals: self.withdrawn.values().map(|(_, w)| w.clone()).collect(),
            stars: self
                .stars
                .values()
                .flat_map(|by| by.values().map(|s| s.signed.clone()))
                .collect(),
            pictures: self.pictures.values().cloned().collect(),
        }
    }

    /// What `owner` signed: their listings, withdrawals and stars, to say again.
    pub fn signed_by(&self, owner: &str) -> Catalog {
        Catalog {
            listings: self
                .listings
                .iter()
                .filter(|((o, _), _)| o == owner)
                .map(|(_, h)| h.signed.clone())
                .collect(),
            withdrawals: self
                .withdrawn
                .iter()
                .filter(|((o, _), _)| o == owner)
                .map(|(_, (_, w))| w.clone())
                .collect(),
            stars: self
                .stars
                .values()
                .filter_map(|by| by.get(owner).map(|s| s.signed.clone()))
                .collect(),
            pictures: Vec::new(),
        }
    }

    pub fn get(&self, key: &Key) -> Option<&Held> {
        self.listings.get(key)
    }

    pub fn len(&self) -> usize {
        self.listings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.listings.is_empty()
    }

    /// How many star the document.
    pub fn stars(&self, key: &Key) -> usize {
        self.stars
            .get(key)
            .map_or(0, |by| by.values().filter(|s| s.starred).count())
    }

    /// Whether `who` stars it.
    pub fn starred_by(&self, key: &Key, who: &str) -> bool {
        self.stars
            .get(key)
            .and_then(|by| by.get(who))
            .is_some_and(|s| s.starred)
    }

    /// The time of `who`'s latest word on the document's star (a new one must be later).
    pub fn star_time(&self, key: &Key, who: &str) -> Option<i64> {
        self.stars.get(key)?.get(who).map(|s| s.time)
    }

    /// The latest word on a document: when it was last published or withdrawn, and the
    /// revision it was at (a new revision is published later and counts on from it).
    pub fn latest(&self, key: &Key) -> Option<(i64, u32)> {
        let listed = self
            .listings
            .get(key)
            .map(|h| (h.listing.published, h.listing.revision));
        let withdrawn = self.withdrawn.get(key).map(|(w, _)| (w.time, w.revision));
        listed.max(withdrawn)
    }

    /// Takes someone's name: true if it is their latest.
    pub fn add_profile(&mut self, signed: &Signed<Profile>, now: i64) -> Result<bool, String> {
        let p = signed.open()?;
        if p.name.len() > MAX_NAME {
            return Err("The name is too long".into());
        }
        if p.time > now + MAX_AHEAD {
            return Err("Named in the future".into());
        }
        if self
            .profiles
            .get(&p.owner)
            .is_some_and(|(x, _)| x.time >= p.time)
        {
            return Ok(false);
        }
        self.profiles.insert(p.owner.clone(), (p, signed.clone()));
        Ok(true)
    }

    /// What `owner` calls themselves now, if they said.
    pub fn profile(&self, owner: &str) -> Option<&Profile> {
        self.profiles.get(owner).map(|(p, _)| p)
    }

    /// Every name kept, to hand on (or only `owner`'s).
    pub fn profiles(&self, owner: Option<&str>) -> Vec<Signed<Profile>> {
        self.profiles
            .iter()
            .filter(|(o, _)| owner.is_none_or(|w| w == o.as_str()))
            .map(|(_, (_, s))| s.clone())
            .collect()
    }

    /// The public documents, with their stars as `who` sees them, under their owners' names as
    /// they are now.
    pub fn docs(&self, who: &str) -> Vec<PublicDoc> {
        self.listings
            .iter()
            .map(|(key, h)| PublicDoc {
                listing: match self.profile(&h.listing.owner) {
                    Some(p) => Listing {
                        owner_name: p.name.clone(),
                        ..h.listing.clone()
                    },
                    None => h.listing.clone(),
                },
                picture: h
                    .listing
                    .picture
                    .and_then(|p| self.pictures.get(&p).cloned()),
                stars: self.stars(key),
                starred: self.starred_by(key, who),
            })
            .collect()
    }

    /// The hashes of the revisions listed now.
    pub fn hashes(&self) -> HashSet<[u8; 32]> {
        self.listings.values().map(|h| h.listing.hash).collect()
    }

    /// Forgets what nobody has mentioned for [`FORGET_AFTER`]: listings not heard of, and
    /// withdrawals and stars that old with nothing listed to apply to.
    pub fn forget_old(&mut self, now: i64) -> bool {
        let before = (self.listings.len(), self.withdrawn.len(), self.stars.len());
        let old = now - FORGET_AFTER;
        self.listings.retain(|_, h| h.heard >= old);
        let listed: HashSet<Key> = self.listings.keys().cloned().collect();
        self.withdrawn
            .retain(|k, (w, _)| w.time >= old || listed.contains(k));
        let owners: HashSet<&String> = self.listings.keys().map(|(o, _)| o).collect();
        let names_before = self.profiles.len();
        let kept: BTreeMap<String, (Profile, Signed<Profile>)> = std::mem::take(&mut self.profiles)
            .into_iter()
            .filter(|(o, (p, _))| p.time >= old || owners.contains(o))
            .collect();
        self.profiles = kept;
        let names_forgotten = self.profiles.len() != names_before;
        let pictured = self.pictured();
        self.pictures.retain(|h, _| pictured.contains(h));
        self.stars.retain(|k, by| {
            if !listed.contains(k) {
                by.retain(|_, s| s.time >= old);
            }
            !by.is_empty()
        });
        names_forgotten || before != (self.listings.len(), self.withdrawn.len(), self.stars.len())
    }
}

/// The directory's file: what is kept, when each listing was last heard of, and the peers
/// met in the room.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    listings: Vec<(Signed<Listing>, i64)>,
    withdrawals: Vec<Signed<Withdrawal>>,
    stars: Vec<Signed<Star>>,
    pictures: Vec<Vec<u8>>,
    peers: Vec<[u8; 32]>,
}

// ---------------------------------------------------------------------------------------------
// Shared state

/// The directory and the revisions kept, shared by the app's systems and the protocol's
/// handler (which answers on the network's own threads).
#[derive(Resource, Clone, Default)]
pub struct Shared(Arc<Mutex<State>>);

#[derive(Default)]
pub struct State {
    pub directory: Directory,
    /// Where the directory and the revisions are kept; `None` keeps them in memory.
    dir: Option<PathBuf>,
    /// Revisions held in memory (when there is no folder).
    memory: HashMap<[u8; 32], Vec<u8>>,
    /// Peers met in the room, the most recent first.
    peers: Vec<[u8; 32]>,
    /// What peers told this one that was new here: said in the room in turn.
    told: Vec<(EndpointId, Catalog, Merged)>,
    /// Names peers told this one that were new here: said in the room in turn.
    told_profiles: Vec<Signed<Profile>>,
    /// Changed since it was last written out.
    dirty: bool,
    /// One more on every change, for the page to notice.
    generation: u64,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("public::Shared")
    }
}

impl Shared {
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keeps the directory and revisions in `dir`, reading what is there. Revisions no listing
    /// names any more are deleted.
    pub fn open(&self, dir: PathBuf, now: i64) {
        let saved: Saved = cadrs_fs::read(dir.join(DIRECTORY_FILE))
            .ok()
            .and_then(|b| postcard::from_bytes(&b).ok())
            .unwrap_or_default();
        let mut directory = Directory::default();
        for w in &saved.withdrawals {
            let _ = directory.add_withdrawal(w, now);
        }
        for (l, heard) in &saved.listings {
            let _ = directory.add_listing(l, *heard);
        }
        for s in &saved.stars {
            let _ = directory.add_star(s, now);
        }
        for p in saved.pictures {
            directory.add_picture(p);
        }
        let profiles: Vec<Signed<Profile>> = cadrs_fs::read(dir.join(PROFILES_FILE))
            .ok()
            .and_then(|b| postcard::from_bytes(&b).ok())
            .unwrap_or_default();
        for p in &profiles {
            let _ = directory.add_profile(p, now);
        }
        directory.forget_old(now);
        let mut state = self.lock();
        // Anything heard before it was opened (it is opened at startup, so rarely anything).
        let heard = state.directory.catalog();
        directory.merge(&heard, now);
        for p in state.directory.profiles(None) {
            let _ = directory.add_profile(&p, now);
        }
        state.directory = directory;
        state.peers = saved.peers;
        state.dir = Some(dir);
        state.prune_revisions();
        state.changed();
    }

    /// One more on every change.
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }
}

impl State {
    /// Something changed: the page shows it, and it is written out soon.
    pub fn changed(&mut self) {
        self.dirty = true;
        self.generation += 1;
    }

    fn revisions_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(REVISIONS_DIR))
    }

    fn revision_path(dir: &Path, hash: &[u8; 32]) -> PathBuf {
        dir.join(format!("{}.rev", hex(hash)))
    }

    /// Keeps a revision (checked against its hash) to serve it.
    pub fn keep_revision(&mut self, bytes: Vec<u8>) -> [u8; 32] {
        let hash = *blake3::hash(&bytes).as_bytes();
        match self.revisions_dir() {
            Some(dir) => {
                let path = Self::revision_path(&dir, &hash);
                if !cadrs_fs::exists(&path) {
                    let tmp = path.with_extension("tmp");
                    let written = cadrs_fs::create_dir_all(&dir)
                        .and_then(|()| cadrs_fs::write(&tmp, &bytes))
                        .and_then(|()| cadrs_fs::rename(&tmp, &path));
                    if let Err(e) = written {
                        warn!("cannot keep a public revision: {e}");
                    }
                }
            }
            None => {
                self.memory.insert(hash, bytes);
            }
        }
        hash
    }

    pub fn has_revision(&self, hash: &[u8; 32]) -> bool {
        match self.revisions_dir() {
            Some(dir) => cadrs_fs::exists(Self::revision_path(&dir, hash)),
            None => self.memory.contains_key(hash),
        }
    }

    /// A kept revision's bytes.
    pub fn revision(&self, hash: &[u8; 32]) -> Option<Vec<u8>> {
        match self.revisions_dir() {
            Some(dir) => cadrs_fs::read(Self::revision_path(&dir, hash)).ok(),
            None => self.memory.get(hash).cloned(),
        }
    }

    /// Deletes the revisions no listing names.
    fn prune_revisions(&mut self) {
        let keep = self.directory.hashes();
        self.memory.retain(|h, _| keep.contains(h));
        let Some(dir) = self.revisions_dir() else {
            return;
        };
        let Ok(entries) = cadrs_fs::read_dir(&dir) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let kept = name
                .strip_suffix(".rev")
                .and_then(unhex)
                .is_some_and(|h| keep.contains(&h));
            if !kept {
                let _ = cadrs_fs::remove_file(e.path());
            }
        }
    }

    /// Notes a peer met in the room (to start from next time).
    fn remember(&mut self, id: &EndpointId) {
        let bytes = *id.as_bytes();
        if self.peers.first() == Some(&bytes) {
            return;
        }
        self.peers.retain(|p| *p != bytes);
        self.peers.insert(0, bytes);
        self.peers.truncate(REMEMBER_PEERS);
        self.dirty = true;
    }

    /// The peers met before, the most recent first.
    pub fn remembered(&self) -> Vec<EndpointId> {
        self.peers
            .iter()
            .filter_map(|b| EndpointId::from_bytes(b).ok())
            .collect()
    }

    /// Writes the directory out, if it changed.
    fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let d = &self.directory;
        let saved = Saved {
            listings: d
                .listings
                .values()
                .map(|h| (h.signed.clone(), h.heard))
                .collect(),
            withdrawals: d.withdrawn.values().map(|(_, w)| w.clone()).collect(),
            stars: d
                .stars
                .values()
                .flat_map(|by| by.values().map(|s| s.signed.clone()))
                .collect(),
            pictures: d.pictures.values().cloned().collect(),
            peers: self.peers.clone(),
        };
        let write = |file: &str, bytes: Result<Vec<u8>, postcard::Error>| {
            let bytes = bytes.map_err(std::io::Error::other)?;
            cadrs_fs::create_dir_all(&dir)?;
            let path = dir.join(file);
            let tmp = path.with_extension("tmp");
            cadrs_fs::write(&tmp, bytes)?;
            cadrs_fs::rename(&tmp, path)
        };
        let written = write(DIRECTORY_FILE, postcard::to_stdvec(&saved)).and_then(|()| {
            write(
                PROFILES_FILE,
                postcard::to_stdvec(&self.directory.profiles(None)),
            )
        });
        if let Err(e) = written {
            warn!("cannot keep the public documents: {e}");
        }
        self.prune_revisions();
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    data_encoding::HEXLOWER.encode(bytes)
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    data_encoding::HEXLOWER
        .decode(text.as_bytes())
        .ok()?
        .try_into()
        .ok()
}

// ---------------------------------------------------------------------------------------------
// The protocol

/// What one peer asks another, about public documents only: any peer answers.
#[derive(Debug, Serialize, Deserialize)]
enum Ask {
    /// Everything it has heard.
    Catalog,
    /// A revision it keeps, by its hash.
    Revision([u8; 32]),
    /// A listing's picture, by its hash.
    Picture([u8; 32]),
    /// Statements the asker signed or heard (and their pictures), to take and say in the room.
    Tell(Catalog),
    /// The owners' names it keeps ([`Profile`]: asked beside the catalog, which older peers
    /// answer without them).
    Profiles,
    /// Names the asker signed or heard, to take and say in the room.
    TellProfiles(Vec<Signed<Profile>>),
}

#[derive(Debug, Clone)]
pub(crate) struct Handler(Shared);

pub(crate) fn handler(shared: Shared) -> Handler {
    Handler(shared)
}

impl Handler {
    fn answer(&self, from: EndpointId, request: &[u8]) -> Result<Vec<u8>, String> {
        match postcard::from_bytes(request).map_err(|_| "Not a request".to_string())? {
            Ask::Tell(catalog) => {
                let mut state = self.0.lock();
                let merged = state.directory.merge(&catalog, unix_now());
                if merged.changed() {
                    state.changed();
                    state.told.push((from, catalog, merged));
                }
                Ok(Vec::new())
            }
            Ask::Catalog => {
                let catalog = self.0.lock().directory.catalog();
                postcard::to_stdvec(&catalog).map_err(|e| e.to_string())
            }
            Ask::Revision(hash) => self
                .0
                .lock()
                .revision(&hash)
                .ok_or_else(|| "Not kept here".to_string()),
            Ask::Profiles => {
                let profiles = self.0.lock().directory.profiles(None);
                postcard::to_stdvec(&profiles).map_err(|e| e.to_string())
            }
            Ask::TellProfiles(profiles) => {
                let mut state = self.0.lock();
                let now = unix_now();
                let new: Vec<Signed<Profile>> = profiles
                    .into_iter()
                    .filter(|p| state.directory.add_profile(p, now).unwrap_or(false))
                    .collect();
                if !new.is_empty() {
                    state.changed();
                    state.told_profiles.extend(new);
                }
                Ok(Vec::new())
            }
            Ask::Picture(hash) => self
                .0
                .lock()
                .directory
                .picture(&hash)
                .cloned()
                .ok_or_else(|| "Not kept here".to_string()),
        }
    }
}

impl ProtocolHandler for Handler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok((mut send, mut recv)) = connection.accept_bi().await else {
            return Ok(());
        };
        let from = connection.remote_id();
        let answer = match recv.read_to_end(MAX_TELL).await {
            Ok(request) => self.answer(from, &request),
            Err(e) => Err(format!("Cannot read the request: {e}")),
        };
        // One byte of status, then the answer or the reason.
        let reply = match answer {
            Ok(bytes) => [&[0u8][..], &bytes].concat(),
            Err(why) => [&[1u8][..], why.as_bytes()].concat(),
        };
        if send.write_all(&reply).await.is_ok() {
            let _ = send.finish();
        }
        connection.closed().await;
        Ok(())
    }
}

/// Asks `peer` and reads the answer (at most `max` bytes).
async fn ask(
    endpoint: Result<Endpoint, String>,
    peer: EndpointAddr,
    ask: Ask,
    max: usize,
) -> Result<Vec<u8>, String> {
    let endpoint = endpoint?;
    let who = peer.id.fmt_short();
    let reach = async {
        let conn = endpoint
            .connect(peer, ALPN)
            .await
            .map_err(|e| e.to_string())?;
        let streams = conn.open_bi().await.map_err(|e| e.to_string())?;
        Ok::<_, String>((conn, streams))
    };
    let (conn, (mut send, mut recv)) = n0_future::time::timeout(REACH_WITHIN, reach)
        .await
        .map_err(|_| "timed out".to_string())
        .and_then(|r| r)
        .map_err(|e| format!("Cannot reach {who}: {e}"))?;
    let request = postcard::to_stdvec(&ask).map_err(|e| e.to_string())?;
    send.write_all(&request)
        .await
        .map_err(|e| format!("Lost {who}: {e}"))?;
    let _ = send.finish();
    let reply = recv
        .read_to_end(max + 1)
        .await
        .map_err(|e| format!("Lost {who}: {e}"))?;
    conn.close(0u32.into(), b"done");
    match reply.split_first() {
        Some((0, bytes)) => Ok(bytes.to_vec()),
        Some((_, why)) => Err(String::from_utf8_lossy(why).into_owned()),
        None => Err(format!("{who} sent nothing")),
    }
}

/// The endpoint to dial from. In a page it binds a few frames after startup, not before it.
fn dial_from(iroh: &Iroh) -> Result<Endpoint, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return Ok(iroh.endpoint());
    #[cfg(target_arch = "wasm32")]
    iroh.endpoint()
        .ok_or_else(|| "Not connected to the network yet".to_string())
}

/// This node's key, to sign with.
fn secret_key(iroh: &Iroh) -> Result<SecretKey, String> {
    dial_from(iroh).map(|e| e.secret_key().clone())
}

/// Asks `peer` for everything it has heard: poll the task from a system.
pub fn start_catalog(iroh: &Iroh, peer: EndpointAddr) -> IrohTask<Result<Catalog, String>> {
    let endpoint = dial_from(iroh);
    iroh.spawn(async move {
        let bytes = ask(endpoint, peer, Ask::Catalog, MAX_CATALOG).await?;
        postcard::from_bytes(&bytes).map_err(|_| "Not a catalog".to_string())
    })
}

/// Asks `peer` for a revision, checked against its hash.
pub fn start_revision(
    iroh: &Iroh,
    peer: EndpointAddr,
    hash: [u8; 32],
) -> IrohTask<Result<Vec<u8>, String>> {
    let endpoint = dial_from(iroh);
    iroh.spawn(async move {
        let bytes = ask(endpoint, peer, Ask::Revision(hash), MAX_REVISION).await?;
        if *blake3::hash(&bytes).as_bytes() != hash {
            return Err("The document did not match its listing".into());
        }
        Ok(bytes)
    })
}

/// Tells `peer` statements (and pictures), to take and say in its room.
pub fn start_tell(
    iroh: &Iroh,
    peer: EndpointAddr,
    catalog: &Catalog,
) -> IrohTask<Result<Vec<u8>, String>> {
    let endpoint = dial_from(iroh);
    let catalog = catalog.clone();
    iroh.spawn(async move { ask(endpoint, peer, Ask::Tell(catalog), 16).await })
}

/// An HTTPS client as iroh's own (its TLS set-up and embedded roots); a page's is the browser's.
fn http_client() -> Result<reqwest::Client, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use bevy_iroh::iroh::tls::{CaTlsConfig, default_provider};
        let tls = CaTlsConfig::embedded()
            .client_config(default_provider())
            .map_err(|e| e.to_string())?;
        reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .build()
            .map_err(|e| e.to_string())
    }
    #[cfg(target_arch = "wasm32")]
    Ok(reqwest::Client::new())
}

/// What a check-in says: this node's cadrs ID and the time, signed with its key.
fn check_in_body(key: &SecretKey, now: i64) -> serde_json::Value {
    let id = key.public().to_string();
    let signature = key.sign(format!("{CHECK_IN_CONTEXT}{id}\0{now}").as_bytes());
    serde_json::json!({
        "id": id,
        "time": now,
        "signature": data_encoding::HEXLOWER.encode(&signature.to_bytes()),
    })
}

#[derive(Deserialize)]
struct Online {
    peers: Vec<String>,
}

/// Checks in at the rendezvous `base`: the peers online it names (never this one).
pub fn start_check_in(iroh: &Iroh, base: &str) -> IrohTask<Result<Vec<EndpointId>, String>> {
    let body = secret_key(iroh).map(|key| check_in_body(&key, unix_now()));
    let url = format!("{}/v1/here", base.trim_end_matches('/'));
    let me = iroh.id();
    iroh.spawn(async move {
        let body = body?;
        let ask = async {
            http_client()?
                .post(&url)
                .json(&body)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| e.to_string())?
                .json::<Online>()
                .await
                .map_err(|e| e.to_string())
        };
        let online = n0_future::time::timeout(REACH_WITHIN, ask)
            .await
            .map_err(|_| "timed out".to_string())??;
        Ok(online
            .peers
            .iter()
            .filter_map(|p| p.parse::<EndpointId>().ok())
            .filter(|p| *p != me)
            .collect())
    })
}

/// Asks `peer` for the owners' names it keeps.
pub fn start_profiles(
    iroh: &Iroh,
    peer: EndpointAddr,
) -> IrohTask<Result<Vec<Signed<Profile>>, String>> {
    let endpoint = dial_from(iroh);
    iroh.spawn(async move {
        let bytes = ask(endpoint, peer, Ask::Profiles, MAX_CATALOG).await?;
        postcard::from_bytes(&bytes).map_err(|_| "Not a list of names".to_string())
    })
}

/// Tells `peer` names, to take and say in its room.
fn start_tell_profiles(iroh: &Iroh, peer: EndpointAddr, profiles: &[Signed<Profile>]) -> Asked {
    let endpoint = dial_from(iroh);
    let profiles = profiles.to_vec();
    iroh.spawn(async move { ask(endpoint, peer, Ask::TellProfiles(profiles), 16).await })
}

/// Asks `peer` for a listing's picture, checked against its hash.
pub fn start_picture(
    iroh: &Iroh,
    peer: EndpointAddr,
    hash: [u8; 32],
) -> IrohTask<Result<Vec<u8>, String>> {
    let endpoint = dial_from(iroh);
    iroh.spawn(async move {
        let bytes = ask(endpoint, peer, Ask::Picture(hash), MAX_PICTURE).await?;
        if *blake3::hash(&bytes).as_bytes() != hash {
            return Err("The picture did not match its listing".into());
        }
        Ok(bytes)
    })
}

/// Bytes asked of a peer: a revision, a picture, or nothing (a tell's answer).
type Asked = IrohTask<Result<Vec<u8>, String>>;

/// A revision on its way: asked of several peers at once, the first good answer wins.
pub struct RevisionFetch {
    pub key: Key,
    pub hash: [u8; 32],
    ready: Option<Vec<u8>>,
    tasks: Vec<IrohTask<Result<Vec<u8>, String>>>,
    errors: Vec<String>,
}

impl RevisionFetch {
    /// The revision once one peer gave it (it is then kept here, to serve), or why none did.
    pub fn poll(&mut self, shared: &Shared) -> Option<Result<Vec<u8>, String>> {
        if let Some(bytes) = self.ready.take() {
            return Some(Ok(bytes));
        }
        let mut got = None;
        self.tasks.retain_mut(|t| match t.try_take() {
            None => true,
            Some(Ok(bytes)) => {
                got.get_or_insert(bytes);
                false
            }
            Some(Err(why)) => {
                self.errors.push(why);
                false
            }
        });
        if let Some(bytes) = got {
            self.tasks.clear();
            shared.lock().keep_revision(bytes.clone());
            return Some(Ok(bytes));
        }
        if self.tasks.is_empty() {
            let why = if self.errors.is_empty() {
                "Nobody who has it is online".to_string()
            } else {
                format!(
                    "Nobody who has it is online ({})",
                    self.errors.first().cloned().unwrap_or_default()
                )
            };
            return Some(Err(why));
        }
        None
    }
}

// ---------------------------------------------------------------------------------------------
// The room

/// Messages in the public room: each one statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Announce(pub Signed<Listing>);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Withdraw(pub Signed<Withdrawal>);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Starred(pub Signed<Star>);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSaid(pub Signed<Profile>);

/// The peers to start from: seeds and others the app knows (contacts, owners of documents
/// shared with this user). Peers met in the room before are added to them.
#[derive(Resource, Debug, Clone, Default)]
pub struct PublicPeers(pub Vec<EndpointAddr>);

impl PublicPeers {
    /// The seeds: [`DEFAULT_SEEDS`] and `extra` (cadrs IDs; any that don't parse are skipped).
    pub fn seeds(extra: &[String]) -> Vec<EndpointAddr> {
        let mut out: Vec<EndpointAddr> = Vec::new();
        for id in DEFAULT_SEEDS
            .iter()
            .copied()
            .chain(extra.iter().map(|s| s.as_str()))
        {
            if let Ok(key) = id.trim().parse::<EndpointId>()
                && !out.iter().any(|a| a.id == key)
            {
                out.push(EndpointAddr::new(key));
            }
        }
        out
    }
}

/// Whether to take part while online (the user's preference): off, the public room is never
/// joined, though what was heard before is still shown.
#[derive(Resource, Debug, Clone, Copy)]
pub struct PublicEnabled(pub bool);

impl Default for PublicEnabled {
    fn default() -> Self {
        Self(true)
    }
}

/// Where to check in ([`RENDEZVOUS`], or `CADRS_RENDEZVOUS`); `None` never does.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct Rendezvous(pub Option<String>);

impl Default for Rendezvous {
    fn default() -> Self {
        Self(match std::env::var("CADRS_RENDEZVOUS") {
            Ok(v) if v == "0" || v.is_empty() => None,
            Ok(v) => Some(v),
            Err(_) => Some(RENDEZVOUS.to_string()),
        })
    }
}

/// A seed peer's: fetch and keep every revision heard of.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct KeepAll;

/// Statements made here, to say in the room, and names.
#[derive(Resource, Default)]
struct Outbox(Catalog, Vec<Signed<Profile>>);

/// This node's place in the public room.
#[derive(Resource, Default)]
pub struct PublicRoom {
    room: Option<Entity>,
    joined: bool,
    retry_at: f64,
    /// When it last asked the peers it knows for their catalogs, alone in the room.
    asked_alone_at: f64,
    /// The check-in on its way, when it was sent, and the peers online the last one named.
    check_in: Option<IrohTask<Result<Vec<EndpointId>, String>>>,
    checked_in_at: Option<f64>,
    found: Vec<EndpointId>,
    /// Statements told to seeds, on their way.
    tells: Vec<Asked>,
    /// When it last announced again for someone new.
    announced_for_newcomer: f64,
    /// Peers asked for their catalog since the room was joined, and how many answered.
    asked: HashSet<EndpointId>,
    answered: usize,
    syncs: Vec<(EndpointId, IrohTask<Result<Catalog, String>>)>,
    /// The names asked for beside each catalog.
    profile_syncs: Vec<IrohTask<Result<Vec<Signed<Profile>>, String>>>,
    announced_at: f64,
    resynced_at: f64,
    /// A seed's revisions on their way.
    fetches: Vec<RevisionFetch>,
    /// Listings' pictures on their way (heard of in the room, which carries only their hash).
    pictures: Vec<([u8; 32], Asked)>,
}

/// Where this node stands with the public room, for the page to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomState {
    /// Not online.
    Offline,
    /// Online, but not taking part in public documents (the user's choice).
    Off,
    /// Online, the room not joined yet.
    Joining,
    /// In the room, with this many others heard in it.
    Joined(usize),
}

impl PublicRoom {
    /// The room's entity while it is joined or being joined.
    pub fn entity(&self) -> Option<Entity> {
        self.room
    }

    /// The peers online the rendezvous last named.
    pub fn found(&self) -> &[EndpointId] {
        &self.found
    }
}

/// Where this node stands with the public room.
pub fn room_state(world: &World) -> RoomState {
    if !world.contains_resource::<Iroh>() {
        return RoomState::Offline;
    }
    if !world.get_resource::<PublicEnabled>().is_some_and(|e| e.0) {
        return RoomState::Off;
    }
    let Some(room) = world.get_resource::<PublicRoom>() else {
        return RoomState::Offline;
    };
    match room.room {
        Some(e) if room.joined => RoomState::Joined(world.get::<Members>(e).map_or(0, |m| m.len())),
        _ => RoomState::Joining,
    }
}

/// The public room's systems: an app that sets [`PublicPeers`] or [`PublicEnabled`] does so
/// before them, so the room is joined with the peers it knows.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PublicSystems;

/// The room's systems: added by [`crate::NetPlugin`].
pub(crate) fn register(app: &mut App) {
    app.init_resource::<PublicRoom>()
        .init_resource::<PublicPeers>()
        .init_resource::<Rendezvous>()
        .init_resource::<PublicEnabled>()
        .init_resource::<Outbox>()
        .add_message::<PeerJoined>()
        .add_net_message::<Announce>()
        .add_net_message::<Withdraw>()
        .add_net_message::<Starred>()
        .add_net_message::<ProfileSaid>()
        .add_systems(
            Update,
            (keep_room, hear, sync, send, fetch_all, save)
                .chain()
                .in_set(PublicSystems),
        );
}

fn now_secs(time: &Time<Real>) -> f64 {
    time.elapsed_secs_f64()
}

/// Joins the room while online (and taking part), leaves it when not, and joins again after a
/// failure. It checks in at the rendezvous first (waiting a little for the peers it names) and
/// every so often after. Alone in the room, it asks the peers it knows for their catalogs every
/// so often, and whenever they change.
#[allow(clippy::too_many_arguments)]
fn keep_room(
    mut commands: Commands,
    iroh: Option<Res<Iroh>>,
    enabled: Res<PublicEnabled>,
    rendezvous: Res<Rendezvous>,
    peers: Res<PublicPeers>,
    shared: Res<Shared>,
    mut room: ResMut<PublicRoom>,
    status: Query<&RoomStatus>,
    members: Query<&Members>,
    time: Res<Time<Real>>,
) {
    let now = now_secs(&time);
    let (Some(iroh), true) = (iroh, enabled.0) else {
        if let Some(e) = room.room.take() {
            commands.entity(e).try_despawn();
        }
        *room = PublicRoom {
            retry_at: now + REJOIN_GAP,
            ..default()
        };
        return;
    };
    // The rendezvous: who else is online.
    let mut found_now = false;
    if let Some(task) = room.check_in.as_mut()
        && let Some(r) = task.try_take()
    {
        room.check_in = None;
        match r {
            Ok(found) => {
                debug!("public rendezvous: {} others online", found.len());
                found_now = !found.is_empty();
                room.found = found;
            }
            Err(why) => debug!("public rendezvous: {why}"),
        }
    }
    if let Some(base) = &rendezvous.0
        && room.check_in.is_none()
        && room.checked_in_at.is_none_or(|t| now - t > CHECK_IN_EVERY)
    {
        room.checked_in_at = Some(now);
        room.check_in = Some(start_check_in(&iroh, base));
    }
    let found = room.found.clone();
    if let Some(e) = room.room {
        match status.get(e) {
            Ok(RoomStatus::Failed(why)) => {
                debug!("public room: {why}");
                commands.entity(e).try_despawn();
                room.room = None;
                room.joined = false;
                room.retry_at = now + RETRY_AFTER;
            }
            Ok(RoomStatus::Joined) if !room.joined => {
                room.joined = true;
                room.asked.clear();
                room.answered = 0;
                room.announced_at = f64::NEG_INFINITY;
                room.resynced_at = now;
                room.asked_alone_at = now;
                // Everyone it started from is asked what they know.
                for addr in bootstrap(&iroh, &peers, &found, &shared) {
                    ask_catalog(&iroh, &mut room, addr);
                }
            }
            Ok(RoomStatus::Joined) => {
                let alone = members.get(e).is_ok_and(|m| m.is_empty());
                if peers.is_changed()
                    || (alone && found_now)
                    || (alone && now - room.asked_alone_at > ALONE_ASK_EVERY)
                {
                    room.asked_alone_at = now;
                    for addr in bootstrap(&iroh, &peers, &found, &shared) {
                        room.asked.remove(&addr.id);
                        ask_catalog(&iroh, &mut room, addr);
                    }
                }
                return;
            }
            _ => return,
        }
        return;
    }
    // The first check-in may name peers to join through: worth a moment's wait.
    let checking_in =
        room.check_in.is_some() && room.checked_in_at.is_some_and(|t| now - t < CHECK_IN_WAIT);
    if now < room.retry_at || checking_in {
        return;
    }
    let ticket = RoomTicket::new(
        TopicId::from_bytes(TOPIC),
        ROOM_NAME,
        bootstrap(&iroh, &peers, &found, &shared),
    );
    room.room = Some(commands.spawn(Room::join(ticket)).id());
    room.joined = false;
}

/// Who to start from: the seeds and peers the app knows, those the rendezvous named, then the
/// peers met before; never this node itself.
fn bootstrap(
    iroh: &Iroh,
    peers: &PublicPeers,
    found: &[EndpointId],
    shared: &Shared,
) -> Vec<EndpointAddr> {
    let me = iroh.id();
    let mut out: Vec<EndpointAddr> = Vec::new();
    let remembered = shared.lock().remembered();
    for addr in peers
        .0
        .iter()
        .cloned()
        .chain(found.iter().copied().map(EndpointAddr::new))
        .chain(remembered.into_iter().map(EndpointAddr::new))
    {
        if addr.id != me && !out.iter().any(|a| a.id == addr.id) {
            out.push(addr);
        }
    }
    out
}

fn ask_catalog(iroh: &Iroh, room: &mut PublicRoom, addr: EndpointAddr) {
    if room.asked.insert(addr.id) {
        room.profile_syncs.push(start_profiles(iroh, addr.clone()));
        room.syncs.push((addr.id, start_catalog(iroh, addr)));
    }
}

/// Takes the statements heard in the room; a seed fetches the revisions they name.
#[allow(clippy::too_many_arguments)]
fn hear(
    mut announce: MessageReader<Received<Announce>>,
    mut withdraw: MessageReader<Received<Withdraw>>,
    mut starred: MessageReader<Received<Starred>>,
    mut named: MessageReader<Received<ProfileSaid>>,
    mut joined: MessageReader<PeerJoined>,
    iroh: Option<Res<Iroh>>,
    keep_all: Option<Res<KeepAll>>,
    shared: Res<Shared>,
    mut room: ResMut<PublicRoom>,
    time: Res<Time<Real>>,
) {
    let public = room.room;
    let now = unix_now();
    let mut new = Vec::new();
    {
        let mut state = shared.lock();
        let mut changed = false;
        for m in announce.read().filter(|m| Some(m.room) == public) {
            match state.directory.add_listing(&m.msg.0, now) {
                Ok(true) => {
                    changed = true;
                    if let Ok(l) = m.msg.0.open() {
                        new.push((l, m.from));
                    }
                }
                Ok(false) => state.dirty = true,
                Err(why) => debug!("public listing from {}: {why}", m.from.fmt_short()),
            }
        }
        for m in withdraw.read().filter(|m| Some(m.room) == public) {
            changed |= state
                .directory
                .add_withdrawal(&m.msg.0, now)
                .unwrap_or(false);
        }
        for m in starred.read().filter(|m| Some(m.room) == public) {
            changed |= state.directory.add_star(&m.msg.0, now).unwrap_or(false);
        }
        for m in named.read().filter(|m| Some(m.room) == public) {
            changed |= state.directory.add_profile(&m.msg.0, now).unwrap_or(false);
        }
        if changed {
            state.changed();
        }
    }
    let Some(iroh) = iroh else {
        joined.clear();
        return;
    };
    let now = now_secs(&time);
    for j in joined.read().filter(|j| Some(j.room) == public) {
        shared.lock().remember(&j.id);
        // A seed asks everyone; others the first few they meet.
        if keep_all.is_some() || room.answered < SYNC_FIRST {
            ask_catalog(&iroh, &mut room, EndpointAddr::new(j.id));
        }
        // What this node publishes, said again soon for them.
        if now - room.announced_for_newcomer > ANNOUNCE_FOR_NEWCOMERS {
            room.announced_for_newcomer = now;
            room.announced_at = f64::NEG_INFINITY;
        }
    }
    for (listing, from) in new {
        // The room carries a listing without its picture: asked of whoever said it.
        fetch_picture(&iroh, &shared, &mut room, &listing, from);
        if keep_all.is_some() {
            fetch_for_seed(&iroh, &shared, &mut room, &listing, from);
        }
    }
}

/// Asks `from` for a listing's picture this peer doesn't have.
fn fetch_picture(
    iroh: &Iroh,
    shared: &Shared,
    room: &mut PublicRoom,
    listing: &Listing,
    from: EndpointId,
) {
    let Some(hash) = listing.picture else {
        return;
    };
    if from == iroh.id()
        || shared.lock().directory.picture(&hash).is_some()
        || room.pictures.iter().any(|(h, _)| *h == hash)
    {
        return;
    }
    let task = start_picture(iroh, EndpointAddr::new(from), hash);
    room.pictures.push((hash, task));
}

/// A seed's fetch of a revision it doesn't keep yet: from its owner and from whoever said it.
fn fetch_for_seed(
    iroh: &Iroh,
    shared: &Shared,
    room: &mut PublicRoom,
    listing: &Listing,
    from: EndpointId,
) {
    if shared.lock().has_revision(&listing.hash)
        || room.fetches.iter().any(|f| f.hash == listing.hash)
    {
        return;
    }
    let mut peers = vec![from];
    if let Ok(owner) = listing.owner.parse::<EndpointId>()
        && owner != from
    {
        peers.push(owner);
    }
    room.fetches.push(RevisionFetch {
        key: listing.key(),
        hash: listing.hash,
        ready: None,
        tasks: peers
            .into_iter()
            .filter(|p| *p != iroh.id())
            .map(|p| start_revision(iroh, EndpointAddr::new(p), listing.hash))
            .collect(),
        errors: Vec::new(),
    });
}

/// Takes the catalogs that arrived, and asks someone in the room again now and then.
fn sync(
    iroh: Option<Res<Iroh>>,
    keep_all: Option<Res<KeepAll>>,
    shared: Res<Shared>,
    mut room: ResMut<PublicRoom>,
    members: Query<&Members>,
    peers: Query<&Peer>,
    time: Res<Time<Real>>,
) {
    // The names asked for beside the catalogs (a peer before them answers none).
    room.profile_syncs.retain_mut(|task| match task.try_take() {
        None => true,
        Some(Ok(profiles)) => {
            let mut state = shared.lock();
            let now = unix_now();
            let mut changed = false;
            for p in &profiles {
                changed |= state.directory.add_profile(p, now).unwrap_or(false);
            }
            if changed {
                state.changed();
            }
            false
        }
        Some(Err(why)) => {
            debug!("public names: {why}");
            false
        }
    });
    let mut arrived = Vec::new();
    room.syncs.retain_mut(|(id, task)| match task.try_take() {
        None => true,
        Some(r) => {
            arrived.push((*id, r));
            false
        }
    });
    let Some(iroh) = iroh else {
        return;
    };
    for (id, r) in arrived {
        match r {
            Ok(catalog) => {
                room.answered += 1;
                let merged = {
                    let mut state = shared.lock();
                    let merged = state.directory.merge(&catalog, unix_now());
                    if merged.changed() {
                        state.changed();
                    }
                    merged
                };
                debug!(
                    "public catalog from {}: {} listings, {} new",
                    id.fmt_short(),
                    catalog.listings.len(),
                    merged.listings.len()
                );
                if keep_all.is_some() {
                    for key in merged.listings {
                        let listing = shared.lock().directory.get(&key).map(|h| h.listing.clone());
                        if let Some(l) = listing {
                            fetch_for_seed(&iroh, &shared, &mut room, &l, id);
                        }
                    }
                }
            }
            Err(why) => {
                debug!("public catalog from {}: {why}", id.fmt_short());
                // Asked again when it turns up in the room.
                room.asked.remove(&id);
            }
        }
    }
    // Now and then, someone in the room again: what was said while this peer wasn't listening.
    let now = now_secs(&time);
    if room.joined && now - room.resynced_at > RESYNC_EVERY {
        room.resynced_at = now;
        let someone = room
            .room
            .and_then(|r| members.get(r).ok())
            .and_then(|m| {
                let n = m.len();
                (n > 0).then(|| m.iter().nth((now as usize) % n)).flatten()
            })
            .and_then(|p| peers.get(p).ok())
            .map(|p| p.id);
        if let Some(id) = someone {
            room.asked.remove(&id);
            ask_catalog(&iroh, &mut room, EndpointAddr::new(id));
        }
    }
}

/// Says in the room what was signed here, and again every so often for peers that missed it.
#[allow(clippy::too_many_arguments)]
fn send(
    net: NetSender,
    iroh: Option<Res<Iroh>>,
    keep_all: Option<Res<KeepAll>>,
    shared: Res<Shared>,
    peers: Res<PublicPeers>,
    mut outbox: ResMut<Outbox>,
    mut room: ResMut<PublicRoom>,
    members: Query<&Members>,
    time: Res<Time<Real>>,
) {
    let (Some(iroh), Some(public), true) = (iroh, room.room, room.joined) else {
        return;
    };
    // What peers told this one, new here: said in the room in turn.
    let told_profiles = std::mem::take(&mut shared.lock().told_profiles);
    outbox.1.extend(told_profiles);
    let told = std::mem::take(&mut shared.lock().told);
    for (from, catalog, merged) in told {
        outbox.0.listings.extend(catalog.listings);
        outbox.0.withdrawals.extend(catalog.withdrawals);
        outbox.0.stars.extend(catalog.stars);
        for key in merged.listings {
            let listing = shared.lock().directory.get(&key).map(|h| h.listing.clone());
            if let Some(l) = listing {
                fetch_picture(&iroh, &shared, &mut room, &l, from);
                if keep_all.is_some() {
                    fetch_for_seed(&iroh, &shared, &mut room, &l, from);
                }
            }
        }
    }
    let now = now_secs(&time);
    if now - room.announced_at > ANNOUNCE_EVERY {
        room.announced_at = now;
        let me = iroh.id().to_string();
        let (own, name) = {
            let state = shared.lock();
            (
                state.directory.signed_by(&me),
                state.directory.profiles(Some(&me)),
            )
        };
        outbox.0.listings.extend(own.listings);
        outbox.0.withdrawals.extend(own.withdrawals);
        outbox.0.stars.extend(own.stars);
        outbox.1.extend(name);
    }
    let mut out = std::mem::take(&mut outbox.0);
    let names = std::mem::take(&mut outbox.1);
    if out == Catalog::default() && names.is_empty() {
        return;
    }
    for p in &names {
        net.broadcast(public, &ProfileSaid(p.clone()));
    }
    for w in &out.withdrawals {
        net.broadcast(public, &Withdraw(w.clone()));
    }
    for l in &out.listings {
        net.broadcast(public, &Announce(l.clone()));
    }
    for s in &out.stars {
        net.broadcast(public, &Starred(s.clone()));
    }
    // And straight to the seeds (and, alone in the room, the peers met before), with the
    // listings' pictures: gossip may not have joined them.
    {
        let state = shared.lock();
        out.pictures = out
            .listings
            .iter()
            .filter_map(|l| l.open().ok()?.picture)
            .filter_map(|h| state.directory.picture(&h).cloned())
            .collect();
    }
    let alone = members.get(public).is_ok_and(|m| m.is_empty());
    let mut to: Vec<EndpointAddr> = peers.0.clone();
    if alone {
        let found = room.found.clone();
        to.extend(found.into_iter().map(EndpointAddr::new));
        to.extend(
            shared
                .lock()
                .remembered()
                .into_iter()
                .map(EndpointAddr::new),
        );
    }
    let me = iroh.id();
    let mut told = HashSet::new();
    let has_statements = out != Catalog::default();
    for addr in to {
        if addr.id != me && told.insert(addr.id) {
            if !names.is_empty() {
                room.tells
                    .push(start_tell_profiles(&iroh, addr.clone(), &names));
            }
            if has_statements {
                room.tells.push(start_tell(&iroh, addr, &out));
            }
        }
    }
}

/// A seed's fetches (each revision kept once one peer gave it), and the pictures asked for;
/// every [`SWEEP_EVERY`], what is still missing is asked of the owners (a listing may have come
/// any way: the room, a catalog, a tell).
fn fetch_all(
    shared: Res<Shared>,
    iroh: Option<Res<Iroh>>,
    keep_all: Option<Res<KeepAll>>,
    mut room: ResMut<PublicRoom>,
    time: Res<Time<Real>>,
    mut swept: Local<f64>,
) {
    let shared = shared.clone();
    let now = now_secs(&time);
    if let Some(iroh) = iroh.as_deref()
        && room.joined
        && now - *swept > SWEEP_EVERY
    {
        *swept = now;
        let (pictures, revisions): (Vec<Listing>, Vec<Listing>) = {
            let state = shared.lock();
            let d = &state.directory;
            let revisions = d
                .docs("")
                .into_iter()
                .map(|doc| doc.listing)
                .filter(|l| keep_all.is_some() && !state.has_revision(&l.hash))
                .collect();
            (d.missing_pictures(), revisions)
        };
        for l in pictures {
            if let Ok(owner) = l.owner.parse::<EndpointId>() {
                fetch_picture(iroh, &shared, &mut room, &l, owner);
            }
        }
        for l in revisions {
            if let Ok(owner) = l.owner.parse::<EndpointId>() {
                fetch_for_seed(iroh, &shared, &mut room, &l, owner);
            }
        }
    }
    room.fetches.retain_mut(|f| match f.poll(&shared) {
        None => true,
        Some(Ok(_)) => {
            info!("public: keeping a revision of {} by {}", f.key.1.0, f.key.0);
            false
        }
        Some(Err(why)) => {
            debug!("public: cannot fetch {}: {why}", f.key.1.0);
            false
        }
    });
    room.tells.retain_mut(|t| match t.try_take() {
        None => true,
        Some(Ok(_)) => false,
        Some(Err(why)) => {
            debug!("public: cannot tell: {why}");
            false
        }
    });
    room.pictures.retain_mut(|(_, task)| match task.try_take() {
        None => true,
        Some(Ok(bytes)) => {
            let mut state = shared.lock();
            if state.directory.add_picture(bytes) {
                state.changed();
            }
            false
        }
        Some(Err(why)) => {
            debug!("public: cannot fetch a picture: {why}");
            false
        }
    });
}

/// Writes the directory out a few seconds after it changed.
fn save(shared: Res<Shared>, time: Res<Time<Real>>, mut last: Local<f64>) {
    let now = now_secs(&time);
    if now - *last < SAVE_AFTER {
        return;
    }
    *last = now;
    let mut state = shared.lock();
    let old = state.directory.forget_old(unix_now());
    if old {
        state.changed();
    }
    state.save();
}

// ---------------------------------------------------------------------------------------------
// What the app does

/// A document to publish: everything a listing says but what publishing adds.
#[derive(Debug, Clone)]
pub struct Draft {
    pub document: DocumentId,
    pub name: String,
    pub description: String,
    /// The owner's name as it is now (else the node's display name).
    pub owner_name: String,
    /// The documents page's picture of it (PNG); none if empty or too big.
    pub picture: Vec<u8>,
    pub forked_from: Option<Origin>,
    /// The revision it was last published as, if it was (the next is one more).
    pub previous: u32,
}

/// `text` cut to at most `max` bytes, at a character's end.
fn fit(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Publishes `revision` (a packed [`cadrs_core::collab::Snapshot`]) as the document `draft`
/// describes, signed by this node: kept to serve, listed here and said in the room.
pub fn publish(world: &mut World, draft: Draft, revision: Vec<u8>) -> Result<Listing, String> {
    let iroh = world
        .get_resource::<Iroh>()
        .ok_or("Publishing needs cadrs to be online")?;
    if revision.len() > MAX_REVISION {
        return Err("The document is too big to publish".into());
    }
    let key = secret_key(iroh)?;
    let owner = iroh.id().to_string();
    let owner_name = if draft.owner_name.trim().is_empty() {
        iroh.display_name().to_string()
    } else {
        draft.owner_name.clone()
    };
    let now = unix_now();
    let shared = world.resource::<Shared>().clone();
    let mut state = shared.lock();
    let doc_key = (owner.clone(), draft.document);
    // Later than the last revision or withdrawal (the later published wins), and counting on
    // from them.
    let (published, previous) = match state.directory.latest(&doc_key) {
        Some((time, revision)) => (now.max(time + 1), revision.max(draft.previous)),
        None => (now, draft.previous),
    };
    let picture =
        (!draft.picture.is_empty() && draft.picture.len() <= MAX_PICTURE).then_some(draft.picture);
    let listing = Listing {
        document: draft.document,
        owner,
        owner_name: fit(&owner_name, MAX_NAME),
        name: fit(&draft.name, MAX_NAME),
        description: fit(&draft.description, MAX_DESCRIPTION),
        revision: previous + 1,
        published,
        hash: *blake3::hash(&revision).as_bytes(),
        size: revision.len() as u64,
        picture: picture.as_ref().map(|p| *blake3::hash(p).as_bytes()),
        forked_from: draft.forked_from.map(|o| Origin {
            owner_name: fit(&o.owner_name, MAX_NAME),
            name: fit(&o.name, MAX_NAME),
            ..o
        }),
    };
    let signed = Signed::new(&listing, &key)?;
    if postcard::to_stdvec(&Announce(signed.clone())).map_or(0, |b| b.len()) > ROOM_MESSAGE {
        return Err("The listing is too long to publish".into());
    }
    state.keep_revision(revision);
    state.directory.add_listing(&signed, now)?;
    if let Some(p) = picture {
        state.directory.add_picture(p);
    }
    state.changed();
    drop(state);
    world.resource_mut::<Outbox>().0.listings.push(signed);
    info!(
        "published \"{}\" as revision {} ({} bytes)",
        listing.name, listing.revision, listing.size
    );
    Ok(listing)
}

/// Says what this node's user calls themselves now, for their public documents everywhere
/// ([`Profile`]); nothing when it is the name already said. True if it was said.
pub fn say_profile(world: &mut World, name: &str) -> Result<bool, String> {
    let iroh = world
        .get_resource::<Iroh>()
        .ok_or("Renaming yourself for others needs cadrs to be online")?;
    let key = secret_key(iroh)?;
    let owner = iroh.id().to_string();
    let name = fit(name.trim(), MAX_NAME);
    let shared = world.resource::<Shared>().clone();
    let mut state = shared.lock();
    let last = state.directory.profile(&owner).cloned();
    if last.as_ref().is_some_and(|p| p.name == name) {
        return Ok(false);
    }
    // Later than the name it replaces, whatever the clocks did.
    let time = last.map_or(unix_now(), |p| unix_now().max(p.time + 1));
    let signed = Signed::new(&Profile { owner, name, time }, &key)?;
    state.directory.add_profile(&signed, unix_now())?;
    state.changed();
    drop(state);
    world.resource_mut::<Outbox>().1.push(signed);
    Ok(true)
}

/// Makes a document this node published private again.
pub fn withdraw(world: &mut World, document: DocumentId) -> Result<(), String> {
    let iroh = world
        .get_resource::<Iroh>()
        .ok_or("Making a document private needs cadrs to be online")?;
    let key = secret_key(iroh)?;
    let owner = iroh.id().to_string();
    let shared = world.resource::<Shared>().clone();
    let mut state = shared.lock();
    let doc_key = (owner.clone(), document);
    // Later than the revision it hides.
    let (time, revision) = state
        .directory
        .latest(&doc_key)
        .map_or((unix_now(), 0), |(t, r)| (unix_now().max(t), r));
    let signed = Signed::new(
        &Withdrawal {
            document,
            owner,
            time,
            revision,
        },
        &key,
    )?;
    state.directory.add_withdrawal(&signed, unix_now())?;
    state.changed();
    drop(state);
    world.resource_mut::<Outbox>().0.withdrawals.push(signed);
    Ok(())
}

/// Stars a public document as this node's user, or takes the star back.
pub fn star(world: &mut World, doc: &Key, starred: bool) -> Result<(), String> {
    let iroh = world
        .get_resource::<Iroh>()
        .ok_or("Starring needs cadrs to be online")?;
    let key = secret_key(iroh)?;
    let by = iroh.id().to_string();
    let shared = world.resource::<Shared>().clone();
    let mut state = shared.lock();
    // Later than this user's last word on it, whatever the clocks did.
    let time = state
        .directory
        .star_time(doc, &by)
        .map_or(unix_now(), |t| unix_now().max(t + 1));
    let signed = Signed::new(
        &Star {
            document: doc.1,
            owner: doc.0.clone(),
            by,
            starred,
            time,
        },
        &key,
    )?;
    state.directory.add_star(&signed, unix_now())?;
    state.changed();
    drop(state);
    world.resource_mut::<Outbox>().0.stars.push(signed);
    Ok(())
}

/// Fetches the revision a public document is listed at: kept here already, or asked of its
/// owner, the seeds and a few peers in the room at once.
pub fn fetch(world: &mut World, doc: &Key) -> Result<RevisionFetch, String> {
    let shared = world.resource::<Shared>().clone();
    let hash = shared
        .lock()
        .directory
        .get(doc)
        .map(|h| h.listing.hash)
        .ok_or("It is not public any more")?;
    let kept = shared.lock().revision(&hash);
    if let Some(bytes) = kept {
        return Ok(RevisionFetch {
            key: doc.clone(),
            hash,
            ready: Some(bytes),
            tasks: Vec::new(),
            errors: Vec::new(),
        });
    }
    let iroh = world
        .get_resource::<Iroh>()
        .ok_or("Fetching a public document needs cadrs to be online")?;
    let me = iroh.id();
    let mut ask: Vec<EndpointId> = Vec::new();
    let mut add = |id: EndpointId| {
        if id != me && !ask.contains(&id) {
            ask.push(id);
        }
    };
    if let Ok(owner) = doc.0.parse::<EndpointId>() {
        add(owner);
    }
    for a in &world.resource::<PublicPeers>().0 {
        add(a.id);
    }
    if let Some(members) = world
        .resource::<PublicRoom>()
        .room
        .and_then(|r| world.get::<Members>(r))
    {
        for p in members.iter().take(8) {
            if let Some(peer) = world.get::<Peer>(p) {
                add(peer.id);
            }
        }
    }
    let tasks = ask
        .into_iter()
        .map(|id| start_revision(iroh, EndpointAddr::new(id), hash))
        .collect();
    Ok(RevisionFetch {
        key: doc.clone(),
        hash,
        ready: None,
        tasks,
        errors: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SecretKey {
        SecretKey::generate()
    }

    fn listing(k: &SecretKey, document: DocumentId, revision: u32, published: i64) -> Listing {
        Listing {
            document,
            owner: k.public().to_string(),
            owner_name: "Ann".into(),
            name: "Bracket".into(),
            description: String::new(),
            revision,
            published,
            hash: [revision as u8; 32],
            size: 10,
            picture: None,
            forked_from: None,
        }
    }

    #[test]
    fn a_statement_opens_only_as_its_signer_signed_it() {
        let k = key();
        let l = listing(&k, DocumentId::new(), 1, 100);
        let signed = Signed::new(&l, &k).unwrap();
        assert_eq!(signed.open().unwrap(), l);
        // Someone else's key can't sign it.
        assert!(Signed::new(&l, &key()).is_err());
        // A changed byte breaks it.
        let mut bent = signed.clone();
        let last = bent.body.len() - 1;
        bent.body[last] ^= 1;
        assert!(bent.open().is_err());
        // The same bytes don't pass for another kind of statement.
        let as_withdrawal = Signed::<Withdrawal> {
            body: signed.body.clone(),
            signature: signed.signature.clone(),
            of: PhantomData,
        };
        assert!(as_withdrawal.open().is_err());
        // It survives the wire.
        let wire = postcard::to_stdvec(&Announce(signed.clone())).unwrap();
        let back: Announce = postcard::from_bytes(&wire).unwrap();
        assert_eq!(back.0, signed);
    }

    #[test]
    fn a_newer_revision_replaces_an_older_one_and_a_withdrawal_hides_it_until_published_again() {
        let k = key();
        let doc = DocumentId::new();
        let mut d = Directory::default();
        let v1 = Signed::new(&listing(&k, doc, 1, 100), &k).unwrap();
        let v2 = Signed::new(&listing(&k, doc, 2, 200), &k).unwrap();
        assert_eq!(d.add_listing(&v2, 1000), Ok(true));
        assert_eq!(d.add_listing(&v1, 1000), Ok(false));
        let key_ = (k.public().to_string(), doc);
        assert_eq!(d.get(&key_).unwrap().listing.revision, 2);

        let w = Withdrawal {
            document: doc,
            owner: k.public().to_string(),
            time: 300,
            revision: 2,
        };
        assert_eq!(
            d.add_withdrawal(&Signed::new(&w, &k).unwrap(), 1000),
            Ok(true)
        );
        assert!(d.is_empty());
        // Hearing the old revision again doesn't bring it back.
        assert_eq!(d.add_listing(&v2, 1000), Ok(false));
        assert!(d.is_empty());
        // Published again after it was withdrawn.
        let v3 = Signed::new(&listing(&k, doc, 3, 400), &k).unwrap();
        assert_eq!(d.add_listing(&v3, 1000), Ok(true));
        assert_eq!(d.len(), 1);
        assert_eq!(d.latest(&key_), Some((400, 3)));

        // Someone else can't withdraw it, nor list a document as theirs.
        let other = key();
        let forged = Withdrawal {
            document: doc,
            owner: k.public().to_string(),
            time: 500,
            revision: 3,
        };
        assert!(Signed::new(&forged, &other).is_err());
        let theirs = Signed::new(&listing(&other, doc, 9, 500), &other).unwrap();
        assert_eq!(d.add_listing(&theirs, 1000), Ok(true));
        assert_eq!(d.get(&key_).unwrap().listing.revision, 3);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn stars_count_each_id_once_by_its_latest_word() {
        let owner = key();
        let doc = DocumentId::new();
        let k = (owner.public().to_string(), doc);
        let mut d = Directory::default();
        let star = |by: &SecretKey, starred: bool, time: i64| {
            Signed::new(
                &Star {
                    document: doc,
                    owner: k.0.clone(),
                    by: by.public().to_string(),
                    starred,
                    time,
                },
                by,
            )
            .unwrap()
        };
        let (a, b) = (key(), key());
        assert_eq!(d.add_star(&star(&a, true, 10), 100), Ok(true));
        assert_eq!(d.add_star(&star(&a, true, 11), 100), Ok(true));
        assert_eq!(d.add_star(&star(&b, true, 10), 100), Ok(true));
        assert_eq!(d.stars(&k), 2);
        // Taken back, then an older star heard late: still taken back.
        assert_eq!(d.add_star(&star(&a, false, 20), 100), Ok(true));
        assert_eq!(d.add_star(&star(&a, true, 15), 100), Ok(false));
        assert_eq!(d.stars(&k), 1);
        assert!(!d.starred_by(&k, &a.public().to_string()));
        assert!(d.starred_by(&k, &b.public().to_string()));
        // Far in the future: refused.
        assert!(
            d.add_star(&star(&a, true, 100 + 2 * MAX_AHEAD), 100)
                .is_err()
        );
    }

    #[test]
    fn a_catalog_carries_everything_to_another_peer_and_oversized_listings_are_refused() {
        let k = key();
        let mut a = Directory::default();
        let doc = DocumentId::new();
        let picture = b"a png".to_vec();
        let mut l = listing(&k, doc, 1, 100);
        l.picture = Some(*blake3::hash(&picture).as_bytes());
        a.add_listing(&Signed::new(&l, &k).unwrap(), 1000).unwrap();
        // Only a picture a listing names is kept.
        assert!(a.add_picture(picture.clone()));
        assert!(!a.add_picture(b"someone else's".to_vec()));
        let mut big = listing(&k, DocumentId::new(), 1, 100);
        big.description = "x".repeat(MAX_DESCRIPTION + 1);
        assert!(
            a.add_listing(&Signed::new(&big, &k).unwrap(), 1000)
                .is_err()
        );
        let star = Star {
            document: doc,
            owner: k.public().to_string(),
            by: k.public().to_string(),
            starred: true,
            time: 50,
        };
        a.add_star(&Signed::new(&star, &k).unwrap(), 1000).unwrap();

        let wire = postcard::to_stdvec(&a.catalog()).unwrap();
        let catalog: Catalog = postcard::from_bytes(&wire).unwrap();
        let mut b = Directory::default();
        let merged = b.merge(&catalog, 1000);
        assert_eq!(merged.listings, vec![(k.public().to_string(), doc)]);
        assert_eq!(
            b.docs(&k.public().to_string()),
            a.docs(&k.public().to_string())
        );
        assert_eq!(
            b.docs("").first().and_then(|d| d.picture.clone()),
            Some(picture)
        );
        // Again: nothing new.
        assert!(!b.merge(&catalog, 1001).changed());
    }

    #[test]
    fn the_latest_name_shows_for_all_of_an_owner_s_documents_and_only_they_sign_it() {
        let k = key();
        let me = k.public().to_string();
        let mut d = Directory::default();
        d.add_listing(
            &Signed::new(&listing(&k, DocumentId::new(), 1, 100), &k).unwrap(),
            1000,
        )
        .unwrap();
        assert_eq!(d.docs("")[0].listing.owner_name, "Ann");
        let named = |name: &str, time| {
            Signed::new(
                &Profile {
                    owner: me.clone(),
                    name: name.into(),
                    time,
                },
                &k,
            )
            .unwrap()
        };
        assert_eq!(d.add_profile(&named("Annie", 200), 1000), Ok(true));
        assert_eq!(d.add_profile(&named("Old", 150), 1000), Ok(false));
        assert_eq!(d.docs("")[0].listing.owner_name, "Annie");
        assert_eq!(d.profiles(Some(&me)).len(), 1);
        // Nobody else signs this owner's name.
        let forged = Profile {
            owner: me.clone(),
            name: "Mallory".into(),
            time: 300,
        };
        assert!(Signed::new(&forged, &key()).is_err());
    }

    /// What the rendezvous (`tools/peers`) checks: the ID's key signed the context, the ID and
    /// the time.
    #[test]
    fn a_check_in_is_signed_as_the_rendezvous_checks_it() {
        let k = key();
        let body = check_in_body(&k, 1_790_000_000);
        let id = body["id"].as_str().unwrap();
        assert_eq!(id, k.public().to_string());
        assert_eq!(body["time"], 1_790_000_000);
        let sig: [u8; 64] = data_encoding::HEXLOWER
            .decode(body["signature"].as_str().unwrap().as_bytes())
            .unwrap()
            .try_into()
            .unwrap();
        let message = format!("cadrs rendezvous 1\0{id}\01790000000");
        assert!(
            k.public()
                .verify(message.as_bytes(), &Signature::from_bytes(&sig))
                .is_ok()
        );
    }

    #[test]
    fn the_longest_listing_fits_in_a_room_message() {
        let k = key();
        let mut l = listing(&k, DocumentId::new(), u32::MAX, i64::MAX);
        // Names of four-byte characters, cut to their limits.
        l.name = fit(&"𝔅".repeat(MAX_NAME), MAX_NAME);
        l.owner_name = l.name.clone();
        l.description = fit(&"𝔅".repeat(MAX_DESCRIPTION), MAX_DESCRIPTION);
        l.picture = Some([9; 32]);
        l.forked_from = Some(Origin {
            document: DocumentId::new(),
            owner: key().public().to_string(),
            owner_name: l.name.clone(),
            name: l.name.clone(),
        });
        let message = postcard::to_stdvec(&Announce(Signed::new(&l, &k).unwrap())).unwrap();
        assert!(message.len() <= ROOM_MESSAGE, "{} bytes", message.len());
    }

    #[test]
    fn the_directory_and_revisions_outlive_a_restart_and_unlisted_revisions_go() {
        let dir = std::env::temp_dir().join(format!("cadrs-public-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let k = key();
        let doc = DocumentId::new();
        let shared = Shared::default();
        shared.open(dir.clone(), 1000);
        let (kept, stale) = {
            let mut s = shared.lock();
            let kept = s.keep_revision(b"revision one".to_vec());
            let stale = s.keep_revision(b"no longer listed".to_vec());
            let mut l = listing(&k, doc, 1, 100);
            l.hash = kept;
            s.directory
                .add_listing(&Signed::new(&l, &k).unwrap(), 1000)
                .unwrap();
            s.remember(&k.public());
            s.changed();
            s.save();
            (kept, stale)
        };
        let again = Shared::default();
        again.open(dir.clone(), 2000);
        let s = again.lock();
        assert_eq!(s.directory.len(), 1);
        assert_eq!(s.revision(&kept).as_deref(), Some(&b"revision one"[..]));
        assert!(!s.has_revision(&stale));
        assert_eq!(s.remembered(), vec![k.public()]);
        drop(s);
        // Not heard of for long enough: forgotten.
        let mut s = again.lock();
        assert!(s.directory.forget_old(1000 + FORGET_AFTER + 1));
        assert!(s.directory.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
