//! The social side of cadrs on this device: its keys, its contacts, the requests for quotes it
//! sent, and the organizations (shops) it can speak for.
//!
//! Letters (`cadrs_org::letters`) arrive through `cadrs_net::social`. One for an organization
//! this device belongs to goes into that organization's document (opened from disk if it isn't
//! the one on screen); one for the user updates their [`ClientBook`]. Either way the bell says
//! so. [`send`] sends a letter to every device a contact's card lists; one taking it is enough.

use std::collections::HashMap;
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
use cadrs_social::post::{Blobs, Collected, Post};
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
#[derive(Clone, Debug, PartialEq)]
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
    /// Whether it is in the marketplace: then anyone may fetch its logo from this device.
    pub listed: bool,
    /// Its logo, for the account menu (the pictures are blobs in its document's folder).
    pub logo: Option<cadrs_org::Logo>,
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
    /// This device's shops' listings as last signed: (what they said, the bytes, when), so one
    /// that hasn't changed isn't signed and said again every minute.
    listed: HashMap<AccountId, ([u8; 32], Vec<u8>, u64)>,
    /// Other shops' logos asked for, and when (seconds since start), so each is asked for once
    /// in a while, not every frame.
    logos_asked: HashMap<[u8; 32], f64>,
    /// Counts up when a logo arrives: what shows logos looks again.
    pub logos_arrived: u32,
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
            .and_then(|()| cadrs_fs::write(&path, cadrs_social::codec::encode(cert)));
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

/// Reads `file` of this device's social folder ([`load`]).
pub(crate) fn load_file<T: serde::de::DeserializeOwned + Default>(world: &World, file: &str) -> T {
    load(world, file)
}

fn save<T: serde::Serialize>(world: &World, file: &str, value: &T) {
    save_bytes(world, file, cadrs_social::codec::encode(value));
}

/// Reads `file` of this device's social folder; the default when there is none. One that is
/// there but can't be read (damaged, or a newer cadrs's) is moved aside as
/// `<file>.unreadable`, so the next save doesn't write an empty one over what it held.
fn load<T: serde::de::DeserializeOwned + Default>(world: &World, file: &str) -> T {
    let Some(bytes) = read_file(world, file) else {
        return T::default();
    };
    match cadrs_social::codec::decode(&bytes) {
        Ok(value) => value,
        Err(e) => {
            set_aside(world, file, &e.to_string());
            T::default()
        }
    }
}

fn read_file(world: &World, file: &str) -> Option<Vec<u8>> {
    cadrs_fs::read(social_dir(world).join(file)).ok()
}

fn set_aside(world: &World, file: &str, why: &str) {
    warn!("{file} {why}: kept as {file}.unreadable");
    cadrs_social::codec::set_aside(&social_dir(world).join(file));
}

fn save_bytes(world: &World, file: &str, bytes: Vec<u8>) {
    if let Err(e) = cadrs_social::codec::write_whole(&social_dir(world).join(file), &bytes) {
        warn!("cannot save {file}: {e}");
    }
}

/// The shops heard of, from their file (stale listings dropped as it is read).
fn load_directory(world: &World) -> cadrs_org::directory::Directory {
    let Some(bytes) = read_file(world, DIRECTORY) else {
        return Default::default();
    };
    cadrs_org::directory::Directory::load(&bytes, cadrs_social::wall_millis()).unwrap_or_else(
        || {
            set_aside(world, DIRECTORY, "can't be read");
            Default::default()
        },
    )
}

fn save_directory(world: &World) {
    let bytes = world.resource::<Social>().directory.save();
    save_bytes(world, DIRECTORY, bytes);
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
    let directory = load_directory(world);
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
    crate::org_sync::load_invitations(world);
}

