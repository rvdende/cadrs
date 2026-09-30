//! P3G.3: **Move to document** and a minimal **Tab manager** (`derived-and-linking-gaps.md`
//! DV4.1, ER7, ER8.2–ER8.6; essential tips T1.3, X1; drawings D2.10, X1; assemblies A21.13;
//! the model is [`cadrs_core::move_doc`]; `external-references/lesson-move-to-document.png`,
//! `ex2-step2.png`–`ex2-step6.png`).
//!
//! - **The dialog** ("Move to document", from a tab's menu, or the Tab manager for several
//!   tabs): the tabs **New document** | **Other documents**. New document has **Document name**
//!   (the first tab's name by default). Other documents is the linked documents' browser
//!   ([`crate::linked`]: search, My documents, Recently opened, Created by me, folders); picking a
//!   document shows it with **Version name**: a version of it is made with the moved tabs
//!   (ER7.2). Under both, "**N referenced tabs will be moved**" with **Show details** / **Hide
//!   details**: every tab that moves, the referenced ones with a checkbox (unticked: it stays
//!   here, and the moved tabs reference it at a new version of this document, ER7.3). The footer
//!   reads "Moving N tabs to <document>" (ER7.4) beside **Move** and **Cancel**.
//! - **Move** writes the target document (and its version) and then re-points this document's
//!   uses of the moved tabs at that version, removing the tabs: one undo step here; the other
//!   document and the versions stay (the toast says so). The toast offers **Open <document>**.
//! - **The Tab manager** (the tab bar's leftmost icon, ER7.7): a panel listing every tab with its
//!   icon; click selects (and opens) a tab, Ctrl+click adds or removes one, Shift+click selects a
//!   range; **Move to document…** (its button, or a row's right-click) moves the selection.
//!   P3E.2 moved it to [`crate::tab_manager`] and extended it (search, filters, folders,
//!   reordering).
//!
//! Names: `move-to-document-dialog`, `move-tabs`, `move-name`, `move-other-*` (the browser),
//! `move-target`, `move-target-change`, `move-version`, `move-summary`, `move-details`,
//! `move-tab-<k>`, `move-status`, `move-confirm`, `move-cancel` (the Tab manager's names are in
//! [`crate::tab_manager`]).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use cadrs_core::move_doc::{self, MoveTarget};
use cadrs_core::{DocumentId, ElementId, ElementKind};
use cadrs_ui::prelude::*;
use cadrs_ui::{Checkbox, CheckboxChange, DialogClose, DocumentRow, TabStrip, TabStripSelect};

use crate::history_panel::DocLog;
use crate::linked::{self, Owner};
use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

pub struct MoveDocumentPlugin;

impl Plugin for MoveDocumentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (read_fields, sync_sections, sync_other, sync_summary, sync_status).chain().run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
                commands.remove_resource::<MoveSession>();
            })
            .add_observer(on_tab)
            .add_observer(on_button)
            .add_observer(on_check)
            .add_observer(on_dialog_close);
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog

/// The open Move to document dialog.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct MoveSession {
    /// The tabs picked.
    pub tabs: Vec<ElementId>,
    /// The tabs they reference, and whether each moves too.
    pub referenced: Vec<(ElementId, bool)>,
    /// 0: New document, 1: Other documents.
    pub tab: usize,
    pub details: bool,
    pub browse: linked::Browse,
    /// The document picked under Other documents: its id, name and number of versions.
    pub target: Option<(DocumentId, String, usize)>,
    /// The Document name and Version name fields (read every frame).
    pub name: String,
    pub version_name: String,
}

impl MoveSession {
    /// Every tab that moves, in the document's order.
    pub fn moving(&self, doc: &cadrs_core::Document) -> Vec<ElementId> {
        doc.elements.iter().map(|e| e.id).filter(|e| self.tabs.contains(e) || self.referenced.iter().any(|(r, on)| r == e && *on)).collect()
    }

    /// The target's name as the footer says it.
    fn target_name(&self) -> String {
        if self.tab == 0 {
            if self.name.trim().is_empty() { "a new document".into() } else { self.name.trim().to_string() }
        } else {
            self.target.as_ref().map(|t| t.1.clone()).unwrap_or_else(|| "…".into())
        }
    }
}

#[derive(Component)]
struct MoveDialog;

