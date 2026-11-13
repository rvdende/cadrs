//! The phone's state and what it does with posts, apart from the page and the network so it can
//! be tested natively.
//!
//! The phone keeps its keys, contacts, the requests for quotes of the account (those it hears
//! of: [`ClientBook::receive_learning`]), receipts and notices under one folder, through
//! `cadrs_fs` (the page's IndexedDB). It joins an account by a pairing code from the account's
//! main device; letters to the account then reach it too.

use std::io;
use std::path::{Path, PathBuf};

use cadrs_org::letters::{self, ClientBook, Composed, Message, News};
use cadrs_social::contact::{Card, Contacts};
use cadrs_social::keys::{AccountId, DeviceCert, DeviceId, Signed};
use cadrs_social::keystore::DeviceKeys;
use cadrs_social::mailbox;
use cadrs_social::pairing::PairingOffer;
use cadrs_social::post::{Collected, Post};
use cadrs_social::receipts::Ledger;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Something to tell the user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    pub text: String,
    /// Milliseconds since the Unix epoch.
    pub when: u64,
}

/// The phone.
pub struct Phone {
    root: PathBuf,
    pub name: String,
    pub keys: DeviceKeys,
    pub contacts: Contacts,
    pub book: ClientBook,
    pub ledger: Ledger,
    pub notices: Vec<Notice>,
    /// Mailboxes it collects from.
    pub mailboxes: Vec<DeviceId>,
    /// The account it asked to join, until the answer comes.
    joining: Option<PairingOffer>,
    /// The account's other devices it knows: its book of requests is shared with them.
    pub siblings: Vec<DeviceId>,
    /// The shops it belongs to.
    pub shops: crate::shop::Shops,
}

const KEY: &str = "identity.key";
const NAME: &str = "name.txt";
const CONTACTS: &str = "contacts.bin";
const BOOK: &str = "requests.bin";
const LEDGER: &str = "receipts.bin";
const NOTICES: &str = "notices.bin";
const SIBLINGS: &str = "siblings.bin";

fn load<T: DeserializeOwned + Default>(root: &Path, file: &str) -> T {
    cadrs_fs::read(root.join(file))
        .ok()
        .and_then(|b| postcard::from_bytes(&b).ok())
        .unwrap_or_default()
}

fn save<T: Serialize>(root: &Path, file: &str, value: &T) {
    let bytes = postcard::to_allocvec(value).expect("encodes");
    let _ = cadrs_fs::create_dir_all(root).and_then(|()| cadrs_fs::write(root.join(file), bytes));
}

impl Phone {
    /// The phone kept in `root` (made there the first time), collecting from `mailboxes`.
    pub fn load(root: &Path, mailboxes: Vec<DeviceId>, now: u64) -> io::Result<Self> {
        let name = cadrs_fs::read_to_string(root.join(NAME)).unwrap_or_default();
        let keys = DeviceKeys::load_or_create(&root.join(KEY), "phone", now)?;
        Ok(Self {
            root: root.to_owned(),
            name,
            contacts: load(root, CONTACTS),
            book: load(root, BOOK),
            ledger: load(root, LEDGER),
            notices: load(root, NOTICES),
            mailboxes,
            joining: None,
            siblings: load(root, SIBLINGS),
            shops: crate::shop::Shops::load(root, &keys.device),
            keys,
        })
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.trim().to_owned();
        let _ = cadrs_fs::create_dir_all(&self.root)
            .and_then(|()| cadrs_fs::write(self.root.join(NAME), &self.name));
    }

    fn save_all(&self) {
        save(&self.root, CONTACTS, &self.contacts);
        save(&self.root, BOOK, &self.book);
        save(&self.root, LEDGER, &self.ledger);
        save(&self.root, NOTICES, &self.notices);
        save(&self.root, SIBLINGS, &self.siblings);
        self.shops.save(&self.root);
    }

