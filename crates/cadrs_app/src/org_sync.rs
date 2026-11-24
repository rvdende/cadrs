//! Keeping an organization's document the same on all its members' devices.
//!
//! Every [`SYNC_EVERY`] seconds (and right after a change here) each device asks each fellow
//! member's device for the entries of the log it lacks ([`Post::SyncLog`], with its heads and its
//! certificate); the answer brings them, and the answerer's heads, so the asker sends back what
//! the answerer lacks ([`Post::LogEntries`]). The log merges whatever arrives in any order.
//!
//! Only a device certified by a member gets anything. A member's device the organization
//! doesn't know yet introduces itself with its certificate; an admin's device adds it.
//!
//! What the log names but doesn't hold (the logo, a request's document: [`cadrs_org::blob`])
//! follows the same way: after a sync, a device asks the one it synced with for the blobs it
//! lacks ([`Post::WantBlobs`]), and checks each against its hash.
//!
//! *Add member…* sends the new member an [`Post::Invite`]: the organization's log and the devices
//! to sync with. It waits in the account menu until they join ([`Invitations`], [`join`]): an
//! invitation is from someone the user may never have heard of, so nothing is made in their
//! library before they say so. Only one from the user's own account (a device they just linked)
//! is taken at once.

use std::collections::BTreeSet;
use std::path::Path;

use bevy::prelude::*;
use cadrs_core::{Document, DocumentMeta};
use cadrs_org::session::Session;
use cadrs_org::{Op, Role};
use cadrs_social::keys::{AccountId, DeviceCert, DeviceId, Signed};
use cadrs_social::log::{Entry, EntryId, Log, LogId};
use cadrs_social::post::{Blobs, Post, Synced};
use cadrs_ui::i18n::{tr, tr_with};
use serde::{Deserialize, Serialize};

use crate::DocumentStore;
use crate::social::{MyOrg, SendKind, Social, ensure_loaded};

/// How often member devices sync, in seconds.
const SYNC_EVERY: f64 = 20.0;
const INVITE_FILE: &str = "invite.bin";
/// The invitations waiting to be joined, in the social folder.
const INVITATIONS_FILE: &str = "invitations.bin";
/// The most invitations kept waiting (the oldest go first): strangers can't fill the menu.
const MAX_INVITATIONS: usize = 20;
/// The most blobs asked for at once.
const BLOBS_AT_ONCE: usize = 16;

pub struct OrgSyncPlugin;

impl Plugin for OrgSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Invitations>()
            .add_systems(Update, sync_sometimes);
    }
}

/// An invitation kept beside an organization document until its log has synced.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invite {
    pub org: AccountId,
    pub name: String,
    pub devices: Vec<DeviceId>,
}

pub fn invite_of(doc_dir: &Path) -> Option<Invite> {
    let bytes = cadrs_fs::read(doc_dir.join(INVITE_FILE)).ok()?;
    cadrs_social::codec::decode(&bytes).ok()
}

fn org_by_log(world: &World, log: LogId) -> Option<MyOrg> {
    world
        .resource::<Social>()
        .orgs
        .iter()
        .find(|o| o.log == log)
        .cloned()
}

/// The organization's session: the one on screen, else read from disk.
fn session(world: &World, org: &MyOrg) -> Option<Session> {
    if let Some(open) = world.get_resource::<crate::organization::OpenOrg>()
        && open.id == org.doc
    {
        return Some(open.session.clone());
    }
    let keys = world.resource::<Social>().keys()?.clone();
    let store = world.resource::<DocumentStore>().0.clone();
    let path = cadrs_org::file::path(&store.doc_dir(org.doc));
    let log = match cadrs_org::file::load(&path) {
        Ok((log, _)) => log,
        // No file yet (just joined): an empty log. A file that can't be read is not one: with
        // no session nothing is synced into it, and nothing is saved over it.
        Err(_) if !cadrs_fs::is_file(&path) => Log::new(org.log),
        Err(e) => {
            warn!("{}: {e}", path.display());
            return None;
        }
    };
    Some(Session::open(log, keys.device))
}

