//! **Edit in context** and **managed in-context design** in the app (P3B.9 X15,
//! `managed-in-context-design.md` MC1–MC4; [`cadrs_core::assembly::context`]).
//!
//! - **Opening a context**: the instance menu's **Edit in context** opens the instance's Part
//!   Studio with a new context (a snapshot of the assembly around the instance), or **Edit in
//!   context ▸ <context> / New context** when the studio already has contexts in the assembly
//!   (MC2.14, MC4.1). A new context is *pending* until a feature references it (MC2.5,
//!   [`context::set_pending`]): leaving the studio without referencing it leaves nothing behind.
//! - **The active context** ([`ActiveContexts`], view state per Part Studio, not undoable) is
//!   the one whose ghost is drawn: translucent ([`ContextView::opacity`], the bar's slider,
//!   MC2.3) and pickable unless **Select transparent geometry** is off ([`sync_context_parts`]
//!   puts its parts into the [`PartCache`], not the Parts list).
//! - **The bar** at the top of the view (MCC1): "Context 1 of Assembly 1 · Cover <1>" (the
//!   primary instance), "Assembly changed" (an update is available, MC4.4) or "No primary
//!   instance" (MC3.4), **Update context**, the eye, **Go to assembly ▾** (Go to assembly, Insert
//!   and go to assembly, MC2.7, MC2.11), **Done** (edit outside of context), and under them
//!   Select transparent geometry and the transparency slider.
//! - **The Feature list's context row** (MC2.12, MC2.13, MCC2): "Assembly contexts" with a
//!   drop-down of the studio's contexts and **Edit outside of context**, and ⋯ with Rename…,
//!   Update context and Delete context.
//! - **Arrows** (MC2.6, MC2.15, MC3.1, MC4.4): a feature that references a context has an arrow
//!   on its row, yellow when it references the active context; an instance whose Part Studio has
//!   a context in the assembly has a grey one in the Instance list (as Onshape's), solid on the
//!   primary instance, dashed (outlined) on the others; a blue dot beside it when an update is
//!   available.
//!
//! Names: `context-bar`, `context-title`, `context-update`, `context-eye`, `context-go`
//! (menu `context-go-menu-assembly`, `context-go-menu-insert`), `context-done`,
//! `context-changed`, `context-no-primary`, `context-select-transparent`, `context-opacity`;
//! `contexts-row`, `context-select` (menu `context-select-menu-<n>`, `context-select-menu-outside`),
//! `context-more` (menu `context-more-menu-rename`, `-update`, `-delete`), `context-rename`;
//! `<row>-in-context` on feature and instance rows.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::context::{self, AddContext, ContextNo, ContextStatus, RemoveContext, RenameContext, SetContextHidden, StudioContext, UpdateContext};
use cadrs_core::{Document, DocumentId, ElementId, FeatureId, PartId, Solid};
use cadrs_ui::prelude::*;
use cadrs_ui::{CheckboxChange, MenuAction, NamePopup, NamePopupCancel, NamePopupCommit, SliderChange};

use crate::parts::{FaceBase, PartCache};
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct InContextPlugin;

impl Plugin for InContextPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveContexts>()
            .init_resource::<ContextView>()
            .init_resource::<Statuses>()
            .add_systems(
                Update,
                (open_contexts, leave_contexts, update_statuses, sync_context_bar, sync_contexts_row, sync_feature_arrows, sync_instance_arrows)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), reset)
            .add_observer(on_opacity)
            .add_observer(on_select_transparent)
            .add_observer(on_rename_commit)
            .add_observer(on_rename_cancel);
    }
}

/// The active context of each Part Studio (absent: editing outside of context). View state.
#[derive(Resource, Debug, Default, Clone)]
pub struct ActiveContexts(pub HashMap<ElementId, ContextNo>);

/// How the active context is drawn.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ContextView {
    /// The ghost's opacity, 0.05–0.95 (the bar's slider).
    pub opacity: f32,
    /// Select transparent geometry: the ghost's faces, edges and vertices can be picked.
    pub select_transparent: bool,
}

impl Default for ContextView {
    fn default() -> Self {
        Self { opacity: 0.45, select_transparent: true }
    }
}

/// Every context's status, worked out again when the document changes (and every few seconds:
/// a context's assembly may be in another document, MC5), with the names of the assemblies of
/// contexts made from another document, and the arrows of the active assembly's linked
/// instances (their Part Studios' contexts are in other documents).
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Statuses {
    pub status: HashMap<(ElementId, ContextNo), ContextStatus>,
    /// "Assembly 1 (Gripper)" for a context whose assembly is in another document.
    pub assembly_names: HashMap<(ElementId, ContextNo), String>,
    /// Linked instance → (primary of a context, an update is available, its Part Studio).
    pub linked: HashMap<InstanceId, (bool, bool, String)>,
}

impl Statuses {
    pub fn get(&self, studio: ElementId, id: ContextNo) -> ContextStatus {
        self.status.get(&(studio, id)).copied().unwrap_or(ContextStatus::Unknown)
    }
}

fn reset(mut active: ResMut<ActiveContexts>, mut statuses: ResMut<Statuses>, doc: Option<Res<ActiveDocument>>, mut commands: Commands) {
    commands.remove_resource::<super::linked_context::InContextSession>();
    if let Some(doc) = doc {
        for e in &doc.doc.elements {
            context::set_pending(doc.doc.id, e.id, None);
        }
    }
    active.0.clear();
    *statuses = Statuses::default();
}