/// The section only New document shows.
#[derive(Component)]
struct NewSection;

/// The Other documents section (rebuilt when the browser or the target changes).
#[derive(Component)]
struct OtherSection;

/// The referenced tabs' summary and details (rebuilt when they change).
#[derive(Component)]
struct SummarySection;

#[derive(Component)]
struct StatusText;

/// Opens the dialog for `tabs` (one tab's menu, or the Tab manager's selection).
pub fn open_move_dialog(world: &mut World, tabs: Vec<ElementId>) {
    open_move_dialog_named(world, tabs, None);
}

/// [`open_move_dialog`] with the new document's name (P3E.2: a folder moves under its name);
/// `None`: the first tab's name.
pub fn open_move_dialog_named(world: &mut World, tabs: Vec<ElementId>, name: Option<String>) {
    if crate::linked_session::refuse(world) {
        return;
    }
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let tabs: Vec<ElementId> = doc.elements.iter().map(|e| e.id).filter(|e| tabs.contains(e)).collect();
    let Some(first) = tabs.first().and_then(|e| doc.elements.iter().find(|x| x.id == *e)) else { return };
    close_dialog(world);
    let referenced = move_doc::referenced_tabs(&doc, &tabs).into_iter().map(|e| (e, true)).collect();
    let name = name.unwrap_or_else(|| first.name.clone());
    world.insert_resource(MoveSession { tabs, referenced, tab: 0, details: false, browse: linked::Browse::default(), target: None, name: name.clone(), version_name: String::new() });
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("move-to-document-dialog")
            .width(520.0)
            .title("Move to document")
            .title_font(theme.font_lg, FontWeight::MEDIUM)
            .body(move |b| {
                let t = &tb;
                b.spawn(TabStrip::new("move-tabs").tab("New document").tab("Other documents").selected(0).equal().build(t));
                b.spawn((NewSection, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), margin: UiRect::top(Val::Px(10.0)), ..default() })).with_children(|n| {
                    n.spawn(t.text("Document name", t.font_base, FontWeight::BOLD, t.foreground));
                    n.spawn(TextInput::new("move-name").value(name).select_all_on_focus().autofocus().width(Val::Percent(100.0)).height(30.0).build(t));
                });
                b.spawn((OtherSection, Name::new("move-other"), Node { flex_direction: FlexDirection::Column, display: Display::None, margin: UiRect::top(Val::Px(6.0)), ..default() }));
                b.spawn((SummarySection, Name::new("move-summary"), Node { flex_direction: FlexDirection::Column, margin: UiRect::top(Val::Px(12.0)), ..default() }));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    StatusText,
                    Name::new("move-status"),
                    t.text("", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                    Node { flex_grow: 1.0, align_self: AlignSelf::Center, ..default() },
                ));
                f.spawn(cadrs_ui::Button::new("move-confirm").label("Move").primary().build(t));
                f.spawn(cadrs_ui::Button::new("move-cancel").label("Cancel").build(t));
            })
            .build(&theme),
        MoveDialog,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// A tab's menu → Move to document…: the Tab manager's selection when it holds this tab and
/// others, else this tab.
pub fn open_for_tab(world: &mut World, tab: ElementId) {
    let sel = world.resource::<crate::tab_manager::TabManager>().selected.clone();
    let tabs = if sel.len() > 1 && sel.contains(&tab) { sel } else { vec![tab] };
    open_move_dialog(world, tabs);
}

fn close_dialog(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<MoveDialog>>();
    let ds: Vec<Entity> = q.iter(world).collect();
    for d in ds {
        world.trigger(DialogClose { entity: d });
    }
    world.remove_resource::<MoveSession>();
}

fn on_dialog_close(ev: On<DialogClose>, q: Query<(), With<MoveDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.remove_resource::<MoveSession>();
    }
}

fn text_of(q: &Query<(&Name, &bevy::text::EditableText)>, name: &str) -> Option<String> {
    q.iter().find(|(n, _)| n.as_str() == name).map(|(_, t)| t.value().to_string())
}

/// The fields and the browser's search, into the session (without rebuilding anything).
fn read_fields(q: Query<(&Name, &bevy::text::EditableText)>, session: Option<ResMut<MoveSession>>) {
    let Some(mut s) = session else { return };
    let s = s.bypass_change_detection();
    if let Some(v) = text_of(&q, "move-name-field") {
        s.name = v;
    }
    if let Some(v) = text_of(&q, "move-version-field") {
        s.version_name = v;
    }
    if let Some(v) = text_of(&q, "move-other-search-field")
        && s.browse.search != v
    {
        s.browse.search = v;
    }
}