/// Keeps a changed session: on screen and on disk.
fn keep(world: &mut World, org: &MyOrg, session: Session) -> Result<(), String> {
    let store = world.resource::<DocumentStore>().0.clone();
    let path = cadrs_org::file::path(&store.doc_dir(org.doc));
    cadrs_org::file::save(&path, session.log()).map_err(|e| e.to_string())?;
    if let Some(mut open) = world.get_resource_mut::<crate::organization::OpenOrg>()
        && open.id == org.doc
    {
        open.session = session;
    }
    Ok(())
}

/// Whether `device` is a member's device (known to the organization), at `now`.
fn is_member_device(session: &Session, device: &DeviceId, now: u64) -> bool {
    session.org().member_of(device, now).is_some()
}

/// A fellow member's device asks for what it lacks.
pub(crate) fn answer(
    world: &mut World,
    from: DeviceId,
    log: LogId,
    heads: &BTreeSet<EntryId>,
    cert: Signed<DeviceCert>,
) -> Result<Vec<u8>, String> {
    ensure_loaded(world);
    let org = org_by_log(world, log).ok_or("not an organization of this device")?;
    let mut session = session(world, &org).ok_or("no keys")?;
    let now = cadrs_social::wall_millis();
    let value = cert.verify().map_err(|_| "a forged certificate")?;
    if value.device != from {
        return Err("the certificate is another device's".into());
    }
    if !is_member_device(&session, &from, now) {
        // A member's device the organization doesn't know yet: an admin's device adds it.
        if !session.org().members.contains_key(&value.account) {
            return Err("not a member".into());
        }
        let me = world.resource::<Social>().keys().map(|k| k.device_id());
        let admin = me
            .and_then(|d| session.org().member_of(&d, now))
            .is_some_and(|(_, role)| role >= Role::Admin);
        if !admin {
            return Err("ask an admin's device".into());
        }
        session.write(Op::AddDevice(cert), now)?;
        keep(world, &org, session.clone())?;
    }
    let synced: Synced = (session.since(heads), session.log().heads().clone());
    Ok(cadrs_social::codec::encode(&synced))
}

/// Entries from a fellow member's device.
pub(crate) fn take_entries(
    world: &mut World,
    from: DeviceId,
    log: LogId,
    entries: Vec<Entry>,
) -> Result<(), String> {
    ensure_loaded(world);
    let org = org_by_log(world, log).ok_or("not an organization of this device")?;
    let mut session = session(world, &org).ok_or("no keys")?;
    if !is_member_device(&session, &from, cadrs_social::wall_millis()) {
        return Err("not a member's device".into());
    }
    receive(world, &org, &mut session, entries)
}

fn receive(
    world: &mut World,
    org: &MyOrg,
    session: &mut Session,
    entries: Vec<Entry>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let before = session.log().len();
    session.receive(entries, cadrs_social::wall_millis());
    if session.log().len() != before {
        keep(world, org, session.clone())?;
        // Its name and logo may have arrived (an invited device's first sync).
        if !session.org().profile.name.is_empty() {
            crate::social::org_changed(world, org.doc, session.org());
        }
        // A request another member declined: its document is let go here too.
        let dir = world.resource::<DocumentStore>().0.doc_dir(org.doc);
        cadrs_org::blob::sweep(&dir, session.org(), cadrs_social::wall_millis());
    }
    Ok(())
}

/// Asks `device` for the blobs organization `org` names and this device lacks.
fn ask_blobs(world: &mut World, org: &MyOrg, session: &Session, device: DeviceId) {
    let dir = world.resource::<DocumentStore>().0.doc_dir(org.doc);
    let missing = cadrs_org::blob::missing(&dir, session.org());
    let Some(keys) = world.resource::<Social>().keys().cloned() else {
        return;
    };
    if missing.is_empty() {
        return;
    }
    let post = Post::WantBlobs {
        log: org.log,
        cert: keys.cert.clone(),
        blobs: missing.iter().take(BLOBS_AT_ONCE).map(|b| b.hash).collect(),
    };
    crate::social::start(
        world,
        &[device],
        post.encode(),
        String::new(),
        SendKind::Blobs { log: org.log },
    );
}

