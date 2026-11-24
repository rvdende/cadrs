//! The marketplace's directory of organizations: what one that asked to be listed says about
//! itself, and searching what was heard by words, by what the shops can make and by distance.
//!
//! A [`ShopListing`] is signed by the organization's own key and carries its contact card, so
//! someone who finds a shop can send it a request straight away. It names the logo by hash (the
//! picture is fetched from the shop's devices, [`crate::blob`]) and gives the organization's
//! public locations, each coarsened to the accuracy the shop chose
//! ([`crate::Location::public_place`]): the directory never knows the street.
//!
//! A listing travels in the public room, whose messages are small: [`sign`] trims one to
//! [`MAX_LISTING`] bytes (the description first, then the longer lists), so a shop with a long
//! description and forty machines is still listed, rather than silently never heard. It is
//! signed as bytes ([`cadrs_social::keys::SignedBytes`]), so a listing with fields a later cadrs
//! adds is still read by this one.

use std::collections::BTreeMap;

use cadrs_social::contact::Card;
use cadrs_social::keys::{AccountId, Growing, SecretKey, Signed, SignedBytes};
use cadrs_social::place::Place;
use serde::{Deserialize, Serialize};

use crate::capability::Process;
use crate::profile::{BlobRef, Offers};
use crate::{Org, profile};

/// The largest signed listing, in bytes: under what the public room carries in one message.
pub const MAX_LISTING: usize = 3000;
/// The most shops a directory keeps. Past it, the one this device heard of longest ago goes
/// first ([`Directory::protect`] names those that never go this way).
pub const MAX_SHOPS: usize = 5000;
/// How long a listing is believed after it was issued: a shop that is still there signs it
/// afresh every day it is online.
pub const KEEP_FOR: u64 = 7 * 24 * 60 * 60 * 1000;
/// How far ahead of this device's clock a listing may be issued: clocks differ by minutes, not
/// by days.
const CLOCK_SLACK: u64 = 10 * 60 * 1000;
/// The most machines, the most materials of a machine and the most places a listing shows
/// (what [`sign`] keeps of a larger organization at the most).
pub const MAX_OFFERS: usize = 24;
pub const MAX_OFFER_MATERIALS: usize = 8;
pub const MAX_PLACES: usize = 8;

/// What one machine of a shop can do, as the directory shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    pub process: Process,
    /// The bed or envelope, mm.
    pub envelope: [f64; 3],
    /// (material, thickest it cuts in mm).
    pub materials: Vec<(String, f64)>,
}

/// A device's own word that what it publishes is published for an organization: signed with
/// the device's key, so nobody can say it for a device that isn't theirs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Publishes {
    pub org: AccountId,
    /// When it said so, in milliseconds since the Unix epoch: the listing's own date. It
    /// counts for that listing and no other, so an organization can't go on showing a device's
    /// word after the device stopped giving it.
    pub issued: u64,
}

impl cadrs_social::keys::Statement for Publishes {
    const CONTEXT: &'static str = "cadrs publishes for 1";
}

/// The most devices a listing says publish for it.
pub const MAX_PUBLISHERS: usize = 4;

/// An organization in the marketplace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShopListing {
    pub card: Signed<Card>,
    /// The line under its name.
    #[serde(default)]
    pub tagline: String,
    /// Its description (cut short when the listing would be too large).
    #[serde(default)]
    pub about: String,
    /// Manufacturing, designs or both.
    #[serde(default)]
    pub kinds: Offers,
    /// The small logo (for cards) and the full one (for its page), fetched from its devices.
    #[serde(default)]
    pub logo: Option<BlobRef>,
    #[serde(default)]
    pub logo_full: Option<BlobRef>,
    /// Its public locations, coarsened.
    #[serde(default)]
    pub places: Vec<Place>,
    #[serde(default)]
    pub offers: Vec<Offer>,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub website: Option<String>,
    /// Not used: a rating a shop gives of itself proves nothing, so none is said, shown or
    /// sorted by ([`Directory`] forgets what it hears here) until reviews carry their clients'
    /// signatures.
    #[serde(default)]
    pub rating: Option<(f64, usize)>,
    /// Milliseconds since the Unix epoch: a newer listing replaces an older one.
    pub issued: u64,
    /// The devices that publish designs for it, each saying so itself ([`Publishes`]): what
    /// ties a public document to the organization. A card names devices too, but an
    /// organization can name any device on its card; only the device can sign this.
    #[serde(default)]
    pub publishers: Vec<Signed<Publishes>>,
    /// The shop left the marketplace: this replaces its listing and shows nothing
    /// ([`ShopListing::withdrawal`]). A directory keeps it as long as it would a listing, so an
    /// older listing heard later doesn't bring the shop back.
    #[serde(default)]
    pub withdrawn: bool,
}

impl Growing for ShopListing {
    const CONTEXT: &'static str = "cadrs shop listing 1";
}

/// What a listing is cut down to, one step after another, until it fits: (description's
/// characters, machines, materials a machine, places).
const TRIMS: [(usize, usize, usize, usize); 5] = [
    (600, MAX_OFFERS, MAX_OFFER_MATERIALS, MAX_PLACES),
    (300, 12, 6, 6),
    (120, 8, 4, 4),
    (0, 4, 3, 2),
    (0, 1, 1, 1),
];

impl ShopListing {
    /// The listing for `org`, with its card, as of `now`; none when it isn't listed (it didn't
    /// ask to be, or its profile lacks something: [`profile::missing`]).
    pub fn of(org: &Org, card: Signed<Card>, now: u64) -> Option<Self> {
        if !org.listed() {
            return None;
        }
        let p = &org.profile;
        Some(Self {
            card,
            tagline: p.tagline.clone(),
            about: p.about.clone(),
            kinds: p.offers,
            logo: p.logo.map(|l| l.thumb),
            logo_full: p.logo.map(|l| l.full),
            places: org
                .locations
                .values()
                .filter_map(profile::Location::public_place)
                .collect(),
            offers: org
                .machines
                .values()
                .map(|m| Offer {
                    process: m.process,
                    envelope: m.envelope,
                    materials: m
                        .materials
                        .iter()
                        .map(|l| (l.material.clone(), l.max_thickness))
                        .collect(),
                })
                .collect(),
            email: p.email.clone(),
            website: p.website.clone(),
            rating: None,
            issued: now,
            publishers: Vec::new(),
            withdrawn: false,
        })
    }

