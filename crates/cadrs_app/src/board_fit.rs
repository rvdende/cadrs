//! Fitting a board into the parts around it while its layout is edited in a Part Studio
//! (the geometry is [`cadrs_core::pcb::fit`]):
//!
//! - **The parts around** (on by default, the bar's "Parts around"): what of the Part Studio's
//!   other parts lies within the heights the board's components take, outlined in cyan on the
//!   board's face, and the round features standing square to it (a plug's pins, a hole) marked
//!   with their axes.
//! - **Snap** (on by default): a footprint being moved snaps its pads onto those axes (a wire pad
//!   onto a plug's pin) and the sides of its part's shadow onto the edges of the parts around
//!   and of the other footprints' courtyards running beside them (a module up against a wall,
//!   or next to a connector), exactly, off the grid; the snap is marked.
//! - **Room** while moving: the free distance from the part to what is around it along ±x and
//!   ±y of the board, as dimension lines with their values, and up to what is over it ("↑ 3.20
//!   mm"; under it on the bottom side); the part's shadow outlined red, and "In the way", where
//!   it runs into something at its heights.
//! - **Align**: pick an entity of a component (a face, an edge, a pin's axis), then a target in
//!   the Part Studio: the footprint moves in the board's plane so they meet
//!   ([`cadrs_core::pcb::fit::align`]), one undo step.
//! - **Keep-outs**: keep-out areas on the layout (no footprints) where the parts around stand —
//!   on the top side over the board, on the bottom side under it — so the layout keeps clear of
//!   them in the PCB Studio too, where its DRC checks footprints against them.
//!
//! - **Solid parts** (off by default): while the layout is edited the board's components are
//!   see-through so the layout shows under them; on, they are drawn solid. The slider beside it
//!   fades the parts around.
//!
//! Names: `board-fit-solid`, `board-fit-context-opacity`, `board-fit-context`, `board-fit-snap`, `board-fit-align`, `board-fit-keepouts` (the
//! bar's buttons), `board-fit-status` (Align's prompt), `board-gap-{px,nx,py,ny,z}` (the room's
//! values), `board-fit-collision`.

use std::hash::{Hash, Hasher};

use crate::GizmoGroups;
use bevy::prelude::*;
use cadrs_core::pcb::fit::{self, Entity as FitEntity};
use cadrs_core::{PartId, Solid};
use cadrs_eda::poly::Region;
use cadrs_eda::units::Pt;
use cadrs_sketch::PlaneFrame;

use crate::AppState;
use crate::board_in_studio::{LIFT, component_parts};
use crate::parts::PartCache;

pub struct BoardFitPlugin;

impl Plugin for BoardFitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FitState>()
            .gizmo_group::<FitGizmos>()
            .add_systems(Startup, configure_gizmos)
            .add_systems(
                Update,
                (sync_context, snap_move)
                    .chain()
                    .after(crate::board_in_studio::navigate3d)
                    .before(crate::eda::navigate)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                (room, draw_fit, sync_labels)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                sync_ghosts
                    .after(sync_context)
                    .run_if(in_state(AppState::Document)),
            )
            .add_observer(on_fit_button)
            .add_observer(on_context_opacity)
            .add_systems(OnExit(AppState::Document), |mut s: ResMut<FitState>| {
                *s = FitState::default()
            });
    }
}

/// The fit lines: drawn over the board, 2 px.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct FitGizmos;

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (c, _) = store.config_mut::<FitGizmos>();
    c.line.width = 2.0;
    c.depth_bias = -0.5;
}

/// The Align tool's step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AlignStep {
    #[default]
    Off,
    /// Pick the component's entity.
    From,
    /// Pick the target: the component's footprint and its entity.
    To(uuid::Uuid, FitEntity),
}

/// The parts around the edited board copy, worked out once per rebuild and board placement.
#[derive(Clone, Debug, Default)]
pub struct Context {
    key: u64,
    /// The board frame and thickness it was made for.
    pub frame: Option<PlaneFrame>,
    pub thickness: f64,
    /// The heights (board frame) the components take.
    pub slab: (f64, f64),
    /// The parts around within those heights, projected onto the board (layout nm).
    pub around: Region,
    /// Round features standing square to the board: their place (layout mm) and radius.
    pub axes: Vec<([f64; 2], f64)>,
    /// The parts around (for the room at a component's own heights).
    others: Vec<PartId>,
}

/// While a footprint is moved: its part's shadow where it started (layout nm), what is around at
/// its heights, and the layout point the footprint started at.
#[derive(Clone, Debug)]
struct Moving {
    fp: uuid::Uuid,
    shadow: Region,
    blockers: Region,
    /// Where the footprint was when the move started.
    start: cadrs_eda::footprint::FootprintPlacement,
    /// The part's far face from the board (board frame z) and which way that is (up from the
    /// top face, down from the bottom).
    top: f64,
    up: bool,
}

/// The room's reading this frame: per direction (+x, −x, +y, −y) where its line starts and ends
/// (layout mm) and the distance; whether the part is in the way of something.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Room {
    pub gaps: Vec<(usize, [f64; 2], [f64; 2], f64)>,
    /// The room over the part (under it, on the bottom side) to the first part above (mm).
    pub headroom: Option<f64>,
    pub colliding: bool,
    /// The shadow as it is now (layout nm), for the red outline.
    pub shadow: Region,
}

