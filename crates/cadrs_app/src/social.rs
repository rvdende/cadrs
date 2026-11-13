//! The social side of cadrs on this device: its keys, its contacts, the requests for quotes it
//! sent, and the organizations (shops) it can speak for.
//!
//! Letters (`cadrs_org::letters`) arrive through `cadrs_net::social`. One for an organization
//! this device belongs to goes into that organization's document (opened from disk if it isn't
//! the one on screen); one for the user updates their [`ClientBook`]. Either way the bell says
//! so. [`send`] sends a letter to every device a contact's card lists; one taking it is enough.

use std::path::PathBuf;

use bevy::prelude::*;
use cadrs_core::DocumentId;
use cadrs_net::bevy_iroh::iroh::EndpointId;
use cadrs_net::bevy_iroh::{Iroh, IrohTask};
use cadrs_org::letters::{self, ClientBook, Message, News};
use cadrs_org::session::Session;
use cadrs_social::contact::{Card, Contacts, Kind};
use cadrs_social::keys::{Account, AccountId, DeviceCert, DeviceId, PublicKey, SecretKey, Signed};
use cadrs_social::keystore::DeviceKeys;
use cadrs_social::mailbox::{self, Envelope};
use cadrs_social::post::{Collected, Post};
use cadrs_social::receipts::Ledger;
use cadrs_ui::i18n::{tr, tr_with};

use crate::notifications::{Notice, Notifications};
use crate::{DocumentStore, UserProfile};

pub struct SocialPlugin;

impl Plugin for SocialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Social>()
            .init_resource::<Outbox>()
            .add_systems(
                Update,
                (
                    receive,
                    poll_outbox,
                    collect_sometimes,
                    keep_directory,
                    crate::chat_ui::refresh,
                )
                    .chain(),
            );
    }
}

/// An organization this device may speak for: it holds the organization's key, or a
/// certificate from it.
#[derive(Clone, Debug)]
pub struct MyOrg {
    pub doc: DocumentId,
    pub name: String,
    pub account: AccountId,
    /// Its document's log.
    pub log: cadrs_social::log::LogId,
    /// The organization's certificate for this device: its letters are the organization's.
    /// `None` on a member's device the organization's key hasn't certified (it syncs the
    /// document, but doesn't speak for the organization).
    pub cert: Option<Signed<DeviceCert>>,
    /// Devices to sync with besides the members' (those who invited this one).
    pub peers: Vec<DeviceId>,
}

/// This device's social state.
#[derive(Resource, Default)]
pub struct Social {
    pub(crate) keys: Option<DeviceKeys>,
    pub(crate) key_path: PathBuf,
    pub contacts: Contacts,
    pub book: ClientBook,
    pub orgs: Vec<MyOrg>,
    /// Receipts of what mailboxes kept for this device.
    pub ledger: Ledger,
    /// The mailboxes this device's cards name.
    pub mailboxes: Vec<DeviceId>,
    /// The shops heard of in the public room.
    pub directory: cadrs_org::directory::Directory,
    /// Where the user said they are (Find a shop's distances).
    pub my_place: Option<cadrs_social::place::Place>,
    /// The account's other devices this one linked (Link a device…): on the user's card too.
    pub linked: Vec<DeviceId>,
    /// The public chat as heard here, and who is blocked.
    pub public_chat: cadrs_social::chat::Room,
    loaded: bool,
}

impl Social {
    pub fn keys(&self) -> Option<&DeviceKeys> {
        self.keys.as_ref()
    }

    /// The accounts letters to this device may be for: the user's and their organizations'.
    pub fn accounts(&self) -> Vec<AccountId> {
        let mut ours: Vec<AccountId> = self.orgs.iter().map(|o| o.account).collect();
        if let Some(keys) = &self.keys {
            ours.push(keys.account_id());
        }
        ours
    }

    /// This user's own contact card.
    pub fn my_card(&self, name: &str) -> Option<Signed<Card>> {
        let keys = self.keys.as_ref()?;
        let account = keys.account.as_ref()?;
        Some(
            Card {
                account: account.id(),
                kind: Kind::Person,
                name: name.to_owned(),
                devices: std::iter::once(keys.device_id())
                    .chain(self.linked.iter().copied())
                    .collect(),
                mailboxes: self.mailboxes.clone(),
                issued: cadrs_social::wall_millis(),
            }
            .sign(account.key()),
        )
    }

