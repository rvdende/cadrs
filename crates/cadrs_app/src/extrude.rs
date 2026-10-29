//! The Extrude feature (M9; complete in P3.3), like Onshape's (`reference/onshape/screens/22`–`24`,
//! `NOTES.md` "Extrude"; the dialog is [`crate::extrude_dialog`]):
//!
//! 1. **Extrude** (the first tool after Sketch, or Shift+E) inserts "Extrude N" into the feature
//!    list and opens its dialog at the top left of the viewport, the "Faces and sketch regions to
//!    extrude" field waiting for a pick.
//! 2. Clicking a closed sketch region selects it (orange; the field lists "Face of Sketch 1");
//!    clicking it again deselects it. Clicking a sketch in the feature list takes the whole
//!    sketch (PS1.1), and clicking a planar part face takes the face (PS4.2). A translucent preview of the part appears at once with an
//!    **arrow manipulator** on its far face, and "Part 1" appears under Parts.
//! 3. Dragging the arrow changes the depth live (snapped to round values; the Depth field
//!    follows); dragging it through the sketch plane flips the direction. The flip button
//!    reverses the direction. The depth field takes values with units and expressions ("1 in",
//!    "20 + 5").
//! 4. ✓ or Enter accepts ("Insert Extrude 1", one undo step): the part is shaded, the sketch is
//!    consumed (greyed in the list, hidden in the view). ✕ or Esc cancels. Double-click Extrude
//!    1 in the list to edit it again.
//!
//! Every change goes through the command layer; the arrow drag previews through
//! [`PartOverride`] and records one step when released.

use crate::GizmoGroups;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::ui::UiTransform;
use cadrs_core::commands::{AddExtrude, DeleteFeature, ReplaceFeature, SetExtrude};
use cadrs_core::document::{BooleanOp, DirectionRef, EdgeRef, EndType, FaceRef, UpTo, VertexRef};
use cadrs_core::{ElementId, ElementKind, ExtrudeFeature, Feature, FeatureId, PartId, RegionRef};
use cadrs_sketch::units::Quantity;
use cadrs_ui::ShortcutKey;

use crate::parts::{PartCache, PartOverride};
use crate::viewport::{
    Pick, PickRequest, PlaneHighlight, Selection, ViewportArea, ViewportRect, ViewportView,
};
use crate::{ActiveDocument, AppState};

pub struct ExtrudePlugin;

impl Plugin for ExtrudePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ArrowState>()
            .gizmo_group::<RegionGizmos>()
            .add_systems(Startup, configure_gizmos)
            .add_systems(
                Update,
                (
                    end_session_on_tab_change,
                    extrude_picks,
                    extrude_keys,
                    arrow_pointer,
                    crate::extrude_dialog::sync_extrude_dialog,
                    region_layers,
                    draw_regions,
                )
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .before(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                PostUpdate,
                place_arrow
                    .before(bevy::ui::UiSystems::Layout)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                OnExit(AppState::Document),
                finish_on_exit.before(crate::document::save_on_exit),
            )
            .add_plugins(crate::extrude_dialog::ExtrudeDialogPlugin)
            .add_systems(
                Update,
                (automatic_operation, flip_if_behind, show_references)
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            );
    }
}

/// The extrude whose dialog is open.
#[derive(Resource, Debug, Clone)]
pub struct ExtrudeSession {
    pub element: ElementId,
    pub feature: FeatureId,
    /// Inserted by this session (cancel removes it); false when editing.
    pub is_new: bool,
    /// The undo stack's length before the session; accept merges the steps above it.
    pub mark: usize,
    /// The feature before an edit (cancel puts it back).
    pub before: Option<Feature>,
    /// The selection field that takes the viewport's picks.
    pub field: ExtrudeField,
    /// New or Add is chosen by itself (PS5.2) until a tab is clicked.
    pub op_auto: bool,
    /// The Direction option is on (its field may still be empty).
    pub direction_on: bool,
    /// The dialog's Final button (P3.7): show the features after this one too.
    pub show_final: bool,
    /// An "Up to" target was just picked for the first end: the parameters then and the
    /// rebuilds taken by then. Once they are rebuilt, a target behind the start flips the
    /// extrude (once) rather than leaving the error.
    pub flip_check: Option<(ExtrudeFeature, u64)>,
}

/// The dialog's selection fields that take picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtrudeField {
    /// Faces and sketch regions to extrude.
    #[default]
    Input,
    /// The first end's face, part or vertex.
    UpTo,
    /// The second end's.
    SecondUpTo,
    Direction,
    MergeScope,
    /// The Revolve dialog's "Revolve axis" (P3.4; the session is shared, see
    /// [`crate::revolve`]).
    Axis,
    /// P3.10 (PS7.2): the axis field's mate connector button is on: a pick is a mate connector
    /// (an explicit one, or the implicit one of a face, edge, vertex or the origin).
    AxisConnector,
}

impl ExtrudeSession {
    /// Undo may not go below this: the extrude's own insertion stays while its dialog is open.
    pub fn undo_floor(&self) -> usize {
        self.mark + usize::from(self.is_new)
    }
}

/// The arrow manipulator (two stacked icons: a dark halo and a white line): the first end's
/// (`0`) or the second end's (`1`, P3.5).
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct ArrowNode(usize);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct ArrowLine(usize);

/// The arrow manipulator: where it is on screen, whether the pointer is over it, and a drag.
#[derive(Resource, Debug, Clone, Default)]
pub struct ArrowState {
    /// Screen positions (logical px) of the arrow's base and tip, while it is shown.
    pub base_tip: Option<(Vec2, Vec2)>,
    pub hovered: bool,
    pub drag: Option<ArrowDrag>,
    /// The next click was the end of an arrow drag (not a region pick).
    swallow_click: bool,
    /// The extrude's direction (before its flip), how far its end is along it (signed) and
    /// where the arrow sits, for a drag.
    axis: Option<(Vec3, f64, Vec3)>,
    /// The second end's arrow (P3.5): where it is, whether the pointer is over it, and its
    /// direction (away from the first end) and depth.
    pub second_base_tip: Option<(Vec2, Vec2)>,
    pub second_hovered: bool,
    second_axis: Option<(Vec3, f64, Vec3)>,
}

/// An arrow drag in progress.
#[derive(Debug, Clone)]
pub struct ArrowDrag {
    start: Vec2,
    /// Signed depth along the sketch plane's normal when the drag began (negative: flipped).
    start_signed: f64,
    /// Screen px per mm along the sketch plane's normal.
    normal_px: Vec2,
    /// The parameters with the live depth.
    pub extrude: ExtrudeFeature,
    /// The second end's arrow is dragged (its depth changes).
    second: bool,
    /// Where the arrow sat when the drag began: it follows the pointer from there, ahead of
    /// the rebuilt preview.
    anchor: Vec3,
}

/// Arrow length on screen (px) and how close the pointer must be to grab it.
const ARROW_LEN: f32 = 48.0;
const ARROW_GRAB: f32 = 8.0;

// ---------------------------------------------------------------------------------------------
// Lifecycle

