//! The pick-based entity tools (`intro-to-sketching.md` S6, S9), following Onshape's help
//! (`reference/onshape/entity_tools.md`):
//!
//! - **Slot** (Offset ▾): click a line or an arc (usually construction) and it becomes the
//!   spine of a slot, with a Ø width on an end arc. More picks in the same use make more slots
//!   of the same width (Equal to the first). Curves selected before choosing the tool are
//!   slotted at once. The width is edited by double-clicking its value (S6.2).
//! - **Sketch fillet (Shift+F):** click a corner where two lines meet (or the two lines) and
//!   it is filleted; the radius box opens to type the radius. Further clicks in the same use
//!   make fillets equal to the first, so the one "R" drives them all. Pressing on a corner and
//!   dragging sizes the fillet live; releasing places it at that size (S9.1).
//! - **Sketch chamfer** (Fillet ▾): the same picks; two equal distances (45°) by default, then
//!   the first distance's box, Enter, the second's (S9.2). Dragging sizes it live.
//!
//! Every edit is an [`EditSketch`] command, one undo step per pick.

use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_core::commands::EditSketch;
use cadrs_sketch::entity::{chamfer_geometry, corner_lines, fillet_corner, fillet_geometry, max_fillet};
use cadrs_sketch::hit::hit_test;
use cadrs_sketch::{
    ArcGeom, CurveId, CurveKind, DimensionId, DimensionKind, PointId, Sketch, SketchEntity,
    SketchOp,
};

use crate::sketch::{ActiveSketchTool, PartStudioMode, SketchSession, SketchTool};
use crate::sketch_tools::{
    DRAG_THRESHOLD, SVec2, ScreenMap, SketchScreen, SketchSelection, SketchToolsSet,
    over_viewport, session_sketch, world_sketch,
};
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct SketchEntityToolsPlugin;

impl Plugin for SketchEntityToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityTools>()
            .add_systems(
                Update,
                entity_tools_pointer
                    .in_set(SketchToolsSet)
                    .after(crate::sketch_tools::sketch_pointer)
                    .before(crate::sketch::sketch_keys)
                    .run_if(in_state(PartStudioMode::Sketching))
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                draw_handle
                    .after(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(PartStudioMode::Sketching), reset_entity_tools);
    }
}

/// The pick tools' state.
#[derive(Resource, Debug, Default)]
pub struct EntityTools {
    last_tool: SketchTool,
    /// The first slot end arc, fillet arc made in this use of the tool: the next ones are
    /// equal to it.
    first: Option<CurveId>,
    /// A line picked first, waiting for the second line of a corner.
    first_line: Option<CurveId>,
    /// Where the primary button went down, and what was under it.
    press: Option<(Vec2, Option<SketchEntity>)>,
    /// A fillet or chamfer being sized by dragging from its corner.
    pub live: Option<LiveCorner>,
    /// The first chamfer of this use: its extensions (later ones are linked to them) and its
    /// two distance dimensions.
    first_chamfer: Option<[CurveId; 2]>,
    chamfer_dims: Vec<DimensionId>,
    /// The last fillet or chamfer made: its resize arrow.
    pub handle: Option<Handle>,
    /// Its arrow being dragged.
    pub handle_drag: Option<HandleDrag>,
}

/// A fillet or chamfer sized live by a drag (S9.1, S9.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveCorner {
    pub corner: PointId,
    /// The fillet radius, or the chamfer distance (mm).
    pub size: f64,
    pub chamfer: bool,
}

impl LiveCorner {
    /// The preview: the new piece (the arc or the line) and its label, in sketch mm.
    pub fn preview(&self, s: &Sketch) -> Option<(Vec<SVec2>, String, SVec2)> {
        if self.chamfer {
            let (a, b) = chamfer_geometry(s, self.corner, self.size, self.size)?;
            Some((vec![a, b], String::new(), a.midpoint(b)))
        } else {
            let (a, b, c) = fillet_geometry(s, self.corner, self.size)?;
            let g = ArcGeom::ccw(c, a, b);
            let g = if g.sweep > std::f64::consts::PI {
                ArcGeom::ccw(c, b, a)
            } else {
                g
            };
            Some((
                g.tessellate(std::f64::consts::PI / 90.0, 8),
                "R".into(),
                g.mid(),
            ))
        }
    }
}