    /// An organization's card, if this device holds its key.
    pub fn org_card(&self, org: &AccountId) -> Option<Signed<Card>> {
        let mine = self.orgs.iter().find(|o| &o.account == org)?;
        let account = org_account(&self.key_path, org)?;
        Some(
            Card {
                account: *org,
                kind: Kind::Organization,
                name: mine.name.clone(),
                devices: vec![self.keys.as_ref()?.device_id()],
                mailboxes: self.mailboxes.clone(),
                issued: cadrs_social::wall_millis(),
            }
            .sign(account.key()),
        )
    }
}

/// Where an organization's key and this device's certificate from it are kept.
/// Keeps the organization's certificate for this device (from an invitation).
pub(crate) fn keep_org_cert(world: &World, org: &AccountId, cert: &Signed<DeviceCert>) {
    let social = world.resource::<Social>();
    let ok = cert.verify().is_ok_and(|c| {
        &c.account == org && social.keys().is_some_and(|k| c.device == k.device_id())
    }) && &cert.signer == org;
    if ok {
        let (_, path) = org_files(&social.key_path, org);
        let _ = path
            .parent()
            .map_or(Ok(()), cadrs_fs::create_dir_all)
            .and_then(|()| cadrs_fs::write(&path, postcard::to_allocvec(cert).expect("encodes")));
    }
}

pub(crate) fn org_files(key_path: &std::path::Path, org: &AccountId) -> (PathBuf, PathBuf) {
    let dir = key_path.with_file_name("orgs");
    (
        dir.join(format!("{org}.key")),
        dir.join(format!("{org}.cert")),
    )
}

/// The organization's own key, when this device holds it.
pub fn org_key(world: &World, org: &AccountId) -> Option<SecretKey> {
    let social = world.resource::<Social>();
    org_account(&social.key_path, org).map(|a| a.key().clone())
}

fn org_account(key_path: &std::path::Path, org: &AccountId) -> Option<Account> {
    let bytes = cadrs_fs::read(org_files(key_path, org).0).ok()?;
    let bytes: [u8; 32] = bytes.as_slice().try_into().ok()?;
    Some(Account::from_key(SecretKey::from_bytes(&bytes)))
}

fn social_dir(world: &World) -> PathBuf {
    world.resource::<DocumentStore>().0.root().join("social")
}

/// Saves `value` in this device's social folder.
pub(crate) fn save_file<T: serde::Serialize>(world: &World, file: &str, value: &T) {
    save(world, file, value);
}

fn save<T: serde::Serialize>(world: &World, file: &str, value: &T) {
    let dir = social_dir(world);
    let bytes = postcard::to_allocvec(value).expect("encodes");
    let written = cadrs_fs::create_dir_all(&dir).and_then(|()| {
        let tmp = dir.join(format!("{file}.tmp"));
        cadrs_fs::write(&tmp, bytes)?;
        cadrs_fs::rename(&tmp, dir.join(file))
    });
    if let Err(e) = written {
        warn!("cannot save {file}: {e}");
    }
}

fn load<T: serde::de::DeserializeOwned + Default>(world: &World, file: &str) -> T {
    cadrs_fs::read(social_dir(world).join(file))
        .ok()
        .and_then(|b| postcard::from_bytes(&b).ok())
        .unwrap_or_default()
}

const CONTACTS: &str = "contacts.bin";
const BOOK: &str = "requests-sent.bin";
const LEDGER: &str = "receipts.bin";
const DIRECTORY: &str = "shops.bin";
const MY_PLACE: &str = "my-place.bin";
const LINKED: &str = "linked-devices.bin";

