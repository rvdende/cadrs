//! Booleans (union, subtract, intersect) of B-rep bodies.
//!
//! 1. Both bodies go into one pool of vertices and edges (coincident vertices and edges shared).
//! 2. Vertices lying on edges of the other body, edge–edge and edge–face intersections split
//!    edges; every such point becomes a vertex.
//! 3. Each pair of faces whose boxes overlap is intersected; the intersection curves are cut at
//!    the vertices lying on them and the pieces inside both faces become cut edges of both.
//!    Faces on the same surface imprint each other's boundary edges instead.
//! 4. Every face is split along its cuts in its (u, v) domain.
//! 5. Each piece is classified against the other body (inside, outside, on with the same or the
//!    opposite orientation) and kept or dropped by the operation's rule; Subtract flips the
//!    tool's kept pieces.
//! 6. The kept faces are assembled into shells and solids; a union merges faces of the two
//!    inputs that lie on the same surface and meet along an edge.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use rayon::prelude::*;

use super::classify::{self, InFace};
use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::pcurve;
use super::split::{self, Arc};
use super::ssi;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, Edge, Face, Shell, Solid};
use crate::*;

/// Accumulated time per phase of an operation (CADRS_PROFILE=1 prints it after each one).
pub(crate) struct Prof {
    t: web_time::Instant,
    on: bool,
    phases: Vec<(&'static str, std::time::Duration)>,
}

/// Whether `CADRS_PROFILE` is set (read once).
pub(crate) fn profiling() -> bool {
    super::env_flag!("CADRS_PROFILE")
}

impl Prof {
    pub(crate) fn new() -> Self {
        Self {
            t: web_time::Instant::now(),
            on: profiling(),
            phases: Vec::new(),
        }
    }
    pub(crate) fn mark(&mut self, name: &'static str) {
        if self.on {
            let now = web_time::Instant::now();
            self.phases.push((name, now - self.t));
            self.t = now;
        }
    }
    pub(crate) fn report(&self, what: &str) {
        if self.on {
            let total: std::time::Duration = self.phases.iter().map(|p| p.1).sum();
            let parts: Vec<String> = self
                .phases
                .iter()
                .map(|(n, d)| format!("{n} {:.1}ms", d.as_secs_f64() * 1e3))
                .collect();
            eprintln!(
                "[profile] {what}: {:.1}ms = {}",
                total.as_secs_f64() * 1e3,
                parts.join(", ")
            );
        }
    }
}

/// Boolean runs and face pairs sent to surface intersection since the start (with
/// [`classify::RAYS`], what `CADRS_PROFILE` reports per boolean).
static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PAIRS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The counts of [`RUNS`], [`PAIRS`], [`classify::RAYS`], [`classify::POINTS`],
/// [`pcurve::FITTED`] and [`pcurve::FITTED_NS`] now.
fn counts() -> [u64; 6] {
    use std::sync::atomic::Ordering::Relaxed;
    [
        RUNS.load(Relaxed),
        PAIRS.load(Relaxed),
        classify::RAYS.load(Relaxed),
        classify::POINTS.load(Relaxed),
        pcurve::FITTED.load(Relaxed),
        pcurve::FITTED_NS.load(Relaxed),
    ]
}

/// The default distance under which two points of a boolean are one (mm; [`tol::POINT`]; a
/// run's is [`BoolOptions::tol`]'s).
pub const VTOL: f64 = super::tol::POINT;

/// How a boolean runs: its tolerances, and which faces on one surface are merged afterwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoolOptions {
    /// The merge distance of the first run and of the retries after one that fails.
    pub tol: super::tol::Tolerances,
    /// Faces of the two inputs on one surface are merged whatever the operation (a blend's
    /// tool face flush with a face it ends against becomes part of it); otherwise only a
    /// union merges them.
    pub flush: bool,
    /// Faces on one surface are merged after the run (true by default); OCCT's booleans (the
    /// DRAW harness's) keep them apart.
    pub merge_coplanar: bool,
    /// The most work a run may do before it fails ([`KernelError::BudgetExhausted`]): the
    /// surface intersections' cell pairs and traced points, each split face's arcs squared,
    /// and the classified and assembled faces (counts of the inputs, so the same run always
    /// spends the same). [`DEFAULT_WORK_BUDGET`] by default; a caller may pass less.
    pub work_budget: u64,
    /// OCCT's fuzzy value: entities of the two inputs within this distance of each other are
    /// one (faces on surfaces this far apart coincide). None: the larger of the tolerances
    /// the inputs' edges and vertices carry (a gap closed when they were made), at most
    /// [`FUZZY_MAX`] of their size.
    pub fuzzy: Option<f64>,
    /// A run that leaves the result broken is tried again (fuzzy, a solid at a time, at
    /// growing tolerances: the ladder); off, the first run's answer stands (a question that
    /// needs no more, as whether a New's body touches a part).
    pub retry: bool,
}

/// The largest default fuzzy value, as a part of the inputs' size.
pub const FUZZY_MAX: f64 = 1e-5;

/// The default work budget of a boolean run ([`BoolOptions::work_budget`]): about three times
/// the most a passing case of the test suites spends (7.1 million units, an OCCT boolean case);
/// the runaway runs it stops spend 30 million and more over minutes.
pub const DEFAULT_WORK_BUDGET: u64 = 20_000_000;

impl Default for BoolOptions {
    fn default() -> Self {
        BoolOptions {
            tol: super::tol::Tolerances::default(),
            flush: false,
            merge_coplanar: true,
            work_budget: DEFAULT_WORK_BUDGET,
            fuzzy: None,
            retry: true,
        }
    }
}

/// What one run of the boolean takes from its [`BoolOptions`]: the merge distance (the first
/// run's or a retry's) and the rest as given.
#[derive(Debug, Clone, Copy)]
struct RunOpts {
    tol: f64,
    flush: bool,
    merge_coplanar: bool,
    work_budget: u64,
    /// Whether a result whose new faces cross (each other, or their own loops in (u, v)) is
    /// refused ([`CROSSING`]).
    check_crossings: bool,
}

/// The stage of a run's refusal of a result whose new faces cross.
const CROSSING: &str = "crossing";

thread_local! {
    /// How many booleans taking an input one solid at a time this thread is within.
    static PER_SOLID: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The result a run on this thread just refused for crossing faces (and nothing else),
    /// for [`ladder`] to fall back on.
    static REFUSED: std::cell::RefCell<Option<BoolOut>> = const { std::cell::RefCell::new(None) };
    /// Booleans on this thread that handed back a result with crossing faces (no run of the
    /// ladder gave one without): see [`take_invalid_results`].
    static INVALID_RESULTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many booleans on this thread handed back a result with crossing faces since the last
/// call (a run refused it and no retry did better, so it came back as it was). With
/// `CADRS_BOOL_STRICT` set, such a boolean fails instead.
pub fn take_invalid_results() -> usize {
    INVALID_RESULTS.with(|c| c.replace(0))
}

/// A run's work so far against its budget ([`BoolOptions::work_budget`]).
struct Work {
    done: u64,
    budget: u64,
}

impl Work {
    /// Counts `w` more; fails once the budget is spent.
    fn spend(&mut self, w: u64, stage: &'static str) -> Result<()> {
        self.done = self.done.saturating_add(w);
        if self.done > self.budget {
            return Err(KernelError::BudgetExhausted {
                stage,
                why: format!(
                    "the boolean ran out of its work budget ({} units) in its {stage} stage",
                    self.budget
                ),
            });
        }
        Ok(())
    }
}

impl RunOpts {
    /// Section and imprint runs: a boolean's defaults.
    const PLAIN: RunOpts = RunOpts {
        tol: VTOL,
        flush: false,
        merge_coplanar: true,
        work_budget: DEFAULT_WORK_BUDGET,
        check_crossings: false,
    };
}

#[derive(Debug, Clone)]
struct WEdge {
    curve: Curve,
    t: [f64; 2],
    v: [usize; 2],
    degenerate: bool,
    /// The edge's tolerance (the input's, or the run's for new edges).
    tol: f64,
    /// Split points (t, vertex).
    splits: Vec<(f64, usize)>,
    /// A traced intersection curve's p-curves on its two surfaces ([`ssi::IntCurve::pcs`]).
    pcs: Option<std::sync::Arc<[Option<Curve>; 2]>>,
}

#[derive(Debug, Clone)]
struct WFace {
    surf: Surface,
    sense: bool,
    loops: Vec<Vec<(usize, bool, Curve)>>,
    owner: usize,
    orig: usize,
    /// Its polygons and their boxes and rows, to cull them by ([`classify::poly_boxes`]).
    polys: classify::KeptPolygons,
    bbox: Bbox,
    uv_center: P2,
    /// The face's own tolerance (0: exact, the run's applies).
    tol: f64,
}

struct Pool {
    verts: Vec<P3>,
    /// Each vertex's tolerance: points within the larger of two tolerances (and the run's) are
    /// one vertex.
    vtols: Vec<f64>,
    /// The grid's cell: big enough that a merge never reaches past the neighbouring cells.
    cell: f64,
    /// The run's merge distance.
    tol: f64,
    grid: HashMap<(i64, i64, i64), Vec<usize>>,
    edges: Vec<WEdge>,
}

impl Pool {
    /// A pool for a run merging within `tol`, of entities of tolerances up to `max_tol`.
    fn with_tolerance(tol: f64, max_tol: f64) -> Self {
        Pool {
            verts: Vec::new(),
            vtols: Vec::new(),
            cell: (tol * 16.0).max(max_tol * 2.0),
            tol,
            grid: HashMap::default(),
            edges: Vec::new(),
        }
    }
    fn cell_of(&self, p: &P3) -> (i64, i64, i64) {
        let s = 1.0 / self.cell;
        (
            (p.x * s).floor() as i64,
            (p.y * s).floor() as i64,
            (p.z * s).floor() as i64,
        )
    }
    /// The vertex `p` (of tolerance `tol`) falls on.
    fn find_tol(&self, p: &P3, tol: f64) -> Option<usize> {
        let (cx, cy, cz) = self.cell_of(p);
        let mut best: Option<(f64, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(v) = self.grid.get(&(cx + dx, cy + dy, cz + dz)) {
                        for &i in v {
                            let d = (self.verts[i] - p).norm();
                            if d < self.tol.max(tol).max(self.vtols[i])
                                && best.is_none_or(|(bd, _)| d < bd)
                            {
                                best = Some((d, i));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(_, i)| i)
    }
    /// The vertex at `p` with tolerance `tol`: an existing one within reach (its tolerance grown
    /// to hold this one's ball), else a new one.
    fn vertex_tol(&mut self, p: P3, tol: f64) -> usize {
        if let Some(i) = self.find_tol(&p, tol) {
            if tol > 0.0 {
                let reach = (self.verts[i] - p).norm() + tol;
                if reach > self.vtols[i] {
                    self.vtols[i] = reach;
                }
            }
            return i;
        }
        if super::env_flag!("CADRS_VERTEX_DEBUG") {
            let near_round = |x: f64| {
                let r = (x * 2.0).round() / 2.0;
                (x - r).abs() > 1e-11 && (x - r).abs() < 1e-5
            };
            if near_round(p.x) || near_round(p.y) || near_round(p.z) {
                eprintln!(
                    "inexact vertex {p:?}\n{}",
                    std::backtrace::Backtrace::force_capture()
                );
            }
        }
        self.new_vertex_tol(p, tol)
    }
    fn new_vertex_tol(&mut self, p: P3, tol: f64) -> usize {
        self.verts.push(p);
        self.vtols.push(tol);
        let i = self.verts.len() - 1;
        let c = self.cell_of(&p);
        self.grid.entry(c).or_default().push(i);
        i
    }
    fn split(&mut self, e: usize, t: f64, v: usize) {
        let ed = &mut self.edges[e];
        if ed.degenerate {
            return;
        }
        let span = (ed.t[1] - ed.t[0]).abs();
        if t <= ed.t[0] + span * tol::REL_FINE || t >= ed.t[1] - span * tol::REL_FINE {
            return;
        }
        if v == ed.v[0] || v == ed.v[1] {
            // A closed edge can be split at its own vertex only at its ends.
            if !(ed.v[0] == ed.v[1]) {
                return;
            }
            return;
        }
        if ed.splits.iter().any(|(_, sv)| *sv == v) {
            return;
        }
        ed.splits.push((t, v));
    }
}

fn edge_bbox(e: &WEdge, verts: &[P3], tol: f64) -> Bbox {
    let mut bb = Bbox::empty();
    bb.add(&verts[e.v[0]]);
    bb.add(&verts[e.v[1]]);
    if !e.degenerate {
        bb.union(&curve_bbox(&e.curve, e.t[0], e.t[1]));
    }
    bb.grow(tol * 10.0 + e.tol)
}

/// A box that holds the curve between t0 and t1: exact for lines, circles and ellipses (their
/// extremes in each axis), the poles' box for B-splines (a rational B-spline with positive
/// weights lies in its poles' convex hull).
pub fn curve_bbox(c: &Curve, t0: f64, t1: f64) -> Bbox {
    let (lo, hi) = (t0.min(t1), t0.max(t1));
    let mut bb = Bbox::empty();
    bb.add(&c.point(lo));
    bb.add(&c.point(hi));
    match c {
        Curve::Line { .. } => {}
        Curve::Circle { c: o, x, y, r }
        | Curve::Ellipse {
            c: o, x, y, a: r, ..
        } => {
            let (rx, ry) = match c {
                Curve::Ellipse { a, b, .. } => (*a, *b),
                _ => (*r, *r),
            };
            let _ = o;
            // Coordinate k is o_k + rx·x_k·cos t + ry·y_k·sin t: extreme where tan t = ry·y_k / (rx·x_k).
            for k in 0..3 {
                let t = (ry * y[k]).atan2(rx * x[k]);
                for base in [t, t + std::f64::consts::PI] {
                    // Every copy of the extreme inside [lo, hi].
                    let mut tt =
                        base + ((lo - base) / std::f64::consts::TAU).ceil() * std::f64::consts::TAU;
                    while tt <= hi {
                        bb.add(&c.point(tt));
                        tt += std::f64::consts::TAU;
                    }
                }
            }
        }
        Curve::Nurbs(_) => {
            let n = c.to_nurbs(lo, hi);
            if n.cps.iter().all(|h| h.w > 0.0) {
                for h in &n.cps {
                    bb.add(&super::nurbs::dehom(h));
                }
            } else {
                for i in 0..=64 {
                    bb.add(&c.point(lo + (hi - lo) * i as f64 / 64.0));
                }
                bb = bb.grow(bb.diagonal() * 0.02);
            }
        }
    }
    bb
}

/// Imports a body's vertices, edges and faces into the pool.
fn import(pool: &mut Pool, faces: &mut Vec<WFace>, b: &Body, owner: usize, merge: bool) {
    // Exact inputs carry no tolerance of their own (the run's applies).
    let own = |t: f64| if t > super::topo::TOL_MIN { t } else { 0.0 };
    // (Without `merge`, only the body's own vertices at one point exactly are one: a body
    // touching itself along an edge, two blocks' corners there as two vertices.)
    let mut same: HashMap<[u64; 3], usize> = HashMap::default();
    let vmap: Vec<usize> = b
        .verts
        .iter()
        .map(|v| {
            if merge {
                return pool.vertex_tol(v.p, own(v.tol));
            }
            *same
                .entry([
                    (v.p.x + 0.0).to_bits(),
                    (v.p.y + 0.0).to_bits(),
                    (v.p.z + 0.0).to_bits(),
                ])
                .or_insert_with(|| pool.new_vertex_tol(v.p, own(v.tol)))
        })
        .collect();
    let e0 = pool.edges.len();
    for e in &b.edges {
        pool.edges.push(WEdge {
            curve: e.curve.clone(),
            t: e.t,
            v: [vmap[e.v[0]], vmap[e.v[1]]],
            degenerate: e.degenerate,
            tol: own(e.tol),
            splits: Vec::new(),
            pcs: None,
        });
    }
    for (fi, f) in b.faces.iter().enumerate() {
        let mut loops: Vec<Vec<(usize, bool, Curve)>> = f
            .loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|c| (c.edge + e0, c.fwd, c.pc.clone()))
                    .collect()
            })
            .collect();
        loops.iter_mut().for_each(drop_slits);
        let polys = classify::face_polygons_kept(b, f);
        let mut bb = Bbox::empty();
        let mut uvb = Bbox::empty();
        for p in &polys.0 {
            for q in p {
                uvb.add(&P3::new(q.x, q.y, 0.0));
            }
        }
        for l in &f.loops {
            for c in l {
                let e = &b.edges[c.edge];
                bb.add(&b.verts[e.v[0]].p);
                // (A conic's own box: samples along an arc miss its extremes by up to
                // r·(1 − cos(π/16)), and a pair of faces then misses a crossing near there.)
                if !e.degenerate
                    && matches!(
                        e.curve,
                        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. }
                    )
                {
                    bb.union(&curve_bbox(&e.curve, e.t[0], e.t[1]));
                } else if !e.degenerate {
                    for i in 0..=16 {
                        bb.add(&e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 16.0));
                    }
                }
            }
        }
        // Curved faces may bulge past their edges: a box sure to hold the face.
        if !matches!(f.surf, Surface::Plane { .. }) {
            bb.union(&b.face_bbox(fi));
        }
        let center = uvb.center();
        // Faces don't carry a tolerance of their own yet (OCCT's are usually exact even where
        // their edges' are not): the run's applies, and the edges' and vertices' where they are
        // involved.
        let ftol = 0.0;
        faces.push(WFace {
            surf: f.surf.clone(),
            sense: f.sense,
            loops,
            owner,
            orig: fi,
            polys,
            bbox: bb.grow(pool.tol * 10.0 + ftol),
            uv_center: P2::new(center.x, center.y),
            tol: ftol,
        });
    }
}

/// Drops from a loop each run along an edge and straight back along it (two uses of one edge
/// in a row, opposite ways: a slit, which bounds nothing; a merge of a seamed face with one it
/// borders can leave one).
fn drop_slits(l: &mut Vec<(usize, bool, Curve)>) {
    while l.len() > 2 {
        let n = l.len();
        let Some(k) = (0..n).find(|&k| l[k].0 == l[(k + 1) % n].0 && l[k].1 != l[(k + 1) % n].1)
        else {
            break;
        };
        let k2 = (k + 1) % n;
        l.remove(k.max(k2));
        l.remove(k.min(k2));
    }
}

fn on_curve(c: &Curve, t0: f64, t1: f64, p: &P3, tol: f64) -> Option<f64> {
    let t = match c {
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            let t = c.closest(p, t0, t1);
            // The closest point for a periodic curve: retry unwrapped.
            t
        }
        _ => c.closest(p, t0, t1),
    };
    if (c.point(t) - p).norm() < tol {
        Some(t)
    } else {
        None
    }
}

/// The parameter of a point on a closed curve, unwrapped into [t0, t1].
fn param_on(c: &Curve, t0: f64, t1: f64, p: &P3) -> f64 {
    c.closest(p, t0, t1)
}

/// The parameter of the point of the curve nearest `p`, starting from `t` (exact for lines and
/// circles; Newton otherwise).
fn refine_param(c: &Curve, t0: f64, t1: f64, p: &P3, t: f64) -> f64 {
    match c {
        Curve::Line { .. } | Curve::Circle { .. } => {
            let tc = c.closest(p, t0 - (t1 - t0), t1 + (t1 - t0));
            if (tc - t).abs() < 1e-4 * (1.0 + t.abs()) {
                tc.clamp(t0, t1)
            } else {
                t
            }
        }
        _ => {
            let mut x = t;
            for _ in 0..20 {
                let (q, d1, d2) = c.d2(x);
                let r = q - p;
                let f = r.dot(&d1);
                let df = d1.norm_squared() + r.dot(&d2);
                if df.abs() < tol::TINY {
                    break;
                }
                let nx = (x - f / df).clamp(t0, t1);
                if (nx - x).abs() < 1e-16 * (1.0 + x.abs()) {
                    x = nx;
                    break;
                }
                x = nx;
            }
            x
        }
    }
}

/// Where a surface lies, to tell a point too far from it to be on it without projecting the
/// point onto it: a B-spline's control points' box, or an extruded B-spline curve's control
/// points' box seen along the extrusion (the weights all positive).
enum SurfaceBound {
    Box(Bbox),
    /// The box in `across`'s x and y (its z along the extrusion).
    Across {
        across: Frame,
        min: (f64, f64),
        max: (f64, f64),
    },
}

impl SurfaceBound {
    fn of(s: &Surface) -> Option<Self> {
        let points = |cps: &mut dyn Iterator<Item = &super::nurbs::H>| -> Option<Vec<P3>> {
            cps.map(|h| (h.w > 0.0).then(|| super::nurbs::dehom(h)))
                .collect()
        };
        match s {
            Surface::Nurbs(n) => {
                let mut b = Bbox::empty();
                for p in points(&mut n.cps.iter().flatten())? {
                    b.add(&p);
                }
                Some(Self::Box(b))
            }
            Surface::Extrusion {
                c: Curve::Nurbs(c),
                d,
            } if d.norm() > 0.0 => {
                let across = Frame::from_z(P3::origin(), d.normalize());
                let (mut min, mut max) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
                for p in points(&mut c.cps.iter())? {
                    let l = across.local(&p);
                    min = (min.0.min(l.x), min.1.min(l.y));
                    max = (max.0.max(l.x), max.1.max(l.y));
                }
                Some(Self::Across { across, min, max })
            }
            _ => None,
        }
    }

    /// Whether `p` is farther than `tol` from the surface.
    fn far(&self, p: &P3, tol: f64) -> bool {
        match self {
            Self::Box(b) => !b.grow(tol).contains(p),
            Self::Across { across, min, max } => {
                let l = across.local(p);
                l.x < min.0 - tol || l.x > max.0 + tol || l.y < min.1 - tol || l.y > max.1 + tol
            }
        }
    }
}

/// [`edge_on_surface_tol`], told at once when the edge's start is too far from the surface
/// (`bound`, its [`SurfaceBound`]) to be on it: no projection onto a B-spline per edge and face
/// (seconds in all in a union of faces with many edges and extruded B-spline faces).
fn edge_on_bounded(
    e: &WEdge,
    s: &Surface,
    bound: Option<&SurfaceBound>,
    ftol: f64,
    tol: f64,
) -> bool {
    let far = |b: &SurfaceBound| b.far(&e.curve.point(e.t[0]), tol.max(e.tol).max(ftol));
    if !e.degenerate && bound.is_some_and(far) {
        return false;
    }
    edge_on_surface_tol(e, s, ftol, tol)
}

