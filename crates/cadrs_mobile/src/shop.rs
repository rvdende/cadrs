//! The shops (organizations) a phone belongs to: their documents, kept in step with the other
//! members' devices, and what an owner does from a phone: quote a request at the shop's prices,
//! decline it, move an order on. Answers go to the client as the shop's, with the shop's
//! certificate for this phone (from the invitation of the device that holds the shop's key).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cadrs_org::letters::{self, Composed, Message};
use cadrs_org::session::Session;
use cadrs_org::{Id, Op, Stage};
use cadrs_social::contact::Card;
use cadrs_social::keys::{AccountId, DeviceCert, DeviceId, SecretKey, Signed};
use cadrs_social::log::{Entry, EntryId, Log, LogId};
use cadrs_social::post::{Post, Synced};
use serde::{Deserialize, Serialize};

/// A shop as the phone knows it (kept in `shops.bin`; its log beside, `shops/<log>.log`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShopInfo {
    pub log: LogId,
    pub org: AccountId,
    pub name: String,
    /// Devices to sync with besides the members' (those that invited this one).
    pub peers: Vec<DeviceId>,
    /// The shop's certificate for this phone: it answers clients as the shop.
    pub cert: Option<Signed<DeviceCert>>,
}

/// A shop's document on this phone.
pub struct Shop {
    pub info: ShopInfo,
    pub session: Session,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn log_path(root: &Path, log: &LogId) -> PathBuf {
    root.join("shops").join(format!("{}.log", hex(&log.0)))
}

/// The phone's shops.
#[derive(Default)]
pub struct Shops {
    pub list: Vec<Shop>,
}

impl Shops {
    pub fn load(root: &Path, key: &SecretKey) -> Self {
        let infos: Vec<ShopInfo> = cadrs_fs::read(root.join("shops.bin"))
            .ok()
            .and_then(|b| postcard::from_bytes(&b).ok())
            .unwrap_or_default();
        Self {
            list: infos
                .into_iter()
                .map(|info| {
                    let log = cadrs_org::file::load(&log_path(root, &info.log))
                        .map(|(log, _)| log)
                        .unwrap_or_else(|_| Log::new(info.log));
                    Shop {
                        session: Session::open(log, key.clone()),
                        info,
                    }
                })
                .collect(),
        }
    }

    pub fn save(&self, root: &Path) {
        let infos: Vec<&ShopInfo> = self.list.iter().map(|s| &s.info).collect();
        let _ = cadrs_fs::create_dir_all(root.join("shops"));
        let _ = cadrs_fs::write(
            root.join("shops.bin"),
            postcard::to_allocvec(&infos).expect("encodes"),
        );
        for shop in &self.list {
            let _ = cadrs_org::file::save(&log_path(root, &shop.info.log), shop.session.log());
        }
    }

    pub fn accounts(&self) -> Vec<AccountId> {
        self.list.iter().map(|s| s.info.org).collect()
    }

    fn by_log(&mut self, log: &LogId) -> Option<&mut Shop> {
        self.list.iter_mut().find(|s| s.info.log == *log)
    }

    /// An invitation to a shop.
    pub fn invited(&mut self, info: ShopInfo, key: &SecretKey) {
        if let Some(shop) = self.by_log(&info.log) {
            if info.cert.is_some() {
                shop.info.cert = info.cert;
            }
            return;
        }
        self.list.push(Shop {
            session: Session::open(Log::new(info.log), key.clone()),
            info,
        });
    }

    /// The posts that ask the other members' devices for what this phone lacks.
    pub fn sync_posts(
        &self,
        me: DeviceId,
        cert: &Signed<DeviceCert>,
    ) -> Vec<(LogId, DeviceId, Vec<u8>)> {
        let mut out = Vec::new();
        for shop in &self.list {
            let org = shop.session.org();
            let mut devices: Vec<DeviceId> = org
                .members
                .keys()
                .flat_map(|a| org.devices.of(a))
                .map(|c| c.device)
                .chain(shop.info.peers.iter().copied())
                .filter(|d| *d != me)
                .collect();
            devices.sort();
            devices.dedup();
            for device in devices {
                let post = Post::SyncLog {
                    log: shop.info.log,
                    heads: shop.session.log().heads().clone(),
                    cert: cert.clone(),
                };
                out.push((shop.info.log, device, post.encode()));
            }
        }
        out
    }

