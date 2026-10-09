//! Constraints in the sketch (M6): the solver's analysis (status colours, conflicts), the
//! constraint tools, dragging, glyph menus and the conflict banner.
//!
//! - **Analysis:** whenever the edited sketch changes, [`cadrs_sketch::solve::analyze`] gives
//!   each entity's status: blue (under-constrained), black (fully constrained) or red
//!   (involved in a conflicting constraint). Every sketch of the Part Studio is also checked for
//!   conflicts, so its feature-list row (and the dialog title) turn red, as in `screens/15`.
//! - **Constraint tools** (Constraints ▾ and their keys, `constraints.md`): with entities
//!   selected, the tool applies to them at once and the selection clears. With nothing
//!   selected the tool stays active: each set of picks that fits gets the constraint, until Esc
//!   or another tool.
//! - **Dragging** (no tool): a dragged point follows the cursor, a dragged line moves with it,
//!   a dragged circle or arc changes its radius; the solver keeps the constraints every frame.
//!   The live geometry is shown in the document without history; releasing records it as one
//!   "Drag" step through the command layer.
//! - **Glyphs:** click to select (Delete removes the constraint), right-click → Delete; a
//!   hovered glyph highlights its geometry.
//! - **Banner:** "Some constraints are not applicable …" while the sketch has conflicts.

use std::collections::HashSet;

use bevy::prelude::*;
use cadrs_core::FeatureId;
use cadrs_core::commands::EditSketch;
use cadrs_sketch::constraint::{FitOp, fit_op};
use cadrs_sketch::solve::{self, Analysis, Drag, Source};
use cadrs_sketch::{
    ConstraintId, ConstraintKind, CurveId, CurveKind, PointId, Sketch, SketchEntity, SketchOp,
};
use cadrs_ui::menu::{Menu, MenuAction, MenuItem};
use cadrs_ui::{Notification, Theme, show_notification, show_toast};

use crate::sketch::{ActiveSketchTool, SketchSession, SketchTool, SketchViewSettings};
use crate::sketch_tools::{SVec2, SketchDraw, SketchSelection, session_sketch};
use crate::{ActiveDocument, AppState};

pub struct SketchConstrainPlugin;

impl Plugin for SketchConstrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SketchAnalysis>()
            .init_resource::<SketchErrors>()
            .init_resource::<SketchUnderDefined>()
            .init_resource::<SketchFacesLost>()
            .add_systems(
                Update,
                (update_analysis, constraint_activation, normal_to_plane, sync_conflict_banner)
                    .chain()
                    .after(crate::sketch_tools::SketchToolsSet)
                    .before(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_observer(on_glyph_menu);
    }
}

/// The analysis of the sketch being edited.
#[derive(Resource, Debug, Default)]
pub struct SketchAnalysis {
    /// What it was computed from.
    key: Option<(FeatureId, Sketch)>,
    pub analysis: Analysis,
}

impl SketchAnalysis {
    /// The conflicting constraints and dimensions (left out while dragging).
    pub fn conflicting_sources(&self) -> HashSet<Source> {
        self.analysis
            .conflicting
            .iter()
            .map(|k| Source::Constraint(*k))
            .chain(
                self.analysis
                    .conflicting_dimensions
                    .iter()
                    .map(|k| Source::Dimension(*k)),
            )
            .collect()
    }
}

/// The sketch features (of the active Part Studio) with conflicting constraints.
#[derive(Resource, Debug, Default, PartialEq)]
pub struct SketchErrors(pub HashSet<FeatureId>);

/// The sketch features (of the active Part Studio) whose face is gone or no longer resolves
/// (S20.2), worked out in the background by [`update_analysis`] whenever the parts change.
#[derive(Resource, Debug, Default, PartialEq)]
pub struct SketchFacesLost(pub HashSet<FeatureId>);