/// Starts a new extrude in the active Part Studio (the Extrude button and Shift+E).
pub fn begin_extrude(world: &mut World) {
    if world.contains_resource::<ExtrudeSession>()
        || world.contains_resource::<crate::sketch::SketchSession>()
    {
        return;
    }
    // P3I.6 (SM14.2): regions of a flat-pattern sketch get the abbreviated flat extrude.
    if crate::flat_ui::extrude_redirect(world) {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc
        .active_element()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .map(|e| e.id)
    else {
        return;
    };
    // Planar part faces, sketch regions and sketches selected before: they are what it
    // extrudes, a short step the way it can be seen (see `preselected`).
    let mut extrude = ExtrudeFeature::default();
    let mut cut = false;
    if let Some(pre) = preselected(world) {
        extrude.faces = pre.faces;
        extrude.regions = pre.regions;
        extrude.sketches = pre.sketches;
        extrude.depth = PRESELECTED_DEPTH;
        extrude.depth_expr = world
            .resource::<crate::WorkspaceUnits>()
            .0
            .with_unit(PRESELECTED_DEPTH, Quantity::Length);
        // Material right beyond them: going that way would add nothing to see, so it goes the
        // other way; with material there too (a part's own face), it cuts into it.
        if pre.blocked_out {
            extrude.flip = true;
            if pre.blocked_in {
                extrude.op = BooleanOp::Remove;
                cut = true;
            }
        }
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let feature = FeatureId::new();
    let mark = doc.history.undo_len();
    if let Err(e) = doc.execute(&AddExtrude {
        element,
        feature,
        extrude,
    }) {
        warn!("cannot insert an extrude: {e}");
        return;
    }
    start_session(world, element, feature, true, mark, None);
    if cut && let Some(mut s) = world.get_resource_mut::<ExtrudeSession>() {
        // Not turned back into an Add or a New by what it touches.
        s.op_auto = false;
    }
}

/// The depth of an extrude of what was selected before it: a short step out of the part (or
/// into it), so it shows without hiding what is around it.
const PRESELECTED_DEPTH: f64 = 5.0;

/// What a new extrude takes from the selection.
struct Preselected {
    faces: Vec<FaceRef>,
    regions: Vec<RegionRef>,
    sketches: Vec<FeatureId>,
    /// Solid material lies right beyond every face and region, along its normal.
    blocked_out: bool,
    /// ... and right behind each, against it.
    blocked_in: bool,
}

/// The planar part faces, sketch regions and whole sketches in the selection, and whether
/// solid material lies right beyond (and behind) them. `None` when none is selected.
/// More faces than a CAD part has: an imported mesh's facets (as the kernel's limit for merging
/// faces).
const HUGE_PART_FACES: usize = 10_000;

fn preselected(world: &World) -> Option<Preselected> {
    let cache = world.resource::<PartCache>();
    let doc = world.get_resource::<ActiveDocument>();
    let mut out = Preselected {
        faces: Vec::new(),
        regions: Vec::new(),
        sketches: Vec::new(),
        blocked_out: true,
        blocked_in: true,
    };
    let v = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    // A point on each and its normal.
    let mut probes: Vec<(Vec3, Vec3)> = Vec::new();
    for pick in &world.resource::<Selection>().0 {
        match *pick {
            Pick::Face(part, face) => {
                let Some(p) = cache.part(part) else { continue };
                let Some(i) = p.solid.faces.iter().position(|f| f.name == face) else {
                    continue;
                };
                if p.solid.faces[i].plane.is_none() {
                    continue;
                }
                let (Some(seed), Some(n)) = (p.solid.face_point(i), p.solid.face_normal(i)) else {
                    continue;
                };
                probes.push((v(seed), v(n).normalize_or_zero()));
                out.faces.push(FaceRef { part, face, seed });
            }
            Pick::Region(sketch, index) => {
                let Some(sr) = cache.sketch_regions(sketch) else {
                    continue;
                };
                let Some(region) = sr.regions.get(index as usize) else {
                    continue;
                };
                let r = RegionRef::new(sketch, region);
                probes.push((
                    v(sr.frame.to_world(r.seed)),
                    v(sr.frame.normal()).normalize_or_zero(),
                ));
                out.regions.push(r);
            }
            Pick::Feature(f)
                if doc
                    .and_then(|d| d.active_element()?.feature(f)?.sketch())
                    .is_some() =>
            {
                out.sketches.push(f)
            }
            _ => {}
        }
    }
    // A whole sketch replaces its single regions.
    out.regions.retain(|r| !out.sketches.contains(&r.sketch));
    if out.faces.is_empty() && out.regions.is_empty() && out.sketches.is_empty() {
        return None;
    }
    // A huge part (an imported mesh's facets) doesn't choose for it: a new extrude beside or
    // through an STL starts as a plain New (issue #11).
    let solid = |at: Vec3| {
        cache
            .parts
            .iter()
            .filter(|q| {
                q.kind == cadrs_core::PartKind::Solid && q.solid.faces.len() <= HUGE_PART_FACES
            })
            .any(|q| inside(&q.solid, at))
    };
    out.blocked_out = !probes.is_empty() && probes.iter().all(|(p, n)| solid(*p + *n * 0.05));
    out.blocked_in = probes.iter().all(|(p, n)| solid(*p - *n * 0.05));
    Some(out)
}

/// Whether switching the open extrude to `to` should flip its direction (#72): a Remove cuts
/// into material and an Add or New grows away from it. Switching to Remove flips when there is
/// no material ahead of the profiles but some behind them (the parts on screen are the ones
/// before an Add or New, whose body is the separate preview); back to New or Add, when there is
/// none behind them (what a Remove cuts ahead is gone from the parts on screen; what is behind
/// is as it was).
pub(crate) fn op_flips(world: &World, e: &ExtrudeFeature, to: BooleanOp) -> bool {
    let grows = |op| matches!(op, BooleanOp::New | BooleanOp::Add);
    let (cuts_now, cuts_next) = (e.op == BooleanOp::Remove, to == BooleanOp::Remove);
    if !((grows(e.op) && cuts_next) || (cuts_now && grows(to))) {
        return false;
    }
    let (Some(s), Some(cache)) = (
        world.get_resource::<ExtrudeSession>(),
        world.get_resource::<PartCache>(),
    ) else {
        return false;
    };
    let doc = world.get_resource::<ActiveDocument>();
    let v = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    // A point on each profile.
    let mut points: Vec<Vec3> = e.faces.iter().map(|f| v(f.seed)).collect();
    for r in &e.regions {
        if let Some(sr) = cache.sketch_regions(r.sketch) {
            points.push(v(sr.frame.to_world(r.seed)));
        }
    }
    for k in &e.sketches {
        if let Some(sr) = cache.sketch_regions(*k) {
            points.extend(
                sr.regions
                    .iter()
                    .map(|r| v(sr.frame.to_world(RegionRef::new(*k, r).seed))),
            );
        }
    }
    let base = extrude_direction(doc, cache, s, e);
    let dir = if e.flip { -base } else { base };
    let solid = |at: Vec3| {
        cache
            .parts
            .iter()
            .filter(|q| {
                q.kind == cadrs_core::PartKind::Solid && q.solid.faces.len() <= HUGE_PART_FACES
            })
            .any(|q| inside(&q.solid, at))
    };
    // Material somewhere along the way: 0.05 mm out, doubling to about 3 m.
    let along = |d: Vec3| {
        points
            .iter()
            .any(|p| (0..16).any(|i| solid(*p + d * (0.05 * 2f32.powi(i)))))
    };
    let behind = along(-dir);
    if cuts_next {
        behind && !along(dir)
    } else {
        !behind
    }
}

/// True if `p` is inside the closed mesh of `s`: an odd number of its triangles across a ray
/// from `p` (in a slanted direction, so it doesn't graze edges).
fn inside(s: &cadrs_core::Solid, p: Vec3) -> bool {
    if let Some((lo, hi)) = s.bounds() {
        let (lo, hi) = (
            Vec3::new(lo[0] as f32, lo[1] as f32, lo[2] as f32),
            Vec3::new(hi[0] as f32, hi[1] as f32, hi[2] as f32),
        );
        if p.cmplt(lo).any() || p.cmpgt(hi).any() {
            return false;
        }
    }
    let d = Vec3::new(0.5773, 0.6127, 0.5397).normalize();
    let v = |i: u32| {
        let q = s.positions[i as usize];
        Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)
    };
    let mut hits = 0;
    for [i, j, k] in s.indices.as_chunks::<3>().0 {
        let (a, b, c) = (v(*i), v(*j), v(*k));
        // Möller–Trumbore.
        let (e1, e2) = (b - a, c - a);
        let h = d.cross(e2);
        let det = e1.dot(h);
        if det.abs() < 1e-9 {
            continue;
        }
        let f = 1.0 / det;
        let sv = p - a;
        let u = f * sv.dot(h);
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = sv.cross(e1);
        let w = f * d.dot(q);
        if w < 0.0 || u + w > 1.0 {
            continue;
        }
        if f * e2.dot(q) > 0.0 {
            hits += 1;
        }
    }
    hits % 2 == 1
}

/// Opens an existing extrude for editing (double-click in the feature list, or Edit).
pub fn edit_extrude(world: &mut World, feature: FeatureId) {
    if let Some(s) = world.get_resource::<ExtrudeSession>() {
        if s.feature == feature {
            return;
        }
        finish_session(world);
    }
    if world.contains_resource::<crate::sketch::SketchSession>() {
        crate::sketch::finish_session(world);
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let Some(before) = el
        .feature(feature)
        .filter(|f| f.extrude().is_some())
        .cloned()
    else {
        return;
    };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    start_session(world, element, feature, false, mark, Some(before));
}

pub(crate) fn start_session(
    world: &mut World,
    element: ElementId,
    feature: FeatureId,
    is_new: bool,
    mark: usize,
    before: Option<Feature>,
) {
    world.resource_mut::<Selection>().0.clear();
    let direction_on = before
        .as_ref()
        .and_then(|f| f.extrude())
        .is_some_and(|e| e.direction.is_some());
    world.insert_resource(ExtrudeSession {
        element,
        feature,
        is_new,
        mark,
        before,
        field: ExtrudeField::Input,
        op_auto: is_new,
        direction_on,
        show_final: false,
        flip_check: None,
    });
    *world.resource_mut::<ArrowState>() = ArrowState::default();
    *world.resource_mut::<crate::revolve::AngleArrow>() = Default::default();
    let mut o = world.resource_mut::<PartOverride>();
    o.editing = Some(feature);
    o.extrude = None;
    o.revolve = None;
    cadrs_ui::close_toasts(world);
}

pub(crate) fn feature_name(world: &World, s: &ExtrudeSession) -> String {
    world
        .get_resource::<ActiveDocument>()
        .and_then(|d| {
            d.doc
                .element(s.element)?
                .feature(s.feature)
                .map(|f| f.name.clone())
        })
        .unwrap_or_else(|| "Extrude".into())
}

pub(crate) fn current(world: &World, s: &ExtrudeSession) -> Option<Feature> {
    world
        .get_resource::<ActiveDocument>()?
        .doc
        .element(s.element)?
        .feature(s.feature)
        .cloned()
}

/// ✓ / Enter: keeps the extrude (or revolve). Does nothing while it has nothing to extrude.
pub fn accept_extrude(world: &mut World) {
    let Some(s) = world.get_resource::<ExtrudeSession>().cloned() else {
        return;
    };
    end_drag(world, true);
    crate::revolve::end_angle_drag(world, true);
    let name = feature_name(world, &s);
    let Some(f) = current(world, &s).filter(|f| f.is_valid()) else {
        return;
    };
    // The rebuild with these parameters must succeed (the kernel may refuse a depth): ask for
    // it rather than trust the parts on screen, which may be a frame old (this comes back when
    // it is done).
    let features = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| Some(d.doc.element(s.element)?.active_features()))
        .unwrap_or_default();
    if crate::accept::builds(world, "extrude", features, f.id, accept_extrude) != Some(true) {
        return;
    }
    let label = if s.is_new {
        format!("Insert {name}")
    } else {
        format!("Edit {name}")
    };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        // A sketch shown again with its eye is hidden again once a feature uses it (PS1.4, as
        // after Extrude 2 in `ex1-step5.png`).
        let shown: Vec<FeatureId> = f
            .input_sketches()
            .into_iter()
            .filter(|sk| {
                doc.doc
                    .element(s.element)
                    .is_some_and(|el| el.sketch_visibility(*sk) == Some(true))
            })
            .collect();
        for sketch in shown {
            let _ = doc.execute(&cadrs_core::commands::SetSketchVisibility {
                element: s.element,
                sketch,
                visible: None,
            });
        }
        doc.squash_element_since(s.mark, s.element, label);
    }
    end_session(world);
}