/// Loads the keys, contacts, sent requests and organizations, once.
pub fn ensure_loaded(world: &mut World) {
    if world.resource::<Social>().loaded {
        // The mailboxes cards name follow the setting.
        if let Some(mailboxes) = world.get_resource::<MyMailboxes>().map(|m| m.0.clone()) {
            world.resource_mut::<Social>().mailboxes = mailboxes;
        }
        return;
    }
    let (key_path, name) = crate::organization::identity_key(world);
    let keys = DeviceKeys::load_or_create(&key_path, &name, cadrs_social::wall_millis())
        .map_err(|e| warn!("cannot load this device's keys: {e}"))
        .ok();
    let contacts: Contacts = load(world, CONTACTS);
    let book: ClientBook = load(world, BOOK);
    let ledger: Ledger = load(world, LEDGER);
    let directory: cadrs_org::directory::Directory = load(world, DIRECTORY);
    let my_place: Option<cadrs_social::place::Place> = load(world, MY_PLACE);
    let linked: Vec<DeviceId> = load(world, LINKED);
    let public_chat: cadrs_social::chat::Room = load(world, "public-chat.bin");
    let mailboxes = my_mailboxes(world);
    let orgs = keys
        .as_ref()
        .map(|k| find_orgs(world, &key_path, k))
        .unwrap_or_default();
    let mut social = world.resource_mut::<Social>();
    social.keys = keys;
    social.key_path = key_path;
    social.contacts = contacts;
    social.book = book;
    social.ledger = ledger;
    social.directory = directory;
    social.my_place = my_place;
    social.linked = linked;
    social.public_chat = public_chat;
    social.mailboxes = mailboxes;
    social.orgs = orgs;
    social.loaded = true;
}

/// The organization documents in the store this device speaks for.
fn find_orgs(world: &World, key_path: &std::path::Path, keys: &DeviceKeys) -> Vec<MyOrg> {
    let store = world.resource::<DocumentStore>().0.clone();
    let (library, _) = store.list();
    library
        .entries
        .iter()
        .filter(|e| e.meta.trashed.is_none())
        .filter_map(|e| {
            let log_id = cadrs_social::log::LogId(e.meta.organization?.log);
            let dir = store.doc_dir(e.id);
            let invite = crate::org_sync::invite_of(&dir);
            let log = cadrs_org::file::load(&cadrs_org::file::path(&dir))
                .map(|(log, _)| log)
                .unwrap_or_else(|_| cadrs_social::log::Log::new(log_id));
            let session = Session::open(log, keys.device.clone());
            let org = session.org();
            let account = org.org.or(invite.as_ref().map(|i| i.org))?;
            let member = org.members.contains_key(&keys.account_id());
            let cert = org_cert(key_path, &account, keys);
            if !member && cert.is_none() && invite.is_none() {
                return None;
            }
            Some(MyOrg {
                doc: e.id,
                name: if org.profile.name.is_empty() {
                    invite.as_ref().map_or_else(String::new, |i| i.name.clone())
                } else {
                    org.profile.name.clone()
                },
                account,
                log: log_id,
                cert,
                peers: invite.map(|i| i.devices).unwrap_or_default(),
            })
        })
        .collect()
}

/// This device's certificate from organization `org`: kept, or made when the device holds the
/// organization's key.
fn org_cert(
    key_path: &std::path::Path,
    org: &AccountId,
    keys: &DeviceKeys,
) -> Option<Signed<DeviceCert>> {
    let (_, cert_path) = org_files(key_path, org);
    let kept = cadrs_fs::read(&cert_path)
        .ok()
        .and_then(|b| postcard::from_bytes::<Signed<DeviceCert>>(&b).ok())
        .filter(|c| {
            c.verify().is_ok() && c.value.device == keys.device_id() && &c.value.account == org
        });
    if kept.is_some() {
        return kept;
    }
    let account = org_account(key_path, org)?;
    let cert = account.certify(keys.device_id(), "", cadrs_social::wall_millis());
    let _ = cadrs_fs::write(&cert_path, postcard::to_allocvec(&cert).expect("encodes"));
    Some(cert)
}

/// Notes a new organization this device founded (its key is kept already).
pub fn add_org(
    world: &mut World,
    doc: DocumentId,
    name: &str,
    account: AccountId,
    log: cadrs_social::log::LogId,
    peers: Vec<DeviceId>,
) {
    ensure_loaded(world);
    let social = world.resource::<Social>();
    let Some(keys) = social.keys.clone() else {
        return;
    };
    let key_path = social.key_path.clone();
    let cert = org_cert(&key_path, &account, &keys);
    world.resource_mut::<Social>().orgs.push(MyOrg {
        doc,
        name: name.to_owned(),
        account,
        log,
        cert,
        peers,
    });
}

/// Adds a contact from its card; true when it was new.
pub fn add_contact(world: &mut World, card: Signed<Card>) -> bool {
    ensure_loaded(world);
    let added = world.resource_mut::<Social>().contacts.add(card);
    if added {
        let contacts = world.resource::<Social>().contacts.clone();
        save(world, CONTACTS, &contacts);
    }
    added
}