    /// The post that shares this phone's book with the account's other devices.
    pub fn mirror_post(&self) -> Vec<u8> {
        Post::Mirror {
            cert: self.keys.cert.clone(),
            book: postcard::to_allocvec(&self.book).expect("encodes"),
        }
        .encode()
    }

    pub fn account(&self) -> AccountId {
        self.keys.account_id()
    }

    /// Whether this phone joined an account (it doesn't hold the account's key).
    pub fn joined(&self) -> bool {
        self.keys.account.is_none()
    }

    /// The user's card, when this phone holds the account's key (a phone that joined an
    /// account shows the card of the account's main device instead).
    pub fn card(&self, now: u64) -> Option<Signed<Card>> {
        let account = self.keys.account.as_ref()?;
        Some(
            Card {
                account: account.id(),
                kind: cadrs_social::contact::Kind::Person,
                name: self.name.clone(),
                devices: vec![self.keys.device_id()],
                mailboxes: self.mailboxes.clone(),
                issued: now,
            }
            .sign(account.key()),
        )
    }

    fn notice(&mut self, text: String, now: u64) {
        self.notices.insert(0, Notice { text, when: now });
        self.notices.truncate(50);
    }

    /// Adds a contact from its link.
    pub fn add_contact(&mut self, link: &str) -> Result<String, String> {
        let card =
            cadrs_social::contact::from_link(link).map_err(|_| "not a cadrs link".to_string())?;
        let name = card.value.name.clone();
        self.contacts.add(card);
        self.save_all();
        Ok(name)
    }

    /// Does what a post asks; its answer. `device` sent it, when it came straight from it.
    pub fn handle(
        &mut self,
        post: Post,
        device: Option<DeviceId>,
        now: u64,
    ) -> Result<Vec<u8>, String> {
        match post {
            Post::Letter(bytes) => {
                let mut ours = self.shops.accounts();
                ours.push(self.account());
                let (letter, from) = letters::read(&bytes, &ours).map_err(|e| format!("{e:?}"))?;
                if letter.to != self.account() {
                    // For one of the shops.
                    let shop = self.shops.take_letter(&letter, from, now)?;
                    self.notice(format!("{shop}: a client wrote"), now);
                    self.save_all();
                    return Ok(Vec::new());
                }
                let shop = self
                    .contacts
                    .get(&from)
                    .map_or_else(|| from.to_string()[..8].to_string(), |c| c.name.clone());
                let news = self
                    .book
                    .receive_learning(from, &shop, &letter.message, now)
                    .map_err(|e| format!("{e:?}"))?;
                let text = match news {
                    News::Quoted { shop, total } => format!("{shop} quoted {total}"),
                    News::Declined { shop } => format!("{shop} declined"),
                    News::Stage { shop, stage } => format!("{shop}: {stage:?}"),
                    News::Chat { shop, text } => format!("{shop}: {text}"),
                    News::Delivered { shop } => format!("{shop} delivered"),
                };
                self.notice(text, now);
                self.save_all();
                Ok(Vec::new())
            }
            Post::Grant(cert) => self.granted(cert).map(|()| Vec::new()),
            Post::Mirror { cert, book } => {
                let value = cert.verify().map_err(|_| "a forged certificate")?;
                if value.account != self.account() {
                    return Err("not a device of this account".into());
                }
                let theirs: ClientBook = postcard::from_bytes(&book).map_err(|_| "not a book")?;
                if !self.siblings.contains(&value.device) {
                    self.siblings.push(value.device);
                }
                self.book.merge(&theirs);
                self.save_all();
                Ok(Vec::new())
            }
            Post::Invite {
                log,
                org,
                name,
                devices,
                cert,
            } => {
                self.shops.invited(
                    crate::shop::ShopInfo {
                        log,
                        org,
                        name: name.clone(),
                        peers: devices,
                        cert,
                    },
                    &self.keys.device,
                );
                self.notice(format!("You were added to {name}"), now);
                self.save_all();
                Ok(Vec::new())
            }
            Post::SyncLog { log, heads, .. } => {
                let from = device.ok_or("a sync comes straight from a device")?;
                self.shops.answer(from, &log, &heads, now)
            }
            Post::LogEntries { log, entries } => {
                let from = device.ok_or("entries come straight from a device")?;
                self.shops.take_entries(from, &log, entries, now)?;
                self.save_all();
                Ok(Vec::new())
            }
            _ => Err("a phone takes letters, its account's answers and its shops' syncs".into()),
        }
    }