/// ✕ / Esc: removes a new extrude or reverts an edit, as an undoable step.
pub fn cancel_extrude(world: &mut World) {
    let Some(s) = world.get_resource::<ExtrudeSession>().cloned() else {
        return;
    };
    end_drag(world, false);
    crate::revolve::end_angle_drag(world, false);
    let name = feature_name(world, &s);
    let cur = current(world, &s);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let label = format!("Cancel {name}");
    match (&cur, s.is_new) {
        (Some(f), true)
            if f.extrude().is_some_and(|e| e.is_empty())
                || f.revolve().is_some_and(|r| r.is_empty()) =>
        {
            // Nothing worth keeping: forget it entirely.
            doc.discard_element_since(s.mark, s.element);
        }
        (Some(_), true) => {
            doc.squash_element_since(s.mark, s.element, format!("Insert {name}"));
            let _ = doc.execute(&DeleteFeature {
                element: s.element,
                feature: s.feature,
                label,
            });
        }
        (Some(f), false) => {
            doc.squash_element_since(s.mark, s.element, format!("Edit {name}"));
            if let Some(before) = s.before.clone()
                && *f != before
            {
                let _ = doc.execute(&ReplaceFeature {
                    element: s.element,
                    feature: before,
                    label,
                });
            }
        }
        (None, _) => {
            doc.squash_element_since(s.mark, s.element, format!("Edit {name}"));
        }
    }
    end_session(world);
}

/// Accepts the extrude if it is valid, otherwise cancels it.
pub fn finish_session(world: &mut World) {
    accept_extrude(world);
    if world.contains_resource::<ExtrudeSession>() {
        cancel_extrude(world);
    }
}

fn end_session(world: &mut World) {
    world.remove_resource::<ExtrudeSession>();
    // The dialog's references were shown as the selection.
    world.resource_mut::<Selection>().0.clear();
    *world.resource_mut::<ArrowState>() = ArrowState::default();
    *world.resource_mut::<crate::revolve::AngleArrow>() = Default::default();
    *world.resource_mut::<PartOverride>() = PartOverride::default();
}

fn finish_on_exit(world: &mut World) {
    if world.contains_resource::<ExtrudeSession>() {
        finish_session(world);
    }
    world.remove_resource::<ExtrudeSession>();
}

fn end_session_on_tab_change(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<ExtrudeSession>>,
    mut commands: Commands,
) {
    let (Some(doc), Some(s)) = (doc, session) else {
        return;
    };
    if doc.active_element().map(|e| e.id) != Some(s.element) {
        commands.queue(finish_session);
    }
}

/// Records a parameter change as an undo step.
pub(crate) fn set_params(
    world: &mut World,
    extrude: ExtrudeFeature,
    label: impl Into<String>,
) -> bool {
    let Some(s) = world.get_resource::<ExtrudeSession>().cloned() else {
        return false;
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return false;
    };
    match doc.execute(&SetExtrude {
        element: s.element,
        feature: s.feature,
        extrude,
        label: label.into(),
    }) {
        Ok(()) => true,
        Err(e) => {
            warn!("cannot change the extrude: {e}");
            false
        }
    }
}

pub(crate) fn params(world: &World) -> Option<ExtrudeFeature> {
    let s = world.get_resource::<ExtrudeSession>()?;
    current(world, s)?.extrude().cloned()
}

/// A picked region toggles in the "Faces and sketch regions" field.
fn toggle_region(world: &mut World, sketch: FeatureId, index: u32) {
    let Some(mut e) = params(world) else {
        return;
    };
    let cache = world.resource::<PartCache>();
    let Some(region) = cache
        .sketch_regions(sketch)
        .and_then(|r| r.regions.get(index as usize))
        .cloned()
    else {
        return;
    };
    let name = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element()?.feature(sketch).map(|f| f.name.clone()))
        .unwrap_or_default();
    let r = RegionRef::new(sketch, &region);
    // The same region: the same boundary curves around the same seed (two regions can share
    // their curves, like the two halves of a circle a line cuts).
    let same =
        |x: &RegionRef| x.sketch == sketch && x.curves == r.curves && region.contains(x.seed);
    let label = if let Some(i) = e.regions.iter().position(same) {
        e.regions.remove(i);
        format!("Deselect Face of {name}")
    } else {
        e.regions.push(r);
        format!("Select Face of {name}")
    };
    set_params(world, e, label);
}

// ---------------------------------------------------------------------------------------------
// Input

fn extrude_picks(
    mut picks: MessageReader<PickRequest>,
    session: Option<Res<ExtrudeSession>>,
    mut arrow: ResMut<ArrowState>,
    replace: Option<Res<crate::replace_reference::ReplaceSession>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        return;
    };
    // P3D.4: the Replace reference dialog takes the picks while it is open.
    if replace.is_some() {
        picks.clear();
        return;
    }
    for p in picks.read() {
        if std::mem::take(&mut arrow.swallow_click) {
            continue;
        }
        let Some(pick) = p.0 else { continue };
        let field = s.field;
        commands.queue(move |world: &mut World| take_pick(world, field, pick));
    }
}

/// A pick goes to the active selection field.
fn take_pick(world: &mut World, field: ExtrudeField, pick: Pick) {
    let _ = take_pick_inner(world, field, pick);
}

/// What a pick on a part stands for, read from the parts before changing the parameters.
struct Picked {
    face: Option<(FaceRef, bool)>,
    vertex: Option<VertexRef>,
    edge: Option<EdgeRef>,
}

fn take_pick_inner(world: &mut World, field: ExtrudeField, pick: Pick) -> Option<()> {
    let s = world.get_resource::<ExtrudeSession>().cloned()?;
    let picked = {
        let cache = world.resource::<PartCache>();
        let face = match pick {
            Pick::Face(part, face) => (|| {
                let p = cache.part(part)?;
                let i = p.solid.faces.iter().position(|f| f.name == face)?;
                let seed = p.solid.face_point(i)?;
                Some((
                    FaceRef { part, face, seed },
                    p.solid.faces[i].plane.is_some(),
                ))
            })(),
            _ => None,
        };
        let vertex = match pick {
            Pick::Vertex(part, vertex) => cache
                .part(part)
                .and_then(|p| p.solid.vertex(&vertex))
                .map(|v| VertexRef {
                    part,
                    vertex,
                    point: v.point,
                }),
            _ => None,
        };
        let edge = match pick {
            Pick::Edge(part, edge) => {
                cache
                    .part(part)
                    .and_then(|p| p.solid.edge(&edge))
                    .map(|e| EdgeRef {
                        part,
                        edge,
                        seed: e.midpoint(),
                    })
            }
            _ => None,
        };
        Picked { face, vertex, edge }
    };
    // Nothing the extrude itself made can be its own input or target.
    let own = |op: cadrs_sketch::OpId| op == s.feature.0;
    match (field, pick) {
        (ExtrudeField::Input, Pick::Region(sketch, index)) => toggle_region(world, sketch, index),
        (ExtrudeField::Input, Pick::Feature(sketch)) => toggle_sketch(world, sketch),
        (ExtrudeField::Input, Pick::Face(_, face)) if !own(face.op) => {
            let (r, true) = picked.face? else { return None };
            let mut e = params(world)?;
            let label = if let Some(i) = e.faces.iter().position(|f| f.face == face) {
                e.faces.remove(i);
                "Deselect face"
            } else {
                e.faces.push(r);
                "Select face"
            };
            set_params(world, e, label);
        }
        (ExtrudeField::UpTo | ExtrudeField::SecondUpTo, pick) => {
            let mut e = params(world)?;
            let second = field == ExtrudeField::SecondUpTo;
            let end = if second {
                e.second.as_ref().map(|s| s.end)
            } else {
                Some(e.end)
            };
            let target = match (end, pick) {
                (Some(EndType::UpToFace), Pick::Face(_, face)) if !own(face.op) => {
                    picked.face.map(|(r, _)| UpTo::Face(r))
                }
                (Some(EndType::UpToPart), p) => p.part().map(UpTo::Part),
                (Some(EndType::UpToVertex), Pick::Vertex(..)) => picked.vertex.map(UpTo::Vertex),
                _ => None,
            }?;
            if second {
                if let Some(s) = &mut e.second {
                    s.up_to = Some(target);
                }
            } else {
                e.up_to = Some(target);
            }
            let check = (!second).then(|| (e.clone(), world.resource::<PartCache>().builds));
            set_params(world, e, "Select up to");
            if let Some(mut s) = world.get_resource_mut::<ExtrudeSession>() {
                s.flip_check = check;
            }
        }
        (ExtrudeField::Direction, pick) => {
            let dir = match pick {
                Pick::Edge(_, edge) if !own(edge.op()) => picked.edge.map(DirectionRef::Edge),
                Pick::Face(_, face) if !own(face.op) => match picked.face {
                    Some((r, true)) => Some(DirectionRef::FaceNormal(r)),
                    _ => None,
                },
                // P3.7 (PS12.3): a default plane's or a Plane feature's normal.
                Pick::Plane(k) => Some(DirectionRef::PlaneNormal(k.plane_ref())),
                Pick::Feature(f) => world
                    .get_resource::<ActiveDocument>()
                    .and_then(|d| {
                        cadrs_core::parts::plane_feature_ref(d.active_element()?.features(), f)
                    })
                    .map(DirectionRef::PlaneNormal),
                _ => None,
            }?;
            let mut e = params(world)?;
            e.direction = Some(dir);
            set_params(world, e, "Select direction");
        }
        (ExtrudeField::MergeScope, pick) => {
            let part = pick.part()?;
            edit_scope(world, "Merge scope", |scope| {
                if let Some(i) = scope.iter().position(|p| *p == part) {
                    scope.remove(i);
                } else {
                    scope.push(part);
                }
            });
        }
        _ => {}
    }
    Some(())
}

