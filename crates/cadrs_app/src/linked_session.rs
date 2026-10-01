//! P3G.3: **Open linked document** (`derived-and-linking-gaps.md` DV1.9, ER6.7; the model is
//! [`cadrs_core::external`]).
//!
//! A linked reference (an instance of another document, or of this document at a version; a
//! drawing's linked source) can be opened: its source document **at the referenced version**,
//! in this window, with the source tab active and the referenced part selected. The session is
//! **read-only** ([`crate::ActiveDocument::read_only`]): every command, undo and redo is refused
//! with a toast, the toolbar, the tab bar's **+** and Create version are disabled, a feature
//! can't be opened for editing, nothing is saved and its history is only read. The top bar says
//! which version is shown instead of "Main", and a blue banner at the top of the graphics area
//! says so too, with **Back to <document>**, which returns to the document it was opened from
//! exactly as it was (its tab, its undo history).
//!
//! Entry points: the instance menu's **Open linked document** (also for several instances: the
//! first), a linked icon's right-click menu, a tab's menu (the tab's first linked reference;
//! "Open linked document ▸" names each document when there are several), the Sheets pane's
//! reference menu, and the Reference manager's rows.
//!
//! P3H.7 (PCB7.10, PCB11.2): the banner's **Edit Main** opens the source document's workspace
//! ("Main") for editing, saved as usual, with its own banner "Editing Main of <document>" and
//! **Back to <document>**, which saves it and returns to the document the linked session was
//! opened from exactly as it was (its tab, its undo history), where the reference then shows its
//! update badge once a version was made ([`EditingSource`]).
//!
//! Names: `linked-session-banner`, `linked-session-banner-text`, `linked-session-main`,
//! `linked-session-back`; `linked-edit-banner`, `linked-edit-banner-text`, `linked-edit-back`.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use bevy::ui_widgets::Button as WidgetButton;
use cadrs_core::assembly::InstanceSource;
use cadrs_core::external::{LinkState, RefAt};
use cadrs_core::link_update::{RefSite, RefUse};
use cadrs_core::{Document, DocumentId, ElementId, PartId};
use cadrs_ui::prelude::*;

use crate::history_panel::DocLog;
use crate::linked::{self, LinkStatus};
use crate::viewport::{Pick, Selection, ViewportArea, ViewportView};
use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

pub struct LinkedSessionPlugin;

impl Plugin for LinkedSessionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_banner, sync_edit_banner, sync_branch_label, disable_editing, sync_veil, notify_refused, apply_pending_selection, apply_restore_selection).run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), (leave, |mut commands: Commands| commands.remove_resource::<EditingSource>()))
        .add_observer(on_back);
    }
}

/// The document a linked session was opened from, and what it shows.
#[derive(Resource, Clone)]
pub struct LinkedSession {
    /// The document to go back to, as it was (tab, undo history).
    pub back: ActiveDocument,
    /// The document shown and its version.
    pub document: DocumentId,
    pub name: String,
    pub version: cadrs_core::history_log::VersionId,
    pub version_name: String,
    /// The part to select once its tab shows.
    pending: Option<(ElementId, PartId)>,
    /// P3G.4 (P3G.3 carried): the selection of the document it was opened from, given back.
    selection: Vec<Pick>,
}

impl LinkedSession {
    /// "V1 of Block source".
    pub fn label(&self) -> String {
        format!("{} of {}", self.version_name, self.name)
    }
}

/// True while a linked document is open read-only.
pub fn is_read_only(world: &World) -> bool {
    world.get_resource::<ActiveDocument>().is_some_and(|d| d.read_only.is_some())
}

/// Refuses an edit that doesn't go through a command (opening a feature to edit it): true, with
/// the toast, while read-only.
pub fn refuse(world: &mut World) -> bool {
    if !is_read_only(world) {
        return false;
    }
    world.resource_mut::<ActiveDocument>().refused += 1;
    true
}

/// Whether a use can be opened: a version reference whose source can be reached (DV1.7: not a
/// trashed, deleted or unreadable document).
pub fn can_open(doc: &Document, status: &LinkStatus, u: &RefUse) -> bool {
    matches!(u.reference.at, RefAt::Version(_)) && (u.reference.document_or(doc.id) == doc.id || status.state(u.reference.document_or(doc.id)) == LinkState::Ok)
}

