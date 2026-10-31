//! Versions and branches of a shared document (Onshape's Versions and History, editing
//! together).
//!
//! - **Versions are the document's**: whoever may edit makes one with **Create version** as
//!   usual. The owner's cadrs makes it of the document as everyone has it (its edits numbered
//!   first) and tells the room ([`VersionCreated`]); everyone else makes the same version, with
//!   the same id, of their copy at that edit. Someone else's request goes to the owner.
//! - **Main is the shared workspace**: the live session follows Main (`super::live`). A branch
//!   is one's own: switching to it leaves the room (the document is then edited only here),
//!   and switching back to Main joins again and catches up. Restore and Merge into Main are
//!   commands like any other, so they reach everyone.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use cadrs_core::Document;
use cadrs_core::collab::Permission;
use cadrs_core::history_log::VersionId;
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::bevy_iroh::prelude::{NetSender, Received};
use cadrs_net::{Answer, Ask, Request, Verdict, VersionCreated, transfer};
use cadrs_ui::i18n::{tr, tr_with};

use super::live::{GuestState, Live, Role};
use crate::history_panel::DocLog;
use crate::{ActiveDocument, AppClock, DocumentStore, UserProfile};

pub(super) fn plugin(app: &mut App) {
    app.add_message::<Received<VersionCreated>>()
        .init_resource::<Waiting>()
        .add_systems(
            Update,
            (collect, hear, poll)
                .chain()
                .run_if(resource_exists::<Iroh>),
        );
}

/// Versions announced ahead of the edit they were made at, and requests on their way.
#[derive(Resource, Default)]
struct Waiting {
    versions: Vec<VersionCreated>,
    asks: Vec<Answer<Verdict>>,
}

/// Makes version `id` of `doc` in the open document's log (catching the log up to `doc` first,
/// then back to the open document). Returns its name. Nothing if the log has it already.
fn make_version(
    world: &mut World,
    doc: &Document,
    id: VersionId,
    name: &str,
    description: &str,
    time: i64,
    user: &str,
) -> Option<String> {
    let current = world.resource::<ActiveDocument>().doc.clone();
    let stored = world.resource::<ActiveDocument>().meta.is_some();
    let store = world.get_resource::<DocumentStore>().map(|s| s.0.clone());
    let mut l = world.resource_mut::<DocLog>();
    let log = l.log.as_mut()?;
    if log.versions().iter().any(|v| v.id() == id) {
        return None;
    }
    if log.head() != doc {
        log.catch_up(doc, time, user);
    }
    log.create_version_with_id(id, name, description, time, user);
    let made = log.versions().last().map(|v| v.name().to_string());
    if log.head() != &current {
        log.catch_up(&current, time, user);
    }
    if let Some(s) = store.filter(|_| stored) {
        let _ = log.save(&s);
    }
    l.generation += 1;
    made
}

/// The owner's version of the document as everyone has it, told to the room.
fn owner_version(
    world: &mut World,
    name: &str,
    description: &str,
    user: &str,
) -> Result<String, String> {
    super::live::number_own(world);
    let (document, room, epoch, seq, doc) = {
        let live = world
            .get_resource::<Live>()
            .ok_or_else(|| tr("shell-locks-not-shared"))?;
        let Role::Owner(o) = &live.role else {
            return Err(tr("shell-versions-not-owner"));
        };
        (
            live.document,
            live.room(),
            o.epoch,
            o.seq(),
            o.confirmed().clone(),
        )
    };
    let id = VersionId::new();
    let time = world.resource::<AppClock>().now();
    let name = make_version(world, &doc, id, name, description, time, user)
        .ok_or_else(|| tr("shell-versions-history-closed"))?;
    let msg = VersionCreated {
        document,
        epoch,
        seq,
        id: id.0,
        name: name.clone(),
        description: description.to_string(),
        time,
        user: user.to_string(),
    };
    let mut state = SystemState::<NetSender>::new(world);
    if let Ok(net) = state.get(world) {
        net.broadcast(room, &msg);
    }
    Ok(name)
}

