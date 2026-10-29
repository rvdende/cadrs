//! The Comments panel, as Onshape's (`comments_on_workspaces.htm`): the left rail's
//! **Comments** button opens it beside the feature list.
//!
//! - **Add comment**: a message, optionally tagged to a feature (right-click a feature ▸
//!   **Add comment**), mentioning people with `@name`. With **Assign to** checked it is a task
//!   for the first person mentioned, who resolves it with its checkbox.
//! - Threads, newest first, with their replies; **Reply**, and ✕ for one's own (the owner
//!   may remove any). The filter: All comments, All open tasks, Open tasks assigned to me,
//!   Completed tasks. A dot on the rail button says there is something new.
//! - On a shared document the owner's cadrs keeps the comments in order: everyone else sends
//!   theirs to it (if they may comment) and applies what it passes on. Someone mentioned or
//!   given a task gets a toast. Comments are kept beside the document (`comments.ron`), not in
//!   its history.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::Activate;
use cadrs_core::collab::comments::{Comment, CommentOp, Comments, Tag};
use cadrs_core::{DocumentId, FeatureId};
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::bevy_iroh::prelude::{NetSender, Received};
use cadrs_net::{Answer, Ask, CommentChanged, Request, ShareLink, Verdict, transfer};
use cadrs_ui::prelude::*;
use cadrs_ui::{
    Avatar, Button, Checkbox, CheckboxChange, CheckboxState, IconButton, Select, SelectChange,
    TextInput, TextInputField,
};

use super::live::{GuestState, Live, Role};
use super::sharing::{self, ContactBook};
use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<CommentsPanel>()
        .init_resource::<DocComments>()
        .init_resource::<PendingComments>()
        .add_message::<Received<CommentChanged>>()
        .add_systems(
            Update,
            (
                load,
                hear.run_if(resource_exists::<Iroh>),
                poll.run_if(resource_exists::<Iroh>),
                sync_panel,
                sync_rail,
            )
                .chain(),
        )
        .add_observer(on_activate)
        .add_observer(on_filter)
        .add_observer(on_resolve);
}

/// The open document's comments.
#[derive(Resource, Default)]
pub struct DocComments {
    pub document: Option<DocumentId>,
    pub comments: Comments,
    /// Bumped on every change, for the panel.
    generation: u64,
    /// Something new arrived while the panel was closed.
    unread: bool,
}

/// The panel's state.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct CommentsPanel {
    pub open: bool,
    /// All comments, All open tasks, Open tasks assigned to me, Completed tasks.
    filter: usize,
    /// What the next comment is tagged to.
    tag: Option<Tag>,
    /// The thread the next comment replies to.
    reply_to: Option<uuid::Uuid>,
}

const FILTERS: [&str; 4] = [
    "All comments",
    "All open tasks",
    "Open tasks assigned to me",
    "Completed tasks",
];

/// This user's id (their cadrs ID, or the local user's without the network) and name.
fn me(world: &World) -> (String, String) {
    let name = world.resource::<UserProfile>().display_name.clone();
    let id = match world.get_resource::<Iroh>() {
        Some(i) => i.id().to_string(),
        None => world.resource::<UserProfile>().id.clone(),
    };
    (id, name)
}

/// The open document's owner's id.
fn owner_of(world: &World) -> String {
    world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.meta.as_ref()?.shared.as_ref().map(|s| s.owner.clone()))
        .unwrap_or_else(|| me(world).0)
}

fn dir(world: &World, id: DocumentId) -> Option<std::path::PathBuf> {
    world
        .get_resource::<DocumentStore>()
        .map(|s| s.0.doc_dir(id))
}