/// A document just opened: its Part Studios start in the context they open in
/// ([`cadrs_core::Element::open_context`]).
fn open_contexts(doc: Option<Res<ActiveDocument>>, mut active: ResMut<ActiveContexts>, mut last: Local<Option<cadrs_core::DocumentId>>) {
    let Some(doc) = doc else { return };
    if *last == Some(doc.doc.id) && !doc.is_added() {
        return;
    }
    *last = Some(doc.doc.id);
    for e in &doc.doc.elements {
        if let Some(c) = e.open_context.filter(|c| e.context(*c).is_some()) {
            active.0.entry(e.id).or_insert(c);
        }
    }
}

/// The context `id` of the Part Studio `studio`: in the document, or pending.
pub fn find_context(doc: &Document, studio: ElementId, id: ContextNo) -> Option<StudioContext> {
    context::with_pending(doc, studio).into_iter().find(|c| c.id == id)
}

/// The active context of the Part Studio `studio`, if any.
pub fn active_context(doc: &Document, active: &ActiveContexts, studio: ElementId) -> Option<StudioContext> {
    find_context(doc, studio, *active.0.get(&studio)?)
}

/// Whether the context is only pending (not referenced yet, MC2.5).
pub fn is_pending(doc: &Document, studio: ElementId, id: ContextNo) -> bool {
    doc.element(studio).is_some_and(|e| e.context(id).is_none()) && context::pending_id(doc.id, studio) == Some(id)
}

/// Makes `id` the active context of `studio` (`None`: edit outside of context). Leaving a
/// pending context drops it.
pub fn set_active(world: &mut World, studio: ElementId, id: Option<ContextNo>) {
    let doc = &world.resource::<ActiveDocument>().doc;
    let doc_id = doc.id;
    let drop_pending = context::pending_id(doc_id, studio).is_some_and(|p| Some(p) != id && doc.element(studio).is_some_and(|e| e.context(p).is_none()));
    if drop_pending {
        context::set_pending(doc_id, studio, None);
    }
    let mut active = world.resource_mut::<ActiveContexts>();
    match id {
        Some(i) => active.0.insert(studio, i),
        None => active.0.remove(&studio),
    };
}

/// Which context Edit in context opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// A new one (pending until referenced).
    New,
    /// An existing one.
    Context(ContextNo),
}

/// **Edit in context**: the instance's Part Studio with a context of the assembly around it,
/// switched to.
pub fn edit_in_context(world: &mut World, assembly: ElementId, instance: InstanceId, which: Which) {
    let (studio, id) = {
        let doc = &world.resource::<ActiveDocument>().doc;
        let Some(studio) = context::studio_of(doc, assembly, instance) else { return };
        let id = match which {
            Which::Context(id) => {
                if find_context(doc, studio, id).is_none() {
                    return;
                }
                id
            }
            Which::New => {
                let next = context::with_pending(doc, studio).iter().map(|c| c.id + 1).max().unwrap_or(0);
                let id = next.max(doc.element(studio).map_or(0, |e| e.next_context_id()));
                match context::snapshot_as(doc, assembly, instance, id) {
                    Ok(c) => {
                        context::set_pending(doc.id, studio, Some(c));
                        id
                    }
                    Err(e) => {
                        warn!("edit in context: {e}");
                        return;
                    }
                }
            }
        };
        (studio, id)
    };
    world.resource_mut::<ActiveContexts>().0.insert(studio, id);
    world.resource_mut::<ActiveDocument>().set_active(studio);
    world.resource_mut::<crate::viewport::Selection>().0.clear();
}

/// Drops a pending context when its studio is left (MC2.5), and forgets the active context of
/// a context that is gone.
fn leave_contexts(doc: Option<Res<ActiveDocument>>, mut active: ResMut<ActiveContexts>, mut last: Local<Option<ElementId>>) {
    let Some(doc) = doc else { return };
    let now = doc.active;
    if *last != now {
        let d = doc.doc.id;
        if let Some(prev) = *last
            && let Some(p) = context::pending_id(d, prev)
        {
            // Not referenced: gone (and no longer active). Committed: the document has it.
            if doc.doc.element(prev).is_none_or(|e| e.context(p).is_none()) && active.0.get(&prev) == Some(&p) {
                active.0.remove(&prev);
            }
            context::set_pending(d, prev, None);
        }
        *last = now;
    }
    if !doc.is_changed() {
        return;
    }
    let gone: Vec<ElementId> = active.0.iter().filter(|(s, id)| find_context(&doc.doc, **s, **id).is_none()).map(|(s, _)| *s).collect();
    for s in gone {
        active.0.remove(&s);
    }
}

/// When the statuses were last worked out: the document, its undo and redo lengths, its tab, and
/// the time.
type StatusKey = (DocumentId, usize, usize, Option<ElementId>, f64);

