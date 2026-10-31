//! Sharing and collaboration, peer to peer ([`cadrs_net`]; Onshape's "Sharing and
//! Collaboration").
//!
//! - **Share** (the document's top bar) opens the share dialog ([`share_dialog`]): people to
//!   share with, by their cadrs ID, with **Can edit** or **Can view** and the checkboxes; and
//!   link sharing. Who the owner shares with is kept in the document's metadata
//!   ([`cadrs_core::collab::Sharing`]) and gets the link from the owner's cadrs ([`sharing`]).
//! - **Create ▸ Join shared document…** on the documents page fetches a document by a link,
//!   keeps it in the store marked as shared ([`cadrs_core::collab::SharedInfo`]: it is listed
//!   under **Shared with me**) and opens it. Someone shared with by name gets it there
//!   without asking.
//! - Opened, a shared document is edited together ([`live`]), with everyone's avatars, where
//!   they are, and Follow mode ([`presence`]), and everyone's pointer and camera live
//!   ([`pointers`]).

use bevy::prelude::*;
use cadrs_core::DocumentMeta;
use cadrs_core::collab::{Permission, Rights, SharedInfo};
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::{Answer, ShareLink, Welcome, transfer};
use cadrs_ui::i18n::{tr, tr_with};

use crate::{ActiveDocument, AppClock, AppState, DocumentStore};

pub mod comments;
pub mod live;
pub mod locks;
pub mod pointers;
pub mod presence;
pub mod share_dialog;
pub mod sharing;
pub mod versions;
#[cfg(feature = "voice")]
pub mod voice;

pub struct CollabPlugin;

impl Plugin for CollabPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Joins>()
            .init_resource::<PendingJoin>()
            .add_systems(Update, poll_joins.run_if(resource_exists::<Iroh>));
        comments::plugin(app);
        live::plugin(app);
        locks::plugin(app);
        pointers::plugin(app);
        presence::plugin(app);
        sharing::plugin(app);
        share_dialog::plugin(app);
        versions::plugin(app);
        #[cfg(feature = "voice")]
        voice::plugin(app);
    }
}

/// Why a shared document opens read-only: until its room is joined ([`live`]), or while
/// cadrs is offline ([`crate::network`]).
pub fn read_only_reason(shared: &SharedInfo) -> String {
    tr_with(
        "shell-collab-read-only-reason",
        &[("owner", shared.owner_name.as_str())],
    )
}

/// Before going offline ([`crate::network::disconnect`]): the open shared document's session
/// ends (someone else's document turns read-only until online again), and who is here, who
/// holds which tab and who is followed are forgotten.
pub fn go_offline(world: &mut World) {
    live::end(world);
    *world.resource_mut::<presence::People>() = Default::default();
    *world.resource_mut::<pointers::Pointers>() = Default::default();
    *world.resource_mut::<locks::TabLocks>() = Default::default();
    world.remove_resource::<presence::Following>();
}

/// True, with a toast saying why, when the open document is shared with this user without
/// the right `what` asks for (Copy, Export: their own cadrs keeps the owner's checkboxes).
pub fn lacks(world: &mut World, right: fn(&Rights) -> bool, what: &str) -> bool {
    let shared = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.meta.as_ref()?.shared.clone());
    match shared {
        Some(s) if !right(&s.access().rights) => {
            let owner = s.owner_name.as_str();
            let text = match what {
                "export" => tr_with("shell-collab-may-not-export", &[("owner", owner)]),
                _ => tr_with("shell-collab-may-not", &[("owner", owner), ("what", what)]),
            };
            crate::linked::toast(world, text);
            true
        }
        _ => false,
    }
}