/// The part an instance use references (selected when its source opens).
fn part_of(doc: &Document, u: &RefUse) -> Option<PartId> {
    let RefSite::Instance { element, instance } = u.site else { return None };
    match doc.element(element)?.assembly_model()?.instance(instance)?.source {
        InstanceSource::Part { part, .. } => Some(part),
        _ => None,
    }
}

/// Opens the source of `u` at its version, read-only (DV1.9).
pub fn open_use(world: &mut World, u: RefUse) {
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let RefAt::Version(v) = u.reference.at else {
        linked::toast(world, "It references the workspace: switch to its tab instead");
        return;
    };
    let d = u.reference.document_or(doc.id);
    let part = part_of(&doc, &u);
    let copy = doc.linked_element(u.source).cloned();
    let (at, name, version_name) = if d == doc.id {
        // This document at a version (ER3): from its own history.
        let log = world.resource::<DocLog>().log.clone();
        let Some(at) = log.as_ref().and_then(|l| l.document_at_version(v)) else {
            linked::error_toast(world, "The version can't be read");
            return;
        };
        let vn = log.as_ref().and_then(|l| l.version(v).map(|x| x.name().to_string())).unwrap_or_default();
        (at, doc.name.clone(), vn)
    } else {
        let mut r = linked::resolver(world);
        let state = r.0.state(d);
        if let Some(m) = state.message() {
            linked::error_toast(world, m);
            return;
        }
        let at = match r.0.document_at(d, v) {
            Ok(at) => (*at).clone(),
            Err(e) => {
                linked::error_toast(world, e.to_string());
                return;
            }
        };
        let vn = r.0.versions(d).into_iter().find(|x| x.id() == v).map(|x| x.name().to_string()).or_else(|| copy.as_ref().map(|c| c.version_name.clone())).unwrap_or_default();
        let name = at.name.clone();
        (at, name, vn)
    };
    let element = u.reference.element;
    if at.elements.iter().all(|e| e.id != element) {
        linked::error_toast(world, format!("{} has no such tab at {version_name}", name));
        return;
    }
    // Save what is open, and keep it to go back to (the first document, when a linked
    // document is opened from one open read-only).
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let store = world.resource::<DocumentStore>().0.clone();
    let previous = world.remove_resource::<LinkedSession>();
    let selection = match &previous {
        Some(s) => s.selection.clone(),
        None => world.resource::<Selection>().0.clone(),
    };
    let back = match previous {
        Some(s) => s.back,
        None => {
            let mut cur = world.resource::<ActiveDocument>().clone();
            if let Err(e) = cur.save_if_changed(&store, now, &user) {
                warn!("cannot save before opening a linked document: {e}");
            }
            cur
        }
    };
    crate::reference_manager::close(world);
    let label = format!("{version_name} of {name}");
    let mut open = ActiveDocument::new(at);
    open.read_only = Some(label);
    open.set_active(element);
    world.insert_resource(open);
    world.insert_resource(LinkedSession { back, document: d, name, version: v, version_name, pending: part.map(|p| (element, p)), selection });
    world.resource_mut::<LinkStatus>().invalidate();
}

/// The linked documents tab `tab` references at a version, one use of each (document, version)
/// with its label "Block source (V1)" (the tab menu's Open linked document).
pub fn tab_sources(doc: &Document, tab: ElementId) -> Vec<(String, RefUse)> {
    let mut out: Vec<(String, RefUse)> = Vec::new();
    for u in cadrs_core::link_update::uses(doc).into_iter().filter(|u| u.site.tab() == tab) {
        let key = (u.reference.document_or(doc.id), u.reference.at);
        if out.iter().any(|(_, x)| (x.reference.document_or(doc.id), x.reference.at) == key) {
            continue;
        }
        let label = match doc.linked_element(u.source) {
            Some(l) if key.0 == doc.id => format!("{} ({})", doc.name, l.version_name),
            Some(l) => format!("{} ({})", l.document_name, l.version_name),
            None => doc.name.clone(),
        };
        out.push((label, u));
    }
    out
}

/// Opens the source of the use at `site`.
pub fn open_site(world: &mut World, site: RefSite) {
    let Some(u) = cadrs_core::link_update::use_at(&world.resource::<ActiveDocument>().doc, site) else { return };
    open_use(world, u);
}

/// Back to the document the session was opened from.
pub fn back(world: &mut World) {
    let Some(s) = world.remove_resource::<LinkedSession>() else { return };
    crate::reference_manager::close(world);
    let element = s.back.active;
    world.insert_resource(s.back);
    // Given back once its tab shows (switching tabs clears the selection).
    world.insert_resource(RestoreSelection { element, picks: s.selection });
    world.resource_mut::<LinkStatus>().invalidate();
}