/// Saves the requests sent, and shares them with the account's other devices.
pub fn save_book(world: &mut World) {
    let book = world.resource::<Social>().book.clone();
    save(world, BOOK, &book);
    mirror(world);
}

/// Sends this device's book to the account's other devices it knows (those it linked).
fn mirror(world: &mut World) {
    let social = world.resource::<Social>();
    let (Some(keys), linked) = (social.keys().cloned(), social.linked.clone()) else {
        return;
    };
    if linked.is_empty() || !world.contains_resource::<Iroh>() {
        return;
    }
    let post = Post::Mirror {
        cert: keys.cert.clone(),
        book: postcard::to_allocvec(&social.book).expect("encodes"),
    };
    for device in linked {
        start(
            world,
            &[device],
            post.encode(),
            String::new(),
            SendKind::Quiet,
        );
    }
}

/// Another device of this account sent its book: merge it.
fn mirrored(
    world: &mut World,
    from: Option<DeviceId>,
    cert: &Signed<DeviceCert>,
    book: &[u8],
) -> Result<(), String> {
    let value = cert.verify().map_err(|_| "a forged certificate")?;
    let me = world.resource::<Social>().keys().map(|k| k.account_id());
    if Some(value.account) != me || from != Some(value.device) {
        return Err("not a device of this account".into());
    }
    let theirs: ClientBook = postcard::from_bytes(book).map_err(|_| "not a book")?;
    let mut social = world.resource_mut::<Social>();
    if social.book.merge(&theirs) {
        if !social.linked.contains(&value.device) {
            social.linked.push(value.device);
        }
        let book = social.book.clone();
        save(world, BOOK, &book);
    }
    Ok(())
}

/// Posts on their way.
#[derive(Resource, Default)]
struct Outbox(Vec<Sending>);

struct Sending {
    what: String,
    tasks: Vec<IrohTask<Result<Vec<u8>, String>>>,
    errors: Vec<String>,
    kind: SendKind,
}

pub(crate) enum SendKind {
    /// Straight to the recipient's devices; left at `fallback` when none takes it.
    Deliver { fallback: Option<Fallback> },
    /// Left at a mailbox.
    Deposit,
    /// Collecting from a mailbox: the answer is what it kept for us.
    Collect,
    /// Quietly: nobody is told whether it got through (a sync between the account's devices).
    Quiet,
    /// Asking a fellow member's device for the entries of a log this one lacks.
    Sync {
        log: cadrs_social::log::LogId,
        device: DeviceId,
    },
}

/// Where to leave a letter whose recipient is offline: sealed envelopes, one per device, and
/// the mailboxes the recipient collects from.
pub(crate) struct Fallback {
    mailboxes: Vec<DeviceId>,
    envelopes: Vec<Signed<Envelope>>,
}

/// How often a device collects from its mailboxes while online, in seconds.
const COLLECT_EVERY: f64 = 300.0;

/// Sends `message` to the account on `to` (from the organization `as_org` when given, else
/// from the user): to its devices, or sealed to each and left at its mailboxes when none is
/// online. False when it couldn't start (offline, no keys).
pub fn send(
    world: &mut World,
    to: &Card,
    message: Message,
    as_org: Option<AccountId>,
    what: String,
) -> bool {
    ensure_loaded(world);
    let social = world.resource::<Social>();
    let Some(keys) = social.keys.clone() else {
        crate::linked::toast(world, tr("social-no-keys"));
        return false;
    };
    let cert = match as_org {
        Some(org) => match social.orgs.iter().find(|o| o.account == org) {
            Some(MyOrg {
                cert: Some(cert), ..
            }) => cert.clone(),
            Some(_) | None => {
                crate::linked::toast(world, tr("social-not-a-member"));
                return false;
            }
        },
        None => keys.cert.clone(),
    };
    let letters::Composed { post, envelopes } = letters::compose(
        &keys.device,
        &cert,
        to,
        message,
        cadrs_social::wall_millis(),
    );
    let fallback = (!to.mailboxes.is_empty()).then(|| Fallback {
        mailboxes: to.mailboxes.clone(),
        envelopes,
    });
    start(
        world,
        &to.devices,
        post,
        what,
        SendKind::Deliver { fallback },
    )
}

