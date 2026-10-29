//! The Assembly tab (P3B.1, `intro-to-assemblies.md` A1–A5): instances of Part Studio parts,
//! drawn, picked and measured like parts.
//!
//! - **Instances in the view** ([`update_assembly_parts`]): each instance is its source part
//!   from the source Part Studio's current rebuild (the rebuild worker's cache, so a studio that
//!   didn't change costs nothing), moved by its pose, put into the [`PartCache`] as a part
//!   whose id stands for the instance ([`cadrs_core::assembly::InstanceId::part_id`]). So the
//!   shading, edges, hover, face/edge/vertex picking, the selection tint and the Mass properties
//!   panel all work on instances unchanged, and a studio edit shows in the assembly at once.
//! - **Instances list** ([`list`]), **context menus** ([`menu`]), the **Insert dialog**
//!   ([`insert`]) and the **triad manipulator** ([`triad`]).
//! - P3B.2: **mates** ([`mate_dialog`], picking implicit connectors, [`connectors`]), **Group**
//!   ([`group_dialog`]), the **Mate Features list** with Reset / Apply limit position and the
//!   instances' DOF ([`mates_list`]), and the **instance clipboard** ([`InstanceClipboard`]).
//! - P3B.3: the other six mates, **dragging** under the mates ([`drag`]), **animation**
//!   ([`animate`]), mates **in the view** with J and H ([`mate_display`]), and the mate list's
//!   tooltips, eyes and menu.
//! - P3B.4: **subassemblies** (instances of other Assembly tabs, drawn as their parts
//!   `PartId(instance, k)`; the solver works on the flattened model,
//!   [`cadrs_core::assembly::structure::solver_model`], whose instances are the parts, the
//!   [`occurrence_of`] a pick), **folders** in both lists and the list **filter** ([`list`],
//!   [`folders`]), and the Insert dialog's **Assemblies** tab ([`insert`]).
//! - P3B.5: **standard content** ([`standard`]): the Insert dialog's Standard content tab
//!   (batch and single placement with Fastened mates, A flips, stacking, auto-size), the list's
//!   standard content icon, Select instances with same configuration and **Edit standard
//!   content instance** (bulk edit).
//! - P3B.6: the **Bill of Materials** panel ([`bom_panel`]) and the instances' **Properties…**
//!   ([`crate::properties_dialog`]).
//! - P3B.8: **rigid Part Studio instances** (the Insert dialog's Insert as rigid; the list's
//!   ▸ shows their parts; Edit… in [`studio_edit`]), **Named positions** ([`named_positions`]),
//!   **Exploded views** ([`explode`]: a view state over the placements, [`AssemblyParts::exploded`]),
//!   **Replicate** ([`replicate_dialog`]) and the **Items** group ([`items`]); the assembly's own
//!   mate connectors are rows of the Mate Features list, and a part's Part Studio connectors are
//!   listed under its row.
//! - P3H.6: **box select** in the view ([`box_select`]; window left to right, crossing right to
//!   left), for grouping a generated PCB assembly's components.
//! - Keys: **Delete** / **Backspace** removes selected instances, **I** opens Insert,
//!   **M** a Fastened mate, **Y** hides the instance under the pointer,
//!   **Shift+Y** shows every hidden instance (A4.5), **J** shows or hides all mates, **H** toggles
//!   show mates mode (A14), **Ctrl+C** / **Ctrl+V** copy the selected instance and paste it at
//!   the pointer (A15.13).

pub mod animate;
pub mod bom_panel;
pub mod box_select;
pub mod connector_tool;
pub mod connectors;
pub mod drag;
pub mod explode;
pub mod folders;
pub mod group_dialog;
pub mod in_context;
pub mod insert;
pub mod insert_linked;
pub mod interference;
pub mod items;
pub mod linked_context;
pub mod list;
pub mod managed_context;
pub mod mate_dialog;
pub mod mate_display;
pub mod mates_list;
pub mod menu;
pub mod named_positions;
pub mod relation_dialog;
pub mod replace_dialog;
pub mod replicate_dialog;
pub mod standard;
pub mod studio_edit;
pub mod triad;
pub mod where_used;

use cadrs_ui::ShortcutKey;
use std::collections::HashMap;
use std::sync::Arc;

use crate::GizmoGroups;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::assembly::commands::DeleteMateFeatures;
use cadrs_core::assembly::{Instance, InstanceId, Pose};
use cadrs_core::rebuild::Build;
use cadrs_core::{Element, ElementId, Feature, FeatureId, PartId};

use crate::parts::{PartCache, PickFilter};
use crate::viewport::{Pick, PlaneHighlight, Selection};
use crate::{ActiveDocument, AppState};

pub struct AssemblyPlugin;