fn sync_sections(session: Option<Res<MoveSession>>, mut q_new: Query<&mut Node, (With<NewSection>, Without<OtherSection>)>, mut q_other: Query<&mut Node, (With<OtherSection>, Without<NewSection>)>) {
    let Some(s) = session else { return };
    let (a, b) = if s.tab == 0 { (Display::Flex, Display::None) } else { (Display::None, Display::Flex) };
    for mut n in &mut q_new {
        if n.display != a {
            n.display = a;
        }
    }
    for mut n in &mut q_other {
        if n.display != b {
            n.display = b;
        }
    }
}

type OtherKey = (linked::Browse, Option<(DocumentId, String, usize)>);

/// Other documents: the browser, or the document picked with its Version name.
/// A new read of the library or its thumbnails landing rebuilds the list too.
fn sync_other(world: &mut World, mut last: Local<Option<(OtherKey, u64)>>) {
    let Some(s) = world.get_resource::<MoveSession>() else {
        *last = None;
        return;
    };
    let key: OtherKey = (s.browse.clone(), s.target.clone());
    let generation = world.resource::<linked::LibrarySnapshot>().generation;
    let mut q = world.query_filtered::<(Entity, Ref<OtherSection>), ()>();
    let Some((panel, added)) = q.iter(world).next().map(|(e, r)| (e, r.is_added())) else { return };
    if !added && last.as_ref().is_some_and(|(k, g)| k == &key && *g == generation) {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let (browse, target) = key.clone();
    let only_search = !added
        && target.is_none()
        && last.as_ref().is_some_and(|((b, t), g)| t.is_none() && b.location == browse.location && (b.search != browse.search || *g != generation));
    if added {
        world.resource_mut::<linked::LibrarySnapshot>().invalidate();
    }
    let list = if target.is_none() { linked::browser_list(world, &browse) } else { linked::BrowserList::default() };
    let thumbs = linked::browser_thumbs(world, &list.rows);
    let mut qn = world.query::<(Entity, &Name)>();
    let list_node = qn.iter(world).find(|(_, n)| n.as_str() == "move-other-list").map(|(e, _)| e);
    if let (true, Some(node)) = (only_search, list_node) {
        world.entity_mut(node).despawn();
        let mut commands = world.commands();
        commands.entity(panel).with_children(|p| linked::spawn_browser_list(p, &theme, Owner::Move, &browse, &list, &thumbs));
        world.flush();
        *last = Some((key, generation));
        return;
    }
    let target_thumb = target.as_ref().and_then(|(d, n, _)| linked::thumbs_for(world, &[linked::DocListRow { id: *d, name: n.clone(), subtitle: String::new(), versions: true, count: 1 }]).remove(d));
    world.entity_mut(panel).despawn_children();
    let mut commands = world.commands();
    commands.entity(panel).with_children(|p| {
        let t = &theme;
        match &target {
            None => linked::spawn_browser(p, t, Owner::Move, &browse, &list, &thumbs),
            Some((_, name, versions)) => {
                p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|r| {
                    r.spawn(
                        DocumentRow::new("move-target", name.clone(), if *versions == 0 { "No versions".to_string() } else { format!("{versions} version{}", if *versions == 1 { "" } else { "s" }) })
                            .thumbnail(target_thumb.clone())
                            .selected(true)
                            .build(t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| n.flex_grow = 1.0);
                    r.spawn(cadrs_ui::Button::new("move-target-change").label("Change").small().build(t));
                });
                p.spawn((t.text("Version name", t.font_base, FontWeight::BOLD, t.foreground), Node { margin: UiRect::top(Val::Px(8.0)), ..default() }));
                p.spawn(TextInput::new("move-version").value(format!("V{}", versions + 1)).select_all_on_focus().width(Val::Percent(100.0)).height(30.0).build(t));
                p.spawn((
                    t.text(format!("The tabs are added to {name}, and a version of it is made with them: this document then references that version."), t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                    Node { margin: UiRect::top(Val::Px(6.0)), max_width: Val::Px(480.0), ..default() },
                ))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            }
        }
    });
    world.flush();
    *last = Some((key, generation));
}

/// Other documents → a document picked (from [`crate::linked`]'s browser).
pub fn pick_target(world: &mut World, document: DocumentId) {
    let store = world.resource::<DocumentStore>().0.clone();
    let state = cadrs_core::external::link_state(&store, document);
    if let Some(m) = state.message() {
        linked::error_toast(world, m);
        return;
    }
    let name = store.load(document).map(|f| f.document.name).unwrap_or_default();
    let versions = linked::resolver(world).0.versions(document).len();
    if let Some(mut s) = world.get_resource_mut::<MoveSession>() {
        s.version_name = format!("V{}", versions + 1);
        s.target = Some((document, name, versions));
    }
}

/// The tabs the details list: the picked ones, then the referenced ones, each in tab order.
fn listed_tabs(doc: &cadrs_core::Document, s: &MoveSession) -> Vec<ElementId> {
    let order = doc.elements.iter().map(|e| e.id);
    order.clone().filter(|e| s.tabs.contains(e)).chain(order.filter(|e| s.referenced.iter().any(|(r, _)| r == e))).collect()
}

type SummaryKey = (Vec<ElementId>, Vec<(ElementId, bool)>, bool);

fn tab_icon(el: &cadrs_core::Element) -> &'static str {
    match el.kind {
        ElementKind::PartStudio { .. } => "part-studio",
        ElementKind::Assembly => "assembly",
        ElementKind::Drawing(_) => "details",
        ElementKind::PcbStudio(_) => "pcb-studio",
        ElementKind::Render(_) => "render-studio",
    }
}

/// "N referenced tabs will be moved", Show / Hide details, and the tabs.
fn sync_summary(world: &mut World, mut last: Local<Option<SummaryKey>>) {
    let Some(s) = world.get_resource::<MoveSession>().cloned() else {
        *last = None;
        return;
    };
    let key: SummaryKey = (s.tabs.clone(), s.referenced.clone(), s.details);
    let mut q = world.query_filtered::<(Entity, Ref<SummarySection>), ()>();
    let Some((panel, added)) = q.iter(world).next().map(|(e, r)| (e, r.is_added())) else { return };
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let theme = world.resource::<Theme>().clone();
    world.entity_mut(panel).despawn_children();
    let mut commands = world.commands();
    commands.entity(panel).with_children(|p| {
        let t = &theme;
        let n = s.moving(&doc).len();
        if s.referenced.is_empty() {
            p.spawn((Name::new("move-summary-text"), t.text(if n == 1 { "1 tab will be moved".to_string() } else { format!("{n} tabs will be moved") }, t.font_base, FontWeight::BOLD, t.foreground)));
            return;
        }
        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
            let text = if n == 1 { "1 tab will be moved".to_string() } else { format!("{n} referenced tabs will be moved") };
            r.spawn((Name::new("move-summary-text"), t.text(text, t.font_base, FontWeight::BOLD, t.foreground)));
            r.spawn(cadrs_ui::Button::new("move-details").label(if s.details { "Hide details" } else { "Show details" }).link().small().build(t));
        });
        if !s.details {
            return;
        }
        // The tabs picked first, then those they reference (`lesson-move-to-document.png`).
        let listed = listed_tabs(&doc, &s);
        for (k, el) in listed.iter().filter_map(|e| doc.elements.iter().find(|x| x.id == *e)).enumerate() {
            let k = k + 1;
            let picked = s.tabs.contains(&el.id);
            let r = s.referenced.iter().find(|(e, _)| *e == el.id);
            p.spawn((Name::new(format!("move-tab-row-{k}")), Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), height: Val::Px(24.0), margin: UiRect::left(Val::Px(2.0)), ..default() }))
                .with_children(|row| {
                    let tip = if picked { "Picked to move" } else { "Referenced by a tab that moves. Untick it to leave it here: the moved tabs then reference it at a new version of this document." };
                    row.spawn((Checkbox::new(format!("move-tab-{k}")).checked(picked || r.is_some_and(|x| x.1)).disabled(picked).build(t), Tooltip::new(tip)));
                    row.spawn((cadrs_ui::icon::icon(tab_icon(el), 15.0, t.tool_foreground), Pickable::IGNORE));
                    row.spawn((t.text(el.name.clone(), t.font_sm, FontWeight::NORMAL, t.foreground), Pickable::IGNORE));
                });
        }
        if s.referenced.iter().any(|(_, on)| !on) {
            p.spawn((
                Name::new("move-left-behind"),
                t.text("Tabs left here are referenced from the new location at a version of this document, made when you move.", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                Node { margin: UiRect::top(Val::Px(4.0)), max_width: Val::Px(480.0), ..default() },
            ))
            .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
        }
    });
    world.flush();
    *last = Some(key);
}

