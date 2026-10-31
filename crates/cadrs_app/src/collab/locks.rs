//! "Open tab, close it for other users": Onshape's focus lock for tabs that can't be edited
//! together (a Drawing, a PCB Studio: an edit replaces the whole tab).
//!
//! - Right-click such a tab ▸ **Open tab, close it for other users**: whoever may edit holds
//!   it; the others can look but not change it (their commands on it are refused, and so are
//!   their edits to it at the owner's), and see a lock on the tab. **Take over** from the
//!   holder takes it; **Let others edit this tab** gives it back.
//! - The owner's cadrs keeps the locks ([`LocksChanged`] tells the room), and lets one go when
//!   its holder leaves the tab or the document.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use cadrs_core::collab::Permission;
use cadrs_core::collab::sync::Key;
use cadrs_core::{DocumentId, ElementId, ElementKind};
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::bevy_iroh::prelude::{NetSender, Received};
use cadrs_net::{Answer, Ask, LocksChanged, Request, Verdict, transfer};
use cadrs_ui::i18n::{tr, tr_with};

use super::live::{GuestState, Live, Role};
use super::presence::People;
use crate::{ActiveDocument, UserProfile};

/// How long a holder may be away from the tab before the owner lets it go.
const AWAY: f64 = 3.0;

pub(super) fn plugin(app: &mut App) {
    app.add_message::<Received<LocksChanged>>()
        .init_resource::<TabLocks>()
        .init_resource::<LockAsks>()
        .add_systems(
            Update,
            (collect, release_left, poll, apply)
                .chain()
                .run_if(resource_exists::<Iroh>),
        )
        .add_systems(Update, (notice, sync_lock_icons));
}

/// The open shared document's locked tabs: each with its holder's id and name, and (on the
/// owner's side) since when the holder has been away from it.
#[derive(Resource, Default)]
pub struct TabLocks {
    document: Option<DocumentId>,
    pub locks: Vec<(ElementId, String, String)>,
    away: Vec<(ElementId, f64)>,
}

/// Requests on their way to the owner.
#[derive(Resource, Default)]
struct LockAsks(Vec<Answer<Verdict>>);

/// Whether a tab can be locked: one whose edits replace it whole.
pub fn lockable(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Drawing(_) | ElementKind::PcbStudio { .. }
    )
}

fn me(world: &World) -> Option<(String, String)> {
    let id = world.get_resource::<Iroh>()?.id().to_string();
    Some((id, world.resource::<UserProfile>().display_name.clone()))
}

/// Who holds `element`, if someone does.
pub fn holder(world: &World, element: ElementId) -> Option<(String, String)> {
    world
        .get_resource::<TabLocks>()?
        .locks
        .iter()
        .find(|(e, ..)| *e == element)
        .map(|(_, id, name)| (id.clone(), name.clone()))
}

/// The lock on `key`'s tab held by someone other than `by`, if any: its holder's name.
pub fn held_against(world: &World, key: Key, by: &str) -> Option<String> {
    let element = match key {
        Key::Head => return None,
        Key::Element(e) | Key::Shell(e) | Key::Feature(e, _) => e,
    };
    holder(world, element)
        .filter(|(id, _)| id != by)
        .map(|(_, name)| name)
}

/// The owner's side: grants or gives back a lock and tells the room.
fn set(
    world: &mut World,
    element: ElementId,
    holder: Option<(String, String)>,
    take: bool,
) -> Result<(), String> {
    let (document, room) = world
        .get_resource::<Live>()
        .filter(|l| l.is_owner())
        .map(|l| (l.document, l.room()))
        .ok_or_else(|| tr("shell-locks-not-shared"))?;
    {
        let mut locks = world.resource_mut::<TabLocks>();
        if locks.document != Some(document) {
            *locks = TabLocks {
                document: Some(document),
                ..default()
            };
        }
        let current = locks.locks.iter().position(|(e, ..)| *e == element);
        match (holder, current) {
            (Some((id, name)), Some(i)) => {
                let (_, held_by, held_name) = &locks.locks[i];
                if *held_by != id && !take {
                    return Err(tr_with(
                        "shell-locks-tab-open",
                        &[("name", held_name.as_str())],
                    ));
                }
                locks.locks[i] = (element, id, name);
            }
            (Some((id, name)), None) => locks.locks.push((element, id, name)),
            (None, Some(i)) => {
                locks.locks.remove(i);
            }
            (None, None) => {}
        }
        locks.away.retain(|(e, _)| *e != element);
    }
    let msg = LocksChanged {
        document,
        locks: world.resource::<TabLocks>().locks.clone(),
    };
    let mut state = SystemState::<NetSender>::new(world);
    if let Ok(net) = state.get(world) {
        net.broadcast(room, &msg);
    }
    Ok(())
}

