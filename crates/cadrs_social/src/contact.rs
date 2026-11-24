//! Contact cards: what a profile's QR code and link carry, so two people (or a person and a
//! shop) can add each other.
//!
//! A card names the account, says whether it is a person or an organization, and lists the
//! devices to reach it at (for a shop: its always-on box and its owners' computers). The
//! account signs it, so a card passed along can't be changed. As text it is a link,
//! `https://cadrs.net/c/<base32>`, which a phone's camera app opens and cadrs reads.

use serde::{Deserialize, Serialize};

use crate::keys::{AccountId, DeviceId, SecretKey, Signed, Statement};

/// Where cards point: the site that opens cadrs.
pub const LINK_PREFIX: &str = "https://cadrs.net/c/";

/// A person or an organization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Person,
    Organization,
}

/// Who someone is and how to reach them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub account: AccountId,
    pub kind: Kind,
    pub name: String,
    /// The devices to reach the account at, most likely online first.
    pub devices: Vec<DeviceId>,
    /// Mailboxes the account collects from: where to leave sealed letters while it is offline.
    pub mailboxes: Vec<DeviceId>,
    /// Milliseconds since the Unix epoch: a newer card replaces an older one.
    pub issued: u64,
}

impl Statement for Card {
    const CONTEXT: &'static str = "cadrs contact card 1";
}

/// The longest name on a card, in characters; the most devices and mailboxes it names. A card
/// comes from whoever hands it over (a link, a request, a listing), so whoever keeps one keeps
/// no more than this.
pub const MAX_NAME: usize = 60;
pub const MAX_DEVICES: usize = 64;
pub const MAX_MAILBOXES: usize = 16;

impl Card {
    /// Whether this is something a card may say: a name of a sensible length, and no more
    /// devices and mailboxes than anyone has.
    pub fn valid(&self) -> bool {
        let name = self.name.trim().chars().count();
        (1..=MAX_NAME).contains(&name)
            && self.devices.len() <= MAX_DEVICES
            && self.mailboxes.len() <= MAX_MAILBOXES
    }
}

/// Why a card couldn't be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    NotACard,
    /// The signature doesn't hold, or isn't the account's.
    Forged,
}

impl Card {
    /// The card, signed by the account's key.
    pub fn sign(self, account: &SecretKey) -> Signed<Card> {
        Signed::new(account, self)
    }
}

/// The card as a link.
pub fn to_link(card: &Signed<Card>) -> String {
    let bytes = postcard::to_allocvec(card).expect("cards encode");
    format!(
        "{LINK_PREFIX}{}",
        data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
    )
}

/// Reads a card from its link (or the bare code after the prefix), checking its signature.
pub fn from_link(text: &str) -> Result<Signed<Card>, Error> {
    let text = text.trim();
    let code = text
        .strip_prefix(LINK_PREFIX)
        .or_else(|| text.strip_prefix("cadrs.net/c/"))
        .unwrap_or(text);
    let bytes = data_encoding::BASE32_NOPAD
        .decode(code.to_uppercase().as_bytes())
        .map_err(|_| Error::NotACard)?;
    let card: Signed<Card> = postcard::from_bytes(&bytes).map_err(|_| Error::NotACard)?;
    let value = card.verify().map_err(|_| Error::Forged)?;
    if card.signer != value.account {
        return Err(Error::Forged);
    }
    if !value.valid() {
        return Err(Error::NotACard);
    }
    Ok(card)
}

/// The people and organizations someone has added, newest card for each.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Contacts {
    pub cards: Vec<Signed<Card>>,
}

impl Contacts {
    /// Adds a card, or replaces an older one of the same account. False when it was no news,
    /// or isn't something a card may say ([`Card::valid`]).
    pub fn add(&mut self, card: Signed<Card>) -> bool {
        if !card.value.valid() {
            return false;
        }
        match self
            .cards
            .iter_mut()
            .find(|c| c.value.account == card.value.account)
        {
            Some(old) if old.value.issued >= card.value.issued => false,
            Some(old) => {
                *old = card;
                true
            }
            None => {
                self.cards.push(card);
                true
            }
        }
    }

    pub fn get(&self, account: &AccountId) -> Option<&Card> {
        self.cards
            .iter()
            .map(|c| &c.value)
            .find(|c| &c.account == account)
    }

    /// The organizations among them.
    pub fn organizations(&self) -> impl Iterator<Item = &Card> {
        self.cards
            .iter()
            .map(|c| &c.value)
            .filter(|c| c.kind == Kind::Organization)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Account;

    fn card(account: &Account, issued: u64) -> Signed<Card> {
        Card {
            account: account.id(),
            kind: Kind::Organization,
            name: "Cape Waterjet".into(),
            devices: vec![SecretKey::generate().public()],
            mailboxes: vec![],
            issued,
        }
        .sign(account.key())
    }

    #[test]
    fn a_card_round_trips_through_its_link() {
        let shop = Account::generate();
        let card = card(&shop, 1);
        let link = to_link(&card);
        assert!(link.starts_with(LINK_PREFIX));
        assert_eq!(link, link.to_lowercase());
        assert_eq!(from_link(&link), Ok(card.clone()));
        // The bare code too, and with the scheme left off.
        let code = link.strip_prefix(LINK_PREFIX).unwrap();
        assert_eq!(from_link(code), Ok(card.clone()));
        assert_eq!(from_link(&format!("cadrs.net/c/{code}")), Ok(card));
    }

    #[test]
    fn a_changed_card_is_refused() {
        let shop = Account::generate();
        let mut card = card(&shop, 1);
        card.value.name = "Somebody Else".into();
        assert_eq!(from_link(&to_link(&card)), Err(Error::Forged));
        // Signed by a key other than the account's.
        let thief = Account::generate();
        let mut stolen = card.clone();
        stolen.value.name = "Cape Waterjet".into();
        let stolen = stolen.value.sign(thief.key());
        assert_eq!(from_link(&to_link(&stolen)), Err(Error::Forged));
        assert_eq!(from_link("hello"), Err(Error::NotACard));
    }

    #[test]
    fn newer_cards_replace_older_ones() {
        let shop = Account::generate();
        let mut contacts = Contacts::default();
        assert!(contacts.add(card(&shop, 1)));
        assert!(!contacts.add(card(&shop, 1)));
        assert!(contacts.add(card(&shop, 2)));
        assert_eq!(contacts.cards.len(), 1);
        assert_eq!(contacts.organizations().count(), 1);
    }

    /// A card says a name and where to reach its account, and no more than that: one that
    /// says more isn't kept, from a link or anywhere else.
    #[test]
    fn a_card_is_only_so_large() {
        let account = Account::generate();
        let with = |name: String, devices: usize| {
            Card {
                account: account.id(),
                kind: Kind::Person,
                name,
                devices: vec![SecretKey::generate().public(); devices],
                mailboxes: vec![],
                issued: 1,
            }
            .sign(account.key())
        };
        let good = with("Alex".into(), 3);
        assert!(good.value.valid());
        assert_eq!(from_link(&to_link(&good)), Ok(good.clone()));
        let mut contacts = Contacts::default();
        assert!(contacts.add(good));
        for bad in [
            with(String::new(), 1),
            with("N".repeat(MAX_NAME + 1), 1),
            with("Many".into(), MAX_DEVICES + 1),
        ] {
            assert!(!bad.value.valid());
            assert_eq!(from_link(&to_link(&bad)), Err(Error::NotACard));
            let mut contacts = Contacts::default();
            assert!(!contacts.add(bad));
            assert!(contacts.cards.is_empty());
        }
    }
}
