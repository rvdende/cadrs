//! The public directory of shops: what a shop that takes requests from anyone says about
//! itself, and searching what was heard by distance and by what the shops can make.
//!
//! A [`ShopListing`] is signed by the organization's own key and carries its contact card, so a
//! designer who finds a shop can send it a request straight away. Its place is coarsened to the
//! accuracy the shop chose ([`cadrs_social::place::Place::coarsened`]): the directory never knows
//! the street.

use std::collections::BTreeMap;

use cadrs_social::contact::Card;
use cadrs_social::keys::{AccountId, Signed, Statement};
use cadrs_social::place::Place;
use serde::{Deserialize, Serialize};

use crate::Org;
use crate::capability::Process;

/// What one machine of a shop can do, as the directory shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    pub process: Process,
    /// The bed or envelope, mm.
    pub envelope: [f64; 3],
    /// (material, thickest it cuts in mm).
    pub materials: Vec<(String, f64)>,
}

/// A shop in the public directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShopListing {
    pub card: Signed<Card>,
    pub about: String,
    pub place: Option<Place>,
    pub offers: Vec<Offer>,
    /// (average stars, how many reviews).
    pub rating: Option<(f64, usize)>,
    /// Milliseconds since the Unix epoch: a newer listing replaces an older one.
    pub issued: u64,
}

impl Statement for ShopListing {
    const CONTEXT: &'static str = "cadrs shop listing 1";
}

impl ShopListing {
    /// The listing for `org`, with its card, as of `now`.
    pub fn of(org: &Org, card: Signed<Card>, now: u64) -> Self {
        let profile = &org.profile;
        Self {
            card,
            about: profile.about.clone(),
            place: profile
                .place
                .as_ref()
                .map(|p| p.coarsened(profile.public_accuracy_m.max(1_000.0))),
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
            rating: org.rating(),
            issued: now,
        }
    }

    pub fn name(&self) -> &str {
        &self.card.value.name
    }

    pub fn account(&self) -> AccountId {
        self.card.value.account
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
                    Some(m) => o.materials.iter().any(|(name, max)| {
                        name.eq_ignore_ascii_case(m) && thickness.is_none_or(|t| t <= *max)
                    }),
                }
        })
    }
}

/// Why a listing was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not a listing, or its signatures don't hold.
    Forged,
    /// Signed by someone other than the shop its card names.
    NotTheShops,
}

/// The shops heard of, newest listing for each.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Directory {
    shops: BTreeMap<AccountId, Signed<ShopListing>>,
}

/// A shop found by a search, and how far it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Found<'a> {
    pub listing: &'a ShopListing,
    /// Metres, when both places are known.
    pub distance_m: Option<f64>,
}

impl Directory {
    /// Takes a listing in (as signed bytes from the room); true when it was news.
    pub fn hear(&mut self, bytes: &[u8]) -> Result<bool, Refused> {
        let listing: Signed<ShopListing> =
            postcard::from_bytes(bytes).map_err(|_| Refused::Forged)?;
        self.add(listing)
    }

    pub fn add(&mut self, listing: Signed<ShopListing>) -> Result<bool, Refused> {
        let value = listing.verify().map_err(|_| Refused::Forged)?;
        let card = value.card.verify().map_err(|_| Refused::Forged)?;
        if listing.signer != card.account || value.card.signer != card.account {
            return Err(Refused::NotTheShops);
        }
        let account = card.account;
        match self.shops.get(&account) {
            Some(old) if old.value.issued >= value.issued => Ok(false),
            _ => {
                self.shops.insert(account, listing);
                Ok(true)
            }
        }
    }

    pub fn len(&self) -> usize {
        self.shops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shops.is_empty()
    }

    pub fn get(&self, account: &AccountId) -> Option<&ShopListing> {
        self.shops.get(account).map(|l| &l.value)
    }

    /// Shops that can do it, nearest to `near` first (those with no place last), then by name.
    pub fn search(
        &self,
        near: Option<&Place>,
        process: Option<Process>,
        material: Option<&str>,
        thickness: Option<f64>,
    ) -> Vec<Found<'_>> {
        let mut found: Vec<Found> = self
            .shops
            .values()
            .map(|l| &l.value)
            .filter(|l| l.can(process, material, thickness))
            .map(|listing| Found {
                listing,
                distance_m: near
                    .zip(listing.place.as_ref())
                    .and_then(|(a, b)| a.distance_m(b)),
            })
            .collect();
        found.sort_by(|a, b| {
            let key = |f: &Found| f.distance_m.unwrap_or(f64::INFINITY);
            key(a)
                .total_cmp(&key(b))
                .then_with(|| a.listing.name().cmp(b.listing.name()))
        });
        found
    }
}