/// The comments follow the open document.
fn load(world: &mut World) {
    let open = world
        .get_resource::<ActiveDocument>()
        .filter(|_| *world.resource::<State<AppState>>().get() == AppState::Document)
        .map(|d| d.doc.id);
    if world.resource::<DocComments>().document == open {
        return;
    }
    let comments = open
        .and_then(|id| dir(world, id))
        .map(|d| Comments::load(&d))
        .unwrap_or_default();
    let mut dc = world.resource_mut::<DocComments>();
    dc.document = open;
    dc.comments = comments;
    dc.generation += 1;
    dc.unread = false;
    let mut panel = world.resource_mut::<CommentsPanel>();
    panel.tag = None;
    panel.reply_to = None;
}

/// Applies `op` by `by` to the open document's comments and keeps them.
fn apply_local(world: &mut World, op: &CommentOp, by: &str) -> Result<(), String> {
    let owner = owner_of(world);
    let Some(id) = world.resource::<DocComments>().document else {
        return Err("No document is open".into());
    };
    world
        .resource_mut::<DocComments>()
        .comments
        .apply(op, by, &owner)
        .map_err(|e| e.to_string())?;
    let mut dc = world.resource_mut::<DocComments>();
    dc.generation += 1;
    let comments = dc.comments.clone();
    if let Some(d) = dir(world, id)
        && let Err(e) = comments.save(&d)
    {
        warn!("cannot save the comments: {e}");
    }
    Ok(())
}

/// The open document's comments as they are, for a peer joining (the owner's side).
pub fn for_document(world: &World, id: DocumentId) -> Comments {
    let dc = world.resource::<DocComments>();
    if dc.document == Some(id) {
        return dc.comments.clone();
    }
    dir(world, id)
        .map(|d| Comments::load(&d))
        .unwrap_or_default()
}

/// The owner's comments arrived with the document: they replace this copy's.
pub fn welcome(world: &mut World, id: DocumentId, comments: Comments) {
    if let Some(d) = dir(world, id)
        && let Err(e) = comments.save(&d)
    {
        warn!("cannot save the comments: {e}");
    }
    let mut dc = world.resource_mut::<DocComments>();
    if dc.document == Some(id) && dc.comments != comments {
        dc.comments = comments;
        dc.generation += 1;
    }
}

/// Changes the comments: at once on one's own document (and to everyone in its room); through
/// the owner on a shared one.
pub fn post(world: &mut World, op: CommentOp) -> Result<(), String> {
    let (my_id, _) = me(world);
    let shared = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.meta.as_ref()?.shared.clone());
    let Some(shared) = shared else {
        apply_local(world, &op, &my_id)?;
        tell_room(world, &op, &my_id);
        return Ok(());
    };
    if !shared.access().rights.comment {
        return Err(format!(
            "{} hasn't let you comment on this document",
            shared.owner_name
        ));
    }
    let following = world.get_resource::<Live>().is_some_and(
        |l| matches!(&l.role, Role::Guest(g) if matches!(g.state, GuestState::Following { .. })),
    );
    if !following {
        return Err(format!(
            "Comments reach {} once they're back",
            shared.owner_name
        ));
    }
    let link: ShareLink = shared.link.parse()?;
    let task = transfer::start_comment(world.resource::<Iroh>(), &link, op);
    world.resource_mut::<PendingComments>().0.push(task);
    Ok(())
}

/// Passes `op` (applied here) on to the room, when this is the owner of the shared document.
fn tell_room(world: &mut World, op: &CommentOp, by: &str) {
    let Some((document, room)) = world
        .get_resource::<Live>()
        .filter(|l| l.is_owner())
        .map(|l| (l.document, l.room()))
    else {
        return;
    };
    let Some(msg) = CommentChanged::new(document, by, op) else {
        return;
    };
    let mut state = SystemState::<NetSender>::new(world);
    if let Ok(net) = state.get(world) {
        net.broadcast(room, &msg);
    }
}