/// **Create version** on a shared document open live: made by the owner (here, or asked).
/// False when the document isn't shared live, and the History panel makes it as usual.
pub fn intercept(world: &mut World, name: &str, description: &str) -> bool {
    let Some(role) = world.get_resource::<Live>().map(|l| match &l.role {
        Role::Owner(_) => None,
        Role::Guest(g) => Some((
            g.link.clone(),
            g.owner_name.clone(),
            g.access.permission,
            match &g.state {
                GuestState::Following { epoch, .. } => Some(*epoch),
                _ => None,
            },
        )),
    }) else {
        return false;
    };
    match role {
        None => {
            let user = world.resource::<UserProfile>().display_name.clone();
            if let Err(why) = owner_version(world, name, description, &user) {
                crate::linked::toast(world, why);
            }
        }
        Some((_, owner, Permission::View, _)) => {
            crate::linked::toast(
                world,
                tr_with(
                    "shell-versions-no-edit-access",
                    &[("owner", owner.as_str())],
                ),
            );
        }
        Some((link, owner, _, Some(epoch))) => {
            let task = transfer::start_create_version(
                world.resource::<Iroh>(),
                &link,
                epoch,
                name.to_string(),
                description.to_string(),
            );
            world.resource_mut::<Waiting>().asks.push(task);
            crate::linked::toast(
                world,
                tr_with("shell-versions-asked-owner", &[("owner", owner.as_str())]),
            );
        }
        Some((_, owner, _, None)) => {
            crate::linked::toast(
                world,
                tr_with("shell-versions-owner-away", &[("owner", owner.as_str())]),
            );
        }
    }
    true
}

/// The owner's side: someone who may edit asks for a version.
pub(super) fn answer(world: &mut World, r: Request) {
    let Ask::CreateVersion {
        epoch,
        name,
        description,
    } = &r.ask
    else {
        return;
    };
    let from = r.from.to_string();
    let ok = world.get_resource::<Live>().and_then(|l| match &l.role {
        Role::Owner(o) if l.document == r.document => Some(o.epoch == *epoch),
        _ => None,
    });
    let may = super::sharing::read(world, r.document)
        .and_then(|(_, m)| m.sharing)
        .filter(|s| s.topic == r.topic)
        .and_then(|s| s.access(&from))
        .is_some_and(|a| a.permission == Permission::Edit);
    let verdict = match ok {
        None => Verdict::Closed,
        Some(false) => Verdict::Resync,
        Some(true) if !may => Verdict::Refused("You have view access to this document".into()),
        Some(true) => {
            let user = super::live::peer_name(world, r.from);
            match owner_version(world, name, description, &user) {
                Ok(_) => Verdict::Accepted(0),
                Err(why) => Verdict::Refused(why),
            }
        }
    };
    r.verdict(verdict);
}

/// The owner's versions as they arrive (from the owner, for the document open).
fn collect(
    mut rx: MessageReader<Received<VersionCreated>>,
    live: Option<Res<Live>>,
    mut waiting: ResMut<Waiting>,
) {
    let owner = live.as_ref().and_then(|l| match &l.role {
        Role::Guest(g) => Some((l.document, g.link.owner.id)),
        Role::Owner(_) => None,
    });
    let Some((document, owner)) = owner else {
        rx.clear();
        waiting.versions.clear();
        return;
    };
    for m in rx.read() {
        if m.from == owner && m.msg.document == document {
            waiting.versions.push(m.msg.clone());
        }
    }
}

/// The owner's versions, made here of the copy at the edit they were made at.
fn hear(world: &mut World) {
    let (seq, epoch, confirmed) = match world.get_resource::<Live>().map(|l| &l.role) {
        Some(Role::Guest(g)) => match &g.state {
            GuestState::Following { epoch, replica, .. } => {
                (replica.seq(), *epoch, replica.confirmed().clone())
            }
            _ => return,
        },
        _ => return,
    };
    let ready: Vec<VersionCreated> = {
        let mut w = world.resource_mut::<Waiting>();
        w.versions.retain(|v| v.epoch == epoch);
        let (ready, later) = w.versions.drain(..).partition(|v| v.seq <= seq);
        w.versions = later;
        ready
    };
    for v in ready {
        make_version(
            world,
            &confirmed,
            VersionId(v.id),
            &v.name,
            &v.description,
            v.time,
            &v.user,
        );
    }
}

fn poll(world: &mut World) {
    let mut done = Vec::new();
    world
        .resource_mut::<Waiting>()
        .asks
        .retain_mut(|t| match t.try_take() {
            Some(r) => {
                done.push(r);
                false
            }
            None => true,
        });
    for r in done {
        match r {
            Ok(Verdict::Refused(why)) | Err(why) => crate::linked::toast(world, why),
            Ok(Verdict::Closed) => crate::linked::toast(world, tr("shell-versions-owner-closed")),
            _ => {}
        }
    }
}