fn update_statuses(world: &mut World, mut last: Local<Option<StatusKey>>) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let t = world.resource::<Time>().elapsed_secs_f64();
    let key = (doc.doc.id, doc.history.undo_len(), doc.history.redo_len(), doc.active);
    if last.as_ref().is_some_and(|l| (l.0, l.1, l.2, l.3) == key && t - l.4 < 3.0) {
        return;
    }
    *last = Some((key.0, key.1, key.2, key.3, t));
    let doc = doc.doc.clone();
    let mut now = Statuses::default();
    for el in &doc.elements {
        for c in &el.contexts {
            let st = match c.document.filter(|d| *d != doc.id) {
                None => context::status(&doc, el.id, c),
                Some(d) => match super::linked_context::document(world, d) {
                    Some(asm_doc) => {
                        if let Some(a) = asm_doc.element(c.assembly) {
                            now.assembly_names.insert((el.id, c.id), format!("{} ({})", a.name, asm_doc.name));
                        }
                        context::external_status(&asm_doc, c)
                    }
                    None => ContextStatus::Unknown,
                },
            };
            now.status.insert((el.id, c.id), st);
        }
    }
    // The active assembly's linked instances whose Part Studios have contexts made in it.
    let asm = world.resource::<ActiveDocument>().active_element().filter(|e| e.assembly_model().is_some()).map(|e| e.id);
    if let Some(asm) = asm {
        let instances: Vec<InstanceId> = doc.element(asm).and_then(|e| e.assembly_model()).map(|m| m.instances.iter().filter(|i| i.link.is_some()).map(|i| i.id).collect()).unwrap_or_default();
        for i in instances {
            let Some((d, studio)) = context::linked_studio_of(&doc, asm, i) else { continue };
            let Some(part_doc) = super::linked_context::document(world, d) else { continue };
            let Some(el) = part_doc.element(studio) else { continue };
            let ctxs: Vec<&StudioContext> = el.contexts.iter().filter(|c| c.document == Some(doc.id) && c.assembly == asm).collect();
            if ctxs.is_empty() {
                continue;
            }
            let primary = ctxs.iter().any(|c| c.instance == i);
            let stale = ctxs.iter().any(|c| context::external_status(&doc, c) == ContextStatus::OutOfDate);
            now.linked.insert(i, (primary, stale, el.name.clone()));
        }
    }
    if *world.resource::<Statuses>() != now {
        *world.resource_mut::<Statuses>() = now;
    }
}

// ---------------------------------------------------------------------------------------------
// The ghost

/// The active context's solids in the Part Studio shown now, by feature id (for sketch planes on
/// their faces, [`context_face_plane`]).
static SOLIDS: RwLock<Vec<(FeatureId, Arc<Solid>)>> = RwLock::new(Vec::new());

/// The colour context geometry is drawn in: a cool grey, so the studio's own parts stand out.
const CONTEXT_RGB: [f32; 3] = [196.0, 202.0, 210.0];

/// The part in the view of a context part's face, for a sketch plane on it (the view's face
/// picks name the context part's feature, which is not a feature of the studio).
pub fn context_face_plane(feature: FeatureId, face: cadrs_core::solid::FaceName) -> Option<cadrs_sketch::PlaneRef> {
    let solids = SOLIDS.read().ok()?;
    let (_, s) = solids.iter().find(|(f, _)| *f == feature)?;
    context::face_plane_on(s, feature, face)
}

/// The ghost being built or shown: what it was made from and its parts.
#[derive(Default)]
pub struct Ghost {
    /// The studio and context shown (worked out when something changed).
    ctx: Option<(ElementId, StudioContext)>,
    key: Option<(ElementId, String)>,
    pending: Vec<(ElementId, cadrs_core::rebuild::Pending)>,
    builds: HashMap<ElementId, Arc<cadrs_core::rebuild::Build>>,
    parts: Vec<cadrs_core::Part>,
}

/// Keeps the active Part Studio's active context's parts in the [`PartCache`] (after its
/// rebuild), tinted and (Select transparent geometry) pickable; none in an assembly, outside of
/// context, or with the context's eye closed. The source studios are rebuilt from the
/// context's frozen features on the rebuild thread.
pub fn sync_context_parts(doc: Option<Res<ActiveDocument>>, active: Res<ActiveContexts>, view: Res<ContextView>, mut cache: ResMut<PartCache>, mut ghost: Local<Ghost>) {
    let Some(doc) = doc else { return };
    let changed = doc.is_changed() || active.is_changed() || ghost.key.is_none();
    if changed {
        let studio = doc.active_element().filter(|e| e.assembly_model().is_none()).map(|e| e.id);
        let ctx = studio.and_then(|s| active_context(&doc.doc, &active, s)).filter(|c| !c.hidden);
        ghost.ctx = studio.zip(ctx);
    }
    let Some((studio, ctx)) = ghost.ctx.clone() else {
        if cache.parts.iter().any(|p| context::is_context(p.feature)) {
            cache.parts.retain(|p| !context::is_context(p.feature));
            cache.generation += 1;
        }
        if !cache.tints.is_empty() {
            cache.set_tints(HashMap::new());
        }
        if !cache.unpickable.is_empty() {
            cache.unpickable.clear();
        }
        if let Ok(mut s) = SOLIDS.write() {
            s.clear();
        }
        *ghost = Ghost::default();
        return;
    };
    if changed || ghost.key.is_none() {
        let key = (studio, format!("{:?}", (&ctx.id, &ctx.parts, &ctx.studios, &ctx.sources)));
        if ghost.key.as_ref() != Some(&key) {
            // Rebuild each source studio from the snapshot's features.
            let mut sources: Vec<ElementId> = ctx.parts.iter().map(|p| p.element).collect();
            sources.sort();
            sources.dedup();
            ghost.pending = sources
                .into_iter()
                .filter_map(|e| Some((e, cadrs_core::rebuild::request(context::source_features(&doc.doc, &ctx, e)?))))
                .collect();
            ghost.builds.clear();
            ghost.key = Some(key);
        }
    }
    if !ghost.pending.is_empty() {
        let mut still = Vec::new();
        for (e, mut p) in std::mem::take(&mut ghost.pending) {
            match p.poll() {
                Some(b) => {
                    ghost.builds.insert(e, b);
                }
                None => still.push((e, p)),
            }
        }
        ghost.pending = still;
        if !ghost.pending.is_empty() {
            return;
        }
        let builds = ghost.builds.clone();
        let (parts, _) = context::parts_of(&doc.doc, &ctx, |e, _| builds.get(&e).cloned());
        if let Ok(mut s) = SOLIDS.write() {
            *s = parts.iter().map(|p| (p.feature, p.solid.clone())).collect();
        }
        ghost.parts = parts;
    }
    let parts = &ghost.parts;
    let present: Vec<PartId> = cache.parts.iter().filter(|p| context::is_context(p.feature)).map(|p| p.id).collect();
    let want: Vec<PartId> = parts.iter().map(|p| p.id).collect();
    let fresh = present != want || cache.parts.iter().filter(|p| context::is_context(p.feature)).zip(parts).any(|(a, b)| !Arc::ptr_eq(&a.solid, &b.solid));
    if fresh {
        cache.parts.retain(|p| !context::is_context(p.feature));
        cache.parts.extend(parts.iter().cloned());
        cache.generation += 1;
    }
    let base = FaceBase { rgb: CONTEXT_RGB, alpha: view.opacity.clamp(0.05, 0.95) };
    let tints: HashMap<PartId, FaceBase> = want.iter().map(|p| (*p, base)).collect();
    cache.set_tints(tints);
    let unpickable: std::collections::HashSet<PartId> = if view.select_transparent { Default::default() } else { want.iter().copied().collect() };
    if cache.unpickable != unpickable {
        cache.unpickable = unpickable;
    }
}

