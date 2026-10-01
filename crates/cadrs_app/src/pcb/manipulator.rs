//! The **custom part manipulator** (PCB11.6, X10): while the Component pane edits a custom
//! part's placement (in the component view, or on the board with the component selected), the
//! part shows a triad like an assembly instance's (A3): **dragging an arrow** moves the part
//! along that axis of the package frame (the Translate X/Y/Z fields follow, in steps that suit
//! the zoom), **dragging a ring** turns it about its middle (the Rotate fields follow, in 5°
//! steps). Like a typed value it is a preview: **Accept** (✓) puts it into the library (one undo
//! step), **Cancel** goes back.
//!
//! The rings turn about the axes the Rotate fields turn about: the part is rotated about X, then
//! Y, then Z ([`cadrs_core::pcb::PartTransform`]), so changing Z turns it about the package's Z,
//! Y about the Z-turned Y, and X about the Y- and Z-turned X.
//!
//! Names (6 px markers on the handles, for scenarios): `pcb-manip-origin`,
//! `pcb-manip-arrow-x|y|z`, `pcb-manip-ring-x|y|z`.

use std::sync::Arc;

use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_core::Solid;
use cadrs_core::pcb::PartTransform;
use nalgebra::{Matrix3, Point3, Vector3};

use super::{PcbPane, PcbUi, PcbView};
use crate::assembly::ViewportGrab;
use crate::assembly::triad::{TriadGizmos, TriadHaloGizmos};
use crate::viewport::{ActiveKind, ViewportArea, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

pub fn register(app: &mut App) {
    app.init_resource::<PcbManip>().add_systems(
        Update,
        (update_frame, manip_pointer, draw_manip, place_markers)
            .chain()
            .after(super::panes::sync_panes)
            .run_if(in_state(AppState::Document)),
    );
}

// Sizes in screen pixels, as the assembly triad's.
const ARROW: f32 = 66.0;
const HEAD: f32 = 9.0;
const HEAD_W: f32 = 3.5;
const RING_R: f32 = 6.0;
const RING_GAP: f32 = 9.0;
const ORIGIN_R: f32 = 7.0;
const GRAB: f32 = 7.0;
/// Ring drags turn in steps of this many degrees.
const ANGLE_STEP: f64 = 5.0;

/// A handle of the manipulator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManipHandle {
    Arrow(usize),
    Ring(usize),
}

/// Where the manipulator is (world coordinates).
#[derive(Clone, Debug)]
pub struct ManipFrame {
    /// The middle of the part's box, where it is shown now.
    pub origin: Vec3,
    /// The Translate axes (the package frame's x, y, z).
    pub arrows: [Vec3; 3],
    /// The Rotate axes (see the module docs).
    pub rings: [Vec3; 3],
    /// The custom part's points, in its own frame (a turn is about its middle).
    pub points: Arc<Solid>,
}

#[derive(Clone, Debug)]
struct ManipDrag {
    handle: ManipHandle,
    /// Where the press was (screen).
    start: Vec2,
    /// The edit's placement at the press.
    from: PartTransform,
    /// The arrow on screen: pixels per mm along it.
    dir_px: Vec2,
    /// The ring's centre turn: the part's middle on screen.
    centre: Vec2,
    /// +1 when the ring's axis points at the viewer.
    facing: f32,
}

/// The manipulator's state (view state: nothing is saved or undone until Accept).
#[derive(Resource, Default)]
pub struct PcbManip {
    pub frame: Option<ManipFrame>,
    pub hover: Option<ManipHandle>,
    drag: Option<ManipDrag>,
}