/// The resize arrow: a hollow arrow, orange on a fillet, grey on a chamfer
/// (`entity_tools/sketch-fillet-manipulator.png`, `chamfer-manipulator-01.png`).
fn draw_handle(
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    tool: Res<ActiveSketchTool>,
    t: Res<EntityTools>,
    mut g: Gizmos<crate::sketch_draw::SketchLineGizmos>,
) {
    let (Some(map), Some(sketch), Some(h)) = (
        screen.active,
        session_sketch(session.as_deref(), doc.as_deref()),
        t.handle.as_ref(),
    ) else {
        return;
    };
    if !matches!(tool.tool, SketchTool::Fillet | SketchTool::Chamfer) {
        return;
    }
    let Some((b, d)) = h.arrow(sketch, &map) else {
        return;
    };
    let n = d.perp();
    let (shaft, head, neck) = (2.5, 7.0, ARROW - 11.0);
    let outline = [
        b + n * shaft,
        b + d * neck + n * shaft,
        b + d * neck + n * head,
        b + d * ARROW,
        b + d * neck - n * head,
        b + d * neck - n * shaft,
        b - n * shaft,
        b + n * shaft,
    ];
    let color = match h {
        Handle::Fillet { .. } => Color::srgb_u8(0xf3, 0x9c, 0x1e),
        Handle::Chamfer { .. } => Color::srgb_u8(0x8a, 0x8f, 0x94),
    };
    let frame = map.plane.frame();
    let pts: Vec<Vec3> = outline
        .iter()
        .filter_map(|p| map.to_sketch(*p))
        .map(|q| {
            let w = frame.to_world(q);
            Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32)
        })
        .collect();
    if pts.len() == outline.len() {
        g.linestrip(pts, color);
    }
}

fn reset_entity_tools(mut t: ResMut<EntityTools>) {
    *t = EntityTools::default();
}

/// A round default size: `v` to one significant figure (at least 0.1 mm).
pub fn nice(v: f64) -> f64 {
    if !(v.is_finite() && v > 0.0) {
        return 1.0;
    }
    let p = 10f64.powf(v.log10().floor());
    ((v / p).round() * p).max(0.1)
}

/// True if a fillet (lines and arcs, S9.1) or a chamfer (lines, S9.2) can go at `p`.
fn corner_ok(s: &Sketch, p: PointId, chamfer: bool) -> bool {
    if chamfer { corner_lines(s, p).is_some() } else { fillet_corner(s, p).is_some() }
}

/// A new fillet's radius: a fifth of the shorter line (or arc chord), rounded (the fillet must
/// fit).
fn default_radius(s: &Sketch, corner: PointId) -> Option<f64> {
    let [(_, a), (_, b)] = fillet_corner(s, corner)?;
    let p = s.pos(corner);
    let len = p.distance(s.pos(a)).min(p.distance(s.pos(b)));
    let max = max_fillet(s, corner)?;
    Some(nice(len * 0.2).min(max * 0.9))
}

/// The corner two lines (or, for a fillet, lines and arcs) share, if they meet at an end (and
/// only they meet there).
fn shared_corner(s: &Sketch, l1: CurveId, l2: CurveId, chamfer: bool) -> Option<PointId> {
    let (a, b) = s.curve_ends(l1)?;
    let (c, d) = s.curve_ends(l2)?;
    [a, b]
        .into_iter()
        .find(|p| (*p == c || *p == d) && corner_ok(s, *p, chamfer))
}

/// Runs `op` on the edited sketch and returns what it added: curves and dimensions.
fn run(world: &mut World, op: SketchOp) -> Option<(Vec<CurveId>, Vec<DimensionId>)> {
    let s = world.get_resource::<SketchSession>()?.clone();
    let before = world_sketch(world)?.clone();
    let mut doc = world.get_resource_mut::<ActiveDocument>()?;
    if let Err(e) = doc.execute(&EditSketch {
        element: s.element,
        feature: s.feature,
        op,
    }) {
        warn!("sketch edit failed: {e}");
        return None;
    }
    let after = world_sketch(world)?;
    let curves = after
        .curves
        .keys()
        .filter(|k| !before.curves.contains_key(*k))
        .collect();
    let dims = after
        .dimensions
        .keys()
        .filter(|k| !before.dimensions.contains_key(*k))
        .collect();
    Some((curves, dims))
}