/// Addresses of devices known without discovery (devices on this network; tests, where there
/// is no relay to find them through).
#[derive(Resource, Default)]
pub struct PeerAddrs(
    pub std::collections::HashMap<EndpointId, cadrs_net::bevy_iroh::iroh::EndpointAddr>,
);

/// The mailboxes this user's cards name (where others leave letters while this device is
/// offline, and where it collects them); the seed peers when not set.
#[derive(Resource, Clone, Default)]
pub struct MyMailboxes(pub Vec<DeviceId>);

fn my_mailboxes(world: &World) -> Vec<DeviceId> {
    match world.get_resource::<MyMailboxes>() {
        Some(m) => m.0.clone(),
        None => cadrs_net::public::DEFAULT_SEEDS
            .iter()
            .filter_map(|s| s.parse::<EndpointId>().ok())
            .map(|id| PublicKey(*id.as_bytes()))
            .collect(),
    }
}

/// Sends `post` to every one of `devices`; one taking it is enough. False when it couldn't
/// start (offline, no devices).
pub fn send_post(world: &mut World, devices: &[DeviceId], post: &Post, what: String) -> bool {
    let fallback = None;
    start(
        world,
        devices,
        post.encode(),
        what,
        SendKind::Deliver { fallback },
    )
}

pub(crate) fn start(
    world: &mut World,
    devices: &[DeviceId],
    bytes: Vec<u8>,
    what: String,
    kind: SendKind,
) -> bool {
    if !world.contains_resource::<Iroh>() && !crate::network::connect(world) {
        return false;
    }
    let Some(iroh) = world.get_resource::<Iroh>() else {
        return false;
    };
    let hints = world.get_resource::<PeerAddrs>();
    let tasks: Vec<_> = devices
        .iter()
        .filter_map(|d| EndpointId::from_bytes(&d.0).ok())
        .map(|d| {
            // A known address, else the bare key for discovery to find.
            let addr = hints
                .and_then(|h| h.0.get(&d).cloned())
                .unwrap_or_else(|| d.into());
            cadrs_net::social::start_send(iroh, addr, bytes.clone())
        })
        .collect();
    if tasks.is_empty() {
        crate::linked::toast(world, tr("social-no-devices"));
        return false;
    }
    world.resource_mut::<Outbox>().0.push(Sending {
        what,
        tasks,
        errors: Vec::new(),
        kind,
    });
    true
}

enum Done {
    Delivered(String),
    Failed(String, String),
    Deposit(String, Fallback),
    Deposited(String),
    Collected(Vec<Vec<u8>>),
    Synced(cadrs_social::log::LogId, DeviceId, Vec<u8>),
}

fn poll_outbox(world: &mut World) {
    let mut done = Vec::new();
    world.resource_mut::<Outbox>().0.retain_mut(|s| {
        let mut answers = Vec::new();
        let errors = &mut s.errors;
        s.tasks.retain_mut(|t| match t.try_take() {
            Some(Ok(answer)) => {
                answers.push(answer);
                false
            }
            Some(Err(e)) => {
                errors.push(e);
                false
            }
            None => true,
        });
        match &mut s.kind {
            SendKind::Collect => {
                if !answers.is_empty() {
                    done.push(Done::Collected(answers));
                }
                !s.tasks.is_empty()
            }
            SendKind::Quiet => !s.tasks.is_empty() && answers.is_empty(),
            // Quietly: a member's device offline is no news.
            SendKind::Sync { log, device } => {
                if let Some(answer) = answers.pop() {
                    done.push(Done::Synced(*log, *device, answer));
                }
                !s.tasks.is_empty()
            }
            _ if !answers.is_empty() => {
                done.push(match s.kind {
                    SendKind::Deposit => Done::Deposited(s.what.clone()),
                    _ => Done::Delivered(s.what.clone()),
                });
                false
            }
            _ if !s.tasks.is_empty() => true,
            SendKind::Deliver { fallback } => {
                done.push(match fallback.take() {
                    Some(f) => Done::Deposit(s.what.clone(), f),
                    None => Done::Failed(s.what.clone(), s.errors.join("; ")),
                });
                false
            }
            SendKind::Deposit => {
                done.push(Done::Failed(s.what.clone(), s.errors.join("; ")));
                false
            }
        }
    });
    for d in done {
        match d {
            Done::Delivered(what) => {
                crate::linked::toast(world, tr_with("social-sent", &[("what", &what)]))
            }
            Done::Failed(what, why) => crate::linked::toast(
                world,
                tr_with("social-not-sent", &[("what", &what), ("why", &why)]),
            ),
            Done::Deposited(what) => {
                crate::linked::toast(world, tr_with("social-left", &[("what", &what)]))
            }
            Done::Deposit(what, fallback) => {
                // Nobody online: leave it at their mailboxes, one envelope per device.
                for envelope in fallback.envelopes {
                    start(
                        world,
                        &fallback.mailboxes,
                        Post::Deposit(envelope).encode(),
                        what.clone(),
                        SendKind::Deposit,
                    );
                }
            }
            Done::Collected(answers) => {
                for answer in answers {
                    take_collected(world, &answer);
                }
            }
            Done::Synced(log, device, answer) => {
                crate::org_sync::synced(world, log, device, &answer);
            }
        }
    }
}