impl Plugin for AssemblyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssemblyParts>()
            .init_resource::<ViewportGrab>()
            .init_resource::<PendingSelection>()
            .init_resource::<InstanceClipboard>()
            .gizmo_group::<connectors::ConnectorHaloGizmos>()
            .gizmo_group::<connectors::ConnectorGizmos>()
            .add_systems(Startup, |mut store: ResMut<GizmoConfigStore>| {
                connectors::configure(&mut store)
            })
            .add_systems(
                Update,
                (assembly_keys, apply_pending_selection)
                    .after(crate::viewport::apply_view_to_camera)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                prune_selection
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                mark_live
                    .before(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            // P3H.6: box select in the view.
            .init_resource::<box_select::AssemblyBox>()
            .add_systems(
                Update,
                (box_select::box_select, box_select::draw_box)
                    .chain()
                    .after(triad::TriadSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                OnExit(AppState::Document),
                |mut a: ResMut<AssemblyParts>, mut p: ResMut<PendingSelection>| {
                    *a = AssemblyParts::default();
                    p.0 = None;
                },
            )
            .add_plugins((
                list::InstanceListPlugin,
                folders::AssemblyFoldersPlugin,
                menu::AssemblyMenuPlugin,
                insert::InsertPlugin,
                triad::TriadPlugin,
                mate_dialog::MateDialogPlugin,
                group_dialog::GroupDialogPlugin,
                mates_list::MatesListPlugin,
                animate::AnimatePlugin,
                drag::DragPlugin,
                mate_display::MateDisplayPlugin,
                standard::StandardContentPlugin,
                bom_panel::BomPanelPlugin,
                connector_tool::ConnectorToolPlugin,
            ))
            .add_plugins((
                studio_edit::StudioEditPlugin,
                named_positions::NamedPositionsPlugin,
                explode::ExplodePlugin,
                replicate_dialog::ReplicatePlugin,
                items::ItemsPlugin,
            ))
            .add_plugins((
                relation_dialog::RelationDialogPlugin,
                in_context::InContextPlugin,
                managed_context::ManagedContextPlugin,
                interference::InterferencePlugin,
                replace_dialog::ReplaceDialogPlugin,
                where_used::WhereUsedPlugin,
            ))
            .add_plugins(insert_linked::InsertLinkedPlugin);
    }
}

/// The instance clipboard (A15.13): what Copy took, for Paste.
#[derive(Resource, Debug, Clone, Default)]
pub struct InstanceClipboard(pub Option<ClipboardInstance>);

#[derive(Debug, Clone)]
pub struct ClipboardInstance {
    pub source: cadrs_core::assembly::InstanceSource,
    /// A rigid Part Studio instance's parts.
    pub parts: Vec<PartId>,
    pub pose: Pose,
    /// "Structural Rod <1>" (the menu items' labels; "3 items" for several).
    pub name: String,
    /// P3G.1 (ER X5): a linked instance's reference, kept by the copy.
    pub link: Option<cadrs_core::external::SourceRef>,
    /// The other instances copied with it (P3G.1, ER6.7: Copy of a multi-selection).
    pub more: Vec<ClipboardInstance>,
}

/// **Copy <instance>** (A15.13).
pub fn copy_instance(world: &mut World, instance: InstanceId) {
    copy_instances(world, &[instance]);
}

/// **Copy n items**: every instance of `instances` (P3G.1, ER6.7), each with its link (ER X5).
pub fn copy_instances(world: &mut World, instances: &[InstanceId]) {
    let Some(model) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element()?.assembly_model().cloned())
    else {
        return;
    };
    let cache = world.resource::<PartCache>();
    let mut items: Vec<ClipboardInstance> = instances
        .iter()
        .filter_map(|i| model.instance(*i))
        .map(|inst| ClipboardInstance {
            source: inst.source,
            parts: inst.parts.clone(),
            pose: inst.pose,
            name: cache
                .part_name(inst.id.part_id())
                .unwrap_or("instance")
                .to_string(),
            link: inst.link,
            more: Vec::new(),
        })
        .collect();
    if items.is_empty() {
        return;
    }
    let mut first = items.remove(0);
    if !items.is_empty() {
        first.name = format!("{} items", items.len() + 1);
    }
    first.more = items;
    world.resource_mut::<InstanceClipboard>().0 = Some(first);
}

/// **Paste <instance>** (A15.13): new instances of the copied parts (the next numbers), the first
/// under the pointer (`at`, screen px) and the others where they are relative to it, or, without
/// a pointer, next to the copies. A linked instance's copy keeps its reference (ER X5). One undo
/// step.
pub fn paste_instance(world: &mut World, at: Option<Vec2>) {
    let Some(clip) = world.resource::<InstanceClipboard>().0.clone() else {
        return;
    };
    let Some(element) = world
        .get_resource::<ActiveDocument>()
        .and_then(active_assembly)
    else {
        return;
    };
    let pose = match at {
        Some(p) => {
            let item = insert::Item {
                sources: vec![clip.source],
                label: clip.name.clone(),
                rigid: clip.parts.clone(),
                link: None,
            };
            insert::pose_at_pointer(world, &item, p)
        }
        None => clip.pose.then(&Pose::translation([25.4, 0.0, 0.0])),
    };
    // The move that takes the first copy to its new place takes the others along.
    let shift = clip.pose.inverse().then(&pose);
    let mark = world.resource::<ActiveDocument>().history.undo_len();
    let all: Vec<&ClipboardInstance> = std::iter::once(&clip).chain(clip.more.iter()).collect();
    for (k, c) in all.iter().enumerate() {
        let p = if k == 0 { pose } else { c.pose.then(&shift) };
        let instance = Instance {
            parts: c.parts.clone(),
            link: c.link,
            ..Instance::new(InstanceId::new(), c.source, p)
        };
        run(
            world,
            &cadrs_core::assembly::commands::InsertInstance { element, instance },
        );
    }
    if all.len() > 1 {
        world
            .resource_mut::<ActiveDocument>()
            .squash_since(mark, "Paste instances");
    }
}

/// A press in the viewport that a manipulator took (the triad, a placement of the Insert
/// dialog): its release is not a click that selects.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ViewportGrab(pub bool);

