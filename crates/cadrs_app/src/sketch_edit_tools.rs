//! The Mirror and Offset sketch tools (`intro-to-sketching.md` S19), following Onshape:
//!
//! - **Mirror** (toolbar; Onshape gives it no key): pick the mirror line first (it turns
//!   yellow), then each entity picked is mirrored at once, as one undo step, linked to its
//!   source by a Symmetric constraint ([`SketchOp::Mirror`]). With a line and entities already
//!   selected, choosing Mirror mirrors them in the first-selected line.
//! - **Offset (O):** click an entity to offset it, or press on it and drag to take its whole
//!   chain (the lines and arcs joined end to end with it). The offset follows the cursor's
//!   distance on the side it was picked from; the arrow at the preview points to that side and
//!   clicking it flips the side. Clicking anywhere else places the offset, with a driving offset
//!   dimension, and opens a quick-dimension box to type the distance ([`SketchOp::Offset`]).
//!
//! - **Offset of a face region** (P3D.4, IR6.10): in a sketch on a part face, clicking inside
//!   the face (where no entity is) offsets the face's outer loop, its edges taken from the
//!   kernel as exact curves and used as construction geometry ([`SketchOp::OffsetLoop`]); the
//!   preview, the side, the arrow and the distance box work as for a chain.
//!
//! Both change the sketch only through [`cadrs_core::commands::EditSketch`] commands.

use std::collections::VecDeque;

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_sketch::edit::{OffsetShape, chain_of, offset_geometry};
use cadrs_sketch::geom::dist_point_segment;
use cadrs_sketch::hit::hit_test;
use cadrs_sketch::{CurveId, CurveKind, DimensionKind, PlaneFrame, Sketch, SketchEntity, SketchOp};
use cadrs_ui::input::TextInputField;

use crate::sketch::{ActiveSketchTool, PartStudioMode, SketchSession, SketchTool};
use crate::sketch_draw::SketchRubberGizmos;
use crate::sketch_tools::{
    QuickDimFlow, QuickDimTarget, SVec2, ScreenMap, SketchScreen, SketchSelection,
    SketchToolsSet, execute, over_viewport, session_sketch,
};
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct SketchEditToolsPlugin;

impl Plugin for SketchEditToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OffsetTool>()
            .add_systems(
                Update,
                (edit_tools_keys, edit_tools_pointer)
                    .chain()
                    .before(crate::sketch::sketch_keys)
                    .in_set(SketchToolsSet)
                    .after(crate::sketch_tools::sketch_pointer)
                    .run_if(in_state(PartStudioMode::Sketching))
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                draw_offset_preview
                    .after(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(PartStudioMode::Sketching), reset_offset);
    }
}

/// The Offset tool's state.
#[derive(Resource, Debug, Default)]
pub struct OffsetTool {
    /// The chain being offset (curve, runs backwards), empty until an entity is picked.
    pub chain: Vec<(CurveId, bool)>,
    /// Offset to the left of the chain's direction of travel.
    pub left: bool,
    /// The distance the preview is at (mm): the cursor's distance from the chain.
    pub distance: f64,
    /// The flip arrow on screen: its base and tip (px).
    pub arrow: Option<(Vec2, Vec2)>,
    press: Option<(Vec2, Option<SketchEntity>)>,
    cursor: Option<Vec2>,
    last_tool: SketchTool,
    /// P3D.4: offsetting the face region the sketch is on: its loop, and the sketch with the
    /// loop used (what `chain` is in).
    pub face: Option<FaceLoop>,
}

/// A face's outer loop being offset (P3D.4, IR6.10).
#[derive(Debug, Clone)]
pub struct FaceLoop {
    pub items: Vec<(cadrs_sketch::projection::Projected, cadrs_sketch::Link)>,
    pub sketch: Sketch,
}

impl OffsetTool {
    /// The sketch `chain` is in: the face's copy while offsetting a face region.
    fn chain_sketch<'a>(&'a self, sketch: &'a Sketch) -> &'a Sketch {
        self.face.as_ref().map_or(sketch, |f| &f.sketch)
    }

    fn clear(&mut self) {
        self.face = None;
        self.chain.clear();
        self.arrow = None;
        self.press = None;
    }
}