#[derive(Resource, Debug, Clone)]
pub struct FitState {
    pub show: bool,
    pub snap: bool,
    pub align: AlignStep,
    pub context: Context,
    moving: Option<Moving>,
    pub room: Option<Room>,
    /// Where the moved footprint snapped (layout mm), for its mark.
    pub snapped: Option<[f64; 2]>,
    /// The board's components drawn solid (else see-through while the layout is edited, so it
    /// shows under them).
    pub solid: bool,
    /// The parts around's opacity while the layout is edited (the bar's slider).
    pub context_alpha: f32,
}

impl Default for FitState {
    fn default() -> Self {
        FitState {
            show: true,
            snap: true,
            align: AlignStep::Off,
            context: Context::default(),
            moving: None,
            room: None,
            snapped: None,
            solid: false,
            context_alpha: 1.0,
        }
    }
}

/// While a board's layout is edited: its components see-through unless Solid parts is on, the
/// parts around at the slider's opacity.
fn sync_ghosts(
    eda: Res<crate::eda::Eda2d>,
    cache: Res<PartCache>,
    state: Res<FitState>,
    mut ghosts: ResMut<crate::parts::PartGhosts>,
    mut had: Local<bool>,
) {
    let Some(host) = eda.plane().copied() else {
        if *had {
            *had = false;
            *ghosts = crate::parts::PartGhosts::default();
        }
        return;
    };
    *had = true;
    let comps = cache
        .derived
        .get(&host.target.feature)
        .map(|o| component_parts(o, host.target.copy))
        .unwrap_or_default();
    let want = crate::parts::PartGhosts {
        parts: if state.solid { vec![] } else { comps },
        faces: vec![],
        alpha: if state.context_alpha < 1.0 {
            state
                .context
                .others
                .iter()
                .map(|p| (*p, state.context_alpha))
                .collect()
        } else {
            vec![]
        },
    };
    if *ghosts != want {
        *ghosts = want;
    }
}

/// The bar's opacity slider moved.
fn on_context_opacity(
    c: On<cadrs_ui::dialog_fields::SliderChange>,
    q: Query<&Name>,
    mut state: ResMut<FitState>,
) {
    if q.get(c.entity).is_ok_and(|n| n.as_str() == CONTEXT_OPACITY) {
        state.context_alpha = 0.1 + 0.9 * c.value;
    }
}

/// The bar's slider for the parts around's opacity.
pub const CONTEXT_OPACITY: &str = "board-fit-context-opacity";

/// The edited board copy's parts (its board and components).
fn own_parts(cache: &PartCache, host: &crate::eda::PlaneHost) -> Vec<PartId> {
    let Some(out) = cache.derived.get(&host.target.feature) else {
        return vec![];
    };
    let mut v = component_parts(out, host.target.copy);
    if let Some(b) = out.boards.get(host.target.copy) {
        v.push(b.part);
    }
    v
}

