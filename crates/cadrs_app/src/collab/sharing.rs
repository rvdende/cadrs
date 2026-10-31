//! Who a document is shared with (Onshape's share dialog), peer to peer.
//!
//! - The owner's [`edit_sharing`] changes it: people added with a permission and the
//!   checkboxes, link sharing on or off. Everyone in the room hears the new access at once
//!   ([`AccessChanged`]); turning link sharing off, or removing someone while it is off, moves
//!   the document to a fresh room, so old links stop working.
//! - Everyone it is shared with gets the link from the owner's cadrs ([`deliver_invites`]):
//!   the first time with the share dialog's message ("Rouan shared “Bracket” with you"), again
//!   whenever the room moves. A member whose cadrs is not running gets it later; the owner's
//!   keeps trying while the document is open.
//! - Someone who may share (the **Share** checkbox) asks the owner to add people.
//!   **Unshare with me** tells the owner and moves the copy to the trash.
//! - [`ContactBook`]: people shared with or by, to pick in the dialog.
//! - **Transfer ownership** ([`transfer`]): the owner gives the document, who it is shared
//!   with and its comments to a member, whose cadrs takes it on and sends everyone the new
//!   link; the old owner keeps it shared with them, with Can edit.

use std::collections::HashMap;

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use cadrs_core::collab::{Access, Contacts, Member, Sharing};
use cadrs_core::{Document, DocumentId, DocumentMeta};
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::bevy_iroh::iroh::EndpointId;
use cadrs_net::bevy_iroh::prelude::NetSender;
use cadrs_net::{
    AccessChanged, Answer, Ask, Invite, Ownership, Request, ShareLink, Verdict, transfer,
};

use super::live::Live;
use crate::{ActiveDocument, DocumentStore, UserProfile};
use cadrs_ui::i18n::{tr, tr_with};

/// How long a member's cadrs gets between tries to deliver their link.
const RETRY_INVITES: f64 = 30.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Invites>()
        .init_resource::<PendingAsks>()
        .add_systems(Update, load_contacts)
        .init_resource::<Transfers>()
        .add_systems(
            Update,
            (deliver_invites, poll_asks, poll_transfers).run_if(resource_exists::<Iroh>),
        );
}

/// People this user shared with or was shared by (`contacts.ron` next to the documents).
#[derive(Resource, Default)]
pub struct ContactBook(pub Contacts);

fn load_contacts(world: &mut World) {
    if world.contains_resource::<ContactBook>() {
        return;
    }
    let Some(root) = world
        .get_resource::<DocumentStore>()
        .map(|s| s.0.root().to_path_buf())
    else {
        return;
    };
    world.insert_resource(ContactBook(Contacts::load(&root)));
}

/// Remembers `id` as `name` among the contacts.
pub fn note_contact(world: &mut World, id: &str, name: &str) {
    let Some(root) = world
        .get_resource::<DocumentStore>()
        .map(|s| s.0.root().to_path_buf())
    else {
        return;
    };
    let mut book = world.get_resource_or_insert_with(|| ContactBook(Contacts::load(&root)));
    if book.0.note(id, name)
        && let Err(e) = book.0.save(&root)
    {
        warn!("cannot save the contacts: {e}");
    }
}