/// Changes the merge scope by hand: it starts from the parts found by themselves (while it
/// hasn't been edited) and from then on is exactly what it holds, even empty, so the parts
/// found don't come back. The operation stops following what the body touches too.
pub(crate) fn edit_scope(world: &mut World, label: &'static str, f: impl FnOnce(&mut Vec<PartId>)) {
    let Some(feature) = world.get_resource::<ExtrudeSession>().map(|s| s.feature) else {
        return;
    };
    let Some(mut e) = params(world) else { return };
    if !e.merge_scope_set && e.merge_scope.is_empty() {
        e.merge_scope = auto_scope(world.resource::<PartCache>(), feature, e.op);
    }
    let before = (e.merge_scope.clone(), e.merge_scope_set);
    f(&mut e.merge_scope);
    e.merge_scope_set = true;
    if let Some(mut s) = world.get_resource_mut::<ExtrudeSession>() {
        s.op_auto = false;
    }
    if (e.merge_scope.clone(), e.merge_scope_set) != before {
        set_params(world, e, label);
    }
}

/// The parts an Add (touching) or a Remove or Intersect (overlapping) finds by itself.
pub(crate) fn auto_scope(cache: &PartCache, feature: FeatureId, op: BooleanOp) -> Vec<PartId> {
    cache
        .contacts
        .get(&feature)
        .map(|c| match op {
            BooleanOp::Add => c.touches.clone(),
            _ => c.overlaps.clone(),
        })
        .unwrap_or_default()
}

/// A sketch picked in the feature list toggles as a whole (PS1.1).
fn toggle_sketch(world: &mut World, sketch: FeatureId) {
    let Some(mut e) = params(world) else { return };
    let name = world.get_resource::<ActiveDocument>().and_then(|d| {
        let f = d.active_element()?.feature(sketch)?;
        f.sketch()?;
        Some(f.name.clone())
    });
    let Some(name) = name else { return };
    let label = if let Some(i) = e.sketches.iter().position(|s| *s == sketch) {
        e.sketches.remove(i);
        format!("Deselect {name}")
    } else {
        // The whole sketch replaces its single regions.
        e.regions.retain(|r| r.sketch != sketch);
        e.sketches.push(sketch);
        format!("Select {name}")
    };
    set_params(world, e, label);
}

/// The faces, vertices, parts and edges the open dialog refers to (its input faces, its Up to
/// targets, its direction, its merge scope's parts) are highlighted in the view as selected, as Onshape shows a dialog's
/// picks.
fn show_references(
    session: Option<Res<ExtrudeSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    mut selection: ResMut<Selection>,
) {
    let (Some(s), Some(doc)) = (session, doc) else {
        return;
    };
    let Some(e) = doc
        .doc
        .element(s.element)
        .and_then(|el| el.feature(s.feature))
        .and_then(|f| f.extrude())
    else {
        return;
    };
    // The part a stored reference is on now (a boolean may have joined its part to another).
    let face_pick = |f: &FaceRef| {
        let part = cache
            .part(f.part)
            .filter(|p| p.solid.face(&f.face).is_some())
            .or_else(|| cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()))?;
        Some(Pick::Face(part.id, f.face))
    };
    let mut want: Vec<Pick> = e.faces.iter().filter_map(face_pick).collect();
    let ends = std::iter::once(e.up_to).chain(e.second.as_ref().map(|s| s.up_to));
    for u in ends.flatten() {
        let p = match u {
            UpTo::Face(f) => face_pick(&f),
            UpTo::Part(p) => cache.part(p).map(|_| Pick::Part(p)),
            UpTo::Vertex(v) => cache
                .parts
                .iter()
                .find(|p| p.solid.vertex(&v.vertex).is_some())
                .map(|p| Pick::Vertex(p.id, v.vertex)),
        };
        want.extend(p);
    }
    match e.direction {
        Some(DirectionRef::Edge(r)) => want.extend(
            cache
                .parts
                .iter()
                .find(|p| p.solid.edge(&r.edge).is_some())
                .map(|p| Pick::Edge(p.id, r.edge)),
        ),
        Some(DirectionRef::FaceNormal(f)) => want.extend(face_pick(&f)),
        // A sketch line stays highlighted too (P3.5: every reference of the dialog shows while
        // another field is active).
        Some(DirectionRef::SketchLine { sketch, curve }) => {
            want.push(Pick::SketchCurve(sketch, curve))
        }
        Some(DirectionRef::PlaneNormal(p)) => want.extend(crate::viewport::plane_pick(p)),
        Some(DirectionRef::Connector(c)) => want.extend(crate::pattern::connector_pick(&c)),
        None => {}
    }
    // The parts the boolean acts on (as the Merge scope lists them).
    if !matches!(e.op, BooleanOp::New) && e.body != cadrs_core::BodyType::Surface && !e.merge_all {
        let scope = if e.merge_scope_set || !e.merge_scope.is_empty() {
            e.merge_scope.clone()
        } else {
            auto_scope(&cache, s.feature, e.op)
        };
        want.extend(
            scope
                .into_iter()
                .filter(|p| cache.part(*p).is_some())
                .map(Pick::Part),
        );
    }
    if selection.0 != want {
        selection.0 = want;
    }
}

/// An "Up to" face, part or vertex picked behind the extrude's start: the extrude is flipped to
/// reach it, instead of failing with "… is behind the start of the extrude". Tried once per
/// pick, after the rebuild of that pick; nothing changes when the flip fails too.
fn flip_if_behind(
    session: Option<Res<ExtrudeSession>>,
    cache: Res<PartCache>,
    mut commands: Commands,
) {
    let Some((want, builds)) = session.as_ref().and_then(|s| s.flip_check.clone()) else {
        return;
    };
    if cache.rebuilding || cache.builds == builds {
        return;
    }
    let feature = session.map(|s| s.feature);
    let behind = feature
        .and_then(|f| cache.errors.get(&f))
        .is_some_and(|e| e.contains("is behind the start of the extrude"));
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<ExtrudeSession>() {
            s.flip_check = None;
        }
        // Not when something else has changed since the pick.
        if let Some(mut e) = params(world)
            && behind
            && e == want
        {
            e.flip = !e.flip;
            set_params(world, e, "Flip direction");
        }
    });
}

/// A new extrude picks New or Add by itself (PS5.2): Add once its body touches a part, New
/// while it doesn't (until a tab is clicked).
fn automatic_operation(
    session: Option<Res<ExtrudeSession>>,
    cache: Res<PartCache>,
    mut commands: Commands,
) {
    let Some(s) = session.filter(|s| s.op_auto) else {
        return;
    };
    if cache.rebuilding {
        return;
    }
    let touches = cache
        .contacts
        .get(&s.feature)
        .is_some_and(|c| !c.touches.is_empty());
    let feature = s.feature;
    commands.queue(move |world: &mut World| {
        let Some(mut e) = params(world) else { return };
        if !matches!(e.op, BooleanOp::New | BooleanOp::Add)
            || e.body == cadrs_core::BodyType::Surface
        {
            return;
        }
        let want = if touches {
            BooleanOp::Add
        } else {
            BooleanOp::New
        };
        // Only once the rebuild knows about this extrude's body.
        let known = world
            .resource::<PartCache>()
            .contacts
            .contains_key(&feature);
        if e.op != want && (known || want == BooleanOp::New) && !e.is_empty() {
            e.op = want;
            set_params(world, e, want.label());
        }
    });
}