/// The rebuilds of the Part Studios instances come from, and what the instances were last
/// built from; plus the live overrides of manipulators (a triad drag, an insert following the
/// cursor), which move instances on screen without an undo step per frame.
#[derive(Resource, Default)]
pub struct AssemblyParts {
    /// Each source Part Studio's features and their rebuild.
    builds: HashMap<ElementId, (Vec<Feature>, Arc<Build>)>,
    /// Rebuilds still running on the kernel thread (a big studio takes seconds; the view
    /// carries on meanwhile, [`Self::build_within`]).
    pending: HashMap<ElementId, (Vec<Feature>, cadrs_core::rebuild::Pending)>,
    key: Option<AssemblyKey>,
    /// Each assembly's view parts with the key they were placed for: going back to its tab
    /// reuses them (the same meshes, [`crate::parts`] keeps theirs) instead of placing every
    /// instance's parts again.
    placed: HashMap<
        ElementId,
        (
            AssemblyKey,
            Vec<cadrs_core::Part>,
            Vec<cadrs_core::PartProps>,
        ),
    >,
    /// Poses shown instead of the document's (a triad drag in progress).
    pub preview: HashMap<InstanceId, Pose>,
    /// P3B.8: the top-level instances' placements of the exploded view shown (a view state:
    /// the document's placements don't change, [`explode`]).
    pub exploded: HashMap<InstanceId, Pose>,
    /// P3B.8: a rigid Part Studio instance's parts while its Edit dialog is open (the unticked
    /// ones hidden at once).
    pub studio_preview: Option<(InstanceId, Vec<PartId>)>,
    /// Instances shown that are not in the document yet (the Insert dialog's placement).
    pub ghosts: Vec<Instance>,
    /// Studios the ghosts may come from that are not in the document yet (a standard content
    /// configuration being placed, P3B.5).
    pub extra: Vec<Element>,
    /// P3G.1: the copies of the linked source the Insert dialog shows (another document, or this
    /// one at a version), for its tree, thumbnails and ghosts before they are inserted.
    pub linked_extra: Vec<cadrs_core::external::LinkedElement>,
    /// The instances' solids as last placed: a drag makes new ones only for what it moves.
    placed_solids: cadrs_core::assembly::PlacedSolids,
    /// The preview changes every frame (an instance or triad drag, an animation playing,
    /// [`mark_live`]): it is drawn by moving the parts' meshes and edges
    /// ([`PartCache::moved`]), not by placing their solids again. Picking meanwhile sees them
    /// where they were, so the view doesn't pre-highlight.
    pub live: bool,
}

/// Whether the preview is live ([`AssemblyParts::live`]).
fn mark_live(
    drag: Res<drag::InstanceDrag>,
    triad: Res<triad::Triad>,
    playback: Option<Res<animate::Playback>>,
    mut asm: ResMut<AssemblyParts>,
) {
    let live = drag.dragging() || triad.dragging() || playback.is_some();
    if asm.live != live {
        asm.live = live;
    }
}

/// A pose as a matrix.
fn pose_matrix(p: &Pose) -> Mat4 {
    let col = |v: [f64; 3]| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32);
    Mat4::from_cols(
        col(p.rotate([1.0, 0.0, 0.0])).extend(0.0),
        col(p.rotate([0.0, 1.0, 0.0])).extend(0.0),
        col(p.rotate([0.0, 0.0, 1.0])).extend(0.0),
        col(p.translation).extend(1.0),
    )
}

type AssemblyKey = (
    ElementId,
    Vec<cadrs_core::assembly::structure::Occurrence>,
    Vec<(ElementId, usize, Vec<cadrs_core::PartProps>, String)>,
    Vec<Instance>,
);

/// The parts of the view that are parts inside subassemblies, and the occurrences they stand for
/// (P3B.4): a pick on `PartId(S, k)` is on the occurrence `derive(S, …)` for mates and drags, and
/// on the instance `S` for the lists and menus ([`instance_of`]). Set with the view's parts.
type OccurrenceTable = (HashMap<PartId, InstanceId>, HashMap<InstanceId, PartId>);
static OCCURRENCES: std::sync::RwLock<Option<OccurrenceTable>> = std::sync::RwLock::new(None);

/// The board instances the active assembly's view hosts (Edit layout in context), at any depth:
/// by assembly: [(id as a feature's, PCB Studio, board)]. Set with the view's parts, so the
/// per-frame lookups ([`crate::board_in_studio::instance_board`]) don't walk the assembly.
pub type BoardHosts = HashMap<ElementId, Vec<(FeatureId, ElementId, cadrs_core::pcb::BoardId)>>;
pub static BOARD_HOSTS: std::sync::RwLock<Option<BoardHosts>> = std::sync::RwLock::new(None);

/// The occurrence a part of the view stands for (the instance itself for a part instance).
pub fn occurrence_of(part: PartId) -> InstanceId {
    OCCURRENCES
        .read()
        .ok()
        .and_then(|t| t.as_ref().and_then(|(m, _)| m.get(&part).copied()))
        .unwrap_or_else(|| InstanceId::of_part(part))
}

/// The part of the view that shows an occurrence.
pub fn occurrence_part(i: InstanceId) -> PartId {
    OCCURRENCES
        .read()
        .ok()
        .and_then(|t| t.as_ref().and_then(|(_, m)| m.get(&i).copied()))
        .unwrap_or_else(|| i.part_id())
}

/// An occurrence of the active assembly (or a ghost of the Insert dialog): its placement and
/// its source part.
pub fn occurrence_source(
    doc: &ActiveDocument,
    parts: &AssemblyParts,
    id: InstanceId,
) -> Option<(Pose, ElementId, PartId)> {
    if let Some(g) = parts.ghosts.iter().find(|g| g.id == id) {
        return Some((g.pose, g.source.element(), g.source.part()?));
    }
    let model = doc.active_element()?.assembly_model()?;
    if let Some(i) = model.instance(id) {
        return Some((i.pose, i.source.element(), i.source.part()?));
    }
    cadrs_core::assembly::structure::occurrences(&doc.doc, model)
        .into_iter()
        .find(|o| o.id == id)
        .map(|o| (o.pose, o.element, o.part))
}

/// The active assembly as the solver sees it (its parts at any depth, [`solver_model`]).
///
/// [`solver_model`]: cadrs_core::assembly::structure::solver_model
pub fn flat_model(doc: &ActiveDocument) -> Option<cadrs_core::assembly::Assembly> {
    let model = doc.active_element()?.assembly_model()?;
    Some(cadrs_core::assembly::structure::solver_model(
        &doc.doc, model,
    ))
}

/// For a subassembly instance: its first part's occurrence and its placement in the
/// subassembly (so a triad on the subassembly can drag that part).
pub fn first_part_of(doc: &ActiveDocument, instance: InstanceId) -> Option<(InstanceId, Pose)> {
    let model = doc.active_element()?.assembly_model()?;
    if !model.instance(instance)?.source.is_composite() {
        return None;
    }
    cadrs_core::assembly::structure::occurrences(&doc.doc, model)
        .into_iter()
        .find(|o| o.top == instance)
        .map(|o| (o.id, o.in_top))
}