/// The tab menu's items: open `element` for oneself (`take`: from whoever has it), or give it
/// back.
pub fn lock(world: &mut World, element: ElementId, lock: bool, take: bool) {
    let Some((my_id, my_name)) = me(world) else {
        return;
    };
    let role = world.get_resource::<Live>().map(|l| match &l.role {
        Role::Owner(_) => None,
        Role::Guest(g) => Some((
            g.link.clone(),
            g.access.permission,
            matches!(g.state, GuestState::Following { .. }),
        )),
    });
    let result = match role {
        None => Err(tr("shell-locks-document-not-shared")),
        Some(None) => set(world, element, lock.then_some((my_id, my_name)), take),
        Some(Some((_, Permission::View, _))) => Err(tr("shell-collab-view-access")),
        Some(Some((_, _, false))) => Err(tr("shell-locks-owner-away")),
        Some(Some((link, _, true))) => {
            let task = transfer::start_lock(world.resource::<Iroh>(), &link, element, lock, take);
            world.resource_mut::<LockAsks>().0.push(task);
            Ok(())
        }
    };
    if let Err(why) = result {
        crate::linked::toast(world, why);
    }
}

/// The owner's side: a lock asked for by someone it is shared with.
pub(super) fn answer(world: &mut World, r: Request) {
    let from = r.from.to_string();
    let edit = super::sharing::read(world, r.document)
        .and_then(|(_, m)| m.sharing)
        .filter(|s| s.topic == r.topic)
        .and_then(|s| s.access(&from))
        .is_some_and(|a| a.permission == Permission::Edit);
    let open = world
        .get_resource::<Live>()
        .is_some_and(|l| l.is_owner() && l.document == r.document);
    let verdict = if !open {
        Verdict::Closed
    } else if !edit {
        Verdict::Refused("You have view access to this document".into())
    } else {
        let name = super::live::peer_name(world, r.from);
        let result = match r.ask {
            Ask::Lock { element, take } => set(world, element, Some((from, name)), take),
            Ask::Unlock { element } => match holder(world, element) {
                Some((id, _)) if id == from => set(world, element, None, false),
                _ => Ok(()),
            },
            _ => Ok(()),
        };
        match result {
            Ok(()) => Verdict::Accepted(0),
            Err(why) => Verdict::Refused(why),
        }
    };
    r.verdict(verdict);
}

/// The owner's locks, as they arrive (from the owner, for the document open).
fn collect(
    mut rx: MessageReader<Received<LocksChanged>>,
    live: Option<Res<Live>>,
    mut locks: ResMut<TabLocks>,
) {
    let Some(live) = live else {
        rx.clear();
        if locks.document.is_some() {
            *locks = TabLocks::default();
        }
        return;
    };
    if locks.document != Some(live.document) {
        *locks = TabLocks {
            document: Some(live.document),
            ..default()
        };
    }
    let owner = match &live.role {
        Role::Guest(g) => g.link.owner.id,
        Role::Owner(_) => {
            rx.clear();
            return;
        }
    };
    for m in rx.read() {
        if m.from == owner && m.msg.document == live.document {
            locks.locks = m.msg.locks.clone();
        }
    }
}