/// "Moving 2 tabs to Pneumatic Piston", and Move enabled when there is a target.
fn sync_status(session: Option<Res<MoveSession>>, doc: Option<Res<ActiveDocument>>, mut q: Query<&mut Text, With<StatusText>>, q_btn: Query<(Entity, &Name, Has<InteractionDisabled>)>, mut commands: Commands) {
    let (Some(s), Some(doc)) = (session, doc) else { return };
    let n = s.moving(&doc.doc).len();
    let want = format!("Moving {n} tab{} to {}", if n == 1 { "" } else { "s" }, s.target_name());
    for mut t in &mut q {
        if t.0 != want {
            t.0 = want.clone();
        }
    }
    let ok = s.tab == 0 || s.target.is_some();
    for (e, name, disabled) in &q_btn {
        if name.as_str() == "move-confirm" && disabled == ok {
            if ok {
                commands.entity(e).try_remove::<InteractionDisabled>();
            } else {
                commands.entity(e).try_insert(InteractionDisabled);
            }
        }
    }
}

fn on_tab(ev: On<TabStripSelect>, q: Query<&Name>, session: Option<ResMut<MoveSession>>) {
    let (Some(mut s), Ok(n)) = (session, q.get(ev.entity)) else { return };
    if n.as_str() == "move-tabs" && s.tab != ev.index {
        s.tab = ev.index;
    }
}