fn leave(mut commands: Commands) {
    commands.remove_resource::<LinkedSession>();
}

fn on_back(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    match q.get(a.entity).map(|n| n.as_str()) {
        Ok("linked-session-back") => commands.queue(back),
        Ok("linked-session-main") => commands.queue(edit_main),
        Ok("linked-edit-back") => commands.queue(back_from_main),
        _ => {}
    }
}

/// P3H.7: the source document of a linked session opened at its workspace for editing (**Edit
/// Main**), and the document to go back to, as it was.
#[derive(Resource, Clone)]
pub struct EditingSource {
    pub back: ActiveDocument,
    /// The document being edited.
    pub document: DocumentId,
    pub name: String,
}

/// Edit Main: the linked session's document at its workspace, editable (saved as usual), on the
/// tab that was shown.
pub fn edit_main(world: &mut World) {
    let Some(s) = world.remove_resource::<LinkedSession>() else { return };
    if s.document == s.back.doc.id {
        world.insert_resource(s);
        return;
    }
    let element = world.resource::<ActiveDocument>().active;
    let store = world.resource::<DocumentStore>().0.clone();
    let mut file = match store.load(s.document) {
        Ok(f) => f,
        Err(e) => {
            world.insert_resource(s);
            linked::error_toast(world, e.to_string());
            return;
        }
    };
    if let Some(m) = LinkState::message(cadrs_core::external::link_state(&store, s.document)) {
        world.insert_resource(s);
        linked::error_toast(world, m);
        return;
    }
    crate::reference_manager::close(world);
    let now = world.resource::<AppClock>().now();
    file.meta.last_opened = Some(now);
    let _ = store.save(&file.document, &file.meta);
    let name = file.document.name.clone();
    let mut open = ActiveDocument::stored(file.document, file.meta);
    if let Some(e) = element {
        open.set_active(e);
    }
    world.insert_resource(open);
    world.insert_resource(EditingSource { back: s.back, document: s.document, name });
    world.resource_mut::<LinkStatus>().invalidate();
}

/// Back from Edit Main: the edited document saved, the document the linked session was opened
/// from shown again as it was.
pub fn back_from_main(world: &mut World) {
    let Some(e) = world.remove_resource::<EditingSource>() else { return };
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
        && let Err(err) = d.save_if_changed(&store, now, &user)
    {
        warn!("cannot save {}: {err}", e.name);
    }
    crate::reference_manager::close(world);
    // The edited document's versions are read again (the badges).
    linked::resolver(world).0.forget(e.document);
    world.insert_resource(e.back);
    world.resource_mut::<LinkStatus>().invalidate();
}

/// The banner.
#[derive(Component)]
struct Banner(String);