    /// The devices that publish designs for this organization: those that said so themselves
    /// (their own signature, naming this organization).
    pub fn publishing_devices(&self) -> Vec<cadrs_social::keys::DeviceId> {
        let org = self.account();
        self.publishers
            .iter()
            .filter(|said| {
                said.verify()
                    .is_ok_and(|p| p.org == org && p.issued == self.issued)
            })
            .map(|said| said.signer)
            .collect()
    }

    /// What a shop that left the marketplace says in place of its listing: its card and
    /// nothing else.
    pub fn withdrawal(card: Signed<Card>, now: u64) -> Self {
        Self {
            card,
            tagline: String::new(),
            about: String::new(),
            kinds: Offers::default(),
            logo: None,
            logo_full: None,
            places: Vec::new(),
            offers: Vec::new(),
            email: String::new(),
            website: None,
            rating: None,
            issued: now,
            publishers: Vec::new(),
            withdrawn: true,
        }
    }

    pub fn name(&self) -> &str {
        &self.card.value.name
    }

    pub fn account(&self) -> AccountId {
        self.card.value.account
    }

    /// The processes its machines offer, each once, in a set order.
    pub fn processes(&self) -> Vec<Process> {
        let mut all: Vec<Process> = self.offers.iter().map(|o| o.process).collect();
        all.sort();
        all.dedup();
        all
    }

    /// "Stellenbosch, South Africa": its first public location, as a card shows it.
    pub fn where_label(&self) -> Option<String> {
        self.places
            .iter()
            .filter_map(|p| p.region.as_ref())
            .map(|r| r.label())
            .find(|l| !l.is_empty())
    }

    /// Whether one of its machines cuts `material` at `thickness` mm with `process` (any, when
    /// not given).
    pub fn can(
        &self,
        process: Option<Process>,
        material: Option<&str>,
        thickness: Option<f64>,
    ) -> bool {
        if process.is_none() && material.is_none() {
            // Nothing asked for: every shop, machines listed or not.
            return true;
        }
        self.offers.iter().any(|o| {
            process.is_none_or(|p| o.process == p)
                && match material {
                    None => true,
                    // (No thickness said for a material: any.)
                    Some(m) => o.materials.iter().any(|(name, max)| {
                        name.eq_ignore_ascii_case(m)
                            && thickness.is_none_or(|t| *max <= 0.0 || t <= *max)
                    }),
                }
        })
    }

    /// Whether every word of `query` is in its name, tagline, description, places, processes
    /// or materials (any case). An empty query matches.
    pub fn matches(&self, query: &str) -> bool {
        let mut text = format!("{} {} {}", self.name(), self.tagline, self.about);
        for place in &self.places {
            if let Some(region) = &place.region {
                text.push(' ');
                text.push_str(&region.label());
            }
        }
        for offer in &self.offers {
            text.push(' ');
            text.push_str(offer.process.label());
            for (material, _) in &offer.materials {
                text.push(' ');
                text.push_str(material);
            }
        }
        let text = text.to_lowercase();
        query
            .to_lowercase()
            .split_whitespace()
            .all(|word| text.contains(word))
    }

    /// Metres from `near` to the nearest of its places that says where it is.
    pub fn distance_m(&self, near: &Place) -> Option<f64> {
        self.places
            .iter()
            .filter_map(|p| near.distance_m(p))
            .min_by(f64::total_cmp)
    }

    fn trimmed(&self, (about, machines, materials, places): (usize, usize, usize, usize)) -> Self {
        let mut listing = self.clone();
        if listing.about.chars().count() > about {
            listing.about = listing.about.chars().take(about).collect();
            if about > 0 {
                listing.about.push('…');
            }
        }
        listing.offers.truncate(machines);
        for offer in &mut listing.offers {
            offer.materials.truncate(materials);
        }
        listing.places.truncate(places);
        listing
    }
}

/// Signs `listing` with the organization's key, as bytes for the room: whole when it fits in
/// [`MAX_LISTING`], else cut down until it does. `None` only when even its name, tagline and
/// card alone are too large (they have limits of their own, so they aren't).
pub fn sign(key: &SecretKey, listing: &ShopListing) -> Option<Vec<u8>> {
    // Never more than a listing shows, however small the rest is.
    let listing = &listing.trimmed((
        profile::MAX_ABOUT,
        MAX_OFFERS,
        MAX_OFFER_MATERIALS,
        MAX_PLACES,
    ));
    let whole = encode(&SignedBytes::new(key, listing));
    if whole.len() <= MAX_LISTING {
        return Some(whole);
    }
    TRIMS
        .iter()
        .map(|trim| encode(&SignedBytes::new(key, &listing.trimmed(*trim))))
        .find(|bytes| bytes.len() <= MAX_LISTING)
}

fn encode(listing: &SignedBytes<ShopListing>) -> Vec<u8> {
    cadrs_social::codec::encode(listing)
}

/// Why a listing was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not a listing, or its signatures don't hold.
    Forged,
    /// Signed by someone other than the shop its card names.
    NotTheShops,
    /// Larger than a listing may be.
    TooLarge,
    /// Issued too long ago, or in the future.
    Stale,
    /// A shop this directory hasn't heard of, when enough new ones were taken in for now
    /// ([`Directory::hear_paced`]): it says its listing again later.
    NotNow,
    /// Says something a listing can't: a name or line longer than an organization's may be, a
    /// card that isn't an organization's, more machines or places than a listing shows, a
    /// size that isn't a number.
    Invalid,
}

/// A shop as a directory keeps it.
#[derive(Clone, Debug)]
struct Heard {
    /// Its listing as it was signed (what is kept on disk).
    signed: SignedBytes<ShopListing>,
    listing: ShopListing,
    /// When this device last heard it say this, or anything newer (this device's clock).
    heard: u64,
}

/// A listing in the directory's file.
#[derive(Serialize, Deserialize)]
struct Kept {
    listing: SignedBytes<ShopListing>,
    #[serde(default)]
    heard: u64,
}