/// Looks again for this device's organizations when the documents that are organizations
/// aren't the ones known (one was removed from the device, or restored from the trash).
/// Otherwise nothing is read.
pub fn refresh_orgs(world: &mut World) {
    ensure_loaded(world);
    let store = world.resource::<DocumentStore>().0.clone();
    let there: std::collections::HashSet<DocumentId> = store
        .list()
        .0
        .entries
        .iter()
        .filter(|e| e.meta.trashed.is_none() && e.meta.organization.is_some())
        .map(|e| e.id)
        .collect();
    let social = world.resource::<Social>();
    let known: std::collections::HashSet<DocumentId> = social.orgs.iter().map(|o| o.doc).collect();
    if there == known {
        return;
    }
    let Some(keys) = social.keys.clone() else {
        return;
    };
    let orgs = find_orgs(world, &social.key_path.clone(), &keys);
    world.resource_mut::<Social>().orgs = orgs;
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
                listed: {
                    // While its document is read anyway: what it no longer wants kept goes
                    // (a request's document, a day after it was declined).
                    cadrs_org::blob::sweep(&dir, org, cadrs_social::wall_millis());
                    org.listed()
                },
                logo: org.profile.logo,
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
        .and_then(|b| cadrs_social::codec::decode::<Signed<DeviceCert>>(&b).ok())
        .filter(|c| {
            c.verify().is_ok() && c.value.device == keys.device_id() && &c.value.account == org
        });
    if kept.is_some() {
        return kept;
    }
    let account = org_account(key_path, org)?;
    let cert = account.certify(keys.device_id(), "", cadrs_social::wall_millis());
    let _ = cadrs_fs::write(&cert_path, cadrs_social::codec::encode(&cert));
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
        listed: false,
        logo: None,
    });
}

/// An organization of this device's changed: what the account menu shows of it follows.
pub fn org_changed(world: &mut World, doc: DocumentId, org: &cadrs_org::Org) {
    let mut social = world.resource_mut::<Social>();
    let listed = org.listed();
    if let Some(mine) = social.orgs.iter_mut().find(|o| o.doc == doc)
        && (mine.name != org.profile.name || mine.logo != org.profile.logo || mine.listed != listed)
    {
        mine.name = org.profile.name.clone();
        mine.logo = org.profile.logo;
        mine.listed = listed;
    }
}

