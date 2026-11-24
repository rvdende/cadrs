//! An organization's public face and where it is: its [`Profile`] (name, logo, what it offers)
//! and its [`Location`]s.
//!
//! An organization has no place of its own. It has as many locations as it works from: a
//! one-person waterjet shop has one, a firm with two factories and a showroom three, and
//! someone who sells designs from a laptop none at all. What is made where, and where clients
//! collect, hangs off a location; the marketplace shows the ones marked public, no more
//! precisely than each says.
//!
//! [`missing`] says what a profile still lacks before the organization can be listed in the
//! marketplace: the listing is what strangers judge a shop by, so it asks for more than a name.

use cadrs_social::place::{Body, Frame, Place, Region};
use serde::{Deserialize, Serialize};

use crate::{Id, Org};

/// The longest name, in characters.
pub const MAX_NAME: usize = 60;
/// The shortest.
pub const MIN_NAME: usize = 2;
/// The longest tagline: one line under the name on a card.
pub const MAX_TAGLINE: usize = 150;
/// The longest description.
pub const MAX_ABOUT: usize = 2000;
/// The longest e-mail address or website.
pub const MAX_CONTACT: usize = 254;
/// The longest line of an address, a phone number or a location's name.
pub const MAX_LINE: usize = 120;
/// A logo's side in pixels: square, kept at this size.
pub const LOGO_SIDE: u32 = 512;
/// The side of the small logo the marketplace's cards show.
pub const LOGO_THUMB_SIDE: u32 = 128;
/// The largest logo file kept (a 512 px PNG).
pub const MAX_LOGO_BYTES: u64 = 1024 * 1024;
/// The largest small logo: anyone browsing the marketplace fetches it.
pub const MAX_LOGO_THUMB_BYTES: u64 = 48 * 1024;
/// The most locations an organization keeps.
pub const MAX_LOCATIONS: usize = 200;

/// Bytes kept beside the document ([`crate::blob`]), by their BLAKE3 hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlobRef {
    pub hash: [u8; 32],
    pub len: u64,
}

impl BlobRef {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            hash: *blake3::hash(bytes).as_bytes(),
            len: bytes.len() as u64,
        }
    }
}

/// An organization's logo: a square PNG [`LOGO_SIDE`] px a side, and the same at
/// [`LOGO_THUMB_SIDE`] px for lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logo {
    pub full: BlobRef,
    pub thumb: BlobRef,
}

/// What an organization offers. Both, either or (not yet decided) neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offers {
    /// Makes parts for clients: takes requests for quotes, so it needs somewhere it makes them.
    #[serde(default)]
    pub manufacturing: bool,
    /// Publishes designs (documents others can open, fork or buy): needs no location.
    #[serde(default)]
    pub designs: bool,
}

/// The organization's public face.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// One line that says what it does, shown under its name ([`MAX_TAGLINE`]).
    #[serde(default)]
    pub tagline: String,
    /// The longer description on its page.
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub logo: Option<Logo>,
    #[serde(default)]
    pub offers: Offers,
    /// Where clients write to.
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub website: Option<String>,
    /// Asks to be shown in the marketplace (it is once nothing is [`missing`]). Whether it is
    /// shown or not, an organization that manufactures takes requests for quotes from whoever
    /// has its link, up to what may wait unanswered ([`crate::MAX_WAITING`]).
    #[serde(default)]
    pub listed: bool,
}

/// A change to some of a [`Profile`]: each part that is `Some` is set, the rest is left as it
/// is ([`crate::Op::EditProfile`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileEdit {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tagline: Option<String>,
    #[serde(default)]
    pub about: Option<String>,
    /// A new logo (a logo is replaced, never taken away).
    #[serde(default)]
    pub logo: Option<Logo>,
    #[serde(default)]
    pub offers: Option<Offers>,
    #[serde(default)]
    pub email: Option<String>,
    /// The website; an empty one for none.
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub listed: Option<bool>,
}

impl ProfileEdit {
    /// What changed from `before` to `after`.
    pub fn between(before: &Profile, after: &Profile) -> Self {
        fn changed<T: Clone + PartialEq>(before: &T, after: &T) -> Option<T> {
            (before != after).then(|| after.clone())
        }
        Self {
            name: changed(&before.name, &after.name),
            tagline: changed(&before.tagline, &after.tagline),
            about: changed(&before.about, &after.about),
            logo: after.logo.filter(|logo| before.logo != Some(*logo)),
            offers: changed(&before.offers, &after.offers),
            email: changed(&before.email, &after.email),
            website: (before.website != after.website)
                .then(|| after.website.clone().unwrap_or_default()),
            listed: changed(&before.listed, &after.listed),
        }
    }