impl PcbManip {
    /// True while a handle is dragged.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

fn v3(v: Vector3<f64>) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

fn rot(axis: usize, deg: f64) -> Matrix3<f64> {
    PartTransform { translate: [0.0; 3], rotate: std::array::from_fn(|i| if i == axis { deg } else { 0.0 }) }.motion().linear
}

/// The frame of the custom part the Component pane edits, if it is shown (see the module docs).
fn frame_of(world: &mut World) -> Option<ManipFrame> {
    if *world.resource::<ActiveKind>() != ActiveKind::PcbStudio {
        return None;
    }
    let ui = world.resource::<PcbUi>();
    if ui.pane != PcbPane::Component {
        return None;
    }
    let edit = ui.edit.clone()?;
    let view = ui.view.clone();
    let selected = ui.selected.first().copied();
    let (el, _b, board) = super::shown(world)?;
    if el != edit.element {
        return None;
    }
    // Package frame → world: the component view shows the package in its own frame; on the
    // board, the selected component's placement.
    let to_world = match &view {
        PcbView::Component { package, .. } if *package == edit.package => cadrs_kernel::Motion::default(),
        PcbView::Board => {
            let p = board.component(selected?)?;
            if p.package != edit.package {
                return None;
            }
            cadrs_pcb::placement::placement_motion(p, board.thickness())
        }
        _ => return None,
    };
    let src = world.resource::<ActiveDocument>().doc.element(el)?.pcb()?.library.get(&edit.package).custom()?.source.clone();
    let (solid, _) = super::view::custom_solid(world, &src)?;
    let m = edit.transform.motion();
    let (mut lo, mut hi) = (Vector3::repeat(f64::INFINITY), Vector3::repeat(f64::NEG_INFINITY));
    for p in &solid.positions {
        let q = m.point(&Point3::new(p[0], p[1], p[2])).coords;
        lo = lo.inf(&q);
        hi = hi.sup(&q);
    }
    if !lo.x.is_finite() {
        return None;
    }
    let mid = (lo + hi) / 2.0;
    let origin = v3(to_world.point(&Point3::from(mid)).coords);
    let dir = |v: Vector3<f64>| v3(to_world.linear * v).normalize_or_zero();
    let [_, ry, rz] = edit.transform.rotate;
    let arrows = [dir(Vector3::x()), dir(Vector3::y()), dir(Vector3::z())];
    let rings = [dir(rot(2, rz) * rot(1, ry) * Vector3::x()), dir(rot(2, rz) * Vector3::y()), dir(Vector3::z())];
    Some(ManipFrame { origin, arrows, rings, points: solid })
}

fn update_frame(world: &mut World) {
    let f = frame_of(world);
    let mut m = world.resource_mut::<PcbManip>();
    if f.is_none() {
        m.drag = None;
        m.hover = None;
    }
    m.frame = f;
}

/// The handles on screen.
struct Screen {
    origin: Vec2,
    bases: [Vec2; 3],
    tips: [Vec2; 3],
    rings: [Vec2; 3],
}

fn screen(view: &crate::camera::ViewState, rect: &ViewportRect, f: &ManipFrame) -> Screen {
    let s = view.scale;
    let to = |p: Vec3| rect.to_screen(view.project(p));
    let o = f.origin;
    Screen {
        origin: to(o),
        bases: [0, 1, 2].map(|i| to(o + f.arrows[i] * ORIGIN_R * 1.4 * s)),
        tips: [0, 1, 2].map(|i| to(o + f.arrows[i] * ARROW * s)),
        rings: [0, 1, 2].map(|i| to(ring_centre(f, i, s))),
    }
}

/// Ring `i` sits past the tip of arrow `i` (the axis it turns about may differ, see the module
/// docs).
fn ring_centre(f: &ManipFrame, i: usize, s: f32) -> Vec3 {
    f.origin + f.arrows[i] * (ARROW + RING_GAP + RING_R) * s
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn hit(sc: &Screen, p: Vec2) -> Option<ManipHandle> {
    if let Some(i) = sc.rings.iter().position(|r| r.distance(p) <= RING_R + GRAB * 0.6) {
        return Some(ManipHandle::Ring(i));
    }
    (0..3)
        .map(|i| (i, segment_distance(p, sc.bases[i], sc.tips[i])))
        .filter(|(_, d)| *d <= GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| ManipHandle::Arrow(i))
}

#[allow(clippy::too_many_arguments)]
fn manip_pointer(
    mut inputs: MessageReader<PointerInput>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    vdrag: Res<crate::viewport::ViewportDrag>,
    mut manip: ResMut<PcbManip>,
    mut ui: ResMut<PcbUi>,
    mut grab: ResMut<ViewportGrab>,
) {
    let Some(f) = manip.frame.clone() else {
        inputs.clear();
        return;
    };
    let sc = screen(&view.view, &rect, &f);
    let hover = manip.drag.as_ref().map(|d| d.handle).or_else(|| hit(&sc, vdrag.pointer()));
    if manip.hover != hover {
        manip.hover = hover;
    }
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                let Some(h) = hit(&sc, pos) else { continue };
                let Some(from) = ui.edit.as_ref().map(|e| e.transform) else { continue };
                let (dir_px, facing) = match h {
                    ManipHandle::Arrow(i) => (view.view.project_vector(f.arrows[i]), 1.0),
                    ManipHandle::Ring(i) => (Vec2::ZERO, if f.rings[i].dot(view.view.back()) >= 0.0 { 1.0 } else { -1.0 }),
                };
                if matches!(h, ManipHandle::Arrow(_)) && dir_px.length() < 0.05 {
                    continue;
                }
                manip.drag = Some(ManipDrag { handle: h, start: pos, from, dir_px, centre: sc.origin, facing });
                // Not a click that picks a component.
                grab.0 = true;
            }
            PointerAction::Move { .. } => {
                let Some(d) = manip.drag.as_ref() else { continue };
                let Some(e) = ui.edit.as_mut() else { continue };
                let mut t = d.from;
                match d.handle {
                    ManipHandle::Arrow(i) => {
                        let along = (pos - d.start).dot(d.dir_px.normalize()) / d.dir_px.length();
                        let step = crate::extrude::snap_step(d.dir_px.length());
                        let v = ((d.from.translate[i] + along as f64) / step).round() * step;
                        t.translate[i] = (v * 1e6).round() / 1e6 + 0.0;
                    }
                    ManipHandle::Ring(i) => {
                        let (a, b) = (d.start - d.centre, pos - d.centre);
                        if a.length() < 2.0 || b.length() < 2.0 {
                            continue;
                        }
                        // Screen y points down: a visually counter-clockwise turn has a negative
                        // cross product; counter-clockwise about an axis facing the viewer is a
                        // positive turn.
                        let turn = -a.perp_dot(b).atan2(a.dot(b)) * d.facing;
                        let delta = ((turn.to_degrees() as f64) / ANGLE_STEP).round() * ANGLE_STEP;
                        let mut r = d.from.rotate;
                        r[i] = cadrs_pcb::placement::normalize_deg(r[i] + delta);
                        if r[i] > 180.0 {
                            r[i] -= 360.0;
                        }
                        t = d.from.rotated_in_place(r, &f.points.positions);
                    }
                }
                if e.transform != t {
                    e.transform = t;
                }
            }
            PointerAction::Release(PointerButton::Primary) => {
                manip.drag = None;
            }
            _ => {}
        }
    }
}