/// Writes `op` into organization `account`'s document as something that arrived (not the
/// person's own doing): the open one's, else the one on disk.
pub(crate) fn write_org(
    world: &mut World,
    account: &AccountId,
    op: cadrs_org::Op,
) -> Result<(), String> {
    let open_here = world
        .get_resource::<crate::organization::OpenOrg>()
        .is_some_and(|open| open.session.org().org.as_ref() == Some(account));
    if open_here {
        return crate::organization::take_op(world, op);
    }
    let social = world.resource::<Social>();
    let mine = social
        .orgs
        .iter()
        .find(|o| &o.account == account)
        .ok_or("not an organization of this device")?;
    let keys = social.keys.clone().ok_or("no keys")?;
    let store = world.resource::<DocumentStore>().0.clone();
    let path = cadrs_org::file::path(&store.doc_dir(mine.doc));
    let (log, _) = cadrs_org::file::load(&path).map_err(|e| e.to_string())?;
    let mut session = Session::open(log, keys.device);
    session.write(op, cadrs_social::wall_millis())?;
    cadrs_org::file::save(&path, session.log()).map_err(|e| e.to_string())
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
        book: cadrs_social::codec::encode(&social.book),
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
    let theirs: ClientBook = cadrs_social::codec::decode(book).map_err(|_| "not a book")?;
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
    /// Straight to the recipient's devices; left at `fallback` when none is reached. One that
    /// is reached and refuses it is another matter: it isn't left anywhere, and the sender is
    /// told why. `request` is the request for a quote this carries, if it is one.
    Deliver {
        fallback: Option<Fallback>,
        request: Option<cadrs_org::Id>,
    },
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
    /// Asking a fellow member's device for blobs of an organization this one lacks.
    Blobs { log: cadrs_social::log::LogId },
    /// Asking a listed shop's devices for the logo its listing names.
    Logo { blob: cadrs_org::BlobRef },
    /// Telling a device it now belongs to this account (Link a device…). Once it has taken
    /// that in, the account's organizations are sent after it: an invitation counts as the
    /// account's own only to a device that is already on the account.
    Grant { device: DeviceId },
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
    let request = match &message {
        Message::Request { id, .. } => Some(*id),
        _ => None,
    };
    let letters::Composed { post, envelopes } = letters::compose(
        &keys.device,
        &cert,
        to,
        message,
        cadrs_social::wall_millis(),
    );
    // Left at a couple of the mailboxes the card names, not at all of them.
    let fallback = (!to.mailboxes.is_empty()).then(|| Fallback {
        mailboxes: dialed(&to.mailboxes)
            .into_iter()
            .take(letters::MAX_MAILBOXES)
            .collect(),
        envelopes,
    });
    start(
        world,
        &to.devices,
        post,
        what,
        SendKind::Deliver { fallback, request },
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
    let (fallback, request) = (None, None);
    start(
        world,
        devices,
        post.encode(),
        what,
        SendKind::Deliver { fallback, request },
    )
}

/// What was sent was refused by whoever it was for: said, in the catalogs' words where there
/// are any for the reason, and a request for a quote the shop didn't take is marked so in the
/// sender's list (declined, with the reason), not left there as if it were waiting.
fn refused(world: &mut World, what: &str, why: &str, request: Option<cadrs_org::Id>) {
    // The reason is the other device's to give, and so is its length: a sentence of it is
    // shown and kept, whatever the transport let through.
    let why: String = why.chars().take(cadrs_social::post::MAX_REASON).collect();
    let reason = crate::organization::refusal(&why);
    crate::linked::toast(
        world,
        tr_with("social-refused", &[("what", what), ("why", &reason)]),
    );
    let Some(request) = request else {
        return;
    };
    let mut social = world.resource_mut::<Social>();
    if let Some(sent) = social.book.sent.get_mut(&request)
        && sent.stage == cadrs_org::Stage::Requested
    {
        sent.stage = cadrs_org::Stage::Declined;
        sent.decline_reason = Some(reason);
        save_book(world);
    }
}

/// The most devices one thing is sent to at a time.
pub(crate) const MAX_DIALED: usize = 8;

/// The devices to dial of those a card (or a member list) names: each once, and never more than
/// a handful, whatever it names. The first ones: a card lists the most likely online first.
pub(crate) fn dialed(devices: &[DeviceId]) -> Vec<DeviceId> {
    let mut dial: Vec<DeviceId> = Vec::new();
    for device in devices {
        if dial.len() < MAX_DIALED && !dial.contains(device) {
            dial.push(*device);
        }
    }
    dial
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
    let tasks: Vec<_> = dialed(devices)
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

/// [`start`], saying nothing when it can't (a background ask: offline is no news).
fn start_quiet(world: &mut World, devices: &[DeviceId], bytes: Vec<u8>, kind: SendKind) -> bool {
    let Some(iroh) = world.get_resource::<Iroh>() else {
        return false;
    };
    let hints = world.get_resource::<PeerAddrs>();
    let tasks: Vec<_> = dialed(devices)
        .iter()
        .filter_map(|d| EndpointId::from_bytes(&d.0).ok())
        .map(|d| {
            let addr = hints
                .and_then(|h| h.0.get(&d).cloned())
                .unwrap_or_else(|| d.into());
            cadrs_net::social::start_send(iroh, addr, bytes.clone())
        })
        .collect();
    if tasks.is_empty() {
        return false;
    }
    world.resource_mut::<Outbox>().0.push(Sending {
        what: String::new(),
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
    Blobs(cadrs_social::log::LogId, Vec<u8>),
    Logo(cadrs_org::BlobRef, Vec<u8>),
    Granted(String, DeviceId),
    /// Reached and refused: (what, why, the request for a quote it was).
    Refused(String, String, Option<cadrs_org::Id>),
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
            SendKind::Blobs { log } => {
                for answer in answers.drain(..) {
                    done.push(Done::Blobs(*log, answer));
                }
                !s.tasks.is_empty()
            }
            // The first device that has it is enough.
            SendKind::Logo { blob } => match answers.pop() {
                Some(answer) => {
                    done.push(Done::Logo(*blob, answer));
                    false
                }
                None => !s.tasks.is_empty(),
            },
            _ if !answers.is_empty() => {
                done.push(match s.kind {
                    SendKind::Grant { device } => Done::Granted(s.what.clone(), device),
                    SendKind::Deposit => Done::Deposited(s.what.clone()),
                    _ => Done::Delivered(s.what.clone()),
                });
                false
            }
            _ if !s.tasks.is_empty() => true,
            SendKind::Deliver { fallback, request } => {
                // A device that was reached and said no: that is the answer. Leaving the
                // letter at a mailbox would only have it refused again later, unseen.
                let refused = s
                    .errors
                    .iter()
                    .find_map(|e| cadrs_social::post::refusal(e))
                    .map(str::to_owned);
                done.push(match (refused, fallback.take()) {
                    (Some(why), _) => Done::Refused(s.what.clone(), why, *request),
                    (None, Some(f)) => Done::Deposit(s.what.clone(), f),
                    (None, None) => Done::Failed(s.what.clone(), s.errors.join("; ")),
                });
                false
            }
            SendKind::Deposit | SendKind::Grant { .. } => {
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
            Done::Refused(what, why, request) => refused(world, &what, &why, request),
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
            Done::Blobs(log, answer) => crate::org_sync::took_blobs(world, log, &answer),
            Done::Logo(blob, answer) => took_logo(world, &blob, &answer),
            Done::Granted(what, device) => {
                crate::linked::toast(world, tr_with("social-sent", &[("what", &what)]));
                // The account's organizations come along to the new device.
                crate::org_sync::invite_linked(world, device);
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
    let Ok(collected) = cadrs_social::codec::decode::<Collected>(answer) else {
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
        if let Some(post) = Post::decode(&bytes).ok()
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
                    .map_err(|why| why.reason())
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
            inviter,
        } => none(crate::org_sync::invited(
            world,
            crate::org_sync::Offered {
                log,
                org,
                name,
                devices,
                cert,
                inviter,
            },
            from,
        )),
        Post::Mirror { cert, book } => none(mirrored(world, from, &cert, &book)),
        Post::WantBlobs { log, cert, blobs } => {
            let from = from.ok_or("blobs are asked for straight from a device")?;
            crate::org_sync::blobs_wanted(world, from, log, &cert, &blobs)
        }
        Post::PublicBlob { org, hash } => public_blob(world, &org, &hash),
        // Someone says they paid a wallet kept here (the user's, an organization's).
        Post::Credited(note) => none(crate::credits::credited(world, &note)),
    }
}

/// Anyone asks for a picture a listed organization's listing names: its logo, if this device
/// has it. Nothing else of the organization's is handed out this way.
fn public_blob(world: &mut World, org: &AccountId, hash: &[u8; 32]) -> Result<Vec<u8>, String> {
    let social = world.resource::<Social>();
    let mine = social
        .orgs
        .iter()
        .find(|o| &o.account == org)
        .ok_or("not an organization of this device")?;
    // Only a listed organization's logo is ever public. Whether it is listed and what its logo
    // is are kept as the organization changes ([`org_changed`]), so a stranger's asking costs
    // one small file read: the organization's document isn't opened for it.
    let logo = mine
        .logo
        .filter(|_| mine.listed)
        .into_iter()
        .flat_map(|logo| [logo.full, logo.thumb])
        .find(|blob| blob.hash == *hash)
        .ok_or("not something this organization shows")?;
    let dir = world.resource::<DocumentStore>().0.doc_dir(mine.doc);
    let allowed = std::collections::BTreeSet::from([logo]);
    let answer = cadrs_org::blob::answer(&dir, &[*hash], &allowed);
    Ok(cadrs_social::codec::encode(&answer))
}

/// An organization of this device's as it is now: the open one's state, else read from disk.
pub(crate) fn org_state(world: &World, mine: &MyOrg) -> Option<cadrs_org::Org> {
    if let Some(open) = world.get_resource::<crate::organization::OpenOrg>()
        && open.id == mine.doc
    {
        return Some(open.session.org().clone());
    }
    let keys = world.resource::<Social>().keys.as_ref()?;
    let store = &world.resource::<DocumentStore>().0;
    let path = cadrs_org::file::path(&store.doc_dir(mine.doc));
    let (log, _) = cadrs_org::file::load(&path).ok()?;
    Some(Session::open(log, keys.device.clone()).org().clone())
}

// ---------------------------------------------------------------------------------------------
// Other shops' logos, for the marketplace

/// How long before a logo that didn't come is asked for again, in seconds.
const ASK_LOGO_AGAIN: f64 = 600.0;

fn logo_path(world: &World, hash: &[u8; 32]) -> PathBuf {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    social_dir(world).join("logos").join(hex)
}

/// A listed shop's logo, when it is here: this device's own organization's from its document,
/// another's from what was fetched before.
pub fn logo_bytes(world: &World, blob: &cadrs_org::BlobRef) -> Option<Vec<u8>> {
    let store = &world.resource::<DocumentStore>().0;
    let own = world
        .resource::<Social>()
        .orgs
        .iter()
        .filter(|o| o.logo.is_some_and(|l| l.thumb == *blob || l.full == *blob))
        .find_map(|o| cadrs_org::blob::get(&store.doc_dir(o.doc), &blob.hash));
    own.or_else(|| {
        let path = logo_path(world, &blob.hash);
        // Never more than a logo's worth read of a file that claims to be one.
        if cadrs_fs::metadata(&path).ok()?.len() > cadrs_org::profile::MAX_LOGO_BYTES {
            return None;
        }
        let bytes = cadrs_fs::read(path).ok()?;
        (cadrs_org::BlobRef::of(&bytes) == *blob).then_some(bytes)
    })
}

/// Asks the shop on `card` for the logo `blob` its listing names, unless it was asked for
/// lately. It arrives in [`logo_bytes`] ([`Social::logos_arrived`] counts up).
pub fn want_logo(world: &mut World, card: &Card, blob: cadrs_org::BlobRef) {
    if blob.len > cadrs_org::profile::MAX_LOGO_BYTES || !world.contains_resource::<Iroh>() {
        return;
    }
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    let mut social = world.resource_mut::<Social>();
    if social
        .logos_asked
        .get(&blob.hash)
        .is_some_and(|at| now - at < ASK_LOGO_AGAIN)
    {
        return;
    }
    social.logos_asked.insert(blob.hash, now);
    let post = Post::PublicBlob {
        org: card.account,
        hash: blob.hash,
    };
    start_quiet(world, &card.devices, post.encode(), SendKind::Logo { blob });
}

fn took_logo(world: &mut World, blob: &cadrs_org::BlobRef, answer: &[u8]) {
    let Ok(Blobs(blobs)) = cadrs_social::codec::decode::<Blobs>(answer) else {
        return;
    };
    let Some(bytes) = blobs
        .into_iter()
        .find(|b| cadrs_org::BlobRef::of(b) == *blob)
    else {
        return;
    };
    // Its hash is right; whether it is a logo at all is another matter. What isn't one is
    // never kept (nor asked for again for a while: the asking was noted).
    if !crate::org_logo::is_logo(&bytes) {
        warn!("a shop's logo isn't a logo: not kept");
        return;
    }
    keep_logo(world, &bytes);
}

/// Keeps a listed shop's logo where [`logo_bytes`] finds it.
pub(crate) fn keep_logo(world: &mut World, bytes: &[u8]) {
    let path = logo_path(world, &cadrs_org::BlobRef::of(bytes).hash);
    let kept = path
        .parent()
        .map_or(Ok(()), cadrs_fs::create_dir_all)
        .and_then(|()| cadrs_fs::write(&path, bytes));
    match kept {
        Ok(()) => world.resource_mut::<Social>().logos_arrived += 1,
        Err(e) => warn!("cannot keep a shop's logo: {e}"),
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
            Ok(post) => handle(world, post, Some(from)),
            Err(e) if e.is_newer() => {
                Err("this device's cadrs is older than yours: it can't take that yet".into())
            }
            Err(_) => Err("not something cadrs sends".into()),
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
        let known = world.resource::<Social>().contacts.get(&from).is_some();
        let news = world
            .resource_mut::<Social>()
            .book
            .receive_learning(
                from,
                &shop_name,
                &letter.message,
                cadrs_social::wall_millis(),
                known,
            )
            .map_err(|e| e.reason())?;
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
                    &[
                        ("shop", &shop),
                        ("stage", &tr(crate::organization::stage_key(stage))),
                    ],
                ),
            ),
            News::Chat { shop, text } => {
                // A notice is a line, not the message.
                let start: String = text.chars().take(120).collect();
                ("chat", format!("{shop}: {start}"))
            }
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
    // Who it is from, as the notice says it: a contact's name, else the name on the card a
    // request comes with. Asking for a quote doesn't make a stranger one of the user's
    // contacts (those are who the user chose to trust); the request keeps the card to answer.
    let carded = match &letter.message {
        Message::Request { client, .. } => Some(client.value.name.trim().to_owned()),
        _ => None,
    }
    .filter(|name| !name.is_empty())
    .map(|name| name.chars().take(cadrs_org::profile::MAX_NAME).collect());
    let client_name = world
        .resource::<Social>()
        .contacts
        .get(&from)
        .map(|c| c.name.clone())
        .or(carded)
        .unwrap_or_else(|| from.to_string()[..8].to_string());
    let open_here = world
        .get_resource::<crate::organization::OpenOrg>()
        .is_some_and(|o| o.id == org.doc);
    let op_ok = if open_here {
        let op = {
            let open = world.resource::<crate::organization::OpenOrg>();
            letters::shop_op(open.session.org(), from, &letter.message, letter.sent)
                .map_err(|e| e.reason())?
        };
        crate::organization::take_op(world, op)?;
        true
    } else {
        let store = world.resource::<DocumentStore>().0.clone();
        let path = cadrs_org::file::path(&store.doc_dir(org.doc));
        let (log, _) = cadrs_org::file::load(&path).map_err(|e| e.to_string())?;
        let keys = world.resource::<Social>().keys.clone().ok_or("no keys")?;
        let mut session = Session::open(log, keys.device);
        let op = letters::shop_op(session.org(), from, &letter.message, letter.sent)
            .map_err(|e| e.reason())?;
        session.write(op, cadrs_social::wall_millis())?;
        cadrs_org::file::save(&path, session.log()).map_err(|e| e.to_string())?;
        true
    };
    if !op_ok {
        return Err("the shop's document refused it".into());
    }
    // The document the client sent, kept beside the organization's log now that the log took
    // the request (one it refused leaves nothing behind): for this and the other members'
    // devices to open.
    if let Message::Request { snapshot, .. } = &letter.message
        && !snapshot.is_empty()
    {
        let dir = world.resource::<DocumentStore>().0.doc_dir(org.doc);
        if let Err(e) = cadrs_org::blob::put(&dir, snapshot) {
            warn!("cannot keep a request's document: {e}");
        }
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
/// How long the directory's file may be behind what was heard, in seconds: it is some
/// megabytes when full, and news comes all the time.
const SAVE_DIRECTORY_EVERY: f64 = 30.0;

/// What [`keep_directory`] remembers between frames.
#[derive(Default)]
struct Keeping {
    listed_at: Option<f64>,
    /// The listings heard, on their way in at a pace a flood can't force.
    intake: cadrs_org::directory::Intake,
    /// The directory changed since its file was written; when it was last written.
    dirty: bool,
    saved_at: f64,
}

fn keep_directory(world: &mut World, mut state: Local<Keeping>) {
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
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    state.intake.queue(heard);
    if state.intake.waiting() > 0 {
        ensure_loaded(world);
        // Who is never dropped to make room: the shops the user knows, and their own.
        let known: Vec<AccountId> = {
            let social = world.resource::<Social>();
            social
                .contacts
                .organizations()
                .map(|card| card.account)
                .chain(social.orgs.iter().map(|o| o.account))
                .collect()
        };
        let wall = cadrs_social::wall_millis();
        let mut social = world.resource_mut::<Social>();
        // Only news marks what shows the shops as changed.
        let directory = &mut social.bypass_change_detection().directory;
        directory.protect(known);
        if state.intake.take(directory, now, wall) {
            social.set_changed();
            state.dirty = true;
        }
    }
    if state.dirty && now - state.saved_at > SAVE_DIRECTORY_EVERY {
        state.dirty = false;
        state.saved_at = now;
        save_directory(world);
        forget_unlisted_logos(world);
    }
    if state.listed_at.is_some_and(|t| now - t < LIST_EVERY) {
        return;
    }
    state.listed_at = Some(now);
    relist(world);
}

/// Removes the fetched logos no listing names any more (the shop left, or changed its logo).
fn forget_unlisted_logos(world: &World) {
    let named = world.resource::<Social>().directory.logos();
    let Ok(entries) = cadrs_fs::read_dir(social_dir(world).join("logos")) else {
        return;
    };
    for entry in entries.flatten() {
        // A file is named by its hash: read once, looked up in the set.
        let listed = entry
            .file_name()
            .to_str()
            .and_then(cadrs_org::blob::hash_of)
            .is_some_and(|hash| named.contains(&hash));
        if !listed {
            let _ = cadrs_fs::remove_file(entry.path());
        }
    }
}

/// How long a listing that hasn't changed stands before it is signed afresh, in milliseconds:
/// well inside how long a directory believes one ([`cadrs_org::directory::KEEP_FOR`]).
const RESIGN_AFTER: u64 = 24 * 60 * 60 * 1000;

/// Works out the listings of this device's shops again (one changed: say so at once).
pub fn relist(world: &mut World) {
    ensure_loaded(world);
    let mine = my_listings(world);
    // This device's directory hears them too (the room doesn't say one's own words back): the
    // marketplace shows the user their own shop as others see it.
    let wall = cadrs_social::wall_millis();
    let mut changed = false;
    for bytes in &mine {
        changed |= world
            .resource_mut::<Social>()
            .directory
            .hear(bytes, wall)
            .unwrap_or(false);
    }
    if changed {
        save_directory(world);
    }
    // Said in the room while this cadrs is online.
    if let Some(mut shops) = world.get_resource_mut::<cadrs_net::shops::Shops>()
        && shops.mine != mine
    {
        shops.mine = mine;
    }
}

/// In [`Social::listed`], what a shop that left the marketplace "said": its withdrawal.
const WITHDRAWN: [u8; 32] = [0; 32];

/// The signed listings of the organizations this device holds the key of and that are listed
/// (they asked to be, and their profile lacks nothing), and the withdrawals of those that were
/// and aren't any more. One that says what it said before
/// keeps its signature and date, so it isn't said again in the room as if it were news.
fn my_listings(world: &mut World) -> Vec<Vec<u8>> {
    let now = cadrs_social::wall_millis();
    let orgs = world.resource::<Social>().orgs.clone();
    let mut signed = Vec::new();
    let mut keep = HashMap::new();
    for mine in &orgs {
        let social = world.resource::<Social>();
        let Some(account) = org_account(&social.key_path, &mine.account) else {
            continue;
        };
        let Some(card) = social.org_card(&mine.account) else {
            continue;
        };
        // One that isn't listed, never was and isn't shown has nothing to say: its document
        // isn't opened for the asking.
        if !mine.listed
            && !social.listed.contains_key(&mine.account)
            && social.directory.get(&mine.account).is_none()
        {
            continue;
        }
        let Some(org) = org_state(world, mine) else {
            continue;
        };
        let listing = cadrs_org::directory::ShopListing::of(&org, card.clone(), now).map(|l| {
            // This device says, with its own key, that what it publishes is the
            // organization's: its public documents are then shown as the organization's designs.
            let publishes = cadrs_org::directory::Publishes {
                org: mine.account,
                issued: l.issued,
            };
            cadrs_org::directory::ShopListing {
                publishers: social
                    .keys()
                    .map(|keys| Signed::new(&keys.device, publishes))
                    .into_iter()
                    .collect(),
                ..l
            }
        });
        let Some(listing) = listing else {
            // Not listed (any more). If the marketplace still shows it, it is taken out: said
            // until this device's own directory has heard it, and for a day after so that peers
            // that were away hear it too.
            let shown = social.directory.get(&mine.account).is_some();
            let before = social.listed.get(&mine.account).cloned();
            let entry = match before {
                Some(entry) if entry.0 == WITHDRAWN => {
                    (now.saturating_sub(entry.2) < RESIGN_AFTER).then_some(entry)
                }
                _ if shown || before.is_some() => {
                    let gone = cadrs_org::directory::ShopListing::withdrawal(card, now);
                    cadrs_org::directory::sign(account.key(), &gone)
                        .map(|bytes| (WITHDRAWN, bytes, now))
                }
                _ => None,
            };
            if let Some(entry) = entry {
                signed.push(entry.1.clone());
                keep.insert(mine.account, entry);
            }
            continue;
        };
        // What it says, whenever it is said.
        let says = cadrs_org::directory::ShopListing {
            issued: 0,
            card: Card {
                issued: 0,
                ..listing.card.value.clone()
            }
            .sign(account.key()),
            ..listing.clone()
        };
        let said = *blake3::hash(&cadrs_social::codec::encode(&(
            &says.card.value,
            &says.tagline,
            &says.about,
            (&says.kinds, &says.logo, &says.logo_full, &says.places),
            (&says.offers, &says.email, &says.website, &says.rating),
        )))
        .as_bytes();
        let fresh = social
            .listed
            .get(&mine.account)
            .filter(|(before, _, at)| *before == said && now.saturating_sub(*at) < RESIGN_AFTER)
            .cloned();
        let entry = match fresh {
            Some(entry) => entry,
            None => match cadrs_org::directory::sign(account.key(), &listing) {
                Some(bytes) => (said, bytes, now),
                None => continue,
            },
        };
        signed.push(entry.1.clone());
        keep.insert(mine.account, entry);
    }
    world.resource_mut::<Social>().listed = keep;
    signed
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A card can name any number of devices; one thing sent dials a handful of them, each
    /// once, the first ones named.
    #[test]
    fn only_a_handful_of_devices_are_dialed() {
        let device = |n: u8| PublicKey([n; 32]);
        let many: Vec<DeviceId> = (0..200u8).map(device).collect();
        assert_eq!(dialed(&many), many[..MAX_DIALED]);
        // Fifty thousand times the same one is one.
        assert_eq!(dialed(&vec![device(7); 50_000]), [device(7)]);
        assert_eq!(
            dialed(&[device(1), device(2), device(1)]),
            [device(1), device(2)]
        );
        assert!(dialed(&[]).is_empty());
    }
}