fn sync_banner(session: Option<Res<LinkedSession>>, doc: Option<Res<ActiveDocument>>, q: Query<(Entity, &Banner)>, qa: Query<Entity, With<ViewportArea>>, theme: Res<Theme>, mut commands: Commands) {
    let want = match (&session, &doc) {
        (Some(s), Some(d)) if d.read_only.is_some() => Some(format!("{}|{}", s.label(), s.back.doc.name)),
        _ => None,
    };
    let have = q.iter().next().map(|(e, b)| (e, b.0.clone()));
    if have.as_ref().map(|h| &h.1) == want.as_ref() {
        return;
    }
    if let Some((e, _)) = have {
        commands.entity(e).despawn();
    }
    let (Some(s), Some(key)) = (session, want) else { return };
    let Some(area) = qa.iter().next() else { return };
    let t = &*theme;
    let blue = Color::srgb_u8(0xe3, 0xef, 0xfc);
    // A full-width strip at the bottom of the graphics area, the banner centred in it.
    let banner = commands
        .spawn((
            Banner(key),
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), bottom: Val::Px(14.0), justify_content: JustifyContent::Center, ..default() },
            Pickable::IGNORE,
            GlobalZIndex(40),
        ))
        .with_children(|w| {
            w.spawn((
                Name::new("linked-session-banner"),
                Node {
                    max_width: Val::Px(760.0),
                    min_height: Val::Px(34.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(blue),
                BorderColor::all(linked::UPDATE_BLUE),
            ))
            .with_children(|b| {
                b.spawn((cadrs_ui::icon::icon("info-filled", 16.0, linked::UPDATE_BLUE), cadrs_ui::icon::SolidTint));
                b.spawn((
                    Name::new("linked-session-banner-text"),
                    t.text(format!("Viewing {} (read-only). Edits are turned off.", s.label()), t.font_sm, FontWeight::MEDIUM, t.foreground),
                ))
                .insert(TextLayout::no_wrap());
                // Edit Main: another document's workspace (a version of this document goes back
                // to it with Back).
                if s.document != s.back.doc.id {
                    b.spawn(cadrs_ui::Button::new("linked-session-main").label("Edit Main").icon("edit").tooltip(format!("Open {}'s workspace (Main) to edit it", s.name)).small().build(t));
                }
                b.spawn(cadrs_ui::Button::new("linked-session-back").label(format!("Back to {}", s.back.doc.name)).icon("chevron-left").primary().small().build(t));
            });
        })
        .id();
    commands.entity(area).add_child(banner);
}

/// The Edit Main banner.
#[derive(Component)]
struct EditBanner(String);

fn sync_edit_banner(editing: Option<Res<EditingSource>>, doc: Option<Res<ActiveDocument>>, q: Query<(Entity, &EditBanner)>, qa: Query<Entity, With<ViewportArea>>, theme: Res<Theme>, mut commands: Commands) {
    // Opening another document (the documents page, Open linked document) ends it.
    if let (Some(e), Some(d)) = (&editing, &doc)
        && (d.doc.id != e.document || d.read_only.is_some())
    {
        commands.remove_resource::<EditingSource>();
    }
    let want = match (&editing, &doc) {
        (Some(e), Some(d)) if d.doc.id == e.document && d.read_only.is_none() => Some(format!("{}|{}", e.name, e.back.doc.name)),
        _ => None,
    };
    let have = q.iter().next().map(|(e, b)| (e, b.0.clone()));
    if have.as_ref().map(|h| &h.1) == want.as_ref() {
        return;
    }
    if let Some((e, _)) = have {
        commands.entity(e).despawn();
    }
    let (Some(e), Some(key)) = (editing, want) else { return };
    let Some(area) = qa.iter().next() else { return };
    let t = &*theme;
    let banner = commands
        .spawn((
            EditBanner(key),
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), bottom: Val::Px(14.0), justify_content: JustifyContent::Center, ..default() },
            Pickable::IGNORE,
            GlobalZIndex(40),
        ))
        .with_children(|w| {
            w.spawn((
                Name::new("linked-edit-banner"),
                Node {
                    margin: UiRect::horizontal(Val::Px(16.0)),
                    min_height: Val::Px(34.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(t.background),
                BorderColor::all(linked::UPDATE_BLUE),
            ))
            .with_children(|b| {
                b.spawn((Node { flex_shrink: 0.0, ..default() }, Pickable::IGNORE)).with_children(|i| {
                    i.spawn((cadrs_ui::icon::icon("edit", 16.0, linked::UPDATE_BLUE), cadrs_ui::icon::SolidTint));
                });
                // The banner is as wide as what it holds; the text wraps past 640 px (long
                // document names), so the Back button always stays inside the border.
                b.spawn((
                    Name::new("linked-edit-banner-text"),
                    t.text(format!("Editing Main of {}. Create a version, then update the reference in {}.", e.name, e.back.doc.name), t.font_sm, FontWeight::MEDIUM, t.foreground),
                ))
                .insert((Node { flex_shrink: 0.0, max_width: Val::Px(640.0), ..default() }, TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary)));
                b.spawn((Name::new("linked-edit-back-slot"), Node { flex_shrink: 0.0, ..default() })).with_children(|s| {
                    s.spawn(cadrs_ui::Button::new("linked-edit-back").label(format!("Back to {}", e.back.doc.name)).icon("chevron-left").primary().small().build(t));
                });
            });
        })
        .id();
    commands.entity(area).add_child(banner);
}

/// The top bar says which version is shown ("V1") instead of "Main".
fn sync_branch_label(session: Option<Res<LinkedSession>>, doc: Option<Res<ActiveDocument>>, mut q: Query<(&Name, &mut Text)>) {
    let want = match (&session, &doc) {
        (Some(s), Some(d)) if d.read_only.is_some() => s.version_name.clone(),
        _ => "Main".to_string(),
    };
    for (n, mut t) in &mut q {
        if n.as_str() == "branch-label" && t.0 != want {
            t.0 = want.clone();
        }
    }
}