impl AssemblyParts {
    /// The current rebuild of the Part Studio `element` (rebuilt if its features changed).
    pub fn build(&mut self, doc: &cadrs_core::Document, element: ElementId) -> Option<Arc<Build>> {
        self.build_within(doc, element, None).0
    }

    /// The rebuild of the Part Studio `element`, waiting at most `budget` for it (`None`: until
    /// it is done). A longer one goes on in the background: this gives the last rebuild (if
    /// any) and `false` until it is done.
    pub fn build_within(
        &mut self,
        doc: &cadrs_core::Document,
        element: ElementId,
        budget: Option<std::time::Duration>,
    ) -> (Option<Arc<Build>>, bool) {
        let Some(el) = doc
            .element(element)
            .or_else(|| self.extra.iter().find(|e| e.id == element))
            .or_else(|| {
                self.linked_extra
                    .iter()
                    .find(|l| l.id() == element)
                    .map(|l| &l.element)
            })
        else {
            return (None, true);
        };
        if el.assembly_model().is_some() {
            return (None, true);
        }
        // (A PCB Studio's boards as Derived features: a board's instance.)
        let features = el.part_features();
        if let Some((f, b)) = self.builds.get(&element)
            && f.as_slice() == features
        {
            return (Some(b.clone()), true);
        }
        if !self
            .pending
            .get(&element)
            .is_some_and(|(f, _)| f.as_slice() == features)
        {
            // Restored from the session snapshot when the document was opened before.
            let p = cadrs_core::rebuild::request_persisted(features.to_vec());
            self.pending.insert(element, (features.to_vec(), p));
        }
        let done = self
            .pending
            .get_mut(&element)
            .and_then(|(_, p)| p.wait(budget));
        match done {
            Some(b) => {
                self.pending.remove(&element);
                self.builds.insert(element, (features.to_vec(), b.clone()));
                (Some(b), true)
            }
            None => (self.builds.get(&element).map(|(_, b)| b.clone()), false),
        }
    }
}