fn on_check(ev: On<CheckboxChange>, q: Query<&Name>, session: Option<ResMut<MoveSession>>, doc: Option<Res<ActiveDocument>>) {
    let (Some(mut s), Some(doc), Ok(n)) = (session, doc, q.get(ev.entity)) else { return };
    let Some(k) = n.as_str().strip_prefix("move-tab-").and_then(|k| k.parse::<usize>().ok()) else { return };
    let listed = listed_tabs(&doc.doc, &s);
    let Some(e) = listed.get(k - 1).copied() else { return };
    if let Some(r) = s.referenced.iter_mut().find(|(x, _)| *x == e) {
        r.1 = ev.checked;
    }
}

fn on_button(a: On<Activate>, q: Query<&Name>, session: Option<Res<MoveSession>>, mut commands: Commands) {
    let Ok(n) = q.get(a.entity) else { return };
    match n.as_str() {
        "move-confirm" if session.is_some() => commands.queue(commit),
        "move-cancel" if session.is_some() => commands.queue(close_dialog),
        "move-details" => commands.queue(|w: &mut World| {
            if let Some(mut s) = w.get_resource_mut::<MoveSession>() {
                s.details = !s.details;
            }
        }),
        "move-target-change" => commands.queue(|w: &mut World| {
            if let Some(mut s) = w.get_resource_mut::<MoveSession>() {
                s.target = None;
            }
        }),
        _ => {}
    }
}