    /// Asks to join the account whose pairing code is `text`: the device to send to, the post,
    /// and the code both screens show.
    pub fn join(&mut self, text: &str, now: u64) -> Result<(DeviceId, Vec<u8>, String), String> {
        let offer = PairingOffer::from_text(text).map_err(|_| "not a pairing code".to_string())?;
        let request = offer
            .request(&self.keys.device, &self.name, now)
            .map_err(|_| "the code has expired".to_string())?;
        let to: [u8; 32] = offer
            .address
            .as_slice()
            .try_into()
            .map_err(|_| "not a pairing code".to_string())?;
        let code = offer.code(&self.keys.device_id());
        self.joining = Some(offer);
        Ok((
            cadrs_social::keys::PublicKey(to),
            Post::Pair(request).encode(),
            code,
        ))
    }

    fn granted(&mut self, cert: Signed<DeviceCert>) -> Result<(), String> {
        let offer = self
            .joining
            .as_ref()
            .ok_or("this phone didn't ask to join")?;
        offer
            .accept(&self.keys.device_id(), &cert)
            .map_err(|e| format!("{e:?}"))?;
        let main: [u8; 32] = offer
            .address
            .as_slice()
            .try_into()
            .map_err(|_| "a bad code")?;
        self.keys
            .join(&self.root.join(KEY), cert)
            .map_err(|e| e.to_string())?;
        self.joining = None;
        // The device it joined through shares the account's book with it.
        let main = cadrs_social::keys::PublicKey(main);
        if !self.siblings.contains(&main) {
            self.siblings.push(main);
        }
        self.save_all();
        Ok(())
    }

    /// A letter to the shop with account `shop` (from its card in the contacts).
    pub fn letter(&self, shop: &AccountId, message: Message, now: u64) -> Option<(Card, Composed)> {
        let card = self.contacts.get(shop)?.clone();
        let composed = letters::compose(&self.keys.device, &self.keys.cert, &card, message, now);
        Some((card, composed))
    }

    /// Accepts a quote: the letter to send.
    pub fn accept(&mut self, id: cadrs_org::Id, now: u64) -> Result<(Card, Composed), String> {
        let message = self.book.accept(id).map_err(|e| format!("{e:?}"))?;
        let shop = self.book.sent[&id].shop;
        let out = self
            .letter(&shop, message, now)
            .ok_or("the shop isn't in the contacts")?;
        self.save_all();
        Ok(out)
    }

    /// Writes to the shop about a request: the letter to send.
    pub fn say(
        &mut self,
        id: cadrs_org::Id,
        text: &str,
        now: u64,
    ) -> Result<(Card, Composed), String> {
        let message = self.book.say(id, text, now).map_err(|e| format!("{e:?}"))?;
        let shop = self.book.sent[&id].shop;
        let out = self
            .letter(&shop, message, now)
            .ok_or("the shop isn't in the contacts")?;
        self.save_all();
        Ok(out)
    }

    /// Saves everything (after a shop changed).
    pub fn save(&self) {
        self.save_all();
    }

    /// Collect requests for each mailbox.
    pub fn collect_posts(&self, now: u64) -> Vec<(DeviceId, Vec<u8>)> {
        self.mailboxes
            .iter()
            .map(|m| {
                let post = Post::Collect {
                    request: mailbox::collect_request(&self.keys.device, *m, now),
                    cert: self.keys.cert.clone(),
                };
                (*m, post.encode())
            })
            .collect()
    }