    /// Whether it changes nothing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn apply(&self, profile: &mut Profile) {
        fn set<T: Clone>(to: &mut T, from: &Option<T>) {
            if let Some(from) = from {
                *to = from.clone();
            }
        }
        set(&mut profile.name, &self.name);
        set(&mut profile.tagline, &self.tagline);
        set(&mut profile.about, &self.about);
        set(&mut profile.offers, &self.offers);
        set(&mut profile.email, &self.email);
        set(&mut profile.listed, &self.listed);
        if let Some(logo) = self.logo {
            profile.logo = Some(logo);
        }
        if let Some(website) = &self.website {
            profile.website = Some(website.trim().to_owned()).filter(|w| !w.is_empty());
        }
    }
}

impl Profile {
    /// Why this profile can't be kept, if it can't: the limits every peer checks.
    pub fn problem(&self) -> Option<&'static str> {
        let name = self.name.trim().chars().count();
        if !(MIN_NAME..=MAX_NAME).contains(&name) {
            return Some("a name is 2 to 60 characters");
        }
        if self.tagline.chars().count() > MAX_TAGLINE {
            return Some("a tagline is at most 150 characters");
        }
        if self.about.chars().count() > MAX_ABOUT {
            return Some("a description is at most 2000 characters");
        }
        if self.email.chars().count() > MAX_CONTACT
            || self
                .website
                .as_ref()
                .is_some_and(|w| w.chars().count() > MAX_CONTACT)
        {
            return Some("an address is at most 254 characters");
        }
        if !self.email.is_empty() && !plausible_email(&self.email) {
            return Some("that is not an e-mail address");
        }
        if let Some(logo) = &self.logo
            && (logo.full.len > MAX_LOGO_BYTES || logo.thumb.len > MAX_LOGO_THUMB_BYTES)
        {
            return Some("the logo is too large");
        }
        None
    }
}

/// `a@b.c`, loosely: one `@`, something either side, a dot after it, no spaces.
pub fn plausible_email(text: &str) -> bool {
    let Some((who, host)) = text.split_once('@') else {
        return false;
    };
    !who.is_empty()
        && !host.contains('@')
        && host.split('.').count() >= 2
        && host.split('.').all(|part| !part.is_empty())
        && !text.contains(char::is_whitespace)
}

/// A postal address. Every line may be empty.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    #[serde(default)]
    pub line1: String,
    #[serde(default)]
    pub line2: String,
    #[serde(default)]
    pub city: String,
    /// The state, province or county.
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub postal_code: String,
    /// The country's ISO 3166-1 alpha-2 code ("ZA"), or empty ([`crate::country`]).
    #[serde(default)]
    pub country: String,
}

impl Address {
    /// "Stellenbosch, Western Cape, South Africa": what a card shows.
    pub fn short(&self) -> String {
        let country = crate::country::name(&self.country).unwrap_or("");
        [self.city.as_str(), self.region.as_str(), country]
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn lines(&self) -> [&String; 5] {
        [
            &self.line1,
            &self.line2,
            &self.city,
            &self.region,
            &self.postal_code,
        ]
    }
}

/// Somewhere an organization works from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Location {
    /// "Workshop", "Cape Town warehouse".
    pub name: String,
    /// Parts are made here: machines stand here.
    #[serde(default)]
    pub production: bool,
    /// Clients collect orders here.
    #[serde(default)]
    pub pickup: bool,
    #[serde(default)]
    pub address: Address,
    /// Latitude and longitude in degrees, when given: what "shops near me" measures from.
    #[serde(default)]
    pub point: Option<(f64, f64)>,
    #[serde(default)]
    pub phone: String,
    /// Shown in the marketplace, its point no more precise than [`Location::public_accuracy_m`].
    #[serde(default)]
    pub public: bool,
    /// How precisely the marketplace shows the point, in metres (never under 1 km).
    #[serde(default)]
    pub public_accuracy_m: f64,
}

/// The least a public point is blurred by, in metres.
pub const MIN_PUBLIC_ACCURACY_M: f64 = 1_000.0;

impl Location {
    /// Why this location can't be kept, if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        let name = self.name.trim().chars().count();
        if name == 0 || name > MAX_NAME {
            return Some("a location's name is 1 to 60 characters");
        }
        if self
            .address
            .lines()
            .iter()
            .any(|l| l.chars().count() > MAX_LINE)
            || self.phone.chars().count() > MAX_LINE
        {
            return Some("a line is at most 120 characters");
        }
        if !self.address.country.is_empty() && crate::country::name(&self.address.country).is_none()
        {
            return Some("that is not a country's code");
        }
        if let Some((lat, lon)) = self.point
            && !(lat.is_finite()
                && lon.is_finite()
                && (-90.0..=90.0).contains(&lat)
                && (-180.0..=180.0).contains(&lon))
        {
            return Some("a latitude is -90 to 90 and a longitude -180 to 180");
        }
        if !self.public_accuracy_m.is_finite() || self.public_accuracy_m < 0.0 {
            return Some("an accuracy is zero or more");
        }
        None
    }

    /// Where it is, exactly: for the organization's own eyes and its contacts'.
    pub fn place(&self) -> Place {
        let region = Region {
            country: crate::country::name(&self.address.country).map(str::to_owned),
            area: some(&self.address.region),
            city: some(&self.address.city),
        };
        Place {
            body: Body::Earth,
            frame: self.point.map(|(lat, lon)| Frame::Surface {
                lat,
                lon,
                alt_m: 0.0,
            }),
            accuracy_m: None,
            region: (region != Region::default()).then_some(region),
        }
    }

    /// Where the marketplace says it is, if it is public: no street, the point blurred.
    pub fn public_place(&self) -> Option<Place> {
        self.public.then(|| {
            self.place()
                .coarsened(self.public_accuracy_m.max(MIN_PUBLIC_ACCURACY_M))
        })
    }
}