// ---------------------------------------------------------------------------------------------
// The bar

/// The bar at the top of the view.
#[derive(Component)]
struct ContextBar(String);

/// What the bar shows.
#[derive(Debug, Clone, PartialEq)]
struct BarKey {
    studio: ElementId,
    id: ContextNo,
    label: String,
    assembly: String,
    primary: Option<String>,
    status: ContextStatus,
    pending: bool,
    hidden: bool,
    view: (u32, bool),
    /// The assembly is in another document (MC5).
    external: bool,
}

#[allow(clippy::too_many_arguments)]
fn sync_context_bar(
    doc: Option<Res<ActiveDocument>>,
    active: Res<ActiveContexts>,
    statuses: Res<Statuses>,
    view: Res<ContextView>,
    q: Query<(Entity, &ContextBar)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    if !(doc.is_changed() || active.is_changed() || statuses.is_changed() || view.is_changed() || q_area.iter().next().is_some_and(|_| q.is_empty())) {
        return;
    }
    let want = doc.active_element().filter(|e| e.assembly_model().is_none()).and_then(|el| {
        let ctx = active_context(&doc.doc, &active, el.id)?;
        let external = ctx.document.is_some_and(|d| d != doc.doc.id);
        let asm = if external {
            statuses.assembly_names.get(&(el.id, ctx.id)).cloned().unwrap_or_else(|| "the assembly".into())
        } else {
            doc.doc.element(ctx.assembly).map(|a| a.name.clone()).unwrap_or_else(|| "the assembly".into())
        };
        let pending = is_pending(&doc.doc, el.id, ctx.id);
        let status = if pending { context::status(&doc.doc, el.id, &ctx) } else { statuses.get(el.id, ctx.id) };
        Some(BarKey {
            studio: el.id,
            id: ctx.id,
            label: ctx.label(),
            assembly: asm,
            primary: if external { None } else { cadrs_core::assembly::managed_context::origin_name(&doc.doc, &ctx) },
            status,
            pending,
            hidden: ctx.hidden,
            view: ((view.opacity * 100.0).round() as u32, view.select_transparent),
            external,
        })
    });
    let key = want.as_ref().map(|w| format!("{w:?}")).unwrap_or_default();
    if let Some((e, k)) = q.iter().next() {
        if k.0 == key {
            return;
        }
        commands.entity(e).try_despawn();
    }
    let Some(w) = want else { return };
    let Some(area) = q_area.iter().next() else { return };
    // The bar is the in-context yellow (as the arrows of the active context's features), with
    // the light theme's text and controls on it in either theme.
    let t = Theme::light();
    let (studio, id, external) = (w.studio, w.id, w.external);
    // Centred in the viewport by a full-width row, kept clear of the view cube on both sides.
    let row = commands
        .spawn((
            Name::new("context-bar"),
            ContextBar(key),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(8.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                padding: UiRect::horizontal(Val::Px(170.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(AppState::Document),
        ))
        .id();
    let panel = commands
        .spawn((
            Name::new("context-bar-panel"),
            ChildOf(row),
            Node {
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(Val::Px(10.0), Val::Px(4.0), Val::Px(3.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                row_gap: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(BAR_YELLOW),
            BorderColor::all(ACTIVE_ARROW),
            BoxShadow::new(t.shadow, Val::Px(0.0), Val::Px(1.0), Val::Px(0.0), Val::Px(4.0)),
        ))
        .id();
    commands.entity(panel).with_children(|p| {
        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|b| {
            b.spawn((icon("assembly", 15.0, t.primary), Pickable::IGNORE));
            b.spawn((Name::new("context-title"), t.text(format!("{} of {}", w.label, w.assembly), 12.0, FontWeight::SEMIBOLD, t.foreground), Pickable::IGNORE));
            if let Some(n) = &w.primary {
                b.spawn((t.text(format!("· {n}"), 12.0, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE));
            }
            match w.status {
                ContextStatus::OutOfDate => {
                    b.spawn((
                        Name::new("context-changed"),
                        t.text("Assembly changed", 11.0, FontWeight::MEDIUM, Color::srgb_u8(0xc2, 0x6a, 0x00)),
                        Tooltip::new("The assembly has changed since this context was taken: Update context"),
                    ));
                }
                ContextStatus::NoPrimary => {
                    b.spawn((
                        Name::new("context-no-primary"),
                        t.text("No primary instance", 11.0, FontWeight::MEDIUM, t.feature_error),
                        Tooltip::new(context::NO_PRIMARY),
                    ));
                }
                _ => {}
            }
            let can_update = w.status != ContextStatus::NoPrimary;
            b.spawn(
                cadrs_ui::Button::new("context-update")
                    .label("Update context")
                    .icon("restore")
                    .small()
                    .ghost()
                    .disabled(!can_update)
                    .tooltip(if can_update { "Take the assembly as it is now; the features that reference it follow" } else { context::NO_PRIMARY })
                    .build(&t),
            )
            .observe(move |_: On<Activate>, mut commands: Commands| {
                commands.queue(move |world: &mut World| update_context(world, studio, id));
            });
            b.spawn(IconButton::new("context-eye", if w.hidden { "hidden" } else { "visible" }).icon_size(15.0).tooltip(if w.hidden { "Show context" } else { "Hide context" }).build(&t))
                .observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |world: &mut World| set_hidden(world, studio, id));
                });
            b.spawn(cadrs_ui::Button::new("context-go").label("Go to assembly").small().ghost().dropdown_caret().build(&t)).observe(
                move |a: On<Activate>, q: Query<(&ComputedNode, &UiGlobalTransform)>, theme: Res<Theme>, mut commands: Commands| {
                    let at = below(&q, a.entity);
                    let mut menu = Menu::new("context-go-menu").min_width(210.0).item(MenuItem::new("context-go-menu-assembly", "Go to assembly").icon("assembly"));
                    // MC5.5: the part's document is another than the assembly's; MC5.6: parts
                    // made in context are always in the assembly's document.
                    menu = if external {
                        menu.item(MenuItem::new("context-go-menu-version", "Create version and go to assembly").icon("versions"))
                    } else {
                        menu.item(MenuItem::new("context-go-menu-insert", "Insert and go to assembly").icon("file-import"))
                    };
                    let anchor = cadrs_ui::menu::open_context_menu(&mut commands, at, menu.build(&theme));
                    commands.entity(anchor).observe(move |ev: On<MenuAction>, mut commands: Commands| {
                        let item = ev.item.clone();
                        commands.queue(move |world: &mut World| match item.as_str() {
                            "context-go-menu-assembly" => go_to_assembly(world, studio, id),
                            "context-go-menu-insert" => super::managed_context::open_insert_dialog(world, studio),
                            "context-go-menu-version" => super::linked_context::go_back(world, true),
                            _ => {}
                        });
                    });
                },
            );
            b.spawn(cadrs_ui::Button::new("context-done").label("Done").small().secondary().tooltip("Edit outside of the context").build(&t))
                .observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |world: &mut World| set_active(world, studio, None));
                });
        });
        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), padding: UiRect::left(Val::Px(21.0)), ..default() }).with_children(|b| {
            b.spawn(cadrs_ui::Checkbox::new("context-select-transparent").label("Select transparent geometry").checked(w.view.1).height(18.0).build(&t));
            b.spawn((t.text("Transparency", 11.0, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE));
            // Left: see-through; right: solid.
            b.spawn(cadrs_ui::Slider::new("context-opacity").value((w.view.0 as f32 / 100.0 - 0.05) / 0.9).width(110.0).tooltip("How see-through the context is").build(&t));
        });
    });
    commands.entity(area).add_child(row);
}