/// Works out the parts around the edited board (see [`Context`]) when the parts or the board's
/// place change.
fn sync_context(eda: Res<crate::eda::Eda2d>, cache: Res<PartCache>, mut state: ResMut<FitState>) {
    let Some(host) = eda.plane().copied() else {
        if state.context.frame.is_some() {
            state.context = Context::default();
        }
        return;
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (cache.generation, host.target.feature, host.target.copy).hash(&mut h);
    format!("{:?}", host.frame).hash(&mut h);
    let key = h.finish();
    if state.context.key == key && state.context.frame.is_some() {
        return;
    }
    let own = own_parts(&cache, &host);
    let comps: Vec<&Solid> = own
        .iter()
        .filter_map(|p| cache.part(*p))
        .map(|p| &*p.solid)
        .collect();
    // The heights the components take, and the board's own.
    let (mut z0, mut z1) = (0.0f64, host.thickness);
    for s in &comps {
        if let Some((lo, hi)) = fit::bounds_in(&host.frame, s) {
            z0 = z0.min(lo[2]);
            z1 = z1.max(hi[2]);
        }
    }
    let others: Vec<&cadrs_core::Part> = cache.shown().filter(|p| !own.contains(&p.id)).collect();
    let solids: Vec<&Solid> = others.iter().map(|p| &*p.solid).collect();
    let around = fit::section(&solids, &host.frame, z0, z1, None);
    let axes = fit::axes(&solids, &host.frame, z0, z1);
    state.context = Context {
        key,
        frame: Some(host.frame),
        thickness: host.thickness,
        slab: (z0, z1),
        around,
        axes,
        others: others.iter().map(|p| p.id).collect(),
    };
}

/// The bounds of a region (layout nm).
fn bounds(r: &Region) -> Option<cadrs_eda::units::Bounds> {
    r.iter().flatten().fold(None, |acc, p| {
        Some(cadrs_eda::units::Bounds::union(
            acc,
            cadrs_eda::units::Bounds::of(*p),
        ))
    })
}

/// How near (mm at the board) a pointer must come to snap: 12 px.
fn snap_reach(world: &World) -> f64 {
    let eda = world.resource::<crate::eda::Eda2d>();
    let key = eda.0;
    let scale = key
        .and_then(|k| world.resource::<crate::eda::EdaUi>().views.get(&k))
        .map_or(4.0, |v| v.0.scale);
    12.0 / scale.max(1e-6)
}

/// While a footprint is moved on the board in the view: where it snaps (see the module docs),
/// handed to the Layout's tools as their exact point ([`crate::eda::layout_tools::LayoutState`]).
fn snap_move(world: &mut World) {
    let host = world.resource::<crate::eda::Eda2d>().plane().copied();
    let moving = world
        .resource::<crate::eda::layout_tools::LayoutState>()
        .moving
        .clone();
    let (Some(host), Some(m)) = (host, moving) else {
        let mut st = world.resource_mut::<FitState>();
        st.moving = None;
        st.snapped = None;
        world
            .resource_mut::<crate::eda::layout_tools::LayoutState>()
            .exact = None;
        return;
    };
    let Some(fp) = crate::derived_ui::derived_board_ref(world, host.target.feature)
        .and_then(|b| b.layout.footprints.iter().find(|f| f.id == m.fp).cloned())
    else {
        return;
    };
    // The part's shadow and what is around at its heights, once per move.
    if world
        .resource::<FitState>()
        .moving
        .as_ref()
        .is_none_or(|x| x.fp != m.fp)
    {
        let cache = world.resource::<PartCache>();
        let part = cache
            .derived
            .get(&host.target.feature)
            .and_then(|o| {
                o.footprints
                    .iter()
                    .find(|(p, f)| *f == m.fp && component_parts(o, host.target.copy).contains(p))
            })
            .and_then(|(p, _)| cache.part(*p));
        let up = fp.placement.side == cadrs_eda::layer::Side::Top;
        let (shadow, blockers, top) = match part {
            Some(p) => {
                let shadow = fit::footprint_of(&p.solid, &host.frame);
                let (lo, hi) = fit::bounds_in(&host.frame, &p.solid)
                    .map_or((0.0, 0.0), |(lo, hi)| (lo[2], hi[2]));
                let others: Vec<&Solid> = world
                    .resource::<FitState>()
                    .context
                    .others
                    .iter()
                    .filter_map(|id| cache.part(*id))
                    .map(|p| &*p.solid)
                    .collect();
                // Inside its heights: a part only touching it isn't in the way.
                (
                    shadow,
                    fit::section(&others, &host.frame, lo + 1e-3, hi - 1e-3, None),
                    if up { hi } else { lo },
                )
            }
            None => (Region::new(), Region::new(), 0.0),
        };
        world.resource_mut::<FitState>().moving = Some(Moving {
            fp: m.fp,
            shadow,
            blockers,
            start: fp.placement,
            top,
            up,
        });
    }
    if !world.resource::<FitState>().snap {
        world.resource_mut::<FitState>().snapped = None;
        world
            .resource_mut::<crate::eda::layout_tools::LayoutState>()
            .exact = None;
        return;
    }
    let at = world.resource::<crate::eda::EdaPointer>().at;
    let reach = snap_reach(world);
    let st = world.resource::<FitState>().clone();
    let Some(mv) = st.moving.clone() else { return };
    let mm = cadrs_eda::units::to_mm;
    // How far the footprint has moved, unsnapped (nm).
    let raw = at - m.from;
    // 1. A pad onto an axis (the nearest within reach).
    let mut best: Option<(f64, [f64; 2], [f64; 2])> = None;
    for pad in &fp.footprint.pads {
        let c = fp.placement.apply(pad.at) + raw;
        let c = [mm(c.x), mm(c.y)];
        for (a, _) in &st.context.axes {
            let d = (a[0] - c[0]).hypot(a[1] - c[1]);
            if d < reach && best.is_none_or(|b| d < b.0) {
                best = Some((d, [a[0] - c[0], a[1] - c[1]], *a));
            }
        }
    }
    let (fix, mark) = match best {
        Some((_, fix, mark)) => (Some(fix), Some(mark)),
        // 2. The shadow's sides onto the parts' edges running along them (within reach, and
        // beside the shadow, not anywhere along the line), each way on its own.
        None => match bounds(&mv.shadow) {
            Some(bb) => {
                let (x0, x1, y0, y1) = (
                    mm(bb.min.x + raw.x),
                    mm(bb.max.x + raw.x),
                    mm(bb.min.y + raw.y),
                    mm(bb.max.y + raw.y),
                );
                // The parts around, and the other footprints' courtyards on its side.
                let mut edges = st.context.around.clone();
                if let Some(b) = crate::derived_ui::derived_board_ref(world, host.target.feature) {
                    for f in b
                        .layout
                        .footprints
                        .iter()
                        .filter(|f| f.id != m.fp && f.placement.side == fp.placement.side)
                    {
                        edges.extend(cadrs_eda::drc::courtyard(f));
                    }
                }
                let (fx, fy) = edge_snap(&edges, [x0, y0, x1, y1], reach);
                (
                    (fx.is_some() || fy.is_some()).then(|| [fx.unwrap_or(0.0), fy.unwrap_or(0.0)]),
                    None,
                )
            }
            None => (None, None),
        },
    };
    let exact = fix.map(|f| {
        Pt::new(
            at.x + cadrs_eda::units::mm(f[0]),
            at.y + cadrs_eda::units::mm(f[1]),
        )
    });
    world.resource_mut::<FitState>().snapped = mark;
    world
        .resource_mut::<crate::eda::layout_tools::LayoutState>()
        .exact = exact;
}

/// How far to move a box (layout mm: x0, y0, x1, y1) so one of its sides lies on an edge of
/// `around` running along it (within 2°) within `reach`, and overlapping the box's span the
/// other way (give or take `reach`): (along x, along y), each when there is one.
fn edge_snap(around: &Region, b: [f64; 4], reach: f64) -> (Option<f64>, Option<f64>) {
    let mm = cadrs_eda::units::to_mm;
    let [x0, y0, x1, y1] = b;
    let (mut fx, mut fy): (Option<f64>, Option<f64>) = (None, None);
    let take = |f: &mut Option<f64>, d: f64| {
        if d.abs() < reach && f.is_none_or(|g| d.abs() < g.abs()) {
            *f = Some(d);
        }
    };
    let slope = 2.0_f64.to_radians().tan();
    for ring in around {
        for i in 0..ring.len() {
            let (a, c) = (ring[i], ring[(i + 1) % ring.len()]);
            let (ax, ay, cx, cy) = (mm(a.x), mm(a.y), mm(c.x), mm(c.y));
            let (dx, dy) = (cx - ax, cy - ay);
            // Along y (a wall's side seen from above): its x against the box's left and right.
            if dx.abs() <= dy.abs() * slope
                && dy.abs() > 1e-6
                && ay.max(cy) > y0 - reach
                && ay.min(cy) < y1 + reach
            {
                let x = (ax + cx) / 2.0;
                take(&mut fx, x - x0);
                take(&mut fx, x - x1);
            }
            if dy.abs() <= dx.abs() * slope
                && dx.abs() > 1e-6
                && ax.max(cx) > x0 - reach
                && ax.min(cx) < x1 + reach
            {
                let y = (ay + cy) / 2.0;
                take(&mut fy, y - y0);
                take(&mut fy, y - y1);
            }
        }
    }
    (fx, fy)
}

/// The room around the moved part this frame (see [`Room`]).
fn room(
    eda: Res<crate::eda::Eda2d>,
    preview: Res<crate::eda::Preview>,
    cache: Res<PartCache>,
    mut state: ResMut<FitState>,
    mut last: Local<Option<(uuid::Uuid, cadrs_eda::footprint::FootprintPlacement, u64)>>,
) {
    let (Some(_), Some(mv)) = (eda.plane(), state.moving.clone()) else {
        if state.room.is_some() {
            state.room = None;
        }
        *last = None;
        return;
    };
    // Where the preview has the footprint now: worked out again only when that (or the parts)
    // changed, not every frame of a drag.
    let Some(now) = preview
        .get()
        .and_then(|d| d.board.footprints.iter().find(|f| f.id == mv.fp))
        .map(|f| f.placement)
        .or(Some(mv.start))
    else {
        return;
    };
    let key = (mv.fp, now, cache.generation);
    if last.as_ref() == Some(&key) && state.room.is_some() {
        return;
    }
    *last = Some(key);
    // Flipped to the other side: what is around there differs; no readings until it's dropped.
    if now.side != mv.start.side {
        state.room = None;
        return;
    }
    // The shadow turned about the footprint's start and moved with it.
    let turn = (now.angle - mv.start.angle).to_radians();
    let (c, s_) = (turn.cos(), turn.sin());
    let (o, to) = (mv.start.at, now.at);
    let place = |p: Pt| {
        let (x, y) = ((p.x - o.x) as f64, (p.y - o.y) as f64);
        Pt::new(
            to.x + (c * x - s_ * y).round() as i64,
            to.y + (s_ * x + c * y).round() as i64,
        )
    };
    let shadow: Region = mv
        .shadow
        .iter()
        .map(|r| r.iter().map(|p| place(*p)).collect())
        .collect();
    let mut room = Room {
        colliding: cadrs_eda::poly::overlaps(&shadow, &mv.blockers),
        ..Default::default()
    };
    if let Some(frame) = state.context.frame {
        let others: Vec<&Solid> = state
            .context
            .others
            .iter()
            .filter_map(|id| cache.part(*id))
            .map(|p| &*p.solid)
            .collect();
        room.headroom = fit::headroom(&shadow, &others, &frame, mv.top, mv.up, 200.0);
    }
    if let Some(bb) = bounds(&shadow) {
        let g = fit::gaps(&shadow, &mv.blockers, 200.0);
        let mm = cadrs_eda::units::to_mm;
        let (cx, cy) = (mm((bb.min.x + bb.max.x) / 2), mm((bb.min.y + bb.max.y) / 2));
        let starts = [
            [mm(bb.max.x), cy],
            [mm(bb.min.x), cy],
            [cx, mm(bb.max.y)],
            [cx, mm(bb.min.y)],
        ];
        let dirs = [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]];
        let colliding = room.colliding;
        for (i, gap) in g.iter().enumerate().filter(|_| !colliding) {
            if let Some(v) = gap.filter(|v| *v > 1e-3) {
                let s = starts[i];
                room.gaps
                    .push((i, s, [s[0] + dirs[i][0] * v, s[1] + dirs[i][1] * v], v));
            }
        }
    }
    room.shadow = shadow;
    if state.room.as_ref() != Some(&room) {
        state.room = Some(room);
    }
}