/// The instances of the active Assembly tab `el`, with the overrides, as parts of the
/// [`PartCache`] (called by the part cache's update while an Assembly tab is active).
pub fn update_assembly_parts(
    doc: &ActiveDocument,
    el: &Element,
    cache: &mut PartCache,
    asm: &mut AssemblyParts,
    budget: Option<std::time::Duration>,
) {
    let Some(model) = el.assembly_model() else {
        return;
    };
    // The previews (by occurrence: a part of a subassembly moves it, or moves within it).
    let mut view = model.clone();
    // P3B.8: a rigid studio instance being edited shows only the parts ticked.
    if let Some((id, parts)) = &asm.studio_preview
        && let Some(i) = view.instance_mut(*id)
    {
        i.parts = parts.clone();
    }
    // P3B.8: an exploded view shown.
    for (id, p) in &asm.exploded {
        if let Some(i) = view.instance_mut(*id) {
            i.pose = *p;
        }
    }
    let poses: Vec<(InstanceId, Pose)> = asm.preview.iter().map(|(i, p)| (*i, *p)).collect();
    // A live preview moves the parts as drawn (below); another places them.
    let live = asm.live && !poses.is_empty();
    if !poses.is_empty() && !live {
        let _ = cadrs_core::assembly::structure::place_in(&doc.doc, &mut view, &poses);
    }
    view.instances.extend(asm.ghosts.iter().cloned());
    view.mates.clear();
    // P3G.1: ghosts of a linked subassembly are seen with its copies.
    let with_links;
    let d: &cadrs_core::Document = if asm.ghosts.is_empty() || asm.linked_extra.is_empty() {
        &doc.doc
    } else {
        with_links = crate::linked::with_links(&doc.doc, &asm.linked_extra);
        &with_links
    };
    let occ = cadrs_core::assembly::structure::occurrences(d, &view);
    // Live: each moved part's move from where its solid is to where the preview puts it.
    let mut moved = HashMap::new();
    if live {
        let mut to = view.clone();
        let _ = cadrs_core::assembly::structure::place_in(&doc.doc, &mut to, &poses);
        let at: HashMap<PartId, Pose> = cadrs_core::assembly::structure::occurrences(d, &to)
            .into_iter()
            .map(|o| (o.view_part, o.pose))
            .collect();
        for o in &occ {
            if let Some(p) = at.get(&o.view_part)
                && *p != o.pose
            {
                moved.insert(o.view_part, pose_matrix(&o.pose.inverse().then(p)));
            }
        }
    }
    if cache.moved != moved {
        cache.moved = moved;
    }
    let mut sources: Vec<ElementId> = occ.iter().map(|o| o.element).collect();
    sources.sort();
    sources.dedup();
    let mut srcs = Vec::new();
    let mut waiting = false;
    for s in &sources {
        let (b, current) = asm.build_within(d, *s, budget);
        waiting |= !current;
        if let Some(b) = b {
            let e = d.element(*s);
            srcs.push((
                *s,
                Arc::as_ptr(&b) as usize,
                e.map(|e| e.part_props().to_vec()).unwrap_or_default(),
                e.map(|e| e.name.clone()).unwrap_or_default(),
            ));
        }
    }
    let key = (el.id, occ.clone(), srcs, view.instances.clone());
    if cache.assembly == Some(el.id) && asm.key.as_ref() == Some(&key) {
        if cache.rebuilding != waiting {
            cache.rebuilding = waiting;
        }
        return;
    }
    let by_part: HashMap<PartId, InstanceId> = occ
        .iter()
        .filter(|o| o.id != o.top)
        .map(|o| (o.view_part, o.id))
        .collect();
    let by_occ: HashMap<InstanceId, PartId> = by_part.iter().map(|(p, i)| (*i, *p)).collect();
    if let Ok(mut t) = OCCURRENCES.write() {
        *t = Some((by_part, by_occ));
    }
    let builds: HashMap<ElementId, Arc<Build>> = sources
        .iter()
        .filter_map(|s| Some((*s, asm.builds.get(s)?.1.clone())))
        .collect();
    let (parts, props) = match asm.placed.get(&el.id) {
        Some((k, parts, props)) if *k == key => (parts.clone(), props.clone()),
        _ => {
            let (parts, props) = cadrs_core::assembly::instance_parts_placed(
                d,
                &view,
                |e| builds.get(&e).cloned(),
                &mut asm.placed_solids,
            );
            asm.placed.retain(|e, _| doc.doc.element(*e).is_some());
            asm.placed
                .insert(el.id, (key.clone(), parts.clone(), props.clone()));
            (parts, props)
        }
    };
    // The source studios' feature appearances (faces keep their feature's colour).
    let mut appearances = Vec::new();
    for s in &sources {
        if let Some(e) = d.element(*s) {
            appearances.extend(e.feature_appearances().iter().cloned());
        }
    }
    cache.set_assembly_parts(el.id, parts, props, appearances);
    // Each board instance as a derived board's output (its frame where the instance puts it,
    // its parts the instance's), keyed by the instance's id: Edit layout in context works on it
    // as on a Part Studio's derived board (`crate::board_in_studio`).
    // (Top-level instances by their own id; one inside a subassembly by its board part's
    // occurrence, `crate::board_in_studio::instance_board`.)
    // (key, PCB Studio, board, pose, source part → view part)
    type Host = (
        FeatureId,
        ElementId,
        cadrs_core::pcb::BoardId,
        Pose,
        HashMap<PartId, PartId>,
    );
    let mut hosts: Vec<Host> = Vec::new();
    for i in &view.instances {
        let cadrs_core::assembly::InstanceSource::Studio {
            element,
            board: Some(b),
        } = i.source
        else {
            continue;
        };
        let ids = occ
            .iter()
            .filter(|o| o.top == i.id)
            .map(|o| (o.part, o.view_part))
            .collect();
        hosts.push((FeatureId(i.id.0), element, b, i.pose, ids));
    }
    for o in occ.iter().filter(|o| o.child.is_some()) {
        let Some(b) = crate::board_in_studio::board_of_occurrence(d, o) else {
            continue;
        };
        // Its parts: those of the same board instance (same subassembly instance, studio, board
        // and place in it: deeper down, `child` is the innermost subassembly's instance).
        let ids = occ
            .iter()
            .filter(|x| {
                x.top == o.top
                    && x.child == o.child
                    && x.element == o.element
                    && x.part.feature == o.part.feature
                    && x.in_top == o.in_top
            })
            .map(|x| (x.part, x.view_part))
            .collect();
        hosts.push((FeatureId(o.id.0), o.element, b, o.pose, ids));
    }
    if let Ok(mut t) = BOARD_HOSTS.write() {
        let t = t.get_or_insert_with(HashMap::new);
        // (Only this document's assemblies: a closed one's entries go.)
        t.retain(|a, _| doc.doc.element(*a).is_some());
        t.insert(el.id, hosts.iter().map(|h| (h.0, h.1, h.2)).collect());
    }
    let mut boards = HashMap::new();
    for (key, element, b, p, ids) in hosts {
        let Some(out) = builds
            .get(&element)
            .and_then(|x| x.derived.get(&cadrs_core::pcb::board_feature(b)))
        else {
            continue;
        };
        let id = |q: &PartId| ids.get(q).copied();
        let mut mine = cadrs_core::derived::DerivedOutput {
            parts: out.parts.iter().filter_map(id).collect(),
            boards: out
                .boards
                .iter()
                .filter_map(|c| {
                    let f = c.frame;
                    let frame = cadrs_sketch::PlaneFrame {
                        origin: p.apply(f.origin),
                        u: p.rotate(f.u),
                        v: p.rotate(f.v),
                    };
                    Some(cadrs_core::derived::BoardCopy {
                        part: id(&c.part)?,
                        frame,
                        thickness: c.thickness,
                    })
                })
                .collect(),
            footprints: out
                .footprints
                .iter()
                .filter_map(|(q, f)| Some((id(q)?, *f)))
                .collect(),
            ..Default::default()
        };
        // Its components running into the other instances, hidden or not (on their meshes, as
        // a Part Studio's derived board checks them).
        let solid = |p: &PartId| {
            cache
                .parts
                .iter()
                .find(|x| x.id == *p)
                .map(|x| (x.id, x.solid.clone()))
        };
        let comps: Vec<(PartId, Arc<cadrs_core::solid::Solid>)> = mine
            .footprints
            .iter()
            .filter_map(|(p, _)| solid(p))
            .collect();
        let others: Vec<(PartId, Arc<cadrs_core::solid::Solid>)> = cache
            .parts
            .iter()
            .filter(|x| !ids.values().any(|v| *v == x.id))
            .map(|x| (x.id, x.solid.clone()))
            .collect();
        if let Some(frame) = mine.boards.first().map(|b| b.frame) {
            let c: Vec<(PartId, &Arc<cadrs_core::solid::Solid>)> =
                comps.iter().map(|(p, s)| (*p, s)).collect();
            let o: Vec<(PartId, &Arc<cadrs_core::solid::Solid>)> =
                others.iter().map(|(p, s)| (*p, s)).collect();
            mine.collisions = cadrs_core::pcb::fit::collisions(&c, &o, &frame);
        }
        boards.insert(key, mine);
    }
    if cache.derived != boards {
        cache.derived = boards;
    }
    // Sources still rebuilding: the Rebuilding… pill, and the first view's fit waits for them.
    cache.rebuilding = waiting;
    asm.key = Some(key);
}

/// What a click picks in an assembly: the origin, and faces, edges and vertices of instances.
pub fn pick_filter() -> PickFilter {
    PickFilter {
        origin: true,
        faces: true,
        edges: true,
        ..PickFilter::none()
    }
}

/// The instance a pick is on (a face, edge or vertex of it, or the whole instance).
pub fn instance_of(p: &Pick) -> Option<InstanceId> {
    p.part().map(InstanceId::of_part)
}

/// The active tab, if it is an assembly.
pub fn active_assembly(doc: &ActiveDocument) -> Option<ElementId> {
    doc.active_element()
        .filter(|e| e.assembly_model().is_some())
        .map(|e| e.id)
}