/// The point under a node (a menu's anchor).
fn below(q: &Query<(&ComputedNode, &UiGlobalTransform)>, e: Entity) -> Vec2 {
    q.get(e).map_or(Vec2::ZERO, |(n, t)| {
        let s = n.inverse_scale_factor();
        let size = n.size() * s;
        t.translation * s + Vec2::new(-size.x / 2.0, size.y / 2.0 + 2.0)
    })
}

fn on_opacity(ev: On<SliderChange>, q: Query<&Name>, mut view: ResMut<ContextView>) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("context-opacity") {
        return;
    }
    let v = 0.05 + ev.value.clamp(0.0, 1.0) * 0.9;
    if (view.opacity - v).abs() > 1e-4 {
        view.opacity = v;
    }
}

fn on_select_transparent(ev: On<CheckboxChange>, q: Query<&Name>, mut view: ResMut<ContextView>) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("context-select-transparent") {
        return;
    }
    view.select_transparent = ev.checked;
}

/// The eye: a context in the document through its command (one undo step), a pending one
/// directly.
fn set_hidden(world: &mut World, studio: ElementId, id: ContextNo) {
    let doc = &world.resource::<ActiveDocument>().doc;
    if is_pending(doc, studio, id) {
        let d = doc.id;
        if let Some(mut p) = context::pending(d, studio) {
            p.hidden = !p.hidden;
            context::set_pending(d, studio, Some(p));
            world.resource_mut::<ActiveContexts>().set_changed();
        }
        return;
    }
    let Some(hidden) = doc.element(studio).and_then(|e| e.context(id)).map(|c| c.hidden) else { return };
    super::run(world, &SetContextHidden { studio, id, hidden: !hidden });
}