/// A layout point (mm) on the board's top face, a little over the layout, in the Part Studio.
fn world_at(c: &Context, x: f64, y: f64) -> Vec3 {
    let f = c.frame.unwrap_or(PlaneFrame {
        origin: [0.0; 3],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 1.0, 0.0],
    });
    let p = cadrs_core::pcb::frames::to_world(&f, [x, y, c.thickness + 2.0 * LIFT]);
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

const AROUND: Color = Color::srgb(0.10, 0.80, 0.95);
const ROOM: Color = Color::srgb(1.0, 0.62, 0.10);
const IN_THE_WAY: Color = Color::srgb(0.95, 0.15, 0.15);

/// Draws the parts around, the axes, the snap mark, the room and the collision.
fn draw_fit(eda: Res<crate::eda::Eda2d>, state: Res<FitState>, mut g: Gizmos<FitGizmos>) {
    if eda.plane().is_none() || state.context.frame.is_none() {
        return;
    }
    let c = &state.context;
    let mm = cadrs_eda::units::to_mm;
    if state.show {
        for ring in &c.around {
            let pts: Vec<Vec3> = ring
                .iter()
                .chain(ring.first())
                .map(|p| world_at(c, mm(p.x), mm(p.y)))
                .collect();
            g.linestrip(pts, AROUND);
        }
        for (a, r) in &c.axes {
            let n = 24;
            let pts: Vec<Vec3> = (0..=n)
                .map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / n as f64;
                    world_at(c, a[0] + r * t.cos(), a[1] + r * t.sin())
                })
                .collect();
            g.linestrip(pts, AROUND);
            g.line(
                world_at(c, a[0] - r * 1.6, a[1]),
                world_at(c, a[0] + r * 1.6, a[1]),
                AROUND,
            );
            g.line(
                world_at(c, a[0], a[1] - r * 1.6),
                world_at(c, a[0], a[1] + r * 1.6),
                AROUND,
            );
        }
    }
    if let Some(m) = state.snapped {
        for k in 0..4 {
            let t = std::f64::consts::FRAC_PI_2 * k as f64 + std::f64::consts::FRAC_PI_4;
            g.line(
                world_at(c, m[0], m[1]),
                world_at(c, m[0] + 1.2 * t.cos(), m[1] + 1.2 * t.sin()),
                ROOM,
            );
        }
    }
    if let Some(room) = &state.room {
        for (_, a, b, _) in &room.gaps {
            g.line(world_at(c, a[0], a[1]), world_at(c, b[0], b[1]), ROOM);
            // End ticks across the line.
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = dx.hypot(dy).max(1e-9);
            let (nx, ny) = (-dy / l * 0.6, dx / l * 0.6);
            for p in [a, b] {
                g.line(
                    world_at(c, p[0] - nx, p[1] - ny),
                    world_at(c, p[0] + nx, p[1] + ny),
                    ROOM,
                );
            }
        }
        if room.colliding {
            for ring in &room.shadow {
                let pts: Vec<Vec3> = ring
                    .iter()
                    .chain(ring.first())
                    .map(|p| world_at(c, mm(p.x), mm(p.y)))
                    .collect();
                g.linestrip(pts, IN_THE_WAY);
            }
        }
    }
}