/// Makes a slot round `source` (equal to the first of this use, if any).
fn make_slot(world: &mut World, source: CurveId) {
    let Some(sk) = world_sketch(world) else { return };
    let (width, first_end) = match sk.curves.get(source).map(|c| c.kind) {
        Some(CurveKind::Line { a, b }) => (nice(sk.pos(a).distance(sk.pos(b)) * 0.25), a),
        Some(CurveKind::Arc { start, .. }) => {
            let Some(g) = sk.arc_geom(source) else { return };
            (nice((g.length() * 0.25).min(g.radius * 0.5)), start)
        }
        _ => return,
    };
    let first = world.resource::<EntityTools>().first;
    // Slots made together share one width: the first one's.
    let width = first
        .and_then(|c| sk.arc_geom(c))
        .map_or(width, |g| g.radius * 2.0);
    let construction = world.resource::<ActiveSketchTool>().construction;
    let Some((curves, _)) = run(
        world,
        SketchOp::Slot {
            source,
            width,
            equal_to: first,
            construction,
        },
    ) else {
        return;
    };
    let Some(sk) = world_sketch(world) else { return };
    let cap = curves.into_iter().find(|c| {
        matches!(sk.curves.get(*c).map(|c| c.kind), Some(CurveKind::Arc { center, .. }) if center == first_end)
    });
    let mut t = world.resource_mut::<EntityTools>();
    if t.first.is_none() {
        t.first = cap;
    }
}

/// Fillets or chamfers `corner`, at `size` if given (a drag), else the tool's size. Later
/// ones in the same use reuse the first one's size and are linked to it (one "R" or one pair
/// of distances drives them all).
fn make_corner(world: &mut World, corner: PointId, chamfer: bool, size: Option<f64>) {
    let Some(sk) = world_sketch(world) else { return };
    if !corner_ok(sk, corner, chamfer) {
        return;
    }
    if chamfer {
        let [(_, a), (_, b)] = corner_lines(sk, corner).unwrap_or_default();
        let p = sk.pos(corner);
        let len = p.distance(sk.pos(a)).min(p.distance(sk.pos(b)));
        let t = world.resource::<EntityTools>();
        let first = t
            .first_chamfer
            .filter(|e| e.iter().all(|k| sk.curves.contains_key(*k)))
            .filter(|_| size.is_none());
        let (d1, d2) = match first {
            Some(e) => (
                sk.line_length(e[0]).unwrap_or(1.0),
                sk.line_length(e[1]).unwrap_or(1.0),
            ),
            None => {
                let d = size.unwrap_or_else(|| nice(len * 0.2).min(len * 0.9));
                (d, d)
            }
        };
        let first_dims = t.chamfer_dims.clone();
        let Some((curves, dims)) = run(
            world,
            SketchOp::Chamfer {
                corner,
                d1: d1.min(len * 0.95),
                d2: d2.min(len * 0.95),
                equal_to: first,
            },
        ) else {
            return;
        };
        let Some(sk) = world_sketch(world) else { return };
        let ext: Vec<CurveId> = curves
            .into_iter()
            .filter(|c| sk.curves[*c].construction)
            .collect();
        let dims = if dims.is_empty() { first_dims } else { dims };
        {
            let mut t = world.resource_mut::<EntityTools>();
            if first.is_none() && ext.len() == 2 {
                t.first_chamfer = Some([ext[0], ext[1]]);
                t.chamfer_dims = dims.clone();
            }
            t.handle = Some(Handle::Chamfer {
                corner,
                dims: dims.clone(),
            });
        }
        // S9.2: type the first distance, Enter, then the second.
        if dims.len() == 2 && first.is_none() {
            crate::sketch_dimension::edit_in_turn(world, &dims);
        }
        return;
    }
    let first = world
        .resource::<EntityTools>()
        .first
        .filter(|c| sk.curves.contains_key(*c));
    // Further clicks reuse the first fillet's radius (it drives them all).
    let r = match (size, first.and_then(|c| sk.arc_geom(c))) {
        (Some(r), _) => Some(r),
        (None, Some(g)) => Some(g.radius),
        (None, None) => default_radius(sk, corner),
    };
    let Some(r) = r else { return };
    let r = r.min(max_fillet(sk, corner).unwrap_or(r) * 0.999);
    let linked = first.filter(|_| size.is_none());
    let Some((curves, dims)) = run(
        world,
        SketchOp::Fillet {
            corner,
            radius: r,
            equal_to: linked,
        },
    ) else {
        return;
    };
    let Some(sk) = world_sketch(world) else { return };
    let arc = curves
        .into_iter()
        .find(|c| matches!(sk.curves.get(*c).map(|c| c.kind), Some(CurveKind::Arc { .. })));
    let radius_dim = dims.first().copied().or_else(|| {
        let f = linked?;
        sk.dimensions
            .iter()
            .find(|(_, d)| d.kind == DimensionKind::Radius { curve: f })
            .map(|(k, _)| k)
    });
    {
        let mut t = world.resource_mut::<EntityTools>();
        if t.first.is_none() {
            t.first = arc;
        }
        if let (Some(arc), Some(dim)) = (arc, radius_dim) {
            t.handle = Some(Handle::Fillet { arc, corner, dim });
        }
    }
    // The radius, to type (S9.1): the first fillet's; the ones linked to it reuse it.
    if let Some(d) = radius_dim.filter(|_| linked.is_none()) {
        crate::sketch_dimension::edit_in_turn(world, &[d]);
    }
}