/// **Go to assembly**: the context's assembly.
pub fn go_to_assembly(world: &mut World, studio: ElementId, id: ContextNo) {
    let this = world.resource::<ActiveDocument>().doc.id;
    if let Some(ctx) = find_context(&world.resource::<ActiveDocument>().doc, studio, id)
        && ctx.document.is_some_and(|d| d != this)
    {
        // MC5.4: back to the assembly's document, without a version.
        if world.get_resource::<super::linked_context::InContextSession>().is_some_and(|s| s.part_doc == this) {
            super::linked_context::go_back(world, false);
        } else {
            super::linked_context::open_assembly(world, &ctx);
        }
        return;
    }
    let a = find_context(&world.resource::<ActiveDocument>().doc, studio, id).map(|c| c.assembly);
    if let Some(a) = a
        && world.resource::<ActiveDocument>().doc.element(a).is_some()
    {
        world.resource_mut::<ActiveDocument>().set_active(a);
    }
}

/// **Update context** (MC4.6, MC4.7): a new snapshot of the assembly, one undo step (a pending
/// context just takes it).
pub fn update_context(world: &mut World, studio: ElementId, id: ContextNo) {
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let doc = &doc;
    let Some(ctx) = find_context(doc, studio, id) else { return };
    // MC5.8.2: a context made from another document takes that assembly as it is now.
    let now = if ctx.document.is_some_and(|d| d != doc.id) { super::linked_context::resnapshot(world, &ctx).map_err(cadrs_core::CommandError::Invalid) } else { context::resnapshot(doc, studio, &ctx) };
    let now = match now {
        Ok(n) => n,
        Err(e) => {
            crate::linked::error_toast(world, e.to_string());
            return;
        }
    };
    if is_pending(doc, studio, id) {
        context::set_pending(doc.id, studio, Some(now));
        world.resource_mut::<ActiveContexts>().set_changed();
        return;
    }
    if now != ctx {
        super::run(world, &UpdateContext { studio, context: now });
    }
}

// ---------------------------------------------------------------------------------------------
// The Feature list's context row

#[derive(Component)]
struct ContextsRow(String);

fn sync_contexts_row(
    doc: Option<Res<ActiveDocument>>,
    active: Res<ActiveContexts>,
    statuses: Res<Statuses>,
    theme: Res<Theme>,
    q: Query<(Entity, &ContextsRow)>,
    q_pane: Query<(Entity, &Name)>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    if !(doc.is_changed() || active.is_changed() || statuses.is_changed() || q.is_empty()) {
        return;
    }
    let want = doc.active_element().filter(|e| e.assembly_model().is_none()).and_then(|el| {
        let all = context::with_pending(&doc.doc, el.id);
        if all.is_empty() {
            return None;
        }
        let act = active.0.get(&el.id).copied();
        let current = act.and_then(|a| all.iter().find(|c| c.id == a)).map(|c| c.label()).unwrap_or_else(|| "Edit outside of context".into());
        let items: Vec<(ContextNo, String, bool)> = all
            .iter()
            .map(|c| {
                let asm = statuses.assembly_names.get(&(el.id, c.id)).cloned().or_else(|| doc.doc.element(c.assembly).map(|a| a.name.clone())).unwrap_or_default();
                (c.id, if asm.is_empty() { c.label() } else { format!("{} · {asm}", c.label()) }, statuses.get(el.id, c.id) == ContextStatus::OutOfDate)
            })
            .collect();
        Some((el.id, act, items, current))
    });
    let key = want.as_ref().map(|w| format!("{w:?}")).unwrap_or_default();
    if let Some((e, k)) = q.iter().next() {
        if k.0 == key {
            return;
        }
        commands.entity(e).try_despawn();
    }
    let Some((studio, act, items, current)) = want else { return };
    let Some(pane) = q_pane.iter().find(|(_, n)| n.as_str() == "features-pane").map(|(e, _)| e) else { return };
    let t = theme.clone();
    let items_menu = items.clone();
    let row = commands
        .spawn((
            Name::new("contexts-row"),
            ContextsRow(key),
            Node {
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(4.0), Val::Px(4.0)),
                row_gap: Val::Px(2.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(t.border),
        ))
        .with_children(|r| {
            r.spawn((t.text("Assembly contexts", t.font_sm, FontWeight::SEMIBOLD, t.foreground), Pickable::IGNORE));
            r.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|b| {
                b.spawn(cadrs_ui::Button::new("context-select").label(current).small().outline().dropdown_caret().width(Val::Px(140.0)).tooltip("The context shown, or editing outside of context").build(&t))
                    .observe(move |a: On<Activate>, q: Query<(&ComputedNode, &UiGlobalTransform)>, theme: Res<Theme>, mut commands: Commands| {
                        let at = below(&q, a.entity);
                        let mut menu = Menu::new("context-select-menu").min_width(190.0);
                        for (i, label, stale) in &items_menu {
                            let mut item = MenuItem::new(format!("context-select-menu-{i}"), label.clone()).checked(act == Some(*i));
                            if *stale {
                                item = item.dot(STALE).tooltip("An update is available");
                            }
                            menu = menu.item(item);
                        }
                        menu = menu.separator().item(MenuItem::new("context-select-menu-outside", "Edit outside of context").checked(act.is_none()));
                        let anchor = cadrs_ui::menu::open_context_menu(&mut commands, at, menu.build(&theme));
                        commands.entity(anchor).observe(move |ev: On<MenuAction>, mut commands: Commands| {
                            let item = ev.item.clone();
                            commands.queue(move |world: &mut World| {
                                if item == "context-select-menu-outside" {
                                    set_active(world, studio, None);
                                } else if let Some(i) = item.strip_prefix("context-select-menu-").and_then(|n| n.parse::<ContextNo>().ok()) {
                                    set_active(world, studio, Some(i));
                                }
                            });
                        });
                    });
                b.spawn(IconButton::new("context-more", "more-horizontal").icon_size(15.0).tooltip("Rename, update or delete the context").build(&t)).observe(
                    move |a: On<Activate>, q: Query<(&ComputedNode, &UiGlobalTransform)>, theme: Res<Theme>, mut commands: Commands| {
                        let at = below(&q, a.entity);
                        let none = act.is_none();
                        let menu = Menu::new("context-more-menu")
                            .min_width(170.0)
                            .item(MenuItem::new("context-more-menu-rename", "Rename…").icon("edit").disabled(none))
                            .item(MenuItem::new("context-more-menu-update", "Update context").icon("restore").disabled(none))
                            .item(MenuItem::new("context-more-menu-delete", "Delete context").icon("delete").disabled(none));
                        let anchor = cadrs_ui::menu::open_context_menu(&mut commands, at, menu.build(&theme));
                        commands.entity(anchor).observe(move |ev: On<MenuAction>, mut commands: Commands| {
                            let (item, at) = (ev.item.clone(), at);
                            commands.queue(move |world: &mut World| {
                                let Some(id) = act else { return };
                                match item.as_str() {
                                    "context-more-menu-rename" => open_rename(world, studio, id, at),
                                    "context-more-menu-update" => update_context(world, studio, id),
                                    "context-more-menu-delete" => delete_context(world, studio, id),
                                    _ => {}
                                }
                            });
                        });
                    },
                );
            });
        })
        .id();
    commands.entity(pane).insert_children(0, &[row]);
}