/// A value label of the room (or the collision's), placed over the view.
#[derive(Component)]
struct FitLabel(usize);

/// Keeps a label at the middle of each room line ("4.20 mm") and "In the way" at the part.
fn sync_labels(world: &mut World, q: &mut QueryState<(Entity, &FitLabel)>) {
    let state = world.resource::<FitState>().clone();
    let view = world.resource::<crate::viewport::ViewportView>().view;
    let rect = *world.resource::<crate::viewport::ViewportRect>();
    // (index, text, screen position, colour)
    let mut want: Vec<(usize, String, Vec2, Color)> = vec![];
    if let (Some(room), Some(_)) = (&state.room, state.context.frame) {
        for (i, a, b, v) in &room.gaps {
            let mid = world_at(&state.context, (a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
            want.push((
                *i,
                format!("{v:.2} mm"),
                rect.to_screen(view.project(mid)),
                ROOM,
            ));
        }
        let mm = cadrs_eda::units::to_mm;
        if room.colliding
            && let Some(bb) = bounds(&room.shadow)
        {
            let p = world_at(&state.context, mm((bb.min.x + bb.max.x) / 2), mm(bb.max.y));
            want.push((
                4,
                "In the way".into(),
                rect.to_screen(view.project(p)),
                IN_THE_WAY,
            ));
        }
        // The room over it (under it on the bottom side), at its middle.
        if let (Some(h), Some(bb)) = (room.headroom, bounds(&room.shadow)) {
            let up = state.moving.as_ref().is_none_or(|m| m.up);
            let p = world_at(
                &state.context,
                mm((bb.min.x + bb.max.x) / 2),
                mm((bb.min.y + bb.max.y) / 2),
            );
            want.push((
                5,
                format!("{} {h:.2} mm", if up { "↑" } else { "↓" }),
                rect.to_screen(view.project(p)),
                ROOM,
            ));
        }
    }
    let names = [
        "board-gap-px",
        "board-gap-nx",
        "board-gap-py",
        "board-gap-ny",
        "board-fit-collision",
        "board-gap-z",
    ];
    let have: Vec<(Entity, usize)> = q.iter(world).map(|(e, l)| (e, l.0)).collect();
    for (e, i) in &have {
        if !want.iter().any(|w| w.0 == *i) {
            world.entity_mut(*e).despawn();
        }
    }
    let theme = world.resource::<cadrs_ui::Theme>().clone();
    for (i, text, at, color) in want {
        let left = Val::Px(at.x + 6.0);
        let top = Val::Px(at.y - 8.0);
        match have.iter().find(|h| h.1 == i) {
            Some((e, _)) => {
                if let Some(mut n) = world.get_mut::<Node>(*e) {
                    n.left = left;
                    n.top = top;
                }
                let child = world.get::<Children>(*e).and_then(|c| c.first().copied());
                if let Some(c) = child
                    && let Some(mut t) = world.get_mut::<Text>(c)
                    && t.0 != text
                {
                    t.0 = text;
                }
            }
            None => {
                world.spawn((
                    Name::new(names[i]),
                    FitLabel(i),
                    Node {
                        position_type: PositionType::Absolute,
                        left,
                        top,
                        padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.9)),
                    GlobalZIndex(20),
                    Pickable::IGNORE,
                    DespawnOnExit(AppState::Document),
                    children![(
                        theme.text(text, theme.font_sm, bevy::text::FontWeight::BOLD, color),
                        Pickable::IGNORE
                    )],
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The bar's buttons, Align and Keep-outs

/// The edit bar's fit buttons (see the module docs), as (name, label, tooltip).
pub const BUTTONS: [(&str, &str, &str); 5] = [
    (
        "board-fit-solid",
        "Solid parts",
        "Show the board's components solid (while the layout is edited they are see-through, so it shows under them)",
    ),
    (
        "board-fit-context",
        "Parts around",
        "Show the parts around the board's components, and the pins and holes square to it",
    ),
    (
        "board-fit-snap",
        "Snap",
        "Snap a moved footprint's pads onto pins and its part's edges onto the parts around",
    ),
    (
        "board-fit-align",
        "Align",
        "Move a footprint so an entity of its part meets a target: pick the part's face, edge or pin, then the target",
    ),
    (
        "board-fit-keepouts",
        "Keep-outs",
        "Keep-out areas on the layout where the parts around stand",
    ),
];

/// Whether a fit button shows as on.
pub fn is_on(state: &FitState, name: &str) -> bool {
    match name {
        "board-fit-solid" => state.solid,
        "board-fit-context" => state.show,
        "board-fit-snap" => state.snap,
        "board-fit-align" => state.align != AlignStep::Off,
        _ => false,
    }
}

fn on_fit_button(a: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(a.entity) else { return };
    let name = n.as_str().to_string();
    match name.as_str() {
        "board-fit-solid" => commands.queue(|w: &mut World| {
            let mut s = w.resource_mut::<FitState>();
            s.solid = !s.solid;
        }),
        "board-fit-context" => commands.queue(|w: &mut World| {
            let mut s = w.resource_mut::<FitState>();
            s.show = !s.show;
        }),
        "board-fit-snap" => commands.queue(|w: &mut World| {
            let mut s = w.resource_mut::<FitState>();
            s.snap = !s.snap;
        }),
        "board-fit-align" => commands.queue(|w: &mut World| {
            let mut s = w.resource_mut::<FitState>();
            s.align = if s.align == AlignStep::Off {
                AlignStep::From
            } else {
                AlignStep::Off
            };
        }),
        "board-fit-keepouts" => commands.queue(keepouts_from_parts),
        _ => {}
    }
}

/// What Align says it waits for.
pub fn align_prompt(state: &FitState) -> Option<&'static str> {
    match state.align {
        AlignStep::Off => None,
        AlignStep::From => Some("Align: pick a face, edge or pin of a component"),
        AlignStep::To(..) => {
            Some("Align: pick where it goes (a face, edge, pin, hole or mate connector)")
        }
    }
}

/// The entity a pick is, for Align: a planar face's plane, a round face's axis, a circular
/// edge's axis (its centre, square to its plane), another edge's middle, a vertex.
fn entity_of(part: &cadrs_core::Part, pick: &crate::viewport::Pick) -> Option<FitEntity> {
    use crate::viewport::Pick;
    match pick {
        Pick::Face(_, f) => {
            let face = part.solid.face(f)?;
            if let Some(p) = face.plane {
                return Some(FitEntity::Plane(p.origin, p.normal()));
            }
            face.axis
                .map(|(p, d)| FitEntity::Axis(p, d))
                .or(face.center.map(FitEntity::Point))
        }
        Pick::Edge(_, e) => {
            let edge = part.solid.edge(e)?;
            match edge.circle {
                Some(c) => Some(FitEntity::Axis(c.center, c.normal)),
                // A mesh part's rim: the axis of the round face it bounds.
                None => e
                    .faces
                    .iter()
                    .find_map(|f| part.solid.face(f)?.axis)
                    .map(|(p, d)| FitEntity::Axis(p, d))
                    .or(Some(FitEntity::Point(edge.midpoint()))),
            }
        }
        Pick::Vertex(_, v) => part.solid.vertex(v).map(|v| FitEntity::Point(v.point)),
        _ => None,
    }
}

/// A press in the view while Align waits for a pick: the first on a component of the edited
/// board, the second anywhere; then the footprint moves (one undo step). Returns whether Align
/// took the press.
pub fn align_press(world: &mut World, offset: Vec2) -> bool {
    let step = world.resource::<FitState>().align;
    if step == AlignStep::Off {
        return false;
    }
    let Some(host) = world.resource::<crate::eda::Eda2d>().plane().copied() else {
        return false;
    };
    let view = world.resource::<crate::viewport::ViewportView>().view;
    // The target can be an axis mark of the parts around (a pin under the board).
    if let AlignStep::To(fp, from) = step {
        let c = world.resource::<FitState>().context.clone();
        let mark = c.axes.iter().find(|(a, r)| {
            let at = view.project(world_at(&c, a[0], a[1]));
            at.distance(offset) < 12.0_f32.max((*r as f32) / view.scale.max(1e-6))
        });
        if let (Some((a, _)), Some(frame)) = (mark, c.frame) {
            let p = cadrs_core::pcb::frames::to_world(&frame, [a[0], a[1], 0.0]);
            finish_align(world, host, fp, from, FitEntity::Axis(p, frame.normal()));
            return true;
        }
    }
    // (The target can also be a mate connector: its Z axis or the plane it is normal to.)
    let to = matches!(step, AlignStep::To(..));
    let filter = crate::parts::PickFilter {
        faces: true,
        edges: true,
        connectors: to,
        ..crate::parts::PickFilter::none()
    };
    let cache = world.resource::<PartCache>();
    let Some(pick) = crate::parts::pick_scene(cache, &view, offset, filter) else {
        return true;
    };
    if let (crate::viewport::Pick::Feature(id), AlignStep::To(fp, from)) = (&pick, step)
        && let Some(c) = cache.connectors.get(id).copied()
    {
        finish_align(
            world,
            host,
            fp,
            from,
            fit::connector_entity(&c, &host.frame),
        );
        return true;
    }
    let part_id = match pick {
        crate::viewport::Pick::Face(p, _)
        | crate::viewport::Pick::Edge(p, _)
        | crate::viewport::Pick::Vertex(p, _) => p,
        _ => return true,
    };
    let Some(part) = cache.part(part_id).cloned() else {
        return true;
    };
    let Some(entity) = entity_of(&part, &pick) else {
        return true;
    };
    match step {
        AlignStep::From => {
            let out = cache.derived.get(&host.target.feature);
            let comps = out
                .map(|o| component_parts(o, host.target.copy))
                .unwrap_or_default();
            let fp = out.and_then(|o| {
                o.footprints
                    .iter()
                    .find(|(p, _)| *p == part_id && comps.contains(p))
                    .map(|(_, f)| *f)
            });
            match fp {
                Some(fp) => world.resource_mut::<FitState>().align = AlignStep::To(fp, entity),
                None => crate::eda::ui::toast(
                    world,
                    "Pick a face, edge or pin of one of the board's components first",
                ),
            }
        }
        AlignStep::To(fp, from) => finish_align(world, host, fp, from, entity),
        AlignStep::Off => {}
    }
    true
}

/// Moves (and turns) the footprint `fp` so `from` (on its part) meets `to` (one undo step), and
/// ends Align; then says what is still off: the distance out of the board's plane, and the
/// courtyards or keep-outs it now overlaps.
fn finish_align(
    world: &mut World,
    host: crate::eda::PlaneHost,
    fp: uuid::Uuid,
    from: FitEntity,
    to: FitEntity,
) {
    world.resource_mut::<FitState>().align = AlignStep::Off;
    let Some(pivot) = crate::derived_ui::derived_board_ref(world, host.target.feature)
        .and_then(|b| b.layout.footprints.iter().find(|f| f.id == fp))
        .map(|f| cadrs_core::pcb::frames::layout_to_board(f.placement.at))
    else {
        return;
    };
    let Some(a) = fit::align(from, to, &host.frame, pivot) else {
        crate::eda::ui::toast(
            world,
            "They can't meet by moving the footprint on the board: pick a pin or hole square to the board, or faces square to it",
        );
        return;
    };
    let reference = footprint_reference(world, host.target.feature, fp);
    let label = format!("Align {reference}");
    crate::eda::ui::commit(world, &label, |des| {
        let i = cadrs_eda::board_edit::footprint_by_id(&des.board, fp)
            .ok_or("The footprint is gone")?;
        if a.turn != 0.0 {
            cadrs_eda::board_edit::rotate_footprint(&mut des.board, i, a.turn);
        }
        let to = des.board.footprints[i].placement.at + fit::delta(a.shift);
        cadrs_eda::board_edit::move_footprint(&mut des.board, i, to);
        Ok(())
    });
    // What is still off.
    let mut notes: Vec<String> = vec![];
    if let Some(z) = a.off_plane.filter(|z| z.abs() > 1e-3) {
        notes.push(format!(
            "{reference} is {:.2} mm {} it, square to the board",
            z.abs(),
            if z > 0.0 { "under" } else { "over" }
        ));
    }
    if let Some(b) = crate::derived_ui::derived_board_ref(world, host.target.feature) {
        use cadrs_eda::drc::Rule;
        let report = cadrs_eda::drc::check(&b.layout);
        // Its items: "J5" (a courtyard) or "PTH pad 1 [/NEUTRAL] of J5" (copper).
        let of = |x: &str| x == reference || x.ends_with(&format!(" of {reference}"));
        let mut copper: Vec<String> = vec![];
        for v in report
            .violations
            .iter()
            .filter(|v| v.items.iter().any(|x| of(x)))
        {
            let others: Vec<String> = v
                .items
                .iter()
                .filter(|x| !of(x))
                .map(|x| x.rsplit(" of ").next().unwrap_or(x).to_string())
                .collect();
            match v.rule {
                Rule::CourtyardOverlap => notes.push(format!(
                    "{reference}'s courtyard overlaps {}",
                    others.join(", ")
                )),
                Rule::KeepoutViolation => notes.push(format!("{reference}: {}", v.message)),
                Rule::Clearance | Rule::HoleClearance | Rule::HoleToHole => copper.extend(others),
                _ => {}
            }
        }
        copper.sort();
        copper.dedup();
        if !copper.is_empty() {
            notes.push(format!(
                "{reference}'s pads are too close to {}",
                copper.join(", ")
            ));
        }
    }
    if !notes.is_empty() {
        notes.dedup();
        crate::eda::ui::toast(world, &format!("Aligned. {}", notes.join("; ")));
    }
}

fn footprint_reference(world: &World, feature: cadrs_core::FeatureId, fp: uuid::Uuid) -> String {
    crate::derived_ui::derived_board_ref(world, feature)
        .and_then(|b| {
            b.layout
                .footprints
                .iter()
                .find(|f| f.id == fp)
                .map(|f| f.reference().to_string())
        })
        .unwrap_or_else(|| "footprint".into())
}

/// The name of the keep-outs [`keepouts_from_parts`] makes (it replaces its own).
pub const KEEPOUT_NAME: &str = "Parts around";

/// `r`'s area as rings without holes: a keep-out's outline can't have holes, so a ring-shaped
/// part (a tube's wall around the board) is cut through each hole into pieces.
fn hole_free(r: &Region) -> Vec<Vec<Pt>> {
    let mut out = vec![];
    let mut todo = cadrs_eda::zone::to_polygons(r);
    let mut guard = 0;
    while let Some(p) = todo.pop() {
        guard += 1;
        if p.holes.is_empty() || guard > 256 {
            out.push(p.outer);
            continue;
        }
        let Some(hb) = bounds(&vec![p.holes[0].clone()]) else {
            continue;
        };
        let x = (hb.min.x + hb.max.x) / 2;
        let big = 1_000_000_000_000i64;
        let whole: Region = std::iter::once(p.outer.clone())
            .chain(p.holes.iter().cloned())
            .collect();
        for (a, b) in [(-big, x), (x, big)] {
            let half = vec![vec![
                Pt::new(a, -big),
                Pt::new(b, -big),
                Pt::new(b, big),
                Pt::new(a, big),
            ]];
            todo.extend(cadrs_eda::zone::to_polygons(
                &cadrs_eda::poly::intersection(&whole, &half),
            ));
        }
    }
    out.retain(|r| cadrs_eda::poly::ring_area(r).abs() > 1e12);
    out
}

/// Keep-outs (no footprints) on the layout where the parts around stand within the
/// components' heights: on the top side where they are above the board, on the bottom side where
/// they are below it, each named with the room left under it ("Parts around: 3.20 mm clear").
/// Doing it again replaces those it made (one undo step).
pub fn keepouts_from_parts(world: &mut World) {
    use cadrs_eda::layer::{Layer, LayerSet};
    let Some(host) = world.resource::<crate::eda::Eda2d>().plane().copied() else {
        return;
    };
    let c = world.resource::<FitState>().context.clone();
    let Some(layout) =
        crate::derived_ui::derived_board_ref(world, host.target.feature).map(|b| b.layout.clone())
    else {
        return;
    };
    let cache = world.resource::<PartCache>();
    let others: Vec<&Solid> = c
        .others
        .iter()
        .filter_map(|id| cache.part(*id))
        .map(|p| &*p.solid)
        .collect();
    let board = cadrs_eda::outline::board_region(&layout);
    let eps = 1e-3;
    let sides = [
        (
            fit::section(
                &others,
                &host.frame,
                host.thickness + eps,
                c.slab.1.max(host.thickness + eps),
                None,
            ),
            Layer::TopCopper,
            host.thickness,
            true,
        ),
        (
            fit::section(&others, &host.frame, c.slab.0.min(-eps), -eps, None),
            Layer::BottomCopper,
            0.0,
            false,
        ),
    ];
    // (ring, layer, room between the board's face and the part there)
    let mut areas: Vec<(Vec<Pt>, Layer, Option<f64>)> = vec![];
    for (region, layer, face, up) in sides {
        let inside = cadrs_eda::poly::intersection(&region, &board);
        for r in hole_free(&inside) {
            let room = fit::headroom(&vec![r.clone()], &others, &host.frame, face, up, 1e3);
            areas.push((r, layer, room));
        }
    }
    let had = layout
        .zones
        .iter()
        .filter(|z| z.keepout.is_some() && z.name.starts_with(KEEPOUT_NAME))
        .count();
    if areas.is_empty() && had == 0 {
        crate::eda::ui::toast(world, "Nothing around stands over or under the board");
        return;
    }
    let (top, bottom) = (
        areas.iter().filter(|a| a.1 == Layer::TopCopper).count(),
        areas.iter().filter(|a| a.1 == Layer::BottomCopper).count(),
    );
    let rules = cadrs_eda::board::Keepout {
        footprints: true,
        ..Default::default()
    };
    // (Each with the room there: footprints low enough to fit under the part are allowed.)
    let n = areas.len();
    crate::eda::ui::commit(
        world,
        &format!("Keep-outs from the parts around ({n})"),
        |d| {
            d.board
                .zones
                .retain(|z| !(z.keepout.is_some() && z.name.starts_with(KEEPOUT_NAME)));
            for (r, layer, room) in &areas {
                let rules = cadrs_eda::board::Keepout {
                    max_height: room.map(cadrs_eda::units::mm),
                    ..rules
                };
                let id = cadrs_eda::zone::add_keepout(
                    &mut d.board,
                    LayerSet::of(&[*layer]),
                    r.clone(),
                    rules,
                );
                if let Some(z) = d.board.zones.iter_mut().find(|z| z.id == id) {
                    z.name = match room {
                        Some(h) => format!("{KEEPOUT_NAME}: {h:.2} mm clear"),
                        None => KEEPOUT_NAME.to_string(),
                    };
                }
            }
            Ok(())
        },
    );
    let what = match (top, bottom) {
        (0, 0) => "the keep-outs from the parts around removed: nothing stands over or under the board now".to_string(),
        (t, 0) => format!("{t} keep-out{} on the top side", if t == 1 { "" } else { "s" }),
        (0, b) => format!("{b} keep-out{} on the bottom side (seen from below: Shift+N)", if b == 1 { "" } else { "s" }),
        (t, b) => format!("{t} keep-out{} on the top side, {b} on the bottom", if t == 1 { "" } else { "s" }),
    };
    let replaced = if had > 0 && n > 0 {
        format!(" (replacing {had})")
    } else {
        String::new()
    };
    crate::eda::ui::toast(world, &format!("Parts around: {what}{replaced}"));
}