/// The shops heard of, newest listing for each.
///
/// Anyone can say a listing, under as many keys as they care to make, so what a listing claims
/// decides nothing about what is kept: when the directory is full, the shop this device heard
/// from longest ago goes, by this device's own clock. A shop that is there keeps saying its
/// listing; one made up to push the others out has to keep saying all of them. The shops the
/// app names ([`Directory::protect`]: the user's contacts and own organizations) never go that
/// way.
#[derive(Clone, Debug, Default)]
pub struct Directory {
    /// One that left the marketplace keeps its withdrawal here, and is shown nowhere.
    shops: BTreeMap<AccountId, Heard>,
    protected: std::collections::BTreeSet<AccountId>,
}

/// How many shops a device hadn't heard of it takes in a minute, at the most. Anyone can make
/// up shops; a real one says its listing again a little later, and is taken in then.
pub const NEW_SHOPS_A_MINUTE: f64 = 60.0;
/// How many listings wait to be looked at, at the most (the rest of a flood is dropped).
pub const MAX_HEARD_WAITING: usize = 2000;
/// How many listings are looked at in one go (the app: in one frame): each costs two
/// signature checks.
pub const HEARD_AT_ONCE: usize = 32;

/// The listings heard in the room on their way into a [`Directory`], at a pace a flood can't
/// force: a bounded queue, so many looked at a time, and so many shops not heard of before a
/// minute. What doesn't hold (forged, or not something a listing may say) is dropped without
/// using any of that.
#[derive(Clone, Debug)]
pub struct Intake {
    waiting: std::collections::VecDeque<Vec<u8>>,
    /// How many new shops may still be taken in now (it fills at [`NEW_SHOPS_A_MINUTE`]).
    new_shops: f64,
    /// When that was last filled, in seconds on a steady clock.
    filled_at: f64,
}

impl Default for Intake {
    fn default() -> Self {
        Self {
            waiting: Default::default(),
            // A minute's worth to start with: what the room says on joining is taken in.
            new_shops: NEW_SHOPS_A_MINUTE,
            filled_at: 0.0,
        }
    }
}

impl Intake {
    /// Listings just heard: queued while there is room.
    pub fn queue(&mut self, heard: impl IntoIterator<Item = Vec<u8>>) {
        let room = MAX_HEARD_WAITING.saturating_sub(self.waiting.len());
        self.waiting.extend(heard.into_iter().take(room));
    }

    /// How many wait to be looked at.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Looks at the next [`HEARD_AT_ONCE`] of them: `secs` is a steady clock's seconds, `wall`
    /// the time of day in milliseconds. True when the directory has news.
    pub fn take(&mut self, directory: &mut Directory, secs: f64, wall: u64) -> bool {
        self.new_shops = (self.new_shops
            + (secs - self.filled_at).max(0.0) * NEW_SHOPS_A_MINUTE / 60.0)
            .min(NEW_SHOPS_A_MINUTE);
        self.filled_at = secs;
        let mut news = false;
        for _ in 0..HEARD_AT_ONCE {
            let Some(bytes) = self.waiting.pop_front() else {
                break;
            };
            news |= directory
                .hear_paced(&bytes, wall, &mut self.new_shops)
                .unwrap_or(false);
        }
        news
    }
}

/// A shop found by a search, and how far it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Found<'a> {
    pub listing: &'a ShopListing,
    /// Metres, when both places are known.
    pub distance_m: Option<f64>,
}

/// What a search asks for. The default asks for nothing: every shop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query<'a> {
    /// Words to find in the shop's name, tagline, description, places and what it cuts.
    pub text: &'a str,
    pub process: Option<Process>,
    pub material: Option<&'a str>,
    pub thickness: Option<f64>,
    /// Nearest to here first.
    pub near: Option<&'a Place>,
}

impl ShopListing {
    /// Whether this is something a listing may say: what [`Directory`] believes of one beyond
    /// its signatures. The limits are the organization's own ([`profile`]), so nothing a
    /// well-behaved cadrs signs is refused.
    pub fn valid(&self) -> bool {
        let card = &self.card.value;
        let chars = |text: &str| text.chars().count();
        let size = |x: f64| x.is_finite() && (0.0..=1.0e6).contains(&x);
        card.kind == cadrs_social::contact::Kind::Organization
            && card.valid()
            && (profile::MIN_NAME..=profile::MAX_NAME).contains(&chars(card.name.trim()))
            && chars(&self.tagline) <= profile::MAX_TAGLINE
            && chars(&self.about) <= profile::MAX_ABOUT
            && chars(&self.email) <= profile::MAX_CONTACT
            && self
                .website
                .as_ref()
                .is_none_or(|w| chars(w) <= profile::MAX_CONTACT)
            && self
                .logo
                .iter()
                .all(|l| l.len <= profile::MAX_LOGO_THUMB_BYTES)
            && self
                .logo_full
                .iter()
                .all(|l| l.len <= profile::MAX_LOGO_BYTES)
            && self.publishers.len() <= MAX_PUBLISHERS
            && self.places.len() <= MAX_PLACES
            && self.offers.len() <= MAX_OFFERS
            && self.offers.iter().all(|o| {
                o.envelope.iter().all(|side| size(*side))
                    && o.materials.len() <= MAX_OFFER_MATERIALS
                    && o.materials
                        .iter()
                        .all(|(name, mm)| chars(name) <= crate::MAX_LABEL && size(*mm))
            })
    }
}

impl Directory {
    /// Takes a listing in (as signed bytes from the room) at `now`; true when it was news.
    pub fn hear(&mut self, bytes: &[u8], now: u64) -> Result<bool, Refused> {
        if bytes.len() > MAX_LISTING {
            return Err(Refused::TooLarge);
        }
        let signed: SignedBytes<ShopListing> =
            cadrs_social::codec::decode(bytes).map_err(|_| Refused::Forged)?;
        self.add(signed, now, now, None)
    }

    /// As [`Directory::hear`], taking in a shop this directory hasn't heard of only while
    /// `new_shops` (how many more may be) is at least one, and counting it. Only a listing that
    /// holds in every other way is counted: what is forged or says what a listing can't uses
    /// none of the allowance, so junk doesn't keep real shops out.
    pub fn hear_paced(
        &mut self,
        bytes: &[u8],
        now: u64,
        new_shops: &mut f64,
    ) -> Result<bool, Refused> {
        if bytes.len() > MAX_LISTING {
            return Err(Refused::TooLarge);
        }
        let signed: SignedBytes<ShopListing> =
            cadrs_social::codec::decode(bytes).map_err(|_| Refused::Forged)?;
        self.add(signed, now, now, Some(new_shops))
    }