/// The owner's side: a comment from someone the document is shared with.
pub(super) fn answer(world: &mut World, r: Request) {
    let Ask::Comment(op) = &r.ask else { return };
    let op = op.clone();
    let from = r.from.to_string();
    let allowed = sharing::read(world, r.document)
        .and_then(|(_, m)| m.sharing)
        .filter(|s| s.topic == r.topic)
        .and_then(|s| s.access(&from))
        .is_some_and(|a| a.rights.comment);
    if !allowed {
        info!("refused a comment from {}", r.from.fmt_short());
        return r.verdict(Verdict::Refused(
            "You may not comment on this document".into(),
        ));
    }
    let open = world.resource::<DocComments>().document == Some(r.document);
    let result = if open {
        apply_local(world, &op, &from).map(|()| tell_room(world, &op, &from))
    } else {
        // Closed: kept for when it opens (nobody is in its room to tell).
        let owner = me(world).0;
        let d = dir(world, r.document);
        let mut cs = d.as_deref().map(Comments::load).unwrap_or_default();
        cs.apply(&op, &from, &owner)
            .map_err(|e| e.to_string())
            .and_then(|()| match &d {
                Some(d) => cs.save(d).map_err(|e| e.to_string()),
                None => Ok(()),
            })
    };
    match &result {
        Ok(()) => notify(world, &op, &from),
        Err(why) => info!("comment from {} not applied: {why}", r.from.fmt_short()),
    }
    r.verdict(match result {
        Ok(()) => Verdict::Accepted(0),
        Err(why) => Verdict::Refused(why),
    });
}

/// What the owner passes on, applied here.
fn hear(
    mut rx: MessageReader<Received<CommentChanged>>,
    live: Option<Res<Live>>,
    mut commands: Commands,
) {
    let owner = live.as_ref().and_then(|l| match &l.role {
        Role::Guest(g) => Some((l.document, g.link.owner.id)),
        Role::Owner(_) => None,
    });
    for m in rx.read() {
        let Some((document, owner)) = owner else {
            continue;
        };
        if m.from != owner || m.msg.document != document {
            continue;
        }
        let Some(op) = m.msg.op() else { continue };
        let by = m.msg.by.clone();
        commands.queue(move |w: &mut World| {
            if apply_local(w, &op, &by).is_ok() {
                notify(w, &op, &by);
            }
        });
    }
}

/// A new comment by someone else: the rail's dot, and a toast for whoever it mentions or
/// gives a task to.
fn notify(world: &mut World, op: &CommentOp, by: &str) {
    let CommentOp::Add(c) = op else { return };
    let (my_id, my_name) = me(world);
    if by == my_id {
        return;
    }
    if !world.resource::<CommentsPanel>().open {
        world.resource_mut::<DocComments>().unread = true;
    }
    let mentioned = c.mentions().iter().any(|m| matches_name(m, &my_name));
    let task = c.assignee.as_ref().is_some_and(|(a, _)| *a == my_id);
    if task {
        crate::linked::toast(
            world,
            format!("{} gave you a task: {}", c.author_name, c.text),
        );
    } else if mentioned {
        crate::linked::toast(
            world,
            format!("{} mentioned you: {}", c.author_name, c.text),
        );
    }
}

/// Whether `@mention` names `name` (its first word, or all of it without spaces).
fn matches_name(mention: &str, name: &str) -> bool {
    let first = name.split_whitespace().next().unwrap_or("");
    let joined: String = name.split_whitespace().collect();
    mention.eq_ignore_ascii_case(first) || mention.eq_ignore_ascii_case(&joined)
}

/// Comments on their way to the owner.
#[derive(Resource, Default)]
struct PendingComments(Vec<Answer<Verdict>>);