/// The instances selected (in the list or the view; a face or edge selects its instance).
pub fn selected_instances(selection: &Selection) -> Vec<InstanceId> {
    let mut out = Vec::new();
    for p in &selection.0 {
        if let Some(i) = instance_of(p)
            && !out.contains(&i)
        {
            out.push(i);
        }
    }
    out
}

/// The points of the shown instances of the active assembly, for zoom to fit.
pub fn shown_points(doc: &ActiveDocument) -> Vec<Vec3> {
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for o in cadrs_core::assembly::structure::occurrences(&doc.doc, model)
        .iter()
        .filter(|o| !o.hidden)
    {
        let Some(el) = doc.doc.element(o.element) else {
            continue;
        };
        let build = cadrs_core::rebuild::build(&el.part_features());
        let Some(p) = build.part(o.part) else {
            continue;
        };
        out.extend(p.solid.positions.iter().map(|q| {
            let w = o.pose.apply(*q);
            Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32)
        }));
    }
    out
}

/// Runs an assembly command on the active assembly.
pub fn run(world: &mut World, cmd: &dyn cadrs_core::Command) -> bool {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return false;
    };
    match doc.execute(cmd) {
        Ok(()) => true,
        Err(e) => {
            warn!("assembly: {e}");
            false
        }
    }
}

/// Shared by the instance menu and keyboard: remove placements in one undo step, retaining
/// their source parts and clearing picks of the removed instances only after success.
fn delete_instances(world: &mut World, element: ElementId, instances: Vec<InstanceId>) {
    if instances.is_empty()
        || !run(
            world,
            &cadrs_core::assembly::commands::DeleteInstances {
                element,
                instances: instances.clone(),
            },
        )
    {
        return;
    }
    world
        .resource_mut::<Selection>()
        .0
        .retain(|p| instance_of(p).is_none_or(|i| !instances.contains(&i)));
}

/// A selection to make once the view shows `element` (Switch to: the source part is selected
/// in its Part Studio, after the tab change cleared the selection).
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingSelection(pub Option<(ElementId, Vec<Pick>)>);

fn apply_pending_selection(
    mut pending: ResMut<PendingSelection>,
    view: Res<crate::viewport::ViewportView>,
    mut selection: ResMut<Selection>,
) {
    let Some((el, picks)) = pending.0.clone() else {
        return;
    };
    if view.element == Some(el) {
        selection.0 = picks;
        pending.0 = None;
    }
}

/// Picks of instances that are gone (deleted, or replaced by a Replicate edit) leave the
/// selection, so an undo that brings them back doesn't bring them back selected (P3B.8 judge).
fn prune_selection(
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    mut selection: ResMut<Selection>,
) {
    let Some(doc) = doc else { return };
    if active_assembly(&doc).is_none()
        || !(doc.is_changed() || cache.is_changed())
        || cache.assembly != doc.active
    {
        return;
    }
    // An instance's pick is on any part of it (a subassembly's row picks `PartId(S, 0)`).
    let shown: std::collections::HashSet<InstanceId> = cache
        .parts
        .iter()
        .map(|p| InstanceId::of_part(p.id))
        .collect();
    let gone = |p: &Pick| {
        p.part()
            .is_some_and(|part| !shown.contains(&InstanceId::of_part(part)))
    };
    if selection.0.iter().any(gone) {
        selection.0.retain(|p| !gone(p));
    }
}