/// Collects what this device's mailboxes kept for the user and their organizations.
pub fn collect_now(world: &mut World) {
    ensure_loaded(world);
    let social = world.resource::<Social>();
    let Some(keys) = social.keys.clone() else {
        return;
    };
    if !world.contains_resource::<Iroh>() {
        return;
    }
    let mut certs = vec![keys.cert.clone()];
    certs.extend(social.orgs.iter().filter_map(|o| o.cert.clone()));
    let now = cadrs_social::wall_millis();
    for mailbox in my_mailboxes(world) {
        for cert in &certs {
            let post = Post::Collect {
                request: mailbox::collect_request(&keys.device, mailbox, now),
                cert: cert.clone(),
            };
            start(
                world,
                &[mailbox],
                post.encode(),
                String::new(),
                SendKind::Collect,
            );
        }
    }
}

/// Collects now and then while online, and as soon as cadrs goes online.
fn collect_sometimes(world: &mut World, mut last: Local<Option<f64>>) {
    let online = world.contains_resource::<Iroh>();
    if !online {
        *last = None;
        return;
    }
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    if last.is_some_and(|t| now - t < COLLECT_EVERY) {
        return;
    }
    *last = Some(now);
    collect_now(world);
}

/// What a mailbox handed over: open each envelope meant for this device, take what is inside,
/// and countersign the mailbox's storage receipts.
fn take_collected(world: &mut World, answer: &[u8]) {
    let Ok(collected) = postcard::from_bytes::<Collected>(answer) else {
        return;
    };
    let Some(keys) = world.resource::<Social>().keys.clone() else {
        return;
    };
    for (envelope, receipt) in collected {
        if let Ok(receipt) = receipt.countersign(&keys.device) {
            let _ = world.resource_mut::<Social>().ledger.add(receipt);
        }
        // Sealed for each of the account's devices: this one opens its own.
        let Ok(bytes) = cadrs_social::seal::open(&keys.device, &envelope.value.sealed) else {
            continue;
        };
        if let Some(post) = Post::decode(&bytes)
            && let Err(why) = handle(world, post, None)
        {
            warn!("a letter from a mailbox: {why}");
        }
    }
    let ledger = world.resource::<Social>().ledger.clone();
    save(world, LEDGER, &ledger);
}

fn notify(world: &mut World, id: String, icon: &'static str, title: String, body: String) {
    world.resource_mut::<Notifications>().push(Notice {
        id,
        icon,
        title,
        body,
        unread: true,
    });
}

/// Does what a post asks; its answer. `from` is the device that sent it, when it came straight
/// from it (not through a mailbox).
fn handle(world: &mut World, post: Post, from: Option<DeviceId>) -> Result<Vec<u8>, String> {
    let none = |r: Result<(), String>| r.map(|()| Vec::new());
    match post {
        Post::Letter(bytes) => {
            let ours = world.resource::<Social>().accounts();
            none(
                letters::read(&bytes, &ours)
                    .map_err(|why| format!("{why:?}"))
                    .and_then(|(read, from)| take(world, read, from)),
            )
        }
        Post::Pair(request) => none(crate::pairing_ui::requested(world, request)),
        Post::Grant(cert) => none(crate::pairing_ui::granted(world, cert)),
        Post::Deposit(_) | Post::Collect { .. } => Err("this device keeps no mailbox".into()),
        Post::SyncLog { log, heads, cert } => {
            let from = from.ok_or("a sync comes straight from a device")?;
            crate::org_sync::answer(world, from, log, &heads, cert)
        }
        Post::LogEntries { log, entries } => {
            let from = from.ok_or("entries come straight from a device")?;
            none(crate::org_sync::take_entries(world, from, log, entries))
        }
        Post::Invite {
            log,
            org,
            name,
            devices,
            cert,
        } => none(crate::org_sync::invited(
            world, log, org, &name, devices, cert,
        )),
        Post::Mirror { cert, book } => none(mirrored(world, from, &cert, &book)),
    }
}