/// Enter accepts, Esc cancels (an arrow drag first), Shift+E starts an extrude.
#[allow(clippy::too_many_arguments)]
fn extrude_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    session: Option<Res<ExtrudeSession>>,
    sketch: Option<Res<crate::sketch::SketchSession>>,
    kind: Res<crate::viewport::ActiveKind>,
    mut commands: Commands,
) {
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed
            || typing
            || ctrl
            || !q_dialogs.is_empty()
            || !q_menus.is_empty()
            || sketch.is_some()
        {
            continue;
        }
        match k.shortcut_code() {
            KeyCode::KeyE
                if shift
                    && session.is_none()
                    && *kind == crate::viewport::ActiveKind::PartStudio =>
            {
                commands.queue(begin_extrude)
            }
            KeyCode::KeyW
                if shift
                    && session.is_none()
                    && *kind == crate::viewport::ActiveKind::PartStudio =>
            {
                commands.queue(crate::revolve::begin_revolve)
            }
            // P3.6: Shift+F, Fillet.
            KeyCode::KeyF
                if shift
                    && session.is_none()
                    && *kind == crate::viewport::ActiveKind::PartStudio =>
            {
                commands.queue(|world: &mut World| {
                    crate::applied::begin(world, crate::applied::AppliedKind::Fillet)
                })
            }
            KeyCode::Enter | KeyCode::NumpadEnter if session.is_some() => {
                commands.queue(accept_extrude)
            }
            KeyCode::Escape if session.is_some() => commands.queue(|world: &mut World| {
                if world.resource::<ArrowState>().drag.is_some() {
                    end_drag(world, false);
                } else if world
                    .resource::<crate::revolve::AngleArrow>()
                    .drag
                    .is_some()
                {
                    crate::revolve::end_angle_drag(world, false);
                } else {
                    cancel_extrude(world);
                }
            }),
            _ => {}
        }
    }
}

/// A round step (mm) for dragging: at least about 2.5 px on screen (whole millimetres at the
/// default isometric zoom, where the normal is about 2.9 px/mm).
pub fn snap_step(px_per_mm: f32) -> f64 {
    let min = 2.5 / px_per_mm.max(1e-6) as f64;
    [0.1, 0.2, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0]
        .into_iter()
        .find(|s| *s >= min)
        .unwrap_or(100.0)
}

/// The depth for a drag that moved the pointer by `delta` px: the signed depth along the plane
/// normal, snapped; negative means flipped. Never zero.
pub fn dragged_depth(start_signed: f64, normal_px: Vec2, delta: Vec2) -> (f64, bool) {
    let len2 = normal_px.length_squared().max(1e-9);
    let signed = start_signed + (delta.dot(normal_px) / len2) as f64;
    let step = snap_step(normal_px.length());
    let mut depth = (signed.abs() / step).round() * step;
    if depth < step {
        depth = step;
    }
    // Keep the snapped value tidy (0.1 steps).
    depth = (depth * 1e6).round() / 1e6;
    (depth, signed < 0.0)
}

/// Grabs, drags and releases the arrow manipulator.
#[allow(clippy::too_many_arguments)]
fn arrow_pointer(
    mut inputs: MessageReader<PointerInput>,
    session: Option<Res<ExtrudeSession>>,
    mut arrow: ResMut<ArrowState>,
    mut over: ResMut<PartOverride>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<ViewportView>,
    drag: Res<crate::viewport::ViewportDrag>,
    units: Res<crate::WorkspaceUnits>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        inputs.clear();
        return;
    };
    let pointer = drag.pointer();
    arrow.hovered = arrow
        .base_tip
        .is_some_and(|(a, b)| segment_distance(pointer, a, b) <= ARROW_GRAB);
    arrow.second_hovered = !arrow.hovered
        && arrow
            .second_base_tip
            .is_some_and(|(a, b)| segment_distance(pointer, a, b) <= ARROW_GRAB);
    let params = doc.as_ref().and_then(|d| {
        d.doc
            .element(s.element)?
            .feature(s.feature)?
            .extrude()
            .cloned()
    });
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                let hit = arrow
                    .base_tip
                    .is_some_and(|(a, b)| segment_distance(pos, a, b) <= ARROW_GRAB);
                let hit2 = !hit
                    && arrow
                        .second_base_tip
                        .is_some_and(|(a, b)| segment_distance(pos, a, b) <= ARROW_GRAB);
                let axis = if hit { arrow.axis } else { arrow.second_axis };
                if let (true, Some(p), Some((n, signed, anchor))) =
                    (hit || hit2, params.clone(), axis)
                {
                    let normal_px = view.view.project_vector(n);
                    if normal_px.length() > 0.05 {
                        arrow.drag = Some(ArrowDrag {
                            start: pos,
                            start_signed: signed,
                            normal_px,
                            extrude: p,
                            second: hit2,
                            anchor,
                        });
                    }
                }
            }
            PointerAction::Move { .. } => {
                if let Some(d) = arrow.drag.as_mut()
                    && d.second
                {
                    // The second end goes the other way from the sketch plane; dragging it back
                    // through the plane stops at the smallest step.
                    let (depth, flip) = dragged_depth(d.start_signed, d.normal_px, pos - d.start);
                    let depth = if flip {
                        snap_step(d.normal_px.length())
                    } else {
                        depth
                    };
                    if let Some(end) = d.extrude.second.as_mut()
                        && (end.depth != depth || end.end != EndType::Blind)
                    {
                        end.end = EndType::Blind;
                        end.up_to = None;
                        end.offset = None;
                        end.depth = depth;
                        end.depth_expr = units.0.with_unit(depth, Quantity::Length);
                    }
                    let want = Some((s.feature, d.extrude.clone()));
                    if over.extrude != want {
                        over.extrude = want;
                    }
                } else if let Some(d) = arrow.drag.as_mut() {
                    let (depth, flip) = dragged_depth(d.start_signed, d.normal_px, pos - d.start);
                    if (depth, flip) != (d.extrude.depth, d.extrude.flip)
                        || d.extrude.end != EndType::Blind
                    {
                        // Dragging an "Up to" end makes it Blind at the dragged depth.
                        d.extrude.end = EndType::Blind;
                        d.extrude.up_to = None;
                        d.extrude.offset = None;
                        d.extrude.depth = depth;
                        d.extrude.flip = flip;
                        d.extrude.depth_expr = units.0.with_unit(depth, Quantity::Length);
                    }
                    let want = Some((s.feature, d.extrude.clone()));
                    if over.extrude != want {
                        over.extrude = want;
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) if arrow.drag.is_some() => {
                arrow.swallow_click = true;
                commands.queue(|world: &mut World| end_drag(world, true));
            }
            PointerAction::Cancel => {
                commands.queue(|world: &mut World| end_drag(world, false));
            }
            _ => {}
        }
    }
}