/// The sketch features (of the active Part Studio) that have geometry but are not fully
/// defined (their feature-list icon gets the blue "−" badge).
#[derive(Resource, Debug, Default, PartialEq)]
pub struct SketchUnderDefined(pub HashSet<FeatureId>);

/// A sketch's badge state: (sketch, the geometry analysed, conflicting, under-defined).
type SketchStatus = (FeatureId, Sketch, bool, bool);

/// The feature-list badges' analyses of the active Part Studio's sketches, worked out on a
/// background thread: big (imported) sketches take seconds to analyse, which on the main thread
/// froze the app while a document opened. A badge keeps its last state until its new one lands.
#[derive(Default)]
pub(crate) struct BadgeAnalyses {
    done: Vec<SketchStatus>,
    running: Option<Pending<Vec<SketchStatus>>>,
}

/// The sketches of the active Part Studio whose face is lost, keyed by the parts' generation
/// and the tab they were worked out for, and the background thread working them out again.
#[derive(Default)]
pub(crate) struct LostFaces {
    done: Option<LostKey>,
    running: Option<(PartsKey, Pending<HashSet<FeatureId>>)>,
}

type PartsKey = (u64, Option<cadrs_core::ElementId>);
type LostKey = (PartsKey, HashSet<FeatureId>);

/// A result being worked out on a background thread (taken once it is there).
type Pending<T> = std::sync::Arc<std::sync::Mutex<Option<T>>>;

/// Runs `f` on a background thread, or right here when `wait` (scripted runs).
fn run_or_spawn<T: Send + 'static>(wait: bool, f: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
    if wait {
        return std::sync::Arc::new(std::sync::Mutex::new(Some(f())));
    }
    let slot: Pending<T> = Default::default();
    let out = slot.clone();
    std::thread::spawn(move || {
        let v = f();
        if let Ok(mut s) = out.lock() {
            *s = Some(v);
        }
    });
    slot
}

/// Whether `g` has conflicts (or a broken link, S20.2) and whether it is under-defined.
fn sketch_status(g: &Sketch) -> (bool, bool) {
    let a = solve::analyze(g);
    let under = !g.curves.is_empty() && !a.fully_constrained();
    (a.has_conflicts() || !g.broken.is_empty(), under)
}

