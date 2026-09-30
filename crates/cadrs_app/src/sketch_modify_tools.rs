//! Trim, Extend and Split (`intro-to-sketching.md` S17, S18; Trim ▾ in the sketch toolbar),
//! following Onshape's help (`reference/onshape/edit_tools.md`):
//!
//! - **Trim (M):** hovering a curve shows, in the hover tan, the piece a click removes: back to
//!   the crossings on either side (`edit_tools/trim-points-03.png`), or the whole curve if
//!   nothing crosses it. Clicking removes it. Pressing and dragging draws a thin grey trail;
//!   every curve the trail crosses is trimmed there as it is crossed
//!   (`trim-points-02.png`), and releasing records the whole drag as one undo step. A
//!   standalone point is deleted.
//! - **Extend (X):** hovering a line or arc shows, in the rubber-band blue, how its nearer
//!   free end would grow to the next curve in the way. Clicking picks it; the extension then
//!   follows the cursor (to the first curve in the way before the cursor, or to the cursor),
//!   and a second click, or releasing a press-drag, extends it (`extend-line-02.png`).
//! - **Split:** hovering a curve shows the point it would be split at (snapped to the
//!   inference points on it: midpoints, crossings, points). Clicking splits a line or arc
//!   there; a circle takes two clicks (`sketch_split.htm`: closed curves need two points).
//!
//! Every edit is an [`EditSketch`](cadrs_core::commands::EditSketch) command.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_sketch::hit::{CURVE_TOLERANCE_PX, POINT_TOLERANCE_PX, curve_distance_px};
use cadrs_sketch::infer::{self, ToolContext};
use cadrs_sketch::modify::{closest_on, extension, free_end_near, trim_removed_path};
use cadrs_sketch::{CurveId, CurveKind, PlaneFrame, PointId, Sketch, SketchOp};
use cadrs_ui::input::TextInputField;

use crate::sketch::{ActiveSketchTool, PartStudioMode, SketchSession, SketchTool};
use crate::sketch_draw::{SketchDotGizmos, SketchRubberGizmos, SketchThinGizmos};
use crate::sketch_tools::{
    DRAG_THRESHOLD, QuickDimFlow, SVec2, ScreenMap, SketchScreen, SketchToolsSet, execute,
    over_viewport, session_sketch,
};
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct SketchModifyToolsPlugin;

impl Plugin for SketchModifyToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModifyTool>()
            .add_systems(
                Update,
                (modify_keys, modify_pointer)
                    .chain()
                    .in_set(SketchToolsSet)
                    .after(crate::sketch_tools::sketch_pointer)
                    .before(crate::sketch::sketch_keys)
                    .run_if(in_state(PartStudioMode::Sketching))
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                draw_modify_preview
                    .after(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(PartStudioMode::Sketching), reset_modify);
    }
}

/// What the tools show under the cursor.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Preview {
    #[default]
    None,
    /// Trim: the piece of `curve` a click removes (sketch mm) and what is left of it.
    Trim {
        curve: CurveId,
        removed: Vec<SVec2>,
        kept: Vec<Vec<SVec2>>,
    },
    /// Trim: a standalone point a click deletes.
    TrimPoint(SVec2),
    /// Extend: the curve and the added part.
    Extend { curve: CurveId, path: Vec<SVec2> },
    /// Split: the curve and the split point (and the first point picked on a circle).
    Split {
        curve: CurveId,
        at: SVec2,
        first: Option<SVec2>,
    },
}

/// A drag-trim in progress.
#[derive(Debug, Clone)]
pub struct TrimDrag {
    /// The sketch when the drag began (crossings are found on it).
    pub base: Sketch,
    /// The trail (screen px).
    pub trail: Vec<Vec2>,
    /// What the trail has crossed so far, in order.
    pub picks: Vec<(CurveId, SVec2)>,
}

/// The Trim, Extend and Split tools' state.
#[derive(Resource, Debug, Default)]
pub struct ModifyTool {
    last_tool: SketchTool,
    press: Option<Vec2>,
    cursor: Option<Vec2>,
    pub drag: Option<TrimDrag>,
    /// Extend: the picked curve and the end it grows from.
    pub extend: Option<(CurveId, PointId)>,
    /// The Extend curve was picked by the current press (its release only picks).
    picked_on_press: bool,
    /// Split: the first point picked on a circle.
    pub split_first: Option<(CurveId, SVec2)>,
    pub preview: Preview,
}