/// Ends an arrow drag: with `keep`, records the dragged depth as one undo step.
fn end_drag(world: &mut World, keep: bool) {
    let Some(d) = world.resource_mut::<ArrowState>().drag.take() else {
        return;
    };
    let changed = params(world).is_some_and(|p| p != d.extrude);
    if keep && changed {
        set_params(world, d.extrude, "Drag depth");
    }
    world.resource_mut::<PartOverride>().extrude = None;
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The origin and unit normal of the first extruded region's sketch plane (or picked face).
fn sketch_origin_normal(
    doc: Option<&ActiveDocument>,
    cache: &PartCache,
    s: &ExtrudeSession,
    e: &ExtrudeFeature,
) -> (Vec3, Vec3) {
    let frame = doc.and_then(|d| {
        let sketch = e
            .regions
            .first()
            .map(|r| r.sketch)
            .or(e.sketches.first().copied())?;
        Some(
            d.doc
                .element(s.element)?
                .feature(sketch)?
                .sketch()?
                .plane?
                .frame(),
        )
    });
    let v = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    match frame {
        Some(f) => (v(f.origin), v(f.normal()).normalize()),
        // A face profile: where the rebuild started the sweep.
        None => match built_start(cache, s, e) {
            Some((o, n)) => (o, n),
            None => match face_frame(cache, e) {
                Some(f) => (v(f.origin), v(f.normal()).normalize()),
                None => (Vec3::ZERO, sketch_normal(doc, cache, s, e)),
            },
        },
    }
}

/// Where the last rebuild started this extrude's first sweep and the way it went before the
/// flip (the faces a Remove starts from are gone from the parts after it, so they can't tell).
fn built_start(cache: &PartCache, s: &ExtrudeSession, e: &ExtrudeFeature) -> Option<(Vec3, Vec3)> {
    let (o, d) = cache.axes.get(&s.feature)?;
    let v = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let d = v(*d).normalize_or_zero();
    (d != Vec3::ZERO).then(|| (v(*o), if e.flip { -d } else { d }))
}

/// The plane of the first part face an extrude extrudes (its outward normal is the way the
/// extrude goes, PS4.2), where the parts on screen still show it (an Add's; a Remove consumes
/// it, see [`built_start`]).
fn face_frame(cache: &PartCache, e: &ExtrudeFeature) -> Option<cadrs_sketch::PlaneFrame> {
    let f = e.faces.first()?;
    cache
        .parts
        .iter()
        .filter(|p| p.id == f.part)
        .chain(cache.parts.iter())
        .find_map(|p| p.solid.face(&f.face)?.plane)
}

/// The unit direction an extrude goes before its flip: its Direction (a part edge, a planar
/// face's normal, a sketch line), else its sketch plane's normal.
fn extrude_direction(
    doc: Option<&ActiveDocument>,
    cache: &PartCache,
    s: &ExtrudeSession,
    e: &ExtrudeFeature,
) -> Vec3 {
    let v = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let picked = e.direction.and_then(|d| match d {
        DirectionRef::Edge(r) => {
            let edge = cache
                .parts
                .iter()
                .chain(cache.tool.iter())
                .find_map(|p| p.solid.edge(&r.edge))?;
            let (a, b) = (edge.points.first()?, edge.points.last()?);
            Some(v(*b) - v(*a))
        }
        DirectionRef::FaceNormal(f) => {
            let face = cache.parts.iter().find_map(|p| p.solid.face(&f.face))?;
            Some(v(face.plane?.normal()))
        }
        DirectionRef::SketchLine { sketch, curve } => {
            let sk = doc?.doc.element(s.element)?.feature(sketch)?.sketch()?;
            let frame = sk.plane?.frame();
            let cadrs_sketch::CurveKind::Line { a, b } = sk.geometry.curves.get(curve)?.kind else {
                return None;
            };
            Some(v(frame.to_world(sk.geometry.pos(b))) - v(frame.to_world(sk.geometry.pos(a))))
        }
        DirectionRef::Connector(c) => {
            let features = doc?.doc.element(s.element)?.features();
            let f = cadrs_core::mate::frame(&c, features, &cache.parts, &cache.connectors).ok()?;
            Some(v(f.normal()))
        }
        DirectionRef::PlaneNormal(p) => Some(v(p.frame().normal())),
    });
    picked
        .filter(|d| d.length() > 1e-9)
        .map(Vec3::normalize)
        .unwrap_or_else(|| sketch_normal(doc, cache, s, e))
}

/// The world normal of the first extruded region's sketch plane.
fn sketch_normal(
    doc: Option<&ActiveDocument>,
    cache: &PartCache,
    s: &ExtrudeSession,
    e: &ExtrudeFeature,
) -> Vec3 {
    let n = doc
        .and_then(|d| {
            let sketch = e
                .regions
                .first()
                .map(|r| r.sketch)
                .or(e.sketches.first().copied())?;
            d.doc.element(s.element)?.feature(sketch)?.sketch()?.plane
        })
        .map(|p| p.frame().normal());
    if let Some(n) = n {
        return Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32);
    }
    // A face profile (P3.11 fix round 1: the funnel's spout arrow pointed up, along +Z, while it
    // extruded the loft's bottom face down): its outward normal, as the rebuild swept it (a
    // Remove's faces are gone from the parts after it: its arrow pointed along −Z), else as the
    // parts show the face.
    built_start(cache, s, e)
        .map(|(_, n)| n)
        .or_else(|| {
            face_frame(cache, e).map(|f| {
                Vec3::new(
                    f.normal()[0] as f32,
                    f.normal()[1] as f32,
                    f.normal()[2] as f32,
                )
            })
        })
        .unwrap_or(Vec3::Z)
}

// ---------------------------------------------------------------------------------------------
// Viewport