    /// What a mailbox handed over: open what is for this phone, countersign the receipts.
    pub fn take_collected(&mut self, answer: &[u8], now: u64) {
        let Ok(collected) = postcard::from_bytes::<Collected>(answer) else {
            return;
        };
        for (envelope, receipt) in collected {
            if let Ok(receipt) = receipt.countersign(&self.keys.device) {
                let _ = self.ledger.add(receipt);
            }
            if let Ok(bytes) = cadrs_social::seal::open(&self.keys.device, &envelope.value.sealed)
                && let Some(post) = Post::decode(&bytes)
            {
                let _ = self.handle(post, None, now);
            }
        }
        self.save_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_org::Quote;
    use cadrs_org::quote::Money;
    use cadrs_social::keys::{Account, SecretKey};
    use cadrs_social::pairing::Pairing;

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cadrs-mobile-{name}-{}", std::process::id()));
        let _ = cadrs_fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_phone_joins_an_account_and_hears_a_shops_quote() {
        let dir = root("join");
        let mut phone = Phone::load(&dir, vec![], 0).unwrap();
        phone.set_name("Rouan's phone");
        assert!(!phone.joined());

        // The laptop offers; the phone asks; the laptop confirms; the phone joins.
        let laptop = Account::generate();
        let laptop_device = SecretKey::generate();
        let offer = PairingOffer::new(laptop.id(), laptop_device.public().0.to_vec(), 1_000);
        let (to, post, code) = phone.join(&offer.to_text(), 1_000).unwrap();
        assert_eq!(to, laptop_device.public());
        let Some(Post::Pair(request)) = Post::decode(&post) else {
            panic!("a pairing request");
        };
        let mut pairing = Pairing::new(offer);
        assert_eq!(pairing.check(&request, 1_000).unwrap(), code);
        let cert = pairing.confirm(&laptop, &request, 1_000).unwrap();
        phone.handle(Post::Grant(cert), None, 1_000).unwrap();
        assert!(phone.joined());
        assert_eq!(phone.account(), laptop.id());

        // A shop the account asked (from the laptop) quotes; the phone hears it.
        let shop = Account::generate();
        let shop_device = SecretKey::generate();
        let shop_cert = shop.certify(shop_device.public(), "box", 0);
        let card = Card {
            account: shop.id(),
            kind: cadrs_social::contact::Kind::Organization,
            name: "Cape Waterjet".into(),
            devices: vec![shop_device.public()],
            mailboxes: vec![],
            issued: 1,
        }
        .sign(shop.key());
        phone
            .add_contact(&cadrs_social::contact::to_link(&card))
            .unwrap();
        let quote = Message::Quote {
            request: 7,
            quote: Quote {
                total: Money::from_major(724.0),
                currency: "ZAR".into(),
                lines: vec![],
                valid_until: 0,
                note: String::new(),
            },
        };
        let letter = letters::write(&shop_device, &shop_cert, laptop.id(), quote, 2_000);
        phone.handle(Post::Letter(letter), None, 2_000).unwrap();
        assert_eq!(phone.notices[0].text, "Cape Waterjet quoted ZAR 724.00");

        // The phone accepts: a letter for the shop's device, sealed copies for its mailboxes.
        let (to, composed) = phone.accept(7, 3_000).unwrap();
        assert_eq!(to.account, shop.id());
        assert_eq!(composed.envelopes.len(), 1);

        // It all survives a reload.
        let again = Phone::load(&dir, vec![], 4_000).unwrap();
        assert_eq!(again.name, "Rouan's phone");
        assert_eq!(again.account(), laptop.id());
        assert_eq!(again.book.sent[&7].stage, cadrs_org::Stage::Accepted);
        let _ = cadrs_fs::remove_dir_all(&dir);
    }
}
