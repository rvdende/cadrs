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
//! *Add member…* sends the new member an [`Post::Invite`]: the organization's log and the devices
//! to sync with. Their cadrs makes the organization document and fills it by syncing.

use std::collections::BTreeSet;
use std::path::Path;

use bevy::prelude::*;
use cadrs_core::{Document, DocumentMeta};
use cadrs_org::session::Session;
use cadrs_org::{Op, Role};
use cadrs_social::keys::{AccountId, DeviceCert, DeviceId, Signed};
use cadrs_social::log::{Entry, EntryId, Log, LogId};
use cadrs_social::post::{Post, Synced};
use cadrs_ui::i18n::tr_with;
use serde::{Deserialize, Serialize};

use crate::DocumentStore;
use crate::social::{MyOrg, SendKind, Social, ensure_loaded};

/// How often member devices sync, in seconds.
const SYNC_EVERY: f64 = 20.0;
const INVITE_FILE: &str = "invite.bin";

pub struct OrgSyncPlugin;

impl Plugin for OrgSyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_sometimes);
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
    postcard::from_bytes(&bytes).ok()
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
    let log = cadrs_org::file::load(&path)
        .map(|(log, _)| log)
        .unwrap_or_else(|_| Log::new(org.log));
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
    Ok(postcard::to_allocvec(&synced).expect("encodes"))
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
        // Its name may have arrived (an invited device's first sync).
        let name = session.org().profile.name.clone();
        if !name.is_empty()
            && let Some(mine) = world
                .resource_mut::<Social>()
                .orgs
                .iter_mut()
                .find(|o| o.doc == org.doc)
        {
            mine.name = name;
        }
    }
    Ok(())
}

/// The answer to our ask: take what we lacked, send what they lack.
pub(crate) fn synced(world: &mut World, log: LogId, device: DeviceId, answer: &[u8]) {
    let Ok((entries, theirs)) = postcard::from_bytes::<Synced>(answer) else {
        return;
    };
    let Some(org) = org_by_log(world, log) else {
        return;
    };
    let Some(mut session) = session(world, &org) else {
        return;
    };
    if let Err(why) = receive(world, &org, &mut session, entries) {
        warn!("syncing {}: {why}", org.name);
        return;
    }
    let lacking = session.since(&theirs);
    if !lacking.is_empty() {
        let post = Post::LogEntries {
            log,
            entries: lacking,
        };
        crate::social::send_post(world, &[device], &post, String::new());
    }
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
            let post = Post::SyncLog {
                log: org.log,
                heads: session.log().heads().clone(),
                cert: keys.cert.clone(),
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
    }
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

/// Someone made this user a member: make the organization document here, to fill by syncing.
pub(crate) fn invited(
    world: &mut World,
    log: LogId,
    org: AccountId,
    name: &str,
    devices: Vec<DeviceId>,
    cert: Option<Signed<DeviceCert>>,
) -> Result<(), String> {
    ensure_loaded(world);
    if let Some(cert) = &cert {
        crate::social::keep_org_cert(world, &org, cert);
    }
    if org_by_log(world, log).is_some() {
        return Ok(());
    }
    let store = world.resource::<DocumentStore>().0.clone();
    let user = world.resource::<crate::UserProfile>().id.clone();
    let doc = Document::empty(name.to_owned());
    let mut meta = DocumentMeta::new(&user, world.resource::<crate::AppClock>().now());
    meta.organization = Some(cadrs_core::library::OrgRef { log: log.0 });
    store.create(&doc, &meta).map_err(|e| e.to_string())?;
    let dir = store.doc_dir(doc.id);
    let invite = Invite {
        org,
        name: name.to_owned(),
        devices: devices.clone(),
    };
    cadrs_fs::write(
        dir.join(INVITE_FILE),
        postcard::to_allocvec(&invite).expect("encodes"),
    )
    .map_err(|e| e.to_string())?;
    cadrs_org::file::save(&cadrs_org::file::path(&dir), &Log::new(log))
        .map_err(|e| e.to_string())?;
    crate::social::add_org(world, doc.id, name, org, log, devices);
    world
        .resource_mut::<crate::notifications::Notifications>()
        .push(crate::notifications::Notice {
            id: format!("invite-{}", doc.id),
            icon: "users",
            title: tr_with("org-invited", &[("name", name)]),
            body: String::new(),
            unread: true,
        });
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
        };
        crate::social::send_post(world, &[device], &post, String::new());
    }
}