    /// The shops that are never dropped to make room: the user's contacts and their own
    /// organizations.
    pub fn protect(&mut self, accounts: impl IntoIterator<Item = AccountId>) {
        self.protected = accounts.into_iter().collect();
    }

    fn add(
        &mut self,
        signed: SignedBytes<ShopListing>,
        now: u64,
        heard: u64,
        new_shops: Option<&mut f64>,
    ) -> Result<bool, Refused> {
        let mut listing = signed.open().map_err(|_| Refused::Forged)?;
        let card = listing.card.verify().map_err(|_| Refused::Forged)?;
        if signed.signer != card.account || listing.card.signer != card.account {
            return Err(Refused::NotTheShops);
        }
        if listing.issued > now.saturating_add(CLOCK_SLACK)
            || listing.issued.saturating_add(KEEP_FOR) < now
        {
            return Err(Refused::Stale);
        }
        if !listing.valid() {
            return Err(Refused::Invalid);
        }
        // What a shop says of its own rating proves nothing.
        listing.rating = None;
        let account = card.account;
        // A shop not heard of before, a withdrawal of one included: only so many at a time.
        if let Some(allowance) = new_shops
            && !self.shops.contains_key(&account)
        {
            if *allowance < 1.0 {
                return Err(Refused::NotNow);
            }
            *allowance -= 1.0;
        }
        match self.shops.get_mut(&account) {
            Some(old) if old.listing.issued >= listing.issued => {
                // Nothing new, but it is still there.
                if old.listing.issued == listing.issued {
                    old.heard = old.heard.max(heard);
                }
                Ok(false)
            }
            _ => {
                self.shops.insert(
                    account,
                    Heard {
                        signed,
                        listing,
                        heard,
                    },
                );
                self.prune(now);
                Ok(true)
            }
        }
    }

    /// Forgets listings too old to believe and, beyond [`MAX_SHOPS`], the shops heard from
    /// longest ago that aren't protected. True when any went.
    pub fn prune(&mut self, now: u64) -> bool {
        let before = self.shops.len();
        self.shops
            .retain(|_, h| h.listing.issued.saturating_add(KEEP_FOR) >= now);
        if self.shops.len() > MAX_SHOPS {
            let mut by_heard: Vec<(u64, AccountId)> = self
                .shops
                .iter()
                .filter(|(account, _)| !self.protected.contains(account))
                .map(|(account, h)| (h.heard, *account))
                .collect();
            by_heard.sort_unstable();
            for (_, account) in by_heard.iter().take(self.shops.len() - MAX_SHOPS) {
                self.shops.remove(account);
            }
        }
        self.shops.len() != before
    }

    /// How many shops are listed.
    pub fn len(&self) -> usize {
        self.listings().count()
    }

    pub fn is_empty(&self) -> bool {
        self.listings().next().is_none()
    }

    /// The listing of the shop with this account, if it is listed.
    pub fn get(&self, account: &AccountId) -> Option<&ShopListing> {
        self.shops
            .get(account)
            .map(|h| &h.listing)
            .filter(|l| !l.withdrawn)
    }

    /// Every listing, by account (not the shops that left).
    pub fn listings(&self) -> impl Iterator<Item = &ShopListing> {
        self.shops
            .values()
            .map(|h| &h.listing)
            .filter(|l| !l.withdrawn)
    }

    /// The hashes of the logos the listings name: what is worth keeping of the logos fetched.
    pub fn logos(&self) -> std::collections::BTreeSet<[u8; 32]> {
        self.listings()
            .flat_map(|l| l.logo.iter().chain(l.logo_full.iter()))
            .map(|blob| blob.hash)
            .collect()
    }

