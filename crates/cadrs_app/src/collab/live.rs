//! Editing a shared document together ([`cadrs_core::collab::sync`]), peer to peer.
//!
//! Whoever has a shared document open is in its room ([`Live`]):
//!
//! - **The owner** numbers every edit ([`Sequencer`]): their own as they make them, and the
//!   ones the others propose, first come first served. An edit that changes something someone
//!   else changed first (the same feature, a tab they removed, …) is refused and taken back on
//!   its author's side, with a toast saying who got there first. Each numbered edit reaches
//!   everyone over the room ([`Sequenced`]).
//! - **Everyone else** edits their own copy at once and proposes the edits to the owner, one
//!   at a time ([`Replica`]); the numbered edits of others come in over the room. Their own
//!   undo and redo stay theirs ([`ActiveDocument::apply_remote`]). A copy that falls behind
//!   (a numbered edit missing for a while, a session that started again) or no longer matches
//!   the owner's is fetched again.
//! - Someone with view access follows along read-only.
//! - **While the owner is away** (their cadrs closed or the document not open), the editor
//!   here with the lowest cadrs ID keeps the edits in order instead ([`Acting`]), and the
//!   others follow them, so editing goes on. When the owner is back, that editor hands the
//!   document over ([`Ask::Handover`]: the owner takes it on as one edit) and everyone follows
//!   the owner again. With no editor here, the copy stays read-only until the owner is back.

use bevy::prelude::*;
use cadrs_core::collab::sync::{Incoming, Key, Refusal, Replica, Sequencer};
use cadrs_core::collab::{Access, Permission, Snapshot};
use cadrs_core::history_log::document_hash;
use cadrs_core::{Document, DocumentId};
use cadrs_net::bevy_iroh::iroh::{EndpointAddr, EndpointId};
use cadrs_net::bevy_iroh::prelude::{NetSender, Peer, PeerJoined, PeerLeft, Received};
use cadrs_net::bevy_iroh::{Iroh, IrohTask};
use cadrs_net::{AccessChanged, Ask, Request, Requests, Sequenced, ShareLink, Verdict, Welcome};
use cadrs_net::{session, transfer};
use cadrs_ui::i18n::{tr, tr_with};

use crate::{ActiveDocument, AppState, DocumentStore, UserProfile};

/// The owner numbers its own edits at most this often: a drag that changes the document every
/// frame goes out as a few edits a second, not sixty.
const OWN_EVERY: f64 = 0.1;
/// How long the owner's old room stays after the document moved to a new one.
const LINGER: f64 = 3.0;
/// How long numbered edits may wait for a missing one before the copy is fetched again.
const GAP_PATIENCE: f64 = 5.0;
/// How long the owner may be gone before someone else keeps the edits in order.
const ELECT_AFTER: f64 = 5.0;
/// How often a peer whose owner is away joins the room again (gossip meets only peers it
/// dials, so one that joined while the owner was away would never see them come back).
const RETRY_EVERY: f64 = 15.0;

pub(super) fn plugin(app: &mut App) {
    // Registered here as well: without the network (scenarios, tests, CADRS_NET=0) there is
    // no node, and these systems don't run, but their parameters still need the messages.
    app.add_message::<Received<Sequenced>>()
        .add_message::<Received<AccessChanged>>()
        .add_message::<PeerJoined>()
        .add_message::<PeerLeft>()
        .init_resource::<Outbox>()
        .init_resource::<Leaving>()
        .init_resource::<Inbox>()
        .add_systems(
            Update,
            (
                start_stop,
                collect,
                own_edits,
                serve,
                guest,
                send,
                leave_rooms,
            )
                .chain()
                .run_if(resource_exists::<Iroh>),
        );
}

/// The shared document open now, and this peer's part in it.
#[derive(Resource)]
pub struct Live {
    pub document: DocumentId,
    room: Entity,
    pub role: Role,
}

pub enum Role {
    Owner(Owner),
    Guest(Guest),
}

pub struct Owner {
    /// This session: edits are numbered from 0 each time the owner opens the document.
    pub epoch: u64,
    topic: [u8; 32],
    seq: Sequencer,
    /// The document after the last numbered edit.
    confirmed: Box<Document>,
    /// Set when this peer keeps the edits in order for an owner who is away.
    pub acting: Option<Acting>,
}

/// Keeping the edits in order for an owner who is away.
pub struct Acting {
    pub link: ShareLink,
    pub owner_name: String,
    /// Who may do what, as the owner last said.
    pub acl: Option<AccessChanged>,
    /// When the owner was last asked whether they are back, and the answer to come.
    ping: (f64, Option<IrohTask<Result<Verdict, String>>>),
}

impl Owner {
    /// The number of the last edit.
    pub fn seq(&self) -> u64 {
        self.seq.seq()
    }

    /// The document after it.
    pub fn confirmed(&self) -> &Document {
        &self.confirmed
    }
}

pub struct Guest {
    pub link: ShareLink,
    pub owner_name: String,
    pub access: Access,
    pub state: GuestState,
    /// Who numbers the edits now: the owner, or whoever keeps them in order while the owner
    /// is away.
    pub host: EndpointId,
    pub host_name: String,
    /// Who may do what, as the owner last said (to choose who keeps the edits in order).
    pub acl: Option<AccessChanged>,
}

impl Guest {
    /// The link, pointed at whoever numbers the edits now.
    pub fn host_link(&self) -> ShareLink {
        let mut link = self.link.clone();
        if self.host != link.owner.id {
            link.owner = EndpointAddr::new(self.host);
        }
        link
    }
}