/// Selected regions: an orange outline over a translucent orange fill; a hovered region gets a
/// paler outline (`screens/23`). Outside the Extrude dialog, the regions selected in the view
/// or the sketch ([`crate::region_select`]) get a light orange fill, as in
/// `intro-to-sketching/ex1-step8.png`.
#[allow(clippy::too_many_arguments)]
fn draw_regions(
    session: Option<Res<ExtrudeSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    highlight: Res<PlaneHighlight>,
    plain: Res<crate::region_select::SelectedRegions>,
    sketching: Option<Res<crate::sketch::SketchSession>>,
    mut outline: Gizmos<RegionGizmos>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fill: Local<FillState>,
    q_fill: Query<(), With<RegionFill>>,
    failed: Res<crate::parts::FailedReferences>,
    applied: Option<Res<crate::applied::AppliedSession>>,
    selection: Res<crate::viewport::Selection>,
    mut commands: Commands,
) {
    // The fill entity goes away with the document.
    if fill.as_ref().is_some_and(|(e, ..)| !q_fill.contains(*e)) {
        *fill = None;
    }
    // While sketching, the wash is drawn with the edited sketch's translucent fill over the parts
    // (P3.8, `crate::sketch_draw`) and after it (its larger depth bias sorts it nearer), so the
    // selected region stays orange (`course_s17_regions_without_trim` 02); otherwise behind parts.
    let layer = if sketching.is_some() {
        crate::viewport::OVERLAY_LAYER
    } else {
        crate::viewport::OCCLUDED_LAYER
    };
    if let Some((e, _, mat, _)) = fill.as_ref() {
        commands
            .entity(*e)
            .insert(bevy::camera::visibility::RenderLayers::layer(layer));
        let bias = wash_bias(layer);
        if materials.get(mat).is_some_and(|m| m.depth_bias != bias)
            && let Some(mut m) = materials.get_mut(mat)
        {
            m.depth_bias = bias;
        }
    }
    let selected: SelectedRegions = match (&session, &doc) {
        (Some(s), Some(d)) => {
            let el = d.doc.element(s.element);
            let f = el.and_then(|e| e.feature(s.feature));
            let (regions, sketches) = f.map(|f| f.input_regions()).unwrap_or((&[], &[]));
            let mut v: SelectedRegions = regions
                .iter()
                .map(|r| (r.sketch, r.curves.clone(), r.seed))
                .collect();
            // A whole sketch: the regions it extrudes (PS1.1).
            for sk in sketches.iter().copied() {
                let Some(g) = el.and_then(|el| el.feature(sk)).and_then(|f| f.sketch()) else {
                    continue;
                };
                for r in cadrs_core::rebuild::whole_sketch_regions(&g.geometry) {
                    let rr = RegionRef::new(sk, &r);
                    v.push((sk, rr.curves, rr.seed));
                }
            }
            v
        }
        _ => Vec::new(),
    };
    // P3I.6 (exercise E1, `ex1-importing-dxf-bend/step-04.png`): the Sheet metal model's Thicken
    // regions are picked regions too, orange like the Extrude dialog's.
    let thicken: SelectedRegions = match (&session, &applied, &doc) {
        (None, Some(a), Some(d)) => {
            let el = d.doc.element(a.element);
            match el.and_then(|e| e.feature(a.feature)).map(|f| &f.kind) {
                Some(cadrs_core::FeatureKind::SheetMetalModel(x))
                    if x.operation == cadrs_core::sheetmetal::SheetMetalOp::Thicken =>
                {
                    let mut v: SelectedRegions = x
                        .regions
                        .iter()
                        .map(|r| (r.sketch, r.curves.clone(), r.seed))
                        .collect();
                    for sk in x.region_sketches.iter().copied() {
                        let Some(g) = el.and_then(|el| el.feature(sk)).and_then(|f| f.sketch())
                        else {
                            continue;
                        };
                        for r in cadrs_core::rebuild::whole_sketch_regions(&g.geometry) {
                            let rr = RegionRef::new(sk, &r);
                            v.push((sk, rr.curves, rr.seed));
                        }
                    }
                    v
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    };
    let extruding = session.is_some() || !thicken.is_empty();
    let selected = if thicken.is_empty() {
        selected
    } else {
        thicken
    };
    // The selected region's outline in `screens/23`; a plain selection's in `ex1-step8`.
    // A failing feature's regions are red (P3.7, PS20.5).
    let orange = if failed.0 {
        crate::parts::FAILED
    } else if extruding {
        Color::srgb_u8(0xe8, 0xb0, 0x4a)
    } else {
        Color::srgb_u8(0xfe, 0xbf, 0x4a)
    };
    let mut tris: Vec<[f32; 3]> = Vec::new();
    let mut plain_key: Vec<(FeatureId, usize)> = Vec::new();
    // Sketches on a flat pattern show their regions in the flat view (`crate::flat_ui`).
    for sr in cache
        .regions
        .iter()
        .filter(|sr| !cache.flat_sketches.contains(&sr.sketch))
    {
        for (i, r) in sr.regions.iter().enumerate() {
            let mut curves = r.curves.clone();
            curves.sort();
            curves.dedup();
            let is_selected = if extruding {
                selected
                    .iter()
                    .any(|(s, c, seed)| *s == sr.sketch && *c == curves && r.contains(*seed))
            } else {
                // A sketch selected whole (its feature row, or a right-click on it) fills every
                // region orange, as Onshape's.
                plain.contains(sr.sketch, i) || selection.contains(Pick::Feature(sr.sketch))
            };
            if is_selected && !extruding {
                plain_key.push((sr.sketch, i));
            }
            let hovered = highlight.is_hovered(Pick::Region(sr.sketch, i as u32));
            if !is_selected && !hovered {
                continue;
            }
            let w = |p: cadrs_sketch::Vec2| {
                let q = sr.frame.to_world(p);
                Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)
            };
            let color = if hovered && !is_selected {
                Color::srgb_u8(0xff, 0xc6, 0x85)
            } else {
                orange
            };
            for ring in std::iter::once(&r.outer).chain(&r.holes) {
                let mut pts: Vec<Vec3> = ring.iter().map(|p| w(*p)).collect();
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
                outline.linestrip(pts, color);
            }
            if is_selected {
                let (verts, idx) = r.triangulate();
                for k in idx {
                    tris.push(w(verts[k as usize]).to_array());
                }
            }
        }
    }
    // The fill: one mesh, rebuilt when the selection changes.
    // Rebuilt when the selection changes, or when its regions become available (the frame an
    // edit opens, the used sketch's regions come back a frame later).
    // The triangles' hash, not their count: a square moved to a rectangle keeps its two
    // triangles (`pcb_board_from_sketch` 03 kept the square's fill).
    let shape = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for t in &tris {
            t.map(f32::to_bits).hash(&mut h);
        }
        failed.0.hash(&mut h);
        cadrs_ui::dark_in_force().hash(&mut h);
        h.finish()
    };
    let key: FillKey = (selected.clone(), plain_key, shape);
    let rebuild = fill.as_ref().is_none_or(|(.., k)| *k != key);
    if !rebuild {
        return;
    }
    // An orange wash over the region, drawn over the preview (`screens/23`: the region shows
    // tan through the dark preview); a plain selection is a light orange over the sketch's
    // grey fill (`ex1-step8`: #face7a, with the construction lines just showing through).
    // While sketching, lighter, so the geometry inside the region stays visible.
    let wash = if failed.0 {
        crate::parts::FAILED.with_alpha(0.35)
    } else if extruding {
        Color::srgba_u8(0xe1, 0xa5, 0x0a, 0x4d)
    } else if sketching.is_some() {
        Color::srgba_u8(0xfd, 0xcc, 0x6e, 0x8c)
    } else {
        // In dark, a deeper amber, so the light sketch and dimension lines read over it.
        cadrs_ui::by_scheme(
            Color::srgba_u8(0xfd, 0xcc, 0x6e, 0xe6),
            Color::srgba_u8(0x9a, 0x67, 0x16, 0xd9),
        )
    };
    // Hidden while there's nothing to fill: the occluded camera renders only while something on
    // its layer shows (`crate::viewport::occluded_camera_when_drawn`).
    let shown = if tris.is_empty() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    let mesh = region_fill_mesh(tris);
    match fill.as_mut() {
        Some((e, handle, mat, k)) => {
            commands.entity(*e).insert(shown);
            if let Some(mut m) = meshes.get_mut(&*handle) {
                *m = mesh;
            }
            let bias = wash_bias(layer);
            if let Some(mut m) = materials.get_mut(&*mat)
                && (m.base_color != wash || m.depth_bias != bias)
            {
                m.base_color = wash;
                m.depth_bias = bias;
            }
            *k = key;
        }
        None => {
            let handle = meshes.add(mesh);
            let material = materials.add(StandardMaterial {
                base_color: wash,
                unlit: true,
                cull_mode: None,
                double_sided: true,
                alpha_mode: AlphaMode::Blend,
                // In front of a face it lies on, and of the edited sketch's fill.
                depth_bias: wash_bias(layer),
                ..default()
            });
            let e = commands
                .spawn((
                    Name::new("extrude-region-fill"),
                    RegionFill,
                    crate::plane_display::LabelOccluder,
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::IDENTITY,
                    shown,
                    bevy::camera::visibility::RenderLayers::layer(layer),
                    DespawnOnExit(AppState::Document),
                ))
                .id();
            *fill = Some((e, handle, material, key));
        }
    }
}

/// The wash's depth bias: over the edited sketch's translucent fill while sketching (on the
/// overlay, which has no parts' depth); otherwise a hair in front of a face the region lies on
/// and of a shown sketch's fill, but not through a part (P3I.6: 1001 showed through sheets).
fn wash_bias(layer: usize) -> f32 {
    if layer == crate::viewport::OVERLAY_LAYER {
        1001.0
    } else {
        crate::sketch_draw::FILL_BIAS + 8.0
    }
}

/// The region fill entity, its mesh and material, and what it was built from.
type FillState = Option<(Entity, Handle<Mesh>, Handle<StandardMaterial>, FillKey)>;

/// The selected regions (sketch, outer curves and seed point), which the fill was built for.
type SelectedRegions = Vec<(FeatureId, Vec<cadrs_sketch::CurveId>, cadrs_sketch::Vec2)>;

/// What a region fill was built from: the selected regions (the Extrude dialog's, and the plain
/// selection's) and its triangles' hash.
type FillKey = (SelectedRegions, Vec<(FeatureId, usize)>, u64);

/// The selected regions' fill.
#[derive(Component)]
struct RegionFill;

/// Region outlines: 3 px, over everything (`screens/23`).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct RegionGizmos;

/// The selected regions' outline and wash: over everything while a sketch is edited (they sit
/// on its opaque fill), hidden behind parts otherwise (the Extrude dialog's regions,
/// `ex1-step4`).
fn region_layers(
    session: Option<Res<crate::sketch::SketchSession>>,
    mut store: ResMut<GizmoConfigStore>,
    q: Query<(Entity, &bevy::camera::visibility::RenderLayers), With<RegionFill>>,
    mut commands: Commands,
) {
    use bevy::camera::visibility::RenderLayers;
    let (layer, bias) = if session.is_some() {
        (crate::viewport::OVERLAY_LAYER, -1.0)
    } else {
        (crate::viewport::OCCLUDED_LAYER, -3e-4)
    };
    let want = RenderLayers::layer(layer);
    let (config, _) = store.config::<RegionGizmos>();
    if config.render_layers != want {
        let (config, _) = store.config_mut::<RegionGizmos>();
        config.render_layers = want.clone();
        config.depth_bias = bias;
    }
    for (e, l) in &q {
        if *l != want {
            commands.entity(e).insert(want.clone());
        }
    }
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<RegionGizmos>();
    config.line.width = 3.0;
    // Over the sketch's own (grey) edges, like the selected region's outline in `ex1-step8`,
    // and hidden behind parts (`ex1-step4`).
    config.depth_bias = -3e-4;
    config.render_layers =
        bevy::camera::visibility::RenderLayers::layer(crate::viewport::OCCLUDED_LAYER);
}

fn region_fill_mesh(tris: Vec<[f32; 3]>) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let positions = if tris.is_empty() {
        vec![[0.0; 3]; 3]
    } else {
        tris
    };
    let n = positions.len() as u32;
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n as usize])
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(Indices::U32((0..n).collect()))
}