fn reset_offset(mut off: ResMut<OffsetTool>) {
    off.clear();
    off.last_tool = SketchTool::Select;
}

/// How near (px) a click must be to the flip arrow to flip it.
const ARROW_PICK: f32 = 10.0;
/// The flip arrow's length (px).
const ARROW_LEN: f32 = 28.0;

/// Esc drops the Offset tool's picked chain first (the next Esc leaves the tool).
#[allow(clippy::too_many_arguments)]
fn edit_tools_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    tool: Res<ActiveSketchTool>,
    mut off: ResMut<OffsetTool>,
    mut selection: ResMut<SketchSelection>,
    mut flow: ResMut<QuickDimFlow>,
    frame: Res<bevy::diagnostic::FrameCount>,
) {
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed || typing || k.key_code != KeyCode::Escape {
            continue;
        }
        if tool.tool == SketchTool::Offset && !off.chain.is_empty() {
            off.clear();
            selection.0.clear();
            flow.consumed_frame = Some(frame.0);
        }
    }
}

/// The Mirror and Offset tools' clicks.
#[allow(clippy::too_many_arguments)]
fn edit_tools_pointer(
    mut inputs: MessageReader<PointerInput>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    mut tool: ResMut<ActiveSketchTool>,
    mut selection: ResMut<SketchSelection>,
    mut off: ResMut<OffsetTool>,
    cache: Res<crate::parts::PartCache>,
    mut commands: Commands,
) {
    let (Some(map), Some(sketch), Some(s)) = (
        screen.active,
        session_sketch(session.as_deref(), doc.as_deref()),
        session.as_deref(),
    ) else {
        inputs.clear();
        return;
    };
    let target = (s.element, s.feature);
    // Switching tools: leaving Mirror or Offset drops their picks; choosing Mirror with a
    // line and entities selected mirrors them.
    if off.last_tool != tool.tool {
        let from = off.last_tool;
        off.last_tool = tool.tool;
        if matches!(from, SketchTool::Mirror | SketchTool::Offset) {
            off.clear();
            selection.0.clear();
        }
        match tool.tool {
            SketchTool::Mirror => {
                let first_line = selection.0.first().and_then(|e| match *e {
                    SketchEntity::Curve(c) if is_line(sketch, c) => Some(c),
                    _ => None,
                });
                match first_line {
                    Some(axis) if selection.0.len() > 1 => {
                        let curves: Vec<CurveId> = selection.0[1..]
                            .iter()
                            .filter_map(|e| match *e {
                                SketchEntity::Curve(c) => Some(c),
                                _ => None,
                            })
                            .collect();
                        selection.0.clear();
                        if !curves.is_empty() {
                            execute(&mut commands, target, SketchOp::Mirror { axis, curves });
                        }
                        tool.tool = SketchTool::Select;
                        off.last_tool = SketchTool::Select;
                    }
                    Some(_) => {}
                    None => selection.0.clear(),
                }
            }
            SketchTool::Offset => selection.0.clear(),
            _ => {}
        }
    }
    let active = matches!(tool.tool, SketchTool::Mirror | SketchTool::Offset);
    let over = over_viewport(&hover_map, &q_area);
    let hit_at = |pos: Vec2| {
        hit_test(sketch, SVec2::new(pos.x as f64, pos.y as f64), |p| {
            map.to_screen64(p)
        })
        .map(|h| h.entity)
    };
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse || !active {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Move { .. } => off.cursor = Some(pos),
            PointerAction::Press(PointerButton::Primary) => {
                off.cursor = Some(pos);
                off.press = over.then(|| (pos, hit_at(pos)));
            }
            PointerAction::Release(PointerButton::Primary) => {
                off.cursor = Some(pos);
                let Some((down, hit)) = off.press.take() else {
                    continue;
                };
                let moved = down.distance(pos) > crate::sketch_tools::DRAG_THRESHOLD;
                if tool.tool == SketchTool::Mirror {
                    if !moved {
                        mirror_pick(sketch, hit, &mut selection, &mut commands, target);
                    }
                    continue;
                }
                // Offset.
                if off.chain.is_empty() {
                    let Some(SketchEntity::Curve(c)) = hit else {
                        // P3D.4 (IR6.10): a click inside the face region the sketch is on.
                        let at = map.to_sketch(pos).unwrap_or_default();
                        if !moved
                            && let Some(items) = face_loop_items(doc.as_deref(), s, &cache)
                            && cadrs_sketch::face_offset::loop_contains(&items, at)
                        {
                            let mut temp = sketch.clone();
                            if let Ok(chain) = cadrs_sketch::face_offset::use_loop(&mut temp, &items) {
                                off.left = side_at(&temp, &chain, at);
                                off.chain = chain;
                                off.face = Some(FaceLoop { items, sketch: temp });
                                selection.0.clear();
                            }
                        }
                        continue;
                    };
                    off.chain = if moved {
                        chain_of(sketch, c).0
                    } else {
                        vec![(c, false)]
                    };
                    let at = map.to_sketch(pos).unwrap_or_default();
                    off.left = side_at(sketch, &off.chain, at);
                    selection.0 = off.chain.iter().map(|(c, _)| SketchEntity::Curve(*c)).collect();
                    continue;
                }
                if moved {
                    continue;
                }
                if let Some((base, tip)) = off.arrow
                    && dist_point_segment(
                        SVec2::new(pos.x as f64, pos.y as f64),
                        SVec2::new(base.x as f64, base.y as f64),
                        SVec2::new(tip.x as f64, tip.y as f64),
                    ) <= ARROW_PICK as f64
                {
                    // The arrow flips the side.
                    off.left = !off.left;
                    continue;
                }
                if let Some(face) = off.face.clone() {
                    place_face_offset(sketch, &face, &map, &off, pos, &mut commands, target);
                } else {
                    place_offset(sketch, &map, &off, pos, &mut commands, target);
                }
                off.clear();
                selection.0.clear();
            }
            _ => {}
        }
    }
    // The preview's distance follows the cursor.
    if tool.tool == SketchTool::Offset
        && !off.chain.is_empty()
        && let Some(c) = off.cursor.and_then(|c| map.to_sketch(c))
    {
        let csk = off.chain_sketch(sketch);
        let d = distance_to_chain(csk, &off.chain, c).max(0.01);
        let arrow = arrow_for(csk, &map, &off.chain, d, off.left);
        if off.distance != d {
            off.distance = d;
        }
        off.arrow = arrow;
    } else if off.arrow.is_some() {
        off.arrow = None;
    }
}