/// The resize arrow of the last fillet or chamfer made (`entity_tools/sketch-fillet-manipulator.png`,
/// `chamfer-manipulator-01.png`): dragging it changes the radius or distances live.
#[derive(Debug, Clone, PartialEq)]
pub enum Handle {
    /// Just outside the fillet's corner (its virtual sharp), pointing out along the corner's
    /// bisector (orange); dragging it out makes the fillet bigger.
    Fillet {
        arc: CurveId,
        corner: PointId,
        dim: DimensionId,
    },
    /// On the chamfer's first end, pointing along its edge to the corner (grey).
    Chamfer {
        corner: PointId,
        dims: Vec<DimensionId>,
    },
}

/// The arrow's length (px).
const ARROW: f32 = 26.0;

impl Handle {
    /// Where the arrow is on screen: its base and unit direction.
    pub fn arrow(&self, s: &Sketch, map: &ScreenMap) -> Option<(Vec2, Vec2)> {
        match self {
            Handle::Fillet { arc, corner, .. } => {
                let g = s.arc_geom(*arc)?;
                let (m, p) = (map.to_screen(g.mid()), map.to_screen(s.pos(*corner)));
                let d = (p - m).normalize_or_zero();
                (d != Vec2::ZERO).then_some((p + d * 6.0, d))
            }
            Handle::Chamfer { corner, dims } => {
                let d = s.dimensions.get(*dims.first()?)?;
                let DimensionKind::Aligned { a, b } = d.kind else {
                    return None;
                };
                let end = if a == *corner { b } else { a };
                let (t, p) = (map.to_screen(s.pos(end)), map.to_screen(s.pos(*corner)));
                let dir = (p - t).normalize_or_zero();
                (dir != Vec2::ZERO).then_some((t + dir * 3.0, dir))
            }
        }
    }

    /// True if the screen point is on the arrow.
    pub fn hit(&self, s: &Sketch, map: &ScreenMap, p: Vec2) -> bool {
        self.arrow(s, map).is_some_and(|(b, d)| {
            let t = (p - b).dot(d).clamp(0.0, ARROW);
            (b + d * t).distance(p) <= 9.0
        })
    }

    /// The edit for a drag of the arrow from `grab` to `cursor` (sketch mm), from `s` (the
    /// sketch when the drag began).
    pub fn resize_op(&self, s: &Sketch, grab: SVec2, cursor: SVec2) -> Option<SketchOp> {
        match self {
            Handle::Fillet { arc, corner, dim } => {
                let g = s.arc_geom(*arc)?;
                let p = s.pos(*corner);
                let k = p.distance(g.center);
                if k <= g.radius + 1e-9 {
                    return None;
                }
                let w = (g.center - p).normalize();
                // The arc's middle moves out with the pointer: it is k − r from the corner, and
                // k/r stays as the angle is.
                let out = -(cursor - grab).dot(w);
                let r = (g.radius + out / (k / g.radius - 1.0)).max(0.01);
                Some(SketchOp::SetDimensionValue { id: *dim, value: r })
            }
            Handle::Chamfer { corner, dims } => {
                let p = s.pos(*corner);
                let (k1, k2) = (*dims.first()?, *dims.get(1)?);
                let (v1, v2) = (s.dimensions.get(k1)?.value, s.dimensions.get(k2)?.value);
                let DimensionKind::Aligned { a, b } = s.dimensions.get(k1)?.kind else {
                    return None;
                };
                let end = if a == *corner { b } else { a };
                let u = (s.pos(end) - p).normalize();
                let d1 = (cursor - p).dot(u).max(0.01);
                Some(SketchOp::Batch(vec![
                    SketchOp::SetDimensionValue { id: k1, value: d1 },
                    SketchOp::SetDimensionValue {
                        id: k2,
                        value: (d1 * v2 / v1.max(1e-9)).max(0.01),
                    },
                ]))
            }
        }
    }
}