impl ModifyTool {
    /// The curve Extend or Split highlights (drawn in the hover colours).
    pub fn hovered_curve(&self, tool: SketchTool) -> Option<CurveId> {
        match (&self.preview, tool) {
            (Preview::Extend { curve, .. }, SketchTool::Extend)
            | (Preview::Split { curve, .. }, SketchTool::Split) => Some(*curve),
            _ => None,
        }
    }

    fn clear(&mut self) {
        self.press = None;
        self.drag = None;
        self.extend = None;
        self.split_first = None;
        self.preview = Preview::None;
    }
}

fn reset_modify(mut t: ResMut<ModifyTool>) {
    t.clear();
    t.last_tool = SketchTool::Select;
}

fn is_modify(t: SketchTool) -> bool {
    matches!(t, SketchTool::Trim | SketchTool::Extend | SketchTool::Split)
}

/// Esc drops a picked Extend curve or Split point first (the next Esc leaves the tool).
fn modify_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    tool: Res<ActiveSketchTool>,
    mut t: ResMut<ModifyTool>,
    mut flow: ResMut<QuickDimFlow>,
    frame: Res<bevy::diagnostic::FrameCount>,
) {
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed || typing || k.key_code != KeyCode::Escape {
            continue;
        }
        if is_modify(tool.tool) && (t.extend.is_some() || t.split_first.is_some()) {
            t.extend = None;
            t.split_first = None;
            t.preview = Preview::None;
            flow.consumed_frame = Some(frame.0);
        }
    }
}