/// The **Share** button: the share dialog, for the owner or someone who may share.
pub fn open_share(world: &mut World) {
    if world.get_resource::<Iroh>().is_none() {
        crate::network::offer_connect(
            world,
            &tr("shell-collab-share-net-off"),
            &tr("shell-collab-share-offline"),
            open_share,
        );
        return;
    }
    let Some((id, meta)) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| Some((d.doc.id, d.meta.clone()?)))
    else {
        crate::linked::toast(world, tr("shell-collab-only-saved"));
        return;
    };
    if let Some(s) = &meta.shared
        && !s.access().rights.share
    {
        crate::linked::toast(
            world,
            tr_with(
                "shell-collab-only-owner-shares",
                &[("owner", s.owner_name.as_str())],
            ),
        );
        return;
    }
    share_dialog::open(world, id);
}

/// Link sharing on with Can edit, the link on the clipboard (the `share` script command).
pub fn share_current(world: &mut World) {
    let Some(id) = world.get_resource::<ActiveDocument>().map(|d| d.doc.id) else {
        return;
    };
    let text = match sharing::set_link(world, id, Some(Permission::Edit), true).and_then(|()| {
        sharing::link_for(world, id).ok_or_else(|| tr("shell-collab-share-needs-network"))
    }) {
        Ok(link) => {
            copy_link(world, &link);
            tr("shell-collab-link-copied-edit")
        }
        Err(why) => why,
    };
    crate::linked::toast(world, text);
}

/// The sharing script commands (`crate::script`): `share-with <cadrs ID> <name> edit|view`,
/// `unshare <cadrs ID>`, `share-link off|view|edit`, on the open document.
pub fn script(world: &mut World, command: &str, args: &str) {
    let Some(id) = world.get_resource::<ActiveDocument>().map(|d| d.doc.id) else {
        return;
    };
    let words: Vec<&str> = args.split_whitespace().collect();
    let permission = |w: Option<&&str>| match w.copied() {
        Some("view") => Permission::View,
        _ => Permission::Edit,
    };
    let r = match (command, words.as_slice()) {
        ("share-with", [member, name, rest @ ..]) => {
            let member = cadrs_core::collab::Member {
                id: member.to_string(),
                name: name.to_string(),
                permission: permission(rest.first()),
                rights: None,
                delivered: None,
            };
            // As the dialog does: someone it is shared with asks the owner.
            let shared = sharing::read(world, id).is_some_and(|(_, m)| m.shared.is_some());
            if shared {
                sharing::ask_owner_to_share(world, id, member)
            } else {
                sharing::share_with(world, id, member, String::new())
            }
        }
        ("unshare", [member]) => sharing::unshare(world, id, member),
        // Transfer ownership to the member whose cadrs ID starts with this.
        ("transfer", [prefix]) => {
            let to = sharing::read(world, id).and_then(|(_, m)| {
                m.sharing?
                    .members
                    .into_iter()
                    .find(|x| x.id.starts_with(prefix))
                    .map(|x| x.id)
            });
            match to {
                Some(to) => sharing::transfer(world, id, &to),
                None => Err(format!("no member's cadrs ID starts with {prefix}")),
            }
        }
        ("share-link", ["off"]) => sharing::set_link(world, id, None, false),
        ("share-link", [p]) => sharing::set_link(world, id, Some(permission(Some(p))), true),
        _ => Err(format!("{command} {args}: not understood")),
    };
    if let Err(why) = r {
        warn!("{command}: {why}");
    }
}

/// Puts `link` on the clipboard (and in the log, for scripts).
pub fn copy_link(world: &mut World, link: &ShareLink) {
    info!("share link: {link}");
    if let Some(mut clip) = world.get_resource_mut::<bevy::clipboard::Clipboard>()
        && let Err(e) = clip.set_text(link.to_string())
    {
        warn!("clipboard: {e:?}");
    }
}

/// Documents being fetched by their links: whether to open each when it arrives.
#[derive(Resource, Default)]
struct Joins(Vec<(ShareLink, bool, Answer<Welcome>)>);

/// The link of a join that waits for the user to go online.
#[derive(Resource, Default)]
struct PendingJoin(Option<String>);