/// Move: the target is written, then this document's command runs (one undo step).
fn commit(world: &mut World) {
    let Some(s) = world.get_resource::<MoveSession>().cloned() else { return };
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let target = if s.tab == 0 {
        MoveTarget::New { name: s.name.clone() }
    } else {
        let Some((d, _, _)) = s.target.clone() else { return };
        MoveTarget::Existing { document: d, version_name: s.version_name.clone() }
    };
    let tabs = s.moving(&doc);
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let mut log = world.resource_mut::<DocLog>().log.take();
    let before = log.as_ref().map(|l| l.versions().len());
    let result = move_doc::move_tabs(&store, &doc, &mut log, &tabs, &target, now, &user);
    {
        let mut l = world.resource_mut::<DocLog>();
        l.log = log;
        if l.log.as_ref().map(|l| l.versions().len()) != before {
            l.generation += 1;
        }
    }
    let out = match result {
        Ok(o) => o,
        Err(e) => {
            linked::error_toast(world, e.to_string());
            return;
        }
    };
    close_dialog(world);
    if out.created {
        write_thumbnail(world, out.target);
    }
    linked::resolver(world).0.forget(out.target);
    linked::resolver(world).0.forget(doc.id);
    let moving_active = world.resource::<ActiveDocument>().active.is_some_and(|a| tabs.contains(&a));
    if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&out.command) {
        linked::error_toast(world, format!("{} was written, but this document couldn't be changed: {e}", out.target_name));
        return;
    }
    // The open tab moved: show the tab that used it (the Hexapod, `ex2-step4.png`).
    if moving_active {
        let d = &world.resource::<ActiveDocument>().doc;
        let user_of = cadrs_core::link_update::uses(d).into_iter().find(|u| tabs.contains(&u.reference.element)).map(|u| u.site.tab());
        if let Some(t) = out.command.changes.first().map(|c| c.site.tab()).or(user_of).or(d.elements.first().map(|e| e.id)) {
            world.resource_mut::<ActiveDocument>().set_active(t);
        }
    }
    world.resource_mut::<crate::tab_manager::TabManager>().selected.clear();
    world.resource_mut::<crate::linked::LinkStatus>().invalidate();
    // P3G.4 (P3G.3 carried): "now references" only when uses were re-pointed; references to
    // an older version of a moved tab (ER7.6) are counted and left to the Reference manager.
    let mut text = format!("{}.", out.summary());
    if !out.command.changes.is_empty() {
        text.push_str(&format!(" This document now references its {}.", out.version_name));
    }
    {
        let d = &world.resource::<ActiveDocument>().doc;
        let left: Vec<cadrs_core::link_update::RefUse> = cadrs_core::link_update::uses(d)
            .into_iter()
            .filter(|u| tabs.contains(&u.reference.element) && u.reference.document_or(d.id) == d.id && u.is_version())
            .collect();
        if let Some(first) = left.first() {
            let v = d.linked_element(first.source).map(|l| l.version_name.clone()).unwrap_or_default();
            let n = left.len();
            let s = if n == 1 { "reference still points" } else { "references still point" };
            text.push_str(&format!(" {n} {s} at {v} here; update them in the Reference manager."));
        }
    }
    if let Some((_, v)) = &out.source_version {
        text.push_str(&format!(" {v} of this document was made for the tabs left here."));
    }
    text.push_str(" Undo puts the tabs back here; the other document and the versions stay.");
    let theme = world.resource::<Theme>().clone();
    let target = out.target;
    let label = format!("Open {}", out.target_name);
    let mut commands = world.commands();
    let toast = cadrs_ui::show_notification(&mut commands, &theme, cadrs_ui::Notification::info(text).name("move-toast").seconds(10.0));
    let link = cadrs_ui::toast_action(&mut commands, &theme, toast, "move-open-target", label);
    commands.entity(link).observe(move |_: On<Activate>, mut commands: Commands| {
        commands.queue(move |w: &mut World| {
            cadrs_ui::close_toasts(w);
            open_document(w, target);
        });
    });
    world.flush();
}

/// A thumbnail of the new document (its first Part Studio's parts), for the documents page.
fn write_thumbnail(world: &mut World, id: DocumentId) {
    let store = world.resource::<DocumentStore>().0.clone();
    let Ok(file) = store.load(id) else { return };
    let Some(el) = file.document.elements.iter().find(|e| matches!(e.kind, ElementKind::PartStudio { .. })) else { return };
    let build = cadrs_core::rebuild::build(el.features());
    let list: Vec<(&cadrs_core::Solid, [u8; 3])> = build.parts.iter().map(|p| (&*p.solid, cadrs_core::appearance::part_appearance(p, el.part_props()).rgb)).collect();
    if list.is_empty() {
        return;
    }
    let img = cadrs_core::assembly::thumb::render(&list, 96);
    let _ = store.write_thumbnail(id, &img);
    world.resource_mut::<linked::DocThumbs>().forget(id);
}