fn is_line(s: &Sketch, c: CurveId) -> bool {
    matches!(s.curves.get(c).map(|c| c.kind), Some(CurveKind::Line { .. }))
}

/// A Mirror click: the first pick must be a line (the mirror line, kept selected); each
/// entity picked after it is mirrored at once.
fn mirror_pick(
    sketch: &Sketch,
    hit: Option<SketchEntity>,
    selection: &mut SketchSelection,
    commands: &mut Commands,
    target: (cadrs_core::ElementId, cadrs_core::FeatureId),
) {
    let axis = selection.0.first().and_then(|e| match *e {
        SketchEntity::Curve(c) if is_line(sketch, c) => Some(c),
        _ => None,
    });
    match (axis, hit) {
        (None, Some(SketchEntity::Curve(c))) if is_line(sketch, c) => {
            selection.0 = vec![SketchEntity::Curve(c)];
        }
        (Some(axis), Some(SketchEntity::Curve(c))) if c != axis => {
            execute(
                commands,
                target,
                SketchOp::Mirror {
                    axis,
                    curves: vec![c],
                },
            );
        }
        _ => {}
    }
}

/// Which side of the chain `p` is on: true for the left of its direction of travel (for an arc
/// running counter-clockwise, or a circle, the inside).
pub fn side_at(s: &Sketch, chain: &[(CurveId, bool)], p: SVec2) -> bool {
    let nearest = chain
        .iter()
        .min_by(|a, b| {
            piece_distance(s, a.0, p, true).total_cmp(&piece_distance(s, b.0, p, true))
        })
        .copied();
    let Some((c, reversed)) = nearest else {
        return true;
    };
    match s.curves.get(c).map(|c| c.kind) {
        Some(CurveKind::Line { a, b }) => {
            let (from, to) = if reversed {
                (s.pos(b), s.pos(a))
            } else {
                (s.pos(a), s.pos(b))
            };
            (to - from).cross(p - from) > 0.0
        }
        Some(CurveKind::Arc { center, start, .. }) => {
            let inside = p.distance(s.pos(center)) < s.pos(start).distance(s.pos(center));
            inside != reversed
        }
        Some(CurveKind::Circle { center, radius }) => p.distance(s.pos(center)) < radius,
        // A closed ellipse (or offset ellipse) runs counter-clockwise for a positive minor radius,
        // and left of a counter-clockwise run is inside.
        Some(CurveKind::Ellipse { minor, .. } | CurveKind::EllipseOffset { minor, .. }) => {
            let inside = s.ellipse_geom(c).is_some_and(|g| g.implicit(p) < 0.0);
            inside == ((minor > 0.0) != reversed)
        }
        Some(CurveKind::Spline { .. }) => true,
        // Not offset (the Offset tool refuses it).
        Some(CurveKind::Bezier { .. }) => true,
        None => true,
    }
}