const WHITE: Color = Color::srgb(0.98, 0.98, 0.98);
const HALO: Color = Color::srgba(0.12, 0.13, 0.15, 0.85);
const HOVER: Color = Color::srgb(1.0, 0.62, 0.1);
const ACTIVE: Color = Color::srgb(0.27, 0.55, 0.95);

fn draw_manip(manip: Res<PcbManip>, view: Res<ViewportView>, mut halo: Gizmos<TriadHaloGizmos>, mut line: Gizmos<TriadGizmos>) {
    let Some(f) = manip.frame.as_ref() else { return };
    let v = view.view;
    let s = v.scale;
    let back = v.back();
    let o = f.origin;
    let active = manip.drag.as_ref().map(|d| d.handle);
    let color = |h: ManipHandle| {
        if active == Some(h) {
            ACTIVE
        } else if manip.hover == Some(h) && active.is_none() {
            HOVER
        } else {
            WHITE
        }
    };
    let mut seg = |a: Vec3, b: Vec3, c: Color| {
        halo.line(a, b, HALO);
        line.line(a, b, c);
    };
    for i in 0..3 {
        let axis = f.arrows[i];
        let c = color(ManipHandle::Arrow(i));
        let tip = o + axis * ARROW * s;
        let base = tip - axis * HEAD * s;
        seg(o + axis * ORIGIN_R * s, base, c);
        let side = axis.cross(back).normalize_or_zero() * HEAD_W * s;
        seg(base - side, tip, c);
        seg(base + side, tip, c);
        seg(base - side, base + side, c);
        // The ring: a circle about the axis it turns about, past the arrow's tip.
        let rc = color(ManipHandle::Ring(i));
        let centre = ring_centre(f, i, s);
        let (u, w) = f.rings[i].any_orthonormal_pair();
        let pts: Vec<Vec3> = (0..=24)
            .map(|k| {
                let t = k as f32 / 24.0 * std::f32::consts::TAU;
                centre + (u * t.cos() + w * t.sin()) * RING_R * s
            })
            .collect();
        for p in pts.windows(2) {
            seg(p[0], p[1], rc);
        }
        seg(tip, centre - axis * RING_R * 0.2 * s, rc);
    }
    // The origin: a small ring facing the viewer.
    let (u, w) = (v.right(), v.up());
    let ring: Vec<Vec3> = (0..=28)
        .map(|k| {
            let t = k as f32 / 28.0 * std::f32::consts::TAU;
            o + (u * t.cos() + w * t.sin()) * ORIGIN_R * s
        })
        .collect();
    for p in ring.windows(2) {
        halo.line(p[0], p[1], HALO);
        line.line(p[0], p[1], HOVER);
    }
}