/// The blue of "an update is available" (MC4.4).
const STALE: Color = Color::srgb(0.17, 0.49, 0.91);

/// The Rename popup's context.
#[derive(Resource, Debug, Clone, Copy)]
struct Renaming(ElementId, ContextNo);

fn open_rename(world: &mut World, studio: ElementId, id: ContextNo, at: Vec2) {
    let Some(ctx) = find_context(&world.resource::<ActiveDocument>().doc, studio, id) else { return };
    world.insert_resource(Renaming(studio, id));
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    NamePopup::new("context-rename", "Rename context", at).value(ctx.label()).spawn(&mut commands, &theme);
    world.flush();
}

fn on_rename_commit(ev: On<NamePopupCommit>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("context-rename") {
        return;
    }
    let (popup, name) = (ev.entity, ev.value.clone());
    commands.queue(move |world: &mut World| {
        if let Some(Renaming(studio, id)) = world.remove_resource::<Renaming>() {
            let d = world.resource::<ActiveDocument>().doc.id;
            if is_pending(&world.resource::<ActiveDocument>().doc, studio, id) {
                if let Some(mut p) = context::pending(d, studio) {
                    p.name = name.trim().to_string();
                    context::set_pending(d, studio, Some(p));
                    world.resource_mut::<ActiveContexts>().set_changed();
                }
            } else {
                super::run(world, &RenameContext { studio, id, name });
            }
        }
        if let Ok(e) = world.get_entity_mut(popup) {
            e.despawn();
        }
    });
}

fn on_rename_cancel(ev: On<NamePopupCancel>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("context-rename") {
        return;
    }
    let popup = ev.entity;
    commands.queue(move |world: &mut World| {
        world.remove_resource::<Renaming>();
        if let Ok(e) = world.get_entity_mut(popup) {
            e.despawn();
        }
    });
}

/// **Delete context** (MC2.13): one undo step; a pending one is just dropped.
pub fn delete_context(world: &mut World, studio: ElementId, id: ContextNo) {
    if is_pending(&world.resource::<ActiveDocument>().doc, studio, id) {
        let d = world.resource::<ActiveDocument>().doc.id;
        set_active(world, studio, None);
        context::set_pending(d, studio, None);
        return;
    }
    super::run(world, &RemoveContext { studio, id });
}

/// Adds a context to the document now (not waiting for a reference): for scripts and tests.
pub fn commit_pending(world: &mut World, studio: ElementId) {
    let Some(ctx) = context::pending(world.resource::<ActiveDocument>().doc.id, studio) else { return };
    if world.resource::<ActiveDocument>().doc.element(studio).is_some_and(|e| e.context(ctx.id).is_none()) {
        super::run(world, &AddContext { studio, context: ctx });
    }
}

// ---------------------------------------------------------------------------------------------
// Arrows

/// The yellow of the active context's arrows (MC2.15).
const ACTIVE_ARROW: Color = Color::srgb(0.91, 0.64, 0.0);
/// The context bar's background (MCC1).
const BAR_YELLOW: Color = Color::srgb(0.99, 0.80, 0.22);
const OTHER_ARROW: Color = Color::srgb(0.62, 0.62, 0.62);

/// The in-context arrow on a row: what it shows, and its node.
#[derive(Component, Debug, Clone, PartialEq)]
struct Arrow {
    state: ArrowState,
    node: Entity,
}

#[derive(Debug, Clone, PartialEq)]
struct ArrowState {
    /// Solid (a feature's, a primary instance's) or dashed (a secondary instance's).
    solid: bool,
    color: Color,
    stale: bool,
    tip: String,
}