    /// The answer to our ask: take what we lacked; the entries to send back, if they lack some.
    pub fn synced(&mut self, log: &LogId, answer: &[u8], now: u64) -> Option<Vec<u8>> {
        let (entries, theirs) = postcard::from_bytes::<Synced>(answer).ok()?;
        let shop = self.by_log(log)?;
        shop.session.receive(entries, now);
        if !shop.session.org().profile.name.is_empty() {
            shop.info.name = shop.session.org().profile.name.clone();
        }
        let lacking = shop.session.since(&theirs);
        (!lacking.is_empty()).then(|| {
            Post::LogEntries {
                log: *log,
                entries: lacking,
            }
            .encode()
        })
    }

    /// Another member's device asks for what it lacks.
    pub fn answer(
        &mut self,
        from: DeviceId,
        log: &LogId,
        heads: &BTreeSet<EntryId>,
        now: u64,
    ) -> Result<Vec<u8>, String> {
        let shop = self.by_log(log).ok_or("not a shop of this phone")?;
        if shop.session.org().member_of(&from, now).is_none() {
            return Err("not a member's device".into());
        }
        let synced: Synced = (
            shop.session.since(heads),
            shop.session.log().heads().clone(),
        );
        Ok(postcard::to_allocvec(&synced).expect("encodes"))
    }

    /// Entries from another member's device.
    pub fn take_entries(
        &mut self,
        from: DeviceId,
        log: &LogId,
        entries: Vec<Entry>,
        now: u64,
    ) -> Result<(), String> {
        let shop = self.by_log(log).ok_or("not a shop of this phone")?;
        if shop.session.org().member_of(&from, now).is_none() {
            return Err("not a member's device".into());
        }
        shop.session.receive(entries, now);
        Ok(())
    }

    /// A letter to one of the shops: into its document.
    pub fn take_letter(
        &mut self,
        letter: &letters::Letter,
        from: AccountId,
        now: u64,
    ) -> Result<String, String> {
        let shop = self
            .list
            .iter_mut()
            .find(|s| s.info.org == letter.to)
            .ok_or("not for one of this phone's shops")?;
        let op = letters::shop_op(shop.session.org(), from, &letter.message)
            .map_err(|e| format!("{e:?}"))?;
        shop.session.write(op, now)?;
        Ok(shop.info.name.clone())
    }

    /// Quotes a request at the shop's prices: the letter to the client.
    pub fn quote(
        &mut self,
        i: usize,
        request: Id,
        key: &SecretKey,
        now: u64,
    ) -> Result<(Card, Composed), String> {
        let shop = self.list.get_mut(i).ok_or("no such shop")?;
        let org = shop.session.org();
        let rates = org.rates.clone().ok_or("the shop has no prices")?;
        let r = org.requests.get(&request).ok_or("no such request")?;
        let machines: Vec<_> = org.machines.values().cloned().collect();
        let estimate = cadrs_org::quote::estimate(&machines, &rates, &r.parts)
            .map_err(|e| format!("{e:?}"))?;
        let quote = cadrs_org::Quote {
            total: estimate.total,
            currency: estimate.currency,
            lines: estimate.lines,
            valid_until: now + 14 * 86_400_000,
            note: String::new(),
        };
        shop.session.write(
            Op::Quote {
                request,
                quote: quote.clone(),
            },
            now,
        )?;
        tell(shop, request, Message::Quote { request, quote }, key, now)
    }

    pub fn decline(
        &mut self,
        i: usize,
        request: Id,
        key: &SecretKey,
        now: u64,
    ) -> Result<(Card, Composed), String> {
        let shop = self.list.get_mut(i).ok_or("no such shop")?;
        shop.session.write(
            Op::Decline {
                request,
                reason: String::new(),
            },
            now,
        )?;
        tell(
            shop,
            request,
            Message::Decline {
                request,
                reason: String::new(),
            },
            key,
            now,
        )
    }