/// The handles' markers (see the module docs).
#[derive(Component)]
struct ManipMarker;

fn place_markers(
    manip: Res<PcbManip>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q: Query<(Entity, &Name, &mut Node), With<ManipMarker>>,
    mut commands: Commands,
) {
    let Some(f) = manip.frame.as_ref() else {
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let sc = screen(&view.view, &rect, f);
    let mut spots: Vec<(String, Vec2)> = vec![("pcb-manip-origin".into(), sc.origin)];
    for (i, axis) in ["x", "y", "z"].iter().enumerate() {
        spots.push((format!("pcb-manip-arrow-{axis}"), sc.bases[i].lerp(sc.tips[i], 0.55)));
        spots.push((format!("pcb-manip-ring-{axis}"), sc.rings[i]));
    }
    let size = 6.0;
    let mut have: Vec<String> = Vec::new();
    for (_, name, mut n) in &mut q {
        if let Some((_, p)) = spots.iter().find(|(s, _)| s.as_str() == name.as_str()) {
            let at = *p - rect.0.min - Vec2::splat(size / 2.0);
            n.left = Val::Px(at.x);
            n.top = Val::Px(at.y);
            have.push(name.to_string());
        }
    }
    let Some(area) = q_area.iter().next() else { return };
    for (name, p) in spots {
        if have.contains(&name) {
            continue;
        }
        let at = p - rect.0.min - Vec2::splat(size / 2.0);
        let e = commands
            .spawn((
                Name::new(name),
                ManipMarker,
                Node { position_type: PositionType::Absolute, left: Val::Px(at.x), top: Val::Px(at.y), width: Val::Px(size), height: Val::Px(size), ..default() },
                Pickable::IGNORE,
                DespawnOnExit(AppState::Document),
            ))
            .id();
        commands.entity(area).add_child(e);
    }
}