/// Assembly shortcuts, including deletion of the selected instances through undo/redo.
#[allow(clippy::too_many_arguments)]
fn assembly_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    q_features: Query<(), With<cadrs_ui::FeatureDialogState>>,
    doc: Option<Res<ActiveDocument>>,
    highlight: Res<PlaneHighlight>,
    insert: Option<Res<insert::InsertSession>>,
    mut was_typing: Local<bool>,
    mut commands: Commands,
) {
    let Some(doc) = doc else {
        keys_in.clear();
        return;
    };
    let Some(element) = active_assembly(&doc) else {
        keys_in.clear();
        return;
    };
    // A field that just took Enter or Esc (and dropped the focus) still counts as typing.
    let typing_now = focus.get().is_some_and(|e| q_fields.contains(e));
    let typing = typing_now || *was_typing;
    *was_typing = typing_now;
    let modifiers = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed || typing || !q_dialogs.is_empty() {
            continue;
        }
        // Ctrl+C / Ctrl+V: the instance clipboard (A15.13).
        if ctrl && !shift {
            match k.shortcut_code() {
                KeyCode::KeyC => {
                    commands.queue(|world: &mut World| {
                        let selected = selected_instances(world.resource::<Selection>());
                        copy_instances(world, &selected);
                    });
                }
                KeyCode::KeyV => {
                    commands.queue(|world: &mut World| {
                        let hover = world.resource::<bevy::picking::hover::HoverMap>();
                        let over = hover
                            .get(&bevy::picking::pointer::PointerId::Mouse)
                            .is_some_and(|hits| {
                                hits.keys().any(|e| {
                                    world.get::<crate::viewport::ViewportArea>(*e).is_some()
                                })
                            });
                        let at = over
                            .then(|| world.resource::<crate::viewport::ViewportDrag>().pointer());
                        paste_instance(world, at);
                    });
                }
                _ => {}
            }
            continue;
        }
        if modifiers {
            continue;
        }
        // The mate, Group and Animate dialogs: Enter accepts, Esc cancels.
        if k.shortcut_code() == KeyCode::Escape || k.shortcut_code() == KeyCode::Enter {
            let esc = k.shortcut_code() == KeyCode::Escape;
            commands.queue(move |world: &mut World| {
                if world.contains_resource::<animate::AnimateSession>() {
                    world.remove_resource::<animate::AnimateSession>();
                    animate::stop(world);
                } else if world.contains_resource::<connector_tool::ConnectorSession>() {
                    if esc {
                        connector_tool::cancel(world);
                    } else {
                        connector_tool::accept(world);
                    }
                } else if world.contains_resource::<mate_dialog::MateSession>() {
                    if esc {
                        mate_dialog::cancel(world);
                    } else {
                        mate_dialog::accept(world);
                    }
                } else if world.contains_resource::<group_dialog::GroupSession>() {
                    if esc {
                        world.remove_resource::<group_dialog::GroupSession>();
                    } else {
                        group_dialog::accept(world);
                    }
                }
            });
            continue;
        }
        match k.shortcut_code() {
            KeyCode::Delete | KeyCode::Backspace
                if !k.repeat && insert.is_none() && q_features.is_empty() =>
            {
                commands.queue(move |world: &mut World| {
                    let instances = selected_instances(world.resource::<Selection>());
                    if !instances.is_empty() {
                        delete_instances(world, element, instances);
                        return;
                    }
                    // #64: with no instance selected, the mate selected in the list (its row
                    // stays highlighted while parts are picked, so it is only the fallback).
                    if let Some(mate) = world
                        .get_resource_mut::<mate_display::MateSelection>()
                        .and_then(|mut s| s.0.take())
                    {
                        let mates = vec![mate];
                        run(world, &DeleteMateFeatures { element, mates });
                    }
                });
            }
            KeyCode::KeyM if !shift => {
                commands.queue(|world: &mut World| {
                    mate_dialog::open_mate_dialog(
                        world,
                        cadrs_core::assembly::mate::MateType::Fastened,
                    )
                });
            }
            // A14.1: all mates shown or hidden.
            KeyCode::KeyJ if !shift => {
                commands.queue(|world: &mut World| {
                    let Some(model) = world
                        .get_resource::<ActiveDocument>()
                        .and_then(|d| d.active_element()?.assembly_model().cloned())
                    else {
                        return;
                    };
                    world
                        .resource_mut::<mate_display::MateDisplay>()
                        .toggle_all(&model);
                });
            }
            // A14.4: show mates mode.
            KeyCode::KeyH if !shift => {
                commands.queue(|world: &mut World| {
                    let mut d = world.resource_mut::<mate_display::MateDisplay>();
                    d.mode = !d.mode;
                    if !d.mode {
                        d.pinned.clear();
                    }
                });
            }
            KeyCode::KeyI if !shift && insert.is_none() => {
                commands.queue(insert::open_insert_dialog);
            }
            KeyCode::KeyY if !shift => {
                let hovered = highlight.viewport.as_ref().and_then(instance_of);
                if let Some(i) = hovered {
                    commands.queue(move |world: &mut World| {
                        run(
                            world,
                            &cadrs_core::assembly::commands::SetInstancesHidden {
                                element,
                                instances: vec![i],
                                hidden: true,
                            },
                        );
                    });
                }
            }
            KeyCode::KeyY => {
                commands.queue(move |world: &mut World| menu::show_all_instances(world, element));
            }
            _ => {}
        }
    }
}

/// The parts of the view that show instances (a subassembly instance: all of its parts).
pub fn view_parts(cache: &PartCache, instances: &[InstanceId]) -> Vec<PartId> {
    cache
        .parts
        .iter()
        .map(|p| p.id)
        .filter(|p| instances.contains(&InstanceId::of_part(*p)))
        .collect()
}

/// The part id an instance has in the view.
pub fn part_of(i: InstanceId) -> PartId {
    i.part_id()
}

#[cfg(test)]
mod deletion_tests {
    use super::*;
    use cadrs_core::assembly::InstanceSource;
    use cadrs_ui::input::TextInputField;

    fn scene() -> (App, ElementId, [InstanceId; 3]) {
        let mut doc = cadrs_core::Document::new("Delete instances");
        let (studio, assembly) = (doc.elements[0].id, doc.elements[1].id);
        let source = PartId::new(FeatureId::new(), 0);
        let child = Element::assembly("Subassembly");
        let child_id = child.id;
        doc.elements.push(child);
        let ids = [InstanceId::new(), InstanceId::new(), InstanceId::new()];
        doc.elements[1].assembly.instances = vec![
            Instance::new(
                ids[0],
                InstanceSource::Part {
                    element: studio,
                    part: source,
                },
                Pose::IDENTITY,
            ),
            Instance::new(
                ids[1],
                InstanceSource::Assembly { element: child_id },
                Pose::IDENTITY,
            ),
            Instance::new(
                ids[2],
                InstanceSource::Part {
                    element: studio,
                    part: source,
                },
                Pose::IDENTITY,
            ),
        ];
        let mut active = ActiveDocument::new(doc);
        active.active = Some(assembly);
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .insert_resource(active)
            .insert_resource(Selection(vec![Pick::Part(ids[0].part_id())]))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<PlaneHighlight>()
            .add_systems(Update, assembly_keys);
        (app, assembly, ids)
    }