/// A fellow member's device asks for blobs it lacks: those this device has.
pub(crate) fn blobs_wanted(
    world: &mut World,
    from: DeviceId,
    log: LogId,
    cert: &Signed<DeviceCert>,
    wanted: &[[u8; 32]],
) -> Result<Vec<u8>, String> {
    ensure_loaded(world);
    let org = org_by_log(world, log).ok_or("not an organization of this device")?;
    let session = session(world, &org).ok_or("the organization can't be read")?;
    let value = cert.verify().map_err(|_| "a forged certificate")?;
    if value.device != from || !is_member_device(&session, &from, cadrs_social::wall_millis()) {
        return Err("not a member's device".into());
    }
    let dir = world.resource::<DocumentStore>().0.doc_dir(org.doc);
    let answer = cadrs_org::blob::answer(&dir, wanted, &session.org().blobs());
    Ok(cadrs_social::codec::encode(&answer))
}

/// Blobs a fellow member's device sent: kept, when the organization names them.
pub(crate) fn took_blobs(world: &mut World, log: LogId, answer: &[u8]) {
    let Ok(blobs) = cadrs_social::codec::decode::<Blobs>(answer) else {
        return;
    };
    let Some(org) = org_by_log(world, log) else {
        return;
    };
    let Some(session) = session(world, &org) else {
        return;
    };
    let dir = world.resource::<DocumentStore>().0.doc_dir(org.doc);
    if cadrs_org::blob::take(&dir, session.org(), blobs) > 0 {
        // What shows them (the logo, a request's document) looks again.
        world.resource_mut::<Social>().logos_arrived += 1;
    }
}

/// The answer to our ask: take what we lacked, send what they lack.
pub(crate) fn synced(world: &mut World, log: LogId, device: DeviceId, answer: &[u8]) {
    let Ok((entries, theirs)) = cadrs_social::codec::decode::<Synced>(answer) else {
        return;
    };
    let Some(org) = org_by_log(world, log) else {
        return;
    };
    let Some(mut session) = session(world, &org) else {
        return;
    };
    let before = session.log().len();
    if let Err(why) = receive(world, &org, &mut session, entries) {
        warn!("syncing {}: {why}", org.name);
        return;
    }
    // An answer is a page ([`Session::since`]): while it brought something and they still have
    // more, the next page is asked for at once, not at the next round.
    if session.log().len() > before
        && !session.has_all(&theirs)
        && let Some(cert) = world.resource::<Social>().keys().map(|k| k.cert.clone())
    {
        ask(world, &org, &session, device, &cert);
    }
    let lacking = session.since(&theirs);
    if !lacking.is_empty() {
        let post = Post::LogEntries {
            log,
            entries: lacking,
        };
        crate::social::send_post(world, &[device], &post, String::new());
    }
    ask_blobs(world, &org, &session, device);
}

/// Asks every fellow member's device what this one lacks, for every organization.
pub fn sync_now(world: &mut World) {
    ensure_loaded(world);
    if !world.contains_resource::<cadrs_net::bevy_iroh::Iroh>() {
        return;
    }
    let social = world.resource::<Social>();
    let Some(keys) = social.keys().cloned() else {
        return;
    };
    let orgs = social.orgs.clone();
    let me = keys.device_id();
    for org in orgs {
        let Some(session) = session(world, &org) else {
            continue;
        };
        let mut devices: Vec<DeviceId> = session
            .org()
            .members
            .keys()
            .flat_map(|account| session.org().devices.of(account))
            .map(|cert| cert.device)
            .chain(org.peers.iter().copied())
            .filter(|d| *d != me)
            .collect();
        devices.sort();
        devices.dedup();
        for device in devices {
            ask(world, &org, &session, device, &keys.cert);
        }
    }
}

/// Asks `device` for the entries of `org`'s log this device lacks.
fn ask(
    world: &mut World,
    org: &MyOrg,
    session: &Session,
    device: DeviceId,
    cert: &Signed<DeviceCert>,
) {
    let post = Post::SyncLog {
        log: org.log,
        heads: session.log().heads().clone(),
        cert: cert.clone(),
    };
    crate::social::start(
        world,
        &[device],
        post.encode(),
        String::new(),
        SendKind::Sync {
            log: org.log,
            device,
        },
    );
}

fn sync_sometimes(world: &mut World, mut last: Local<Option<f64>>) {
    if !world.contains_resource::<cadrs_net::bevy_iroh::Iroh>() {
        *last = None;
        return;
    }
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    if last.is_some_and(|t| now - t < SYNC_EVERY) {
        return;
    }
    *last = Some(now);
    sync_now(world);
}