/// Takes in the posts that arrived.
fn receive(world: &mut World) {
    let Some(mut letters) = world.get_resource_mut::<cadrs_net::social::Letters>() else {
        return;
    };
    let arrived = letters.drain();
    if arrived.is_empty() {
        return;
    }
    ensure_loaded(world);
    for letter in arrived {
        let from = PublicKey(*letter.from.as_bytes());
        let result = match Post::decode(&letter.bytes) {
            Some(post) => handle(world, post, Some(from)),
            None => Err("not something cadrs sends".into()),
        };
        match result {
            Ok(answer) => letter.take_with(answer),
            Err(why) => letter.refuse(why),
        }
    }
}

fn take(world: &mut World, letter: letters::Letter, from: AccountId) -> Result<(), String> {
    let me = world
        .resource::<Social>()
        .keys
        .as_ref()
        .map(|k| k.account_id());
    if Some(letter.to) == me {
        let shop_name = world
            .resource::<Social>()
            .contacts
            .get(&from)
            .map_or_else(|| from.to_string()[..8].to_string(), |c| c.name.clone());
        let news = world
            .resource_mut::<Social>()
            .book
            .receive_learning(
                from,
                &shop_name,
                &letter.message,
                cadrs_social::wall_millis(),
            )
            .map_err(|e| format!("{e:?}"))?;
        save_book(world);
        let id = format!("letter-c-{}-{}", letter.message.request(), letter.sent);
        let (icon, title) = match news {
            News::Quoted { shop, total } => (
                "receipt",
                tr_with(
                    "social-notice-quoted",
                    &[("shop", &shop), ("total", &total)],
                ),
            ),
            News::Declined { shop } => (
                "close",
                tr_with("social-notice-declined", &[("shop", &shop)]),
            ),
            News::Stage { shop, stage } => (
                "info",
                tr_with(
                    "social-notice-stage",
                    &[("shop", &shop), ("stage", &format!("{stage:?}"))],
                ),
            ),
            News::Chat { shop, text } => ("chat", format!("{shop}: {text}")),
            News::Delivered { shop } => (
                "receipt",
                tr_with("social-notice-delivered", &[("shop", &shop)]),
            ),
        };
        notify(world, id, icon, title, String::new());
        return Ok(());
    }
    // A letter for one of our organizations.
    let org = world
        .resource::<Social>()
        .orgs
        .iter()
        .find(|o| o.account == letter.to)
        .cloned()
        .ok_or("not for us")?;
    if let Message::Request { client, .. } = &letter.message {
        add_contact(world, client.clone());
    }
    let client_name = world
        .resource::<Social>()
        .contacts
        .get(&from)
        .map_or_else(|| from.to_string()[..8].to_string(), |c| c.name.clone());
    let open_here = world
        .get_resource::<crate::organization::OpenOrg>()
        .is_some_and(|o| o.id == org.doc);
    let op_ok = if open_here {
        let op = {
            let open = world.resource::<crate::organization::OpenOrg>();
            letters::shop_op(open.session.org(), from, &letter.message)
                .map_err(|e| format!("{e:?}"))?
        };
        crate::organization::write(world, op)
    } else {
        let store = world.resource::<DocumentStore>().0.clone();
        let path = cadrs_org::file::path(&store.doc_dir(org.doc));
        let (log, _) = cadrs_org::file::load(&path).map_err(|e| e.to_string())?;
        let keys = world.resource::<Social>().keys.clone().ok_or("no keys")?;
        let mut session = Session::open(log, keys.device);
        let op =
            letters::shop_op(session.org(), from, &letter.message).map_err(|e| format!("{e:?}"))?;
        session.write(op, cadrs_social::wall_millis())?;
        cadrs_org::file::save(&path, session.log()).map_err(|e| e.to_string())?;
        true
    };
    if !op_ok {
        return Err("the shop's document refused it".into());
    }
    let id = format!("letter-s-{}-{}", org.doc, letter.sent);
    let title = match &letter.message {
        Message::Request { .. } => tr_with(
            "social-notice-request",
            &[("client", &client_name), ("shop", &org.name)],
        ),
        Message::Accept { .. } => tr_with(
            "social-notice-accepted",
            &[("client", &client_name), ("shop", &org.name)],
        ),
        _ => client_name,
    };
    notify(world, id, "receipt", title, String::new());
    Ok(())
}