/// The distance from `p` to a curve (`bounded`: to the segment or arc itself; otherwise to its
/// infinite line or whole circle).
fn piece_distance(s: &Sketch, c: CurveId, p: SVec2, bounded: bool) -> f64 {
    match s.curves.get(c).map(|c| c.kind) {
        Some(CurveKind::Line { a, b }) => {
            let (a, b) = (s.pos(a), s.pos(b));
            if bounded {
                dist_point_segment(p, a, b)
            } else {
                let f = cadrs_sketch::dimension::foot(p, a, b);
                p.distance(f)
            }
        }
        Some(CurveKind::Arc { .. }) => {
            let g = s.arc_geom(c).unwrap_or(cadrs_sketch::ArcGeom {
                center: p,
                radius: 0.0,
                start_angle: 0.0,
                sweep: 0.0,
            });
            if bounded {
                g.distance(p)
            } else {
                (p.distance(g.center) - g.radius).abs()
            }
        }
        Some(CurveKind::Circle { center, radius }) => (p.distance(s.pos(center)) - radius).abs(),
        Some(CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. }) => s.ellipse_geom(c).map_or(f64::INFINITY, |g| g.distance(p)),
        Some(CurveKind::Spline { .. }) => s
            .spline_spans(c)
            .and_then(|sp| cadrs_sketch::spline::nearest(&sp, p))
            .map_or(f64::INFINITY, |(_, _, d)| d),
        Some(CurveKind::Bezier { .. }) => s.bezier_geom(c).map_or(f64::INFINITY, |g| g.distance(p)),
        None => f64::INFINITY,
    }
}

/// The offset distance for the cursor at `p`: its distance from the nearest piece of the
/// chain (measured square to it).
pub fn distance_to_chain(s: &Sketch, chain: &[(CurveId, bool)], p: SVec2) -> f64 {
    let nearest = chain
        .iter()
        .min_by(|a, b| {
            piece_distance(s, a.0, p, true).total_cmp(&piece_distance(s, b.0, p, true))
        })
        .copied();
    nearest.map_or(0.0, |(c, _)| piece_distance(s, c, p, false))
}

/// The middle of a shape and of its source curve.
fn shape_mid(s: &Sketch, sh: &OffsetShape) -> SVec2 {
    let pts = sh.polyline(s);
    match *sh {
        OffsetShape::Line(a, b) => a.midpoint(b),
        OffsetShape::Arc { geom, .. } => geom.mid(),
        OffsetShape::Circle { .. } | OffsetShape::Ellipse { .. } => pts.first().copied().unwrap_or_default(),
    }
}