    /// The shops that match, nearest to `near` first (those with no place last), then by name.
    pub fn search(&self, query: &Query) -> Vec<Found<'_>> {
        let mut found: Vec<Found> = self
            .listings()
            .filter(|l| l.matches(query.text))
            .filter(|l| l.can(query.process, query.material, query.thickness))
            .map(|listing| Found {
                listing,
                distance_m: query.near.and_then(|near| listing.distance_m(near)),
            })
            .collect();
        found.sort_by(|a, b| {
            let far = |f: &Found| f.distance_m.unwrap_or(f64::INFINITY);
            far(a)
                .total_cmp(&far(b))
                .then_with(|| a.listing.name().cmp(b.listing.name()))
        });
        found
    }

    /// The directory as bytes, for the app's file.
    pub fn save(&self) -> Vec<u8> {
        let kept: Vec<Kept> = self
            .shops
            .values()
            .map(|h| Kept {
                listing: h.signed.clone(),
                heard: h.heard,
            })
            .collect();
        cadrs_social::codec::encode(&kept)
    }

    /// A directory from [`Directory::save`]'s bytes, each listing checked again as it is taken
    /// in at `now` (so stale ones go). `None` when the bytes aren't one.
    pub fn load(bytes: &[u8], now: u64) -> Option<Self> {
        let kept: Vec<Kept> = cadrs_social::codec::decode(bytes).ok()?;
        let mut directory = Self::default();
        for Kept { listing, heard } in kept {
            // Never heard later than now, whatever the file says.
            let _ = directory.add(listing, now, heard.min(now), None);
        }
        Some(directory)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::capability::MaterialLimit;
    use crate::profile::{Address, Location, Logo};
    use cadrs_social::contact::Kind;
    use cadrs_social::keys::Account;

    const NOW: u64 = 1_800_000_000_000;

    fn card(shop: &Account, name: &str, issued: u64) -> Signed<Card> {
        Card {
            account: shop.id(),
            kind: Kind::Organization,
            name: name.into(),
            devices: vec![],
            mailboxes: vec![],
            issued,
        }
        .sign(shop.key())
    }

    fn listing(
        shop: &Account,
        name: &str,
        place: Option<Place>,
        steel_mm: f64,
        issued: u64,
    ) -> ShopListing {
        ShopListing {
            card: card(shop, name, issued),
            tagline: String::new(),
            about: String::new(),
            kinds: Offers {
                manufacturing: true,
                designs: false,
            },
            logo: None,
            logo_full: None,
            places: place.into_iter().collect(),
            offers: vec![Offer {
                process: Process::Waterjet,
                envelope: [3000.0, 1500.0, 0.0],
                materials: vec![("mild steel".into(), steel_mm)],
            }],
            email: String::new(),
            website: None,
            rating: None,
            issued,
            publishers: Vec::new(),
            withdrawn: false,
        }
    }

    fn said(shop: &Account, listing: &ShopListing) -> Vec<u8> {
        sign(shop.key(), listing).unwrap()
    }

    /// A shop that leaves the marketplace says so: it goes from every directory that hears it,
    /// stays gone when its old listing is heard again, and comes back with a newer listing.
    #[test]
    fn a_withdrawal_takes_a_shop_out() {
        let shop = Account::generate();
        let other = Account::generate();
        let mut dir = Directory::default();
        let listed = said(&shop, &listing(&shop, "Cape Cutters", None, 50.0, NOW));
        assert_eq!(dir.hear(&listed, NOW), Ok(true));
        assert_eq!(dir.len(), 1);
        // Only the shop itself can withdraw its listing.
        let forged = said(
            &other,
            &ShopListing::withdrawal(card(&shop, "Cape Cutters", NOW + 1), NOW + 1),
        );
        assert_eq!(dir.hear(&forged, NOW + 1), Err(Refused::NotTheShops));
        assert_eq!(dir.len(), 1);
        let gone = said(
            &shop,
            &ShopListing::withdrawal(card(&shop, "Cape Cutters", NOW + 1), NOW + 1),
        );
        assert_eq!(dir.hear(&gone, NOW + 1), Ok(true));
        assert!(dir.is_empty() && dir.get(&shop.id()).is_none());
        assert!(dir.search(&Query::default()).is_empty());
        // The old listing, heard again from a peer that hasn't caught up, doesn't bring it back;
        // nor does reading the directory from its file.
        assert_eq!(dir.hear(&listed, NOW + 2), Ok(false));
        let read = Directory::load(&dir.save(), NOW + 2).unwrap();
        assert!(read.is_empty());
        let mut read = read;
        assert_eq!(read.hear(&listed, NOW + 2), Ok(false));
        // Listed again, later: back.
        let again = said(&shop, &listing(&shop, "Cape Cutters", None, 50.0, NOW + 3));
        assert_eq!(dir.hear(&again, NOW + 3), Ok(true));
        assert_eq!(dir.len(), 1);
    }

    /// A listing is believed only as far as an organization could say it: signatures don't
    /// make a 300-character name, a person's card, a machine of no size or a rating of the
    /// shop's own a listing.
    #[test]
    fn what_a_listing_cannot_say_is_refused() {
        let shop = Account::generate();
        let good = listing(&shop, "Cape Cutters", None, 50.0, NOW);
        assert!(good.valid());
        let person = Card {
            kind: Kind::Person,
            ..good.card.value.clone()
        }
        .sign(shop.key());
        let bad: Vec<(&str, ShopListing)> = vec![
            (
                "a long name",
                listing(&shop, &"N".repeat(profile::MAX_NAME + 1), None, 50.0, NOW),
            ),
            (
                "a person",
                ShopListing {
                    card: person,
                    ..good.clone()
                },
            ),
            (
                "a long tagline",
                ShopListing {
                    tagline: "T".repeat(profile::MAX_TAGLINE + 1),
                    ..good.clone()
                },
            ),
            (
                "a machine of no size",
                ShopListing {
                    offers: vec![Offer {
                        process: Process::Laser,
                        envelope: [f64::NAN, f64::INFINITY, -1.0],
                        materials: vec![],
                    }],
                    ..good.clone()
                },
            ),
            (
                "a logo too large to be one",
                ShopListing {
                    logo: Some(BlobRef {
                        hash: [1; 32],
                        len: profile::MAX_LOGO_THUMB_BYTES + 1,
                    }),
                    ..good.clone()
                },
            ),
        ];
        let mut dir = Directory::default();
        for (what, listing) in bad {
            assert!(!listing.valid(), "{what}");
            // Signed as it stands (not through `sign`, which would cut it to fit).
            let bytes = encode(&SignedBytes::new(shop.key(), &listing));
            if bytes.len() <= MAX_LISTING {
                assert_eq!(dir.hear(&bytes, NOW), Err(Refused::Invalid), "{what}");
            }
        }
        assert!(dir.is_empty());
        // A rating the shop gives itself is heard and forgotten.
        let boast = ShopListing {
            rating: Some((5.0, 1_000_000)),
            ..good
        };
        assert_eq!(dir.hear(&said(&shop, &boast), NOW), Ok(true));
        assert_eq!(dir.get(&shop.id()).unwrap().rating, None);
    }

    /// Shops made up to fill the directory push out neither a shop that keeps saying it is
    /// there nor one the user's contacts name, whatever dates they claim.
    #[test]
    fn made_up_shops_do_not_push_out_real_ones() {
        let mut dir = Directory::default();
        let (live, contact, quiet) = (
            Account::generate(),
            Account::generate(),
            Account::generate(),
        );
        for (shop, name) in [
            (&live, "Live Shop"),
            (&contact, "A Contact"),
            (&quiet, "Quiet"),
        ] {
            let bytes = said(shop, &listing(shop, name, None, 50.0, NOW));
            assert_eq!(dir.hear(&bytes, NOW), Ok(true));
        }
        dir.protect([contact.id()]);
        // Dated a day ahead to look the newest: not believed at all.
        let spam = Account::generate();
        let ahead = NOW + 23 * 60 * 60 * 1000;
        assert_eq!(
            dir.hear(&said(&spam, &listing(&spam, "Spam", None, 1.0, ahead)), NOW),
            Err(Refused::Stale)
        );
        // Dated as far ahead as clocks may differ, a directory's worth of them, while the live
        // shop says its listing again now and then.
        let live_bytes = said(&live, &listing(&live, "Live Shop", None, 50.0, NOW));
        for i in 0..MAX_SHOPS as u64 {
            let spam = Account::generate();
            let at = NOW + 1 + i;
            let issued = at + CLOCK_SLACK;
            let bytes = said(&spam, &listing(&spam, "Spam", None, 1.0, issued));
            assert_eq!(dir.hear(&bytes, at), Ok(true));
            if i % 1000 == 999 {
                assert_eq!(dir.hear(&live_bytes, at), Ok(false), "nothing new");
            }
        }
        assert!(
            dir.get(&live.id()).is_some(),
            "still there: it kept saying so"
        );
        assert!(
            dir.get(&contact.id()).is_some(),
            "a contact is never dropped"
        );
        assert!(
            dir.get(&quiet.id()).is_none(),
            "heard once, long before the rest"
        );
        assert_eq!(dir.len(), MAX_SHOPS);
        // The quiet shop says its listing again: back, in place of the stalest made-up one.
        let later = NOW + 1_000_000;
        let again = said(&quiet, &listing(&quiet, "Quiet", None, 50.0, later));
        assert_eq!(dir.hear(&again, later), Ok(true));
        assert!(dir.get(&quiet.id()).is_some());
        // When each was last heard survives the file.
        let read = Directory::load(&dir.save(), later).unwrap();
        assert_eq!(read.len(), dir.len());
        assert!(read.get(&live.id()).is_some() && read.get(&quiet.id()).is_some());
    }

    /// Listings come in at a pace: a bounded queue, so many at a time, so many new shops a
    /// minute. Junk (unsigned, forged, not something a listing says) uses none of the new
    /// shops' allowance, so sending it doesn't keep real shops out.
    #[test]
    fn junk_does_not_keep_new_shops_out() {
        let mut dir = Directory::default();
        let mut intake = Intake::default();
        let real = |n: usize| -> Vec<Vec<u8>> {
            (0..n)
                .map(|_| {
                    let shop = Account::generate();
                    said(&shop, &listing(&shop, "A Shop", None, 10.0, NOW))
                })
                .collect()
        };
        // Junk that names a signer nobody heard of: bytes that aren't a listing, a listing
        // signed by someone else than its card's account, and one that says what none can.
        let junk = || -> Vec<Vec<u8>> {
            let (a, b) = (Account::generate(), Account::generate());
            let long = ShopListing {
                tagline: "T".repeat(profile::MAX_TAGLINE + 1),
                ..listing(&a, "Junk", None, 1.0, NOW)
            };
            vec![
                vec![0xc1; 200],
                encode(&SignedBytes::new(
                    b.key(),
                    &listing(&a, "Junk", None, 1.0, NOW),
                )),
                encode(&SignedBytes::new(a.key(), &long)),
            ]
        };
        // The allowance is used up by real shops: a minute's worth, then none.
        let first = NEW_SHOPS_A_MINUTE as usize;
        intake.queue(real(first + 10));
        let mut secs = 0.0;
        while intake.waiting() > 0 {
            intake.take(&mut dir, secs, NOW);
        }
        assert_eq!(dir.len(), first, "no more new shops than a minute's worth");
        // A flood of junk, then one real shop a second later: the junk used nothing, the
        // second's allowance is the shop's.
        for _ in 0..200 {
            intake.queue(junk());
        }
        secs += 1.0;
        let newcomer = Account::generate();
        intake.queue([said(
            &newcomer,
            &listing(&newcomer, "Newcomer", None, 10.0, NOW),
        )]);
        let mut rounds = 0;
        while intake.waiting() > 0 {
            intake.take(&mut dir, secs, NOW);
            rounds += 1;
        }
        assert!(
            dir.get(&newcomer.id()).is_some(),
            "junk kept a real shop out"
        );
        assert_eq!(dir.len(), first + 1);
        // So many at a time: a flood is spread over the frames, not looked at in one.
        assert_eq!(rounds, (600 + 1usize).div_ceil(HEARD_AT_ONCE));
        // A shop already known is news whenever it says something new, allowance or none.
        assert!(intake.new_shops < 1.0);
        let again = said(
            &newcomer,
            &listing(&newcomer, "Newcomer Ltd", None, 10.0, NOW + 1),
        );
        intake.queue([again]);
        assert!(intake.take(&mut dir, secs, NOW + 1));
        assert_eq!(dir.get(&newcomer.id()).unwrap().name(), "Newcomer Ltd");
        // One not heard of yet waits its turn: refused for now, taken in when it says so again.
        let late = Account::generate();
        let late_bytes = said(&late, &listing(&late, "Late", None, 10.0, NOW));
        let mut none = 0.0;
        assert_eq!(
            dir.hear_paced(&late_bytes, NOW, &mut none),
            Err(Refused::NotNow)
        );
        intake.queue([late_bytes]);
        assert!(intake.take(&mut dir, secs + 2.0, NOW));
        assert!(dir.get(&late.id()).is_some());
        // The queue has a ceiling: the rest of a flood is dropped, not kept.
        intake.queue((0..MAX_HEARD_WAITING + 500).map(|_| vec![0u8; 8]));
        assert_eq!(intake.waiting(), MAX_HEARD_WAITING);
    }

    /// A shop's designs are those of the devices that say, each with its own key, that they
    /// publish for it. Naming someone else's device proves nothing: only that device can sign.
    #[test]
    fn only_a_device_itself_says_it_publishes_for_a_shop() {
        let shop = Account::generate();
        let (own_device, famous) = (SecretKey::generate(), SecretKey::generate());
        let other_shop = Account::generate();
        let mut l = listing(&shop, "Open Gantry", None, 10.0, NOW);
        let says = |org: &Account, issued| Publishes {
            org: org.id(),
            issued,
        };
        l.publishers = vec![
            // Its own device, saying so.
            Signed::new(&own_device, says(&shop, NOW)),
            // A famous publisher's word, but for another organization: copied from its listing.
            Signed::new(&famous, says(&other_shop, NOW)),
        ];
        assert_eq!(l.publishing_devices(), vec![own_device.public()]);
        // A statement tampered to name this shop doesn't hold.
        let mut forged = Signed::new(&famous, says(&other_shop, NOW));
        forged.value.org = shop.id();
        l.publishers.push(forged);
        assert_eq!(l.publishing_devices(), vec![own_device.public()]);
        // Nor does what a device said for an earlier listing: it may have left since.
        l.publishers
            .push(Signed::new(&famous, says(&shop, NOW - 1)));
        assert_eq!(l.publishing_devices(), vec![own_device.public()]);
        l.publishers.truncate(2);
        // It travels with the listing, and too many are not a listing.
        let mut dir = Directory::default();
        assert_eq!(dir.hear(&said(&shop, &l), NOW), Ok(true));
        assert_eq!(
            dir.get(&shop.id()).unwrap().publishing_devices(),
            vec![own_device.public()]
        );
        l.publishers = (0..MAX_PUBLISHERS + 1)
            .map(|_| Signed::new(&SecretKey::generate(), says(&shop, NOW)))
            .collect();
        assert!(!l.valid());
    }

    #[test]
    fn nearest_shop_that_can_cut_it_first() {
        let (a, b, c) = (
            Account::generate(),
            Account::generate(),
            Account::generate(),
        );
        let mut dir = Directory::default();
        let stellenbosch = Place::earth(-33.93, 18.86);
        let joburg = Place::earth(-26.20, 28.05);
        let cape_town = Place::earth(-33.92, 18.42);
        for (shop, name, place, mm) in [
            (&a, "Joburg Jet", Some(joburg), 100.0),
            (&b, "Cape Cutters", Some(cape_town), 50.0),
            (&c, "Nowhere Laser", None, 100.0),
        ] {
            dir.hear(&said(shop, &listing(shop, name, place, mm, NOW)), NOW)
                .unwrap();
        }
        let steel = |thickness| Query {
            near: Some(&stellenbosch),
            material: Some("Mild Steel"),
            thickness: Some(thickness),
            ..Default::default()
        };
        let found = dir.search(&steel(10.0));
        let names: Vec<&str> = found.iter().map(|f| f.listing.name()).collect();
        assert_eq!(names, ["Cape Cutters", "Joburg Jet", "Nowhere Laser"]);
        assert!(found[0].distance_m.unwrap() < 50_000.0);
        // 80 mm steel: Cape Cutters stops at 50.
        let thick = dir.search(&steel(80.0));
        assert_eq!(thick.len(), 2);
        assert_eq!(thick[0].listing.name(), "Joburg Jet");
        assert!(
            dir.search(&Query {
                process: Some(Process::Laser),
                ..Default::default()
            })
            .is_empty()
        );
        // By words: the name, what it cuts, in any case and order.
        let words = |text| {
            dir.search(&Query {
                text,
                ..Default::default()
            })
        };
        assert_eq!(words("cape").len(), 1);
        assert_eq!(words("WATERJET steel").len(), 3);
        assert_eq!(words("cape titanium").len(), 0);
        assert_eq!(words("  ").len(), 3);
    }

    #[test]
    fn a_shop_with_several_places_is_as_near_as_its_nearest() {
        let shop = Account::generate();
        let mut two = listing(&shop, "Two Sites", None, 10.0, NOW);
        two.places = vec![Place::earth(-26.20, 28.05), Place::earth(-33.92, 18.42)];
        let stellenbosch = Place::earth(-33.93, 18.86);
        assert!(two.distance_m(&stellenbosch).unwrap() < 50_000.0);
        // A place that only names its region gives no distance, and isn't an error.
        two.places = vec![Place {
            frame: None,
            ..Place::earth(0.0, 0.0)
        }];
        assert_eq!(two.distance_m(&stellenbosch), None);
    }

    #[test]
    fn a_shop_with_no_machines_listed_still_shows_when_nothing_is_asked_for() {
        let shop = Account::generate();
        let mut bare = listing(&shop, "Bare", None, 10.0, NOW);
        bare.offers.clear();
        let mut dir = Directory::default();
        dir.hear(&said(&shop, &bare), NOW).unwrap();
        assert_eq!(dir.search(&Query::default()).len(), 1);
        assert!(
            dir.search(&Query {
                material: Some("steel"),
                ..Default::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn newer_listings_replace_older_and_forgeries_are_refused() {
        let shop = Account::generate();
        let mut dir = Directory::default();
        let old = said(&shop, &listing(&shop, "Old", None, 10.0, NOW));
        assert_eq!(dir.hear(&old, NOW), Ok(true));
        assert_eq!(dir.hear(&old, NOW), Ok(false));
        let new = said(&shop, &listing(&shop, "New", None, 10.0, NOW + 1));
        assert_eq!(dir.hear(&new, NOW), Ok(true));
        assert_eq!(dir.get(&shop.id()).unwrap().name(), "New");
        assert_eq!(dir.hear(&old, NOW), Ok(false), "the older one again");
        // Someone else signing a listing that carries the shop's card.
        let thief = Account::generate();
        let stolen = said(&thief, &listing(&shop, "Shop", None, 10.0, NOW + 2));
        assert_eq!(dir.hear(&stolen, NOW), Err(Refused::NotTheShops));
        assert_eq!(dir.hear(b"junk", NOW), Err(Refused::Forged));
        assert_eq!(
            dir.hear(&vec![0; MAX_LISTING + 1], NOW),
            Err(Refused::TooLarge)
        );
        // The file round-trips, signatures and all.
        let back = Directory::load(&dir.save(), NOW).unwrap();
        assert_eq!(back.get(&shop.id()), dir.get(&shop.id()));
        assert!(Directory::load(b"junk", NOW).is_none());
    }

    #[test]
    fn stale_listings_go() {
        let (a, b) = (Account::generate(), Account::generate());
        let mut dir = Directory::default();
        dir.hear(&said(&a, &listing(&a, "Then", None, 10.0, NOW)), NOW)
            .unwrap();
        let later = NOW + KEEP_FOR + 1;
        dir.hear(&said(&b, &listing(&b, "Now", None, 10.0, later)), later)
            .unwrap();
        // Taking the new one in pruned the old.
        assert_eq!(dir.len(), 1);
        assert!(dir.get(&a.id()).is_none());
        // One issued long ago, or far in the future, isn't taken at all.
        let ancient = said(&a, &listing(&a, "Then", None, 10.0, NOW));
        assert_eq!(dir.hear(&ancient, later), Err(Refused::Stale));
        let future = said(
            &a,
            &listing(&a, "Soon", None, 10.0, later + 2 * CLOCK_SLACK),
        );
        assert_eq!(dir.hear(&future, later), Err(Refused::Stale));
        // And reading the file later drops what went stale meanwhile.
        let much_later = later + KEEP_FOR + 1;
        assert!(Directory::load(&dir.save(), much_later).unwrap().is_empty());
    }

    /// An organization with everything a listing needs, and its key.
    pub(crate) fn complete_shop() -> (crate::session::Session, Account, SecretKey) {
        let owner = Account::generate();
        let key = SecretKey::generate();
        let (mut session, org) = crate::session::Session::found(
            key.clone(),
            owner.certify(key.public(), "laptop", 0),
            "Cape Waterjet",
            1_000,
        );
        let workshop = crate::new_id();
        let ops = [
            crate::Op::SetLocation {
                id: workshop,
                location: Some(Location {
                    name: "Workshop".into(),
                    production: true,
                    address: Address {
                        line1: "12 Plankenbrug Road".into(),
                        city: "Stellenbosch".into(),
                        country: "ZA".into(),
                        ..Default::default()
                    },
                    point: Some((-33.935712, 18.861123)),
                    public: true,
                    public_accuracy_m: 5_000.0,
                    ..Default::default()
                }),
            },
            crate::Op::SetMachine {
                id: crate::new_id(),
                machine: Some(crate::capability::MachineCaps {
                    name: "Flow Mach 500".into(),
                    process: Process::Waterjet,
                    envelope: [3000.0, 1500.0, 0.0],
                    materials: vec![MaterialLimit {
                        material: "mild steel".into(),
                        max_thickness: 100.0,
                        speeds: Vec::new(),
                    }],
                    kerf: 1.0,
                    min_hole_ratio: 0.5,
                    tolerance: 0.1,
                    pierce_seconds: 2.0,
                    location: Some(workshop),
                }),
            },
            crate::Op::SetProfile(crate::Profile {
                name: "Cape Waterjet".into(),
                tagline: "Waterjet cutting in steel, aluminium and glass".into(),
                about: "We cut what lasers can't.".into(),
                logo: Some(Logo {
                    full: BlobRef::of(b"a 512 px logo"),
                    thumb: BlobRef::of(b"a 128 px logo"),
                }),
                offers: Offers {
                    manufacturing: true,
                    designs: false,
                },
                email: "sales@capewaterjet.example".into(),
                website: None,
                listed: true,
            }),
        ];
        for (i, op) in ops.into_iter().enumerate() {
            session.write(op, 2_000 + i as u64).unwrap();
        }
        (session, org, key)
    }

    #[test]
    fn a_listing_never_gives_away_the_street() {
        let (session, org, _) = complete_shop();
        let listing =
            ShopListing::of(session.org(), card(&org, "Cape Waterjet", 1), 3_000).unwrap();
        let street = Place::earth(-33.935712, 18.861123);
        let shown = &listing.places[0];
        assert_ne!(shown.frame, street.frame);
        assert!(street.distance_m(shown).unwrap() < 5_000.0);
        assert_eq!(
            listing.where_label().as_deref(),
            Some("Stellenbosch, South Africa")
        );
        assert_eq!(listing.processes(), [Process::Waterjet]);
        assert_eq!(listing.logo, Some(BlobRef::of(b"a 128 px logo")));
        // Signed, it says nothing of the street either.
        let bytes = sign(org.key(), &listing).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("Plankenbrug"));
    }

    #[test]
    fn only_a_complete_profile_that_asks_is_listed() {
        let (mut session, org, _) = complete_shop();
        let card = card(&org, "Cape Waterjet", 1);
        assert!(ShopListing::of(session.org(), card.clone(), 3_000).is_some());
        // Not asking: not listed.
        let unlisted = crate::Profile {
            listed: false,
            ..session.org().profile.clone()
        };
        session
            .write(crate::Op::SetProfile(unlisted.clone()), 4_000)
            .unwrap();
        assert!(ShopListing::of(session.org(), card.clone(), 5_000).is_none());
        // Asking, without a logo: not listed, and it says what is missing.
        let no_logo = crate::Profile {
            listed: true,
            logo: None,
            ..unlisted
        };
        session
            .write(crate::Op::SetProfile(no_logo), 6_000)
            .unwrap();
        assert!(ShopListing::of(session.org(), card, 7_000).is_none());
        assert_eq!(profile::missing(session.org()), [profile::Missing::Logo]);
    }

    /// A long description and many machines are cut down to fit the room's message, not dropped.
    #[test]
    fn a_large_listing_is_trimmed_to_fit() {
        let shop = Account::generate();
        let mut big = listing(&shop, "Everything Engineering", None, 10.0, NOW);
        big.tagline = "x".repeat(profile::MAX_TAGLINE);
        big.about = "We make a great many things. ".repeat(60);
        big.offers = (0..40)
            .map(|i| Offer {
                process: Process::Laser,
                envelope: [3000.0, 1500.0, 0.0],
                materials: (0..12)
                    .map(|m| (format!("material number {m} of machine {i}"), 10.0))
                    .collect(),
            })
            .collect();
        big.places = (0..20).map(|i| Place::earth(i as f64, i as f64)).collect();
        let bytes = sign(shop.key(), &big).unwrap();
        assert!(bytes.len() <= MAX_LISTING, "{}", bytes.len());
        let mut dir = Directory::default();
        assert_eq!(dir.hear(&bytes, NOW), Ok(true));
        let heard = dir.get(&shop.id()).unwrap();
        // The name and tagline are whole; the rest is what fitted.
        assert_eq!(heard.name(), "Everything Engineering");
        assert_eq!(heard.tagline, big.tagline);
        assert!(!heard.offers.is_empty() && heard.offers.len() < 40);
        // A small one goes whole.
        let small = listing(&shop, "Small", None, 10.0, NOW + 1);
        let bytes = sign(shop.key(), &small).unwrap();
        dir.hear(&bytes, NOW).unwrap();
        assert_eq!(dir.get(&shop.id()), Some(&small));
    }

    /// A listing with fields a newer cadrs added is still read (and still verifies).
    #[test]
    fn a_newer_cadrs_listing_is_read() {
        #[derive(Serialize, Deserialize)]
        struct FutureListing {
            card: Signed<Card>,
            tagline: String,
            issued: u64,
            certifications: Vec<String>,
        }
        impl Growing for FutureListing {
            const CONTEXT: &'static str = <ShopListing as Growing>::CONTEXT;
        }
        let shop = Account::generate();
        let bytes = cadrs_social::codec::encode(&SignedBytes::new(
            shop.key(),
            &FutureListing {
                card: card(&shop, "Tomorrow's Shop", NOW),
                tagline: "ISO 9001".into(),
                issued: NOW,
                certifications: vec!["ISO 9001:2015".into()],
            },
        ));
        let mut dir = Directory::default();
        assert_eq!(dir.hear(&bytes, NOW), Ok(true));
        let heard = dir.get(&shop.id()).unwrap();
        assert_eq!(heard.tagline, "ISO 9001");
        assert!(heard.offers.is_empty());
    }
}