pub enum GuestState {
    /// Fetching the document; numbered edits that arrive meanwhile wait.
    Connecting {
        task: IrohTask<Result<Welcome, String>>,
        early: Vec<Sequenced>,
    },
    /// Following the owner's numbering.
    Following {
        epoch: u64,
        replica: Box<Replica>,
        /// The edit on its way: its id and the answer to come.
        sending: Option<(u64, IrohTask<Result<Verdict, String>>)>,
        /// Since when numbered edits have waited for a missing one.
        gap_since: Option<f64>,
    },
    /// The owner is away (or has the document closed): the last copy, read-only. Every so
    /// often the owner's cadrs is asked whether it is back (`ping`).
    Away {
        since: f64,
        ping: Option<(f64, IrohTask<Result<Verdict, String>>)>,
    },
    /// The owner stopped sharing it with this user: the last copy, read-only, for good.
    Removed,
    /// Handing the document back to the owner (this peer kept the edits in order while they
    /// were away).
    Returning {
        task: IrohTask<Result<Verdict, String>>,
    },
}

/// True while the open document is someone else's, shared live.
pub fn is_guest(world: &World) -> bool {
    world.get_resource::<Live>().is_some_and(|l| !l.is_owner())
}

/// True while the open document is shared live (with others or waiting for them).
pub fn together(world: &World) -> bool {
    world.contains_resource::<Live>()
}

impl Live {
    pub fn is_owner(&self) -> bool {
        matches!(self.role, Role::Owner(_))
    }

    /// The room's entity.
    pub fn room(&self) -> Entity {
        self.room
    }

    /// A word for the top bar on how this copy stands, when it isn't simply live (the
    /// read-only reason says the rest).
    pub fn status(&self) -> Option<String> {
        let g = match &self.role {
            Role::Owner(Owner {
                acting: Some(a), ..
            }) => {
                return Some(tr_with(
                    "shell-live-status-you-keep-order",
                    &[("owner", a.owner_name.as_str())],
                ));
            }
            Role::Owner(_) => return None,
            Role::Guest(g) => g,
        };
        if g.host != g.link.owner.id && matches!(g.state, GuestState::Following { .. }) {
            return Some(tr_with(
                "shell-live-status-host-keeps-order",
                &[
                    ("owner", g.owner_name.as_str()),
                    ("host", g.host_name.as_str()),
                ],
            ));
        }
        Some(tr(match &g.state {
            GuestState::Connecting { .. } => "shell-live-status-connecting",
            GuestState::Away { .. } => "shell-live-status-owner-away",
            GuestState::Removed => "shell-live-status-removed",
            GuestState::Returning { .. } => "shell-live-status-returning",
            GuestState::Following { .. } if g.access.permission == Permission::View => {
                "shell-live-status-view-only"
            }
            GuestState::Following { .. } => return None,
        }))
    }

    /// This user's access: everything for the owner.
    pub fn access(&self) -> Access {
        match &self.role {
            // Only an editor keeps the edits in order for an owner who is away.
            Role::Owner(Owner {
                acting: Some(_), ..
            }) => Access {
                permission: Permission::Edit,
                rights: cadrs_core::collab::Rights::of(Permission::Edit),
            },
            Role::Owner(_) => Access::owner(),
            Role::Guest(g) => g.access,
        }
    }
}

/// Rooms on their way out: each with when to leave it.
#[derive(Resource, Default)]
struct Leaving(Vec<(Entity, f64)>);

/// Ends the session at once, the node about to go offline ([`super::go_offline`]): its room
/// and the ones on their way out are left. Someone else's document turns read-only, as when it
/// is opened offline; online again, [`start_stop`] starts a new session.
pub(super) fn end(world: &mut World) {
    let mut rooms: Vec<Entity> = std::mem::take(&mut world.resource_mut::<Leaving>().0)
        .into_iter()
        .map(|(room, _)| room)
        .collect();
    rooms.extend(world.remove_resource::<Live>().map(|l| l.room));
    for room in rooms {
        if let Ok(e) = world.get_entity_mut(room) {
            e.despawn();
        }
    }
    if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
        && let Some(shared) = d.meta.as_ref().and_then(|m| m.shared.clone())
    {
        d.read_only = Some(super::read_only_reason(&shared));
    }
}