/// Two faces sharing an edge (the same vertices and curve) whose surfaces are tangent along
/// it, and which aren't both analytic (analytic pairs are intersected exactly). `bounds`: the
/// faces' surfaces' [`SurfaceBound`]s; `ebox`: the edges' boxes.
fn tangent_along_shared_edge(
    pool: &Pool,
    fa: &WFace,
    fb: &WFace,
    ea: &[usize],
    eb: &[usize],
    bounds: (Option<&SurfaceBound>, Option<&SurfaceBound>),
    ebox: &[Bbox],
) -> bool {
    let analytic = |s: &Surface| {
        !matches!(
            s,
            Surface::Nurbs(_) | Surface::Extrusion { .. } | Surface::Revolution { .. }
        )
    };
    if analytic(&fa.surf) && analytic(&fb.surf) {
        return false;
    }
    // A boundary edge of one face lying on the other's surface with the two tangent along it
    // (a blend's contact curve on the face it blends into).
    // (Also a boundary edge lying on the other surface where the face leaves it: a blend
    // tool's strip meeting the face it blends along its contact curve. The edge is imprinted
    // exactly; contouring would only find it again, inexactly.)
    // (Tangent there: an edge of a face lying on a surface the face crosses, as a tube's top
    // line on a wall through its axis, is no contact; the face–face stage drops the curve the
    // intersection finds along such an edge again.)
    let tangent_at = |f: &WFace, g: &WFace, p: &P3| {
        let ((fu, fv), (gu, gv)) = (f.surf.project(p, None), g.surf.project(p, None));
        f.surf.normal(fu, fv).cross(&g.surf.normal(gu, gv)).norm() < 1e-3
    };
    let tangent_on = |edges: &[usize], f: &WFace, g: &WFace, bound: Option<&SurfaceBound>| {
        edges.iter().any(|&x| {
            let e = &pool.edges[x];
            !e.degenerate
                && (e.t[1] - e.t[0]).abs() > 0.0
                && edge_on_bounded(e, &g.surf, bound, 0.0, pool.tol)
                && e.curve.longer_than(e.t[0], e.t[1], 1e-3)
                && [0.25, 0.5, 0.75]
                    .iter()
                    .all(|k| tangent_at(f, g, &e.curve.point(e.t[0] + (e.t[1] - e.t[0]) * k)))
        })
    };
    if tangent_on(eb, fb, fa, bounds.0) || tangent_on(ea, fa, fb, bounds.1) {
        return true;
    }
    for &x in ea {
        let e = &pool.edges[x];
        if e.degenerate {
            continue;
        }
        for &y in eb {
            let o = &pool.edges[y];
            // (Edges whose boxes keep apart aren't the same curve: no points of one sought on the
            // other, for each of two faces' hundreds of edge pairs.)
            if o.degenerate || x != y && !ebox[x].grow(pool.tol * 10.0).overlaps(&ebox[y]) {
                continue;
            }
            // The same curve, at least where they overlap (whole circles may have their seams in
            // different places).
            let same_ends = x == y || {
                let on = |a: &WEdge, b: &WEdge| {
                    [0.25, 0.5, 0.75].iter().all(|f| {
                        let m = b.curve.point(b.t[0] + (b.t[1] - b.t[0]) * f);
                        on_curve(&a.curve, a.t[0], a.t[1], &m, pool.tol * 10.0).is_some()
                    })
                };
                on(e, o) && on(o, e)
            };
            if super::env_flag!("CADRS_BOOL_DEBUG") && same_ends {
                let p = e.curve.point(e.t[0] + (e.t[1] - e.t[0]) * 0.5);
                let (ua, va) = fa.surf.project(&p, Some((fa.uv_center.x, fa.uv_center.y)));
                let (ub, vb) = fb.surf.project(&p, Some((fb.uv_center.x, fb.uv_center.y)));
                eprintln!(
                    "shared edge {x}/{y}: normals dot {:.12}, proj err {:.3e} {:.3e}",
                    fa.surf.normal(ua, va).dot(&fb.surf.normal(ub, vb)),
                    (fa.surf.eval(ua, va) - p).norm(),
                    (fb.surf.eval(ub, vb) - p).norm()
                );
            }
            if !same_ends {
                continue;
            }
            // Tangent along it: the normals agree (up to sign) at a few points.
            let tangent = [0.2, 0.5, 0.8].iter().all(|f| {
                let p = e.curve.point(e.t[0] + (e.t[1] - e.t[0]) * f);
                let (ua, va) = fa.surf.project(&p, None);
                let (ub, vb) = fb.surf.project(&p, None);
                fa.surf.normal(ua, va).dot(&fb.surf.normal(ub, vb)).abs() > 1.0 - tol::ANGULAR_POINT
            });
            if tangent {
                return true;
            }
        }
    }
    false
}

/// Whether an edge lies on a surface (its sample points are on it).
/// (Within the run's `tol` and the edge's own tolerance.)
fn edge_on_surface(e: &WEdge, s: &Surface, tol: f64) -> bool {
    edge_on_surface_tol(e, s, 0.0, tol)
}

/// [`edge_on_surface`] within also `ftol` (the face's).
fn edge_on_surface_tol(e: &WEdge, s: &Surface, ftol: f64, tol: f64) -> bool {
    if e.degenerate {
        return true;
    }
    let tol = tol.max(e.tol).max(ftol);
    [0.0, 0.3, 0.5, 0.7, 1.0].iter().all(|f| {
        let p = e.curve.point(e.t[0] + (e.t[1] - e.t[0]) * f);
        s.distance(&p) < tol
    })
}

/// In-face test of a model point (projected onto the face's surface).
fn point_in_face(f: &WFace, p: &P3, tol: f64) -> InFace {
    let (mut u, mut v) = f.surf.project(p, Some((f.uv_center.x, f.uv_center.y)));
    // A projection from the face's middle can stop short on a B-spline or an extrusion (a
    // local minimum, the end of the range): from the whole surface then.
    if (f.surf.eval(u, v) - p).norm() > tol * 10.0 + tol::ROUNDOFF
        && matches!(f.surf, Surface::Nurbs(_) | Surface::Extrusion { .. })
    {
        let (u2, v2) = f.surf.project(p, None);
        if (f.surf.eval(u2, v2) - p).norm() < (f.surf.eval(u, v) - p).norm() {
            (u, v) = (u2, v2);
        }
    }
    if (f.surf.eval(u, v) - p).norm() > tol * 10.0 + tol::ROUNDOFF {
        return InFace::Outside;
    }
    let mut res = InFace::Outside;
    // (The polygons' range and boxes kept from the start: a face of hundreds of holes holds a
    // million polygon points, which every test went over.)
    let (polys, boxes) = &*f.polys;
    for uv in classify::periodic_candidates_boxed(&f.surf, u, v, boxes) {
        match classify::in_face_boxed(polys, boxes, &f.surf, uv, tol) {
            InFace::Inside => return InFace::Inside,
            InFace::Boundary => res = InFace::Boundary,
            InFace::Outside => {}
        }
    }
    res
}

/// A point inside a face's domain (uv) given its loops as arcs.
fn interior_point(loops: &[Vec<Arc>]) -> Option<P2> {
    interior_of_polys(&arc_polys(loops))
}

fn interior_points(loops: &[Vec<Arc>]) -> Vec<P2> {
    interior_points_of_polys(&arc_polys(loops))
}

/// A face's loops as polygons in (u, v): a straight p-curve by its start, another by 64 points.
fn arc_polys(loops: &[Vec<Arc>]) -> Vec<Vec<P2>> {
    let mut polys: Vec<Vec<P2>> = Vec::new();
    for l in loops {
        let mut pts = Vec::new();
        for a in l {
            let n = match a.pc {
                Curve::Line { .. } => 1,
                _ => 64,
            };
            for i in 0..n {
                pts.push(a.uv_at(i as f64 / n as f64));
            }
        }
        polys.push(pts);
    }
    polys
}

/// Where [`interior_points_of_polys`] looks for points inside polygons: on a few scanlines,
/// each midway across an inside interval; how far from the boundary each is, scaled to the box.
struct Scan<'a> {
    polys: &'a [Vec<P2>],
    vb: (f64, f64),
    su: f64,
    sv: f64,
}

impl<'a> Scan<'a> {
    fn new(polys: &'a [Vec<P2>]) -> Option<Self> {
        let mut vb = (f64::INFINITY, f64::NEG_INFINITY);
        let mut ub = (f64::INFINITY, f64::NEG_INFINITY);
        for p in polys {
            for q in p {
                vb = (vb.0.min(q.y), vb.1.max(q.y));
                ub = (ub.0.min(q.x), ub.1.max(q.x));
            }
        }
        if !vb.0.is_finite() || vb.1 - vb.0 < 1e-14 {
            return None;
        }
        let (su, sv) = (
            1.0 / (ub.1 - ub.0).max(tol::TINY),
            1.0 / (vb.1 - vb.0).max(tol::TINY),
        );
        Some(Scan { polys, vb, su, sv })
    }