fn join_pending(world: &mut World) {
    if let Some(text) = world.resource_mut::<PendingJoin>().0.take() {
        start_join(world, &text);
    }
}

/// Join shared document: fetches the document a link names, and opens it.
pub fn start_join(world: &mut World, text: &str) {
    let link = match text.parse::<ShareLink>() {
        Ok(l) => l,
        Err(why) => {
            crate::linked::toast(world, why);
            return;
        }
    };
    let Some(iroh) = world.get_resource::<Iroh>() else {
        // Asked again once online: the link is kept for it.
        world.resource_mut::<PendingJoin>().0 = Some(text.to_string());
        crate::network::offer_connect(
            world,
            &tr("shell-collab-join-net-off"),
            &tr("shell-collab-join-offline"),
            join_pending,
        );
        return;
    };
    if link.owner.id == iroh.id() {
        crate::linked::toast(world, tr("shell-collab-own-link"));
        return;
    }
    let task = transfer::start_fetch(iroh, link.clone());
    let text = tr_with("shell-collab-joining", &[("name", link.name.as_str())]);
    world.resource_mut::<Joins>().0.push((link, true, task));
    crate::linked::toast(world, text);
}

/// Fetches a document shared with this user by name, into Shared with me.
pub fn fetch_in_background(world: &mut World, link: ShareLink) {
    if world
        .resource::<Joins>()
        .0
        .iter()
        .any(|(l, ..)| l.document == link.document)
    {
        return;
    }
    let task = transfer::start_fetch(world.resource::<Iroh>(), link.clone());
    world.resource_mut::<Joins>().0.push((link, false, task));
}

fn poll_joins(world: &mut World) {
    let mut done = Vec::new();
    world
        .resource_mut::<Joins>()
        .0
        .retain_mut(|(link, open, task)| {
            let Some(r) = task.try_take() else {
                return true;
            };
            done.push((link.clone(), *open, r));
            false
        });
    for (link, open, r) in done {
        match r.and_then(|w| keep_joined(world, &link, w)) {
            Ok(id) => {
                // Opened at once from the documents page; otherwise it waits in Shared with me.
                let landing = *world.resource::<State<AppState>>().get() == AppState::Landing;
                if open && landing {
                    crate::landing::open_document(world, id);
                } else if landing {
                    crate::landing::reload_library(world);
                } else if open {
                    let text = tr_with(
                        "shell-collab-in-shared-with-me",
                        &[("name", link.name.as_str())],
                    );
                    crate::linked::toast(world, text);
                }
            }
            Err(why) if open => crate::linked::toast(
                world,
                tr_with("shell-collab-cannot-join", &[("why", why.as_str())]),
            ),
            Err(why) => info!("cannot fetch {} yet: {why}", link.name),
        }
    }
}

/// Stores a fetched document as shared with this user (over the copy from an earlier join).
fn keep_joined(
    world: &mut World,
    link: &ShareLink,
    welcome: Welcome,
) -> Result<cadrs_core::DocumentId, String> {
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let doc = welcome.snapshot.install()?;
    if doc.id != link.document {
        return Err(tr("shell-collab-another-document"));
    }
    sharing::note_contact(world, &link.owner.id.to_string(), &welcome.owner_name);
    let shared = SharedInfo {
        owner: link.owner.id.to_string(),
        owner_name: welcome.owner_name.clone(),
        link: link.to_string(),
        permission: welcome.access.permission,
        rights: Some(welcome.access.rights),
    };
    let earlier = store.read(doc.id).ok().map(|f| f.meta);
    let mut meta = earlier
        .clone()
        .unwrap_or_else(|| DocumentMeta::new(&welcome.owner_name, now));
    meta.shared = Some(shared);
    meta.trashed = None;
    let saved = if earlier.is_some() {
        store.save(&doc, &meta)
    } else {
        store.create(&doc, &meta).map(|_| ())
    };
    saved.map_err(|e| e.to_string())?;
    Ok(doc.id)
}