    /// Moves an order on (in production, ready).
    pub fn advance(
        &mut self,
        i: usize,
        request: Id,
        stage: Stage,
        key: &SecretKey,
        now: u64,
    ) -> Result<(Card, Composed), String> {
        let shop = self.list.get_mut(i).ok_or("no such shop")?;
        shop.session.write(Op::Advance { request, stage }, now)?;
        tell(shop, request, Message::Stage { request, stage }, key, now)
    }
}

/// The letter to the client of `request`, as the shop.
fn tell(
    shop: &Shop,
    request: Id,
    message: Message,
    key: &SecretKey,
    now: u64,
) -> Result<(Card, Composed), String> {
    let cert = shop.info.cert.as_ref().ok_or(
        "this phone can't answer as the shop: link it from the device that holds the shop's key",
    )?;
    let card = shop
        .session
        .org()
        .requests
        .get(&request)
        .and_then(|r| r.card.as_ref())
        .map(|c| c.value.clone())
        .ok_or("the client can't be reached: their card is missing")?;
    let composed = letters::compose(key, cert, &card, message, now);
    Ok((card, composed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_org::capability::{MachineCaps, MaterialLimit, PartSummary, Process};
    use cadrs_org::quote::{Money, RateCard};
    use cadrs_social::keys::Account;

    #[test]
    fn an_owners_phone_syncs_the_shop_and_quotes_from_it() {
        // The owner's laptop founds the shop and sets it up.
        let owner = Account::generate();
        let laptop = SecretKey::generate();
        let (mut desk, org) = Session::found(
            laptop.clone(),
            owner.certify(laptop.public(), "laptop", 0),
            "Cape Waterjet",
            1_000,
        );
        desk.write(
            Op::SetMachine {
                id: 1,
                machine: Some(MachineCaps {
                    name: "Waterjet".into(),
                    process: Process::Waterjet,
                    envelope: [3000.0, 1500.0, 0.0],
                    materials: vec![MaterialLimit {
                        material: "mild steel".into(),
                        max_thickness: 100.0,
                        speeds: vec![(10.0, 300.0)],
                    }],
                    kerf: 1.0,
                    min_hole_ratio: 0.5,
                    tolerance: 0.1,
                    pierce_seconds: 4.0,
                }),
            },
            1_001,
        )
        .unwrap();
        desk.write(
            Op::SetRates(RateCard {
                currency: "ZAR".into(),
                setup: Money::from_major(250.0),
                minimum: Money::from_major(500.0),
                hourly: vec![("Waterjet".into(), Money::from_major(1200.0))],
                sheet_per_m2: vec![("mild steel".into(), 10.0, Money::from_major(1000.0))],
                nesting_allowance: 1.25,
            }),
            1_002,
        )
        .unwrap();
        // A client's request, with their card.
        let client = Account::generate();
        let client_device = SecretKey::generate();
        let card = Card {
            account: client.id(),
            kind: cadrs_social::contact::Kind::Person,
            name: "Alex".into(),
            devices: vec![client_device.public()],
            mailboxes: vec![],
            issued: 1,
        }
        .sign(client.key());
        desk.write(
            Op::Request {
                id: 9,
                client: client.id(),
                parts: vec![(
                    PartSummary {
                        name: "bracket".into(),
                        flat_thickness: Some(10.0),
                        size: [200.0, 80.0, 10.0],
                        cut_length: 900.0,
                        pierces: 3,
                        area: 12_000.0,
                        material: Some("mild steel".into()),
                        ..Default::default()
                    },
                    6,
                )],
                notes: String::new(),
                snapshot: [0; 32],
                deadline: None,
                card: Some(card),
            },
            1_003,
        )
        .unwrap();

        // The owner's phone (same account), invited with the shop's certificate.
        let phone = SecretKey::generate();
        let phone_cert = owner.certify(phone.public(), "phone", 0);
        let mut shops = Shops::default();
        shops.invited(
            ShopInfo {
                log: desk.log().id(),
                org: org.id(),
                name: "Cape Waterjet".into(),
                peers: vec![laptop.public()],
                cert: Some(org.certify(phone.public(), "", 0)),
            },
            &phone,
        );

        // It asks the laptop; the laptop adds the phone (an owner's device) and answers.
        let (log, to, post) = shops.sync_posts(phone.public(), &phone_cert).remove(0);
        assert_eq!(to, laptop.public());
        let Some(Post::SyncLog { heads, cert, .. }) = Post::decode(&post) else {
            panic!("a sync");
        };
        desk.write(Op::AddDevice(cert), 2_000).unwrap();
        let answer: Synced = (desk.since(&heads), desk.log().heads().clone());
        let back = shops.synced(&log, &postcard::to_allocvec(&answer).unwrap(), 3_000);
        assert!(back.is_none(), "the phone had nothing new");
        assert_eq!(shops.list[0].session.org().requests.len(), 1);

        // It quotes at the shop's prices: a letter to the client, as the shop.
        let (to, composed) = shops.quote(0, 9, &phone, 4_000).unwrap();
        assert_eq!(to.account, client.id());
        let Some(Post::Letter(bytes)) = Post::decode(&composed.post) else {
            panic!("a letter");
        };
        let (letter, from) = letters::read(&bytes, &[client.id()]).unwrap();
        assert_eq!(from, org.id(), "the client hears it from the shop");
        assert!(matches!(letter.message, Message::Quote { .. }));
        assert_eq!(
            shops.list[0].session.org().requests[&9].stage,
            Stage::Quoted
        );

        // The laptop gets the phone's quote by the next sync.
        let theirs = shops.list[0].session.since(desk.log().heads());
        desk.receive(theirs, 5_000);
        assert_eq!(desk.org().requests[&9].stage, Stage::Quoted);
    }
}