fn arrow_node(t: &Theme, name: String, s: &ArrowState) -> impl Bundle {
    let _ = t;
    (
        Name::new(name),
        Node { width: Val::Px(16.0), height: Val::Px(14.0), flex_shrink: 0.0, align_items: AlignItems::Center, justify_content: JustifyContent::Center, margin: UiRect::left(Val::Px(2.0)), ..default() },
        Tooltip::new(s.tip.clone()),
        Pickable::default(),
        Children::spawn((
            // icon-rs's in-context arrows (solid: a feature's, a primary instance's; dashed: a
            // secondary instance's); before its release has them, its up arrow turned left, faint
            // for a secondary instance.
            SpawnWith({
                let (solid, color) = (s.solid, s.color);
                move |p: &mut ChildSpawner| {
                    let name = if solid { "in-context" } else { "in-context-secondary" };
                    if cadrs_ui::icon::has_icon(name) {
                        p.spawn(icon(name, 14.0, color));
                    } else {
                        p.spawn((icon("arrow-up", 14.0, if solid { color } else { color.with_alpha(0.5) }), bevy::ui::UiTransform { rotation: Rot2::degrees(-90.0), ..default() }));
                    }
                }
            }),
            SpawnWith({
                let stale = s.stale;
                move |p: &mut ChildSpawner| {
                    if stale {
                        p.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(0.0),
                                top: Val::Px(1.0),
                                width: Val::Px(5.0),
                                height: Val::Px(5.0),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(STALE),
                            Pickable::IGNORE,
                        ));
                    }
                }
            }),
        )),
    )
}

/// Puts `want` on the row `row` (named `name`), replacing what it had.
fn place_arrow(commands: &mut Commands, t: &Theme, row: Entity, name: &str, have: Option<&Arrow>, want: Option<ArrowState>) {
    if have.map(|a| &a.state) == want.as_ref() {
        return;
    }
    if let Some(a) = have {
        commands.entity(a.node).try_despawn();
        commands.entity(row).remove::<Arrow>();
    }
    if let Some(s) = want {
        let node = commands.spawn(arrow_node(t, format!("{name}-in-context"), &s)).id();
        commands.entity(row).add_child(node).insert(Arrow { state: s, node });
    }
}

fn sync_feature_arrows(
    doc: Option<Res<ActiveDocument>>,
    active: Res<ActiveContexts>,
    statuses: Res<Statuses>,
    theme: Res<Theme>,
    q: Query<(Entity, &crate::document::FeatureRow, &Name, Option<&Arrow>)>,
    added: Query<(), Added<crate::document::FeatureRow>>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    if !(doc.is_changed() || active.is_changed() || statuses.is_changed() || !added.is_empty()) {
        return;
    }
    let Some(el) = doc.active_element().filter(|e| e.assembly_model().is_none()) else { return };
    if el.contexts.is_empty() && q.iter().all(|(.., a)| a.is_none()) {
        return;
    }
    let act = active.0.get(&el.id).copied();
    for (row, fr, name, have) in &q {
        let want = el.feature(fr.0).and_then(|f| {
            let uses = context::feature_contexts(el, f);
            if uses.is_empty() {
                return None;
            }
            let on_active = act.is_some_and(|a| uses.contains(&a));
            let stale = uses.iter().any(|c| statuses.get(el.id, *c) == ContextStatus::OutOfDate);
            let names: Vec<String> = uses.iter().filter_map(|c| el.context(*c)).map(|c| c.label()).collect();
            let mut tip = format!("In context: {}", names.join(", "));
            if stale {
                tip.push_str(" · an update is available");
            }
            Some(ArrowState { solid: true, color: if on_active { ACTIVE_ARROW } else { OTHER_ARROW }, stale, tip })
        });
        place_arrow(&mut commands, &theme, row, name.as_str(), have, want);
    }
}

fn sync_instance_arrows(
    doc: Option<Res<ActiveDocument>>,
    statuses: Res<Statuses>,
    theme: Res<Theme>,
    q: Query<(Entity, &super::list::InstanceRow, &Name, Option<&Arrow>)>,
    added: Query<(), Added<super::list::InstanceRow>>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    if !(doc.is_changed() || statuses.is_changed() || !added.is_empty()) {
        return;
    }
    let Some(asm_el) = doc.active_element().filter(|e| e.assembly_model().is_some()) else { return };
    let assembly = asm_el.id;
    let by_studio = context::contexts_in(&doc.doc, assembly);
    if by_studio.is_empty() && statuses.linked.is_empty() && q.iter().all(|(.., a)| a.is_none()) {
        return;
    }
    for (row, ir, name, have) in &q {
        // MC5: a linked instance's Part Studio is in another document.
        if let Some((primary, stale, studio)) = statuses.linked.get(&ir.0) {
            let mut tip = if *primary { format!("Primary instance of an in-context Part Studio ({studio})") } else { format!("Secondary instance: {studio} has in-context references") };
            if *stale {
                tip.push_str(" · an update is available");
            }
            place_arrow(&mut commands, &theme, row, name.as_str(), have, Some(ArrowState { solid: *primary, color: theme.muted_foreground, stale: *stale, tip }));
            continue;
        }
        let want = context::studio_of(&doc.doc, assembly, ir.0).and_then(|studio| {
            let (_, ids) = by_studio.iter().find(|(s, _)| *s == studio)?;
            let el = doc.doc.element(studio)?;
            let ctxs: Vec<&StudioContext> = ids.iter().filter_map(|i| el.context(*i)).collect();
            let primary: Vec<String> = ctxs.iter().filter(|c| c.instance == ir.0).map(|c| c.label()).collect();
            let stale = ctxs.iter().any(|c| statuses.get(studio, c.id) == ContextStatus::OutOfDate);
            let solid = !primary.is_empty();
            let mut tip = if solid {
                format!("Primary instance of {} ({})", primary.join(", "), el.name)
            } else {
                format!("Secondary instance: {} has in-context references", el.name)
            };
            if stale {
                tip.push_str(" · an update is available");
            }
            Some(ArrowState { solid, color: theme.muted_foreground, stale, tip })
        });
        place_arrow(&mut commands, &theme, row, name.as_str(), have, want);
    }
}