/// A resize arrow being dragged: the handle and the sketch when the drag began.
#[derive(Debug, Clone)]
pub struct HandleDrag {
    pub handle: Handle,
    pub base: Sketch,
    /// Where the arrow was grabbed (sketch mm).
    pub grab: SVec2,
}

/// One frame of an arrow drag: the sketch as the resize would leave it (no undo step).
fn handle_live(world: &mut World, drag: &HandleDrag, cursor: SVec2) {
    let Some(op) = drag.handle.resize_op(&drag.base, drag.grab, cursor) else {
        return;
    };
    let mut trial = drag.base.clone();
    if op.apply(&mut trial).is_ok()
        && cadrs_sketch::solve::analyze(&trial).conflicting.is_empty()
        && let Some(sk) = crate::sketch_constrain::sketch_mut(world)
    {
        *sk = trial;
    }
}

/// The end of an arrow drag: one undo step.
fn handle_commit(world: &mut World, drag: HandleDrag, cursor: SVec2) {
    let Some(sk) = crate::sketch_constrain::sketch_mut(world) else {
        return;
    };
    *sk = drag.base.clone();
    let Some(op) = drag.handle.resize_op(&drag.base, drag.grab, cursor) else {
        return;
    };
    let mut trial = drag.base.clone();
    if op.apply(&mut trial).is_ok() && cadrs_sketch::solve::analyze(&trial).conflicting.is_empty() {
        run(world, op);
    }
}