    fn press(app: &mut App, code: KeyCode, repeat: bool) {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: bevy::input::keyboard::Key::Delete,
            state: ButtonState::Pressed,
            text: None,
            repeat,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    #[test]
    fn delete_and_backspace_remove_selected_instances_in_one_undo_step() {
        for key in [KeyCode::Delete, KeyCode::Backspace] {
            let (mut app, element, ids) = scene();
            let before = app.world().resource::<ActiveDocument>().doc.clone();
            let face = cadrs_sketch::FaceName::new(
                ids[0].0,
                cadrs_sketch::FaceOrigin::Unnamed { index: 0 },
            );
            app.world_mut().resource_mut::<Selection>().0 = vec![
                Pick::Part(ids[0].part_id()),
                Pick::Face(ids[0].part_id(), face),
                Pick::Part(PartId::new(FeatureId(ids[1].0), 1)),
            ];
            press(&mut app, key, false);
            let active = app.world().resource::<ActiveDocument>();
            let remaining = &active.doc.element(element).unwrap().assembly.instances;
            assert_eq!(remaining.len(), 1);
            assert_eq!(remaining[0].id, ids[2]);
            assert_eq!(
                active.doc.elements[0], before.elements[0],
                "source studio must remain intact"
            );
            assert_eq!(
                active.doc.elements[2], before.elements[2],
                "source subassembly must remain intact"
            );
            assert_eq!(active.history.undo_len(), 1);
            assert!(app.world().resource::<Selection>().0.is_empty());
            let mut active = app.world_mut().resource_mut::<ActiveDocument>();
            assert!(active.undo().is_some());
            assert_eq!(active.doc, before);
            assert!(active.redo().is_some());
            assert_eq!(
                active
                    .doc
                    .element(element)
                    .unwrap()
                    .assembly
                    .instances
                    .len(),
                1
            );
        }
    }

    #[test]
    fn delete_removes_the_mate_selected_in_the_list() {
        use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind};
        let (mut app, element, ids) = scene();
        let mate = MateId::new();
        let group = MateKind::Group {
            instances: vec![ids[0], ids[2]],
        };
        let mut active = app.world_mut().resource_mut::<ActiveDocument>();
        let asm = &mut active.doc.elements[1].assembly;
        asm.mates = vec![MateFeature::new(mate, "Group 1", group)];
        app.world_mut().resource_mut::<Selection>().0.clear();
        app.insert_resource(mate_display::MateSelection(Some(mate)));
        press(&mut app, KeyCode::Delete, false);
        let active = app.world().resource::<ActiveDocument>();
        let asm = &active.doc.element(element).unwrap().assembly;
        assert!(asm.mates.is_empty());
        assert_eq!(asm.instances.len(), 3);
        assert_eq!(active.history.undo_len(), 1);
        let sel = app.world().resource::<mate_display::MateSelection>();
        assert_eq!(sel.0, None);
    }

    #[test]
    fn selected_instances_win_over_the_selected_mate() {
        use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind};
        let (mut app, element, ids) = scene();
        let mate = MateId::new();
        let group = MateKind::Group {
            instances: vec![ids[1], ids[2]],
        };
        let mut active = app.world_mut().resource_mut::<ActiveDocument>();
        let asm = &mut active.doc.elements[1].assembly;
        asm.mates = vec![MateFeature::new(mate, "Group 1", group)];
        app.insert_resource(mate_display::MateSelection(Some(mate)));
        press(&mut app, KeyCode::Delete, false);
        let asm = &app.world().resource::<ActiveDocument>().doc;
        let asm = &asm.element(element).unwrap().assembly;
        assert_eq!(asm.instances.len(), 2);
        assert_eq!(asm.mates.len(), 1);
    }

    #[test]
    fn deletion_shortcut_respects_focus_dialogs_repeats_and_empty_selection() {
        for guard in 0..7 {
            let (mut app, _, _) = scene();
            let before = app.world().resource::<ActiveDocument>().doc.clone();
            match guard {
                0 => app.world_mut().resource_mut::<Selection>().0.clear(),
                1 => {
                    let field = app.world_mut().spawn(TextInputField).id();
                    app.insert_resource(InputFocus::from_entity(field));
                }
                2 => {
                    app.world_mut().spawn(cadrs_ui::DialogRoot);
                }
                3 => {
                    app.world_mut().spawn(cadrs_ui::FeatureDialogState {
                        title: "Insert".into(),
                        valid: true,
                        error: false,
                    });
                }
                4 => app
                    .world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::ControlLeft),
                5 => {
                    let studio = before.elements[0].id;
                    app.world_mut().resource_mut::<ActiveDocument>().active = Some(studio);
                }
                _ => {}
            }
            press(&mut app, KeyCode::Delete, guard == 6);
            let active = app.world().resource::<ActiveDocument>();
            assert_eq!(active.doc, before, "guard {guard}");
            assert_eq!(active.history.undo_len(), 0, "guard {guard}");
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use cadrs_core::samples::step_stool as ss;

    fn v3(p: [f64; 3]) -> Vec3 {
        Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
    }

    /// A live preview leaves the solids where they are and moves the parts as drawn, exactly
    /// where placing them would put them; once it isn't live, they are placed as before.
    #[test]
    fn a_live_preview_moves_the_parts_as_drawn_not_their_solids() {
        let mut doc = ActiveDocument::new(ss::document().unwrap());
        doc.active = Some(ss::ASSEMBLY);
        let el = doc.active_element().unwrap().clone();
        let (mut cache, mut asm) = (PartCache::default(), AssemblyParts::default());
        update_assembly_parts(&doc, &el, &mut cache, &mut asm, None);
        let before: Vec<_> = cache.parts.iter().map(|p| p.solid.clone()).collect();
        let generation = cache.generation;
        let model = el.assembly_model().unwrap();
        let inst = model.instances.iter().find(|i| !i.fixed).unwrap();
        let to = inst
            .pose
            .then(&Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], 0.5))
            .then(&Pose::translation([30.0, -10.0, 5.0]));
        asm.preview.insert(inst.id, to);
        asm.live = true;
        update_assembly_parts(&doc, &el, &mut cache, &mut asm, None);
        assert_eq!(
            cache.generation, generation,
            "a live drag regenerates nothing"
        );
        for (p, s) in cache.parts.iter().zip(&before) {
            assert!(Arc::ptr_eq(&p.solid, s), "{}", p.name);
        }
        let part = inst.id.part_id();
        assert_eq!(cache.moved.keys().collect::<Vec<_>>(), vec![&part]);
        // Not live: the solids placed at the preview are where the move draws them.
        let m = cache.moved[&part];
        let drawn: Vec<Vec3> = cache
            .parts
            .iter()
            .find(|p| p.id == part)
            .unwrap()
            .solid
            .positions
            .iter()
            .map(|q| m.transform_point3(v3(*q)))
            .collect();
        asm.live = false;
        update_assembly_parts(&doc, &el, &mut cache, &mut asm, None);
        assert!(cache.moved.is_empty());
        let placed = &cache.parts.iter().find(|p| p.id == part).unwrap().solid;
        assert_eq!(placed.positions.len(), drawn.len());
        for (a, b) in placed.positions.iter().zip(&drawn) {
            assert!(v3(*a).distance(*b) < 1e-3, "{a:?} drawn at {b:?}");
        }
    }
}