fn leave_rooms(world: &mut World) {
    let t = now(world);
    let gone: Vec<Entity> = {
        let mut l = world.resource_mut::<Leaving>();
        let gone =
            l.0.iter()
                .filter(|(_, at)| *at <= t)
                .map(|(e, _)| *e)
                .collect();
        l.0.retain(|(_, at)| *at > t);
        gone
    };
    for e in gone {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
}

/// Numbered edits to send to the room.
#[derive(Resource, Default)]
struct Outbox(Vec<Sequenced>);

/// What came from the rooms: numbered edits (from whoever sent them), and who arrived and
/// left which room.
#[derive(Resource, Default)]
struct Inbox {
    edits: Vec<(EndpointId, Sequenced)>,
    access: Vec<(EndpointId, AccessChanged)>,
    joined: Vec<(Entity, EndpointId)>,
    left: Vec<(Entity, EndpointId)>,
}

/// A fresh session number.
fn epoch() -> u64 {
    uuid::Uuid::new_v4().as_u64_pair().0 | 1
}

fn me(world: &World) -> Option<(String, String)> {
    let iroh = world.get_resource::<Iroh>()?;
    let name = world.resource::<UserProfile>().display_name.clone();
    Some((iroh.id().to_string(), name))
}

/// A peer's name as their cadrs says it, or a short form of their key.
pub(super) fn peer_name(world: &mut World, id: EndpointId) -> String {
    let mut q = world.query::<&Peer>();
    q.iter(world)
        .find(|p| p.id == id)
        .map(Peer::label)
        .unwrap_or_else(|| id.fmt_short().to_string())
}

/// What a key names, for a toast: "Extrude 2", "the tab Assembly 1", …
fn describe(doc: &Document, key: Key) -> String {
    match key {
        Key::Head => "the document's settings".into(),
        Key::Element(e) | Key::Shell(e) => doc
            .element(e)
            .map_or("a tab".into(), |e| format!("the tab {}", e.name)),
        Key::Feature(e, f) => doc
            .element(e)
            .and_then(|e| e.features().iter().find(|x| x.id == f))
            .map_or("a feature".into(), |f| f.name.clone()),
    }
}

/// Why a document shared with this user is read-only now, if it is.
fn guest_read_only(g: &Guest) -> Option<String> {
    let key = match &g.state {
        GuestState::Connecting { .. } => "shell-live-connecting-to",
        GuestState::Away { .. } => "shell-live-owner-away",
        GuestState::Removed => "shell-live-removed",
        GuestState::Returning { .. } => "shell-live-handing-back",
        GuestState::Following { .. } if g.access.permission == Permission::View => {
            "shell-live-view-only"
        }
        GuestState::Following { .. } => return None,
    };
    Some(tr_with(key, &[("owner", g.owner_name.as_str())]))
}

// ---------------------------------------------------------------------------------------------
// Starting and stopping

/// What the open document asks for.
enum Want {
    Owner {
        topic: [u8; 32],
    },
    Guest {
        link: ShareLink,
        owner_name: String,
        access: Access,
    },
}

fn wanted(world: &World) -> Option<(DocumentId, String, Want)> {
    world.get_resource::<Iroh>()?;
    if *world.resource::<State<AppState>>().get() != AppState::Document {
        return None;
    }
    let d = world.get_resource::<ActiveDocument>()?;
    let meta = d.meta.as_ref()?;
    // Main is the shared workspace (`super::versions`): a branch is one's own.
    let on_branch = world
        .get_resource::<crate::history_panel::DocLog>()
        .and_then(|l| l.log.as_ref())
        .is_some_and(|l| l.document == d.doc.id && !l.current_workspace().is_main());
    if on_branch {
        return None;
    }
    let want = match (&meta.shared, &meta.sharing) {
        (Some(s), _) => Want::Guest {
            link: s.link.parse().ok()?,
            owner_name: s.owner_name.clone(),
            access: s.access(),
        },
        (None, Some(s)) => Want::Owner { topic: s.topic },
        (None, None) => return None,
    };
    Some((d.doc.id, d.doc.name.clone(), want))
}

/// Joins the open document's room when it is shared, and leaves when another is opened.
fn start_stop(world: &mut World) {
    let want = wanted(world);
    // The room is left and joined again when the topic changes (link sharing turned off).
    // (Keeping the edits in order for an owner who is away is still being their guest.)
    let current = world.get_resource::<Live>().map(|l| {
        let (owner, topic, by) = match &l.role {
            Role::Owner(o) => match &o.acting {
                None => (true, o.topic, None),
                Some(a) => (false, a.link.topic, Some(a.link.owner.id)),
            },
            Role::Guest(g) => (false, g.link.topic, Some(g.link.owner.id)),
        };
        (l.document, owner, (topic, by), l.room)
    });
    let same = match (&want, current) {
        (Some((id, _, w)), Some((doc, owner, link, _))) => {
            let (wants_owner, want_link) = match w {
                Want::Owner { topic } => (true, (*topic, None)),
                Want::Guest { link, .. } => (false, (link.topic, Some(link.owner.id))),
            };
            *id == doc && owner == wants_owner && link == want_link
        }
        (None, None) => true,
        _ => false,
    };
    if same {
        return;
    }
    if let Some((_, owner, _, room)) = current {
        world.remove_resource::<Live>();
        // The owner's old room stays a moment, for the news of who may do what to get out
        // (the room moves when someone is removed); anyone else's goes at once.
        if owner {
            let until = now(world) + LINGER;
            world.resource_mut::<Leaving>().0.push((room, until));
        } else if let Ok(e) = world.get_entity_mut(room) {
            e.despawn();
        }
    }
    let Some((document, name, want)) = want else {
        // Someone else's document on a branch of one's own: it is edited here.
        if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
            && d.meta.as_ref().is_some_and(|m| m.shared.is_some())
            && d.read_only.is_some()
        {
            d.read_only = None;
        }
        return;
    };
    let live = match want {
        Want::Owner { topic } => {
            let room = session::host_room(&mut world.commands(), topic, &name);
            let confirmed = world.resource::<ActiveDocument>().doc.clone();
            info!("sharing {name} in its room");
            Live {
                document,
                room,
                role: Role::Owner(Owner {
                    epoch: epoch(),
                    topic,
                    seq: Sequencer::new(0),
                    confirmed: Box::new(confirmed),
                    acting: None,
                }),
            }
        }
        Want::Guest {
            link,
            owner_name,
            access,
        } => {
            let room = session::join_room(&mut world.commands(), &link);
            let task = transfer::start_fetch(world.resource::<Iroh>(), link.clone());
            Live {
                document,
                room,
                role: Role::Guest(Guest {
                    host: link.owner.id,
                    host_name: owner_name.clone(),
                    acl: None,
                    link,
                    owner_name,
                    access,
                    state: GuestState::Connecting {
                        task,
                        early: Vec::new(),
                    },
                }),
            }
        }
    };
    world.flush();
    if let Role::Guest(g) = &live.role {
        let reason = guest_read_only(g);
        world.resource_mut::<ActiveDocument>().read_only = reason;
    }
    world.insert_resource(live);
}

// ---------------------------------------------------------------------------------------------
// The room

fn collect(
    mut rx: MessageReader<Received<Sequenced>>,
    mut access: MessageReader<Received<AccessChanged>>,
    mut joined: MessageReader<PeerJoined>,
    mut left: MessageReader<PeerLeft>,
    mut inbox: ResMut<Inbox>,
) {
    for m in rx.read() {
        inbox.edits.push((m.from, m.msg.clone()));
    }
    for m in access.read() {
        inbox.access.push((m.from, m.msg.clone()));
    }
    inbox.joined.extend(joined.read().map(|j| (j.room, j.id)));
    inbox.left.extend(left.read().map(|l| (l.room, l.id)));
}

fn send(mut outbox: ResMut<Outbox>, live: Option<Res<Live>>, net: NetSender) {
    let Some(live) = live else {
        outbox.0.clear();
        return;
    };
    for m in outbox.0.drain(..) {
        net.broadcast(live.room, &m);
    }
}

// ---------------------------------------------------------------------------------------------
// The owner

/// The owner's own edits, numbered as they are made (at most every [`OWN_EVERY`]).
fn own_edits(
    world: &mut World,
    mut seen: Local<Option<bevy::ecs::change_detection::Tick>>,
    mut last: Local<f64>,
) {
    if !world
        .get_resource::<Live>()
        .is_some_and(|l| l.is_owner() && world.contains_resource::<ActiveDocument>())
    {
        return;
    }
    let tick = world
        .get_resource_ref::<ActiveDocument>()
        .map(|d| d.last_changed());
    let t = now(world);
    if tick == *seen || t - *last < OWN_EVERY {
        return;
    }
    *seen = tick;
    *last = t;
    number_own(world);
}

/// Numbers whatever the owner's document has that the last numbered edit doesn't. Also before
/// a peer's edit is taken, so it lands on top of the owner's.
pub(super) fn number_own(world: &mut World) {
    if !world.contains_resource::<Live>() || !world.contains_resource::<ActiveDocument>() {
        return;
    }
    let Some((me, my_name)) = me(world) else {
        return;
    };
    world.resource_scope(|world, mut live: Mut<Live>| {
        let document = live.document;
        let Role::Owner(o) = &mut live.role else {
            return;
        };
        let doc = &world.resource::<ActiveDocument>().doc;
        if doc.id != document || *doc == *o.confirmed {
            return;
        }
        let change = cadrs_core::collab::sync::Change::between(&o.confirmed, doc);
        let base = o.seq.seq();
        let Ok((seq, change)) = o.seq.admit(&me, base, change, &mut o.confirmed) else {
            return;
        };
        *o.confirmed = doc.clone();
        let m = Sequenced::new(
            document,
            o.epoch,
            seq,
            (&me, &my_name),
            0,
            &change,
            document_hash(&o.confirmed),
        );
        world.resource_mut::<Outbox>().0.push(m);
    });
}

/// Answers what peers ask: the document (open or stored), and edits to number.
fn serve(world: &mut World) {
    let Some(requests) = world
        .get_resource_mut::<Requests>()
        .map(|mut r| r.drain())
        .filter(|r| !r.is_empty())
    else {
        return;
    };
    for r in requests {
        match r.ask {
            Ask::Join => join(world, r),
            Ask::Propose { .. } => propose(world, r),
            Ask::Comment(_) => super::comments::answer(world, r),
            Ask::CreateVersion { .. } => super::versions::answer(world, r),
            Ask::Lock { .. } | Ask::Unlock { .. } => super::locks::answer(world, r),
            Ask::Handover(_) => handover(world, r),
            Ask::Status => {
                let open = sharing_for(world, &r).is_ok_and(|(_, _, open)| open)
                    && world.get_resource::<Live>().is_some_and(|l| {
                        l.document == r.document
                            && matches!(&l.role, Role::Owner(o) if o.acting.is_none())
                    });
                r.verdict(Verdict::Accepted(open as u64))
            }
            _ => super::sharing::answer(world, r),
        }
    }
}

/// The owner's sharing of `document`, checked against the topic a request gave; `Err` is the
/// refusal.
fn sharing_for(
    world: &World,
    r: &Request,
) -> Result<(Document, cadrs_core::collab::Sharing, bool), String> {
    let store = world.resource::<DocumentStore>().0.clone();
    let open = world
        .get_resource::<ActiveDocument>()
        .filter(|d| d.doc.id == r.document)
        .and_then(|d| Some((d.doc.clone(), d.meta.clone()?, true)));
    let (doc, meta, open) = match open {
        Some(o) => o,
        None => {
            let file = store
                .load(r.document)
                .map_err(|_| "The owner no longer has this document".to_string())?;
            (file.document, file.meta, false)
        }
    };
    match meta.sharing.filter(|s| s.topic == r.topic) {
        Some(s) if meta.trashed.is_none() => Ok((doc, s, open)),
        _ => Err("This document is no longer shared".into()),
    }
}

/// The access `peer` has to the document `r` names when this peer keeps its edits in order
/// for an owner who is away: as the owner last said (an editor's, before they said anything).
fn acting_access(world: &World, r: &Request) -> Option<Result<(Access, String), String>> {
    let live = world.get_resource::<Live>()?;
    let Role::Owner(Owner {
        acting: Some(a), ..
    }) = &live.role
    else {
        return None;
    };
    if live.document != r.document || a.link.topic != r.topic {
        return Some(Err("This document is no longer shared".into()));
    }
    let access = match &a.acl {
        Some(acl) => acl.access(&r.from.to_string()),
        None => Some(Access {
            permission: Permission::Edit,
            rights: cadrs_core::collab::Rights::of(Permission::Edit),
        }),
    };
    Some(
        access
            .map(|x| (x, a.owner_name.clone()))
            .ok_or("Link sharing is off for this document".into()),
    )
}

fn join(world: &mut World, r: Request) {
    // Keeping the edits in order for an owner who is away: the copy as numbered here.
    if let Some(acting) = acting_access(world, &r) {
        let store = world.resource::<DocumentStore>().0.clone();
        let answer = acting.and_then(|(access, owner_name)| {
            let Some(Role::Owner(o)) = world.get_resource::<Live>().map(|l| &l.role) else {
                return Err("Closed".to_string());
            };
            let snapshot = Snapshot::of(&o.confirmed, o.seq.seq(), &store)?;
            Ok(Welcome {
                comments: super::comments::for_document(world, r.document),
                access,
                owner_name,
                epoch: o.epoch,
                snapshot,
            })
        });
        return r.welcome(answer);
    }
    let owner_name = world.resource::<UserProfile>().display_name.clone();
    let store = world.resource::<DocumentStore>().0.clone();
    let answer = sharing_for(world, &r).and_then(|(doc, sharing, _)| {
        let access = sharing
            .access(&r.from.to_string())
            .ok_or("Link sharing is off for this document")?;
        // The numbered copy while the owner edits it; the stored one (session 0, which
        // numbers nothing) while it is closed.
        let (epoch, seq, doc) = match world.get_resource::<Live>().map(|l| (l.document, &l.role)) {
            Some((d, Role::Owner(o))) if d == r.document => {
                (o.epoch, o.seq.seq(), (*o.confirmed).clone())
            }
            _ => (0, 0, doc),
        };
        let snapshot = Snapshot::of(&doc, seq, &store)?;
        let comments = super::comments::for_document(world, r.document);
        Ok(Welcome {
            comments,
            access,
            owner_name,
            epoch,
            snapshot,
        })
    });
    match &answer {
        Ok(_) => info!("sent {} to {}", r.document, r.from.fmt_short()),
        Err(why) => info!("refused {} to {}: {why}", r.document, r.from.fmt_short()),
    }
    r.welcome(answer);
}

fn propose(world: &mut World, r: Request) {
    let Ask::Propose {
        epoch,
        id,
        base,
        change,
    } = &r.ask
    else {
        return;
    };
    let (epoch, id, base) = (*epoch, *id, *base);
    let permission = match acting_access(world, &r) {
        Some(Ok((a, _))) => Some(a.permission),
        Some(Err(why)) => return r.refuse(why),
        None => match sharing_for(world, &r) {
            Ok((_, s, true)) => s.permission(&r.from.to_string()),
            Ok(_) => return r.verdict(Verdict::Closed),
            Err(why) => return r.refuse(why),
        },
    };
    if permission != Some(Permission::Edit) {
        return r.verdict(Verdict::Refused(
            "You have view access to this document".into(),
        ));
    }
    number_own(world);
    let author = r.from.to_string();
    // A tab someone else has open for themselves (`super::locks`).
    if let Some(name) = change
        .footprint()
        .0
        .iter()
        .find_map(|k| super::locks::held_against(world, *k, &author))
    {
        return r.verdict(Verdict::Refused(format!(
            "{name} has that tab open: your change was undone"
        )));
    }
    let Some(mut live) = world.remove_resource::<Live>() else {
        return r.verdict(Verdict::Closed);
    };
    let author_name = peer_name(world, r.from);
    let verdict = match &mut live.role {
        Role::Owner(o) if live.document == r.document && o.epoch == epoch => {
            match o.seq.admit(&author, base, change.clone(), &mut o.confirmed) {
                Ok((seq, change)) => {
                    world
                        .resource_mut::<ActiveDocument>()
                        .apply_remote((*o.confirmed).clone(), &author_name);
                    let m = Sequenced::new(
                        live.document,
                        o.epoch,
                        seq,
                        (&author, &author_name),
                        id,
                        &change,
                        document_hash(&o.confirmed),
                    );
                    world.resource_mut::<Outbox>().0.push(m);
                    Verdict::Accepted(seq)
                }
                Err(Refusal::Conflict { author: first, key }) => {
                    let who = match first.parse::<EndpointId>() {
                        Ok(id) if Some(id) == world.get_resource::<Iroh>().map(Iroh::id) => {
                            world.resource::<UserProfile>().display_name.clone()
                        }
                        Ok(id) => peer_name(world, id),
                        Err(_) => first,
                    };
                    let what = describe(&o.confirmed, key);
                    Verdict::Refused(format!(
                        "{who} changed {what} first: your change was undone"
                    ))
                }
                Err(Refusal::Stale) => Verdict::Resync,
            }
        }
        Role::Owner(_) => Verdict::Resync,
        Role::Guest(_) => Verdict::Closed,
    };
    world.insert_resource(live);
    r.verdict(verdict);
}

// ---------------------------------------------------------------------------------------------
// Everyone else

/// A peer's side: catching up, applying numbered edits, proposing its own.
fn guest(world: &mut World, mut seen: Local<Option<bevy::ecs::change_detection::Tick>>) {
    let live = world
        .remove_resource::<Live>()
        .filter(|_| world.contains_resource::<ActiveDocument>());
    let Some(mut live) = live else {
        *world.resource_mut::<Inbox>() = Inbox::default();
        return;
    };
    let document = live.document;
    let room = live.room;
    match &mut live.role {
        Role::Guest(g) => {
            let before = guest_read_only(g);
            let tick = world.resource_ref::<ActiveDocument>().last_changed();
            let changed = *seen != Some(tick);
            *seen = Some(tick);
            let next = step(world, g, document, &mut live.room, changed);
            if next == Next::Act {
                live.role = act(world, g);
            } else if let Role::Guest(g) = &live.role {
                let after = guest_read_only(g);
                if after != before
                    || world.resource::<ActiveDocument>().read_only.as_ref() != after.as_ref()
                {
                    world.resource_mut::<ActiveDocument>().read_only = after;
                }
            }
        }
        Role::Owner(o) => {
            let inbox = std::mem::take(&mut *world.resource_mut::<Inbox>());
            // The owner is back (seen in the room, or saying so when asked): the document
            // goes back to them.
            let t = now(world);
            let mut back = false;
            if let Some(a) = &mut o.acting {
                back = inbox.joined.contains(&(room, a.link.owner.id));
                match &mut a.ping {
                    (_, Some(task)) => {
                        if let Some(r) = task.try_take() {
                            back |= matches!(r, Ok(Verdict::Accepted(1)));
                            a.ping = (t, None);
                        }
                    }
                    (at, None) if t - *at > RETRY_EVERY => {
                        a.ping = (
                            t,
                            Some(transfer::start_status(world.resource::<Iroh>(), &a.link)),
                        );
                    }
                    _ => {}
                }
            }
            if back {
                live.role = step_down(world, o);
                if let Role::Guest(g) = &live.role {
                    rejoin(world, g, &mut live.room);
                }
            }
        }
    }
    world.insert_resource(live);
}

/// What a guest does next besides following.
#[derive(PartialEq)]
enum Next {
    Stay,
    /// Keep the edits in order: the owner is away and this peer was chosen.
    Act,
}

/// Starts keeping the edits in order for the owner who is away: numbering from the document
/// as this peer has it.
fn act(world: &mut World, g: &Guest) -> Role {
    info!(
        "keeping {}'s edits in order while they are away",
        g.owner_name
    );
    world.resource_mut::<ActiveDocument>().read_only = None;
    crate::linked::toast(
        world,
        tr_with(
            "shell-live-you-keep-order",
            &[("owner", g.owner_name.as_str())],
        ),
    );
    Role::Owner(Owner {
        epoch: epoch(),
        topic: g.link.topic,
        seq: Sequencer::new(0),
        confirmed: Box::new(world.resource::<ActiveDocument>().doc.clone()),
        acting: Some(Acting {
            link: g.link.clone(),
            owner_name: g.owner_name.clone(),
            acl: g.acl.clone(),
            ping: (now(world), None),
        }),
    })
}

/// The owner is back: hands them the document as it is, and follows them again.
fn step_down(world: &mut World, o: &mut Owner) -> Role {
    let a = o.acting.take().expect("acting");
    info!("{} is back: handing the document over", a.owner_name);
    let store = world.resource::<DocumentStore>().0.clone();
    let task = match Snapshot::of(&o.confirmed, 0, &store) {
        Ok(s) => transfer::start_handover(world.resource::<Iroh>(), &a.link, &s),
        Err(why) => world.resource::<Iroh>().spawn(async move { Err(why) }),
    };
    world.resource_mut::<ActiveDocument>().read_only = Some(tr_with(
        "shell-live-handing-back",
        &[("owner", a.owner_name.as_str())],
    ));
    Role::Guest(Guest {
        host: a.link.owner.id,
        host_name: a.owner_name.clone(),
        acl: a.acl,
        access: Access {
            permission: Permission::Edit,
            rights: cadrs_core::collab::Rights::of(Permission::Edit),
        },
        owner_name: a.owner_name,
        link: a.link,
        state: GuestState::Returning { task },
    })
}

/// The owner's side, back from away: the document as it was kept meanwhile, taken on as one
/// edit (by whoever kept it).
fn handover(world: &mut World, r: Request) {
    let Ask::Handover(snapshot) = &r.ask else {
        return;
    };
    let may = matches!(sharing_for(world, &r), Ok((_, s, true)) if s.permission(&r.from.to_string()) == Some(Permission::Edit));
    let owner = world.get_resource::<Live>().is_some_and(|l| {
        l.document == r.document && matches!(&l.role, Role::Owner(o) if o.acting.is_none())
    });
    if !may || !owner {
        return r.verdict(Verdict::Refused("Not taken".into()));
    }
    let doc = match snapshot.clone().install() {
        Ok(d) if d.id == r.document => d,
        _ => return r.verdict(Verdict::Refused("Not this document".into())),
    };
    number_own(world);
    let name = peer_name(world, r.from);
    world
        .resource_mut::<ActiveDocument>()
        .apply_remote(doc, &name);
    number_own(world);
    info!("took the document back from {name}");
    r.verdict(Verdict::Accepted(0));
}

/// Whoever keeps the edits in order while the owner is away: the editor here with the lowest
/// cadrs ID (this peer among them), as everyone here works out alike.
fn elect(world: &World, g: &Guest, me: &str) -> Option<(EndpointId, String)> {
    let owner = g.link.owner.id;
    let may_edit = |id: &str| match &g.acl {
        Some(acl) => acl
            .access(id)
            .is_some_and(|a| a.permission == Permission::Edit),
        None => true,
    };
    let people = world.resource::<super::presence::People>();
    let mut candidates: Vec<(EndpointId, String)> = people
        .by_id
        .values()
        .filter(|p| p.id != owner && may_edit(&p.key()))
        .map(|p| (p.id, p.presence.name.clone()))
        .collect();
    if g.access.permission == Permission::Edit
        && let Ok(id) = me.parse::<EndpointId>()
    {
        candidates.push((id, world.resource::<UserProfile>().display_name.clone()));
    }
    candidates.into_iter().min_by_key(|(id, _)| id.to_string())
}

fn now(world: &World) -> f64 {
    world.resource::<Time<Real>>().elapsed_secs_f64()
}

/// Joins the room again, through whoever numbers the edits (see [`RETRY_EVERY`]: gossip
/// meets only the peers it dials).
fn rejoin(world: &mut World, g: &Guest, room: &mut Entity) {
    if let Ok(e) = world.get_entity_mut(*room) {
        e.despawn();
    }
    *room = session::join_room(&mut world.commands(), &g.host_link());
    world.flush();
}

/// Follows `host` from now on: the room joined through them, the document fetched from them.
/// The room is joined again even when the host is the same one: one who was away (offline, or
/// their cadrs started again) is a new node in the room, which waits to be dialled, and the
/// old link to them in the room is gone (issue #148: Disconnect, then Connect).
fn follow_host(
    world: &mut World,
    g: &mut Guest,
    room: &mut Entity,
    host: EndpointId,
    name: String,
) {
    g.host = host;
    g.host_name = name;
    rejoin(world, g, room);
    reconnect(world, g);
}

fn reconnect(world: &mut World, g: &mut Guest) {
    let task = transfer::start_fetch(world.resource::<Iroh>(), g.host_link());
    g.state = GuestState::Connecting {
        task,
        early: Vec::new(),
    };
}

fn step(
    world: &mut World,
    g: &mut Guest,
    document: DocumentId,
    room: &mut Entity,
    changed: bool,
) -> Next {
    let owner = g.link.owner.id;
    let inbox = std::mem::take(&mut *world.resource_mut::<Inbox>());
    // Only whoever numbers the edits now does: anything else in the room claiming to is
    // dropped.
    let host = g.host;
    let incoming: Vec<Sequenced> = inbox
        .edits
        .into_iter()
        .filter(|(from, m)| *from == host && m.document == document)
        .map(|(_, m)| m)
        .collect();
    let owner_back = inbox.joined.contains(&(*room, owner));
    let host_gone = inbox.left.contains(&(*room, host));
    // The owner changed who may do what: this user's part of it.
    let me = world.resource::<Iroh>().id().to_string();
    if let Some((_, a)) = inbox
        .access
        .iter()
        .rev()
        .find(|(from, a)| *from == owner && a.document == document)
    {
        g.acl = Some(a.clone());
        match a.access(&me) {
            Some(access) => {
                g.access = access;
                note_access(world, access);
            }
            None => {
                info!("{} stopped sharing {}", g.owner_name, g.link.name);
                g.state = GuestState::Removed;
                return Next::Stay;
            }
        }
    }
    // The owner is back while someone else keeps the edits in order: follow the owner.
    if owner_back && host != owner {
        follow_host(world, g, room, owner, g.owner_name.clone());
        return Next::Stay;
    }
    let t = now(world);
    match &mut g.state {
        GuestState::Connecting { task, early } => {
            early.extend(incoming);
            let Some(answer) = task.try_take() else {
                return Next::Stay;
            };
            let early = std::mem::take(early);
            match answer.and_then(|w| welcome(world, g, w)) {
                Ok(()) => {
                    if let GuestState::Following { epoch, .. } = g.state {
                        let early: Vec<_> =
                            early.into_iter().filter(|m| m.epoch == epoch).collect();
                        follow(world, g, early, t);
                    }
                }
                Err(why) => {
                    info!("cannot reach {}: {why}", g.owner_name);
                    g.state = GuestState::Away {
                        since: t,
                        ping: None,
                    };
                }
            }
        }
        GuestState::Following { .. } if host_gone => {
            info!("{} left", g.host_name);
            g.state = GuestState::Away {
                since: t,
                ping: None,
            };
        }
        GuestState::Following { .. } => {
            follow(world, g, incoming, t);
            propose_next(world, g, changed);
        }
        GuestState::Removed => {}
        GuestState::Returning { task } => {
            if let Some(r) = task.try_take() {
                if let Ok(Verdict::Refused(why)) | Err(why) = r {
                    info!("{} didn't take the document back: {why}", g.owner_name);
                }
                follow_host(world, g, room, owner, g.owner_name.clone());
            }
        }
        GuestState::Away { since, ping } => {
            let since = *since;
            // Asked whether the owner is back.
            let mut back = owner_back;
            match ping {
                Some((_, task)) => {
                    if let Some(r) = task.try_take() {
                        back |= matches!(r, Ok(Verdict::Accepted(1)));
                        *ping = None;
                    }
                }
                None if t - since > ELECT_AFTER => {
                    let task = transfer::start_status(world.resource::<Iroh>(), &g.link);
                    *ping = Some((t, task));
                }
                None => {}
            }
            let asked = matches!(ping, Some((at, _)) if t - *at < RETRY_EVERY);
            if back {
                follow_host(world, g, room, owner, g.owner_name.clone());
            } else if !incoming.is_empty() {
                reconnect(world, g);
            } else if t - since > ELECT_AFTER
                && !asked
                && let Some((id, name)) = elect(world, g, &me)
            {
                if id.to_string() == me {
                    return Next::Act;
                }
                // Someone else keeps the edits in order: follow them (if they aren't yet,
                // the fetch fails and this is tried again).
                follow_host(world, g, room, id, name);
            } else if t - since > RETRY_EVERY {
                g.state = GuestState::Away {
                    since: t,
                    ping: None,
                };
                rejoin(world, g, room);
            }
        }
    }
    Next::Stay
}

/// The owner's copy arrived: it replaces this peer's, which starts following.
fn welcome(world: &mut World, g: &mut Guest, w: Welcome) -> Result<(), String> {
    let seq = w.snapshot.seq;
    let comments = w.comments.clone();
    let doc = w.snapshot.install()?;
    if doc.id != g.link.document {
        return Err("The owner sent another document".into());
    }
    g.owner_name = w.owner_name.clone();
    g.access = w.access;
    super::comments::welcome(world, doc.id, comments);
    let lost = match &g.state {
        GuestState::Following { replica, .. } => {
            replica.in_flight() || world.resource::<ActiveDocument>().doc != *replica.confirmed()
        }
        _ => false,
    };
    if lost {
        crate::linked::toast(
            world,
            tr_with("shell-live-edits-lost", &[("owner", g.owner_name.as_str())]),
        );
    }
    let mut active = world.resource_mut::<ActiveDocument>();
    active.apply_remote(doc.clone(), &w.owner_name);
    if let Some(meta) = active.meta.as_mut()
        && let Some(s) = meta.shared.as_mut()
    {
        s.permission = w.access.permission;
        s.rights = Some(w.access.rights);
        s.owner_name = w.owner_name.clone();
    }
    g.state = if w.epoch == 0 {
        // The owner has the document closed: this is the last copy.
        GuestState::Away {
            since: now(world),
            ping: None,
        }
    } else {
        GuestState::Following {
            epoch: w.epoch,
            replica: Box::new(Replica::new(seq, doc)),
            sending: None,
            gap_since: None,
        }
    };
    Ok(())
}

/// Applies numbered edits, and fetches the document again when the copy fell behind or
/// went wrong.
fn follow(world: &mut World, g: &mut Guest, incoming: Vec<Sequenced>, t: f64) {
    let me = world.resource::<Iroh>().id().to_string();
    let GuestState::Following {
        epoch,
        replica,
        gap_since,
        ..
    } = &mut g.state
    else {
        return;
    };
    let mut again = false;
    for m in incoming {
        let change = match m.change() {
            Some(c) if m.epoch == *epoch => c,
            // Too big for the room, or a new session of the owner's.
            _ => {
                again = true;
                break;
            }
        };
        let own = (m.author == me).then_some(m.id);
        let local = world.resource::<ActiveDocument>().doc.clone();
        let r = replica.receive(
            m.seq,
            Incoming {
                own,
                change,
                hash: m.hash,
            },
            &local,
        );
        if let Some(doc) = r.local {
            world
                .resource_mut::<ActiveDocument>()
                .apply_remote(doc, &m.author_name);
        }
        if r.diverged {
            warn!("the copy of {} no longer matches the owner's", g.link.name);
            again = true;
            break;
        }
    }
    if replica.has_gap() {
        if t - *gap_since.get_or_insert(t) > GAP_PATIENCE {
            again = true;
        }
    } else {
        *gap_since = None;
    }
    if again {
        reconnect(world, g);
    }
}

/// Sends the next edit when none is on its way, and takes the owner's answer.
fn propose_next(world: &mut World, g: &mut Guest, changed: bool) {
    let link = g.host_link();
    let GuestState::Following {
        epoch,
        replica,
        sending,
        ..
    } = &mut g.state
    else {
        return;
    };
    let mut check = changed;
    if let Some((id, task)) = sending {
        let Some(answer) = task.try_take() else {
            return;
        };
        let id = *id;
        *sending = None;
        check = true;
        match answer {
            Ok(Verdict::Accepted(_)) => {}
            Ok(Verdict::Refused(why)) => {
                let local = world.resource::<ActiveDocument>().doc.clone();
                if let Some(doc) = replica.refused(id, &local) {
                    world
                        .resource_mut::<ActiveDocument>()
                        .apply_remote(doc, &g.owner_name);
                }
                info!("{} refused an edit: {why}", g.owner_name);
                crate::linked::toast(world, why);
            }
            Ok(Verdict::Resync) => return reconnect(world, g),
            other => {
                if let Err(why) = other {
                    info!("cannot propose to {}: {why}", g.owner_name);
                }
                g.state = GuestState::Away {
                    since: now(world),
                    ping: None,
                };
                return;
            }
        }
    }
    if !check || g.access.permission != Permission::Edit {
        return;
    }
    let local = world.resource::<ActiveDocument>().doc.clone();
    if let Some((id, base, change)) = replica.propose(&local) {
        let task =
            transfer::start_propose(world.resource::<Iroh>(), &link, *epoch, id, base, &change);
        *sending = Some((id, task));
    }
}

/// Keeps the access the owner gave in the shared copy's metadata.
fn note_access(world: &mut World, access: Access) {
    let mut active = world.resource_mut::<ActiveDocument>();
    if let Some(s) = active.meta.as_mut().and_then(|m| m.shared.as_mut())
        && s.access() != access
    {
        s.permission = access.permission;
        s.rights = Some(access.rights);
    }
}