fn poll(world: &mut World) {
    let mut done = Vec::new();
    world
        .resource_mut::<PendingComments>()
        .0
        .retain_mut(|t| match t.try_take() {
            Some(r) => {
                done.push(r);
                false
            }
            None => true,
        });
    for r in done {
        match r {
            Ok(Verdict::Refused(why)) | Err(why) => {
                info!("comment not taken: {why}");
                crate::linked::toast(world, why);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The panel

/// Right-click a feature ▸ Add comment: the panel, with the next comment tagged to it.
pub fn start_tagged(world: &mut World, feature: FeatureId) {
    let Some(element) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active)
    else {
        return;
    };
    let mut panel = world.resource_mut::<CommentsPanel>();
    panel.open = true;
    panel.tag = Some(Tag::Feature(element, feature));
    panel.reply_to = None;
    world
        .resource_mut::<crate::history_panel::HistoryPanel>()
        .open = false;
}

#[derive(Component)]
struct PanelRoot(u64, CommentsPanel);

/// A floating glass panel left of the feature list.
fn panel_node(t: &Theme) -> impl Bundle {
    (
        Node {
            width: Val::Px(290.0),
            flex_shrink: 0.0,
            margin: UiRect::right(Val::Px(crate::document::PANEL_GAP)),
            overflow: Overflow::clip(),
            ..cadrs_ui::glass::panel_node(t)
        },
        cadrs_ui::glass::panel(t),
    )
}

fn tag_label(doc: &ActiveDocument, tag: Tag) -> String {
    match tag {
        Tag::Element(e) => doc
            .doc
            .element(e)
            .map_or("a tab".into(), |e| e.name.clone()),
        Tag::Feature(e, f) => doc
            .doc
            .element(e)
            .and_then(|e| e.features().iter().find(|x| x.id == f))
            .map_or("a feature".into(), |f| f.name.clone()),
    }
}

fn shown(c: &Comment, filter: usize, me: &str) -> bool {
    match filter {
        1 => c.is_task() && !c.resolved,
        2 => !c.resolved && c.assignee.as_ref().is_some_and(|(a, _)| a == me),
        3 => c.is_task() && c.resolved,
        _ => true,
    }
}

fn sync_panel(world: &mut World, q_root: &mut QueryState<(Entity, &'static PanelRoot)>) {
    let panel = world.resource::<CommentsPanel>().clone();
    let generation = world.resource::<DocComments>().generation;
    let roots: Vec<(Entity, bool)> = q_root
        .iter(world)
        .map(|(e, r)| (e, r.0 == generation && r.1 == panel))
        .collect();
    let open = panel.open && *world.resource::<State<AppState>>().get() == AppState::Document;
    if open && roots.iter().any(|(_, same)| *same) {
        return;
    }
    for (e, _) in &roots {
        if let Ok(e) = world.get_entity_mut(*e) {
            e.despawn();
        }
    }
    if !open {
        return;
    }
    let mut q_named = world.query::<(&Name, &ChildOf)>();
    let Some(parent) = q_named
        .iter(world)
        .find(|(n, _)| n.as_str() == "feature-panel")
        .map(|(_, c)| c.parent())
    else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let t = &theme;
    let (my_id, _) = me(world);
    let owner = owner_of(world);
    let comments = world.resource::<DocComments>().comments.clone();
    let clock = *world.resource::<AppClock>();
    let (doc_tag, threads): (Option<String>, Vec<(Comment, Vec<Comment>)>) = {
        let doc = world.resource::<ActiveDocument>();
        let tag = panel.tag.map(|tag| tag_label(doc, tag));
        let threads = comments
            .threads()
            .filter(|c| shown(c, panel.filter, &my_id))
            .map(|c| {
                let mut c = c.clone();
                if let Some(tag) = c.tag {
                    c.text = format!("#{}  {}", tag_label(doc, tag), c.text);
                }
                (c.clone(), comments.replies(c.id).cloned().collect())
            })
            .collect();
        (tag, threads)
    };
    let reply_name = panel
        .reply_to
        .and_then(|id| comments.get(id))
        .map(|c| c.author_name.clone());
    let root = world
        .spawn((
            Name::new("comments-panel"),
            PanelRoot(generation, panel.clone()),
            DespawnOnExit(AppState::Document),
            panel_node(t),
        ))
        .with_children(|p| {
            // Header: title, the filter, ×.
            p.spawn(Node {
                height: Val::Px(32.0),
                flex_shrink: 0.0,
                padding: UiRect::new(Val::Px(10.0), Val::Px(4.0), Val::ZERO, Val::ZERO),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|h| {
                h.spawn(t.text("Comments", t.font_lg, FontWeight::BOLD, t.foreground))
                    .entry::<Node>()
                    .and_modify(|mut n| n.flex_grow = 1.0);
                let mut select = Select::new("comments-filter").width(Val::Px(150.0));
                for f in FILTERS {
                    select = select.option(f, true);
                }
                h.spawn(select.selected(panel.filter).build(t));
                h.spawn(IconButton::new("comments-close", "close").small().build(t));
            });
            p.spawn((
                Name::new("comments-list"),
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollPosition::default(),
            ))
            .with_children(|l| {
                if threads.is_empty() {
                    l.spawn(t.text(
                        if panel.filter == 0 {
                            "No comments yet"
                        } else {
                            "Nothing here"
                        },
                        t.font_base,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ));
                }
                for (c, replies) in &threads {
                    l.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        ..default()
                    })
                    .with_children(|th| {
                        message(th, t, c, &clock, &my_id, &owner, false);
                        for r in replies {
                            message(th, t, r, &clock, &my_id, &owner, true);
                        }
                    });
                }
            });
            // The composer.
            p.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::top(Val::Px(1.0)),
                flex_shrink: 0.0,
                ..default()
            })
            .insert(BorderColor::all(t.panel_border))
            .with_children(|c| {
                for (label, name) in [
                    (doc_tag.map(|n| format!("Tagged: {n}")), "comments-untag"),
                    (
                        reply_name.map(|n| format!("Replying to {n}")),
                        "comments-unreply",
                    ),
                ] {
                    let Some(label) = label else { continue };
                    c.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(4.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn(t.text(label, t.font_sm, FontWeight::MEDIUM, t.muted_foreground));
                        r.spawn(IconButton::new(name, "close").small().build(t));
                    });
                }
                c.spawn(
                    TextInput::new("comment-text")
                        .placeholder("Add a comment (@name to mention)")
                        .build(t),
                );
                c.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|r| {
                    r.spawn(
                        Checkbox::new("comment-assign")
                            .label("Assign to the first @mention")
                            .build(t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| n.flex_grow = 1.0);
                    r.spawn(
                        Button::new("comment-add")
                            .label("Add")
                            .primary()
                            .small()
                            .build(t),
                    );
                });
            });
        })
        .id();
    world.entity_mut(parent).insert_children(0, &[root]);
}