/// An invitation to an organization, waiting for the user to join.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invitation {
    pub log: LogId,
    pub org: AccountId,
    pub name: String,
    pub devices: Vec<DeviceId>,
    #[serde(default)]
    pub cert: Option<Signed<DeviceCert>>,
    /// The account that invited, when it said.
    #[serde(default)]
    pub inviter: Option<AccountId>,
}

/// The invitations waiting, oldest first (the account menu lists them).
#[derive(Resource, Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invitations(pub Vec<Invitation>);

/// The waiting invitations, read from the social folder the first time.
pub fn invitations(world: &mut World) -> Vec<Invitation> {
    ensure_loaded(world);
    world.resource::<Invitations>().0.clone()
}

pub(crate) fn load_invitations(world: &mut World) {
    let kept: Invitations = crate::social::load_file(world, INVITATIONS_FILE);
    world.insert_resource(kept);
}

fn save_invitations(world: &World) {
    crate::social::save_file(world, INVITATIONS_FILE, world.resource::<Invitations>());
}

/// What an invitation says, as it arrived ([`cadrs_social::post::Post::Invite`]).
pub(crate) struct Offered {
    pub log: LogId,
    pub org: AccountId,
    pub name: String,
    pub devices: Vec<DeviceId>,
    pub cert: Option<Signed<DeviceCert>>,
    pub inviter: Option<Signed<DeviceCert>>,
}

/// Someone made this user a member. From the user's own account (a device they linked) the
/// organization document is made at once; from anyone else the invitation waits in the account
/// menu until the user joins or declines it.
pub(crate) fn invited(
    world: &mut World,
    offered: Offered,
    from: Option<DeviceId>,
) -> Result<(), String> {
    let Offered {
        log,
        org,
        name,
        devices,
        cert,
        inviter,
    } = offered;
    ensure_loaded(world);
    // A name is the organization's to give, within what a name may be.
    let name: String = name
        .trim()
        .chars()
        .take(cadrs_org::profile::MAX_NAME)
        .collect();
    if name.is_empty() || devices.len() > 64 {
        return Err("not an invitation".into());
    }
    // Who invites is whoever's device handed this over: its certificate, and no other. A
    // certificate isn't a secret (every letter carries its sender's), so one that names another
    // device than the sender says nothing about who invites, and neither does an invitation
    // that came some other way than straight from a device.
    let inviter = inviter
        .as_ref()
        .and_then(|c| c.verify().ok())
        .filter(|c| Some(c.device) == from)
        .map(|c| c.account);
    let invitation = Invitation {
        log,
        org,
        name: name.clone(),
        devices,
        cert,
        inviter,
    };
    if let Some(known) = org_by_log(world, log) {
        // Already one of ours: only its certificate for this device may be news.
        if let Some(cert) = &invitation.cert {
            crate::social::keep_org_cert(world, &known.account, cert);
        }
        return Ok(());
    }
    let me = world.resource::<Social>().keys().map(|k| k.account_id());
    if inviter.is_some() && inviter == me {
        return join_invitation(world, invitation);
    }
    let known = inviter.is_some_and(|a| world.resource::<Social>().contacts.get(&a).is_some());
    let mut waiting = world.resource_mut::<Invitations>();
    if waiting.0.iter().any(|i| i.log == log) {
        return Ok(());
    }
    if waiting.0.len() >= MAX_INVITATIONS {
        // Full. A contact's invitation takes the oldest one's place; a stranger's is refused,
        // so strangers can neither push out the invitations that wait nor ring the bell
        // without end.
        if !known {
            return Err("too many invitations are waiting there already".into());
        }
        waiting.0.remove(0);
    }
    waiting.0.push(invitation);
    save_invitations(world);
    world
        .resource_mut::<crate::notifications::Notifications>()
        .push(crate::notifications::Notice {
            id: format!("invite-{}", cadrs_social::keys::PublicKey(log.0)),
            icon: "shop",
            title: tr_with("org-invited", &[("name", &name)]),
            body: tr("org-invited-how"),
            unread: true,
        });
    Ok(())
}