/// Places the arrow manipulator at the middle of the preview's far face, pointing the way the
/// extrusion goes.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn place_arrow(
    session: Option<Res<ExtrudeSession>>,
    cache: Res<PartCache>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    mut arrow: ResMut<ArrowState>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q: Query<(
        Entity,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
        &ArrowNode,
    )>,
    mut q_line: Query<(&ArrowLine, &mut ImageNode)>,
    mut commands: Commands,
) {
    let target = session.as_ref().and_then(|s| {
        let e = arrow.drag.as_ref().map(|d| d.extrude.clone()).or_else(|| {
            doc.as_ref()?
                .doc
                .element(s.element)?
                .feature(s.feature)?
                .extrude()
                .cloned()
        })?;
        if cache.errors.contains_key(&s.feature) {
            return None;
        }
        // The middle of the far cap(s) this extrude made: on its parts, or on the Add
        // preview's body (P3.3 judge: the arrow was missing from every Add preview).
        let mut sum = Vec3::ZERO;
        let mut n = 0.0;
        let faces = cache
            .parts
            .iter()
            .chain(cache.tool.iter())
            .filter(|p| p.features.contains(&s.feature))
            .flat_map(|p| p.solid.faces.iter());
        for f in faces {
            if let cadrs_sketch::FaceOrigin::Cap { end: true, .. } = f.name.origin
                && f.name.op == s.feature.0
                && let Some(l) = crate::parts::outer_loop(&f.loops)
            {
                for p in l {
                    sum += Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
                    n += 1.0;
                }
            }
        }
        // Along the extrude's direction (its Direction, else the sketch normal), P3.3 judge.
        let base = extrude_direction(doc.as_deref(), &cache, s, &e);
        let dir = if e.flip { -base } else { base };
        if n == 0.0 {
            // A surface has no caps: the middle of its far edges.
            let pts: Vec<Vec3> = cache
                .parts
                .iter()
                .chain(cache.tool.iter())
                .filter(|p| p.features.contains(&s.feature))
                .flat_map(|p| p.solid.edges.iter())
                .filter(|e| e.name.faces.iter().any(|f| f.op == s.feature.0))
                .flat_map(|e| {
                    e.points
                        .iter()
                        .map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
                })
                .collect();
            let far = pts
                .iter()
                .map(|p| p.dot(dir))
                .fold(f32::NEG_INFINITY, f32::max);
            let tol = 1e-4 * (1.0 + far.abs());
            for p in pts.iter().filter(|p| (p.dot(dir) - far).abs() <= tol) {
                sum += *p;
                n += 1.0;
            }
        }
        let dragged = arrow.drag.as_ref().filter(|d| !d.second);
        if n == 0.0 && dragged.is_none() {
            return None;
        }
        // How far the end is now, along the direction (signed; negative when flipped): the
        // depth of a Blind end, measured for the others (a drag makes them Blind).
        // While dragged, the arrow moves from where it was by the depth's change (the cap moves
        // along the direction), not waiting for the preview's rebuild.
        let anchor = match dragged {
            Some(d) => {
                d.anchor
                    + base * ((if e.flip { -e.depth } else { e.depth }) - d.start_signed) as f32
            }
            None => sum / n,
        };
        let signed = if e.end == EndType::Blind {
            if e.flip { -e.depth } else { e.depth }
        } else {
            let (o, nrm) = sketch_origin_normal(doc.as_deref(), &cache, s, &e);
            let along = base.dot(nrm);
            if along.abs() < 1e-6 {
                return None;
            }
            ((anchor - o).dot(nrm) / along) as f64
        };
        Some((anchor, dir, base, signed))
    });
    // The second end's arrow (P3.5): at the middle of the start cap (the second end's far face),
    // pointing away from the first end.
    let second = session
        .as_ref()
        .zip(target)
        .and_then(|(s, (_, dir, _, _))| {
            let e = arrow.drag.as_ref().map(|d| d.extrude.clone()).or_else(|| {
                doc.as_ref()?
                    .doc
                    .element(s.element)?
                    .feature(s.feature)?
                    .extrude()
                    .cloned()
            })?;
            let end = e.second.as_ref()?;
            if e.symmetric {
                return None;
            }
            let mut sum = Vec3::ZERO;
            let mut n = 0.0;
            for p in cache
                .parts
                .iter()
                .chain(cache.tool.iter())
                .filter(|p| p.features.contains(&s.feature))
            {
                for f in &p.solid.faces {
                    if let cadrs_sketch::FaceOrigin::Cap { end: false, .. } = f.name.origin
                        && f.name.op == s.feature.0
                        && let Some(l) = crate::parts::outer_loop(&f.loops)
                    {
                        for q in l {
                            sum += Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32);
                            n += 1.0;
                        }
                    }
                }
            }
            let dragged = arrow.drag.as_ref().filter(|d| d.second);
            if n == 0.0 && dragged.is_none() {
                return None;
            }
            let dir2 = -dir;
            let anchor = match dragged {
                Some(d) => d.anchor + dir2 * (end.depth - d.start_signed) as f32,
                None => sum / n,
            };
            let signed = if end.end == EndType::Blind {
                end.depth
            } else {
                let (o, _) = sketch_origin_normal(doc.as_deref(), &cache, s, &e);
                (anchor - o).dot(dir2) as f64
            };
            Some((anchor, dir2, signed))
        });
    arrow.second_axis = second.map(|(anchor, d, signed)| (d, signed, anchor));
    arrow.second_base_tip = second.and_then(|(anchor, d, _)| {
        let p = view.view.project_vector(d);
        (p.length() >= 0.05).then(|| {
            let b = rect.to_screen(view.view.project(anchor));
            (b, b + p.normalize() * ARROW_LEN)
        })
    });
    let second_active = arrow.second_hovered || arrow.drag.as_ref().is_some_and(|d| d.second);
    let second_tip = arrow.second_base_tip;
    arrow.axis = target.map(|(anchor, _, base, signed)| (base, signed, anchor));
    let Some((anchor, dir, _, _)) = target else {
        arrow.base_tip = None;
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let base = rect.to_screen(view.view.project(anchor));
    let d = view.view.project_vector(dir);
    if d.length() * view.view.scale < 0.05 {
        // Looking straight along the extrusion: no arrow to grab. (Its on-screen share, not px
        // per mm, which hid the arrow on large parts zoomed out.)
        arrow.base_tip = None;
        for (_, _, _, mut vis, _) in &mut q {
            vis.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    let u = d.normalize();
    let tip = base + u * ARROW_LEN;
    arrow.base_tip = Some((base, tip));
    let active = arrow.hovered || arrow.drag.as_ref().is_some_and(|d| !d.second);
    let arrows: [(Option<(Vec2, Vec2)>, bool); 2] =
        [(Some((base, tip)), active), (second_tip, second_active)];
    if q.iter().count() < 2 {
        let Some(area) = q_area.iter().next() else {
            return;
        };
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        for i in 0..2 {
            let e = commands.spawn(arrow_node(i)).id();
            commands.entity(area).add_child(e);
        }
        return;
    }
    for (_, mut node, mut transform, mut vis, which) in &mut q {
        let (bt, active) = arrows[which.0];
        let Some((base, tip)) = bt else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let u = (tip - base).normalize_or_zero();
        // The icon points up: turn it to the arrow's screen direction; its center is the arrow's
        // middle.
        let center = (base + tip) / 2.0 - rect.0.min;
        let (l, t) = (
            Val::Px(center.x - ARROW_LEN / 2.0),
            Val::Px(center.y - ARROW_LEN / 2.0),
        );
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
        let angle = u.x.atan2(-u.y);
        let want = UiTransform {
            rotation: Rot2::radians(angle),
            ..default()
        };
        if *transform != want {
            *transform = want;
        }
        vis.set_if_neq(Visibility::Inherited);
        let c = if active {
            Color::srgb_u8(0xff, 0xb4, 0x5a)
        } else {
            Color::WHITE
        };
        for (line, mut img) in &mut q_line {
            if line.0 == which.0 && img.color != c {
                img.color = c;
            }
        }
    }
}

/// An arrow manipulator's node: `extrude-arrow` (the first end) or `extrude-arrow-2`.
fn arrow_node(i: usize) -> impl Bundle {
    (
        Name::new(if i == 0 {
            "extrude-arrow".to_string()
        } else {
            "extrude-arrow-2".to_string()
        }),
        ArrowNode(i),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(ARROW_LEN),
            height: Val::Px(ARROW_LEN),
            ..default()
        },
        UiTransform::default(),
        Visibility::Hidden,
        Pickable::IGNORE,
        ZIndex(-3),
        DespawnOnExit(AppState::Document),
        children![
            (
                cadrs_ui::icon::icon_in(
                    "manipulator-arrow-halo",
                    ARROW_LEN,
                    Color::srgba_u8(0x3c, 0x46, 0x4e, 0xb0),
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                ),
                Pickable::IGNORE,
            ),
            (
                ArrowLine(i),
                cadrs_ui::icon::icon_in(
                    "manipulator-arrow-line",
                    ARROW_LEN,
                    Color::WHITE,
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                ),
                Pickable::IGNORE,
            ),
        ],
    )
}

/// The name a face shows in a selection field: "Face of Part 1".
pub fn face_label(cache: &PartCache, feature: FeatureId) -> String {
    let name = cache
        .part_of_feature(feature)
        .and_then(|p| cache.part_name(p.id))
        .unwrap_or("part");
    format!("Face of {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_snaps_and_flips() {
        // 4 px per mm straight up the screen.
        let n = Vec2::new(0.0, -4.0);
        // Up 40 px: 10 mm deeper (steps of 1 mm at this zoom).
        let (d, flip) = dragged_depth(25.0, n, Vec2::new(0.0, -40.0));
        assert_eq!((d, flip), (35.0, false));
        // Snapped to whole mm.
        let (d, _) = dragged_depth(25.0, n, Vec2::new(0.0, -41.0));
        assert_eq!(d, 35.0);
        // Down past the sketch plane: flipped.
        let (d, flip) = dragged_depth(25.0, n, Vec2::new(0.0, 140.0));
        assert_eq!((d, flip), (10.0, true));
        // Never zero.
        let (d, _) = dragged_depth(25.0, n, Vec2::new(0.0, 100.0));
        assert!(d > 0.0);
        // Zoomed out, the steps grow.
        assert_eq!(snap_step(0.5), 5.0);
        assert_eq!(snap_step(4.0), 1.0);
        // The default isometric view: whole millimetres (8 mm up from 25 is 33).
        assert_eq!(snap_step(2.88), 1.0);
        assert_eq!(
            dragged_depth(25.0, Vec2::new(0.0, -2.8832), Vec2::new(0.0, -23.0)).0,
            33.0
        );
        assert_eq!(snap_step(20.0), 0.2);
    }

    #[test]
    fn depth_expressions() {
        assert_eq!(
            cadrs_sketch::units::eval("20 + 5*2", Quantity::Length),
            Ok(30.0)
        );
        assert_eq!(
            cadrs_sketch::units::eval("1 in", Quantity::Length),
            Ok(25.4)
        );
        assert!(cadrs_sketch::units::eval("20 +", Quantity::Length).is_err());
    }

    #[test]
    fn arrow_grab_distance() {
        let (a, b) = (Vec2::ZERO, Vec2::new(0.0, -48.0));
        assert!(segment_distance(Vec2::new(3.0, -20.0), a, b) < ARROW_GRAB);
        assert!(segment_distance(Vec2::new(20.0, -20.0), a, b) > ARROW_GRAB);
    }
}