/// One message: who and when, the text, a task's state, and its buttons.
fn message(
    p: &mut ChildSpawner,
    t: &Theme,
    c: &Comment,
    clock: &AppClock,
    me: &str,
    owner: &str,
    reply: bool,
) {
    let id = c.id;
    p.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(2.0),
        margin: UiRect::left(Val::Px(if reply { 26.0 } else { 0.0 })),
        ..default()
    })
    .with_children(|m| {
        m.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|h| {
            h.spawn(
                Avatar::new(format!("comment-avatar-{id}"), c.author_name.clone())
                    .size(20.0)
                    .color(super::presence::color_of(&c.author))
                    .build(t),
            );
            h.spawn(t.text(&c.author_name, t.font_base, FontWeight::BOLD, t.foreground));
            h.spawn(t.text(
                clock.format(c.time),
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ))
            .entry::<Node>()
            .and_modify(|mut n| n.flex_grow = 1.0);
            if c.is_task() {
                let may = c.author == me
                    || owner == me
                    || c.assignee.as_ref().is_some_and(|(a, _)| a == me);
                h.spawn(
                    Checkbox::new(format!("comment-resolve-{id}"))
                        .checked(c.resolved)
                        .disabled(!may)
                        .build(t),
                );
            }
            if !reply {
                h.spawn(
                    IconButton::new(format!("comment-reply-{id}"), "comments")
                        .small()
                        .tooltip("Reply")
                        .build(t),
                );
            }
            if c.author == me || owner == me {
                h.spawn(
                    IconButton::new(format!("comment-delete-{id}"), "delete")
                        .small()
                        .tooltip(if reply {
                            "Delete reply"
                        } else {
                            "Delete thread"
                        })
                        .build(t),
                );
            }
        });
        m.spawn((
            t.text(&c.text, t.font_base, FontWeight::NORMAL, t.foreground),
            Node {
                max_width: Val::Px(260.0),
                ..default()
            },
        ))
        .insert(TextLayout::default());
        if let Some((_, name)) = &c.assignee {
            let state = if c.resolved { "done" } else { "open" };
            m.spawn(t.text(
                format!("Task for {name} · {state}"),
                t.font_sm,
                FontWeight::MEDIUM,
                t.primary,
            ));
        }
    });
}