/// While read-only, the toolbar's tools, the tab bar's "+" and Create version are disabled
/// (and enabled again after).
/// A button, whether it is disabled, and whether read-only disabled it.
type ButtonState<'a> = (Entity, &'a Name, Has<InteractionDisabled>, Has<ReadOnlyDisabled>);

fn disable_editing(
    doc: Option<Res<ActiveDocument>>,
    q: Query<ButtonState, With<WidgetButton>>,
    q_parent: Query<&ChildOf>,
    q_name: Query<&Name>,
    mut commands: Commands,
) {
    let ro = doc.is_some_and(|d| d.read_only.is_some());
    for (e, name, disabled, ours) in &q {
        if !ro {
            if ours {
                commands.entity(e).try_remove::<(ReadOnlyDisabled, InteractionDisabled)>();
            }
            continue;
        }
        if disabled {
            continue;
        }
        let named = matches!(name.as_str(), "insert-tab" | "document-create-version");
        let in_toolbar = q_parent.iter_ancestors(e).any(|a| q_name.get(a).is_ok_and(|n| n.as_str() == "toolbar"));
        if named || in_toolbar {
            commands.entity(e).try_insert((ReadOnlyDisabled, InteractionDisabled));
        }
    }
}

/// Disabled because the document is read-only (so it is enabled again after).
#[derive(Component)]
struct ReadOnlyDisabled;

/// A pale veil over the toolbar while read-only (its tools are disabled; coloured icons don't
/// grey by themselves).
#[derive(Component)]
struct ToolbarVeil;

fn sync_veil(doc: Option<Res<ActiveDocument>>, q_veil: Query<(Entity, &ChildOf), With<ToolbarVeil>>, q_bar: Query<(Entity, &Name)>, mut commands: Commands) {
    let ro = doc.is_some_and(|d| d.read_only.is_some());
    let bar = q_bar.iter().find(|(_, n)| n.as_str() == "toolbar").map(|(e, _)| e);
    let have: Vec<(Entity, Entity)> = q_veil.iter().map(|(e, p)| (e, p.parent())).collect();
    let ok = ro && bar.is_some() && have.len() == 1 && have.first().map(|h| h.1) == bar;
    if ok || (!ro && have.is_empty()) {
        return;
    }
    for (e, _) in have {
        commands.entity(e).try_despawn();
    }
    if let (true, Some(bar)) = (ro, bar) {
        let veil = commands
            .spawn((
                Name::new("read-only-toolbar-veil"),
                ToolbarVeil,
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), ..default() },
                BackgroundColor(Color::srgba(0.97, 0.97, 0.97, 0.62)),
                Tooltip::new("This version is open read-only: its tools are turned off"),
            ))
            .id();
        commands.entity(bar).add_child(veil);
    }
}

/// A toast for every refused edit.
fn notify_refused(doc: Option<Res<ActiveDocument>>, session: Option<Res<LinkedSession>>, mut last: Local<u32>, theme: Res<Theme>, mut commands: Commands) {
    let Some(doc) = doc else { return };
    if doc.refused == *last {
        return;
    }
    let grew = doc.refused > *last;
    *last = doc.refused;
    if !grew || doc.read_only.is_none() {
        return;
    }
    let text = match session {
        Some(s) => format!("{} is open read-only: it can't be edited. Go back to {} to edit.", s.label(), s.back.doc.name),
        None => crate::READ_ONLY.to_string(),
    };
    cadrs_ui::show_notification(&mut commands, &theme, cadrs_ui::Notification::info(text).name("read-only-toast").seconds(5.0));
}

/// The selection to give back after Back, once `element` shows.
#[derive(Resource)]
struct RestoreSelection {
    element: Option<ElementId>,
    picks: Vec<Pick>,
}

fn apply_restore_selection(restore: Option<Res<RestoreSelection>>, view: Res<ViewportView>, mut selection: ResMut<Selection>, mut commands: Commands) {
    let Some(r) = restore else { return };
    if r.element.is_some() && view.element != r.element {
        return;
    }
    selection.0 = r.picks.clone();
    commands.remove_resource::<RestoreSelection>();
}

/// Selects the referenced part once its tab shows (switching tabs clears the selection).
fn apply_pending_selection(session: Option<ResMut<LinkedSession>>, view: Res<ViewportView>, mut selection: ResMut<Selection>) {
    let Some(mut s) = session else { return };
    let Some((element, part)) = s.pending else { return };
    if view.element != Some(element) {
        return;
    }
    s.pending = None;
    selection.0 = vec![Pick::Part(part)];
}