/// The flip arrow: from the middle of the first offset piece, pointing away from its source
/// (screen px).
fn arrow_for(
    s: &Sketch,
    map: &ScreenMap,
    chain: &[(CurveId, bool)],
    distance: f64,
    left: bool,
) -> Option<(Vec2, Vec2)> {
    let shapes = offset_geometry(s, chain, distance, left).ok()?;
    let first = shapes.first()?;
    let mid = shape_mid(s, first);
    // The point of the source nearest the offset's middle.
    let (c, _) = chain[0];
    let src = match s.curves.get(c)?.kind {
        CurveKind::Line { a, b } => cadrs_sketch::dimension::foot(mid, s.pos(a), s.pos(b)),
        CurveKind::Arc { center, start, .. } => {
            let o = s.pos(center);
            o + (mid - o).normalize() * o.distance(s.pos(start))
        }
        CurveKind::Circle { center, radius } => {
            let o = s.pos(center);
            o + (mid - o).normalize() * radius
        }
        CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => s.ellipse_geom(c)?.closest(mid),
        CurveKind::Spline { .. } => return None,
        CurveKind::Bezier { .. } => {
            let g = s.bezier_geom(c)?;
            g.point_at(g.nearest_t(mid))
        }
    };
    let (base, from) = (map.to_screen(mid), map.to_screen(src));
    let dir = (base - from).normalize_or_zero();
    if dir == Vec2::ZERO {
        return None;
    }
    Some((base, base + dir * ARROW_LEN))
}

/// Places the offset at the preview's distance, with its dimension's label at the cursor, and
/// opens the distance box.
fn place_offset(
    sketch: &Sketch,
    map: &ScreenMap,
    off: &OffsetTool,
    pos: Vec2,
    commands: &mut Commands,
    target: (cadrs_core::ElementId, cadrs_core::FeatureId),
) {
    let chain = off.chain.clone();
    let (distance, left) = (off.distance, off.left);
    // Where the dimension goes: laid out on a trial copy, so its label sits at the cursor.
    let cursor = map.to_sketch(pos).unwrap_or_default();
    let mut trial = sketch.clone();
    let before: Vec<CurveId> = sketch.curves.keys().collect();
    let label = match cadrs_sketch::edit::offset(&mut trial, &chain, distance, left, (0.0, 0.0)) {
        Ok(made) => trial
            .dimensions
            .values()
            .find(|d| matches!(d.kind, DimensionKind::Offset { target, .. } if target == made[0]))
            .and_then(|d| cadrs_sketch::dimension::label_params(&trial, d.kind, cursor))
            .unwrap_or((0.0, 0.0)),
        Err(e) => {
            warn!("cannot offset: {e}");
            commands.queue(offset_failed_toast);
            return;
        }
    };
    let source = chain[0].0;
    // Between arcs or circles the distance is drawn along the normal with the value between
    // the two curves (`ex2-step8.png`): through an arc's middle, or toward the cursor on a
    // circle.
    let label = match sketch.curves.get(source).map(|c| c.kind) {
        Some(CurveKind::Arc { .. }) => sketch
            .arc_geom(source)
            .map(|g| ((g.mid() - g.center).angle(), 0.0))
            .unwrap_or(label),
        Some(CurveKind::Circle { .. }) => (label.0, 0.0),
        _ => label,
    };
    execute(
        commands,
        target,
        SketchOp::Offset {
            chain,
            distance,
            left,
            label,
        },
    );
    commands.queue(move |world: &mut World| {
        let Some(s) = crate::sketch_tools::world_sketch(world) else {
            return;
        };
        let id = s.dimensions.iter().find_map(|(k, d)| match d.kind {
            DimensionKind::Offset { source: src, target } if src == source && !before.contains(&target) => {
                Some(k)
            }
            _ => None,
        });
        if let Some(id) = id {
            crate::sketch_tools::open_quick_dims(world, VecDeque::from([QuickDimTarget::Dim(id)]));
        }
    });
}

/// The outer loop of the part face the sketch being edited lies on, as exact curves with their
/// links (P3D.4, IR6.10); `None` for a sketch on a plane.
fn face_loop_items(
    doc: Option<&ActiveDocument>,
    s: &SketchSession,
    cache: &crate::parts::PartCache,
) -> Option<Vec<(cadrs_sketch::projection::Projected, cadrs_sketch::Link)>> {
    let el = doc?.doc.element(s.element)?;
    let features = el.features();
    let i = features.iter().position(|f| f.id == s.feature)?;
    let plane = features[i].sketch()?.plane?;
    let items = cadrs_core::repair::face_loop(&features[..i], &cache.parts, &plane);
    (!items.is_empty()).then_some(items)
}