/// Whose `@mention` it is: someone in the room, a contact, the owner or this user.
fn who(world: &World, mention: &str) -> Option<(String, String)> {
    let mut people: Vec<(String, String)> = Vec::new();
    if let Some(p) = world.get_resource::<super::presence::People>() {
        people.extend(
            p.by_id
                .iter()
                .map(|(id, p)| (id.clone(), p.presence.name.clone())),
        );
    }
    if let Some(c) = world.get_resource::<ContactBook>() {
        people.extend(c.0.0.iter().map(|c| (c.id.clone(), c.name.clone())));
    }
    if let Some(s) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.meta.as_ref()?.shared.clone())
    {
        people.push((s.owner, s.owner_name));
    }
    people.push(me(world));
    people.into_iter().find(|(_, n)| matches_name(mention, n))
}

fn field(world: &mut World, name: &str) -> String {
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

/// **Add**: the comment as typed.
fn add(world: &mut World) {
    let text = field(world, "comment-text-field").trim().to_string();
    if text.is_empty() {
        return;
    }
    let panel = world.resource::<CommentsPanel>().clone();
    let (my_id, my_name) = me(world);
    let assign = {
        let mut q = world.query::<(&Name, &CheckboxState)>();
        q.iter(world)
            .any(|(n, s)| n.as_str() == "comment-assign" && s.checked)
    };
    let assignee = if assign {
        let first = cadrs_core::collab::comments::mentions(&text)
            .into_iter()
            .next();
        match first.and_then(|m| who(world, &m)) {
            Some(a) => Some(a),
            None => {
                crate::linked::toast(world, "Mention someone with @ to give them the task");
                return;
            }
        }
    } else {
        None
    };
    let now = world.resource::<AppClock>().now();
    let comment = Comment {
        id: uuid::Uuid::new_v4(),
        parent: panel.reply_to,
        author: my_id,
        author_name: my_name,
        time: now,
        text,
        tag: if panel.reply_to.is_some() {
            None
        } else {
            panel.tag
        },
        assignee,
        resolved: false,
        edited: None,
    };
    match post(world, CommentOp::Add(comment)) {
        Ok(()) => {
            crate::pcb::dialogs::set_text(world, "comment-text-field", "");
            let mut panel = world.resource_mut::<CommentsPanel>();
            panel.tag = None;
            panel.reply_to = None;
        }
        Err(why) => crate::linked::toast(world, why),
    }
}

fn on_activate(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(a.entity).map(|n| n.as_str().to_string()) else {
        return;
    };
    match name.as_str() {
        "rail-comments" => commands.queue(|w: &mut World| {
            let mut p = w.resource_mut::<CommentsPanel>();
            p.open = !p.open;
            let open = p.open;
            if open {
                w.resource_mut::<crate::history_panel::HistoryPanel>().open = false;
                w.resource_mut::<DocComments>().unread = false;
            }
        }),
        "comments-close" => commands.queue(|w: &mut World| {
            w.resource_mut::<CommentsPanel>().open = false;
        }),
        "comments-untag" => commands.queue(|w: &mut World| {
            w.resource_mut::<CommentsPanel>().tag = None;
        }),
        "comments-unreply" => commands.queue(|w: &mut World| {
            w.resource_mut::<CommentsPanel>().reply_to = None;
        }),
        "comment-add" => commands.queue(add),
        n => {
            let id = |p: &str| n.strip_prefix(p).and_then(|i| i.parse::<uuid::Uuid>().ok());
            if let Some(id) = id("comment-reply-") {
                commands.queue(move |w: &mut World| {
                    w.resource_mut::<CommentsPanel>().reply_to = Some(id);
                });
            } else if let Some(id) = id("comment-delete-") {
                commands.queue(move |w: &mut World| {
                    if let Err(why) = post(w, CommentOp::Delete { id }) {
                        crate::linked::toast(w, why);
                    }
                });
            }
        }
    }
}

/// A task's checkbox: resolved, or open again.
fn on_resolve(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Some(id) = q.get(ev.entity).ok().and_then(|n| {
        n.as_str()
            .strip_prefix("comment-resolve-")?
            .parse::<uuid::Uuid>()
            .ok()
    }) else {
        return;
    };
    let resolved = ev.checked;
    commands.queue(move |w: &mut World| {
        if let Err(why) = post(w, CommentOp::Resolve { id, resolved }) {
            crate::linked::toast(w, why);
        }
    });
}

fn on_filter(ev: On<SelectChange>, q: Query<&Name>, mut panel: ResMut<CommentsPanel>) {
    if q.get(ev.entity).map(Name::as_str) == Ok("comments-filter") {
        panel.filter = ev.index.min(FILTERS.len() - 1);
    }
}

/// The rail button: on while the panel is open, a dot while something is unread, and the
/// number of open tasks for this user.
#[derive(Component)]
struct RailDot;

#[allow(clippy::too_many_arguments)]
fn sync_rail(
    panel: Res<CommentsPanel>,
    dc: Res<DocComments>,
    iroh: Option<Res<Iroh>>,
    user: Res<UserProfile>,
    q: Query<(Entity, &Name, Has<cadrs_ui::style::Selected>)>,
    dots: Query<Entity, With<RailDot>>,
    theme: Res<Theme>,
    mut last: Local<Option<(bool, bool, usize)>>,
    mut commands: Commands,
) {
    let me = iroh.map_or(user.id.clone(), |i| i.id().to_string());
    let tasks = dc.comments.tasks_for(&me);
    let want = (panel.open, dc.unread || tasks > 0, tasks);
    let Some((button, _, selected)) = q.iter().find(|(_, n, _)| n.as_str() == "rail-comments")
    else {
        *last = None;
        return;
    };
    if *last == Some(want) && (dots.iter().next().is_some() == want.1) {
        return;
    }
    *last = Some(want);
    if panel.open && !selected {
        commands
            .entity(button)
            .try_insert(cadrs_ui::style::Selected);
    } else if !panel.open && selected {
        commands
            .entity(button)
            .try_remove::<cadrs_ui::style::Selected>();
    }
    for d in &dots {
        commands.entity(d).despawn();
    }
    if want.1 {
        let t = &*theme;
        commands.entity(button).with_child((
            RailDot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(2.0),
                right: Val::Px(2.0),
                min_width: Val::Px(8.0),
                height: Val::Px(if tasks > 0 { 12.0 } else { 8.0 }),
                padding: UiRect::horizontal(Val::Px(if tasks > 0 { 3.0 } else { 0.0 })),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(t.primary),
            Pickable::IGNORE,
            children![(
                t.text(
                    if tasks > 0 {
                        tasks.to_string()
                    } else {
                        String::new()
                    },
                    9.0,
                    FontWeight::BOLD,
                    Color::WHITE
                ),
                Pickable::IGNORE,
            )],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mention_names_a_first_name_or_a_whole_name() {
        assert!(matches_name("alex", "Alex Designer"));
        assert!(matches_name("AlexDesigner", "Alex Designer"));
        assert!(!matches_name("Designer", "Alex Designer"));
    }
}