/// Joins the organization of the waiting invitation with this log: makes its document here, to
/// fill by syncing. False when there is no such invitation, or it can't be made.
pub fn join(world: &mut World, log: LogId) -> bool {
    let waiting = world.resource::<Invitations>();
    let Some(invitation) = waiting.0.iter().find(|i| i.log == log).cloned() else {
        return false;
    };
    // It waits on until the organization is here: one that couldn't be made can be tried again.
    match join_invitation(world, invitation) {
        Ok(()) => {
            world
                .resource_mut::<Invitations>()
                .0
                .retain(|i| i.log != log);
            save_invitations(world);
            true
        }
        Err(why) => {
            crate::linked::toast(world, why);
            false
        }
    }
}

/// Declines the waiting invitation with this log (the inviter isn't told).
pub fn decline(world: &mut World, log: LogId) {
    let mut waiting = world.resource_mut::<Invitations>();
    let before = waiting.0.len();
    waiting.0.retain(|i| i.log != log);
    if waiting.0.len() != before {
        save_invitations(world);
    }
}

fn join_invitation(world: &mut World, invitation: Invitation) -> Result<(), String> {
    let Invitation {
        log,
        org,
        name,
        devices,
        cert,
        ..
    } = invitation;
    if let Some(cert) = &cert {
        crate::social::keep_org_cert(world, &org, cert);
    }
    if org_by_log(world, log).is_some() {
        return Ok(());
    }
    let store = world.resource::<DocumentStore>().0.clone();
    let user = world.resource::<crate::UserProfile>().id.clone();
    let doc = Document::empty(name.clone());
    let mut meta = DocumentMeta::new(&user, world.resource::<crate::AppClock>().now());
    meta.organization = Some(cadrs_core::library::OrgRef { log: log.0 });
    store.create(&doc, &meta).map_err(|e| e.to_string())?;
    let dir = store.doc_dir(doc.id);
    let invite = Invite {
        org,
        name: name.clone(),
        devices: devices.clone(),
    };
    cadrs_fs::write(dir.join(INVITE_FILE), cadrs_social::codec::encode(&invite))
        .map_err(|e| e.to_string())?;
    cadrs_org::file::save(&cadrs_org::file::path(&dir), &Log::new(log))
        .map_err(|e| e.to_string())?;
    crate::social::add_org(world, doc.id, &name, org, log, devices);
    sync_now(world);
    Ok(())
}

/// Tells someone just made a member (their card) about the organization.
pub fn invite(world: &mut World, card: &cadrs_social::contact::Card) -> bool {
    ensure_loaded(world);
    let Some(open) = world.get_resource::<crate::organization::OpenOrg>() else {
        return false;
    };
    let org = open.session.org();
    let (Some(account), log, name) = (org.org, open.session.log().id(), org.profile.name.clone())
    else {
        return false;
    };
    let mut devices: Vec<DeviceId> = org
        .members
        .keys()
        .flat_map(|a| org.devices.of(a))
        .map(|c| c.device)
        .collect();
    if let Some(me) = world.resource::<Social>().keys().map(|k| k.device_id())
        && !devices.contains(&me)
    {
        devices.push(me);
    }
    let post = Post::Invite {
        log,
        org: account,
        name: name.clone(),
        devices,
        cert: None,
        inviter: world.resource::<Social>().keys().map(|k| k.cert.clone()),
    };
    crate::social::send_post(
        world,
        &card.devices,
        &post,
        tr_with("org-what-invite", &[("name", &name)]),
    )
}

/// A device just linked to the account (Link a device…): invite it to every organization this
/// device holds the key of, with the organization's certificate, so it can answer clients too.
pub fn invite_linked(world: &mut World, device: DeviceId) {
    ensure_loaded(world);
    let Some(me) = world.resource::<Social>().keys().map(|k| k.device_id()) else {
        return;
    };
    let orgs = world.resource::<Social>().orgs.clone();
    for org in orgs {
        let Some(key) = crate::social::org_key(world, &org.account) else {
            continue;
        };
        let cert = cadrs_social::keys::Account::from_key(key).certify(
            device,
            "",
            cadrs_social::wall_millis(),
        );
        let post = Post::Invite {
            log: org.log,
            org: org.account,
            name: org.name.clone(),
            devices: vec![me],
            cert: Some(cert),
            inviter: world.resource::<Social>().keys().map(|k| k.cert.clone()),
        };
        crate::social::send_post(world, &[device], &post, String::new());
    }
}