    /// The candidates, scanline by scanline.
    fn candidates(&self) -> Vec<P2> {
        let mut out = Vec::new();
        for frac in [
            0.5123, 0.3137, 0.6863, 0.1571, 0.8429, 0.0713, 0.9287, 0.4231, 0.5869, 0.2377, 0.7623,
        ] {
            let y = self.vb.0 + (self.vb.1 - self.vb.0) * frac;
            let mut xs: Vec<f64> = Vec::new();
            for p in self.polys {
                let n = p.len();
                for i in 0..n {
                    let (a, b) = (p[i], p[(i + 1) % n]);
                    if (a.y > y) != (b.y > y) {
                        xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            let mut k = 0;
            while k + 1 < xs.len() {
                out.push(P2::new((xs[k] + xs[k + 1]) * 0.5, y));
                k += 2;
            }
        }
        out
    }

    /// The squared distance from `q` to the boundary, scaled to the box; once it is down to
    /// `floor`, no further (a candidate that can't beat the best so far).
    fn clearance(&self, q: &P2, floor: f64) -> f64 {
        let (su, sv) = (self.su, self.sv);
        let qs = P2::new(q.x * su, q.y * sv);
        let mut d = f64::INFINITY;
        for p in self.polys {
            let n = p.len();
            for i in 0..n {
                let (a, b) = (p[i], p[(i + 1) % n]);
                d = d.min(seg_dist2(
                    &qs,
                    &P2::new(a.x * su, a.y * sv),
                    &P2::new(b.x * su, b.y * sv),
                ));
                if d <= floor {
                    return d;
                }
            }
        }
        d
    }
}

/// Points inside the polygons (on a few scanlines, each midway across an inside interval),
/// farthest from the boundary first.
pub fn interior_points_of_polys(polys: &[Vec<P2>]) -> Vec<P2> {
    let Some(scan) = Scan::new(polys) else {
        return Vec::new();
    };
    let mut cands: Vec<(f64, P2)> = scan
        .candidates()
        .into_iter()
        .map(|q| (scan.clearance(&q, f64::NEG_INFINITY), q))
        .collect();
    cands.sort_by(|a, b| b.0.total_cmp(&a.0));
    cands.into_iter().map(|(_, p)| p).collect()
}

/// Whether a face (its loops as arcs) has an inside: [`interior_point`] finds a point in it.
fn has_interior(loops: &[Vec<Arc>]) -> bool {
    let polys = arc_polys(loops);
    Scan::new(&polys).is_some_and(|s| !s.candidates().is_empty())
}

/// The first of [`interior_points_of_polys`]: the candidate farthest from the boundary (the
/// earliest of equals), each measured only as far as it could still be.
pub fn interior_of_polys(polys: &[Vec<P2>]) -> Option<P2> {
    let scan = Scan::new(polys)?;
    let mut best: Option<(f64, P2)> = None;
    for q in scan.candidates() {
        let floor = best.map_or(f64::NEG_INFINITY, |(d, _)| d);
        let d = scan.clearance(&q, floor);
        if best.is_none() || d > floor {
            best = Some((d, q));
        }
    }
    best.map(|(_, q)| q)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    In,
    Out,
    OnSame,
    OnOpp,
}

pub struct BoolOut {
    pub body: Body,
    /// For each output face: (owner 0 = target / 1 = tool, its face in that input).
    pub from: Vec<Vec<(usize, usize)>>,
}

/// The boolean of two bodies, with the default [`BoolOptions`].
pub fn boolean_bodies(a: &Body, b: &Body, op: BoolOp) -> Result<BoolOut> {
    boolean_bodies_with(a, b, op, BoolOptions::default())
}

/// The boolean of two bodies with explicit options.
pub fn boolean_bodies_with(a: &Body, b: &Body, op: BoolOp, opts: BoolOptions) -> Result<BoolOut> {
    if !profiling() {
        return boolean_bodies_counted(a, b, op, opts);
    }
    let c0 = counts();
    let r = boolean_bodies_counted(a, b, op, opts);
    let c = counts();
    eprintln!(
        "[profile] boolean {op:?}: {} runs, {} face pairs intersected, {} rays for {} points classified, {} p-curves fitted in {:.1}ms",
        c[0] - c0[0],
        c[1] - c0[1],
        c[2] - c0[2],
        c[3] - c0[3],
        c[4] - c0[4],
        (c[5] - c0[5]) as f64 * 1e-6
    );
    r
}

/// [`boolean_bodies_with`] (counted with `CADRS_PROFILE`).
fn boolean_bodies_counted(a: &Body, b: &Body, op: BoolOp, opts: BoolOptions) -> Result<BoolOut> {
    ladder(a, b, op, opts)
}

/// The runs of a boolean: at the default tolerance, then the ways round a broken result. A
/// result with crossing faces is refused ([`RunOpts::check_crossings`]): the first run's is
/// handed back flagged ([`hand_back`]); a later run's is kept, should no run after it do
/// better.
fn ladder(a: &Body, b: &Body, op: BoolOp, opts: BoolOptions) -> Result<BoolOut> {
    // The first result refused for crossing faces, made over as an accepted one would be.
    let mut refused: Option<BoolOut> = None;
    // A run's result: `Ok` when sound, else the refused one (with `clean` false) or the error.
    let attempt = |r: Result<BoolOut>| -> std::result::Result<(BoolOut, bool), KernelError> {
        match r {
            Ok(r) => Ok((r, true)),
            Err(KernelError::BooleanFailed { stage, why }) if stage == CROSSING => {
                match REFUSED.with(|c| c.borrow_mut().take()) {
                    Some(r) => Ok((r, false)),
                    None => Err(KernelError::BooleanFailed { stage, why }),
                }
            }
            Err(e) => Err(e),
        }
    };
    // At the default tolerance first; when that leaves the result broken (near-coincident
    // geometry the tolerance splits into slivers or gaps: a surface through a corner within
    // 1e-5), again at growing tolerances, as OCCT grows its entities' tolerances.
    // Faces of the two bodies on one surface but for float noise (made apart: radii 25 and
    // 25.0000015) get the gap as their edges' and vertices' tolerance, so they meet.
    let fuzzy = opts.fuzzy.unwrap_or_else(|| carried_fuzzy(a, b));
    let raised = near_coincident_tolerances(a, b, opts.tol.point, fuzzy);
    let (a, b) = match &raised {
        Some((x, y)) => (x, y),
        None => (a, b),
    };
    let at = |tol: f64| RunOpts {
        tol,
        flush: opts.flush,
        merge_coplanar: opts.merge_coplanar,
        work_budget: opts.work_budget,
        check_crossings: true,
    };
    let first = match attempt(run(a, b, Mode::Bool(op), at(opts.tol.point))) {
        Ok((r, true)) => Ok(r),
        // Refused only for crossing faces: no retry has been seen to do better (on the
        // documents, the soak), and the whole ladder costs as much again: handed back now.
        Ok((r, false)) => return hand_back(r),
        Err(e) => Err(e),
    };
    // (Retried: an open shell, a face that doesn't close, an invalid result: `BooleanFailed`, and
    // the failures of the steps a run calls that say so.)
    let broken = |m: &str| {
        ["open shell", "left", "doesn't close", "is invalid"]
            .iter()
            .any(|w| m.contains(w))
    };
    if !opts.retry
        || (!matches!(&first, Err(KernelError::OpenResult(_)))
            && !matches!(&first, Err(e) if e.is_failure() && broken(&e.message())))
    {
        return first;
    }
    // Within the fuzzy value: faces a gap off the other input's surfaces moved onto them, and
    // the slivers that leaves collapsed.
    if let Some((fa, fb, keep_a, keep_b)) = fuzzy_merge(a, b, fuzzy, opts.tol.point, false) {
        if super::env_flag!("CADRS_BOOL_DEBUG") {
            eprintln!(
                "fuzzy merge ({fuzzy:e}): faces {} -> {}, {} -> {}; check {:?} {:?}",
                a.faces.len(),
                fa.faces.len(),
                b.faces.len(),
                fb.faces.len(),
                fa.check(),
                fb.check()
            );
        }
        match attempt(run(&fa, &fb, Mode::Bool(op), at(opts.tol.point))) {
            Ok((mut r, clean)) => {
                for srcs in &mut r.from {
                    for (o, f) in srcs.iter_mut() {
                        *f = if *o == 0 { keep_a[*f] } else { keep_b[*f] };
                    }
                }
                if clean {
                    return Ok(r);
                }
                refused.get_or_insert(r);
            }
            Err(e) => {
                if super::env_flag!("CADRS_BOOL_DEBUG") {
                    eprintln!("fuzzy run: {e}");
                }
            }
        }
    }
    // Inputs of several solids (an extrude of regions that share an edge, solids touching face
    // to face or along an edge): one solid at a time.
    // (Not again from within such a split: a piece's boolean that fails would split the other
    // input in turn, and so on down, one level per solid: hundreds of parts overflow the
    // stack.)
    if PER_SOLID.with(|c| c.get()) < 2 {
        PER_SOLID.with(|c| c.set(c.get() + 1));
        let r = per_target_solid(a, b, op, opts).or_else(|| per_tool_solid(a, b, op, opts));
        PER_SOLID.with(|c| c.set(c.get() - 1));
        if let Some(Ok(r)) = r {
            return Ok(r);
        }
    }
    // A ring whose start and end caps touch (a blend swept all the way round): closed up, and
    // run again at each tolerance.
    let (ga, gb) = (
        glue_touching_caps(a, opts.tol.point),
        glue_touching_caps(b, opts.tol.point),
    );
    if ga.is_some() || gb.is_some() {
        let (a2, b2) = (
            ga.as_ref().map_or(a, |g| &g.0),
            gb.as_ref().map_or(b, |g| &g.0),
        );
        let retries = opts
            .tol
            .retries
            .into_iter()
            .filter(|_| !super::env_flag!("CADRS_DEBUG_NO_RETRY"));
        for t in std::iter::once(opts.tol.point).chain(retries) {
            if let Ok((mut r, clean)) = attempt(run(a2, b2, Mode::Bool(op), at(t))) {
                if t > opts.tol.point {
                    cover_absorbed_faces(&mut r, a2, b2, t);
                }
                // Back to the inputs' face numbers.
                for srcs in &mut r.from {
                    for (o, f) in srcs.iter_mut() {
                        if let Some((_, keep)) = if *o == 0 { &ga } else { &gb } {
                            *f = keep[*f];
                        }
                    }
                }
                if clean {
                    return Ok(r);
                }
                refused.get_or_insert(r);
            }
        }
    }
    for t in opts
        .tol
        .retries
        .into_iter()
        .filter(|_| !super::env_flag!("CADRS_DEBUG_NO_RETRY"))
    {
        if let Ok((mut r, clean)) = attempt(run(a, b, Mode::Bool(op), at(t))) {
            cover_absorbed_faces(&mut r, a, b, t);
            if clean {
                return Ok(r);
            }
            refused.get_or_insert(r);
        }
    }
    // Faces near-coincident by more than the run's tolerance at some places and less at others
    // (a cube and its copy turned 1.2e-5 about a skew axis: the faces part by up to 1e-4), which
    // no tolerance of the ladder takes as one throughout: each moved onto the other input's
    // surface of its own kind within the largest, its edges given the distance as tolerance,
    // and run again.
    for wide in [opts.tol.retries[1], 10.0 * opts.tol.retries[1]] {
        if wide <= fuzzy || super::env_flag!("CADRS_DEBUG_NO_RETRY") {
            continue;
        }
        let Some((fa, fb, keep_a, keep_b)) = fuzzy_merge(a, b, wide, opts.tol.point, true) else {
            continue;
        };
        // (At the ladder's largest tolerance only: the moved faces lie within it of the others.)
        let t = opts.tol.retries[1];
        if let Ok((mut r, clean)) = attempt(run(&fa, &fb, Mode::Bool(op), at(t))) {
            for srcs in &mut r.from {
                for (o, f) in srcs.iter_mut() {
                    *f = if *o == 0 { keep_a[*f] } else { keep_b[*f] };
                }
            }
            cover_absorbed_faces(&mut r, a, b, t.max(wide));
            if clean {
                return Ok(r);
            }
            refused.get_or_insert(r);
        }
    }
    match refused {
        Some(r) => hand_back(r),
        None => first,
    }
}

/// A result refused for crossing faces handed back after all (as before the check), counted
/// on this thread ([`take_invalid_results`]); under `CADRS_BOOL_STRICT` the boolean fails.
fn hand_back(r: BoolOut) -> Result<BoolOut> {
    if super::env_flag!("CADRS_BOOL_STRICT") {
        return Err(KernelError::BooleanFailed {
            stage: CROSSING,
            why: "the boolean's result is invalid: its faces cross".into(),
        });
    }
    INVALID_RESULTS.with(|c| c.set(c.get() + 1));
    if super::env_flag!("CADRS_BOOL_DEBUG") {
        eprintln!("boolean: handed back a result with crossing faces");
    }
    Ok(r)
}

/// A run at a raised tolerance `t` takes faces of the two inputs that lie within it of one
/// another as one, and keeps one input's surface: each face of the result lying within `t` of
/// an input face's surface (over the result face's boundary, the boxes overlapping) gets the
/// distance as its edges' and vertices' tolerance (with margin), so the result says how far its
/// faces may be from where the inputs had them (a cube and its copy turned by 1e-5: faces up to
/// 9e-5 apart, the volume off by area × that).
fn cover_absorbed_faces(r: &mut BoolOut, a: &Body, b: &Body, t: f64) {
    let body = &mut r.body;
    let inputs: Vec<(&Surface, Bbox)> = [a, b]
        .iter()
        .flat_map(|x| (0..x.faces.len()).map(move |f| (&x.faces[f].surf, x.face_bbox(f))))
        .collect();
    for fi in 0..body.faces.len() {
        let edges: Vec<usize> = body.faces[fi]
            .loops
            .iter()
            .flatten()
            .map(|c| c.edge)
            .collect();
        // A point inside the face first (off the surfaces of the faces it meets at its edges,
        // which it is then quick to give up), and its boundary.
        let f = &body.faces[fi];
        let inner =
            interior_of_polys(&classify::face_polygons(body, f)).map(|uv| f.surf.eval(uv.x, uv.y));
        let pts: Vec<P3> = inner
            .into_iter()
            .chain(edges.iter().flat_map(|&e| {
                let ed = &body.edges[e];
                (0..=8).map(move |i| ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / 8.0))
            }))
            .collect();
        let bx = body.face_bbox(fi).grow(t);
        // (Each surface given up at its first point farther than `t`, most at once; a surface
        // without a closed-form distance, first at any point off its face's box, before the
        // projection.)
        let within = |s: &Surface, ib: &Bbox| {
            let exact = s.signed_distance(&pts[0]).is_some();
            let ib = ib.grow(t);
            pts.iter().try_fold(0.0, |g: f64, p| {
                if !exact && !ib.contains(p) {
                    return None;
                }
                let d = s.distance(p);
                (d <= t).then_some(g.max(d))
            })
        };
        // (Not the face's own surface: nothing to cover there.)
        let own = &body.faces[fi].surf;
        let gap = inputs
            .iter()
            .filter(|(s, ib)| ib.overlaps(&bx) && *s != own)
            .filter_map(|(s, ib)| within(s, ib))
            .fold(0.0, f64::max);
        let want = 1.1 * gap;
        for e in edges {
            if body.edges[e].tol < want {
                body.edges[e].tol = want;
            }
            for v in body.edges[e].v {
                if body.verts[v].tol < want {
                    body.verts[v].tol = want;
                }
            }
        }
    }
}

/// `a op b` with `a` taken one solid at a time: `b` united with each in turn; each one's
/// intersection with `b` or the part of it outside `b`, side by side. None when `a` isn't
/// several solids (and nothing else).
fn per_target_solid(a: &Body, b: &Body, op: BoolOp, opts: BoolOptions) -> Option<Result<BoolOut>> {
    let pieces = super::exchange::split_into_solids_mapped(a);
    if pieces.len() < 2 || pieces.iter().any(|(p, _)| p.solids.is_empty()) {
        return None;
    }
    let run_all = || -> Result<BoolOut> {
        if op == BoolOp::Union {
            let mut cur = BoolOut {
                body: b.clone(),
                from: (0..b.faces.len()).map(|f| vec![(1, f)]).collect(),
            };
            // Each piece is the target of its union, as in a single run: where its faces and the
            // tool's coincide, the piece's are kept and come first, and their names with them
            // (kids cart's pedal sketched on such a face).
            for (p, map) in &pieces {
                let r = boolean_bodies_with(p, &cur.body, op, opts)?;
                let from = r
                    .from
                    .iter()
                    .map(|srcs| {
                        srcs.iter()
                            .flat_map(|&(o, f)| {
                                if o == 0 {
                                    vec![(0, map[f])]
                                } else {
                                    cur.from[f].clone()
                                }
                            })
                            .collect()
                    })
                    .collect();
                cur = BoolOut { body: r.body, from };
            }
            return Ok(cur);
        }
        let mut out = BoolOut {
            body: Body::default(),
            from: Vec::new(),
        };
        for (p, map) in &pieces {
            let r = match boolean_bodies_with(p, b, op, opts) {
                Err(KernelError::EmptyResult(_)) => continue,
                r => r?,
            };
            out.body.append(&r.body);
            out.from.extend(r.from.iter().map(|srcs| {
                srcs.iter()
                    .map(|&(o, f)| if o == 0 { (0, map[f]) } else { (1, f) })
                    .collect::<Vec<_>>()
            }));
        }
        if out.body.faces.is_empty() {
            return Err(KernelError::EmptyResult("the result is empty".into()));
        }
        Ok(out)
    };
    Some(run_all())
}

/// `a op b` with `b` taken one solid at a time: a union or subtraction of each in turn, an
/// intersection the union of `a`'s intersections with each. None when `b` isn't several solids
/// (and nothing else).
fn per_tool_solid(a: &Body, b: &Body, op: BoolOp, opts: BoolOptions) -> Option<Result<BoolOut>> {
    let pieces = super::exchange::split_into_solids_mapped(b);
    if pieces.len() < 2 || pieces.iter().any(|(p, _)| p.solids.is_empty()) {
        return None;
    }
    // A result's faces traced through `from` (owner 0: `prior`'s faces; 1: the piece's, mapped
    // to `b`'s through `map`).
    let compose = |from: &[Vec<(usize, usize)>],
                   prior: &[Vec<(usize, usize)>],
                   map: &dyn Fn(usize) -> Vec<(usize, usize)>|
     -> Vec<Vec<(usize, usize)>> {
        from.iter()
            .map(|srcs| {
                srcs.iter()
                    .flat_map(|&(o, f)| if o == 0 { prior[f].clone() } else { map(f) })
                    .collect()
            })
            .collect()
    };
    let run_all = || -> Result<BoolOut> {
        match op {
            BoolOp::Union | BoolOp::Subtract => {
                let mut cur = BoolOut {
                    body: a.clone(),
                    from: (0..a.faces.len()).map(|f| vec![(0, f)]).collect(),
                };
                for (p, map) in &pieces {
                    let r = boolean_bodies_with(&cur.body, p, op, opts)?;
                    let from = compose(&r.from, &cur.from, &|f| vec![(1, map[f])]);
                    cur = BoolOut { body: r.body, from };
                }
                Ok(cur)
            }
            BoolOp::Intersect => {
                let mut acc: Option<BoolOut> = None;
                for (p, map) in &pieces {
                    let r = match boolean_bodies_with(a, p, op, opts) {
                        Err(KernelError::EmptyResult(_)) => continue,
                        r => r?,
                    };
                    let from: Vec<Vec<(usize, usize)>> = r
                        .from
                        .iter()
                        .map(|srcs| {
                            srcs.iter()
                                .map(|&(o, f)| if o == 0 { (0, f) } else { (1, map[f]) })
                                .collect()
                        })
                        .collect();
                    let piece = BoolOut { body: r.body, from };
                    acc = Some(match acc {
                        None => piece,
                        Some(prev) => {
                            let u =
                                boolean_bodies_with(&prev.body, &piece.body, BoolOp::Union, opts)?;
                            let from = compose(&u.from, &prev.from, &|f| piece.from[f].clone());
                            BoolOut { body: u.body, from }
                        }
                    });
                }
                acc.ok_or_else(|| KernelError::EmptyResult("the result is empty".into()))
            }
        }
    };
    Some(run_all())
}

/// The faces of `a` and `b` that lie on one surface only within 1e-7 of their size (beyond the
/// run's tolerance): the two bodies with those faces' edges and vertices given the gap (with
/// margin) as tolerance; None when there are none.
/// The default fuzzy value of a boolean of `a` and `b` ([`BoolOptions::fuzzy`]).
/// Whether the union of `a` and `b` is sure to be more than one solid: a solid of either lies
/// apart from all the others (by their boxes, grown by the bodies' tolerances). Not for sheets
/// (`false` when either has faces outside its solids).
pub(crate) fn union_falls_apart(a: &Body, b: &Body) -> bool {
    let mut boxes: Vec<Bbox> = Vec::new();
    for x in [a, b] {
        let in_solids: usize = (0..x.solids.len()).map(|s| x.solid_faces(s).len()).sum();
        if x.solids.is_empty() || in_solids != x.faces.len() {
            return false;
        }
        let tol = x
            .edges
            .iter()
            .map(|e| e.tol)
            .chain(x.verts.iter().map(|v| v.tol))
            .fold(tol::POINT, f64::max);
        for s in 0..x.solids.len() {
            let mut bb = Bbox::empty();
            for f in x.solid_faces(s) {
                bb.union(&x.face_bbox(f));
            }
            boxes.push(bb.grow(2.0 * tol));
        }
    }
    boxes.len() > 1
        && (0..boxes.len())
            .any(|i| (0..boxes.len()).all(|j| i == j || !boxes[i].overlaps(&boxes[j])))
}

fn carried_fuzzy(a: &Body, b: &Body) -> f64 {
    let carried = [a, b]
        .iter()
        .flat_map(|x| {
            x.edges
                .iter()
                .map(|e| e.tol)
                .chain(x.verts.iter().map(|v| v.tol))
        })
        .fold(0.0, f64::max);
    let size = a.bbox().diagonal().max(b.bbox().diagonal());
    carried.min(size * FUZZY_MAX)
}

fn near_coincident_tolerances(a: &Body, b: &Body, tol: f64, fuzzy: f64) -> Option<(Body, Body)> {
    let face_pts = |body: &Body, f: usize| -> Vec<P3> {
        let mut pts = Vec::new();
        for c in body.faces[f].loops.iter().flatten() {
            let e = &body.edges[c.edge];
            for i in 0..=8 {
                pts.push(e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0));
            }
        }
        pts
    };
    let boxes = |body: &Body| -> Vec<(Vec<P3>, Bbox)> {
        (0..body.faces.len())
            .map(|f| {
                let pts = face_pts(body, f);
                let mut bb = Bbox::empty();
                for p in &pts {
                    bb.add(p);
                }
                (pts, bb)
            })
            .collect()
    };
    let (ba, bb) = (boxes(a), boxes(b));
    let mut raise: Vec<(usize, usize, f64)> = Vec::new();
    for (fa, (pa, xa)) in ba.iter().enumerate() {
        for (fb, (pb, xb)) in bb.iter().enumerate() {
            let size = xa.diagonal().max(xb.diagonal());
            // (Within the fuzzy value, with half again for the faces' own deviations.)
            let ctol = (size * 1e-7).max(1.5 * fuzzy);
            if ctol <= tol || !xa.grow(ctol).overlaps(&xb.grow(ctol)) {
                continue;
            }
            let (sa, sb) = (&a.faces[fa].surf, &b.faces[fb].surf);
            if sa.same_domain(sb, tol).is_some() || sa.same_domain(sb, ctol).is_none() {
                continue;
            }
            // The gap: the farthest boundary point of either face from the other's surface.
            let gap = pb
                .iter()
                .map(|p| sa.distance(p))
                .chain(pa.iter().map(|p| sb.distance(p)))
                .fold(0.0, f64::max);
            if gap > tol && gap <= ctol * 2.0 {
                raise.push((fa, fb, gap * 1.5));
            }
        }
    }
    if super::env_flag!("CADRS_BOOL_DEBUG") {
        eprintln!("near-coincident faces (a, b, tolerance): {raise:?}");
    }
    if raise.is_empty() {
        return None;
    }
    let (mut a2, mut b2) = (a.clone(), b.clone());
    let bump = |body: &mut Body, f: usize, t: f64| {
        let es: Vec<usize> = body.faces[f]
            .loops
            .iter()
            .flatten()
            .map(|c| c.edge)
            .collect();
        for e in es {
            body.edges[e].tol = body.edges[e].tol.max(t);
            for v in body.edges[e].v {
                body.verts[v].tol = body.verts[v].tol.max(t);
            }
        }
    };
    for (fa, fb, t) in raise {
        bump(&mut a2, fa, t);
        bump(&mut b2, fb, t);
    }
    Some((a2, b2))
}

/// A body whose two planar faces lie on one plane facing each other with matching boundaries
/// (a ring swept all the way round, its start and end caps a hair apart): the caps dropped and
/// their edges glued pairwise, so the ring closes on itself; None when there are none. The
/// glued edges take the caps' gap as tolerance.
fn glue_touching_caps(b: &Body, tol: f64) -> Option<(Body, Vec<usize>)> {
    if b.solids.len() != 1 {
        return None;
    }
    // (The caps' edges within ten times the run's tolerance of each other.)
    let gap_max = tol * 10.0;
    let planes: Vec<usize> = (0..b.faces.len())
        .filter(|&f| {
            matches!(b.faces[f].surf, Surface::Plane { .. }) && b.faces[f].loops.len() == 1
        })
        .collect();
    let mut out = b.clone();
    let mut drop = vec![false; b.faces.len()];
    let mut glued = false;
    for (i, &f) in planes.iter().enumerate() {
        for &g in &planes[i + 1..] {
            if drop[f] || drop[g] {
                continue;
            }
            let (lf, lg) = (&b.faces[f].loops[0], &b.faces[g].loops[0]);
            if lf.len() != lg.len() {
                continue;
            }
            let (Surface::Plane { f: pf }, Surface::Plane { f: pg }) =
                (&b.faces[f].surf, &b.faces[g].surf)
            else {
                continue;
            };
            let nf = pf.z * if b.faces[f].sense { 1.0 } else { -1.0 };
            let ng = pg.z * if b.faces[g].sense { 1.0 } else { -1.0 };
            if nf.dot(&ng) > -1.0 + tol::ANGULAR_POINT {
                continue;
            }
            // Each edge of f against the edge of g with the same ends (either way round).
            let ends = |e: usize| (b.verts[b.edges[e].v[0]].p, b.verts[b.edges[e].v[1]].p);
            let mut pairs: Vec<(usize, usize, bool, f64)> = Vec::new(); // (keep, drop, same direction, gap)
            for cf in lf {
                let (p0, p1) = ends(cf.edge);
                let found = lg.iter().find_map(|cg| {
                    let (q0, q1) = ends(cg.edge);
                    let same = (p0 - q0).norm().max((p1 - q1).norm());
                    let rev = (p0 - q1).norm().max((p1 - q0).norm());
                    let (d, dir) = if same <= rev {
                        (same, true)
                    } else {
                        (rev, false)
                    };
                    let e = &b.edges[cf.edge];
                    let ge = &b.edges[cg.edge];
                    let tm = 0.5 * (ge.t[0] + ge.t[1]);
                    let q = ge.curve.point(tm);
                    let dm = (e.curve.point(e.curve.closest(
                        &q,
                        e.t[0].min(e.t[1]),
                        e.t[0].max(e.t[1]),
                    )) - q)
                        .norm();
                    (d.max(dm) <= gap_max && cg.edge != cf.edge).then_some((
                        cg.edge,
                        dir,
                        d.max(dm),
                    ))
                });
                let Some((ge, dir, d)) = found else { break };
                pairs.push((cf.edge, ge, dir, d));
            }
            if pairs.len() != lf.len() {
                continue;
            }
            drop[f] = true;
            drop[g] = true;
            glued = true;
            for (keep, gone, dir, d) in pairs {
                let t = (d * 1.5).max(out.edges[keep].tol);
                out.edges[keep].tol = t;
                let (kv, gv) = (out.edges[keep].v, out.edges[gone].v);
                for (k, gvx) in [(0, gv[0]), (1, gv[1])] {
                    let kvx = if dir { kv[k] } else { kv[1 - k] };
                    out.verts[kvx].tol = out.verts[kvx].tol.max(t);
                    // Every edge ending at the dropped edge's vertex ends at the kept one's.
                    if gvx != kvx {
                        for e in out.edges.iter_mut() {
                            for v in e.v.iter_mut() {
                                if *v == gvx {
                                    *v = kvx;
                                }
                            }
                        }
                    }
                }
                let keep_edge = out.edges[keep].clone();
                for fi in 0..out.faces.len() {
                    if fi == f || fi == g {
                        continue;
                    }
                    let surf = out.faces[fi].surf.clone();
                    for l in &mut out.faces[fi].loops {
                        for c in l.iter_mut() {
                            if c.edge != gone {
                                continue;
                            }
                            let ge = &b.edges[gone];
                            let near = c.pc.point(0.5 * (ge.t[0] + ge.t[1]));
                            c.edge = keep;
                            c.fwd = if dir { c.fwd } else { !c.fwd };
                            if !(dir && ge.t == keep_edge.t) {
                                c.pc = super::pcurve::pcurve(
                                    &surf,
                                    &keep_edge.curve,
                                    keep_edge.t[0],
                                    keep_edge.t[1],
                                    Some(P2::new(near.x, near.y)),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    if !glued {
        return None;
    }
    // Edges now ending at a kept vertex a hair from their curve's end: that as tolerance.
    for e in 0..out.edges.len() {
        let ed = &out.edges[e];
        let gap = (0..2)
            .map(|k| (ed.curve.point(ed.t[k]) - out.verts[ed.v[k]].p).norm())
            .fold(0.0, f64::max);
        if gap > ed.tol {
            let t = gap * 1.5;
            out.edges[e].tol = t;
            for v in out.edges[e].v {
                out.verts[v].tol = out.verts[v].tol.max(t);
            }
        }
    }
    let keep: Vec<usize> = (0..out.faces.len()).filter(|&f| !drop[f]).collect();
    out.faces = keep.iter().map(|&f| out.faces[f].clone()).collect();
    super::unify::compact(&mut out);
    assemble_shells(&mut out).ok()?;
    (out.solids.len() == 1 && out.shells.iter().all(|s| s.closed)).then_some((out, keep))
}

/// Each span's box of a spline edge of more than two spans (grown by the larger of `tol` and
/// the edge's tolerance), else none.
fn span_boxes_of(ed: &WEdge, tol: f64) -> Option<Vec<Bbox>> {
    if ed.degenerate || !matches!(ed.curve, Curve::Nurbs(_)) {
        return None;
    }
    let ps = ed.curve.pieces(ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
    (ps.len() > 2).then(|| {
        ps.windows(2)
            .map(|w| curve_bbox(&ed.curve, w[0], w[1]).grow(tol.max(ed.tol)))
            .collect()
    })
}

/// Faces of `b` on a periodic surface whose seam edge runs across one of their holes (an
/// earlier operation left the seam whole where a hole opened over it): the seam is cut at the
/// hole's two vertices on it, its piece inside the hole dropped and the hole's two halves joined
/// into the outer loop, each on its side of the seam. True when any face changed.
fn cut_seams_at_holes(b: &mut Body, tol: f64) -> bool {
    let mut changed = false;
    for fi in 0..b.faces.len() {
        let surf = b.faces[fi].surf.clone();
        if !(surf.u_periodic() || surf.v_periodic()) || b.faces[fi].loops.len() < 2 {
            continue;
        }
        let l0 = b.faces[fi].loops[0].clone();
        // Seam edges: used twice in the outer loop, once each way.
        let seams: Vec<usize> = l0
            .iter()
            .filter(|c| {
                c.fwd
                    && !b.edges[c.edge].degenerate
                    && l0.iter().any(|d| d.edge == c.edge && !d.fwd)
            })
            .map(|c| c.edge)
            .collect();
        for s in seams {
            let l0 = b.faces[fi].loops[0].clone();
            for hi in 1..b.faces[fi].loops.len() {
                let hole = b.faces[fi].loops[hi].clone();
                let se = b.edges[s].clone();
                let start = |b: &Body, c: &Coedge| {
                    if c.fwd {
                        b.edges[c.edge].v[0]
                    } else {
                        b.edges[c.edge].v[1]
                    }
                };
                // The hole's vertices on the seam, inside it.
                let mut on: Vec<(f64, usize)> = Vec::new();
                for c in &hole {
                    let w = start(b, c);
                    let vt = b.verts[w].tol.max(tol);
                    if let Some(t) = on_curve(&se.curve, se.t[0], se.t[1], &b.verts[w].p, vt)
                        && t > se.t[0] + tol::ROUNDOFF
                        && t < se.t[1] - tol::ROUNDOFF
                        && !on.iter().any(|x| x.1 == w)
                    {
                        on.push((t, w));
                    }
                }
                if on.len() != 2 {
                    continue;
                }
                on.sort_by(|x, y| x.0.total_cmp(&y.0));
                let ((t1, w1), (t2, w2)) = (on[0], on[1]);
                let k1 = hole
                    .iter()
                    .position(|c| start(b, c) == w1)
                    .expect("w1 starts a coedge of the hole");
                let k2 = hole
                    .iter()
                    .position(|c| start(b, c) == w2)
                    .expect("w2 starts a coedge of the hole");
                // The seam's two pieces outside the hole.
                let a = Edge {
                    t: [se.t[0], t1],
                    v: [se.v[0], w1],
                    ..se.clone()
                };
                let bb = Edge {
                    t: [t2, se.t[1]],
                    v: [w2, se.v[1]],
                    ..se.clone()
                };
                b.edges[s] = a;
                b.edges.push(bb);
                let eb = b.edges.len() - 1;
                // The hole's two paths between w1 and w2, each in the hole's direction.
                let n = hole.len();
                let path = |from: usize, to: usize| -> Vec<Coedge> {
                    let mut out = Vec::new();
                    let mut k = from;
                    while k != to {
                        out.push(hole[k].clone());
                        k = (k + 1) % n;
                    }
                    out
                };
                let (p12, p21) = (path(k1, k2), path(k2, k1));
                // A path's p-curves moved by whole periods to start where `uv` is.
                let placed = |b: &Body, p: &[Coedge], uv: P3| -> Vec<Coedge> {
                    let Some(c0) = p.first() else {
                        return Vec::new();
                    };
                    let e0 = &b.edges[c0.edge];
                    let q = c0.pc.point(if c0.fwd { e0.t[0] } else { e0.t[1] });
                    let per = |d: f64, periodic: bool| {
                        if periodic {
                            (d / std::f64::consts::TAU).round() * std::f64::consts::TAU
                        } else {
                            0.0
                        }
                    };
                    let (du, dv) = (
                        per(uv.x - q.x, surf.u_periodic()),
                        per(uv.y - q.y, surf.v_periodic()),
                    );
                    p.iter()
                        .map(|c| Coedge {
                            pc: c.pc.transformed(&Affine::translation(V3::new(du, dv, 0.0))),
                            ..c.clone()
                        })
                        .collect()
                };
                let mut out: Vec<Coedge> = Vec::new();
                for c in &l0 {
                    if c.edge != s {
                        out.push(c.clone());
                    } else if c.fwd {
                        // Along the seam to w1, round the hole to w2, on to the seam's end.
                        out.push(Coedge {
                            edge: s,
                            fwd: true,
                            pc: c.pc.clone(),
                        });
                        out.extend(placed(b, &p12, c.pc.point(t1)));
                        out.push(Coedge {
                            edge: eb,
                            fwd: true,
                            pc: c.pc.clone(),
                        });
                    } else {
                        out.push(Coedge {
                            edge: eb,
                            fwd: false,
                            pc: c.pc.clone(),
                        });
                        out.extend(placed(b, &p21, c.pc.point(t2)));
                        out.push(Coedge {
                            edge: s,
                            fwd: false,
                            pc: c.pc.clone(),
                        });
                    }
                }
                // The seam meets the hole's vertices within their tolerance: the p-curves' ends
                // there may stand that far apart.
                let (mu, mv) = surf.metric();
                for k in 0..out.len() {
                    let (x, y) = (&out[k], &out[(k + 1) % out.len()]);
                    let (ex, ey) = (&b.edges[x.edge], &b.edges[y.edge]);
                    let pa = x.pc.point(if x.fwd { ex.t[1] } else { ex.t[0] });
                    let pb = y.pc.point(if y.fwd { ey.t[0] } else { ey.t[1] });
                    let gap = ((pa.x - pb.x) * mu).hypot((pa.y - pb.y) * mv);
                    let w = if x.fwd { ex.v[1] } else { ex.v[0] };
                    if gap > b.verts[w].tol {
                        b.verts[w].tol = gap * 1.5;
                    }
                }
                b.faces[fi].loops[0] = out;
                b.faces[fi].loops.remove(hi);
                changed = true;
                break;
            }
        }
    }
    changed
}

/// `pc` (the p-curve of `c` over `t` on `s`) or its copy a whole parameter range over in a
/// direction of a B-spline `s`, whichever passes nearer `near` while still on the curve.
fn seam_side(s: &Surface, c: &Curve, t: [f64; 2], pc: Curve, near: P2) -> Curve {
    let Surface::Nurbs(n) = s else { return pc };
    let ((u0, u1), (v0, v1)) = n.range();
    let tm = 0.5 * (t[0] + t[1]);
    let m = pc.point(tm);
    let fits = |q: &Curve| {
        (0..=8).all(|k| {
            let x = t[0] + (t[1] - t[0]) * k as f64 / 8.0;
            let w = q.point(x);
            w.x >= u0 - tol::ROUNDOFF
                && w.x <= u1 + tol::ROUNDOFF
                && w.y >= v0 - tol::ROUNDOFF
                && w.y <= v1 + tol::ROUNDOFF
                && (s.eval(w.x, w.y) - c.point(x)).norm() <= tol::LOOP_GAP
        })
    };
    let mut best = ((m.x - near.x).hypot(m.y - near.y), pc.clone());
    for (du, dv) in [
        (u1 - u0, 0.0),
        (u0 - u1, 0.0),
        (0.0, v1 - v0),
        (0.0, v0 - v1),
    ] {
        let d = (m.x + du - near.x).hypot(m.y + dv - near.y);
        if d < best.0 {
            let q = pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)));
            if fits(&q) {
                best = (d, q);
            }
        }
    }
    best.1
}

/// Faces of one input lying within the fuzzy value of an analytic face of the other on a
/// surface of their own (an arc a gap off the circle it continues, a strip bridging the gap):
/// they take that face's surface, their edges the distance as tolerance. Then faces of either
/// input left a fuzzy width wide (two long edges side by side, the rest no longer than the
/// width) are collapsed onto one of their long edges. None when nothing moved; else the two
/// bodies and, for each, its faces' indices in the input.
fn fuzzy_merge(
    a: &Body,
    b: &Body,
    fuzzy: f64,
    tol: f64,
    alike: bool,
) -> Option<(Body, Body, Vec<usize>, Vec<usize>)> {
    let lim = 1.5 * fuzzy;
    if fuzzy <= tol {
        return None;
    }
    let (mut a2, mut b2) = (a.clone(), b.clone());
    let moved_a = snap_within(&mut a2, b, lim, tol, alike);
    let moved_b = snap_within(&mut b2, &a2, lim, tol, alike);
    if moved_a.is_empty() && moved_b.is_empty() {
        return None;
    }
    let keep_a = collapse_slivers(&mut a2, lim, &moved_a);
    let keep_b = collapse_slivers(&mut b2, lim, &moved_b);
    // (Only when both are still valid: a body flattened onto a face isn't a merge.)
    (a2.check().is_empty() && b2.check().is_empty()).then_some((a2, b2, keep_a, keep_b))
}

/// [`fuzzy_merge`]'s first step on `x` against `other`: the faces moved (with `alike`, only
/// onto a surface of their own kind: a plane onto a plane, near-coincident faces).
fn snap_within(x: &mut Body, other: &Body, lim: f64, tol: f64, alike: bool) -> Vec<usize> {
    let analytic = |s: &Surface| {
        matches!(
            s,
            Surface::Plane { .. }
                | Surface::Cylinder { .. }
                | Surface::Cone { .. }
                | Surface::Sphere { .. }
                | Surface::Torus { .. }
        )
    };
    let targets: Vec<(usize, Bbox)> = (0..other.faces.len())
        .filter(|&g| analytic(&other.faces[g].surf))
        .map(|g| {
            (
                g,
                super::kernel::subset(other, &[g], false).bbox().grow(lim),
            )
        })
        .collect();
    let mut moved = Vec::new();
    for fi in 0..x.faces.len() {
        let f = &x.faces[fi];
        let mut pts: Vec<P3> = Vec::new();
        for c in f.loops.iter().flatten() {
            let e = &x.edges[c.edge];
            pts.extend((0..=8).map(|k| e.point(e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 8.0)));
        }
        for uv in interior_points_of_polys(&classify::face_polygons(x, f))
            .into_iter()
            .take(16)
        {
            pts.push(f.surf.eval(uv.x, uv.y));
        }
        let mut fb = Bbox::empty();
        for p in &pts {
            fb.add(p);
        }
        for &(g, ref gb) in &targets {
            let t = &other.faces[g].surf;
            if !gb.overlaps(&fb)
                || f.surf.same_domain(t, tol).is_some()
                || (alike && std::mem::discriminant(&f.surf) != std::mem::discriminant(t))
            {
                continue;
            }
            let dev = pts.iter().map(|p| t.distance(p)).fold(0.0, f64::max);
            if dev > lim {
                continue;
            }
            let surf = t.clone();
            let edges = x.edges.clone();
            let mid = fb.center();
            let near = surf.project(&mid, None);
            for l in &mut x.faces[fi].loops {
                for co in l.iter_mut() {
                    let e = &edges[co.edge];
                    co.pc = super::pcurve::pcurve(
                        &surf,
                        &e.curve,
                        e.t[0],
                        e.t[1],
                        Some(P2::new(near.0, near.1)),
                    );
                }
            }
            x.faces[fi].surf = surf;
            super::build::fix_senses(x, [fi]);
            let t = (dev * 1.5).max(tol::CONFUSION);
            let es: Vec<usize> = x.faces[fi].loops.iter().flatten().map(|c| c.edge).collect();
            for e in es {
                x.edges[e].tol = x.edges[e].tol.max(t);
                for v in x.edges[e].v {
                    x.verts[v].tol = x.verts[v].tol.max(t);
                }
            }
            moved.push(fi);
            break;
        }
    }
    moved
}

/// [`fuzzy_merge`]'s second step: faces `lim` wide collapsed, and moved faces (`moved`) up to
/// ten times that (a strip bridging the gap at a slant, folded back over its neighbours once
/// on one surface with them). The faces kept, by input index.
fn collapse_slivers(x: &mut Body, lim: f64, moved: &[usize]) -> Vec<usize> {
    let mut drop = vec![false; x.faces.len()];
    for fi in 0..x.faces.len() {
        if x.faces[fi].loops.len() != 1 {
            continue;
        }
        let l = x.faces[fi].loops[0].clone();
        let width = if moved.contains(&fi) { 10.0 * lim } else { lim };
        let long: Vec<usize> = (0..l.len())
            .filter(|&k| {
                let e = &x.edges[l[k].edge];
                !e.degenerate && e.curve.longer_than(e.t[0], e.t[1], 2.0 * width)
            })
            .collect();
        if long.len() != 2 {
            continue;
        }
        // The edge kept: one whose other face hasn't moved (its geometry exact), if either.
        let other_moved = |e: usize| {
            x.faces.iter().enumerate().any(|(g, gf)| {
                g != fi && moved.contains(&g) && gf.loops.iter().flatten().any(|c| c.edge == e)
            })
        };
        let (mut e1, mut e2) = (l[long[0]].edge, l[long[1]].edge);
        if other_moved(e1) && !other_moved(e2) {
            std::mem::swap(&mut e1, &mut e2);
        }
        if e1 == e2 {
            continue;
        }
        // The second long edge along the first within the width.
        let (c1, c2) = (x.edges[e1].clone(), x.edges[e2].clone());
        let side = (0..=8).all(|k| {
            let q = c2.point(c2.t[0] + (c2.t[1] - c2.t[0]) * k as f64 / 8.0);
            (c1.point(c1.curve.closest(&q, c1.t[0], c1.t[1])) - q).norm() <= width
        });
        if !side {
            continue;
        }
        // e2's ends onto e1's (nearest), the short edges between them gone.
        let ends = |e: &Edge| [e.v[0], e.v[1]];
        let (v1, v2) = (ends(&c1), ends(&c2));
        let same = (x.verts[v2[0]].p - x.verts[v1[0]].p).norm()
            + (x.verts[v2[1]].p - x.verts[v1[1]].p).norm()
            <= (x.verts[v2[0]].p - x.verts[v1[1]].p).norm()
                + (x.verts[v2[1]].p - x.verts[v1[0]].p).norm();
        let map = if same {
            [(v2[0], v1[0]), (v2[1], v1[1])]
        } else {
            [(v2[0], v1[1]), (v2[1], v1[0])]
        };
        let short: Vec<usize> = (0..l.len())
            .filter(|k| !long.contains(k))
            .map(|k| l[k].edge)
            .collect();
        for (gone, keep) in map {
            let gap = (x.verts[gone].p - x.verts[keep].p).norm();
            x.verts[keep].tol = x.verts[keep].tol.max(gap * 1.5);
            for e in x.edges.iter_mut() {
                for v in e.v.iter_mut() {
                    if *v == gone {
                        *v = keep;
                    }
                }
            }
        }
        x.edges[e1].tol = x.edges[e1].tol.max(width);
        // The short edges become points (their faces' p-curves still bridge the gap in (u, v)).
        for &e in &short {
            x.edges[e].degenerate = true;
            x.edges[e].tol = x.edges[e].tol.max(width);
        }
        // Other faces: e2 becomes e1.
        let ke = x.edges[e1].clone();
        for gi in 0..x.faces.len() {
            if gi == fi {
                continue;
            }
            let surf = x.faces[gi].surf.clone();
            for lp in &mut x.faces[gi].loops {
                for c in lp.iter_mut() {
                    if c.edge == e2 {
                        let near = c.pc.point(0.5 * (c2.t[0] + c2.t[1]));
                        c.edge = e1;
                        c.fwd = if same { c.fwd } else { !c.fwd };
                        c.pc = super::pcurve::pcurve(
                            &surf,
                            &ke.curve,
                            ke.t[0],
                            ke.t[1],
                            Some(P2::new(near.x, near.y)),
                        );
                    }
                }
            }
        }
        drop[fi] = true;
    }
    let keep: Vec<usize> = (0..x.faces.len()).filter(|&f| !drop[f]).collect();
    if keep.len() < x.faces.len() {
        x.faces = keep.iter().map(|&f| x.faces[f].clone()).collect();
        super::unify::compact(x);
        let _ = assemble_shells(x);
    }
    keep
}

/// [`boolean_bodies`], with faces of the two inputs on one surface merged whatever the
/// operation (a blend's tool face flush with a face it ends against becomes part of it).
pub fn boolean_bodies_flush(a: &Body, b: &Body, op: BoolOp) -> Result<BoolOut> {
    boolean_bodies_with(
        a,
        b,
        op,
        BoolOptions {
            flush: true,
            ..BoolOptions::default()
        },
    )
}

/// The first loop of `b` whose p-curves don't meet in (u, v) at a vertex (beyond the vertex's
/// tolerance, as [`Body::check`] has it; whole turns apart across a periodic seam is meeting).
fn uv_gap(b: &Body) -> Option<String> {
    for (fi, f) in b.faces.iter().enumerate() {
        let (mu, mv) = f.surf.metric();
        let off = |d: f64, periodic: bool| {
            if periodic {
                (d - (d / TAU).round() * TAU).abs()
            } else {
                d.abs()
            }
        };
        for (li, l) in f.loops.iter().enumerate() {
            for k in 0..l.len() {
                let (x, y) = (&l[k], &l[(k + 1) % l.len()]);
                let (ex, ey) = (b.edges.get(x.edge)?, b.edges.get(y.edge)?);
                let (_, t1) = super::topo::co_range(ex, x.fwd);
                let (t0, _) = super::topo::co_range(ey, y.fwd);
                let (pa, pb) = (x.pc.point(t1), y.pc.point(t0));
                let v = if x.fwd { ex.v[1] } else { ex.v[0] };
                let tol = b
                    .verts
                    .get(v)
                    .map_or(0.0, |v| v.tol)
                    .max(super::tol::LOOP_GAP);
                if off(pa.x - pb.x, f.surf.u_periodic()) * mu > tol
                    || off(pa.y - pb.y, f.surf.v_periodic()) * mv > tol
                {
                    return Some(format!(
                        "face {fi} loop {li}: p-curve gap after coedge {k} ({:.3e}, {:.3e})",
                        pa.x - pb.x,
                        pa.y - pb.y
                    ));
                }
            }
        }
    }
    None
}

/// Gives B-spline faces of `a` that lie (within 2 × [`VTOL`]) on a plane or cylinder face of
/// `b` that surface, with their p-curves and sense recomputed. True when any changed.
fn snap_to(a: &mut Body, b: &Body, tol: f64) -> bool {
    if super::env_flag!("CADRS_DEBUG_NO_SNAP") {
        return false;
    }
    let mut changed = false;
    let targets: Vec<(usize, Bbox)> = b
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| matches!(f.surf, Surface::Plane { .. } | Surface::Cylinder { .. }))
        .map(|(i, _)| (i, super::kernel::subset(b, &[i], false).bbox().grow(1e-3)))
        .collect();
    if targets.is_empty() {
        return false;
    }
    for fi in 0..a.faces.len() {
        let Surface::Nurbs(ns) = &a.faces[fi].surf else {
            continue;
        };
        let ((u0, u1), (v0, v1)) = ns.range();
        let s = a.faces[fi].surf.clone();
        let n = 16;
        let pts: Vec<P3> = (0..=n)
            .flat_map(|i| (0..=n).map(move |j| (i, j)))
            .map(|(i, j)| {
                s.eval(
                    u0 + (u1 - u0) * i as f64 / n as f64,
                    v0 + (v1 - v0) * j as f64 / n as f64,
                )
            })
            .collect();
        let mut fb = Bbox::empty();
        for p in &pts {
            fb.add(p);
        }
        for &(ti, ref tb) in &targets {
            if !tb.overlaps(&fb) {
                continue;
            }
            let t = &b.faces[ti].surf;
            if super::env_flag!("CADRS_SNAP_DEBUG") {
                let dev = pts
                    .iter()
                    .filter_map(|p| t.signed_distance(p))
                    .map(f64::abs)
                    .fold(0.0, f64::max);
                eprintln!(
                    "snap? face {fi} onto {ti} ({:?}): deviation {dev:e}",
                    t.kind()
                );
            }
            if pts
                .iter()
                .any(|p| t.signed_distance(p).is_none_or(|d| d.abs() > 2.0 * tol))
            {
                continue;
            }
            // Between the samples too.
            let fine = 64;
            let ok = (0..fine).all(|i| {
                (0..fine).all(|j| {
                    let p = s.eval(
                        u0 + (u1 - u0) * (i as f64 + 0.5) / fine as f64,
                        v0 + (v1 - v0) * (j as f64 + 0.5) / fine as f64,
                    );
                    t.signed_distance(&p).is_some_and(|d| d.abs() <= 2.0 * tol)
                })
            });
            if !ok {
                continue;
            }
            let mid = s.eval(0.5 * (u0 + u1), 0.5 * (v0 + v1));
            let near = t.project(&mid, None);
            let edges = a.edges.clone();
            for l in &mut a.faces[fi].loops {
                for co in l.iter_mut() {
                    let e = &edges[co.edge];
                    co.pc = super::pcurve::pcurve(
                        t,
                        &e.curve,
                        e.t[0],
                        e.t[1],
                        Some(P2::new(near.0, near.1)),
                    );
                }
            }
            a.faces[fi].surf = t.clone();
            super::build::fix_senses(a, [fi]);
            if super::env_flag!("CADRS_BOOL_DEBUG") {
                eprintln!(
                    "snapped face {fi} onto the other body's face {ti} ({:?})",
                    t.kind()
                );
            }
            changed = true;
            break;
        }
    }
    changed
}

/// What to do with the split faces.
#[derive(Debug, Clone)]
pub enum Mode {
    Bool(BoolOp),
    /// Split the listed faces of the target (all when None) by the tool's faces and keep every
    /// piece of the target: the target's faces imprinted by the tool.
    Imprint(Option<Vec<usize>>),
    /// Only where the faces of the two meet: the intersection edges (and edges of one lying
    /// on faces of the other), as a body of edges.
    Section,
}

/// The section of two bodies: the edges where their faces meet (a body of edges, no faces).
pub fn section(a: &Body, b: &Body) -> Result<Body> {
    run(a, b, Mode::Section, RunOpts::PLAIN).map(|o| o.body)
}

pub fn imprint(a: &Body, b: &Body, faces: Option<Vec<usize>>) -> Result<BoolOut> {
    run(a, b, Mode::Imprint(faces), RunOpts::PLAIN)
}

/// One run of the boolean.
fn run(a: &Body, b: &Body, mode: Mode, opts: RunOpts) -> Result<BoolOut> {
    RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let RunOpts {
        tol: run_tol,
        flush,
        merge_coplanar,
        work_budget,
        check_crossings,
    } = opts;
    // The work done so far, against the budget.
    let mut work = Work {
        done: 0,
        budget: work_budget,
    };
    // B-spline faces lying on an analytic face of the other body (within the tolerance:
    // approximated sweeps along helices, lofts of circles) take its exact surface, so the two
    // are imprinted as coincident instead of intersected nearly tangentially.
    let (sa, sb);
    let (a, b) = {
        let mut a2 = a.clone();
        let mut b2 = b.clone();
        let seams_a = cut_seams_at_holes(&mut a2, run_tol);
        let seams_b = cut_seams_at_holes(&mut b2, run_tol);
        let ca = snap_to(&mut a2, b, run_tol) || seams_a;
        let cb = snap_to(&mut b2, a, run_tol) || seams_b;
        sa = a2;
        sb = b2;
        (if ca { &sa } else { a }, if cb { &sb } else { b })
    };
    let op = match &mode {
        Mode::Bool(op) => *op,
        Mode::Imprint(_) | Mode::Section => BoolOp::Subtract,
    };
    let selected = |f: usize| -> bool {
        match &mode {
            Mode::Imprint(Some(list)) => list.contains(&f),
            _ => true,
        }
    };
    let imprinting = matches!(mode, Mode::Imprint(_));
    // Disjoint boxes: nothing to intersect.
    let (ba, bb) = (a.bbox(), b.bbox());
    if !ba.grow(run_tol).overlaps(&bb.grow(run_tol)) {
        if imprinting {
            return Ok(BoolOut {
                body: a.clone(),
                from: (0..a.faces.len()).map(|i| vec![(0, i)]).collect(),
            });
        }
        return match op {
            BoolOp::Union => {
                let mut out = a.clone();
                let n = a.faces.len();
                out.append(b);
                let mut from: Vec<Vec<(usize, usize)>> = (0..n).map(|i| vec![(0, i)]).collect();
                from.extend((0..b.faces.len()).map(|i| vec![(1, i)]));
                Ok(BoolOut { body: out, from })
            }
            BoolOp::Subtract => Ok(BoolOut {
                body: a.clone(),
                from: (0..a.faces.len()).map(|i| vec![(0, i)]).collect(),
            }),
            BoolOp::Intersect => Err(KernelError::EmptyResult("the result is empty".into())),
        };
    }
    let mut prof = Prof::new();
    let max_tol = a
        .verts
        .iter()
        .chain(&b.verts)
        .map(|v| v.tol)
        .chain(a.edges.iter().chain(&b.edges).map(|e| e.tol))
        .fold(0.0, f64::max);
    let mut pool = Pool::with_tolerance(run_tol, max_tol);
    let mut faces: Vec<WFace> = Vec::new();
    import(&mut pool, &mut faces, a, 0, false);
    let na_edges = pool.edges.len();
    let na_faces = faces.len();
    import(&mut pool, &mut faces, b, 1, true);
    let ne = pool.edges.len();
    let owner_of_edge = |e: usize| if e < na_edges { 0 } else { 1 };
    let ebox: Vec<Bbox> = pool
        .edges
        .iter()
        .map(|e| edge_bbox(e, &pool.verts, run_tol))
        .collect();
    // Edges of each face, and faces of each edge.
    let mut face_edges: Vec<Vec<usize>> = vec![Vec::new(); faces.len()];
    for (fi, f) in faces.iter().enumerate() {
        for l in &f.loops {
            for (e, _, _) in l {
                if !face_edges[fi].contains(e) {
                    face_edges[fi].push(*e);
                }
            }
        }
    }
    prof.mark("import");
    // 2a. Vertices on edges of the other body (within the larger of the vertex's, the edge's
    // and the run's tolerances).
    let verts_of = |range: std::ops::Range<usize>, pool: &Pool| -> Vec<usize> {
        let mut seen = HashSet::default();
        let mut vs = Vec::new();
        for o in range {
            for v in pool.edges[o].v {
                if seen.insert(v) {
                    vs.push(v);
                }
            }
        }
        vs
    };
    let body_verts = [verts_of(0..na_edges, &pool), verts_of(na_edges..ne, &pool)];
    // Spatial indexes (candidates come back in the order the full scans used).
    let point_box = |p: &P3| {
        let mut b = Bbox::empty();
        b.add(p);
        b
    };
    let vtrees = [0, 1].map(|k| {
        super::bvh::Bvh::new(
            body_verts[k]
                .iter()
                .map(|&v| point_box(&pool.verts[v]))
                .collect(),
        )
    });
    let reach = pool
        .vtols
        .iter()
        .copied()
        .fold(run_tol, f64::max)
        .max(pool.edges.iter().map(|e| e.tol).fold(0.0, f64::max));
    for e in 0..ne {
        if pool.edges[e].degenerate {
            continue;
        }
        let ob = 1 - owner_of_edge(e);
        let mut near = vtrees[ob].query(&ebox[e].grow(reach));
        near.sort_unstable();
        let other: Vec<usize> = near.into_iter().map(|i| body_verts[ob][i]).collect();
        for &v in &other {
            let p = pool.verts[v];
            let ed = &pool.edges[e];
            let tol = run_tol.max(pool.vtols[v]).max(ed.tol);
            if !ebox[e].grow(tol).contains(&p) {
                continue;
            }
            if v == ed.v[0] || v == ed.v[1] {
                continue;
            }
            if let Some(t) = on_curve(&ed.curve, ed.t[0], ed.t[1], &p, tol) {
                pool.split(e, t, v);
            }
        }
    }
    // 2a'. Closed edges (whole circles) are split at every vertex lying on them, of either body:
    // coincident circles with their seams at different places then share their pieces.
    let all_verts: Vec<usize> = verts_of(0..ne, &pool);
    for e in 0..ne {
        let ed = pool.edges[e].clone();
        if ed.degenerate || ed.v[0] != ed.v[1] {
            continue;
        }
        for &v in &all_verts {
            if v == ed.v[0] {
                continue;
            }
            let p = pool.verts[v];
            let tol = run_tol.max(pool.vtols[v]).max(ed.tol);
            if !ebox[e].grow(tol).contains(&p) {
                continue;
            }
            if let Some(t) = on_curve(&ed.curve, ed.t[0], ed.t[1], &p, tol) {
                pool.split(e, t, v);
            }
        }
    }
    // 2b. Edge–edge intersections (A × B).
    let btree = super::bvh::Bvh::new(ebox[na_edges..ne].to_vec());
    for ea in 0..na_edges {
        if pool.edges[ea].degenerate {
            continue;
        }
        let mut near = btree.query(&ebox[ea]);
        near.sort_unstable();
        for eb in near.into_iter().map(|j| j + na_edges) {
            if pool.edges[eb].degenerate || !ebox[ea].overlaps(&ebox[eb]) {
                continue;
            }
            let (ca, cb) = (pool.edges[ea].clone(), pool.edges[eb].clone());
            let et = ca.tol.max(cb.tol);
            for (ta, tb) in ssi::curve_curve(
                &ca.curve,
                ca.t[0],
                ca.t[1],
                &cb.curve,
                cb.t[0],
                cb.t[1],
                run_tol.max(et) * 0.5,
            ) {
                let p = ca.curve.point(ta);
                let v = pool.vertex_tol(p, et);
                // The parameters of the (possibly merged) vertex itself.
                let q = pool.verts[v];
                let ta = refine_param(&ca.curve, ca.t[0], ca.t[1], &q, ta);
                let tb = refine_param(&cb.curve, cb.t[0], cb.t[1], &q, tb);
                pool.split(ea, ta, v);
                pool.split(eb, tb, v);
            }
        }
    }
    prof.mark("edge-edge");
    // 2c. Edge–face intersections.
    let mut face_pts: Vec<Vec<usize>> = vec![Vec::new(); faces.len()];
    // Edges of one body lying on a face of the other (imprinted as cuts once split).
    let mut on_face: Vec<Vec<usize>> = vec![Vec::new(); faces.len()];
    let ftree = super::bvh::Bvh::new(faces.iter().map(|f| f.bbox).collect());
    let bounds: Vec<Option<SurfaceBound>> =
        faces.iter().map(|f| SurfaceBound::of(&f.surf)).collect();
    // Per spline edge of many spans, each span's box (grown by the run's tolerance): made when
    // first wanted, the edge's own box over a face's.
    let span_boxes: Vec<std::sync::OnceLock<Option<Vec<Bbox>>>> =
        (0..ne).map(|_| std::sync::OnceLock::new()).collect();
    for e in 0..ne {
        if pool.edges[e].degenerate {
            continue;
        }
        let oe = owner_of_edge(e);
        let mut near = ftree.query(&ebox[e]);
        near.sort_unstable();
        for fi in near {
            if faces[fi].owner == oe || !ebox[e].overlaps(&faces[fi].bbox) {
                continue;
            }
            let ed = pool.edges[e].clone();
            let surf = &faces[fi].surf;
            if edge_on_bounded(&ed, surf, bounds[fi].as_ref(), faces[fi].tol, run_tol) {
                if !on_face[fi].contains(&e) {
                    on_face[fi].push(e);
                }
                continue;
            }
            // A spline edge whose spans' boxes (their control points' hulls) all keep off the
            // face's box meets it nowhere: no search along it (each a few hundred projections).
            if let Some(boxes) = span_boxes[e].get_or_init(|| span_boxes_of(&ed, run_tol))
                && !boxes.iter().any(|b| b.overlaps(&faces[fi].bbox))
            {
                continue;
            }
            for t in ssi::curve_surface(&ed.curve, ed.t[0], ed.t[1], &faces[fi].surf) {
                let p = ed.curve.point(t);
                let et = ed.tol.max(faces[fi].tol);
                if point_in_face(&faces[fi], &p, run_tol.max(et)) == InFace::Outside {
                    continue;
                }
                // A point of touch found again a hair from a vertex the edge already has there
                // (an edge resting on the surface touches it along a zone where the distance is
                // rounding noise): that vertex.
                let surf = &faces[fi].surf;
                let pe = &pool.edges[e];
                // (A touch, not a crossing: the edge keeps to one side of the surface either
                // side of the root.)
                let (lo, hi) = (pe.t[0].min(pe.t[1]), pe.t[0].max(pe.t[1]));
                let side = |x: f64| {
                    let q = ed.curve.point(x.clamp(lo, hi));
                    let (u, v) = surf.project(&q, None);
                    (q - surf.eval(u, v)).dot(&surf.normal(u, v))
                };
                let near_vertex = pe
                    .splits
                    .iter()
                    .copied()
                    .chain([(pe.t[0], pe.v[0]), (pe.t[1], pe.v[1])])
                    .find(|&(s, w)| {
                        let h = 0.25 * (s - t);
                        (pool.verts[w] - p).norm() <= et.max(run_tol) * 1e5
                            && side(t - h) * side(t + h) >= 0.0
                            && (0..=8).all(|k| {
                                surf.distance(&ed.curve.point(t + (s - t) * k as f64 / 8.0))
                                    <= et.max(run_tol)
                            })
                    });
                if let Some((_, w)) = near_vertex {
                    if !face_pts[fi].contains(&w) {
                        face_pts[fi].push(w);
                    }
                    continue;
                }
                let v = pool.vertex_tol(p, et);
                let t = refine_param(&ed.curve, ed.t[0], ed.t[1], &pool.verts[v], t);
                pool.split(e, t, v);
                if !face_pts[fi].contains(&v) {
                    face_pts[fi].push(v);
                }
            }
        }
    }
    // Vertices on each face's boundary (with the split points).
    let boundary_verts = |pool: &Pool, fi: usize| -> Vec<usize> {
        let mut vs = Vec::new();
        for &e in &face_edges[fi] {
            let ed = &pool.edges[e];
            for v in ed.v.iter().copied().chain(ed.splits.iter().map(|s| s.1)) {
                if !vs.contains(&v) {
                    vs.push(v);
                }
            }
        }
        vs
    };
    prof.mark("edge-face");
    // 3. Face–face.
    let mut cuts: Vec<Vec<usize>> = vec![Vec::new(); faces.len()];
    let mut coincident: Vec<Vec<usize>> = vec![Vec::new(); faces.len()];
    let mut new_edges: Vec<usize> = Vec::new();
    let uvbox = |f: &WFace| {
        let mut ub = (f64::INFINITY, f64::NEG_INFINITY);
        let mut vb = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &f.polys.0 {
            for q in p {
                ub = (ub.0.min(q.x), ub.1.max(q.x));
                vb = (vb.0.min(q.y), vb.1.max(q.y));
            }
        }
        (ub, vb)
    };
    let overlap = |fa: usize, fb: usize| {
        let mut ob = faces[fa].bbox;
        ob.min = ob.min.sup(&faces[fb].bbox.min);
        ob.max = ob.max.inf(&faces[fb].bbox.max);
        ob
    };
    // The pairs of faces to intersect, in a fixed order.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for fa in 0..na_faces {
        if !selected(faces[fa].orig) {
            continue;
        }
        let mut near = ftree.query(&faces[fa].bbox);
        near.sort_unstable();
        for fb in near.into_iter().filter(|&f| f >= na_faces) {
            if !faces[fa].bbox.overlaps(&faces[fb].bbox) {
                continue;
            }
            // Coincident within the faces' tolerances (near-coincident faces have had theirs
            // raised: `near_coincident_tolerances`).
            let ctol = run_tol.max(faces[fa].tol).max(faces[fb].tol);
            if faces[fa].surf.same_domain(&faces[fb].surf, ctol).is_some() {
                coincident[fa].push(fb);
                coincident[fb].push(fa);
                continue;
            }
            // Faces that share an edge and are tangent along it (one continues the other): the
            // shared edge is their contact; contouring tangent surfaces finds nothing more but
            // noise.
            if tangent_along_shared_edge(
                &pool,
                &faces[fa],
                &faces[fb],
                &face_edges[fa],
                &face_edges[fb],
                (bounds[fa].as_ref(), bounds[fb].as_ref()),
                &ebox,
            ) {
                continue;
            }
            if super::env_flag!("CADRS_TANGENT_DEBUG")
                && (matches!(faces[fa].surf, Surface::Nurbs(_))
                    || matches!(faces[fb].surf, Surface::Nurbs(_)))
            {
                let dev = |edges: &[usize], s: &Surface| -> f64 {
                    edges
                        .iter()
                        .map(|&x| {
                            let e = &pool.edges[x];
                            [0.0, 0.3, 0.5, 0.7, 1.0]
                                .iter()
                                .map(|f| s.distance(&e.curve.point(e.t[0] + (e.t[1] - e.t[0]) * f)))
                                .fold(0.0, f64::max)
                        })
                        .fold(f64::INFINITY, f64::min)
                };
                eprintln!(
                    "not tangent {fa}×{fb}: closest edge of b to a {:e}, of a to b {:e}",
                    dev(&face_edges[fb], &faces[fa].surf),
                    dev(&face_edges[fa], &faces[fb].surf)
                );
            }
            pairs.push((fa, fb));
        }
    }
    // Their intersections, each a function of the two faces alone: computed in parallel and
    // used below in the pairs' order, so the result doesn't depend on the scheduling.
    // (In chunks of pairs, the budget checked after each: a run past it stops within a chunk.)
    // (With the boundary of a region where the two surfaces coincide, when they do over part.)
    type Found = (
        Vec<ssi::IntCurve>,
        Vec<P3>,
        std::time::Duration,
        Vec<ssi::IntCurve>,
    );
    let mut ssis: Vec<Found> = Vec::with_capacity(pairs.len());
    PAIRS.fetch_add(pairs.len() as u64, std::sync::atomic::Ordering::Relaxed);
    for chunk in pairs.chunks(64) {
        let done: Vec<(Found, u64)> = chunk
            .par_iter()
            .map(|&(fa, fb)| {
                let t = web_time::Instant::now();
                // (The curves the surfaces only touch along are left out: the faces' tangent
                // contact is found from their boundaries, as for faces tangent along an edge.)
                let r = ssi::surface_surface_tol(
                    &faces[fa].surf,
                    uvbox(&faces[fa]),
                    &faces[fb].surf,
                    uvbox(&faces[fb]),
                    &overlap(fa, fb),
                    run_tol,
                );
                ((r.curves, r.singular, t.elapsed(), r.coincident), r.work)
            })
            .collect();
        work.spend(done.iter().map(|d| d.1).sum(), "surface intersection")?;
        ssis.extend(done.into_iter().map(|(f, _)| f));
    }
    // Pairs of faces whose surfaces coincide over part of the faces (from the intersections): their
    // sub-faces on the other face's surface classify as on it, as a coincident pair's do.
    let mut partial: HashSet<(usize, usize)> = HashSet::default();
    for (&(fa, fb), (mut curves, singular, el, shared)) in pairs.iter().zip(ssis) {
        // The shared region's boundary cuts both faces, as a crossing does.
        if !shared.is_empty() {
            partial.insert((fa, fb));
            partial.insert((fb, fa));
            coincident[fa].push(fb);
            coincident[fb].push(fa);
            curves.extend(shared);
        }
        let ob = overlap(fa, fb);
        // A boundary edge of one face lying on the other's surface, the faces crossing there (a
        // blend tool's strip ending on the face it blends, a wall through a tube's axis), is
        // imprinted exactly by the edge stages: the curve the intersection finds along it
        // again, inexactly, goes.
        let near = (run_tol * 10.0).max(1e-7 * ob.diagonal());
        let along = |ic: &ssi::IntCurve| {
            let pts: Vec<P3> = (0..=4)
                .map(|k| ic.c.point(ic.t0 + (ic.t1 - ic.t0) * k as f64 / 4.0))
                .collect();
            let runs_on = |x: usize| {
                let e = &pool.edges[x];
                !e.degenerate
                    && ebox[x].grow(near).contains(&pts[2])
                    && pts
                        .iter()
                        .all(|p| on_curve(&e.curve, e.t[0], e.t[1], p, near).is_some())
            };
            face_edges[fa]
                .iter()
                .any(|&x| runs_on(x) && edge_on_surface(&pool.edges[x], &faces[fb].surf, run_tol))
                || face_edges[fb].iter().any(|&x| {
                    runs_on(x) && edge_on_surface(&pool.edges[x], &faces[fa].surf, run_tol)
                })
        };
        let curves: Vec<ssi::IntCurve> = curves.into_iter().filter(|ic| !along(ic)).collect();
        // Whether a boundary edge of either face lies on the other's surface.
        let edge_on_other = {
            let lies = |edges: &[usize], g: &WFace, bound: Option<&SurfaceBound>| {
                edges.iter().any(|&x| {
                    let e = &pool.edges[x];
                    !e.degenerate
                        && e.curve
                            .longer_than(e.t[0].min(e.t[1]), e.t[0].max(e.t[1]), 1e-3)
                        && edge_on_bounded(e, &g.surf, bound, 0.0, pool.tol)
                })
            };
            lies(&face_edges[fa], &faces[fb], bounds[fb].as_ref())
                || lies(&face_edges[fb], &faces[fa], bounds[fa].as_ref())
        };
        // A curve running within the tolerance of another all along (branches of a near
        // tangent contact traced twice) is that curve again: the copy goes, else every point
        // where the two come within the tolerance would cut both.
        let curves: Vec<ssi::IntCurve> = {
            let near_c = (run_tol * 10.0).max(faces[fa].tol).max(faces[fb].tol);
            let mut kept: Vec<ssi::IntCurve> = Vec::new();
            for ic in curves {
                let bb = curve_bbox(&ic.c, ic.t0, ic.t1);
                let copy = kept.iter().any(|k| {
                    curve_bbox(&k.c, k.t0, k.t1).grow(near_c).overlaps(&bb)
                        && (0..=8).all(|q| {
                            on_curve(
                                &k.c,
                                k.t0.min(k.t1),
                                k.t0.max(k.t1),
                                &ic.c.point(ic.t0 + (ic.t1 - ic.t0) * q as f64 / 8.0),
                                near_c,
                            )
                            .is_some()
                        })
                });
                if !copy {
                    kept.push(ic);
                }
            }
            kept
        };
        if super::env_flag!("CADRS_BOOL_DEBUG") {
            eprintln!(
                "ssi {fa}×{fb} {:?}×{:?}: {} curves in {el:?}",
                faces[fa].surf.kind(),
                faces[fb].surf.kind(),
                curves.len()
            );
            for ic in &curves {
                eprintln!(
                    "    curve closed {} from {:?} to {:?}",
                    ic.closed,
                    ic.c.point(ic.t0),
                    ic.c.point(ic.t1)
                );
            }
        }
        if curves.is_empty() {
            continue;
        }
        let mut cand: Vec<usize> = boundary_verts(&pool, fa);
        for v in boundary_verts(&pool, fb)
            .into_iter()
            .chain(face_pts[fa].iter().copied())
            .chain(face_pts[fb].iter().copied())
        {
            if !cand.contains(&v) {
                cand.push(v);
            }
        }
        // The pair's tolerance: the larger of the faces' (new vertices and edges carry it).
        let ft = faces[fa].tol.max(faces[fb].tol);
        let tf = run_tol.max(ft);
        // Where two curves of the pair cross or touch (a branch point: the surfaces are
        // tangent there) the faces' new edges meet, so both need a vertex: the SSI's own
        // such points (exact, each curve passing one runs through it), and crossings found
        // between the curves.
        for p in &singular {
            if ob.grow(tf * 10.0).contains(p) {
                let v = pool.vertex_tol(*p, ft);
                if !cand.contains(&v) {
                    cand.push(v);
                }
            }
        }
        // (Curves whose boxes are apart can't meet.)
        let cboxes: Vec<Bbox> = curves
            .iter()
            .map(|ic| curve_bbox(&ic.c, ic.t0, ic.t1).grow(tf))
            .collect();
        for i in 0..curves.len() {
            for j in i + 1..curves.len() {
                if !cboxes[i].overlaps(&cboxes[j]) {
                    continue;
                }
                let (ci, cj) = (&curves[i], &curves[j]);
                for t in overlap_runs(
                    &ci.c,
                    &cj.c,
                    ssi::curve_curve(&ci.c, ci.t0, ci.t1, &cj.c, cj.t0, cj.t1, tf),
                    tf,
                ) {
                    let p = ci.c.point(t);
                    if ob.grow(tf * 10.0).contains(&p) {
                        let v = pool.vertex_tol(p, ft);
                        if !cand.contains(&v) {
                            cand.push(v);
                        }
                    }
                }
            }
        }
        for ic in curves {
            let mut ps: Vec<(f64, usize)> = Vec::new();
            for &v in &cand {
                let p = pool.verts[v];
                if !ob.grow(tf * 10.0).contains(&p) {
                    continue;
                }
                let mut t = param_on(&ic.c, ic.t0, ic.t1, &p);
                let on = (run_tol * 2.0).max(ft);
                if (ic.c.point(t) - p).norm() < on {
                    // A closed curve: the vertex at its start (within the tolerance, though
                    // its parameter may land a hair off where the curve runs along it) is
                    // also at its end.
                    if ic.closed && (ic.c.point(ic.t0) - p).norm() < on {
                        t = if (t - ic.t0).abs() <= (t - ic.t1).abs() {
                            ic.t0
                        } else {
                            ic.t1
                        };
                    }
                    ps.push((t, v));
                    if ic.closed
                        && ((t - ic.t0).abs() < tol::ROUNDOFF_FINE
                            || (t - ic.t1).abs() < tol::ROUNDOFF_FINE)
                    {
                        ps.push((
                            if (t - ic.t0).abs() < tol::ROUNDOFF_FINE {
                                ic.t1
                            } else {
                                ic.t0
                            },
                            v,
                        ));
                    }
                }
            }
            // An open curve left with no end to run to (fewer than two vertices on it): a
            // vertex within 1.5 times its own tolerance of it, and within that of both
            // surfaces, ends it too. The edge-face stage makes such a vertex where an edge a
            // tolerance off its face meets a surface it grazes: the crossing lands up to the
            // tolerance over the angle along the curve (a slab's rim 7e-4 off its cylinder
            // meeting a plane at a glancing angle, 1.6e-3 off the plane's line on it). Its
            // tolerance then covers the gap.
            if !ic.closed && ps.len() < 2 {
                for &v in &cand {
                    let p = pool.verts[v];
                    let vt = pool.vtols[v];
                    if ps.iter().any(|&(_, w)| w == v) || !ob.grow(tf * 10.0 + vt).contains(&p) {
                        continue;
                    }
                    let t = param_on(&ic.c, ic.t0, ic.t1, &p);
                    let gap = (ic.c.point(t) - p).norm();
                    // (Only where the tolerance is what the gap comes of, a tenth of it or more: a
                    // vertex a hair off a curve with no ends is no crossing of it.)
                    if gap < 1.5 * vt
                        && gap > 0.1 * vt
                        && faces[fa].surf.distance(&p) <= vt
                        && faces[fb].surf.distance(&p) <= vt
                    {
                        if gap > vt {
                            pool.vtols[v] = 1.5 * gap;
                        }
                        ps.push((t, v));
                    }
                }
            }
            // Sections only (booleans rely on the edge–face stage's crossings): where the curve
            // leaves either face (a crossing that stage missed, as on a B-spline face),
            // found by sampling the in-face status along it and bisecting each change, and
            // the open curve's own ends, cut it too.
            let sectioning = matches!(mode, Mode::Section);
            if sectioning {
                let inside = |t: f64| {
                    let p = ic.c.point(t);
                    point_in_face(&faces[fa], &p, tf) != InFace::Outside
                        && point_in_face(&faces[fb], &p, tf) != InFace::Outside
                };
                let n = 64;
                let span = ic.t1 - ic.t0;
                let mut prev = inside(ic.t0);
                for k in 1..=n {
                    let t = ic.t0 + span * k as f64 / n as f64;
                    let cur = inside(t);
                    if cur != prev {
                        let (mut lo, mut hi) = (t - span / n as f64, t);
                        for _ in 0..50 {
                            let m = 0.5 * (lo + hi);
                            if inside(m) == prev {
                                lo = m;
                            } else {
                                hi = m;
                            }
                        }
                        let tc = 0.5 * (lo + hi);
                        let pc = ic.c.point(tc);
                        if !ps.iter().any(|(pt, v)| {
                            (pt - tc).abs() < tol::REL * (1.0 + span.abs())
                                || (pool.verts[*v] - pc).norm() < tf * 10.0
                        }) {
                            let v = pool.vertex_tol(pc, ft);
                            ps.push((tc, v));
                        }
                    }
                    prev = cur;
                }
            }
            if sectioning && !ic.closed {
                for t in [ic.t0, ic.t1] {
                    if !ps
                        .iter()
                        .any(|(pt, _)| (pt - t).abs() < tol::REL_FINE * (1.0 + t.abs()))
                    {
                        let v = pool.vertex_tol(ic.c.point(t), ft);
                        ps.push((t, v));
                    }
                }
            }
            ps.sort_by(|a, b| a.0.total_cmp(&b.0));
            ps.dedup_by(|a, b| (a.0 - b.0).abs() < tol::ROUNDOFF_FINE && a.1 == b.1);
            // Intervals.
            let mut ivs: Vec<(f64, f64, Option<usize>, Option<usize>)> = Vec::new();
            if ic.closed {
                if ps.is_empty() {
                    ivs.push((ic.t0, ic.t1, None, None));
                } else {
                    for k in 0..ps.len() {
                        let (ta, va) = ps[k];
                        let (tb, vb) = if k + 1 < ps.len() {
                            ps[k + 1]
                        } else {
                            (ps[0].0 + (ic.t1 - ic.t0), ps[0].1)
                        };
                        if tb - ta > 1e-10 {
                            ivs.push((ta, tb, Some(va), Some(vb)));
                        }
                    }
                }
            } else {
                for w in ps.windows(2) {
                    if w[1].0 - w[0].0 > 1e-10 {
                        ivs.push((w[0].0, w[1].0, Some(w[0].1), Some(w[1].1)));
                    }
                }
            }
            // A closed curve that isn't periodic (a fitted B-spline) can't run past its end:
            // the interval that wraps becomes two pieces meeting at the curve's start.
            if ic.closed && !ic.c.is_periodic() {
                let period = ic.t1 - ic.t0;
                let mut split: Vec<(f64, f64, Option<usize>, Option<usize>)> = Vec::new();
                for (ta, tb, va, vb) in ivs {
                    if tb > ic.t1 + tol::ROUNDOFF_FINE && ta < ic.t1 - tol::ROUNDOFF_FINE {
                        let vend = pool.vertex_tol(ic.c.point(ic.t1), ft);
                        split.push((ta, ic.t1, va, Some(vend)));
                        if tb - period > ic.t0 + tol::ROUNDOFF_FINE {
                            split.push((ic.t0, tb - period, Some(vend), vb));
                        }
                    } else if ta >= ic.t1 - tol::ROUNDOFF_FINE {
                        split.push((ta - period, tb - period, va, vb));
                    } else {
                        split.push((ta, tb, va, vb));
                    }
                }
                ivs = split;
            }
            for (ta, tb, va, vb) in ivs {
                // Wrap periodic parameters past the end.
                let tm = (ta + tb) * 0.5;
                let pm = ic.c.point(tm);
                let ia = point_in_face(&faces[fa], &pm, tf);
                let ib = point_in_face(&faces[fb], &pm, tf);
                if super::env_flag!("CADRS_BOOL_DEBUG") {
                    eprintln!("  interval {ta}..{tb} at {pm:?}: in a {ia:?}, in b {ib:?}");
                }
                if ia == InFace::Outside || ib == InFace::Outside {
                    continue;
                }
                // A piece no longer than the tolerance is a point (two vertices of the inputs
                // closer than it, both on the curve): no edge.
                if (ic.c.point(ta) - ic.c.point(tb)).norm() <= tf
                    && ic.c.length(ta.min(tb), ta.max(tb)) <= tf
                {
                    continue;
                }
                // Where the faces touch along an edge of one (a blend's spring edge on a wall it
                // is tangent to), the curve is that edge found again: the edge is already there,
                // and the curve would cut the faces at every vertex along it.
                let along_edge = |f: usize| {
                    [0.25, 0.75].iter().all(|k| {
                        point_in_face(&faces[f], &ic.c.point(ta + (tb - ta) * k), tf)
                            == InFace::Boundary
                    })
                };
                let tangent_at = |p: &P3| {
                    let (sa, sb) = (&faces[fa].surf, &faces[fb].surf);
                    let ((ua, va_), (ub, vb_)) = (sa.project(p, None), sb.project(p, None));
                    sa.normal(ua, va_).cross(&sb.normal(ub, vb_)).norm() < 1e-3
                };
                // (Also where they cross along it, the edge lying on the other surface: the edge
                // stages imprint it.)
                if ((ia == InFace::Boundary && along_edge(fa))
                    || (ib == InFace::Boundary && along_edge(fb)))
                    && (edge_on_other || tangent_at(&pm))
                {
                    continue;
                }
                // Where a traced curve (of surfaces with no closed-form intersection) runs along
                // an edge of one face that lies on the other surface within the tolerance all
                // along (two swept faces meeting at a seam of one, the curve traced 1e-7 off it),
                // it is that edge again: the edge–face stage imprints the edge, and the copy would
                // cut the faces along it. (An exact curve near an edge is other geometry.)
                let elementary = |s: &Surface| {
                    matches!(
                        s,
                        Surface::Plane { .. }
                            | Surface::Cylinder { .. }
                            | Surface::Cone { .. }
                            | Surface::Sphere { .. }
                            | Surface::Torus { .. }
                    )
                };
                // (A section reports the stretch: only an edge lying on the other surface all
                // along is imprinted there.)
                let traced = !(elementary(&faces[fa].surf) && elementary(&faces[fb].surf))
                    && !matches!(mode, Mode::Section);
                if traced
                    && ((ia == InFace::Boundary
                        && along_edge(fa)
                        && duplicates_edge(
                            &pool,
                            &face_edges[fa],
                            &faces[fb],
                            &ic.c,
                            ta,
                            tb,
                            tf,
                            run_tol,
                        ))
                        || (ib == InFace::Boundary
                            && along_edge(fb)
                            && duplicates_edge(
                                &pool,
                                &face_edges[fb],
                                &faces[fa],
                                &ic.c,
                                ta,
                                tb,
                                tf,
                                run_tol,
                            )))
                {
                    continue;
                }
                let va = va.unwrap_or_else(|| pool.vertex_tol(ic.c.point(ta), ft));
                let vb = vb.unwrap_or(va);
                // Reuse an existing edge running there.
                let existing = (0..pool.edges.len()).find(|&e| {
                    let ed = &pool.edges[e];
                    if ed.degenerate || !ebox.get(e).is_none_or(|b| b.contains(&pm)) {
                        return false;
                    }
                    // Only an unsplit edge between exactly these vertices (pieces of split
                    // edges that coincide with the new edge are unified later).
                    let ends_ok = (ed.v == [va, vb] || ed.v == [vb, va]) && ed.splits.is_empty();
                    ends_ok && on_curve(&ed.curve, ed.t[0], ed.t[1], &pm, tf.max(ed.tol)).is_some()
                });
                let e = match existing {
                    Some(e) => e,
                    None => {
                        // Unwrap the parameter range for the curve (closed curves may run past
                        // t1).
                        let (ta, tb) = (ta, tb);
                        pool.edges.push(WEdge {
                            curve: ic.c.clone(),
                            t: [ta, tb],
                            v: [va, vb],
                            degenerate: false,
                            tol: ft,
                            splits: Vec::new(),
                            pcs: ic.pcs.clone(),
                        });
                        new_edges.push(pool.edges.len() - 1);
                        pool.edges.len() - 1
                    }
                };
                for f in [fa, fb] {
                    if !face_edges[f].contains(&e) && !cuts[f].contains(&e) {
                        cuts[f].push(e);
                    }
                }
            }
        }
    }
    prof.mark("face-face");
    if matches!(mode, Mode::Section) {
        // The new intersection edges, and edges of one body lying on a face of the other
        // (their parts inside that face).
        let mut out = Body::default();
        let mut vmap: HashMap<usize, usize> = HashMap::default();
        let mut add = |out: &mut Body, curve: &Curve, t: [f64; 2], v: [usize; 2]| {
            let vs = v.map(|x| {
                *vmap
                    .entry(x)
                    .or_insert_with(|| out.add_vertex(pool.verts[x]))
            });
            out.add_edge(curve.clone(), t[0], t[1], vs[0], vs[1]);
        };
        for (fi, list) in on_face.iter().enumerate() {
            for &e in list {
                let ed = pool.edges[e].clone();
                // The edge's pieces between its split points, kept where inside the face.
                let mut ts: Vec<(f64, usize)> = vec![(ed.t[0], ed.v[0])];
                let mut sp = ed.splits.clone();
                sp.sort_by(|a, b| a.0.total_cmp(&b.0));
                ts.extend(sp);
                ts.push((ed.t[1], ed.v[1]));
                for w in ts.windows(2) {
                    let pm = ed.curve.point(0.5 * (w[0].0 + w[1].0));
                    if point_in_face(&faces[fi], &pm, run_tol) != InFace::Outside {
                        add(&mut out, &ed.curve, [w[0].0, w[1].0], [w[0].1, w[1].1]);
                    }
                }
            }
        }
        // An intersection curve running along one of those edges (a plane touching a torus
        // along the circle bounding it) is that edge, found again with ends that the tangency
        // leaves ill-defined: kept once, as the edge.
        let kept = out.edges.len();
        for &e in &new_edges {
            let ed = &pool.edges[e];
            let along = (0..kept).any(|o| {
                let oe = &out.edges[o];
                [0.25, 0.5, 0.75].iter().all(|&f| {
                    let p = ed.curve.point(ed.t[0] + (ed.t[1] - ed.t[0]) * f);
                    (oe.point(oe.curve.closest(&p, oe.t[0], oe.t[1])) - p).norm() < run_tol * 10.0
                })
            });
            if !along {
                add(&mut out, &ed.curve, ed.t, ed.v);
            }
        }
        if super::env_flag!("CADRS_BOOL_DEBUG") {
            for (i, e) in out.edges.iter().enumerate() {
                eprintln!(
                    "section edge {i}: v {:?} t {:?} {:?} -> {:?} mid {:?}",
                    e.v,
                    e.t,
                    e.point(e.t[0]),
                    e.point(e.t[1]),
                    e.point(0.5 * (e.t[0] + e.t[1]))
                );
            }
        }
        return Ok(BoolOut {
            body: out,
            from: Vec::new(),
        });
    }
    // Pieces of the split edges.
    let mut pieces: Vec<Vec<usize>> = Vec::with_capacity(pool.edges.len());
    let n_all = pool.edges.len();
    for e in 0..n_all {
        let mut sp = pool.edges[e].splits.clone();
        if sp.is_empty() {
            pieces.push(vec![e]);
            continue;
        }
        sp.sort_by(|a, b| a.0.total_cmp(&b.0));
        let ed = pool.edges[e].clone();
        let mut bounds = vec![(ed.t[0], ed.v[0])];
        bounds.extend(sp);
        bounds.push((ed.t[1], ed.v[1]));
        let mut ps = Vec::new();
        for w in bounds.windows(2) {
            pool.edges.push(WEdge {
                curve: ed.curve.clone(),
                t: [w[0].0, w[1].0],
                v: [w[0].1, w[1].1],
                degenerate: false,
                tol: ed.tol,
                splits: Vec::new(),
                pcs: ed.pcs.clone(),
            });
            ps.push(pool.edges.len() - 1);
        }
        pieces.push(ps);
    }
    // Coincident pieces (the same curve between the same vertices) become one edge.
    let np = pool.edges.len();
    let mut canon: Vec<usize> = (0..np).collect();
    {
        let mut groups: HashMap<(usize, usize), Vec<usize>> = HashMap::default();
        for ps in &pieces {
            for &p in ps {
                let ed = &pool.edges[p];
                if ed.degenerate {
                    continue;
                }
                let key = (ed.v[0].min(ed.v[1]), ed.v[0].max(ed.v[1]));
                groups.entry(key).or_default().push(p);
            }
        }
        for (_, g) in groups {
            for i in 0..g.len() {
                if canon[g[i]] != g[i] {
                    continue;
                }
                for j in i + 1..g.len() {
                    if canon[g[j]] != g[j] {
                        continue;
                    }
                    let (p, q) = (&pool.edges[g[i]], &pool.edges[g[j]]);
                    // (Within the pieces' own tolerances too: edges a fuzzy gap apart are one.)
                    let within = (run_tol * 10.0).max(p.tol.min(q.tol));
                    let ok = [0.25, 0.5, 0.75].iter().all(|f| {
                        let m = q.curve.point(q.t[0] + (q.t[1] - q.t[0]) * f);
                        on_curve(&p.curve, p.t[0], p.t[1], &m, within).is_some()
                    });
                    if ok {
                        canon[g[j]] = g[i];
                    }
                }
            }
        }
    }
    // Edges lying on a face (not on its boundary) imprint it.
    for fi in 0..faces.len() {
        for &e in &on_face[fi].clone() {
            for &pe in &pieces[e] {
                let ed = &pool.edges[pe];
                if ed.degenerate {
                    continue;
                }
                let pm = ed.curve.point((ed.t[0] + ed.t[1]) * 0.5);
                if point_in_face(&faces[fi], &pm, run_tol) == InFace::Inside
                    && !cuts[fi].contains(&pe)
                {
                    cuts[fi].push(pe);
                }
            }
        }
    }
    // Coincident faces imprint each other's boundary pieces.
    if super::env_flag!("CADRS_BOOL_DEBUG") {
        for (i, c) in coincident.iter().enumerate() {
            if !c.is_empty() {
                eprintln!("coincident {i}: {c:?}");
            }
        }
    }
    for fa in 0..faces.len() {
        for &fb in &coincident[fa].clone() {
            for &e in &face_edges[fb].clone() {
                for &pe in &pieces[e] {
                    let ed = &pool.edges[pe];
                    // (Of a pair coinciding over part, only the pieces lying on fa's surface.)
                    if ed.degenerate
                        || (partial.contains(&(fa, fb))
                            && !edge_on_surface(ed, &faces[fa].surf, run_tol * 10.0))
                    {
                        continue;
                    }
                    let pm = ed.curve.point((ed.t[0] + ed.t[1]) * 0.5);
                    if point_in_face(&faces[fa], &pm, run_tol) == InFace::Inside
                        && !cuts[fa].contains(&pe)
                    {
                        cuts[fa].push(pe);
                    }
                }
            }
        }
    }
    // 4. Split faces.
    // An arc on its canonical edge (re-parametrized onto that edge's curve if aliased).
    let canon_arc = |pool: &Pool, f: &WFace, a: Arc| -> Arc {
        let c = canon[a.edge];
        if c == a.edge {
            return a;
        }
        let ce = &pool.edges[c];
        let near = a.uv_at(0.5);
        let fit = ce
            .pcs
            .as_ref()
            .and_then(|p| ssi::pcurve_from(p, &f.surf, &ce.curve, ce.t[0], ce.t[1]));
        let pc = pcurve::pcurve_or(&f.surf, &ce.curve, ce.t[0], ce.t[1], Some(near), fit);
        // On a B-spline surface closing on itself within a tolerance only (a ring glued at
        // its seam), the projection may land on the seam's other side: the copy a whole range
        // over, where the arc was, when it fits the curve as well.
        let pc = seam_side(&f.surf, &ce.curve, ce.t, pc, near);
        // Same direction when the canonical edge runs from the arc's start vertex (or, closed,
        // when the tangents agree).
        let ae = &pool.edges[a.edge];
        let same = if ce.v[0] != ce.v[1] {
            ce.v[0] == ae.v[0]
        } else {
            let tm = (ae.t[0] + ae.t[1]) * 0.5;
            let p = ae.curve.point(tm);
            let tc = ce.curve.closest(&p, ce.t[0], ce.t[1]);
            ce.curve.tangent(tc).dot(&ae.curve.tangent(tm)) > 0.0
        };
        let fwd = if same { a.fwd } else { !a.fwd };
        let v = if fwd {
            [ce.v[0], ce.v[1]]
        } else {
            [ce.v[1], ce.v[0]]
        };
        Arc {
            edge: c,
            fwd,
            pc,
            t: ce.t,
            v,
            cut: a.cut,
            deg: ce.degenerate,
        }
    };
    let arcs_of = |pool: &Pool, f: &WFace, e: usize, fwd: bool, pc: &Curve| -> Vec<Arc> {
        let ps: Vec<usize> = if e < pieces.len() {
            pieces[e].clone()
        } else {
            vec![e]
        };
        let mut out: Vec<Arc> = ps
            .iter()
            .map(|&p| {
                let ed = &pool.edges[p];
                let v = if fwd {
                    [ed.v[0], ed.v[1]]
                } else {
                    [ed.v[1], ed.v[0]]
                };
                canon_arc(
                    pool,
                    f,
                    Arc {
                        edge: p,
                        fwd,
                        pc: pc.clone(),
                        t: ed.t,
                        v,
                        cut: false,
                        deg: ed.degenerate,
                    },
                )
            })
            .collect();
        if !fwd {
            out.reverse();
        }
        out
    };
    let mut subs: Vec<(usize, Vec<Vec<Arc>>)> = Vec::new(); // (face, loops)
    // The input faces no cut runs across (by owner and face): kept whole, if at all.
    let whole: HashSet<(usize, usize)> = faces
        .iter()
        .enumerate()
        .filter(|(fi, _)| cuts[*fi].is_empty())
        .map(|(_, f)| (f.owner, f.orig))
        .collect();
    for (fi, f) in faces.iter().enumerate() {
        let boundary: Vec<Vec<Arc>> = f
            .loops
            .iter()
            .map(|l| {
                l.iter()
                    .flat_map(|(e, fwd, pc)| arcs_of(&pool, f, *e, *fwd, pc))
                    .collect()
            })
            .collect();
        if cuts[fi].is_empty() {
            subs.push((fi, boundary));
            continue;
        }
        let own: Vec<usize> = boundary.iter().flatten().map(|a| a.edge).collect();
        let mut cut_arcs: Vec<Arc> = Vec::new();
        for &ce in &cuts[fi] {
            let cps: Vec<usize> = if ce < pieces.len() {
                pieces[ce].clone()
            } else {
                vec![ce]
            };
            for p in cps {
                let c = canon[p];
                if own.contains(&c) || cut_arcs.iter().any(|a| a.edge == c) {
                    continue;
                }
                let ed = &pool.edges[c];
                // A traced curve's own p-curve on this face's surface where it has one (no
                // projection), else the curve projected.
                let fit = ed
                    .pcs
                    .as_ref()
                    .and_then(|p| ssi::pcurve_from(p, &f.surf, &ed.curve, ed.t[0], ed.t[1]));
                let pc =
                    pcurve::pcurve_or(&f.surf, &ed.curve, ed.t[0], ed.t[1], Some(f.uv_center), fit);
                if super::env_flag!("CADRS_BOOL_DEBUG") {
                    for (k, t) in ed.t.iter().enumerate() {
                        let q = pc.point(*t);
                        let d = (f.surf.eval(q.x, q.y) - pool.verts[ed.v[k]]).norm();
                        let dc = (ed.curve.point(*t) - pool.verts[ed.v[k]]).norm();
                        if d > tol::POINT || dc > tol::POINT {
                            eprintln!(
                                "cut {c} end {k}: p-curve off by {d:e}, curve off by {dc:e}; {:?} t {:?} surf {:?}",
                                ed.curve, ed.t, f.surf
                            );
                        }
                    }
                }
                cut_arcs.push(Arc {
                    edge: c,
                    fwd: true,
                    pc,
                    t: ed.t,
                    v: ed.v,
                    cut: true,
                    deg: false,
                });
            }
        }
        if cut_arcs.is_empty() {
            subs.push((fi, boundary));
            continue;
        }
        let arcs = (boundary.iter().map(|l| l.len()).sum::<usize>() + cut_arcs.len()) as u64;
        work.spend(arcs * arcs, "face splitting")?;
        let res = split::split_face(&f.surf, !f.sense, &boundary, &cut_arcs);
        if super::env_flag!("CADRS_BOOL_DEBUG") {
            eprintln!(
                "face {fi} (owner {} orig {}): {} cuts -> {} faces {:?}",
                f.owner,
                f.orig,
                cut_arcs.len(),
                res.faces.len(),
                res.faces.iter().map(|l| l.len()).collect::<Vec<_>>()
            );
        }
        for lps in res.faces {
            subs.push((fi, lps));
        }
    }
    prof.mark("split");
    // 5. Classify.
    let kept = |x: &Body| -> Vec<classify::KeptPolygons> {
        x.faces
            .iter()
            .map(|f| classify::face_polygons_kept(x, f))
            .collect()
    };
    let (polys_a, polys_b) = (kept(a), kept(b));
    // (Each body prepared once for the many points classified against it.)
    let (cls_a, cls_b) = (
        classify::Classifier::with_kept(a, &polys_a),
        classify::Classifier::with_kept(b, &polys_b),
    );
    let mut keep: Vec<(usize, Vec<Vec<Arc>>, bool)> = Vec::new(); // (face, loops, flip)
    // Sub-faces of one body joined across an edge piece no cut runs along lie on one side of
    // the other body: grouped (union-find over shared uncut pieces), each group is classified
    // once and its answer shared (faces on a coincident face keep their own test).
    let mut parent: Vec<usize> = (0..subs.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    {
        // Pieces touching the other body (cuts of any face, edges lying on its faces, edges
        // merged with its edges) can separate sides: they don't join.
        let mut touching: HashSet<usize> = HashSet::default();
        for list in cuts.iter().chain(on_face.iter()) {
            for &e in list {
                touching.insert(e);
                touching.insert(canon[e.min(canon.len() - 1)]);
                if e < pieces.len() {
                    for &p in &pieces[e] {
                        touching.insert(p);
                        touching.insert(canon[p]);
                    }
                }
            }
        }
        for (e, &c) in canon.iter().enumerate() {
            if c != e {
                touching.insert(c);
                touching.insert(e);
            }
        }
        // Nor pieces ending where a cut ends (the other body meets this one there; within a
        // tolerance a crossing seen on one face can be missed on its neighbour).
        let cut_ends: HashSet<usize> = subs
            .iter()
            .flat_map(|(_, l)| l.iter().flatten())
            .filter(|a| a.cut)
            .flat_map(|a| a.v)
            .collect();
        let mut by_piece: HashMap<(usize, i64, i64, usize), usize> = HashMap::default();
        for (k, (fi, loops)) in subs.iter().enumerate() {
            for a in loops.iter().flatten().filter(|a| {
                !a.cut
                    && !a.deg
                    && !touching.contains(&a.edge)
                    && !a.v.iter().any(|v| cut_ends.contains(v))
            }) {
                let (lo, hi) = (a.t[0].min(a.t[1]), a.t[0].max(a.t[1]));
                let key = (
                    a.edge,
                    (lo * 1e9).round() as i64,
                    (hi * 1e9).round() as i64,
                    faces[*fi].owner,
                );
                match by_piece.get(&key) {
                    Some(&other) => {
                        let (x, y) = (root(&mut parent, k), root(&mut parent, other));
                        if x != y {
                            parent[x] = y;
                        }
                    }
                    None => {
                        by_piece.insert(key, k);
                    }
                }
            }
        }
    }
    let groups: Vec<usize> = (0..subs.len()).map(|k| root(&mut parent, k)).collect();
    // A group's class: (class, confirmed by a second member). A disagreement makes the group
    // classify every member itself.
    let mut group_class: HashMap<usize, (Class, bool)> = HashMap::default();
    let mut unreliable: HashSet<usize> = HashSet::default();
    let propagate = !super::env_flag!("CADRS_DEBUG_NO_PROPAGATE");
    work.spend(
        subs.len() as u64 * (a.faces.len() + b.faces.len()) as u64,
        "classification",
    )?;
    for (sk, (fi, loops)) in subs.into_iter().enumerate() {
        let f = &faces[fi];
        if imprinting {
            if f.owner == 0 {
                keep.push((fi, loops, false));
            }
            continue;
        }
        let g = groups[sk];
        let settled = match group_class.get(&g) {
            Some(&(c, true)) if propagate && !unreliable.contains(&g) => Some(c),
            _ => None,
        };
        // A sub-face on no face of the other body, whose group's class is settled, takes it: no
        // point inside it to find (most of a large part's faces, untouched); but one with no
        // inside at all is dropped, as ever.
        let at = if settled.is_some() && coincident[fi].is_empty() {
            if !has_interior(&loops) {
                continue;
            }
            None
        } else {
            let Some(uv) = interior_point(&loops) else {
                continue;
            };
            let n_out = f.surf.normal(uv.x, uv.y) * if f.sense { 1.0 } else { -1.0 };
            Some((f.surf.eval(uv.x, uv.y), n_out))
        };
        let mut class = None;
        // (On faces of the other body: the sub-face found its point above.)
        if let Some((p, n_out)) = at {
            for &g in &coincident[fi] {
                let gf = &faces[g];
                // (A face coinciding with g over part: where this sub-face lies on g's surface.)
                let on_g = !partial.contains(&(fi, g))
                    || gf.surf.distance(&p) <= (run_tol * 10.0).max(f.tol).max(gf.tol);
                if on_g && point_in_face(gf, &p, run_tol * 10.0) != InFace::Outside {
                    let (gu, gv) = gf.surf.project(&p, Some((gf.uv_center.x, gf.uv_center.y)));
                    let gn = gf.surf.normal(gu, gv) * if gf.sense { 1.0 } else { -1.0 };
                    // Against a sheet an overlap is an overlap, whichever way the faces face.
                    let other_sheet = if f.owner == 0 {
                        b.solids.is_empty()
                    } else {
                        a.solids.is_empty()
                    };
                    class = Some(if gn.dot(&n_out) > 0.0 || other_sheet {
                        Class::OnSame
                    } else {
                        Class::OnOpp
                    });
                    break;
                }
            }
        }
        let shared = settled.filter(|_| class.is_none());
        let fresh = class.is_none() && shared.is_none();
        let class = class.or(shared).unwrap_or_else(|| {
            // (Its point found above: only a settled group's sub-faces go without.)
            let (p, n_out) = at.expect("a sub-face classified itself has its point");
            let other = if f.owner == 0 { &cls_b } else { &cls_a };
            // A point on the other body's boundary (a sub-face whose sample landed on an edge of
            // the other body lying in it) decides nothing: try the face's other inside points.
            let mut pc = other.classify(&p, run_tol);
            if pc == PointClass::OnBoundary {
                for q in interior_points(&loops).into_iter().skip(1).take(12) {
                    let c = other.classify(&f.surf.eval(q.x, q.y), run_tol);
                    if c != PointClass::OnBoundary {
                        pc = c;
                        break;
                    }
                }
            }
            match pc {
                PointClass::Inside => Class::In,
                PointClass::Outside => Class::Out,
                PointClass::OnBoundary => {
                    // On the other body's boundary but not on a coincident face: decide by a
                    // point nudged inwards along the face.
                    let q = p - n_out * 1e-5;
                    if other.parity_inside(&q) {
                        Class::In
                    } else {
                        Class::Out
                    }
                }
            }
        });
        if fresh && matches!(class, Class::In | Class::Out) {
            match group_class.get(&g).copied() {
                None => {
                    group_class.insert(g, (class, false));
                }
                Some((c, false)) => {
                    if c == class {
                        group_class.insert(g, (c, true));
                    } else {
                        unreliable.insert(g);
                    }
                }
                Some((c, true)) => {
                    if c != class {
                        unreliable.insert(g);
                    }
                }
            }
        }
        let a_side = f.owner == 0;
        if super::env_flag!("CADRS_BOOL_DEBUG") {
            eprintln!(
                "sub of face {fi} (owner {} orig {}): {} loops, class {class:?} at {:?}{}",
                f.owner,
                f.orig,
                loops.len(),
                at.map(|(p, _)| p),
                if shared.is_some() { " (shared)" } else { "" }
            );
        }
        let (k, flip) = match (op, a_side, class) {
            (BoolOp::Union, _, Class::Out) => (true, false),
            (BoolOp::Union, true, Class::OnSame) => (true, false),
            (BoolOp::Intersect, _, Class::In) => (true, false),
            (BoolOp::Intersect, true, Class::OnSame) => (true, false),
            (BoolOp::Subtract, true, Class::Out) => (true, false),
            (BoolOp::Subtract, true, Class::OnOpp) => (true, false),
            (BoolOp::Subtract, false, Class::In) => (true, true),
            _ => (false, false),
        };
        if k {
            keep.push((fi, loops, flip));
        }
    }
    prof.mark("classify");
    if keep.is_empty() {
        return Err(KernelError::EmptyResult("the result is empty".into()));
    }
    // 6. Assemble.
    let mut out = Body::default();
    let mut vmap: HashMap<usize, usize> = HashMap::default();
    let mut emap: HashMap<usize, usize> = HashMap::default();
    let mut from = Vec::new();
    for (fi, loops, flip) in keep {
        let f = &faces[fi];
        let mut nloops: Vec<Vec<Coedge>> = Vec::new();
        for l in loops {
            let mut nl = Vec::new();
            for a in l {
                if a.deg {
                    // A pole piece: its own degenerate edge over the arc's range.
                    let ed = &pool.edges[a.edge];
                    let v = *vmap.entry(ed.v[0]).or_insert_with(|| {
                        out.verts.push(super::topo::Vertex {
                            p: pool.verts[ed.v[0]],
                            tol: pool.vtols[ed.v[0]].max(super::topo::TOL_MIN),
                        });
                        out.verts.len() - 1
                    });
                    out.edges.push(Edge {
                        curve: ed.curve.clone(),
                        t: a.t,
                        v: [v, v],
                        degenerate: true,
                        tol: ed.tol.max(super::topo::TOL_MIN),
                    });
                    nl.push(Coedge {
                        edge: out.edges.len() - 1,
                        fwd: a.fwd,
                        pc: a.pc.clone(),
                    });
                    continue;
                }
                let ne = *emap.entry(a.edge).or_insert_with(|| {
                    let ed = &pool.edges[a.edge];
                    let mut vv = [0usize; 2];
                    for k in 0..2 {
                        vv[k] = *vmap.entry(ed.v[k]).or_insert_with(|| {
                            out.verts.push(super::topo::Vertex {
                                p: pool.verts[ed.v[k]],
                                tol: pool.vtols[ed.v[k]].max(super::topo::TOL_MIN),
                            });
                            out.verts.len() - 1
                        });
                    }
                    out.edges.push(Edge {
                        curve: ed.curve.clone(),
                        t: ed.t,
                        v: vv,
                        degenerate: ed.degenerate,
                        tol: ed.tol.max(super::topo::TOL_MIN),
                    });
                    out.edges.len() - 1
                });
                nl.push(Coedge {
                    edge: ne,
                    fwd: a.fwd,
                    pc: a.pc.clone(),
                });
            }
            nloops.push(nl);
        }
        let mut face = Face {
            surf: f.surf.clone(),
            sense: f.sense,
            loops: nloops,
        };
        if flip {
            Body::reverse_face(&mut face);
        }
        out.faces.push(face);
        from.push((f.owner, f.orig));
    }
    if super::env_flag!("CADRS_BOOL_DEBUG") {
        for (i, f) in out.faces.iter().enumerate() {
            eprintln!(
                "out face {i} {:?} from {:?} loops {:?}",
                f.surf.kind(),
                from[i],
                f.loops
                    .iter()
                    .map(|l| l.iter().map(|c| (c.edge, c.fwd)).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            );
        }
        eprintln!("check: {:?}", out.check());
    }
    let mut from: Vec<Vec<(usize, usize)>> = from.into_iter().map(|x| vec![x]).collect();
    if !imprinting && merge_coplanar {
        from = super::unify::unify_boolean(&mut out, from, flush);
    }
    prof.mark("assemble");
    super::unify::merge_edges(&mut out);
    // The run merged within its tolerance: the result's entities say so, the p-curves' gaps
    // searched for as the Full check measures them. An edge lying off a face by more (an
    // input's edge off its own face, which the cut faces keep) has that recorded, up to a part
    // in 10⁵ of the result's size.
    out.fit_tolerances_searched(run_tol * 10.0);
    let wide = 1e-5 * out.bbox().diagonal();
    for fi in 0..out.faces.len() {
        for c in out.faces[fi].loops.iter().flatten() {
            let e = &out.edges[c.edge];
            if e.degenerate {
                continue;
            }
            let g = super::topo::pcurve_gap(e, &c.pc, &out.faces[fi].surf);
            if g <= wide && g * 1.01 > e.tol {
                out.edges[c.edge].tol = g * 1.01;
            }
        }
    }
    // Likewise a vertex where the p-curves of a loop meet a hair apart (up to the same part
    // in 10⁵), as `uv_gap` measures it.
    for fi in 0..out.faces.len() {
        let f = &out.faces[fi];
        let (mu, mv) = f.surf.metric();
        let off = |d: f64, periodic: bool| {
            if periodic {
                (d - (d / std::f64::consts::TAU).round() * std::f64::consts::TAU).abs()
            } else {
                d.abs()
            }
        };
        let mut raise: Vec<(usize, f64)> = Vec::new();
        for l in &f.loops {
            for k in 0..l.len() {
                let (x, y) = (&l[k], &l[(k + 1) % l.len()]);
                let (ex, ey) = (&out.edges[x.edge], &out.edges[y.edge]);
                let (_, t1) = super::topo::co_range(ex, x.fwd);
                let (t0, _) = super::topo::co_range(ey, y.fwd);
                let (pa, pb) = (x.pc.point(t1), y.pc.point(t0));
                let g = (off(pa.x - pb.x, f.surf.u_periodic()) * mu)
                    .max(off(pa.y - pb.y, f.surf.v_periodic()) * mv);
                let v = if x.fwd { ex.v[1] } else { ex.v[0] };
                if g <= wide && g * 1.01 > out.verts[v].tol {
                    raise.push((v, g * 1.01));
                }
            }
        }
        for (v, t) in raise {
            out.verts[v].tol = out.verts[v].tol.max(t);
        }
    }
    prof.mark("unify");
    work.spend((out.faces.len() as u64).pow(2), "shell assembly")?;
    if super::env_flag!("CADRS_BOOL_WORK") {
        eprintln!("boolean work: {}", work.done);
    }
    assemble_shells(&mut out)?;
    prof.mark("shells");
    touch_voids(&mut out, run_tol * 10.0);
    prof.mark("voids");
    // Faces of the two inputs on one surface (by input face): only a solid holding both of a
    // pair can enclose nothing.
    let pairs: HashSet<(usize, usize)> = (0..na_faces)
        .flat_map(|fa| coincident[fa].iter().map(move |&fb| (fa, fb)))
        .map(|(fa, fb)| (faces[fa].orig, faces[fb].orig))
        .collect();
    let (dropped, volumes) = drop_empty_solids(&mut out, &mut from, &pairs);
    prof.mark("empty solids");
    if dropped && out.faces.is_empty() {
        return Err(KernelError::EmptyResult("the result is empty".into()));
    }
    super::unify::split_shared_vertices(&mut out);
    // A face whose loop's p-curves don't meet in (u, v) can't be closed: an error, never an
    // invalid body.
    if let Some(why) = uv_gap(&out) {
        return Err(KernelError::BooleanFailed {
            stage: "face closure",
            why: format!("the boolean built a face that doesn't close: {why}"),
        });
    }
    // Nor any other invalid body ([`Body::check`]; the volumes of the solids that hold faces on
    // one surface known from above, the others' shells oriented by ray parity).
    let mut crossing = None;
    if matches!(mode, Mode::Bool(_)) {
        let why = out.check_structure().into_iter().next().or_else(|| {
            let (si, v) = volumes
                .iter()
                .enumerate()
                .find_map(|(si, v)| v.filter(|v| !(*v > 0.0)).map(|v| (si, v)))?;
            Some(format!(
                "solid {si} encloses a volume of {v:.3e} (its faces point in)"
            ))
        });
        if let Some(why) = why {
            return Err(KernelError::BooleanFailed {
                stage: "validity",
                why: format!("the boolean's result is invalid: {why}"),
            });
        }
        // Faces crossing each other away from their edges (a cylinder tangent to a box's face:
        // two pieces of the result passing through each other); and the new faces' loops in
        // (u, v), none crossing another. A refusal the ladder retries; when no retry passes,
        // [`ladder`] hands back this result after all, flagged.
        prof.mark("checks");
        if check_crossings {
            let whole_of = |f: usize| match from.get(f).map(|s| s.as_slice()) {
                Some(&[(o, g)]) if whole.contains(&(o, g)) => Some(o),
                _ => None,
            };
            let whole_of: Vec<Option<usize>> = (0..out.faces.len()).map(whole_of).collect();
            // (Faces of one input don't cross each other, cut or not: only pairs from the two
            // inputs, or with a face merged from both.)
            let owner_of: Vec<Option<usize>> = (0..out.faces.len())
                .map(|f| match from.get(f).map(|s| s.as_slice()) {
                    Some(&[(o, _)]) => Some(o),
                    _ => None,
                })
                .collect();
            // (Loops in (u, v) only where they wrap: new faces of several loops on periodic
            // surfaces, a loop running over the seam; flattening every face's loops costs as
            // much as the rest of the run.)
            let wraps = |f: usize| {
                out.faces[f].loops.len() > 1
                    && (out.faces[f].surf.u_periodic() || out.faces[f].surf.v_periodic())
            };
            let why = (0..out.faces.len())
                .filter(|&f| whole_of[f].is_none() && wraps(f))
                .find_map(|f| out.check_face_domain(f).into_iter().next())
                .or_else(|| {
                    super::check_faces::face_crossings_where(&out, &|i, j| {
                        owner_of[i].is_none() || owner_of[i] != owner_of[j]
                    })
                    .into_iter()
                    .next()
                });
            crossing = why;
        }
    }
    // Solids in, faces out but no solid: some piece went missing.
    if !a.solids.is_empty()
        && !b.solids.is_empty()
        && !out.faces.is_empty()
        && out.solids.is_empty()
    {
        return Err(KernelError::OpenResult(
            "the boolean left an open shell".into(),
        ));
    }
    prof.mark("crossings");
    prof.report(&format!("{op:?} {}×{} faces", a.faces.len(), b.faces.len()));
    if let Some(why) = crossing {
        // (The result kept for the ladder, should no other run do better.)
        REFUSED.with(|c| *c.borrow_mut() = Some(BoolOut { body: out, from }));
        return Err(KernelError::BooleanFailed {
            stage: CROSSING,
            why: format!("the boolean's result is invalid: {why}"),
        });
    }
    Ok(BoolOut { body: out, from })
}

/// A void's vertices lying outside its solid's outer shell by no more than `tol` (a void
/// touching the outer shell, as a cube's corners a sphere round it, within the run's
/// tolerance) record the gap in their tolerance: they lie on the shell.
fn touch_voids(b: &mut Body, tol: f64) {
    let mut raise: Vec<(usize, f64)> = Vec::new();
    for s in &b.solids {
        let [outer, voids @ ..] = s.shells.as_slice() else {
            continue;
        };
        if voids.is_empty() {
            continue;
        }
        let out = super::kernel::subset(b, &b.shells[*outer].faces, true);
        let polys: Vec<Vec<Vec<P2>>> = out
            .faces
            .iter()
            .map(|f| classify::face_polygons(&out, f))
            .collect();
        for &sh in voids {
            let mut vs: Vec<usize> = b.shells[sh]
                .faces
                .iter()
                .flat_map(|&f| {
                    b.faces[f]
                        .loops
                        .iter()
                        .flatten()
                        .flat_map(|c| b.edges[c.edge].v)
                })
                .collect();
            vs.sort_unstable();
            vs.dedup();
            for v in vs {
                let vx = &b.verts[v];
                if classify::classify_point(&out, &polys, &vx.p, vx.tol.max(super::tol::POINT))
                    != PointClass::Outside
                {
                    continue;
                }
                let d = out
                    .faces
                    .iter()
                    .map(|f| f.surf.distance(&vx.p))
                    .fold(f64::INFINITY, f64::min);
                if d <= tol {
                    raise.push((v, d * 1.01));
                }
            }
        }
    }
    for (v, t) in raise {
        b.verts[v].tol = b.verts[v].tol.max(t);
    }
}

/// Whether `c` on [ta, tb] runs within `tol` of one of `edges` all along, where that edge lies on
/// the surface of `other` (within the run's tolerance `run_tol`, the edge's, the face's or
/// `tol`): `c` duplicates the edge there.
#[allow(clippy::too_many_arguments)]
fn duplicates_edge(
    pool: &Pool,
    edges: &[usize],
    other: &WFace,
    c: &Curve,
    ta: f64,
    tb: f64,
    tol: f64,
    run_tol: f64,
) -> bool {
    let pm = c.point(0.5 * (ta + tb));
    let dist = |e: usize, p: &P3| {
        let ed = &pool.edges[e];
        let (a, b) = (ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
        (ed.curve.point(ed.curve.closest(p, a, b)) - p).norm()
    };
    let Some(e) = edges
        .iter()
        .copied()
        .filter(|&e| !pool.edges[e].degenerate)
        .min_by(|&x, &y| dist(x, &pm).total_cmp(&dist(y, &pm)))
    else {
        return false;
    };
    let ed = &pool.edges[e];
    let (a, b) = (ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
    let on = run_tol.max(ed.tol).max(other.tol).max(tol);
    let n = 16;
    (0..=n).all(|k| {
        let p = c.point(ta + (tb - ta) * k as f64 / n as f64);
        let q = ed.curve.point(ed.curve.closest(&p, a, b));
        (q - p).norm() <= tol && other.surf.distance(&q) <= on
    })
}

/// The points where two curves meet (`hits`: (t on `a`, t on `b`), as [`ssi::curve_curve`]
/// gives them) with each stretch where the curves run within `tol` of each other reduced to its
/// two ends: two intersection curves along one contact (pieces of a near tangent or partly
/// coincident contact) touch at every wiggle within the tolerance, and each touch would cut
/// both. Two hits are on one stretch when `a` stays within `tol` of `b` at three points between
/// them. The parameters on `a`, in order.
fn overlap_runs(a: &Curve, b: &Curve, mut hits: Vec<(f64, f64)>, tol: f64) -> Vec<f64> {
    hits.sort_by(|x, y| x.0.total_cmp(&y.0));
    let close = |s: f64, t: f64, s2: f64, t2: f64| {
        [0.25, 0.5, 0.75].iter().all(|k| {
            let p = a.point(s + (s2 - s) * k);
            let tb = b.closest(&p, t.min(t2), t.max(t2));
            (b.point(tb) - p).norm() <= 2.0 * tol
        })
    };
    let mut out: Vec<f64> = Vec::new();
    let mut k = 0;
    while k < hits.len() {
        let mut e = k;
        while e + 1 < hits.len() && close(hits[e].0, hits[e].1, hits[e + 1].0, hits[e + 1].1) {
            e += 1;
        }
        out.push(hits[k].0);
        if e > k {
            out.push(hits[e].0);
        }
        k = e + 1;
    }
    out
}

/// Drops the solids of `b` that enclose no volume (to 1e-9 of the body's box, cubed): a closed
/// shell of faces of the two inputs that met face to face, both kept, is a sheet-thin sliver,
/// not a solid, as are two faces closing on each other. Only solids of a few faces, or holding
/// faces from both faces of a pair in `pairs` (input faces of the two bodies on one surface:
/// (face of a, face of b)), are measured. Their faces go too (and
/// their entries in `from`). Returns whether any went, and the volumes of the solids left
/// (None: not measured).
fn drop_empty_solids(
    b: &mut Body,
    from: &mut Vec<Vec<(usize, usize)>>,
    pairs: &HashSet<(usize, usize)>,
) -> (bool, Vec<Option<f64>>) {
    let size = b.bbox().diagonal();
    let suspect = |si: usize| {
        let faces = b.solid_faces(si);
        // (A few faces are cheap to measure: two faces closing on each other are a sliver too.)
        if faces.len() <= 8 {
            return true;
        }
        let origins: Vec<(usize, usize)> = faces
            .iter()
            .flat_map(|&f| from.get(f).into_iter().flatten().copied())
            .collect();
        origins.iter().filter(|o| o.0 == 0).any(|&(_, x)| {
            origins
                .iter()
                .any(|&(w, y)| w == 1 && pairs.contains(&(x, y)))
        })
    };
    let volumes: Vec<Option<f64>> = (0..b.solids.len())
        .map(|si| {
            suspect(si).then(|| {
                super::mass::body_volume(&super::kernel::subset(b, &b.solid_faces(si), true))
            })
        })
        .collect();
    let empty: Vec<usize> = (0..b.solids.len())
        .filter(|&si| volumes[si].is_some_and(|v| v.abs() <= tol::REL * size * size * size))
        .collect();
    if empty.is_empty() {
        return (false, volumes);
    }
    let left: Vec<Option<f64>> = (0..b.solids.len())
        .filter(|si| !empty.contains(si))
        .map(|si| volumes[si])
        .collect();
    let mut dead = vec![false; b.faces.len()];
    for &si in &empty {
        for f in b.solid_faces(si) {
            dead[f] = true;
        }
    }
    let mut fmap = vec![usize::MAX; b.faces.len()];
    let mut faces = Vec::new();
    let mut kept_from = Vec::new();
    for (fi, f) in std::mem::take(&mut b.faces).into_iter().enumerate() {
        if !dead[fi] {
            fmap[fi] = faces.len();
            faces.push(f);
            kept_from.push(from.get(fi).cloned().unwrap_or_default());
        }
    }
    b.faces = faces;
    *from = kept_from;
    let dead_shells: HashSet<usize> = empty
        .iter()
        .flat_map(|&si| b.solids[si].shells.clone())
        .collect();
    let mut smap = vec![usize::MAX; b.shells.len()];
    let mut shells = Vec::new();
    for (si, mut sh) in std::mem::take(&mut b.shells).into_iter().enumerate() {
        if dead_shells.contains(&si) {
            continue;
        }
        sh.faces = sh
            .faces
            .iter()
            .filter(|&&f| !dead[f])
            .map(|&f| fmap[f])
            .collect();
        smap[si] = shells.len();
        shells.push(sh);
    }
    b.shells = shells;
    let solids = std::mem::take(&mut b.solids);
    b.solids = solids
        .into_iter()
        .enumerate()
        .filter(|(si, _)| !empty.contains(si))
        .map(|(_, mut so)| {
            so.shells = so.shells.iter().map(|&s| smap[s]).collect();
            so
        })
        .collect();
    super::unify::compact(b);
    (true, left)
}

/// Splits edges used by more than two faces (solids touching along an edge) into one edge per
/// pair of faces bounding the same material wedge, pairing the faces in their order around
/// the edge.
pub fn split_nonmanifold(b: &mut Body) {
    let ne = b.edges.len();
    let mut uses: Vec<Vec<(usize, usize, usize)>> = vec![Vec::new(); ne]; // (face, loop, index)
    for (fi, f) in b.faces.iter().enumerate() {
        for (li, l) in f.loops.iter().enumerate() {
            for (k, c) in l.iter().enumerate() {
                uses[c.edge].push((fi, li, k));
            }
        }
    }
    for e in 0..ne {
        let u = uses[e].clone();
        if u.len() <= 2 || b.edges[e].degenerate {
            continue;
        }
        let ed = b.edges[e].clone();
        let tm = ed.mid_t();
        let p = ed.point(tm);
        let t = ed.curve.tangent(tm).normalize();
        let a = any_perpendicular(&t);
        let bb = t.cross(&a);
        // Each use: its angle about t and whether the material lies counter-clockwise of it.
        let mut items: Vec<(f64, bool, (usize, usize, usize))> = Vec::new();
        for &(fi, li, k) in &u {
            let f = &b.faces[fi];
            let c = &f.loops[li][k];
            let uv = c.pc.point(tm);
            let n = f.surf.normal(uv.x, uv.y) * if f.sense { 1.0 } else { -1.0 };
            let d = t * if c.fwd { 1.0 } else { -1.0 };
            let w0 = n.cross(&d); // into the face (first order)
            // A point a little way into the face, so faces tangent at the edge separate.
            let (_, duv, _) = c.pc.d2(tm);
            let perp = V2::new(-duv.y, duv.x);
            let len = (ed.t[1] - ed.t[0]).abs() * ed.curve.tangent(tm).norm();
            // (Not `clamp`: a NaN length takes 0.05 here.)
            #[allow(clippy::manual_clamp)]
            let step = (len * 1e-2).min(0.05).max(1e-4);
            let (pu, su, sv) = f.surf.d1(uv.x, uv.y);
            let g = su * perp.x + sv * perp.y;
            let sgn = if g.dot(&w0) >= 0.0 { 1.0 } else { -1.0 };
            let scale = step / g.norm().max(tol::TINY);
            let q = f
                .surf
                .eval(uv.x + perp.x * scale * sgn, uv.y + perp.y * scale * sgn);
            let w = {
                let r = q - pu;
                let r = r - t * r.dot(&t);
                if r.norm() > tol::DEGENERATE {
                    r.normalize()
                } else {
                    w0
                }
            };
            let ang = w.dot(&bb).atan2(w.dot(&a));
            let ccw = t.cross(&w0);
            let mat_ccw = (-n).dot(&ccw) > 0.0;
            items.push((ang, mat_ccw, (fi, li, k)));
        }
        let _ = p;
        items.sort_by(|x, y| x.0.total_cmp(&y.0));
        let m = items.len();
        // Pair each face whose material is counter-clockwise with the next one.
        type Use = (usize, usize, usize);
        let mut pairs: Vec<(Use, Use)> = Vec::new();
        for i in 0..m {
            if items[i].1 {
                let j = (i + 1) % m;
                pairs.push((items[i].2, items[j].2));
            }
        }
        if pairs.len() < 2 {
            continue;
        }
        for (pi, (x, y)) in pairs.into_iter().enumerate() {
            if pi == 0 {
                continue;
            }
            b.edges.push(ed.clone());
            let nid = b.edges.len() - 1;
            for (fi, li, k) in [x, y] {
                b.faces[fi].loops[li][k].edge = nid;
            }
        }
    }
}

/// Groups faces into shells by shared edges and shells into solids (outer shells and voids).
pub fn assemble_shells(b: &mut Body) -> Result<()> {
    split_nonmanifold(b);
    let ef = b.edge_faces();
    let nf = b.faces.len();
    let mut comp = vec![usize::MAX; nf];
    let mut shells: Vec<Vec<usize>> = Vec::new();
    for s in 0..nf {
        if comp[s] != usize::MAX {
            continue;
        }
        let id = shells.len();
        let mut stack = vec![s];
        comp[s] = id;
        let mut fs = Vec::new();
        while let Some(f) = stack.pop() {
            fs.push(f);
            for l in &b.faces[f].loops {
                for c in l {
                    for &g in &ef[c.edge] {
                        if comp[g] == usize::MAX {
                            comp[g] = id;
                            stack.push(g);
                        }
                    }
                }
            }
        }
        fs.sort();
        shells.push(fs);
    }
    // Closed shells: every non-degenerate edge used twice.
    b.shells.clear();
    b.solids.clear();
    let mut outer: Vec<(usize, f64)> = Vec::new();
    let mut voids: Vec<(usize, f64)> = Vec::new();
    for fs in shells {
        let mut closed = true;
        let mut uses: HashMap<usize, i32> = HashMap::default();
        for &f in &fs {
            for l in &b.faces[f].loops {
                for c in l {
                    *uses.entry(c.edge).or_default() += if c.fwd { 1 } else { -1 };
                }
            }
        }
        for (e, u) in &uses {
            if *u != 0 && !b.edges[*e].degenerate {
                closed = false;
                if super::env_flag!("CADRS_BOOL_DEBUG") {
                    eprintln!("open edge {e}: uses {u}, {:?}", b.edges[*e]);
                }
            }
        }
        let si = b.shells.len();
        b.shells.push(Shell {
            faces: fs.clone(),
            closed,
        });
        if closed {
            outer.push((si, f64::NAN));
        }
    }
    // With several closed shells, tell outer shells from voids: whether a shell's faces point
    // out of what it encloses (rays leaving it along a face normal cross it an even number of
    // times). Each is sized by its box, for picking the smallest one around a void. (Their
    // volumes tell the same, but integrating faces cut by long intersection curves is slow.)
    if outer.len() > 1 {
        let polys: Vec<Vec<Vec<P2>>> = b
            .faces
            .iter()
            .map(|f| classify::face_polygons(b, f))
            .collect();
        let all: Vec<(usize, f64, bool)> = outer
            .iter()
            .map(|(si, _)| {
                let fs = &b.shells[*si].faces;
                let mut bb = Bbox::empty();
                for &f in fs {
                    for l in &b.faces[f].loops {
                        for c in l {
                            let e = &b.edges[c.edge];
                            bb.add(&e.point(e.t[0]));
                            bb.add(&e.point(0.5 * (e.t[0] + e.t[1])));
                        }
                    }
                }
                let out = shell_points_out(b, fs, &polys).unwrap_or_else(|| {
                    let shift = b.bbox().center().coords;
                    fs.iter()
                        .map(|&f| super::mass::face_integrals(b, &b.faces[f], &shift)[4])
                        .sum::<f64>()
                        >= 0.0
                });
                (*si, bb.diagonal(), out)
            })
            .collect();
        outer.clear();
        for (si, size, out) in all {
            if out {
                outer.push((si, size));
            } else {
                voids.push((si, size));
            }
        }
    }
    let open = b.shells.iter().any(|s| !s.closed);
    if open && !outer.is_empty() {
        return Err(KernelError::OpenResult(
            "the boolean left an open shell".into(),
        ));
    }
    let mut solids: Vec<Vec<usize>> = outer.iter().map(|(s, _)| vec![*s]).collect();
    for (v, _) in voids {
        // The smallest outer shell containing a point of the void.
        let f = b.shells[v].faces[0];
        let p = {
            let fc = &b.faces[f];
            let polys = classify::face_polygons(b, fc);
            let uv = interior_of_polys(&polys).unwrap_or(P2::origin());
            fc.surf.eval(uv.x, uv.y)
        };
        let mut best: Option<(f64, usize)> = None;
        for (k, (s, vol)) in outer.iter().enumerate() {
            let sub = super::kernel::subset(b, &b.shells[*s].faces, true);
            let polys: Vec<Vec<Vec<P2>>> = sub
                .faces
                .iter()
                .map(|f| classify::face_polygons(&sub, f))
                .collect();
            if classify::parity_inside(&sub, &polys, &p) && best.is_none_or(|(bv, _)| *vol < bv) {
                best = Some((*vol, k));
            }
        }
        if let Some((_, k)) = best {
            solids[k].push(v);
        }
    }
    for s in solids {
        b.solids.push(Solid { shells: s });
    }
    Ok(())
}

/// Whether the faces of a closed shell point out of the region it encloses, by ray parity:
/// from points inside a few of its faces, rays along the outward normal (and two tilts of it)
/// cross the shell an even number of times when they do. None when the votes don't settle it.
fn shell_points_out(b: &Body, faces: &[usize], polys: &[Vec<Vec<P2>>]) -> Option<bool> {
    let size = b.bbox().diagonal().max(tol::ROUNDOFF);
    // Rays against this shell's faces only, each where the line comes to its box.
    let mut boxes = vec![Bbox::empty(); b.faces.len()];
    for &f in faces {
        boxes[f] = b.face_bbox(f);
    }
    let (mut out, mut inward) = (0, 0);
    for &f in faces.iter().take(3) {
        let fc = &b.faces[f];
        let Some(uv) = interior_of_polys(&polys[f]) else {
            continue;
        };
        let p = fc.surf.eval(uv.x, uv.y);
        let n = fc.surf.normal(uv.x, uv.y) * if fc.sense { 1.0 } else { -1.0 };
        let (t1, t2) = (any_perpendicular(&n), n.cross(&any_perpendicular(&n)));
        for d in [n, (n + t1 * 0.3).normalize(), (n + t2 * 0.3).normalize()] {
            let hits = classify::ray_hits_among(b, polys, faces, Some(&boxes), &p, &d);
            let k = hits.iter().filter(|(t, _, _)| *t > 1e-7 * size).count();
            if k % 2 == 0 {
                out += 1;
            } else {
                inward += 1;
            }
        }
    }
    match out.cmp(&inward) {
        std::cmp::Ordering::Greater => Some(true),
        std::cmp::Ordering::Less => Some(false),
        std::cmp::Ordering::Equal => None,
    }
}

/// The kernel operation: the target combined with each tool in turn.
pub fn boolean(
    k: &mut NativeKernel,
    op: BoolOp,
    target: BodyId,
    tools: &[BodyId],
) -> Result<OpResult> {
    boolean_with(k, op, target, tools, true)
}

/// [`boolean`], merging faces on one surface afterwards or not (`merge_coplanar`, as in
/// [`BoolOptions`]).
pub fn boolean_with(
    k: &mut NativeKernel,
    op: BoolOp,
    target: BodyId,
    tools: &[BodyId],
    merge_coplanar: bool,
) -> Result<OpResult> {
    let opts = BoolOptions {
        tol: k.tolerances(),
        merge_coplanar,
        ..BoolOptions::default()
    };
    boolean_opts(k, op, target, tools, opts)
}

/// [`boolean`] with one run only ([`BoolOptions::retry`] off): for a question about the result
/// (whether it is one solid, has volume), not a result to keep.
pub fn boolean_once(
    k: &mut NativeKernel,
    op: BoolOp,
    target: BodyId,
    tools: &[BodyId],
) -> Result<OpResult> {
    let opts = BoolOptions {
        tol: k.tolerances(),
        retry: false,
        ..BoolOptions::default()
    };
    boolean_opts(k, op, target, tools, opts)
}

fn boolean_opts(
    k: &mut NativeKernel,
    op: BoolOp,
    target: BodyId,
    tools: &[BodyId],
    opts: BoolOptions,
) -> Result<OpResult> {
    // Sheets, wires and points: their own rules (`boolean_kinds`).
    if let Some(r) = super::boolean_kinds::boolean(k, op, target, tools, opts) {
        return r;
    }
    let mut cur = (**k.get(target)?).clone();
    // For each face of `cur`: where it came from (body, face).
    let mut origin: Vec<Vec<(BodyId, usize)>> =
        (0..cur.faces.len()).map(|i| vec![(target, i)]).collect();
    // A union's inputs so far, each by a box grown by its tolerance (holding `cur` between
    // them: a union lies within its inputs). A tool apart from them all joins `cur` as it is,
    // no boolean: an Add of a rail onto 45 balusters unioned the balusters one by one, each a
    // boolean over all those before it, before the rail.
    let reach = |b: &Body| {
        let tol = b
            .edges
            .iter()
            .map(|e| e.tol)
            .chain(b.verts.iter().map(|v| v.tol))
            .fold(opts.tol.point, f64::max);
        b.bbox().grow(2.0 * tol)
    };
    let mut boxes: Vec<Bbox> = Vec::new();
    if op == BoolOp::Union {
        boxes.push(reach(&cur));
    }
    for &t in tools {
        let tb = (**k.get(t)?).clone();
        if op == BoolOp::Union {
            let b = reach(&tb);
            let apart = boxes.iter().all(|x| !x.overlaps(&b));
            boxes.push(b);
            if apart && !tb.faces.is_empty() {
                cur.append(&tb);
                origin.extend((0..tb.faces.len()).map(|f| vec![(t, f)]));
                continue;
            }
        }
        let r = boolean_bodies_with(&cur, &tb, op, opts)?;
        origin = r
            .from
            .iter()
            .map(|srcs| {
                srcs.iter()
                    .flat_map(|&(o, f)| {
                        if o == 0 {
                            origin[f].clone()
                        } else {
                            vec![(t, f)]
                        }
                    })
                    .collect()
            })
            .collect();
        cur = r.body;
    }
    let mut history = History::default();
    for (i, srcs) in origin.iter().enumerate() {
        for (bd, f) in srcs {
            history.modified.push((
                FaceId(i as u64),
                InputFace {
                    body: *bd,
                    face: FaceId(*f as u64),
                },
            ));
        }
    }
    let mut inputs = vec![target];
    inputs.extend_from_slice(tools);
    for &ib in &inputs {
        let n = k.get(ib)?.faces.len();
        for f in 0..n {
            if !origin.iter().flatten().any(|(b, g)| *b == ib && *g == f) {
                history.deleted.push(InputFace {
                    body: ib,
                    face: FaceId(f as u64),
                });
            }
        }
    }
    let id = k.insert(cur);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A union of bodies apart puts them side by side without a boolean, as the boolean of
    /// each pair does (the same faces, solids and volume), and a body touching them all then
    /// joins them; whether a union is several solids is told from boxes alone where it can be.
    #[test]
    fn union_of_bodies_apart() {
        let mut k = NativeKernel::new();
        let block = |k: &mut NativeKernel, x: f64, z: f64, dx: f64, dz: f64| {
            let super::super::draw::Shape::Body(id) = k.box_at(x, 0.0, z, dx, 1.0, dz).unwrap()
            else {
                unreachable!()
            };
            id
        };
        let posts: Vec<BodyId> = (0..5)
            .map(|i| block(&mut k, 3.0 * i as f64, 0.0, 1.0, 3.0))
            .collect();
        let mut pairwise = (**k.get(posts[0]).unwrap()).clone();
        for p in &posts[1..] {
            pairwise = boolean_bodies(&pairwise, k.get(*p).unwrap(), BoolOp::Union)
                .unwrap()
                .body;
        }
        let r = boolean(&mut k, BoolOp::Union, posts[0], &posts[1..]).unwrap();
        let side = (**k.get(r.bodies[0]).unwrap()).clone();
        assert_eq!(side.faces.len(), pairwise.faces.len());
        assert_eq!((side.solids.len(), pairwise.solids.len()), (5, 5));
        let v = super::super::mass::body_volume;
        assert!((v(&side) - v(&pairwise)).abs() < 1e-9);
        assert_eq!(r.history.modified.len(), side.faces.len());
        // A rail along their tops joins them.
        let rail = block(&mut k, 0.0, 3.0, 13.0, 0.5);
        let mut tools = posts[1..].to_vec();
        tools.push(rail);
        let r = boolean(&mut k, BoolOp::Union, posts[0], &tools).unwrap();
        assert_eq!(k.solid_count(r.bodies[0]).unwrap(), 1);
        assert!((v(k.get(r.bodies[0]).unwrap()) - (15.0 + 6.5)).abs() < 1e-9);
        // Two posts as one body: a rail over one of them can't join both, by their boxes; one
        // over both can (and does).
        let pair = boolean(&mut k, BoolOp::Union, posts[0], &[posts[1]])
            .unwrap()
            .bodies[0];
        let short = block(&mut k, 0.0, 3.0, 1.0, 0.5);
        let long = block(&mut k, 0.0, 3.0, 4.0, 0.5);
        let (pb, sb, lb) = (
            k.get(pair).unwrap(),
            k.get(short).unwrap(),
            k.get(long).unwrap(),
        );
        assert!(union_falls_apart(pb, sb));
        assert!(!union_falls_apart(pb, lb));
        assert!(!crate::Kernel::joins(&mut k, pair, short).unwrap());
        assert!(crate::Kernel::joins(&mut k, pair, long).unwrap());
    }

    /// A boolean past its work budget fails (deterministically, at the same stage every
    /// time), and the default budget holds a plain one easily.
    #[test]
    fn work_budget() {
        let cube = |x: f64| {
            let mut k = NativeKernel::new();
            let super::super::draw::Shape::Body(id) = k.box_at(x, 0.0, 0.0, 1.0, 1.0, 1.0).unwrap()
            else {
                unreachable!()
            };
            (**k.get(id).unwrap()).clone()
        };
        let (a, b) = (cube(0.0), cube(0.5));
        assert!(boolean_bodies(&a, &b, BoolOp::Union).is_ok());
        let tight = BoolOptions {
            work_budget: 10,
            ..BoolOptions::default()
        };
        let fail = || {
            boolean_bodies_with(&a, &b, BoolOp::Union, tight)
                .err()
                .map(|e| e.to_string())
        };
        let err = fail().unwrap_or_default();
        assert!(err.contains("work budget"), "{err}");
        assert_eq!(fail(), Some(err));
    }

    /// A union retried one target solid at a time keeps the target's faces first where they
    /// merge with the tool's (a face's first source names it, as in a single run).
    #[test]
    fn per_target_solid_union_keeps_target_faces_first() {
        let cube = |x: f64, z: f64, h: f64| {
            let mut k = NativeKernel::new();
            let super::super::draw::Shape::Body(id) = k.box_at(x, 0.0, z, 1.0, 1.0, h).unwrap()
            else {
                unreachable!()
            };
            (**k.get(id).unwrap()).clone()
        };
        let two = boolean_bodies(&cube(0.0, 0.0, 1.0), &cube(3.0, 0.0, 1.0), BoolOp::Union)
            .unwrap()
            .body;
        assert_eq!(two.solids.len(), 2);
        let tool = cube(0.0, 0.5, 1.5);
        let r = per_target_solid(&two, &tool, BoolOp::Union, BoolOptions::default())
            .unwrap()
            .unwrap();
        assert_eq!(r.body.solids.len(), 2);
        // The sides at x = 0, x = 1, y = 0 and y = 1 of the first cube merge with the tool's.
        let merged: Vec<&Vec<(usize, usize)>> = r
            .from
            .iter()
            .filter(|s| s.iter().any(|x| x.0 == 0) && s.iter().any(|x| x.0 == 1))
            .collect();
        assert_eq!(merged.len(), 4, "{:?}", r.from);
        assert!(merged.iter().all(|s| s[0].0 == 0), "{:?}", r.from);
    }

    /// A cylinder face whose seam runs straight through a hole (the hole's vertices on it at
    /// z = 1 and 2, its edges either side of u = 0): the seam is cut there, the piece in the
    /// hole dropped, and the face is one valid loop (Dr Browns cap's torus, Fillet 1).
    #[test]
    fn seam_through_a_hole_is_cut() {
        use super::super::topo::Vertex;
        let (r, h, w) = (5.0, 3.0, 0.2);
        let line_pc = |o: (f64, f64), d: (f64, f64)| Curve::Line {
            o: P3::new(o.0, o.1, 0.0),
            d: V3::new(d.0, d.1, 0.0),
        };
        let at = |u: f64, z: f64| P3::new(r * u.cos(), r * u.sin(), z);
        let circle = |z: f64| Curve::Circle {
            c: P3::new(0.0, 0.0, z),
            x: V3::x(),
            y: V3::y(),
            r,
        };
        let mut b = Body::default();
        // P0, P1 (the seam's ends), A, B (the hole's vertices on the seam), C, D (u = w), E, F (u = −w).
        for p in [
            at(0.0, 0.0),
            at(0.0, h),
            at(0.0, 1.0),
            at(0.0, 2.0),
            at(w, 1.0),
            at(w, 2.0),
            at(-w, 1.0),
            at(-w, 2.0),
        ] {
            b.verts.push(Vertex {
                p,
                tol: tol::CONFUSION,
            });
        }
        let edge = |curve: Curve, t: [f64; 2], v: [usize; 2]| Edge {
            curve,
            t,
            v,
            degenerate: false,
            tol: tol::CONFUSION,
        };
        let up = |u: f64| Curve::Line {
            o: at(u, 0.0),
            d: V3::z(),
        };
        b.edges = vec![
            edge(circle(0.0), [0.0, TAU], [0, 0]), // 0 bottom
            edge(circle(h), [0.0, TAU], [1, 1]),   // 1 top
            edge(up(0.0), [0.0, h], [0, 1]),       // 2 seam
            edge(up(w), [1.0, 2.0], [4, 5]),       // 3 C→D
            edge(circle(2.0), [0.0, w], [3, 5]),   // 4 B→D
            edge(circle(2.0), [-w, 0.0], [7, 3]),  // 5 F→B
            edge(up(-w), [1.0, 2.0], [6, 7]),      // 6 E→F
            edge(circle(1.0), [-w, 0.0], [6, 2]),  // 7 E→A
            edge(circle(1.0), [0.0, w], [2, 4]),   // 8 A→C
        ];
        let co = |edge: usize, fwd: bool, pc: Curve| Coedge { edge, fwd, pc };
        let outer = vec![
            co(0, true, line_pc((0.0, 0.0), (1.0, 0.0))),
            co(2, true, line_pc((TAU, 0.0), (0.0, 1.0))),
            co(1, false, line_pc((0.0, h), (1.0, 0.0))),
            co(2, false, line_pc((0.0, 0.0), (0.0, 1.0))),
        ];
        // Clockwise round the hole: C, A, E, F, B, D.
        let hole = vec![
            co(8, false, line_pc((0.0, 1.0), (1.0, 0.0))),
            co(7, false, line_pc((0.0, 1.0), (1.0, 0.0))),
            co(6, true, line_pc((-w, 0.0), (0.0, 1.0))),
            co(5, true, line_pc((0.0, 2.0), (1.0, 0.0))),
            co(4, true, line_pc((0.0, 2.0), (1.0, 0.0))),
            co(3, false, line_pc((w, 0.0), (0.0, 1.0))),
        ];
        b.faces.push(Face {
            surf: Surface::Cylinder {
                f: Frame::world(),
                r,
            },
            sense: true,
            loops: vec![outer, hole],
        });
        b.shells.push(Shell {
            faces: vec![0],
            closed: false,
        });
        assert!(cut_seams_at_holes(&mut b, tol::POINT));
        let f = &b.faces[0];
        assert_eq!(f.loops.len(), 1);
        // The seam's two pieces (each up one side and down the other), the hole's six edges,
        // the two circles.
        assert_eq!(f.loops[0].len(), 4 + 6 + 2);
        assert_eq!(b.check(), Vec::<String>::new());
        // The seam piece inside the hole is gone: no edge of the face runs from A to B.
        assert!(!f.loops[0].iter().any(|c| {
            let e = &b.edges[c.edge];
            e.v == [2, 3] || e.v == [3, 2]
        }));
    }

    /// A p-curve on a B-spline tube closing on itself only within 1e-6 (not exactly closed:
    /// no period to shift by) goes to the side of the seam the arc it replaces was on.
    #[test]
    fn seam_side_follows_the_arc() {
        use super::super::nurbs::{H, NurbsSurface};
        let ring = [
            (1.0, 0.0),
            (0.0, 1.0),
            (-1.0, 0.0),
            (0.0, -1.0),
            (1.0, 1e-6),
        ];
        let cps: Vec<Vec<H>> = [0.0, 1.0]
            .iter()
            .map(|&z| ring.iter().map(|&(x, y)| H::new(x, y, z, 1.0)).collect())
            .collect();
        let n = NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
            cps,
        )
        .unwrap();
        let surf = Surface::Nurbs(std::sync::Arc::new(n));
        let seam = Curve::Line {
            o: P3::new(1.0, 0.0, 0.0),
            d: V3::z(),
        };
        for v in [0.0, 1.0] {
            let pc = pcurve::pcurve(&surf, &seam, 0.0, 1.0, Some(P2::new(0.5, v)));
            let pc = seam_side(&surf, &seam, [0.0, 1.0], pc, P2::new(0.5, v));
            let m = pc.point(0.5);
            assert!(
                (m.y - v).abs() < 1e-3,
                "near v = {v}: the p-curve runs at v = {}",
                m.y
            );
        }
    }

    /// A face whose loop's p-curves meet 5e-5 apart at two vertices (beyond the 1e-5 a loop
    /// closes within, a part in 10⁵ of the body's size): the result's vertices there take the
    /// gap as tolerance, and the face closes.
    #[test]
    fn loop_gaps_become_vertex_tolerance() {
        let mut k = NativeKernel::new();
        let mut cube = |x: f64, z: f64| {
            let super::super::draw::Shape::Body(id) = k.box_at(x, 2.0, z, 10.0, 6.0, 10.0).unwrap()
            else {
                unreachable!()
            };
            (**k.get(id).unwrap()).clone()
        };
        let mut a = cube(0.0, 0.0);
        let b = cube(5.0, 5.0);
        for f in 0..a.faces.len() {
            let c = &mut a.faces[f].loops[0][0];
            c.pc =
                c.pc.transformed(&Affine::translation(V3::new(5e-5, 0.0, 0.0)));
        }
        assert!(uv_gap(&a).is_some());
        // (One run at the default tolerance: the retries at coarser ones would hide it.)
        let opts = RunOpts {
            tol: tol::POINT,
            flush: false,
            merge_coplanar: true,
            work_budget: DEFAULT_WORK_BUDGET,
            check_crossings: false,
        };
        let r = run(&a, &b, Mode::Bool(BoolOp::Union), opts).unwrap();
        assert_eq!(r.body.solids.len(), 1);
        assert!(uv_gap(&r.body).is_none());
    }

    /// A ring revolved all but a hair round (its start and end caps 1e-6 apart) closes on
    /// itself: the caps dropped, their edges glued, one closed solid (Dr Browns cap's blend
    /// tool, Fillet 1).
    #[test]
    fn touching_caps_glue_into_a_ring() {
        use crate::{Axis, Curve2, Kernel, Loop, Plane, Profile, Region};
        let mut k = NativeKernel::new();
        let (r0, r1, z1) = (10.0, 12.0, 3.0);
        let pt = |x: f64, y: f64| nalgebra::Point2::new(x, y);
        let corners = [pt(r0, 0.0), pt(r1, 0.0), pt(r1, z1), pt(r0, z1)];
        let curves = (0..4)
            .map(|i| Curve2::Line {
                a: corners[i],
                b: corners[(i + 1) % 4],
                source: None,
            })
            .collect();
        let region = Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        };
        // The XZ plane, in-plane y along +Z.
        let front = Plane {
            origin: P3::origin(),
            x_dir: nalgebra::Vector3::x_axis(),
            normal: -nalgebra::Vector3::y_axis(),
        };
        let axis = Axis {
            origin: P3::origin(),
            dir: nalgebra::Vector3::z_axis(),
        };
        let id = k
            .revolve(&Profile::new(front, vec![region]), axis, TAU - 1e-6 / r1)
            .unwrap()
            .bodies[0];
        let ring = (**k.get(id).unwrap()).clone();
        assert_eq!(
            ring.faces
                .iter()
                .filter(|f| matches!(f.surf, Surface::Plane { .. }))
                .count(),
            4
        );
        let (glued, keep) = glue_touching_caps(&ring, tol::POINT).expect("the caps glue");
        assert_eq!(glued.faces.len(), ring.faces.len() - 2);
        assert_eq!(keep.len(), glued.faces.len());
        assert_eq!(
            (glued.solids.len(), glued.shells.iter().all(|s| s.closed)),
            (1, true)
        );
        assert_eq!(glued.check(), Vec::<String>::new());
        // A body of two solids (boxes touching face to face) has no caps to glue.
        let cube = |k: &mut NativeKernel, x: f64| {
            let super::super::draw::Shape::Body(id) = k.box_at(x, 0.0, 0.0, 1.0, 1.0, 1.0).unwrap()
            else {
                unreachable!()
            };
            (**k.get(id).unwrap()).clone()
        };
        let mut two = cube(&mut k, 0.0);
        two.append(&cube(&mut k, 1.0));
        assert!(glue_touching_caps(&two, tol::POINT).is_none());
    }

    /// A tool carrying a closed profile gap of 7.1e-5 as its edges' tolerance (squish bottle
    /// adapter, Extrude 6) united with a target whose wall steps in by that much part of the
    /// way round: within the fuzzy value the two walls are one, the step's sliver goes, and the
    /// union is one valid solid.
    #[test]
    fn fuzzy_union_merges_a_stepped_wall() {
        let mut k = NativeKernel::new();
        let (r, gap) = (18.375, 7.1e-5);
        // (A sector of `degrees`, all the way round at 360.)
        let cyl = |k: &mut NativeKernel, r: f64, z: f64, h: f64, degrees: f64, from: f64| {
            let x = V3::new(from.to_radians().cos(), from.to_radians().sin(), 0.0);
            let ax = super::super::draw::Ax3 {
                origin: P3::new(0.0, 0.0, z),
                z: V3::z(),
                x,
            };
            let super::super::draw::Shape::Body(id) =
                k.pcylinder_a(Some(&ax), r, h, degrees).unwrap()
            else {
                unreachable!()
            };
            (**k.get(id).unwrap()).clone()
        };
        // The target: a full cylinder above; below, a third of the way round a gap smaller,
        // the rest as above (two sliver steps a gap wide where they meet).
        let upper = cyl(&mut k, r, 0.0, 10.0, 360.0, 0.0);
        let lower_in = cyl(&mut k, r - gap, -5.0, 5.0, 120.0, 0.0);
        let lower_out = cyl(&mut k, r, -5.0, 5.0, 240.0, 120.0);
        let target = boolean_bodies(&upper, &lower_in, BoolOp::Union)
            .unwrap()
            .body;
        let target = boolean_bodies(&target, &lower_out, BoolOp::Union)
            .unwrap()
            .body;
        assert_eq!(target.solids.len(), 1);
        // The tool: a full cylinder over the step, its edges carrying the gap.
        let mut tool = cyl(&mut k, r, -5.0, 8.0, 360.0, 0.0);
        for e in &mut tool.edges {
            e.tol = gap;
        }
        for v in &mut tool.verts {
            v.tol = gap;
        }
        let out = boolean_bodies(&target, &tool, BoolOp::Union).unwrap().body;
        assert_eq!(out.check(), Vec::<String>::new());
        assert_eq!(out.solids.len(), 1);
        let v = super::super::mass::body_volume(&out);
        let expected = std::f64::consts::PI * r * r * 15.0;
        assert!(
            !(0..out.faces.len()).any(|f| super::super::mass::face_area(&out, f).0 < 1e-2),
            "a sliver face is left"
        );
        assert!(
            (v - expected).abs() < 1e-3 * expected,
            "volume {v} vs {expected}"
        );
    }

    /// The stepped wall of [`fuzzy_union_merges_a_stepped_wall`] united with a tool carrying no
    /// gap: the fuzzy value is the inputs' own tolerance and the union is as without it
    /// (squish bottle adapter, Sweep 2: its walls a gap apart stay two).
    #[test]
    fn exact_union_keeps_a_stepped_wall() {
        let mut k = NativeKernel::new();
        let (r, gap) = (18.375, 7.1e-5);
        let cyl = |k: &mut NativeKernel, r: f64, z: f64, h: f64, degrees: f64, from: f64| {
            let x = V3::new(from.to_radians().cos(), from.to_radians().sin(), 0.0);
            let ax = super::super::draw::Ax3 {
                origin: P3::new(0.0, 0.0, z),
                z: V3::z(),
                x,
            };
            let super::super::draw::Shape::Body(id) =
                k.pcylinder_a(Some(&ax), r, h, degrees).unwrap()
            else {
                unreachable!()
            };
            (**k.get(id).unwrap()).clone()
        };
        let upper = cyl(&mut k, r, 0.0, 10.0, 360.0, 0.0);
        let lower_in = cyl(&mut k, r - gap, -5.0, 5.0, 120.0, 0.0);
        let lower_out = cyl(&mut k, r, -5.0, 5.0, 240.0, 120.0);
        let target = boolean_bodies(&upper, &lower_in, BoolOp::Union)
            .unwrap()
            .body;
        let target = boolean_bodies(&target, &lower_out, BoolOp::Union)
            .unwrap()
            .body;
        assert_eq!(carried_fuzzy(&target, &target), tol::CONFUSION);
        // A ring round the lower part, inside it, exact.
        let ring = cyl(&mut k, r - 1.0, -4.0, 3.0, 360.0, 0.0);
        let out = boolean_bodies(&target, &ring, BoolOp::Union).unwrap().body;
        assert_eq!(out.check(), Vec::<String>::new());
        assert_eq!(out.solids.len(), 1);
        assert_eq!(out.faces.len(), target.faces.len());
    }

    /// Curves running along each other within the tolerance meet in one stretch, its two ends,
    /// however many touches the intersection reports along it; separate crossings stay.
    #[test]
    fn overlap_runs_keep_the_ends() {
        let a = Curve::Line {
            o: P3::origin(),
            d: V3::x(),
        };
        let b = Curve::Line {
            o: P3::new(0.0, 4e-7, 0.0),
            d: V3::x(),
        };
        assert_eq!(
            overlap_runs(&a, &b, vec![(3.0, 3.0), (1.0, 1.0), (2.0, 2.0)], 1e-6),
            vec![1.0, 3.0]
        );
        let c = Curve::Line {
            o: P3::new(0.0, -1.0, 0.0),
            d: V3::new(1.0, 1.0, 0.0),
        };
        assert_eq!(
            overlap_runs(&a, &c, vec![(1.0, 1.0), (5.0, 5.0)], 1e-6),
            vec![1.0, 5.0]
        );
        assert_eq!(overlap_runs(&a, &c, vec![(1.0, 1.0)], 1e-6), vec![1.0]);
    }

    /// Booleans merge faces on one surface by default, and keep them apart with
    /// `merge_coplanar: false` (the DRAW harness's, as OCCT's).
    #[test]
    fn merge_coplanar_option() {
        let cube = |x: f64| {
            let mut k = NativeKernel::new();
            let super::super::draw::Shape::Body(id) = k.box_at(x, 0.0, 0.0, 1.0, 1.0, 1.0).unwrap()
            else {
                unreachable!()
            };
            (**k.get(id).unwrap()).clone()
        };
        let (a, b) = (cube(0.0), cube(1.0));
        let merged = boolean_bodies(&a, &b, BoolOp::Union).unwrap().body;
        assert_eq!(merged.faces.len(), 6);
        let apart = boolean_bodies_with(
            &a,
            &b,
            BoolOp::Union,
            BoolOptions {
                merge_coplanar: false,
                ..BoolOptions::default()
            },
        )
        .unwrap()
        .body;
        assert_eq!(apart.faces.len(), 10);
    }
}