/// The curve under a screen point (nearest within the pick tolerance).
fn curve_at(s: &Sketch, map: &ScreenMap, pos: Vec2) -> Option<CurveId> {
    let c = SVec2::new(pos.x as f64, pos.y as f64);
    s.curves
        .keys()
        .map(|k| (k, curve_distance_px(s, k, c, &|p| map.to_screen64(p))))
        .filter(|(_, d)| *d <= CURVE_TOLERANCE_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// A standalone point (no curve uses it) under a screen point.
fn lone_point_at(s: &Sketch, map: &ScreenMap, pos: Vec2) -> Option<PointId> {
    s.points
        .iter()
        .filter(|(k, _)| !s.point_in_use(*k) && !s.hollow_point(*k))
        .map(|(k, p)| (k, map.to_screen(p.pos).distance(pos)))
        .filter(|(_, d)| *d <= POINT_TOLERANCE_PX as f32)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// Where a Split at the cursor cuts `curve`: an inference point on it (a midpoint, a crossing,
/// a point) if one is near, else the nearest point of the curve.
fn split_at(s: &Sketch, map: &ScreenMap, curve: CurveId, pos: Vec2) -> Option<SVec2> {
    let raw = map.to_sketch(pos)?;
    let on = closest_on(s, curve, raw)?;
    let snapped = infer::infer(raw, map.px_per_mm() as f64, s, &ToolContext::default())
        .into_iter()
        .map(|c| c.pos)
        .find(|p| closest_on(s, curve, *p).is_some_and(|q| q.distance(*p) < 1e-6 * (1.0 + p.length())));
    Some(snapped.unwrap_or(on))
}

/// True for curves Extend grows (lines and arcs).
fn extendable(s: &Sketch, c: CurveId) -> bool {
    matches!(
        s.curves.get(c).map(|c| c.kind),
        Some(CurveKind::Line { .. } | CurveKind::Arc { .. })
    )
}

/// Where two screen segments cross: the parameter along the first.
fn seg_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<f32> {
    let r = b - a;
    let q = d - c;
    let den = r.perp_dot(q);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = (c - a).perp_dot(q) / den;
    let u = (c - a).perp_dot(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// The curves of `s` a trail segment crosses, in order along it, with where (sketch mm, on the
/// curve).
fn trail_crossings(s: &Sketch, map: &ScreenMap, a: Vec2, b: Vec2) -> Vec<(f32, CurveId, SVec2)> {
    let mut out = Vec::new();
    for k in s.curves.keys() {
        let pts: Vec<Vec2> = cadrs_sketch::hit::curve_polyline(s, k)
            .into_iter()
            .map(|p| map.to_screen(p))
            .collect();
        for w in pts.windows(2) {
            if let Some(t) = seg_cross(a, b, w[0], w[1])
                && let Some(p) = map.to_sketch(a.lerp(b, t))
                && let Some(on) = closest_on(s, k, p)
            {
                out.push((t, k, on));
            }
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    out
}

#[allow(clippy::too_many_arguments)]
fn modify_pointer(
    mut inputs: MessageReader<PointerInput>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    tool: Res<ActiveSketchTool>,
    mut t: ResMut<ModifyTool>,
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
    if t.last_tool != tool.tool {
        t.last_tool = tool.tool;
        if let Some(d) = t.drag.take() {
            // Leaving mid-drag: put the sketch back.
            commands.queue(move |world: &mut World| {
                if let Some(sk) = crate::sketch_constrain::sketch_mut(world) {
                    *sk = d.base;
                }
            });
        }
        t.clear();
    }
    if !is_modify(tool.tool) {
        inputs.clear();
        return;
    }
    let over = over_viewport(&hover_map, &q_area);
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Move { .. } => {
                t.cursor = Some(pos);
                // A Trim press dragged far enough starts the trail.
                if tool.tool == SketchTool::Trim
                    && let Some(down) = t.press
                    && (t.drag.is_some() || down.distance(pos) > DRAG_THRESHOLD)
                {
                    let mut d = t.drag.take().unwrap_or_else(|| TrimDrag {
                        base: sketch.clone(),
                        trail: vec![down],
                        picks: Vec::new(),
                    });
                    let last = *d.trail.last().unwrap_or(&down);
                    let before = d.picks.len();
                    for (_, k, at) in trail_crossings(&d.base, &map, last, pos) {
                        if !d.picks.iter().any(|(c, p)| *c == k && p.distance(at) < 1e-6) {
                            d.picks.push((k, at));
                        }
                    }
                    d.trail.push(pos);
                    if d.picks.len() != before {
                        // The trimmed sketch shows live (no undo step until the release).
                        let (base, picks) = (d.base.clone(), d.picks.clone());
                        commands.queue(move |world: &mut World| {
                            let mut trial = base.clone();
                            let op = SketchOp::Trim {
                                picks,
                                points: vec![],
                            };
                            if op.apply(&mut trial).is_err() {
                                trial = base;
                            }
                            if let Some(sk) = crate::sketch_constrain::sketch_mut(world) {
                                *sk = trial;
                            }
                        });
                    }
                    t.drag = Some(d);
                }
            }
            PointerAction::Press(PointerButton::Primary) => {
                t.cursor = Some(pos);
                t.press = over.then_some(pos);
                t.picked_on_press = false;
                // Extend: a press on a line or arc picks it (the release after a drag, or a
                // second click, extends it).
                if tool.tool == SketchTool::Extend
                    && over
                    && t.extend.is_none()
                    && let Some(c) = curve_at(sketch, &map, pos).filter(|c| extendable(sketch, *c))
                    && let Some(end) = map.to_sketch(pos).and_then(|p| free_end_near(sketch, c, p))
                {
                    t.extend = Some((c, end));
                    t.picked_on_press = true;
                }
            }
            PointerAction::Release(PointerButton::Primary) => {
                t.cursor = Some(pos);
                let Some(down) = t.press.take() else {
                    continue;
                };
                let moved = down.distance(pos) > DRAG_THRESHOLD;
                match tool.tool {
                    SketchTool::Trim => {
                        if let Some(d) = t.drag.take() {
                            // The whole drag: one step.
                            commands.queue(move |world: &mut World| {
                                if let Some(sk) = crate::sketch_constrain::sketch_mut(world) {
                                    *sk = d.base;
                                }
                            });
                            if !d.picks.is_empty() {
                                execute(
                                    &mut commands,
                                    target,
                                    SketchOp::Trim {
                                        picks: d.picks,
                                        points: vec![],
                                    },
                                );
                            }
                            continue;
                        }
                        if moved {
                            continue;
                        }
                        if let Some(p) = lone_point_at(sketch, &map, pos) {
                            execute(
                                &mut commands,
                                target,
                                SketchOp::Trim {
                                    picks: vec![],
                                    points: vec![p],
                                },
                            );
                        } else if let Some(c) = curve_at(sketch, &map, pos)
                            && let Some(at) = map.to_sketch(pos).and_then(|p| closest_on(sketch, c, p))
                        {
                            execute(
                                &mut commands,
                                target,
                                SketchOp::Trim {
                                    picks: vec![(c, at)],
                                    points: vec![],
                                },
                            );
                        }
                        t.preview = Preview::None;
                    }
                    SketchTool::Extend => {
                        let cursor = map.to_sketch(pos);
                        match t.extend {
                            // A click that picked: wait for the second click.
                            Some(_) if t.picked_on_press && !moved => {}
                            // The second click (or the release of a drag) extends.
                            Some((c, end)) => {
                                if let Some(ext) = cursor.and_then(|p| extension(sketch, c, end, Some(p))) {
                                    execute(
                                        &mut commands,
                                        target,
                                        SketchOp::Extend {
                                            curve: c,
                                            end,
                                            to: ext.to,
                                            by: ext.by,
                                        },
                                    );
                                }
                                t.extend = None;
                                t.preview = Preview::None;
                            }
                            None => {}
                        }
                    }
                    SketchTool::Split => {
                        if moved {
                            continue;
                        }
                        let Some(c) = curve_at(sketch, &map, pos) else {
                            continue;
                        };
                        let Some(at) = split_at(sketch, &map, c, pos) else {
                            continue;
                        };
                        let circle = matches!(
                            sketch.curves.get(c).map(|c| c.kind),
                            Some(CurveKind::Circle { .. })
                        );
                        match (circle, t.split_first) {
                            (true, Some((first_c, first))) if first_c == c => {
                                if first.distance(at) > 1e-6 {
                                    execute(
                                        &mut commands,
                                        target,
                                        SketchOp::Split {
                                            curve: c,
                                            at: vec![first, at],
                                        },
                                    );
                                    t.split_first = None;
                                }
                            }
                            (true, _) => t.split_first = Some((c, at)),
                            (false, _) => {
                                if matches!(
                                    sketch.curves.get(c).map(|c| c.kind),
                                    Some(CurveKind::Line { .. } | CurveKind::Arc { .. })
                                ) {
                                    execute(
                                        &mut commands,
                                        target,
                                        SketchOp::Split {
                                            curve: c,
                                            at: vec![at],
                                        },
                                    );
                                }
                                t.split_first = None;
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    // The preview for the cursor's position.
    let Some(cursor) = t.cursor.filter(|_| over || t.drag.is_some() || t.extend.is_some()) else {
        if t.preview != Preview::None {
            t.preview = Preview::None;
        }
        return;
    };
    let raw = map.to_sketch(cursor);
    let preview = match tool.tool {
        SketchTool::Trim if t.drag.is_some() => Preview::None,
        SketchTool::Trim => {
            if let Some(p) = lone_point_at(sketch, &map, cursor) {
                Preview::TrimPoint(sketch.pos(p))
            } else {
                curve_at(sketch, &map, cursor)
                    .zip(raw)
                    .and_then(|(c, p)| trim_preview(sketch, c, closest_on(sketch, c, p)?))
                    .unwrap_or_default()
            }
        }
        SketchTool::Extend => match t.extend {
            Some((c, end)) => raw
                .and_then(|p| extension(sketch, c, end, Some(p)))
                .map_or(Preview::Extend { curve: c, path: vec![] }, |e| Preview::Extend {
                    curve: c,
                    path: e.path,
                }),
            None => curve_at(sketch, &map, cursor)
                .filter(|c| extendable(sketch, *c))
                .zip(raw)
                .and_then(|(c, p)| {
                    let end = free_end_near(sketch, c, p)?;
                    Some(Preview::Extend {
                        curve: c,
                        path: extension(sketch, c, end, None).map(|e| e.path).unwrap_or_default(),
                    })
                })
                .unwrap_or_default(),
        },
        SketchTool::Split => {
            let first = t.split_first.map(|(_, p)| p);
            curve_at(sketch, &map, cursor)
                .and_then(|c| {
                    Some(Preview::Split {
                        curve: c,
                        at: split_at(sketch, &map, c, cursor)?,
                        first: t.split_first.filter(|(k, _)| *k == c).map(|(_, p)| p),
                    })
                })
                .or_else(|| {
                    let (c, p) = t.split_first?;
                    Some(Preview::Split {
                        curve: c,
                        at: p,
                        first,
                    })
                })
                .unwrap_or_default()
        }
        _ => Preview::None,
    };
    if t.preview != preview {
        t.preview = preview;
    }
}

/// The Trim hover preview for a pick on `c` at `at`: the removed piece and the kept ones (the
/// pieces of `c` left by a trial trim).
fn trim_preview(s: &Sketch, c: CurveId, at: SVec2) -> Option<Preview> {
    let removed = trim_removed_path(s, c, at)?;
    let mut trial = s.clone();
    cadrs_sketch::modify::trim(&mut trial, c, at).ok()?;
    let kept = trial
        .curves
        .keys()
        .filter(|k| *k == c || !s.curves.contains_key(*k))
        .map(|k| cadrs_sketch::hit::curve_polyline(&trial, k))
        .collect();
    Some(Preview::Trim {
        curve: c,
        removed,
        kept,
    })
}

fn world(frame: &PlaneFrame, p: SVec2) -> Vec3 {
    let w = frame.to_world(p);
    Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32)
}

/// The hover tan (`edit_tools/trim-points-03.png`).
fn tan() -> Color {
    Color::srgb_u8(0xf0, 0xbe, 0x8a)
}

/// The previews: the piece Trim removes, the extension, the split point, and the drag trail.
#[allow(clippy::too_many_arguments)]
fn draw_modify_preview(
    screen: Res<SketchScreen>,
    tool: Res<ActiveSketchTool>,
    t: Res<ModifyTool>,
    mut rubber: Gizmos<SketchRubberGizmos>,
    mut thin: Gizmos<SketchThinGizmos>,
    mut dots: Gizmos<SketchDotGizmos>,
) {
    if !is_modify(tool.tool) {
        return;
    }
    let Some(map) = screen.active else {
        return;
    };
    let frame = map.plane.frame();
    let px = map.px_per_mm();
    let n = frame.normal();
    let rot = Quat::from_rotation_arc(Vec3::Z, Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32));
    let mut dot = |p: SVec2, r: f32, color: Color| {
        dots.circle(Isometry3d::new(world(&frame, p), rot), r / px, color)
            .resolution(12);
    };
    match &t.preview {
        Preview::Trim { removed, .. } => {
            if let (Some(a), Some(b)) = (removed.first(), removed.last())
                && a.distance(*b) > 1e-9
            {
                dot(*a, 3.0, tan());
                dot(*b, 3.0, tan());
            }
        }
        Preview::TrimPoint(p) => dot(*p, 4.0, tan()),
        Preview::Extend { path, .. } => {
            if path.len() > 1 {
                rubber.linestrip(path.iter().map(|p| world(&frame, *p)), Color::srgb_u8(0x44, 0x9c, 0xcd));
                if let Some(e) = path.last() {
                    dot(*e, 2.5, Color::srgb_u8(0x44, 0x9c, 0xcd));
                }
            }
        }
        Preview::Split { at, first, .. } => {
            dot(*at, 3.5, Color::srgb_u8(0xe0, 0x8a, 0x00));
            if let Some(f) = first {
                dot(*f, 3.5, Color::srgb_u8(0xe0, 0x8a, 0x00));
            }
        }
        Preview::None => {}
    }
    if let Some(d) = &t.drag {
        let pts: Vec<Vec3> = d
            .trail
            .iter()
            .filter_map(|p| map.to_sketch(*p))
            .map(|p| world(&frame, p))
            .collect();
        thin.linestrip(pts, Color::srgb_u8(0x8a, 0x8f, 0x94));
    }
}