/// The corner a pick means: a point where two lines meet (for a fillet, lines or arcs), or the
/// second of two such curves that meet (the first is remembered).
fn corner_pick(s: &Sketch, t: &mut EntityTools, hit: Option<SketchEntity>, chamfer: bool) -> Option<PointId> {
    match hit? {
        SketchEntity::Point(p) if corner_ok(s, p, chamfer) => {
            t.first_line = None;
            Some(p)
        }
        SketchEntity::Curve(c)
            if matches!(s.curves.get(c).map(|c| c.kind), Some(CurveKind::Line { .. }))
                || (!chamfer && matches!(s.curves.get(c).map(|c| c.kind), Some(CurveKind::Arc { .. }))) =>
        {
            match t.first_line.take() {
                Some(l) if l != c => match shared_corner(s, l, c, chamfer) {
                    Some(p) => Some(p),
                    None => {
                        t.first_line = Some(c);
                        None
                    }
                },
                _ => {
                    t.first_line = Some(c);
                    None
                }
            }
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn entity_tools_pointer(
    mut inputs: MessageReader<PointerInput>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    tool: Res<ActiveSketchTool>,
    mut selection: ResMut<SketchSelection>,
    mut t: ResMut<EntityTools>,
    mut commands: Commands,
) {
    let (Some(map), Some(sketch)) = (
        screen.active,
        session_sketch(session.as_deref(), doc.as_deref()),
    ) else {
        inputs.clear();
        return;
    };
    let active = matches!(tool.tool, SketchTool::Slot | SketchTool::Fillet | SketchTool::Chamfer);
    // A new use of the tool: selected entities are used at once (selection first).
    if t.last_tool != tool.tool {
        t.last_tool = tool.tool;
        t.first = None;
        t.first_line = None;
        t.press = None;
        t.live = None;
        t.first_chamfer = None;
        t.chamfer_dims.clear();
        t.handle = None;
        t.handle_drag = None;
        if active && !selection.0.is_empty() {
            let picks = std::mem::take(&mut selection.0);
            let kind = tool.tool;
            commands.queue(move |world: &mut World| use_selection(world, kind, &picks));
        }
    }
    if !active {
        inputs.clear();
        return;
    }
    let over = over_viewport(&hover_map, &q_area);
    let hit_at = |pos: Vec2| {
        hit_test(sketch, SVec2::new(pos.x as f64, pos.y as f64), |p| {
            map.to_screen64(p)
        })
        .map(|h| h.entity)
    };
    let chamfer = tool.tool == SketchTool::Chamfer;
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                t.press = over.then(|| (pos, hit_at(pos)));
                // On the resize arrow: its drag begins.
                if over
                    && let Some(h) = t.handle.clone().filter(|h| h.hit(sketch, &map, pos))
                    && let Some(grab) = map.to_sketch(pos)
                {
                    t.handle_drag = Some(HandleDrag {
                        handle: h,
                        base: sketch.clone(),
                        grab,
                    });
                }
            }
            PointerAction::Move { .. } => {
                if let Some(drag) = t.handle_drag.clone() {
                    if let Some(c) = map.to_sketch(pos) {
                        commands.queue(move |world: &mut World| handle_live(world, &drag, c));
                    }
                    continue;
                }
                // Press on a corner and drag: the fillet or chamfer follows (S9.1, S9.2).
                if tool.tool != SketchTool::Slot
                    && let Some((down, Some(SketchEntity::Point(p)))) = t.press
                    && (t.live.is_some() || down.distance(pos) > DRAG_THRESHOLD)
                    && corner_ok(sketch, p, chamfer)
                    && let Some(size) = live_size(sketch, &map, p, pos, chamfer)
                {
                    t.live = Some(LiveCorner {
                        corner: p,
                        size,
                        chamfer,
                    });
                }
            }
            PointerAction::Release(PointerButton::Primary) => {
                let Some((down, hit)) = t.press.take() else {
                    continue;
                };
                if let Some(drag) = t.handle_drag.take() {
                    if let Some(c) = map.to_sketch(pos)
                        && down.distance(pos) > DRAG_THRESHOLD
                    {
                        commands.queue(move |world: &mut World| handle_commit(world, drag, c));
                    }
                    continue;
                }
                if let Some(live) = t.live.take() {
                    let (corner, size) = (live.corner, live.size);
                    commands.queue(move |world: &mut World| {
                        make_corner(world, corner, chamfer, Some(size))
                    });
                    continue;
                }
                if down.distance(pos) > DRAG_THRESHOLD {
                    continue;
                }
                match tool.tool {
                    SketchTool::Slot => {
                        if let Some(SketchEntity::Curve(c)) = hit
                            && matches!(
                                sketch.curves.get(c).map(|c| c.kind),
                                Some(CurveKind::Line { .. } | CurveKind::Arc { .. })
                            )
                        {
                            commands.queue(move |world: &mut World| make_slot(world, c));
                        }
                    }
                    _ => {
                        let corner = corner_pick(sketch, &mut t, hit, chamfer);
                        selection.0 = t.first_line.map(SketchEntity::Curve).into_iter().collect();
                        if let Some(p) = corner {
                            commands.queue(move |world: &mut World| {
                                make_corner(world, p, chamfer, None)
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// The size a drag from `corner` to `cursor` (screen px) gives: how far the cursor is from the
/// corner along the lines (a chamfer's distance), or the fillet whose ends are that far.
fn live_size(s: &Sketch, map: &ScreenMap, corner: PointId, cursor: Vec2, chamfer: bool) -> Option<f64> {
    let c = map.to_sketch(cursor)?;
    let [(c1, a), (c2, b)] = if chamfer { corner_lines(s, corner)? } else { fillet_corner(s, corner)? };
    let p = s.pos(corner);
    let (pa, pb) = (s.pos(a), s.pos(b));
    let t = c.distance(p).min(p.distance(pa) * 0.999).min(p.distance(pb) * 0.999);
    if t < 1e-6 {
        return None;
    }
    if chamfer {
        return Some(t);
    }
    // The directions the curves leave the corner in (an arc's tangent there).
    let (u1, u2) = (s.direction_from(c1, corner)?, s.direction_from(c2, corner)?);
    let theta = u1.dot(u2).clamp(-1.0, 1.0).acos();
    Some(t * (theta / 2.0).tan())
}

/// Selection first: slots round the selected lines and arcs, fillets or chamfers at the
/// selected corners.
fn use_selection(world: &mut World, tool: SketchTool, picks: &[SketchEntity]) {
    for e in picks {
        match (tool, *e) {
            (SketchTool::Slot, SketchEntity::Curve(c)) => make_slot(world, c),
            (SketchTool::Fillet | SketchTool::Chamfer, SketchEntity::Point(p)) => {
                make_corner(world, p, tool == SketchTool::Chamfer, None)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_default_sizes() {
        assert_eq!(nice(9.4), 9.0);
        assert_eq!(nice(12.0), 10.0);
        assert_eq!(nice(0.034), 0.1);
        assert_eq!(nice(47.0), 50.0);
    }
}