/// Opens document `id` in place of the open one (saved first).
pub fn open_document(world: &mut World, id: DocumentId) {
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    if let Some(s) = world.remove_resource::<crate::linked_session::LinkedSession>() {
        world.insert_resource(s.back);
    }
    if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = d.save_if_changed(&store, now, &user)
    {
        warn!("cannot save: {e}");
    }
    let mut file = match store.load(id) {
        Ok(f) => f,
        Err(e) => {
            linked::error_toast(world, e.to_string());
            return;
        }
    };
    file.meta.last_opened = Some(now);
    let _ = store.save(&file.document, &file.meta);
    crate::reference_manager::close(world);
    world.insert_resource(ActiveDocument::stored(file.document, file.meta));
    world.resource_mut::<crate::linked::LinkStatus>().invalidate();
    if *world.resource::<State<AppState>>().get() != AppState::Document {
        world.resource_mut::<NextState<AppState>>().set(AppState::Document);
    }
}

// ---------------------------------------------------------------------------------------------
// Scenario set-up

/// `move-doc <arg>` (scenarios): `setup` stores ER Ex2's stand-in document ("Exercise: Move to
/// Document": Hexapod, Pneumatic Piston, Piston Assembly, Topplate, Baseplate) and opens it on
/// the Hexapod; `plates` adds "Plates library" (a document with one version) to move tabs into;
/// `open <name>` opens the library's document of that name.
pub fn script(world: &mut World, arg: &str) {
    use cadrs_core::samples::piston as ps;
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let mut words = arg.split_whitespace();
    match words.next() {
        Some("setup") => {
            let doc = match ps::move_to_document() {
                Ok(d) => d,
                Err(e) => {
                    warn!("move-doc setup: {e}");
                    return;
                }
            };
            let meta = cadrs_core::DocumentMeta::new(&user, now - 3_600);
            if let Err(e) = store.create(&doc, &meta) {
                warn!("move-doc setup: {e}");
            }
            let mut d = ActiveDocument::stored(doc, meta);
            d.set_active(ps::HEXAPOD);
            world.insert_resource(d);
        }
        Some("plates") => {
            let mut d = cadrs_core::Document::new("Plates library");
            d.id = DocumentId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_0077);
            let meta = cadrs_core::DocumentMeta::new(&user, now - 7_200);
            if let Err(e) = store.create(&d, &meta) {
                warn!("move-doc plates: {e}");
            }
            let mut log = cadrs_core::history_log::HistoryLog::start(&d, now - 7_200, &user);
            log.create_version("V1", "", now - 7_100, &user);
            let _ = log.save(&store);
        }
        // Drawing 1: a Front view of the whole Pneumatic Piston (D2.10: a drawing tab to move).
        Some("drawing") => {
            crate::script::add_front_view_drawing_of(world, ps::PISTON_STUDIO, None, [140.0, 110.0]);
        }
        // ER7.6: V1 of the open document, and the Hexapod's six pistons pointed at it (as
        // Change to version… does), one undo step.
        Some("version-ref") => {
            let doc = world.resource::<ActiveDocument>().doc.clone();
            let v = {
                let mut l = world.resource_mut::<DocLog>();
                let Some(log) = l.log.as_mut() else { return };
                if log.head() != &doc {
                    log.record(&doc, cadrs_core::history_log::Origin::Command("Move to document".into()), now, &user);
                }
                let v = log.create_version("", "", now, &user);
                let _ = log.save(&store);
                l.generation += 1;
                v
            };
            let log = world.resource::<DocLog>().log.clone();
            let mut changes = Vec::new();
            {
                let mut r = linked::resolver(world);
                for k in 0..6 {
                    let site = cadrs_core::link_update::RefSite::Instance { element: ps::HEXAPOD, instance: ps::piston(k) };
                    let Some(u) = cadrs_core::link_update::use_at(&doc, site) else { continue };
                    match cadrs_core::link_update::change_for(&mut r.0, &doc, log.as_ref(), &u, cadrs_core::link_update::Target::Version(v)) {
                        Ok(Some(c)) => changes.push(c),
                        Ok(None) => {}
                        Err(e) => warn!("move-doc version-ref: {e}"),
                    }
                }
            }
            if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&cadrs_core::link_update::UpdateReferences { changes, label: "Change to version".into() }) {
                warn!("move-doc version-ref: {e}");
            }
        }
        Some("open") => {
            let name: Vec<&str> = words.collect();
            let name = name.join(" ");
            let (lib, _) = store.list();
            if let Some(e) = lib.entries.iter().find(|e| e.name == name) {
                open_document(world, e.id);
            }
        }
        _ => warn!("unknown move-doc command {arg:?}"),
    }
}