/// The owner lets a lock go when its holder has left the tab (or the document) for a while.
fn release_left(world: &mut World) {
    let Some(document) = world
        .get_resource::<Live>()
        .filter(|l| l.is_owner())
        .map(|l| l.document)
    else {
        return;
    };
    if world.resource::<TabLocks>().document != Some(document) {
        return;
    }
    let t = world.resource::<Time<Real>>().elapsed_secs_f64();
    let me = me(world).map(|m| m.0).unwrap_or_default();
    let active = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active);
    let locks = world.resource::<TabLocks>().locks.clone();
    let mut gone = Vec::new();
    for (element, id, _) in locks {
        let there = if id == me {
            active == Some(element)
        } else {
            world
                .resource::<People>()
                .by_id
                .get(&id)
                .is_some_and(|p| p.presence.tab == Some(element))
        };
        let mut l = world.resource_mut::<TabLocks>();
        if there {
            l.away.retain(|(e, _)| *e != element);
            continue;
        }
        let since = match l.away.iter().find(|(e, _)| *e == element) {
            Some((_, s)) => *s,
            None => {
                l.away.push((element, t));
                t
            }
        };
        if t - since > AWAY {
            gone.push(element);
        }
    }
    for element in gone {
        let _ = set(world, element, None, false);
    }
}

fn poll(world: &mut World) {
    let mut done = Vec::new();
    world
        .resource_mut::<LockAsks>()
        .0
        .retain_mut(|t| match t.try_take() {
            Some(r) => {
                done.push(r);
                false
            }
            None => true,
        });
    for r in done {
        if let Ok(Verdict::Refused(why)) | Err(why) = r {
            crate::linked::toast(world, why);
        }
    }
}

/// The open document's command layer refuses commands on tabs someone else holds.
fn apply(world: &mut World) {
    let me = me(world).map(|m| m.0).unwrap_or_default();
    let others: Vec<(ElementId, String)> = world
        .resource::<TabLocks>()
        .locks
        .iter()
        .filter(|(_, id, _)| *id != me)
        .map(|(e, _, name)| (*e, name.clone()))
        .collect();
    if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
        && d.locked_tabs != others
    {
        d.locked_tabs = others;
    }
}

/// A toast when a command was refused because someone else holds the tab, or when this user
/// opens a tab someone else holds.
fn notice(world: &mut World, mut last_tab: Local<Option<ElementId>>) {
    let Some(mut d) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let refused = d.bypass_change_detection().lock_refused.take();
    let active = d.active;
    let held = active.and_then(|a| {
        d.locked_tabs
            .iter()
            .find(|(e, _)| *e == a)
            .map(|(_, n)| n.clone())
    });
    if let Some(name) = refused {
        crate::linked::toast(
            world,
            tr_with("shell-locks-take-over", &[("name", name.as_str())]),
        );
    } else if active != *last_tab
        && let Some(name) = held
    {
        crate::linked::toast(
            world,
            tr_with("shell-locks-open-for-editing", &[("name", name.as_str())]),
        );
    }
    *last_tab = active;
}

/// The lock on a tab someone else holds.
#[derive(Component)]
struct LockIcon(ElementId);

fn sync_lock_icons(
    doc: Option<Res<ActiveDocument>>,
    tabs: Query<(Entity, &crate::document::TabButton)>,
    icons: Query<(Entity, &LockIcon, &ChildOf)>,
    theme: Res<cadrs_ui::Theme>,
    mut commands: Commands,
) {
    let held: Vec<ElementId> = doc
        .map(|d| d.locked_tabs.iter().map(|(e, _)| *e).collect())
        .unwrap_or_default();
    let mut have = Vec::new();
    for (e, icon, parent) in &icons {
        let on_its_tab = tabs.get(parent.parent()).is_ok_and(|(_, t)| t.0 == icon.0);
        if held.contains(&icon.0) && on_its_tab {
            have.push(icon.0);
        } else {
            commands.entity(e).despawn();
        }
    }
    for (tab, button) in &tabs {
        if held.contains(&button.0) && !have.contains(&button.0) {
            commands.entity(tab).with_child((
                LockIcon(button.0),
                cadrs_ui::icon::icon_in(
                    "lock-filled",
                    10.0,
                    theme.muted_foreground,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(3.0),
                        top: Val::Px(3.0),
                        ..default()
                    },
                ),
                Pickable::IGNORE,
            ));
        }
    }
}