fn some(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Something a profile lacks before the organization can be listed in the marketplace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Missing {
    Logo,
    Tagline,
    /// What it offers: manufacturing, designs or both.
    Offers,
    Email,
    /// It manufactures: somewhere it makes things, with a country.
    ProductionLocation,
    /// It manufactures: a machine, so a request can be matched to it.
    Machine,
}

/// What `org` still lacks before it can be listed, most basic first. Empty: nothing.
pub fn missing(org: &Org) -> Vec<Missing> {
    let profile = &org.profile;
    let mut missing = Vec::new();
    if profile.logo.is_none() {
        missing.push(Missing::Logo);
    }
    if profile.tagline.trim().is_empty() {
        missing.push(Missing::Tagline);
    }
    if profile.offers == Offers::default() {
        missing.push(Missing::Offers);
    }
    if profile.email.trim().is_empty() {
        missing.push(Missing::Email);
    }
    if profile.offers.manufacturing {
        let made_somewhere = org
            .locations
            .values()
            .any(|l| l.production && !l.address.country.is_empty());
        if !made_somewhere {
            missing.push(Missing::ProductionLocation);
        }
        if org.machines.is_empty() {
            missing.push(Missing::Machine);
        }
    }
    missing
}

impl Org {
    /// Whether the organization is shown in the marketplace: it asked to be, and lacks nothing.
    pub fn listed(&self) -> bool {
        self.profile.listed && missing(self).is_empty()
    }

    /// The location with this id.
    pub fn location(&self, id: Option<Id>) -> Option<&Location> {
        self.locations.get(&id?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails() {
        for ok in ["a@b.c", "sales@cape-waterjet.co.za"] {
            assert!(plausible_email(ok), "{ok}");
        }
        for bad in ["", "a", "a@b", "@b.c", "a@.c", "a b@c.d", "a@b@c.d", "a@b."] {
            assert!(!plausible_email(bad), "{bad}");
        }
    }

    #[test]
    fn a_profiles_limits() {
        let ok = Profile {
            name: "Cape Waterjet".into(),
            email: "sales@capewaterjet.example".into(),
            ..Default::default()
        };
        assert_eq!(ok.problem(), None);
        let unnamed = Profile::default();
        assert!(unnamed.problem().is_some());
        let wordy = Profile {
            tagline: "x".repeat(MAX_TAGLINE + 1),
            ..ok.clone()
        };
        assert!(wordy.problem().is_some());
        let heavy = Profile {
            logo: Some(Logo {
                full: BlobRef {
                    hash: [0; 32],
                    len: MAX_LOGO_BYTES + 1,
                },
                thumb: BlobRef {
                    hash: [0; 32],
                    len: 10,
                },
            }),
            ..ok
        };
        assert!(heavy.problem().is_some());
    }

    #[test]
    fn a_public_location_shows_no_street_and_a_blurred_point() {
        let workshop = Location {
            name: "Workshop".into(),
            production: true,
            address: Address {
                line1: "12 Plankenbrug Road".into(),
                city: "Stellenbosch".into(),
                country: "ZA".into(),
                ..Default::default()
            },
            point: Some((-33.93571, 18.86112)),
            public: true,
            public_accuracy_m: 0.0,
            ..Default::default()
        };
        assert_eq!(workshop.problem(), None);
        assert_eq!(workshop.address.short(), "Stellenbosch, South Africa");
        let exact = workshop.place();
        let public = workshop.public_place().unwrap();
        // Never more precise than a kilometre, whatever was asked.
        assert_eq!(public.accuracy_m, Some(MIN_PUBLIC_ACCURACY_M));
        assert_ne!(public.frame, exact.frame);
        assert!(exact.distance_m(&public).unwrap() < MIN_PUBLIC_ACCURACY_M);
        assert_eq!(public.region.unwrap().label(), "Stellenbosch, South Africa");
        // A private one isn't shown at all.
        let private = Location {
            public: false,
            ..workshop.clone()
        };
        assert_eq!(private.public_place(), None);
        // Nonsense is refused.
        let nowhere = Location {
            point: Some((91.0, 0.0)),
            ..workshop.clone()
        };
        assert!(nowhere.problem().is_some());
        let atlantis = Location {
            address: Address {
                country: "XX".into(),
                ..Default::default()
            },
            ..workshop
        };
        assert!(atlantis.problem().is_some());
    }
}