/// The user's name, for their card.
pub fn my_name(world: &World) -> String {
    world.resource::<UserProfile>().display_name.clone()
}

/// Saves where the user said they are.
pub fn set_my_place(world: &mut World, place: Option<cadrs_social::place::Place>) {
    world.resource_mut::<Social>().my_place = place.clone();
    save(world, MY_PLACE, &place);
}

/// How often this device works out its shops' listings again, in seconds.
const LIST_EVERY: f64 = 60.0;

/// Takes in the listings heard in the public room, and says the listings of the shops this
/// device speaks for (those taking requests from anyone).
fn keep_directory(world: &mut World, mut last: Local<Option<f64>>) {
    // The mailboxes cards name follow the setting.
    if let Some(mailboxes) = world.get_resource::<MyMailboxes>().map(|m| m.0.clone())
        && world.resource::<Social>().mailboxes != mailboxes
    {
        world.resource_mut::<Social>().mailboxes = mailboxes;
    }
    let Some(heard) = world
        .get_resource_mut::<cadrs_net::shops::Shops>()
        .map(|mut s| s.take_heard())
    else {
        return;
    };
    if !heard.is_empty() {
        ensure_loaded(world);
        let mut changed = false;
        for bytes in heard {
            changed |= world
                .resource_mut::<Social>()
                .directory
                .hear(&bytes)
                .unwrap_or(false);
        }
        if changed {
            let directory = world.resource::<Social>().directory.clone();
            save(world, DIRECTORY, &directory);
        }
    }
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    if last.is_some_and(|t| now - t < LIST_EVERY) {
        return;
    }
    *last = Some(now);
    relist(world);
}

/// Works out the listings of this device's shops again (one changed: say so at once).
pub fn relist(world: &mut World) {
    if !world.contains_resource::<cadrs_net::shops::Shops>() {
        return;
    }
    ensure_loaded(world);
    let mine = my_listings(world);
    world.resource_mut::<cadrs_net::shops::Shops>().mine = mine;
}

/// The signed listings of the shops this device holds the key of and that take requests from
/// anyone.
fn my_listings(world: &World) -> Vec<Vec<u8>> {
    let social = world.resource::<Social>();
    let store = world.resource::<DocumentStore>().0.clone();
    let Some(keys) = social.keys.as_ref() else {
        return Vec::new();
    };
    let now = cadrs_social::wall_millis();
    social
        .orgs
        .iter()
        .filter_map(|mine| {
            let org = match world.get_resource::<crate::organization::OpenOrg>() {
                Some(open) if open.id == mine.doc => open.session.org().clone(),
                _ => {
                    let path = cadrs_org::file::path(&store.doc_dir(mine.doc));
                    let (log, _) = cadrs_org::file::load(&path).ok()?;
                    Session::open(log, keys.device.clone()).org().clone()
                }
            };
            if !org.profile.open_for_quotes {
                return None;
            }
            let account = org_account(&social.key_path, &mine.account)?;
            let card = social.org_card(&mine.account)?;
            let listing = cadrs_org::directory::ShopListing::of(&org, card, now);
            Some(cadrs_org::directory::encode(&Signed::new(
                account.key(),
                listing,
            )))
        })
        .collect()
}

/// Notes a device this one linked to the account: it goes on the user's card.
pub fn add_linked(world: &mut World, device: DeviceId) {
    ensure_loaded(world);
    let mut social = world.resource_mut::<Social>();
    if !social.linked.contains(&device) {
        social.linked.push(device);
    }
    let linked = social.linked.clone();
    save(world, LINKED, &linked);
}