/// A fresh room topic.
pub fn random_topic() -> [u8; 32] {
    let mut t = [0; 32];
    t[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    t[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    t
}

/// `id`'s contents and metadata: the open copy, or the stored one.
pub fn read(world: &World, id: DocumentId) -> Option<(Document, DocumentMeta)> {
    if let Some(d) = world
        .get_resource::<ActiveDocument>()
        .filter(|d| d.doc.id == id)
    {
        return Some((d.doc.clone(), d.meta.clone()?));
    }
    let file = world.resource::<DocumentStore>().0.read(id).ok()?;
    Some((file.document, file.meta))
}

/// Saves `id`'s metadata (the open copy's, or the stored one's).
fn write_meta(world: &mut World, id: DocumentId, meta: DocumentMeta) -> Result<(), String> {
    let store = world.resource::<DocumentStore>().0.clone();
    if let Some(mut d) = world
        .get_resource_mut::<ActiveDocument>()
        .filter(|d| d.doc.id == id)
    {
        d.meta = Some(meta);
        return d.save_now(&store).map_err(|e| e.to_string());
    }
    let file = store.read(id).map_err(|e| e.to_string())?;
    store.save(&file.document, &meta).map_err(|e| e.to_string())
}

/// The link to `id` as shared now (this node is its owner).
pub fn link_for(world: &World, id: DocumentId) -> Option<ShareLink> {
    let owner = world.get_resource::<Iroh>()?.addr()?;
    let (doc, meta) = read(world, id)?;
    let sharing = meta.sharing.as_ref()?;
    Some(ShareLink {
        document: id,
        name: doc.name,
        topic: sharing.topic,
        owner,
    })
}

fn access_message(id: DocumentId, s: &Sharing) -> AccessChanged {
    AccessChanged {
        document: id,
        members: s
            .members
            .iter()
            .map(|m| (m.id.clone(), m.access()))
            .collect(),
        link: s.link_access(),
    }
}

/// Sends `msg` to the room of the shared document open now, if it is `msg`'s.
fn tell_room(world: &mut World, msg: &AccessChanged) {
    let Some(room) = world
        .get_resource::<Live>()
        .filter(|l| l.document == msg.document && l.is_owner())
        .map(|l| l.room())
    else {
        return;
    };
    let mut state = SystemState::<NetSender>::new(world);
    if let Ok(net) = state.get(world) {
        net.broadcast(room, msg);
    }
}

/// Changes how the owner shares `id` (open or stored; shared for the first time if it wasn't)
/// and saves it. `f` returns true when the document must move to a fresh room: everyone in
/// the old one hears the new access first.
pub fn edit_sharing(
    world: &mut World,
    id: DocumentId,
    f: impl FnOnce(&mut Sharing) -> bool,
) -> Result<(), String> {
    let (_, mut meta) = read(world, id).ok_or_else(|| tr("shell-sharing-not-in-store"))?;
    if let Some(s) = &meta.shared {
        return Err(tr_with(
            "shell-sharing-only-owner-changes",
            &[("owner", s.owner_name.as_str())],
        ));
    }
    let sharing = meta
        .sharing
        .get_or_insert_with(|| Sharing::new(random_topic()));
    let rotate = f(sharing);
    tell_room(world, &access_message(id, sharing));
    if rotate {
        sharing.rotate(random_topic());
    }
    write_meta(world, id, meta)
}

/// Shares `id` with `member` (or changes their access), with the share dialog's message for
/// their first link.
pub fn share_with(
    world: &mut World,
    id: DocumentId,
    member: Member,
    message: String,
) -> Result<(), String> {
    let me = world.get_resource::<Iroh>().map(|i| i.id().to_string());
    if me.as_deref() == Some(member.id.as_str()) {
        return Err(tr("shell-sharing-own-id"));
    }
    if member.id.parse::<EndpointId>().is_err() {
        return Err(tr("shell-sharing-not-an-id"));
    }
    note_contact(world, &member.id, &member.name);
    let mid = member.id.clone();
    edit_sharing(world, id, move |s| {
        match s.members.iter_mut().find(|m| m.id == member.id) {
            Some(m) => {
                m.permission = member.permission;
                m.rights = member.rights;
                if !member.name.is_empty() {
                    m.name = member.name;
                }
            }
            None => s.members.push(member),
        }
        false
    })?;
    if !message.is_empty() {
        world
            .resource_mut::<Invites>()
            .messages
            .insert((id, mid), message);
    }
    world.resource_mut::<Invites>().next = 0.0;
    Ok(())
}

/// Stops sharing `id` with `member`. Without link sharing the document moves to a fresh room
/// they don't know; with it, they keep what the link gives (as anyone with the link does),
/// and the link keeps working for everyone.
pub fn unshare(world: &mut World, id: DocumentId, member: &str) -> Result<(), String> {
    edit_sharing(world, id, |s| {
        s.members.retain(|m| m.id != member);
        s.link.is_none()
    })
}

/// Link sharing on with `access` (`None`: off, and the document moves to a fresh room).
pub fn set_link(
    world: &mut World,
    id: DocumentId,
    link: Option<cadrs_core::collab::Permission>,
    export: bool,
) -> Result<(), String> {
    edit_sharing(world, id, |s| {
        let off = s.link.is_some() && link.is_none();
        s.link = link;
        s.link_export = export;
        off
    })?;
    world.resource_mut::<Invites>().next = 0.0;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Getting the link to everyone

/// The links on their way, and when to try again.
#[derive(Resource, Default)]
pub struct Invites {
    /// Per document and member: the room their link names, and the answer to come.
    sending: HashMap<(DocumentId, String), ([u8; 32], Answer<Verdict>)>,
    /// The share dialog's message for a member's first link.
    messages: HashMap<(DocumentId, String), String>,
    /// When to look again (seconds on the real clock).
    next: f64,
}

/// Sends the open document's link to every member who doesn't have the current one.
fn deliver_invites(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let done: Vec<_> = {
        let mut inv = world.resource_mut::<Invites>();
        let mut done = Vec::new();
        inv.sending
            .retain(|key, (topic, task)| match task.try_take() {
                Some(r) => {
                    done.push((key.clone(), *topic, r));
                    false
                }
                None => true,
            });
        done
    };
    for ((doc, member), topic, r) in done {
        match r {
            Ok(Verdict::Accepted(_)) => {
                let delivered = edit_meta(world, doc, |s| {
                    if let Some(m) = s.members.iter_mut().find(|m| m.id == member) {
                        m.delivered = Some(topic);
                    }
                });
                if let Err(e) = delivered {
                    warn!("cannot note the link sent to {member}: {e}");
                }
            }
            Ok(other) => info!("{member} did not take the link: {other:?}"),
            Err(why) => debug!("cannot reach {member} yet: {why}"),
        }
    }
    if now < world.resource::<Invites>().next {
        return;
    }
    world.resource_mut::<Invites>().next = now + RETRY_INVITES;
    let Some(id) = world
        .get_resource::<Live>()
        .filter(|l| l.is_owner())
        .map(|l| l.document)
    else {
        return;
    };
    let Some(link) = link_for(world, id) else {
        return;
    };
    let Some((_, meta)) = read(world, id) else {
        return;
    };
    let Some(sharing) = meta.sharing else { return };
    let owner_name = world.resource::<UserProfile>().display_name.clone();
    for m in &sharing.members {
        let key = (id, m.id.clone());
        if m.delivered == Some(sharing.topic)
            || world.resource::<Invites>().sending.contains_key(&key)
        {
            continue;
        }
        let Ok(to) = m.id.parse::<EndpointId>() else {
            continue;
        };
        let message = world
            .resource_mut::<Invites>()
            .messages
            .remove(&key)
            .unwrap_or_default();
        let invite = Invite {
            link: link.to_string(),
            owner_name: owner_name.clone(),
            access: m.access(),
            message,
            update: m.delivered.is_some(),
        };
        let task = transfer::start_invite(world.resource::<Iroh>(), to, &link, invite);
        world
            .resource_mut::<Invites>()
            .sending
            .insert(key, (sharing.topic, task));
    }
}

/// Changes the sharing without telling anyone or moving rooms (bookkeeping).
fn edit_meta(
    world: &mut World,
    id: DocumentId,
    f: impl FnOnce(&mut Sharing),
) -> Result<(), String> {
    let (_, mut meta) = read(world, id).ok_or_else(|| tr("shell-sharing-not-in-store"))?;
    let Some(s) = meta.sharing.as_mut() else {
        return Ok(());
    };
    f(s);
    write_meta(world, id, meta)
}

// ---------------------------------------------------------------------------------------------
// Requests about sharing

/// Answers an Invite, an AddMember or a Leave.
pub(super) fn answer(world: &mut World, r: Request) {
    match &r.ask {
        Ask::Invite(invite) => {
            let invite = invite.clone();
            invited(world, r, invite)
        }
        Ask::AddMember { id, name, access } => {
            let (id, name, access) = (id.clone(), name.clone(), *access);
            add_member(world, r, id, name, access)
        }
        Ask::Leave => {
            let from = r.from.to_string();
            let ok = owned(world, &r).and_then(|_| {
                edit_sharing(world, r.document, |s| {
                    s.members.retain(|m| m.id != from);
                    false
                })
            });
            r.verdict(match ok {
                Ok(()) => Verdict::Accepted(0),
                Err(why) => Verdict::Refused(why),
            })
        }
        Ask::TakeOwnership(o) => {
            let o = (**o).clone();
            take_ownership(world, r, o)
        }
        Ask::Join
        | Ask::Propose { .. }
        | Ask::Comment(_)
        | Ask::CreateVersion { .. }
        | Ask::Lock { .. }
        | Ask::Unlock { .. }
        | Ask::Handover(_)
        | Ask::Status => {}
    }
}

/// The owner's sharing of the document `r` names, checked against its topic.
fn owned(world: &World, r: &Request) -> Result<Sharing, String> {
    read(world, r.document)
        .and_then(|(_, m)| m.sharing)
        .filter(|s| s.topic == r.topic)
        .ok_or_else(|| "This document is no longer shared".into())
}

/// Someone who may share asks the owner to share the document with someone else; they can't
/// give more than they have.
fn add_member(world: &mut World, r: Request, id: String, name: String, access: Access) {
    let from = r.from.to_string();
    let result = owned(world, &r).and_then(|s| {
        let mine = s
            .access(&from)
            .filter(|a| a.rights.share)
            .ok_or("You may not share this document: ask its owner".to_string())?;
        let permission = if mine.permission.can_edit() {
            access.permission
        } else {
            cadrs_core::collab::Permission::View
        };
        share_with(
            world,
            r.document,
            Member {
                id,
                name,
                permission,
                rights: Some(access.rights),
                delivered: None,
            },
            String::new(),
        )
    });
    r.verdict(match result {
        Ok(()) => Verdict::Accepted(0),
        Err(why) => Verdict::Refused(why),
    });
}

/// The owner says a document is shared with this user: keep its link and fetch it.
fn invited(world: &mut World, r: Request, invite: Invite) {
    let link = match invite.link.parse::<ShareLink>() {
        Ok(l) if l.owner.id == r.from && l.document == r.document => l,
        _ => {
            return r.verdict(Verdict::Refused(
                "Not a link to the sender's document".into(),
            ));
        }
    };
    note_contact(world, &link.owner.id.to_string(), &invite.owner_name);
    let shared = cadrs_core::collab::SharedInfo {
        owner: link.owner.id.to_string(),
        owner_name: invite.owner_name.clone(),
        link: invite.link.clone(),
        permission: invite.access.permission,
        rights: Some(invite.access.rights),
    };
    // A copy already here takes the new link (an open one rejoins the new room); otherwise it
    // is fetched and waits in Shared with me.
    let known = read(world, link.document).filter(|(_, m)| m.shared.is_some());
    match known {
        Some((_, mut meta)) => {
            meta.shared = Some(shared);
            meta.trashed = None;
            if let Err(e) = write_meta(world, link.document, meta) {
                warn!("cannot keep the new link: {e}");
            }
        }
        None => super::fetch_in_background(world, link.clone()),
    }
    if !invite.update {
        let (owner, name) = (invite.owner_name.as_str(), link.name.as_str());
        let text = if invite.message.is_empty() {
            tr_with(
                "shell-sharing-shared-with-you",
                &[("owner", owner), ("name", name)],
            )
        } else {
            tr_with(
                "shell-sharing-shared-with-you-message",
                &[
                    ("owner", owner),
                    ("name", name),
                    ("message", invite.message.as_str()),
                ],
            )
        };
        crate::linked::toast(world, text);
    }
    r.verdict(Verdict::Accepted(0));
}

// ---------------------------------------------------------------------------------------------
// Asking the owner

/// What this user asked owners, and what to say when they answer.
#[derive(Resource, Default)]
struct PendingAsks(Vec<(String, Answer<Verdict>)>);

/// Asks the owner of the shared document `id` to share it with `member`.
pub fn ask_owner_to_share(world: &mut World, id: DocumentId, member: Member) -> Result<(), String> {
    let (_, meta) = read(world, id).ok_or_else(|| tr("shell-sharing-not-in-store"))?;
    let shared = meta
        .shared
        .ok_or_else(|| tr("shell-sharing-own-document"))?;
    if !shared.access().rights.share {
        return Err(tr_with(
            "shell-collab-only-owner-shares",
            &[("owner", shared.owner_name.as_str())],
        ));
    }
    let link: ShareLink = shared.link.parse()?;
    note_contact(world, &member.id, &member.name);
    let access = member.access();
    let task = transfer::start_add_member(
        world.resource::<Iroh>(),
        &link,
        member.id,
        member.name.clone(),
        access,
    );
    let done = tr_with(
        "shell-sharing-owner-shared-it",
        &[
            ("owner", shared.owner_name.as_str()),
            ("name", member.name.as_str()),
        ],
    );
    world.resource_mut::<PendingAsks>().0.push((done, task));
    Ok(())
}

/// Unshare with me: tells the owner and moves the copy to the trash.
pub fn leave(world: &mut World, id: DocumentId) {
    let Some((_, meta)) = read(world, id) else {
        return;
    };
    let Some(shared) = meta.shared else { return };
    if let (Ok(link), Some(iroh)) = (
        shared.link.parse::<ShareLink>(),
        world.get_resource::<Iroh>(),
    ) {
        let task = transfer::start_leave(iroh, &link);
        world
            .resource_mut::<PendingAsks>()
            .0
            .push((String::new(), task));
    }
}

fn poll_asks(world: &mut World) {
    let mut done = Vec::new();
    world
        .resource_mut::<PendingAsks>()
        .0
        .retain_mut(|(text, task)| match task.try_take() {
            Some(r) => {
                done.push((text.clone(), r));
                false
            }
            None => true,
        });
    for (text, r) in done {
        match r {
            Ok(Verdict::Accepted(_)) if !text.is_empty() => crate::linked::toast(world, text),
            Ok(Verdict::Refused(why)) | Err(why) => crate::linked::toast(world, why),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Transfer ownership

/// A room that once had a member's link but no longer does: the new owner sends everyone
/// the link again (as an update, not news).
const OLD_ROOM: [u8; 32] = [0; 32];

/// Transfers on their way: the document, the new owner's id and name, and the answer.
#[derive(Resource, Default)]
pub struct Transfers(Vec<(DocumentId, String, String, Answer<Verdict>)>);

/// Transfer ownership of `id` to member `to` (their cadrs must be running).
pub fn transfer(world: &mut World, id: DocumentId, to: &str) -> Result<(), String> {
    let (doc, meta) = read(world, id).ok_or_else(|| tr("shell-sharing-not-in-store"))?;
    if let Some(s) = &meta.shared {
        return Err(tr_with(
            "shell-sharing-only-owner-transfers",
            &[("owner", s.owner_name.as_str())],
        ));
    }
    let sharing = meta
        .sharing
        .clone()
        .ok_or_else(|| tr("shell-sharing-share-first"))?;
    let member = sharing
        .members
        .iter()
        .find(|m| m.id == to)
        .cloned()
        .ok_or_else(|| tr("shell-sharing-only-members-own"))?;
    let to_id = to.parse::<EndpointId>().map_err(|e| e.to_string())?;
    let link = link_for(world, id).ok_or_else(|| tr("shell-collab-share-needs-network"))?;
    // The document as everyone has it: its edits numbered first.
    super::live::number_own(world);
    let doc = world
        .get_resource::<Live>()
        .filter(|l| l.document == id)
        .and_then(|l| match &l.role {
            super::live::Role::Owner(o) => Some(o.confirmed().clone()),
            _ => None,
        })
        .unwrap_or(doc);
    let store = world.resource::<DocumentStore>().0.clone();
    let ownership = Ownership {
        snapshot: cadrs_core::collab::Snapshot::of(&doc, 0, &store)?,
        sharing,
        comments: super::comments::for_document(world, id),
        from_name: world.resource::<UserProfile>().display_name.clone(),
    };
    let task = transfer::start_transfer(world.resource::<Iroh>(), to_id, &link, &ownership);
    world
        .get_resource_or_init::<Transfers>()
        .0
        .push((id, member.id, member.name, task));
    Ok(())
}

/// The old owner's side: the new owner took it, so the document is shared with this user now.
pub(super) fn poll_transfers(world: &mut World) {
    let Some(mut transfers) = world.get_resource_mut::<Transfers>() else {
        return;
    };
    let mut done = Vec::new();
    transfers
        .0
        .retain_mut(|(doc, id, name, task)| match task.try_take() {
            Some(r) => {
                done.push((*doc, id.clone(), name.clone(), r));
                false
            }
            None => true,
        });
    for (doc, id, name, r) in done {
        match r {
            Ok(Verdict::Accepted(_)) => {
                let Some((d, mut meta)) = read(world, doc) else {
                    continue;
                };
                let Some(sharing) = meta.sharing.take() else {
                    continue;
                };
                let Ok(owner) = id.parse::<EndpointId>() else {
                    continue;
                };
                let link = ShareLink {
                    document: doc,
                    name: d.name.clone(),
                    topic: sharing.topic,
                    owner: cadrs_net::bevy_iroh::iroh::EndpointAddr::new(owner),
                };
                meta.shared = Some(cadrs_core::collab::SharedInfo {
                    owner: id.clone(),
                    owner_name: name.clone(),
                    link: link.to_string(),
                    permission: cadrs_core::collab::Permission::Edit,
                    rights: None,
                });
                meta.owned_by = name.clone();
                if let Err(e) = write_meta(world, doc, meta) {
                    warn!("cannot keep the transfer: {e}");
                }
                crate::linked::toast(
                    world,
                    tr_with(
                        "shell-sharing-now-belongs-to",
                        &[("document", d.name.as_str()), ("name", name.as_str())],
                    ),
                );
            }
            Ok(Verdict::Refused(why)) | Err(why) => crate::linked::toast(
                world,
                tr_with(
                    "shell-sharing-cannot-transfer",
                    &[("name", name.as_str()), ("why", why.as_str())],
                ),
            ),
            Ok(_) => {}
        }
    }
}

/// The new owner's side: the document is this user's now.
fn take_ownership(world: &mut World, r: Request, o: Ownership) {
    let from = r.from.to_string();
    let mine =
        read(world, r.document).filter(|(_, m)| m.shared.as_ref().is_some_and(|s| s.owner == from));
    let Some((_, mut meta)) = mine else {
        return r.verdict(Verdict::Refused("Not from this document's owner".into()));
    };
    let doc = match o.snapshot.install() {
        Ok(d) if d.id == r.document => d,
        _ => return r.verdict(Verdict::Refused("Not this document".into())),
    };
    let me = world
        .get_resource::<Iroh>()
        .map(|i| i.id().to_string())
        .unwrap_or_default();
    let mut sharing = o.sharing;
    sharing.members.retain(|m| m.id != me);
    for m in &mut sharing.members {
        if m.delivered.is_some() {
            m.delivered = Some(OLD_ROOM);
        }
    }
    sharing.members.push(Member {
        id: from.clone(),
        name: o.from_name.clone(),
        permission: cadrs_core::collab::Permission::Edit,
        rights: None,
        delivered: Some(OLD_ROOM),
    });
    meta.shared = None;
    meta.sharing = Some(sharing);
    meta.owned_by = world.resource::<UserProfile>().id.clone();
    note_contact(world, &from, &o.from_name);
    super::comments::welcome(world, doc.id, o.comments);
    let name = doc.name.clone();
    let open = world
        .get_resource::<ActiveDocument>()
        .is_some_and(|d| d.doc.id == doc.id);
    let saved = if open {
        let store = world.resource::<DocumentStore>().0.clone();
        let mut d = world.resource_mut::<ActiveDocument>();
        d.apply_remote(doc, &o.from_name);
        d.meta = Some(meta);
        d.read_only = None;
        d.save_now(&store).map_err(|e| e.to_string())
    } else {
        world
            .resource::<DocumentStore>()
            .0
            .save(&doc, &meta)
            .map_err(|e| e.to_string())
    };
    if let Err(e) = saved {
        return r.verdict(Verdict::Refused(format!("Cannot keep it: {e}")));
    }
    world.resource_mut::<Invites>().next = 0.0;
    crate::linked::toast(
        world,
        tr_with(
            "shell-sharing-made-you-owner",
            &[("name", o.from_name.as_str()), ("document", name.as_str())],
        ),
    );
    r.verdict(Verdict::Accepted(0));
}