/// Places the offset of a face's outer loop (P3D.4, IR6.10) at the preview's distance, with
/// its dimension's label at the cursor, and opens the distance box.
fn place_face_offset(
    sketch: &Sketch,
    face: &FaceLoop,
    map: &ScreenMap,
    off: &OffsetTool,
    pos: Vec2,
    commands: &mut Commands,
    target: (cadrs_core::ElementId, cadrs_core::FeatureId),
) {
    let (distance, left) = (off.distance, off.left);
    let cursor = map.to_sketch(pos).unwrap_or_default();
    let mut trial = face.sketch.clone();
    let label = match cadrs_sketch::edit::offset(&mut trial, &off.chain, distance, left, (0.0, 0.0)) {
        Ok(made) => trial
            .dimensions
            .values()
            .find(|d| matches!(d.kind, DimensionKind::Offset { target, .. } if target == made[0]))
            .and_then(|d| cadrs_sketch::dimension::label_params(&trial, d.kind, cursor))
            .unwrap_or((0.0, 0.0)),
        Err(e) => {
            warn!("cannot offset: {e}");
            commands.queue(offset_failed_toast);
            return;
        }
    };
    let before: Vec<cadrs_sketch::DimensionId> = sketch.dimensions.keys().collect();
    execute(commands, target, SketchOp::OffsetLoop { items: face.items.clone(), distance, left, label });
    commands.queue(move |world: &mut World| {
        let Some(s) = crate::sketch_tools::world_sketch(world) else {
            return;
        };
        let id = s
            .dimensions
            .iter()
            .find_map(|(k, d)| (matches!(d.kind, DimensionKind::Offset { .. }) && !before.contains(&k)).then_some(k));
        if let Some(id) = id {
            crate::sketch_tools::open_quick_dims(world, VecDeque::from([QuickDimTarget::Dim(id)]));
        }
    });
}

/// P3D.1 (IR5.3): the yellow warning when an offset can't be made at the distance asked
/// (`ex1-step10.png`).
pub(crate) fn offset_failed_toast(world: &mut World) {
    let theme = world.resource::<cadrs_ui::Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::show_notification(
        &mut commands,
        &theme,
        cadrs_ui::Notification::warning("Offset could not be created at this distance.")
            .name("sketch-offset-toast")
            // Right of the sketch dialog's title (`ex1-step10.png`).
            .at(212.0, 6.0),
    );
    world.flush();
}

/// P3D.1 (IR5.3): true if giving the Offset dimension `id` the value in `op` leaves an offset
/// that can't exist: the solve can't hold the dimension, or an offset circle, arc or ellipse
/// would have no radius left (offset inward past its center).
pub(crate) fn offset_value_fails(s: &Sketch, id: cadrs_sketch::DimensionId, op: &SketchOp) -> bool {
    let Some(DimensionKind::Offset { source, target }) = s.dimensions.get(id).map(|d| d.kind) else {
        return false;
    };
    // Between circles or arcs: which side of its source the offset is on (outside: +).
    let radius = |g: &Sketch, c: CurveId| match g.curves.get(c).map(|c| c.kind) {
        Some(CurveKind::Circle { radius, .. }) => Some(radius),
        Some(CurveKind::Arc { .. }) => g.arc_geom(c).map(|a| a.radius),
        _ => None,
    };
    let side = |g: &Sketch| Some((radius(g, target)? - radius(g, source)?).signum());
    let mut trial = s.clone();
    if op.apply(&mut trial).is_err() {
        return true;
    }
    if cadrs_sketch::solve::conflicts(&trial).contains(&cadrs_sketch::solve::Source::Dimension(id)) {
        return true;
    }
    // The solve may meet the distance on the other side (a circle offset 10 inward from a
    // radius of 6 becomes one 10 outward): Onshape refuses that.
    if let (Some(a), Some(b)) = (side(s), side(&trial))
        && a != b
    {
        return true;
    }
    let bad = |r: f64| !(r.is_finite() && r > 1e-9);
    match trial.curves.get(target).map(|c| c.kind) {
        Some(CurveKind::Circle { radius, .. }) => bad(radius),
        Some(CurveKind::Arc { .. }) => trial.arc_geom(target).is_none_or(|g| bad(g.radius)),
        Some(CurveKind::EllipseOffset { .. }) => trial.ellipse_geom(target).is_none(),
        Some(_) => false,
        None => true,
    }
}