#[allow(clippy::type_complexity)]
pub(crate) fn update_analysis(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<SketchSession>>,
    (draw, parts, budget): (Res<SketchDraw>, Res<crate::parts::PartCache>, Res<crate::parts::RebuildBudget>),
    mut analysis: ResMut<SketchAnalysis>,
    mut errors: ResMut<SketchErrors>,
    (mut under, mut faces_lost): (ResMut<SketchUnderDefined>, ResMut<SketchFacesLost>),
    (mut badges, mut lost): (Local<BadgeAnalyses>, Local<LostFaces>),
) {
    // The colours stay as they were while dragging (the solve keeps the structure).
    if draw.drag.is_some() {
        return;
    }
    let Some(doc) = doc else {
        return;
    };
    // Scripted runs wait for every rebuild (RebuildBudget(None)): they work these out at once
    // too, so screenshots never show a badge from before an edit.
    let wait = budget.0.is_none();
    let mut live = HashSet::new();
    let mut todo: Vec<(FeatureId, Sketch)> = Vec::new();
    if let Some(el) = doc.active_element() {
        for f in el.features() {
            let Some(sk) = f.sketch() else { continue };
            live.insert(f.id);
            if !badges.done.iter().any(|(id, g, ..)| *id == f.id && *g == sk.geometry) {
                todo.push((f.id, sk.geometry.clone()));
            }
        }
    }
    // One batch at a time; a sketch changed meanwhile is analysed again by the next one.
    if badges.running.is_none() && !todo.is_empty() {
        badges.running = Some(run_or_spawn(wait, move || {
            todo.into_iter()
                .map(|(id, g)| {
                    let (bad, u) = sketch_status(&g);
                    (id, g, bad, u)
                })
                .collect()
        }));
    }
    if let Some(results) = badges.running.as_ref().and_then(|r| r.lock().ok()?.take()) {
        badges.running = None;
        for r in results {
            match badges.done.iter_mut().find(|d| d.0 == r.0) {
                Some(d) => *d = r,
                None => badges.done.push(r),
            }
        }
    }
    badges.done.retain(|(id, ..)| live.contains(id));
    let cache = &badges.done;
    // A sketch whose face is gone (or no longer resolves: a lost reference) is in error too
    // (S20.2). Worked out on a background thread when the parts changed, never while a rebuild
    // runs: a face missing from the parts falls back to rebuilding the features before the
    // sketch and waiting for it (seconds on the main thread while a document opened).
    let key = (parts.generation, doc.active_element().map(|el| el.id));
    let stale = lost.done.as_ref().is_none_or(|(k, _)| *k != key);
    // Only on parts built from these features: before the first rebuild of a tab is done
    // there are none, every face sketch would look lost, and the check would rebuild the
    // features before each one, ahead of the rebuild itself (and of its snapshot).
    if stale
        && !parts.rebuilding
        && lost.running.is_none()
        && let Some(el) = doc.active_element()
        && parts.settled().is_some_and(|(e, f, _)| e == el.id && f == el.features())
    {
        let features = el.features().to_vec();
        let solids = parts.parts.clone();
        let found = run_or_spawn(wait, move || {
            (0..features.len())
                .filter(|i| cadrs_core::parts::sketch_face_lost_in(&features, *i, &solids))
                .map(|i| features[i].id)
                .collect()
        });
        lost.running = Some((key, found));
    }
    if let Some(found) = lost.running.as_ref().and_then(|(k, r)| Some((*k, r.lock().ok()?.take()?))) {
        lost.running = None;
        lost.done = Some(found);
    }
    let lost = lost.done.as_ref().map(|(_, l)| l.clone()).unwrap_or_default();
    if faces_lost.0 != lost {
        faces_lost.0 = lost.clone();
    }
    let want = SketchErrors(
        cache
            .iter()
            .filter(|e| e.2)
            .map(|e| e.0)
            .chain(lost)
            .collect(),
    );
    if *errors != want {
        *errors = want;
    }
    let want = SketchUnderDefined(cache.iter().filter(|e| e.3).map(|e| e.0).collect());
    if *under != want {
        *under = want;
    }
    let edited = session
        .as_deref()
        .and_then(|s| Some((s.feature, session_sketch(Some(s), Some(&doc))?)));
    match edited {
        Some((f, sketch)) => {
            let same = analysis
                .key
                .as_ref()
                .is_some_and(|(k, g)| *k == f && g == sketch);
            if !same {
                analysis.analysis = solve::analyze(sketch);
                analysis.key = Some((f, sketch.clone()));
            }
        }
        None => {
            if analysis.key.is_some() {
                *analysis = SketchAnalysis::default();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Constraint tools

/// Adds constraints to the sketch being edited, as one undoable step.
pub fn add_constraints(
    commands: &mut Commands,
    session: &SketchSession,
    kind: ConstraintKind,
    constraints: Vec<cadrs_sketch::Constraint>,
) {
    edit_sketch(
        commands,
        session,
        SketchOp::AddConstraint {
            constraints,
            label: kind.undo_label(),
        },
    );
}

/// Applies a constraint tool's edit to the sketch being edited, as one undoable step.
fn edit_sketch(commands: &mut Commands, session: &SketchSession, op: SketchOp) {
    let (element, feature) = (session.element, session.feature);
    commands.queue(move |world: &mut World| {
        if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
            && let Err(e) = doc.execute(&EditSketch { element, feature, op })
        {
            warn!("cannot add the constraint: {e}");
        }
    });
}

/// Choosing a constraint tool (key or menu) with entities selected applies it to them and
/// clears the selection; the tool is not kept. With nothing selected, it stays active.
#[allow(clippy::too_many_arguments)]
fn constraint_activation(
    mut tool: ResMut<ActiveSketchTool>,
    mut last: Local<SketchTool>,
    mut selection: ResMut<SketchSelection>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<crate::parts::PartCache>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if tool.tool == *last {
        return;
    }
    *last = tool.tool;
    let SketchTool::Constrain(kind) = tool.tool else {
        return;
    };
    let (Some(s), Some(d), Some(sketch)) = (
        session.as_deref(),
        doc.as_deref(),
        session_sketch(session.as_deref(), doc.as_deref()),
    ) else {
        return;
    };
    if selection.0.is_empty() {
        return;
    }
    let at = crate::sketch_links::sketch_vertex_at(d, s, &cache);
    match fit_op(kind, sketch, &selection.0, at) {
        FitOp::Complete(op) => {
            edit_sketch(&mut commands, s, *op);
            selection.0.clear();
            tool.tool = SketchTool::Select;
            *last = SketchTool::Select;
        }
        // The selection is the first pick; the tool waits for the rest.
        FitOp::Partial => {}
        FitOp::Invalid => {
            show_toast(
                &mut commands,
                &theme,
                format!("{} does not apply to the selection", kind.label()),
            );
            selection.0.clear();
        }
    }
}

/// Normal to a plane (S12.10, Final re-audit): with the Normal tool's line picked, a plane
/// picked in the feature list or the view (a default plane or a Plane feature) makes the line
/// normal to it, through the plane's trace in the sketch (a construction line linked to the
/// plane, see [`SketchOp::NormalToPlane`]).
#[allow(clippy::too_many_arguments)]
fn normal_to_plane(
    mut picks: MessageReader<crate::viewport::PickRequest>,
    tool: Res<ActiveSketchTool>,
    mut selection: ResMut<SketchSelection>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<crate::parts::PartCache>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    use crate::viewport::{Pick, PlaneKind};
    use cadrs_sketch::PlaneRef;
    if tool.tool != SketchTool::Constrain(ConstraintKind::Normal) {
        picks.clear();
        return;
    }
    let (Some(s), Some(doc)) = (session.as_deref(), doc.as_deref()) else {
        picks.clear();
        return;
    };
    for p in picks.read() {
        let Some(pick) = p.0 else { continue };
        let plane = match pick {
            Pick::Plane(PlaneKind::Top) => PlaneRef::Top,
            Pick::Plane(PlaneKind::Front) => PlaneRef::Front,
            Pick::Plane(PlaneKind::Right) => PlaneRef::Right,
            Pick::Feature(id) => match cache.planes.get(&id) {
                Some(f) => PlaneRef::Feature(cadrs_sketch::FeaturePlane::new(id.0, *f)),
                None => continue,
            },
            _ => continue,
        };
        let Some(sketch) = session_sketch(Some(s), Some(doc)) else { continue };
        let [SketchEntity::Curve(line)] = selection.0[..] else {
            show_toast(&mut commands, &theme, "Pick a line first, then the plane".to_string());
            continue;
        };
        if !matches!(sketch.curves.get(line).map(|c| c.kind), Some(CurveKind::Line { .. })) {
            continue;
        }
        let Some(frame) = doc
            .active_element()
            .and_then(|el| el.features().iter().find(|f| f.id == s.feature).and_then(|f| f.sketch()?.plane))
            .map(|p| p.frame())
        else {
            continue;
        };
        let Some(cadrs_core::links::Curve3::Line(a, b)) = cadrs_core::links::plane_trace(&plane.frame(), &frame) else {
            show_toast(&mut commands, &theme, "Normal: the plane is parallel to the sketch".to_string());
            continue;
        };
        let op = SketchOp::NormalToPlane {
            line,
            trace: (frame.to_sketch(a), frame.to_sketch(b)),
            link: cadrs_sketch::Link::Plane(plane),
        };
        let (element, feature) = (s.element, s.feature);
        commands.queue(move |world: &mut World| {
            if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
                && let Err(e) = doc.execute(&EditSketch { element, feature, op })
            {
                warn!("normal to plane: {e}");
            }
        });
        selection.0.clear();
    }
}

/// A pick with a constraint tool active: adds the entity to the picks and applies the
/// constraint once they fit. `at` places part vertices in the sketch (see
/// [`crate::sketch_links::sketch_vertex_at`]).
pub fn pick_for_constraint(
    kind: ConstraintKind,
    sketch: &Sketch,
    e: SketchEntity,
    selection: &mut SketchSelection,
    commands: &mut Commands,
    session: &SketchSession,
    at: impl Fn(cadrs_sketch::Link) -> Option<SVec2>,
) {
    if matches!(e, SketchEntity::Constraint(_) | SketchEntity::Dimension(_)) {
        return;
    }
    if !selection.0.contains(&e) {
        selection.0.push(e);
    }
    match fit_op(kind, sketch, &selection.0, &at) {
        FitOp::Complete(op) => {
            edit_sketch(commands, session, *op);
            // Symmetric keeps its axis for the next pair (Onshape "pre-selects the axis line",
            // `constraints.md`).
            let axis = (kind == ConstraintKind::Symmetric)
                .then(|| selection.0.first().copied())
                .flatten();
            selection.0.clear();
            selection.0.extend(axis);
        }
        FitOp::Partial => {}
        FitOp::Invalid => {
            // Start again from this pick if it can begin a set.
            selection.0.clear();
            if fit_op(kind, sketch, &[e], &at) != FitOp::Invalid {
                selection.0.push(e);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Dragging

/// A drag in progress (no tool): what is dragged and the sketch as it was.
#[derive(Debug, Clone)]
pub struct DragInfo {
    pub entity: SketchEntity,
    /// The sketch when the drag began (the undo step goes from here).
    pub base: Sketch,
    /// Where the pointer was on the sketch plane when it pressed.
    pub grab: SVec2,
    /// Conflicting constraints (left unsolved while dragging).
    pub skip: HashSet<Source>,
}

impl DragInfo {
    /// Starts a drag on a point or curve (not the origin, a dimension or a glyph).
    pub fn new(
        entity: SketchEntity,
        sketch: &Sketch,
        grab: SVec2,
        skip: HashSet<Source>,
    ) -> Option<Self> {
        // A text is dragged by its box's lower edge; the rest of the box follows.
        let entity = match entity {
            SketchEntity::Text(t) => SketchEntity::Curve(sketch.texts.get(t)?.lines[0]),
            e => e,
        };
        match entity {
            SketchEntity::Point(p) if sketch.points.contains_key(p) => {}
            SketchEntity::Curve(c) if sketch.curves.contains_key(c) => {}
            _ => return None,
        }
        Some(Self {
            entity,
            base: sketch.clone(),
            grab,
            skip,
        })
    }

    /// The solver's drag target for the pointer at `cursor` (sketch mm).
    pub fn target(&self, cursor: SVec2) -> Option<Drag> {
        let delta = cursor - self.grab;
        let s = &self.base;
        Some(match self.entity {
            SketchEntity::Point(p) => Drag::Points(vec![(p, s.pos(p) + delta)]),
            SketchEntity::Curve(c) => match s.curves.get(c)?.kind {
                CurveKind::Line { a, b } => {
                    Drag::Points(vec![(a, s.pos(a) + delta), (b, s.pos(b) + delta)])
                }
                CurveKind::Circle { .. } | CurveKind::Arc { .. } => Drag::Rim(c, cursor),
                // An ellipse moves as a whole.
                CurveKind::Ellipse { center, major, .. } | CurveKind::EllipseOffset { center, major, .. } => Drag::Points(vec![
                    (center, s.pos(center) + delta),
                    (major, s.pos(major) + delta),
                ]),
                // A spline or an elliptical arc moves as a whole (its points).
                CurveKind::Spline { .. } | CurveKind::EllipseArc { .. } => Drag::Points(s.curve_points(c).into_iter().map(|p| (p, s.pos(p) + delta)).collect()),
                // A Bézier curve moves as a whole (drag a handle to reshape it).
                CurveKind::Bezier { a, c1, c2, b } => Drag::Points([a, c1, c2, b].map(|p| (p, s.pos(p) + delta)).to_vec()),
            },
            _ => return None,
        })
    }
}

pub(crate) fn sketch_mut(world: &mut World) -> Option<&mut Sketch> {
    let s = world.get_resource::<SketchSession>()?.clone();
    let doc = world.get_resource_mut::<ActiveDocument>()?.into_inner();
    let f = doc
        .doc
        .element_mut(s.element)?
        .features_mut()?
        .iter_mut()
        .find(|f| f.id == s.feature)?;
    Some(&mut f.sketch_mut()?.geometry)
}

/// Moves the live sketch toward a drag target (no undo step: the drag is recorded on release).
/// Each frame solves from `base` (the sketch when the drag began), so dragging out and back
/// returns to the start, and inside-out solutions are refused (see [`solve::drag_from`]).
pub fn drag_live(world: &mut World, base: &Sketch, target: Drag, skip: HashSet<Source>) {
    if let Some(sketch) = sketch_mut(world) {
        solve::drag_from(base, sketch, &target, &skip);
    }
}

/// The edit a drag records: the dragged geometry, plus the constraints the dragged point
/// inferred where it was dropped (S11.4), as one step. Inferred constraints that are already
/// there, or that would over-define the sketch, are left out.
pub fn drag_op(
    base: &Sketch,
    points: Vec<(PointId, SVec2)>,
    radii: Vec<(CurveId, f64)>,
    inferred: Vec<cadrs_sketch::Constraint>,
) -> SketchOp {
    let geometry = SketchOp::SetGeometry { points, radii };
    let inferred: Vec<cadrs_sketch::Constraint> = inferred
        .into_iter()
        .filter(|c| !base.constraints.values().any(|x| x == c))
        .collect();
    if inferred.is_empty() {
        return geometry;
    }
    let with = SketchOp::Batch(vec![
        geometry.clone(),
        SketchOp::AddConstraint {
            constraints: inferred,
            label: "Drag",
        },
    ]);
    let mut trial = base.clone();
    let ok = with.apply(&mut trial).is_ok()
        && (!solve::analyze(&trial).has_conflicts() || solve::analyze(base).has_conflicts());
    if ok { with } else { geometry }
}

/// Ends a drag: puts the sketch back as it was and records the dragged geometry (and the
/// constraints the drop inferred) as one step.
pub fn drag_commit(world: &mut World, base: Sketch, inferred: Vec<cadrs_sketch::Constraint>) {
    let Some(sketch) = sketch_mut(world) else {
        return;
    };
    let points: Vec<(PointId, SVec2)> = sketch.points.iter().map(|(k, p)| (k, p.pos)).collect();
    let radii: Vec<(CurveId, f64)> = sketch
        .curves
        .iter()
        .filter_map(|(k, c)| c.kind.scalar().map(|r| (k, r)))
        .collect();
    let op = drag_op(&base, points, radii, inferred);
    *sketch = base;
    let Some(s) = world.get_resource::<SketchSession>().cloned() else {
        return;
    };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = doc.execute(&EditSketch {
            element: s.element,
            feature: s.feature,
            op,
        })
    {
        warn!("cannot record the drag: {e}");
    }
}

/// Puts the sketch back as it was before a drag (Esc).
pub fn drag_cancel(world: &mut World, base: Sketch) {
    if let Some(sketch) = sketch_mut(world) {
        *sketch = base;
    }
}

// ---------------------------------------------------------------------------------------------
// Glyph menu

/// The anchor of a glyph's context menu.
#[derive(Component)]
struct GlyphMenuFor(ConstraintId);

/// Right-click on a glyph: a menu with Delete.
pub fn open_glyph_menu(world: &mut World, constraint: ConstraintId, at: Vec2) {
    let theme = world.resource::<Theme>().clone();
    let sketch_name = world
        .get_resource::<SketchSession>()
        .and_then(|s| {
            let doc = world.get_resource::<ActiveDocument>()?;
            Some(doc.doc.element(s.element)?.feature(s.feature)?.name.clone())
        })
        .unwrap_or_else(|| "sketch".into());
    world.resource_mut::<SketchSelection>().0 = vec![SketchEntity::Constraint(constraint)];
    // Grouped like Onshape's sketch context menus.
    let menu = Menu::new("sketch-glyph-menu")
        .min_width(200.0)
        .item_height(23.0)
        .item(MenuItem::new("sketch-glyph-delete", "Delete").icon("delete"))
        .item(MenuItem::new("sketch-glyph-select-geometry", "Select geometry"))
        .item(MenuItem::new("sketch-glyph-hide", "Hide constraints"))
        .separator()
        .item(MenuItem::new("sketch-glyph-escape", "Escape constraint"))
        .item(MenuItem::new("sketch-glyph-confirm", format!("Confirm {sketch_name}")))
        .separator()
        .item(MenuItem::new("sketch-glyph-zoom-to-fit", "Zoom to fit"))
        .item(MenuItem::new("sketch-glyph-normal-to", "View normal to sketch plane"));
    let mut commands = world.commands();
    let anchor = cadrs_ui::open_context_menu(&mut commands, at, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((GlyphMenuFor(constraint), DespawnOnExit(AppState::Document)));
    world.flush();
}

fn on_glyph_menu(
    ev: On<MenuAction>,
    q: Query<&GlyphMenuFor>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut selection: ResMut<SketchSelection>,
    mut settings: ResMut<SketchViewSettings>,
    mut commands: Commands,
) {
    let (Ok(anchor), Some(s)) = (q.get(ev.entity), session) else {
        return;
    };
    let (element, feature, id) = (s.element, s.feature, anchor.0);
    match ev.item.as_str() {
        "sketch-glyph-delete" => {}
        "sketch-glyph-select-geometry" => {
            // The constraint's points and curves (not the sketch origin or axes).
            let Some(c) = session_sketch(Some(&s), doc.as_deref())
                .and_then(|sk| sk.constraints.get(id))
            else {
                return;
            };
            selection.0 = c
                .curves()
                .into_iter()
                .filter_map(|k| match k {
                    cadrs_sketch::CurveRef::Curve(k) => Some(SketchEntity::Curve(k)),
                    _ => None,
                })
                .chain(c.points().into_iter().filter_map(|p| match p {
                    cadrs_sketch::PointRef::Point(p) => Some(SketchEntity::Point(p)),
                    _ => None,
                }))
                .collect();
            return;
        }
        "sketch-glyph-hide" => {
            settings.show_constraints = false;
            selection.0.clear();
            return;
        }
        "sketch-glyph-escape" => {
            selection.0.clear();
            return;
        }
        "sketch-glyph-confirm" => {
            selection.0.clear();
            commands.queue(crate::sketch::accept_sketch);
            return;
        }
        "sketch-glyph-zoom-to-fit" => {
            commands.queue(crate::viewport::zoom_to_fit);
            return;
        }
        "sketch-glyph-normal-to" => {
            commands.queue(crate::viewport::normal_to_sketch);
            return;
        }
        _ => return,
    }
    selection.0.clear();
    commands.queue(move |world: &mut World| {
        if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
            let _ = doc.execute(&EditSketch {
                element,
                feature,
                op: SketchOp::Delete {
                    curves: vec![],
                    points: vec![],
                    dimensions: vec![],
                    constraints: vec![id],
                },
            });
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Banner

#[derive(Component)]
struct ConflictBanner;

/// The yellow banner while the edited sketch has conflicting constraints (`screens/15`).
#[allow(clippy::too_many_arguments)]
fn sync_conflict_banner(
    analysis: Res<SketchAnalysis>,
    session: Option<Res<SketchSession>>,
    settings: Res<SketchViewSettings>,
    theme: Res<Theme>,
    q: Query<Entity, With<ConflictBanner>>,
    mut last: Local<Vec<Source>>,
    mut opened_broken: Local<Option<(FeatureId, bool)>>,
    mut commands: Commands,
) {
    // P3D.1 (IR5.3): a sketch opened for editing while it already can't be solved says so
    // ("Sketch could not be solved.", `ex1-step3.png`); a conflict made while drawing gets
    // the longer message (`screens/15`). Worked out once per session.
    if let Some(s) = session.as_ref()
        && opened_broken.is_none_or(|(f, _)| f != s.feature)
    {
        let broken = s
            .before
            .as_ref()
            .and_then(|f| f.sketch())
            .is_some_and(|k| !cadrs_sketch::solve::conflicts(&k.geometry).is_empty());
        *opened_broken = Some((s.feature, broken));
    } else if session.is_none() {
        *opened_broken = None;
    }
    let mut key: Vec<Source> = if session.is_some() && settings.show_errors {
        analysis.conflicting_sources().into_iter().collect()
    } else {
        Vec::new()
    };
    key.sort_by_key(|s| format!("{s:?}"));
    if key == *last {
        return;
    }
    let grew = key.iter().any(|k| !last.contains(k));
    *last = key.clone();
    if key.is_empty() {
        for e in &q {
            commands.entity(e).try_despawn();
        }
        return;
    }
    // A new conflict (re)opens the banner; one closed by its ✕ stays closed otherwise.
    if grew && q.is_empty() {
        let unsolved = opened_broken.is_some_and(|(_, b)| b);
        let (message, name) = if unsolved {
            ("Sketch could not be solved.", "sketch-unsolved-toast")
        } else {
            (
                "Some constraints are not applicable to the current geometry and have not been solved.",
                "sketch-conflict-banner",
            )
        };
        // Left-aligned at x ≈ 266, y ≈ 78, over the dialog's title (`screens/15`); "could not
        // be solved" right of the dialog's title (`ex1-step3.png`).
        let at = if unsolved { (212.0, 6.0) } else { (40.0, 8.0) };
        let banner = show_notification(
            &mut commands,
            &theme,
            Notification::warning(message)
            .name(name)
            .at(at.0, at.1),
        );
        commands.entity(banner).insert(ConflictBanner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_sketch::{ConstraintOf, Orient, PointRef};

    #[test]
    fn a_drag_records_the_constraint_it_inferred_in_the_same_step() {
        // S11.4: a circle's center dragged until straight above the origin.
        let mut base = Sketch::new();
        SketchOp::AddCircle {
            center: SVec2::new(20.0, 30.0),
            radius: 8.0,
            construction: false,
        }
        .apply(&mut base)
        .unwrap();
        let center = base.point_at(SVec2::new(20.0, 30.0), 1e-9).unwrap();
        let vertical = ConstraintOf::Vertical(Orient::Points(PointRef::Point(center), PointRef::Origin));
        let op = drag_op(&base, vec![(center, SVec2::new(0.0, 33.0))], vec![], vec![vertical]);
        assert_eq!(op.label(), "Drag");
        let mut s = base.clone();
        op.apply(&mut s).unwrap();
        assert!(s.pos(center).distance(SVec2::new(0.0, 33.0)) < 1e-6);
        assert!(s.constraints.values().any(|c| *c == vertical));
        // Already there: just the geometry.
        let again = drag_op(&s, vec![(center, SVec2::new(0.0, 40.0))], vec![], vec![vertical]);
        assert!(matches!(again, SketchOp::SetGeometry { .. }));
        // Nothing inferred: just the geometry.
        assert!(matches!(
            drag_op(&base, vec![(center, SVec2::new(5.0, 5.0))], vec![], vec![]),
            SketchOp::SetGeometry { .. }
        ));
    }
}