/// The signed listing, as bytes for the room.
pub fn encode(listing: &Signed<ShopListing>) -> Vec<u8> {
    postcard::to_allocvec(listing).expect("listings encode")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_social::contact::Kind;
    use cadrs_social::keys::Account;

    fn listing(
        shop: &Account,
        name: &str,
        place: Option<Place>,
        steel_mm: f64,
        issued: u64,
    ) -> Signed<ShopListing> {
        let card = Card {
            account: shop.id(),
            kind: Kind::Organization,
            name: name.into(),
            devices: vec![],
            mailboxes: vec![],
            issued,
        }
        .sign(shop.key());
        Signed::new(
            shop.key(),
            ShopListing {
                card,
                about: String::new(),
                place,
                offers: vec![Offer {
                    process: Process::Waterjet,
                    envelope: [3000.0, 1500.0, 0.0],
                    materials: vec![("mild steel".into(), steel_mm)],
                }],
                rating: None,
                issued,
            },
        )
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
        dir.hear(&encode(&listing(&a, "Joburg Jet", Some(joburg), 100.0, 1)))
            .unwrap();
        dir.hear(&encode(&listing(
            &b,
            "Cape Cutters",
            Some(cape_town),
            50.0,
            1,
        )))
        .unwrap();
        dir.hear(&encode(&listing(&c, "Nowhere Laser", None, 100.0, 1)))
            .unwrap();
        let found = dir.search(Some(&stellenbosch), None, Some("Mild Steel"), Some(10.0));
        let names: Vec<&str> = found.iter().map(|f| f.listing.name()).collect();
        assert_eq!(names, ["Cape Cutters", "Joburg Jet", "Nowhere Laser"]);
        assert!(found[0].distance_m.unwrap() < 50_000.0);
        // 80 mm steel: Cape Cutters stops at 50.
        let thick = dir.search(Some(&stellenbosch), None, Some("mild steel"), Some(80.0));
        assert_eq!(thick.len(), 2);
        assert_eq!(thick[0].listing.name(), "Joburg Jet");
        assert!(
            dir.search(None, Some(Process::Laser), None, None)
                .is_empty()
        );
    }

    #[test]
    fn a_shop_with_no_machines_listed_still_shows_when_nothing_is_asked_for() {
        let shop = Account::generate();
        let mut bare = listing(&shop, "Bare", None, 10.0, 1);
        bare.value.offers.clear();
        let bare = Signed::new(shop.key(), bare.value);
        let mut dir = Directory::default();
        dir.add(bare).unwrap();
        assert_eq!(dir.search(None, None, None, None).len(), 1);
        assert!(dir.search(None, None, Some("steel"), None).is_empty());
    }

    #[test]
    fn newer_listings_replace_older_and_forgeries_are_refused() {
        let shop = Account::generate();
        let mut dir = Directory::default();
        assert_eq!(
            dir.hear(&encode(&listing(&shop, "Old", None, 10.0, 1))),
            Ok(true)
        );
        assert_eq!(
            dir.hear(&encode(&listing(&shop, "Old", None, 10.0, 1))),
            Ok(false)
        );
        assert_eq!(
            dir.hear(&encode(&listing(&shop, "New", None, 10.0, 2))),
            Ok(true)
        );
        assert_eq!(dir.get(&shop.id()).unwrap().name(), "New");
        // Someone else signing a listing that carries the shop's card.
        let thief = Account::generate();
        let real = listing(&shop, "Shop", None, 10.0, 3);
        let stolen = Signed::new(thief.key(), real.value.clone());
        assert_eq!(dir.hear(&encode(&stolen)), Err(Refused::NotTheShops));
        assert_eq!(dir.hear(b"junk"), Err(Refused::Forged));
    }

    #[test]
    fn a_listing_never_gives_away_the_street() {
        let owner = Account::generate();
        let key = cadrs_social::keys::SecretKey::generate();
        let (mut session, org) = crate::session::Session::found(
            key.clone(),
            owner.certify(key.public(), "laptop", 0),
            "Shop",
            1_000,
        );
        let street = Place::earth(-33.935712, 18.861123);
        session
            .write(
                crate::Op::SetProfile(crate::Profile {
                    name: "Shop".into(),
                    place: Some(street.clone()),
                    public_accuracy_m: 5_000.0,
                    open_for_quotes: true,
                    ..Default::default()
                }),
                2_000,
            )
            .unwrap();
        let card = Card {
            account: org.id(),
            kind: Kind::Organization,
            name: "Shop".into(),
            devices: vec![key.public()],
            mailboxes: vec![],
            issued: 1,
        }
        .sign(org.key());
        let listing = ShopListing::of(session.org(), card, 3_000);
        let shown = listing.place.unwrap();
        assert_ne!(shown.frame, street.frame);
        assert!(street.distance_m(&shown).unwrap() < 5_000.0);
    }
}