/// The offset preview: the offset curves in the rubber-band blue and the flip arrow.
#[allow(clippy::too_many_arguments)]
fn draw_offset_preview(
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    tool: Res<ActiveSketchTool>,
    off: Res<OffsetTool>,
    mut gizmos: Gizmos<SketchRubberGizmos>,
) {
    if tool.tool != SketchTool::Offset || off.chain.is_empty() {
        return;
    }
    let (Some(map), Some(sketch)) = (
        screen.active,
        session_sketch(session.as_deref(), doc.as_deref()),
    ) else {
        return;
    };
    let frame = map.plane.frame();
    let sketch = off.chain_sketch(sketch);
    let Ok(shapes) = offset_geometry(sketch, &off.chain, off.distance, off.left) else {
        return;
    };
    let color = Color::srgb_u8(0x44, 0x9c, 0xcd);
    for sh in &shapes {
        gizmos.linestrip(sh.polyline(sketch).into_iter().map(|p| world(&frame, p)), color);
    }
    // The arrow: a shaft and a head, drawn on the sketch plane from its screen position.
    if let Some((base, tip)) = off.arrow {
        let dir = (tip - base).normalize_or_zero();
        let side = dir.perp();
        let head = [tip - dir * 9.0 + side * 5.0, tip, tip - dir * 9.0 - side * 5.0];
        let to_w = |p: Vec2| map.to_sketch(p).map(|q| world(&frame, q));
        if let (Some(a), Some(b)) = (to_w(base), to_w(tip)) {
            gizmos.line(a, b, Color::srgb_u8(0x33, 0x33, 0x33));
        }
        let pts: Vec<Vec3> = head.iter().filter_map(|p| to_w(*p)).collect();
        if pts.len() == 3 {
            gizmos.linestrip(pts, Color::srgb_u8(0x33, 0x33, 0x33));
        }
    }
}

fn world(frame: &PlaneFrame, p: SVec2) -> Vec3 {
    let w = frame.to_world(p);
    Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P3D.1 (IR5.3): a circle of radius 6 offset 3 inward can take 2, not 10 (that would
    /// put it on the other side, radius 16): "Offset could not be created at this distance."
    #[test]
    fn an_impossible_offset_distance_is_refused() {
        let mut s = Sketch::new();
        SketchOp::AddCircle { center: SVec2::new(0.0, 0.0), radius: 6.0, construction: false }.apply(&mut s).unwrap();
        let c = s.curves.keys().next().unwrap();
        let chain = vec![(c, false)];
        let left = side_at(&s, &chain, SVec2::new(3.0, 0.0));
        SketchOp::Offset { chain, distance: 3.0, left, label: (0.0, 0.0) }.apply(&mut s).unwrap();
        let (id, _) = s.dimensions.iter().find(|(_, d)| matches!(d.kind, DimensionKind::Offset { .. })).unwrap();
        let inner = s.curves.iter().find(|(k, _)| *k != c).map(|(_, v)| v.kind.scalar().unwrap()).unwrap();
        assert!((inner - 3.0).abs() < 1e-6, "{inner}");
        assert!(!offset_value_fails(&s, id, &SketchOp::SetDimensionValue { id, value: 2.0 }));
        assert!(offset_value_fails(&s, id, &SketchOp::SetDimensionValue { id, value: 10.0 }));
    }

    #[test]
    fn offset_side_and_distance_follow_the_cursor() {
        let mut s = Sketch::new();
        SketchOp::AddArc {
            center: SVec2::new(0.0, 0.0),
            start: SVec2::new(75.0, 0.0),
            end: SVec2::new(-75.0, 0.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let arc = s.curves.keys().next().unwrap();
        let chain = vec![(arc, false)];
        // Inside the (counter-clockwise) arc: its left.
        assert!(side_at(&s, &chain, SVec2::new(0.0, 70.0)));
        assert!(!side_at(&s, &chain, SVec2::new(0.0, 80.0)));
        assert!((distance_to_chain(&s, &chain, SVec2::new(0.0, 35.0)) - 40.0).abs() < 1e-9);
    }
}
