//! The numeric surface–surface intersection engine: every pair of surfaces that
//! [`super::ssi`] has no closed form for comes here. It works in three stages.
//!
//! 1. Branch finding. Each parameter box is cut into cells (at B-spline knots and at every
//!    quarter turn of an angular parameter), and pairs of cells, one per surface, are
//!    subdivided while their bounding boxes overlap. A pair stops once its two normal cones
//!    hold no pair of parallel normals. A closed loop of the intersection inside the pair would
//!    need one (Sederberg and Meyers' loop test), so every branch through such a pair crosses
//!    the boundary of one of its cells, and the edge–surface crossings found there are the
//!    seeds. A pair that never separates is cut down to a small size and searched the same
//!    way; such pairs come from tangency, a branch point or a pole. A closest-point solve from
//!    the middle of each small pair also finds touching points and loops smaller than a cell.
//!    SMLib's `SmSurfaceIntersector::FindBoundaryStartPoints` / `FindInteriorCurves`
//!    (SmSurfaceIntersector.cpp:1078, :5935) start from the boundary and from leaf pairs of
//!    its patch trees in the same spirit; the normal-cone stop is what bounds the work here.
//! 2. Tracing. From every seed that no traced curve already passes through, the curve is
//!    marched both ways along n_a × n_b. Each step is corrected onto both surfaces by Newton's
//!    method on the four parameters, and the step length adapts to the curve's turning. A march
//!    stops on a's box or the end of b's surface (the end solved exactly on that side; b's
//!    face box bounds only the search, its curves run on to where a's face ends, as a boolean
//!    expects), on a curve traced before, or back at its start (a closed curve, then started
//!    where it crosses a seam or else where the surfaces cross most steeply). Where the
//!    surfaces turn tangent it steps straight on (through a branch point).
//! 3. Tangency. Touching points chain into tangent curves (reported apart); a few together
//!    are a point of contact. Those by the traced curves are solved exactly for the point where
//!    the surfaces meet and are tangent (a branch point, or where two loops touch); every curve
//!    passing one is put through it and cut there, so curves meeting there share the point.

use rayon::prelude::*;

/// The sine of the angle between the normals below which the surfaces are tangent at a point:
/// n_a × n_b gives no direction to march along.
const TANGENT_SIN: f64 = 1e-7;
/// The sine of the angle above which a point found by a closest-point solve is a crossing
/// (marched from) rather than a touching point (such a solve stops within the tolerance, so a
/// point of a tangent curve has its normals a little apart).
const TRANSVERSAL_SIN: f64 = 1e-3;
/// The angle (radians) by which two normal cones must clear each other to count as separated.
const CONE_MARGIN: f64 = 1e-3;
/// The size of a cell (a fraction of the region's) below which a pair isn't cut further.
const TINY: f64 = 2e-3;
/// How far apart the surfaces may be at a point of the intersection, relative to the region
/// (at least that times 1 mm).
const REL_TOL: f64 = 1e-9;

use super::math::*;
use super::surface::Surface;
use super::tol;

/// A rectangle of surface parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvBox {
    pub u: (f64, f64),
    pub v: (f64, f64),
}

impl UvBox {
    pub fn new(u: (f64, f64), v: (f64, f64)) -> Self {
        Self { u, v }
    }

    fn mid(&self) -> (f64, f64) {
        (0.5 * (self.u.0 + self.u.1), 0.5 * (self.v.0 + self.v.1))
    }

    fn du(&self) -> f64 {
        self.u.1 - self.u.0
    }

    fn dv(&self) -> f64 {
        self.v.1 - self.v.0
    }

    fn is_valid(&self) -> bool {
        [self.u.0, self.u.1, self.v.0, self.v.1]
            .iter()
            .all(|x| x.is_finite())
            && self.du() > 0.0
            && self.dv() > 0.0
    }
}

/// A point on both surfaces: in space, on a (u, v) and on b (u, v).
#[derive(Debug, Clone, Copy)]
pub struct Pt {
    pub p: P3,
    pub a: (f64, f64),
    pub b: (f64, f64),
}

/// How the march along one end of a traced curve stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// On the side of a parameter box (solved onto it).
    Boundary,
    /// The surfaces turned tangent there (a branch point or the end of a tangent stretch).
    Singular,
    /// It ran onto a curve traced before.
    Merged,
    /// The step could not be corrected even at the smallest length.
    Stuck,
}

/// A curve of the intersection as points on both surfaces.
#[derive(Debug, Clone)]
pub struct Traced {
    pub pts: Vec<Pt>,
    pub closed: bool,
    /// How each end stopped (start, end); both `Boundary` for a closed curve.
    pub ends: [End; 2],
    /// A tangent curve (the surfaces touch along it without crossing).
    pub tangent: bool,
}

/// The intersection of two surfaces over their parameter boxes, within a region.
#[derive(Debug, Clone, Default)]
pub struct Intersection {
    pub curves: Vec<Traced>,
    /// Isolated points where the surfaces touch.
    pub points: Vec<Pt>,
    /// Points on the curves where the surfaces are tangent (branch points, touching points):
    /// each curve passing one goes through it exactly.
    pub singular: Vec<Pt>,
    /// Whether some part of the surfaces coincides (an area of contact: its boundary is
    /// [`super::ssi_coincident`]'s; what the march finds inside it is rounding, left out).
    pub coincident: bool,
}

/// Counters of the branch search (for tests and profiling).
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub pairs: usize,
    pub seeds: usize,
    pub touches: usize,
}

/// One surface over its parameter box.
struct Dom<'a> {
    s: &'a Surface,
    bx: UvBox,
    /// u (v) is an angle bounded by the box modulo a turn.
    wrap_u: bool,
    wrap_v: bool,
    /// u (v) is an angle and the box spans a whole turn: free.
    free_u: bool,
    free_v: bool,
    /// The surface's signed distance is a true distance (1-Lipschitz), usable for culling.
    exact: bool,
}

impl<'a> Dom<'a> {
    fn new(s: &'a Surface, bx: UvBox) -> Self {
        let (pu, pv) = (s.u_periodic(), s.v_periodic());
        let full = |d: f64| d >= TAU - tol::ANGULAR;
        Dom {
            s,
            bx,
            wrap_u: pu && !full(bx.du()),
            wrap_v: pv && !full(bx.dv()),
            free_u: pu && full(bx.du()),
            free_v: pv && full(bx.dv()),
            // (Not a spindle torus's: its distance is to the nearer of two meridians.)
            exact: matches!(
                s,
                Surface::Plane { .. } | Surface::Cylinder { .. } | Surface::Sphere { .. }
            ) || matches!(s, Surface::Torus { rmaj, rmin, .. } if rmin <= rmaj),
        }
    }

    /// The offset (a whole number of turns) that moves `x` into [lo − e, hi + e] along one
    /// direction, or None when it lies outside.
    fn offset1(x: f64, (lo, hi): (f64, f64), e: f64, wrap: bool, free: bool) -> Option<f64> {
        if free {
            return Some(0.0);
        }
        if wrap {
            let w = super::math::wrap(x, lo - e, TAU);
            return (w <= hi + e).then_some(x - w);
        }
        (x >= lo - e && x <= hi + e).then_some(0.0)
    }

    /// Whether (u, v) lies in the box grown by the fraction `f` of its sides (angles modulo a
    /// turn), with the offsets that bring it into the box's window.
    fn locate(&self, (u, v): (f64, f64), f: f64) -> Option<(f64, f64)> {
        let eu =
            self.bx.du() * f + tol::REL_FINE * (1.0 + self.bx.u.0.abs().max(self.bx.u.1.abs()));
        let ev =
            self.bx.dv() * f + tol::REL_FINE * (1.0 + self.bx.v.0.abs().max(self.bx.v.1.abs()));
        Some((
            Self::offset1(u, self.bx.u, eu, self.wrap_u, self.free_u)?,
            Self::offset1(v, self.bx.v, ev, self.wrap_v, self.free_v)?,
        ))
    }
}

/// A cell of one surface: its parameters, a box sure to hold it, and its normal cone.
#[derive(Debug, Clone, Copy)]
struct Cell {
    uv: UvBox,
    bb: Bbox,
    axis: V3,
    /// The cone's half angle (π: no cone, the normals point every way).
    half: f64,
    size: f64,
    /// Whether the cell is longer along u than along v in space.
    long_u: bool,
    /// The 3 × 3 samples (row by row along u).
    pts: [P3; 9],
    /// How far the cell may stray from the bilinear interpolant of its samples.
    sag: f64,
    /// How far a point of the cell may be from the nearest sample.
    reach: f64,
}

fn angle(a: &V3, b: &V3) -> f64 {
    a.cross(b).norm().atan2(a.dot(b))
}

/// The cell of `s` over `uv`: a 3 × 3 grid of samples, its box grown by twice the bilinear
/// interpolation's error bound (h²/8 · max |S''| per direction, the maximum taken over the
/// samples), and the samples' normals in a cone widened by three quarters of the largest angle
/// between neighbouring samples (the most a normal can stray between them).
fn cell(s: &Surface, uv: UvBox) -> Cell {
    const N: usize = 3;
    let mut bb = Bbox::empty();
    let mut ns = [[V3::zeros(); N]; N];
    let mut ps = [[P3::origin(); N]; N];
    let (mut muu, mut mvv) = (0.0f64, 0.0f64);
    let mut degenerate = false;
    for (i, (prow, nrow)) in ps.iter_mut().zip(ns.iter_mut()).enumerate() {
        for (j, (p, n)) in prow.iter_mut().zip(nrow.iter_mut()).enumerate() {
            let u = uv.u.0 + uv.du() * i as f64 / (N - 1) as f64;
            let v = uv.v.0 + uv.dv() * j as f64 / (N - 1) as f64;
            let d = s.d2(u, v);
            *p = P3::from(d[0]);
            bb.add(p);
            muu = muu.max(d[3].norm());
            mvv = mvv.max(d[5].norm());
            let c = d[1].cross(&d[2]);
            let l = c.norm();
            if l > tol::REL_FINE * (d[1].norm() * d[2].norm()).max(tol::TINY) {
                *n = c / l;
            } else {
                // A pole or an apex: the normal there is a limit, and the cone is wide.
                degenerate = true;
                *n = s.normal(u, v);
            }
        }
    }
    let (hu, hv) = (uv.du() / (N - 1) as f64, uv.dv() / (N - 1) as f64);
    let sag = 2.0 * (hu * hu * muu + hv * hv * mvv) / 8.0;
    let scale =
        (bb.max - bb.min).abs().max() + bb.min.coords.abs().max().max(bb.max.coords.abs().max());
    let bb = bb.grow(sag + tol::REL_FINE * (1.0 + scale));
    let sum: V3 = ns.iter().flatten().sum();
    let (axis, half) = match sum.try_normalize(tol::ANGULAR) {
        Some(axis) if !degenerate => {
            let spread = ns
                .iter()
                .flatten()
                .map(|n| angle(&axis, n))
                .fold(0.0, f64::max);
            let mut step = 0.0f64;
            for i in 0..N {
                for j in 0..N {
                    if i + 1 < N {
                        step = step.max(angle(&ns[i][j], &ns[i + 1][j]));
                    }
                    if j + 1 < N {
                        step = step.max(angle(&ns[i][j], &ns[i][j + 1]));
                    }
                }
            }
            (axis, (spread + 0.75 * step).min(PI))
        }
        _ => (V3::z(), PI),
    };
    let ext_u = (0..N)
        .map(|j| (ps[N - 1][j] - ps[0][j]).norm())
        .fold(0.0, f64::max);
    let ext_v = (0..N)
        .map(|i| (ps[i][N - 1] - ps[i][0]).norm())
        .fold(0.0, f64::max);
    let mut reach = 0.0f64;
    for i in 0..N - 1 {
        for j in 0..N - 1 {
            reach = reach.max(
                0.5 * (ps[i][j] - ps[i + 1][j + 1])
                    .norm()
                    .max((ps[i + 1][j] - ps[i][j + 1]).norm()),
            );
        }
    }
    let mut pts = [P3::origin(); 9];
    for (k, p) in ps.iter().flatten().enumerate() {
        pts[k] = *p;
    }
    Cell {
        uv,
        bb,
        axis,
        half,
        size: bb.diagonal(),
        long_u: ext_u >= ext_v,
        pts,
        sag,
        reach: reach + sag,
    }
}

/// Whether two cells are kept apart along a normal axis of either: each lies in a slab square to
/// the axis (its samples' extent along it, widened by how far it strays from them), and the
/// slabs don't meet. Far stronger than the boxes for the flat cells near a tangency.
fn apart(a: &Cell, b: &Cell, tol: f64) -> bool {
    let slab = |c: &Cell, n: &V3, o: &P3| {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &c.pts {
            let x = (p - o).dot(n);
            lo = lo.min(x);
            hi = hi.max(x);
        }
        (lo - c.sag, hi + c.sag)
    };
    [(a, &a.axis), (b, &b.axis)]
        .into_iter()
        .filter(|(c, _)| c.half < 0.25 * PI)
        .any(|(c, n)| {
            let o = c.pts[4];
            let (a0, a1) = slab(a, n, &o);
            let (b0, b1) = slab(b, n, &o);
            a1 + tol < b0 || b1 + tol < a0
        })
}

/// Whether the exact (1-Lipschitz) distance of `dom` keeps the cell `c` off it: every sample on
/// one side, farther than the cell can stray from its nearest sample; or every sample on one
/// side by more than the distance's own interpolation error over the cell (from its second
/// differences across the samples, as a cell's `sag` is taken from the surface's). The second
/// catches a face running a hair off the surface all along (a thread's root 7e-5 off its tube:
/// the distance is all but constant over it), which the first leaves to be cut down to the
/// smallest cells.
fn off_exact(dom: &Dom, c: &Cell, tol: f64) -> bool {
    if !dom.exact {
        return false;
    }
    let mut g = [0.0; 9];
    for (k, p) in c.pts.iter().enumerate() {
        let Some(x) = dom.s.signed_distance(p) else {
            return false;
        };
        g[k] = x;
    }
    let (lo, hi) = g
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
    // Every sample on one side by more than the cell's reach.
    if lo > c.reach + tol || hi < -(c.reach + tol) {
        return true;
    }
    // Every sample on one side by more than g's interpolation error over the cell: the second
    // differences along u and v are h² g'' there, and the bilinear interpolant of g strays
    // from g by at most (h_u² |g_uu| + h_v² |g_vv|) / 8, taken twice over (as the cell's `sag`
    // is). That bounds it only where g is smooth all over the cell and its curvature changes
    // little across it: the cell well clear of the distance's kinks (else a small hole's axis
    // between two samples goes unseen).
    if lo <= tol && hi >= -tol {
        return false;
    }
    let at = |i: usize, j: usize| g[i * 3 + j];
    let duu = (0..3)
        .map(|j| (at(0, j) - 2.0 * at(1, j) + at(2, j)).abs())
        .fold(0.0, f64::max);
    let dvv = (0..3)
        .map(|i| (at(i, 0) - 2.0 * at(i, 1) + at(i, 2)).abs())
        .fold(0.0, f64::max);
    let slack = (duu + dvv) / 4.0 + tol;
    (lo > slack || hi < -slack)
        && c.pts
            .iter()
            .all(|p| kink_clearance(dom.s, p) > 2.0 * c.size + c.reach)
}

/// How far `p` is from where the exact distance of `s` has a kink (a cylinder's axis, a
/// sphere's centre, a torus's core circle and axis): the distance is smooth only clear of it.
fn kink_clearance(s: &Surface, p: &P3) -> f64 {
    match s {
        Surface::Plane { .. } => f64::INFINITY,
        Surface::Cylinder { f, .. } => {
            let l = f.local(p);
            l.x.hypot(l.y)
        }
        Surface::Sphere { f, .. } => (p - f.o).norm(),
        Surface::Torus { f, rmaj, .. } => {
            let l = f.local(p);
            let rho = l.x.hypot(l.y);
            (rho - rmaj).hypot(l.z).min(rho)
        }
        _ => 0.0,
    }
}

/// The two halves of a cell, cut across its longer side.
fn split(s: &Surface, c: &Cell) -> [Cell; 2] {
    let (m0, m1) = c.uv.mid();
    if c.long_u {
        [
            cell(s, UvBox::new((c.uv.u.0, m0), c.uv.v)),
            cell(s, UvBox::new((m0, c.uv.u.1), c.uv.v)),
        ]
    } else {
        [
            cell(s, UvBox::new(c.uv.u, (c.uv.v.0, m1))),
            cell(s, UvBox::new(c.uv.u, (m1, c.uv.v.1))),
        ]
    }
}

/// Whether no normal of one cell can be parallel to a normal of the other (either way round):
/// then the pair holds no closed loop of the intersection.
fn separated(a: &Cell, b: &Cell) -> bool {
    if a.half >= PI || b.half >= PI {
        return false;
    }
    let between = a
        .axis
        .cross(&b.axis)
        .norm()
        .atan2(a.axis.dot(&b.axis).abs());
    between > a.half + b.half + CONE_MARGIN
}

/// The parameter cuts of one direction: the box's ends, B-spline knots inside it, and every
/// quarter turn of an angle (so cells start out near flat), thinned to at most 24 pieces.
fn cuts(lo: f64, hi: f64, knots: &[f64], angular: bool) -> Vec<f64> {
    let mut xs: Vec<f64> = knots
        .iter()
        .copied()
        .filter(|&k| k > lo + tol::REL_FINE * (hi - lo) && k < hi - tol::REL_FINE * (hi - lo))
        .collect();
    if angular {
        let q = PI / 2.0;
        let mut k = (lo / q).floor() + 1.0;
        while k * q < hi - tol::ROUNDOFF {
            if k * q > lo + tol::ROUNDOFF {
                xs.push(k * q);
            }
            k += 1.0;
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < tol::REL_FINE * (1.0 + a.abs()));
    if xs.len() > 23 {
        let step = xs.len().div_ceil(23);
        xs = xs.into_iter().step_by(step).collect();
    }
    std::iter::once(lo)
        .chain(xs)
        .chain(std::iter::once(hi))
        .collect()
}

/// The starting cells of a surface over a box.
fn initial_cells(s: &Surface, bx: UvBox) -> Vec<UvBox> {
    let (ku, kv): (Vec<f64>, Vec<f64>) = match s {
        Surface::Nurbs(n) => n.breaks(),
        Surface::Extrusion {
            c: super::curve::Curve::Nurbs(n),
            ..
        } => (n.breaks(), vec![]),
        Surface::Revolution {
            m: super::curve::Curve::Nurbs(n),
            ..
        } => (vec![], n.breaks()),
        _ => (vec![], vec![]),
    };
    let ang_u = matches!(
        s,
        Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
            | Surface::Revolution { .. }
    ) || matches!(s, Surface::Extrusion { c, .. } if c.is_periodic());
    let ang_v = matches!(s, Surface::Sphere { .. } | Surface::Torus { .. });
    let us = cuts(bx.u.0, bx.u.1, &ku, ang_u);
    let vs = cuts(bx.v.0, bx.v.1, &kv, ang_v);
    let mut out = Vec::new();
    for wu in us.windows(2) {
        for wv in vs.windows(2) {
            out.push(UvBox::new((wu[0], wu[1]), (wv[0], wv[1])));
        }
    }
    out
}

/// Everything the search and the march share.
struct Ctx<'a> {
    a: Dom<'a>,
    /// b over its face's box: where the search looks for branches.
    b: Dom<'a>,
    /// b over its whole surface: where a march may follow a curve (a's box bounds it).
    bt: Dom<'a>,
    region: Bbox,
    /// The size of the problem (the region's diagonal).
    size: f64,
    /// Cells smaller than this aren't cut further.
    tiny: f64,
    /// How far apart the two surfaces may be at a point of the intersection.
    tol: f64,
}

impl Ctx<'_> {
    fn dom(&self, on_a: bool) -> &Dom<'_> {
        if on_a { &self.a } else { &self.b }
    }
}

/// What a branch search found.
#[derive(Default)]
struct Found {
    seeds: Vec<Pt>,
    touches: Vec<Pt>,
    /// The cells of a found lying on b.
    coincident: Vec<UvBox>,
    pairs: usize,
}

/// The most cell pairs one starting pair may visit; past it, pairs are searched as small ones.
const PAIR_BUDGET: usize = 60_000;

/// The signed distance from `p` to `s` and the foot's parameters (hinted).
fn sdist(s: &Surface, p: &P3, hint: (f64, f64)) -> (f64, (f64, f64)) {
    let uv = s.project(p, Some(hint));
    if let Some(d) = s.signed_distance(p) {
        return (d, uv);
    }
    let d = s.d2(uv.0, uv.1);
    let n = d[1].cross(&d[2]);
    let r = p - P3::from(d[0]);
    match n.try_normalize(tol::TINY) {
        Some(n) => (r.dot(&n), uv),
        None => (r.norm(), uv),
    }
}

/// An iso line of a surface: u = `val` when `fix_u`, else v = `val`, over `range` of the other.
#[derive(Clone, Copy)]
struct Iso {
    fix_u: bool,
    val: f64,
    range: (f64, f64),
}

impl Iso {
    fn at(&self, t: f64) -> (f64, f64) {
        if self.fix_u {
            (self.val, t)
        } else {
            (t, self.val)
        }
    }
}

/// Solves for the point where the iso line `iso` of `s` meets `o`, from the line parameter `t`
/// and `o`'s (u, v): Newton on S(t) = O(u, v).
fn edge_newton(
    s: &Surface,
    iso: Iso,
    mut t: f64,
    o: &Surface,
    mut ouv: (f64, f64),
    tol: f64,
) -> Option<(f64, (f64, f64), P3)> {
    let (fix_u, range) = (iso.fix_u, iso.range);
    let at = |t: f64| iso.at(t);
    let pad = (range.1 - range.0) * 0.05;
    for _ in 0..24 {
        let (u, v) = at(t);
        let ds = s.d2(u, v);
        let dd = o.d2(ouv.0, ouv.1);
        let f = ds[0] - dd[0];
        if f.norm() < tol * 1e-3 {
            break;
        }
        let st = if fix_u { ds[2] } else { ds[1] };
        let m = nalgebra::Matrix3::from_columns(&[st, -dd[1], -dd[2]]);
        let dx = m.lu().solve(&(-f))?;
        if !dx.iter().all(|x| x.is_finite()) {
            return None;
        }
        t = (t + dx[0]).clamp(range.0 - pad, range.1 + pad);
        ouv = (ouv.0 + dx[1], ouv.1 + dx[2]);
    }
    let (u, v) = at(t);
    let p = s.eval(u, v);
    ((p - o.eval(ouv.0, ouv.1)).norm() < tol).then_some((t, ouv, p))
}

/// Seeds where one side of a cell (of a when `on_a`) crosses the other surface near its cell
/// `oc`: sign changes of the signed distance along the side, bisected and then solved exactly,
/// and near-zero minima of it (a side touching the other surface).
fn edge_seeds(
    ctx: &Ctx,
    on_a: bool,
    fix_u: bool,
    val: f64,
    range: (f64, f64),
    oc: &Cell,
    out: &mut Found,
) {
    let (dom, odom) = (ctx.dom(on_a), ctx.dom(!on_a));
    let (s, o) = (dom.s, odom.s);
    let at = |t: f64| if fix_u { (val, t) } else { (t, val) };
    const K: usize = 4;
    // The side's samples first: a side that stays well clear of the other cell is skipped
    // before any projection onto the other surface.
    let ts: [f64; K + 1] =
        std::array::from_fn(|i| range.0 + (range.1 - range.0) * i as f64 / K as f64);
    let ps: [P3; K + 1] = std::array::from_fn(|i| {
        let (u, v) = at(ts[i]);
        s.eval(u, v)
    });
    let mut sb = Bbox::empty();
    for p in &ps {
        sb.add(p);
    }
    let reach = (ps[1] - ps[0]).norm().max((ps[K] - ps[K - 1]).norm()) + ctx.tol;
    if !sb.grow(reach).overlaps(&oc.bb) {
        return;
    }
    let mut hint = oc.uv.mid();
    let mut sm: Vec<(f64, f64, (f64, f64), P3)> = Vec::with_capacity(K + 1);
    for (t, p) in ts.into_iter().zip(ps) {
        let (g, foot) = sdist(o, &p, hint);
        hint = foot;
        sm.push((t, g, foot, p));
    }
    let mut starts: Vec<(f64, (f64, f64))> = Vec::new();
    for i in 1..=K {
        let (t0, g0, f0, _) = sm[i - 1];
        let (t1, g1, _, _) = sm[i];
        if g0 == 0.0 {
            starts.push((t0, f0));
        }
        if (g0 < 0.0) != (g1 < 0.0) && g0 != 0.0 && g1 != 0.0 {
            // Illinois regula falsi on the signed distance.
            let (mut a, mut b, mut ga, mut gb) = (t0, t1, g0, g1);
            let mut h = f0;
            let mut side = 0;
            for _ in 0..60 {
                let m = (a * gb - b * ga) / (gb - ga);
                let m = if m.is_finite() && m > a.min(b) && m < a.max(b) {
                    m
                } else {
                    0.5 * (a + b)
                };
                let (u, v) = at(m);
                let (gm, fm) = sdist(o, &s.eval(u, v), h);
                h = fm;
                if gm.abs() < ctx.tol * 1e-3 || (b - a).abs() < 1e-14 * (1.0 + a.abs()) {
                    a = m;
                    b = m;
                    break;
                }
                if (gm < 0.0) == (ga < 0.0) {
                    a = m;
                    ga = gm;
                    if side == -1 {
                        gb *= 0.5;
                    }
                    side = -1;
                } else {
                    b = m;
                    gb = gm;
                    if side == 1 {
                        ga *= 0.5;
                    }
                    side = 1;
                }
            }
            starts.push((0.5 * (a + b), h));
        }
    }
    // A side touching the other surface between samples: a small minimum of |g|.
    for i in 1..K {
        let (gp, g, gn) = (sm[i - 1].1, sm[i].1, sm[i + 1].1);
        let spacing = (sm[i + 1].3 - sm[i - 1].3).norm();
        if g.abs() < gp.abs()
            && g.abs() < gn.abs()
            && (gp < 0.0) == (gn < 0.0)
            && (g < 0.0) == (gp < 0.0)
            && g.abs() < 0.1 * spacing
        {
            starts.push((sm[i].0, sm[i].2));
        }
    }
    for (t, ouv) in starts {
        let Some((t, ouv, p)) = edge_newton(s, Iso { fix_u, val, range }, t, o, ouv, ctx.tol)
        else {
            continue;
        };
        if t < range.0 - (range.1 - range.0) * tol::REL
            || t > range.1 + (range.1 - range.0) * tol::REL
        {
            continue;
        }
        if odom.locate(ouv, tol::ROUNDOFF).is_none() || !ctx.region.contains(&p) {
            continue;
        }
        let own = at(t.clamp(range.0, range.1));
        let pt = if on_a {
            Pt { p, a: own, b: ouv }
        } else {
            Pt { p, a: ouv, b: own }
        };
        out.seeds.push(pt);
    }
}

/// Seeds on the sides of both cells of a pair.
fn boundary_seeds(ctx: &Ctx, ca: &Cell, cb: &Cell, out: &mut Found) {
    for (on_a, c, oc) in [(true, ca, cb), (false, cb, ca)] {
        let r = c.uv;
        edge_seeds(ctx, on_a, true, r.u.0, r.v, oc, out);
        edge_seeds(ctx, on_a, true, r.u.1, r.v, oc, out);
        edge_seeds(ctx, on_a, false, r.v.0, r.u, oc, out);
        edge_seeds(ctx, on_a, false, r.v.1, r.u, oc, out);
    }
}

/// Gauss–Newton (minimum-norm steps) for a point on both surfaces from (a, b): None unless the
/// surfaces meet there within the tolerance.
fn meet(ctx: &Ctx, mut a: (f64, f64), mut b: (f64, f64)) -> Option<Pt> {
    let (sa, sb) = (ctx.a.s, ctx.b.s);
    for _ in 0..40 {
        let da = sa.d2(a.0, a.1);
        let db = sb.d2(b.0, b.1);
        let f = da[0] - db[0];
        if f.norm() < ctx.tol * 1e-3 {
            break;
        }
        let j = nalgebra::Matrix3x4::from_columns(&[da[1], da[2], -db[1], -db[2]]);
        let jjt = j * j.transpose();
        let lambda = 1e-14 * jjt.trace().max(tol::TINY);
        let y = (jjt + nalgebra::Matrix3::identity() * lambda)
            .lu()
            .solve(&(-f))?;
        let dx = j.transpose() * y;
        if !dx.iter().all(|x| x.is_finite()) {
            return None;
        }
        a = (a.0 + dx[0], a.1 + dx[1]);
        b = (b.0 + dx[2], b.1 + dx[3]);
    }
    let p = sa.eval(a.0, a.1);
    ((p - sb.eval(b.0, b.1)).norm() < ctx.tol).then_some(Pt { p, a, b })
}

/// A small pair that never separated: a closest-point solve from its middle. A transversal
/// point found is a seed (a loop smaller than the cells, or a branch through them), a tangent
/// one a touching point.
fn touch(ctx: &Ctx, ca: &Cell, cb: &Cell, out: &mut Found) {
    let Some(pt) = meet(ctx, ca.uv.mid(), cb.uv.mid()) else {
        return;
    };
    let inside = |c: &Cell, uv: (f64, f64)| {
        let r = c.uv;
        uv.0 >= r.u.0 - r.du() * 0.5
            && uv.0 <= r.u.1 + r.du() * 0.5
            && uv.1 >= r.v.0 - r.dv() * 0.5
            && uv.1 <= r.v.1 + r.dv() * 0.5
    };
    if !inside(ca, pt.a)
        || !inside(cb, pt.b)
        || !ctx.region.contains(&pt.p)
        || ctx.a.locate(pt.a, tol::ROUNDOFF).is_none()
        || ctx.b.locate(pt.b, tol::ROUNDOFF).is_none()
    {
        return;
    }
    if transversal(ctx, &pt) {
        out.seeds.push(pt);
    } else {
        out.touches.push(pt);
    }
}

/// Whether the cell of a lies on b (all its samples within a hair of it): an area of contact.
fn coincident(ctx: &Ctx, ca: &Cell, cb: &Cell) -> bool {
    let lim = super::tol::CONFUSION.max(tol::REL * ctx.size);
    let mut hint = cb.uv.mid();
    for i in 0..3 {
        for j in 0..3 {
            let u = ca.uv.u.0 + ca.uv.du() * i as f64 / 2.0;
            let v = ca.uv.v.0 + ca.uv.dv() * j as f64 / 2.0;
            let (g, f) = sdist(ctx.b.s, &ctx.a.s.eval(u, v), hint);
            if g.abs() > lim {
                return false;
            }
            hint = f;
        }
    }
    true
}

/// Searches a starting pair of cells (depth first, in a fixed order).
fn search(ctx: &Ctx, ca: Cell, cb: Cell) -> Found {
    let mut out = Found::default();
    let mut stack = vec![(ca, cb)];
    while let Some((ca, cb)) = stack.pop() {
        out.pairs += 1;
        if !ca.bb.grow(ctx.tol).overlaps(&cb.bb)
            || !ca.bb.overlaps(&ctx.region)
            || !cb.bb.overlaps(&ctx.region)
        {
            continue;
        }
        // A true distance from the middle of one cell's box to the other surface bounds how
        // near the cell can come to it.
        let culled = |dom: &Dom, c: &Cell| {
            dom.exact
                && dom
                    .s
                    .signed_distance(&c.bb.center())
                    .is_some_and(|d| d.abs() > 0.5 * c.size + ctx.tol)
        };
        if culled(&ctx.b, &ca)
            || culled(&ctx.a, &cb)
            || off_exact(&ctx.b, &ca, ctx.tol)
            || off_exact(&ctx.a, &cb, ctx.tol)
            || apart(&ca, &cb, ctx.tol)
        {
            continue;
        }
        let sep = separated(&ca, &cb);
        let small = (ca.size <= ctx.tiny && cb.size <= ctx.tiny) || out.pairs > PAIR_BUDGET;
        if sep || small {
            let first = out.seeds.len();
            boundary_seeds(ctx, &ca, &cb, &mut out);
            if !sep {
                // Where the surfaces turn tangent a crossing found is a touching point.
                let (keep, touching): (Vec<Pt>, Vec<Pt>) =
                    out.seeds.drain(first..).partition(|p| transversal(ctx, p));
                out.seeds.extend(keep);
                out.touches.extend(touching);
            }
            if !sep {
                touch(ctx, &ca, &cb, &mut out);
            }
            continue;
        }
        if ca.size > 8.0 * ctx.tiny && cb.size > 8.0 * ctx.tiny && coincident(ctx, &ca, &cb) {
            out.coincident.push(ca.uv);
            continue;
        }
        if ca.size >= cb.size {
            let [c1, c2] = split(ctx.a.s, &ca);
            stack.push((c2, cb));
            stack.push((c1, cb));
        } else {
            let [c1, c2] = split(ctx.b.s, &cb);
            stack.push((ca, c2));
            stack.push((ca, c1));
        }
    }
    out
}

/// Whether the surfaces cross at a point at more than a thousandth of a radian: marching
/// can follow the curve from there. (A point of a tangent curve found by a solve that stops
/// within the tolerance has normals a little apart.)
fn transversal(ctx: &Ctx, p: &Pt) -> bool {
    let na = ctx.a.s.normal(p.a.0, p.a.1);
    let nb = ctx.b.s.normal(p.b.0, p.b.1);
    na.cross(&nb).norm() > TRANSVERSAL_SIN
}

/// The unit tangent of the intersection at a point (n_a × n_b), None where the surfaces are
/// tangent.
fn tangent(ctx: &Ctx, p: &Pt) -> Option<V3> {
    let na = ctx.a.s.normal(p.a.0, p.a.1);
    let nb = ctx.b.s.normal(p.b.0, p.b.1);
    na.cross(&nb).try_normalize(TANGENT_SIN)
}

/// A further condition on a point of both surfaces: its value at the point (and a's point
/// there), and its gradient in (u_a, v_a, u_b, v_b).
type Extra<'a> = dyn Fn(&Pt, &P3) -> (f64, [f64; 4]) + 'a;

/// Newton for a point on both surfaces that also satisfies `extra` = 0 (its value and its
/// gradient in (u_a, v_a, u_b, v_b)); the 4 × 4 system through its normal equations, since
/// the first three rows turn dependent where the surfaces are nearly tangent.
fn correct(ctx: &Ctx, s: Pt, extra: &Extra<'_>) -> Option<Pt> {
    solve(ctx.a.s, ctx.b.s, ctx.tol, s, extra)
}

/// The point of both surfaces on the plane through `q` square to the chord `p0`–`p1` (two
/// points of a traced curve), from their parameters' average: where a fitted curve between
/// them should pass.
pub fn refine_between(a: &Surface, b: &Surface, p0: &Pt, p1: &Pt, q: &P3) -> Option<Pt> {
    let d = (p1.p - p0.p).try_normalize(tol::TINY)?;
    let mid = |x: (f64, f64), y: (f64, f64)| (0.5 * (x.0 + y.0), 0.5 * (x.1 + y.1));
    let start = Pt {
        p: *q,
        a: mid(p0.a, p1.a),
        b: mid(p0.b, p1.b),
    };
    let tol = tol::REL * (1.0 + q.coords.abs().max());
    let plane = |x: &Pt, pa: &P3| {
        let (_, su, sv) = a.d1(x.a.0, x.a.1);
        ((pa - q).dot(&d), [su.dot(&d), sv.dot(&d), 0.0, 0.0])
    };
    solve(a, b, tol, start, &plane)
}

/// [`correct`] on two surfaces with a tolerance.
fn solve(sa: &Surface, sb: &Surface, tol: f64, mut s: Pt, extra: &Extra<'_>) -> Option<Pt> {
    for _ in 0..20 {
        let (pa, au, av) = sa.d1(s.a.0, s.a.1);
        let (pb, bu, bv) = sb.d1(s.b.0, s.b.1);
        let d = pa - pb;
        let (g, gr) = extra(&s, &pa);
        if d.norm() < tol * 1e-3 && g.abs() < tol * 1e-3 {
            s.p = pa;
            return Some(s);
        }
        #[rustfmt::skip]
        let j = nalgebra::Matrix4::new(
            au.x, av.x, -bu.x, -bv.x,
            au.y, av.y, -bu.y, -bv.y,
            au.z, av.z, -bu.z, -bv.z,
            gr[0], gr[1], gr[2], gr[3],
        );
        let f = nalgebra::Vector4::new(-d.x, -d.y, -d.z, -g);
        let jt = j.transpose();
        let dx = (jt * j).try_inverse().map(|inv| inv * jt * f)?;
        if !dx.iter().all(|x| x.is_finite()) {
            return None;
        }
        s.a = (s.a.0 + dx[0], s.a.1 + dx[1]);
        s.b = (s.b.0 + dx[2], s.b.1 + dx[3]);
    }
    let (pa, pb) = (sa.eval(s.a.0, s.a.1), sb.eval(s.b.0, s.b.1));
    if (pa - pb).norm() < tol {
        s.p = pa;
        Some(s)
    } else {
        None
    }
}

/// Whether `p` (heading along `t`) lies on a curve traced before.
fn on_traced(ctx: &Ctx, p: &P3, t: Option<&V3>, traced: &[(Traced, Bbox)]) -> bool {
    traced_at(ctx, p, t, traced).is_some()
}

/// The curve traced before that `p` (heading along `t`) lies on, and the segment.
fn traced_at(
    ctx: &Ctx,
    p: &P3,
    t: Option<&V3>,
    traced: &[(Traced, Bbox)],
) -> Option<(usize, usize)> {
    let floor = 1e-7 * ctx.size + ctx.tol;
    traced.iter().enumerate().find_map(|(i, (c, bb))| {
        if !bb.contains(p) {
            return None;
        }
        let n = c.pts.len();
        let segs = if c.closed { n } else { n.saturating_sub(1) };
        (0..segs)
            .find(|&k| {
                let (a, b) = (&c.pts[k].p, &c.pts[(k + 1) % n].p);
                let ab = b - a;
                let l = ab.norm();
                seg_dist3(p, a, b) < (0.02 * l).max(floor)
                    && t.is_none_or(|t| l < tol::TINY || (t.dot(&ab) / l).abs() > 0.9)
            })
            .map(|k| (i, k))
    })
}

/// Distance from p to the segment ab.
fn seg_dist3(p: &P3, a: &P3, b: &P3) -> f64 {
    let d = b - a;
    let l = d.norm_squared();
    let t = if l < tol::TINY {
        0.0
    } else {
        ((p - a).dot(&d) / l).clamp(0.0, 1.0)
    };
    (a + d * t - p).norm()
}

/// Where the step from `s` (inside both boxes) to `out` (outside one) leaves them: the side
/// crossed first, fixed, and Newton on the rest.
fn boundary_end(ctx: &Ctx, s: &Pt, out: &Pt) -> Option<Pt> {
    // (fraction, coordinate 0..4, the side's value in the march's unwrapped coordinates)
    let mut best: Option<(f64, usize, f64)> = None;
    for (k, dom) in [(0usize, &ctx.a), (2usize, &ctx.bt)] {
        let (from, to) = if k == 0 { (s.a, out.a) } else { (s.b, out.b) };
        let Some(off) = dom.locate(from, tol::POINT) else {
            continue;
        };
        for (axis, (x0, x1), lo, hi, free) in [
            (0usize, (from.0, to.0), dom.bx.u.0, dom.bx.u.1, dom.free_u),
            (1usize, (from.1, to.1), dom.bx.v.0, dom.bx.v.1, dom.free_v),
        ] {
            if free {
                continue;
            }
            let o = if axis == 0 { off.0 } else { off.1 };
            let d = x1 - x0;
            if d.abs() < tol::TINY {
                continue;
            }
            for side in [lo + o, hi + o] {
                // (A curve running along a side, as where both surfaces end on one rim,
                // doesn't leave by it.)
                if (x0 - side).abs() <= tol::REL * (hi - lo) {
                    continue;
                }
                let f = (side - x0) / d;
                if (-tol::REL..=1.0).contains(&f) && best.is_none_or(|b| f < b.0) {
                    best = Some((f.max(0.0), k + axis, side));
                }
            }
        }
    }
    let (f, coord, val) = best?;
    let lerp = |x: (f64, f64), y: (f64, f64)| (x.0 + (y.0 - x.0) * f, x.1 + (y.1 - x.1) * f);
    let mut g = Pt {
        p: s.p + (out.p - s.p) * f,
        a: lerp(s.a, out.a),
        b: lerp(s.b, out.b),
    };
    match coord {
        0 => g.a.0 = val,
        1 => g.a.1 = val,
        2 => g.b.0 = val,
        _ => g.b.1 = val,
    }
    let fixed = move |x: &Pt, _: &P3| -> (f64, [f64; 4]) {
        let cur = [x.a.0, x.a.1, x.b.0, x.b.1][coord];
        let mut gr = [0.0; 4];
        gr[coord] = 1.0;
        (cur - val, gr)
    };
    correct(ctx, g, &fixed)
}

/// Where a closed curve crosses the seam of a surface whose box spans a whole turn: the index
/// the point on it goes at, and the point (None when it is the point already there).
fn seam_crossing(ctx: &Ctx, pts: &[Pt]) -> Option<(usize, Option<Pt>)> {
    let coord = |p: &Pt, i: usize| [p.a.0, p.a.1, p.b.0, p.b.1][i];
    // Only a crossing where the surfaces cross well (not by a point where they touch, where
    // the curve's points crowd and its fit would overshoot).
    let steep = |p: &Pt| {
        ctx.a
            .s
            .normal(p.a.0, p.a.1)
            .cross(&ctx.b.s.normal(p.b.0, p.b.1))
            .norm()
    };
    let best = pts.iter().map(steep).fold(0.0, f64::max);
    let free = [
        (ctx.a.free_u, ctx.a.bx.u.0),
        (ctx.a.free_v, ctx.a.bx.v.0),
        (ctx.bt.free_u, ctx.bt.bx.u.0),
        (ctx.bt.free_v, ctx.bt.bx.v.0),
    ];
    for (i, &(is_free, lo)) in free.iter().enumerate() {
        if !is_free {
            continue;
        }
        for k in 0..pts.len().saturating_sub(1) {
            let (x0, x1) = (coord(&pts[k], i), coord(&pts[k + 1], i));
            let (n0, n1) = (((x0 - lo) / TAU).floor(), ((x1 - lo) / TAU).floor());
            if n0 == n1 {
                continue;
            }
            let val = lo + n0.max(n1) * TAU;
            let f = ((val - x0) / (x1 - x0)).clamp(0.0, 1.0);
            let lerp =
                |x: (f64, f64), y: (f64, f64)| (x.0 + (y.0 - x.0) * f, x.1 + (y.1 - x.1) * f);
            let mut g = Pt {
                p: pts[k].p + (pts[k + 1].p - pts[k].p) * f,
                a: lerp(pts[k].a, pts[k + 1].a),
                b: lerp(pts[k].b, pts[k + 1].b),
            };
            match i {
                0 => g.a.0 = val,
                1 => g.a.1 = val,
                2 => g.b.0 = val,
                _ => g.b.1 = val,
            }
            let fixed = move |x: &Pt, _: &P3| -> (f64, [f64; 4]) {
                let mut gr = [0.0; 4];
                gr[i] = 1.0;
                (coord(x, i) - val, gr)
            };
            if let Some(q) = correct(ctx, g, &fixed).filter(|q| steep(q) > 0.25 * best) {
                if (q.p - pts[k].p).norm() > ctx.tol && (q.p - pts[k + 1].p).norm() > ctx.tol {
                    return Some((k + 1, Some(q)));
                }
                // On a point already: start there.
                return Some((
                    if (q.p - pts[k].p).norm() <= ctx.tol {
                        k
                    } else {
                        k + 1
                    },
                    None,
                ));
            }
        }
    }
    None
}

/// Marches the curve through `seed` both ways.
fn march(ctx: &Ctx, seed: Pt, traced: &[(Traced, Bbox)]) -> Option<Traced> {
    let start = correct(ctx, seed, &|_, _| (0.0, [0.0; 4]))?;
    if ctx.a.locate(start.a, tol::ROUNDOFF).is_none()
        || ctx.bt.locate(start.b, tol::ROUNDOFF).is_none()
    {
        return None;
    }
    let t0 = tangent(ctx, &start)?;
    let h_max = ctx.size / 40.0;
    let h_min = ctx.size * 1e-7;
    let mut halves: [Vec<Pt>; 2] = [Vec::new(), Vec::new()];
    let mut ends = [End::Stuck; 2];
    for (side, sign) in [1.0, -1.0].into_iter().enumerate() {
        let mut s = start;
        let mut t_prev = t0 * sign;
        let mut h = h_max / 8.0;
        let mut out = Vec::new();
        let mut end = End::Stuck;
        // Steps taken straight on through a point where the surfaces are tangent (a branch
        // point, or where the curve touches): the curve goes on the same way beyond it.
        let mut blind = 0;
        for _ in 0..50_000 {
            let t = match tangent(ctx, &s) {
                Some(t) => {
                    blind = 0;
                    if t.dot(&t_prev) < 0.0 { -t } else { t }
                }
                None if blind < 4 && !out.is_empty() => {
                    blind += 1;
                    t_prev
                }
                None => {
                    end = End::Singular;
                    break;
                }
            };
            let target = s.p + t * h;
            let p_from = s.p;
            let (sa, sb) = (ctx.a.s, ctx.b.s);
            // The step keeps its length along t: (q − p)·t = h, its gradient through a's
            // derivatives.
            let step = |x: &Pt, q: &P3| {
                let (_, qu, qv) = sa.d1(x.a.0, x.a.1);
                ((q - p_from).dot(&t) - h, [qu.dot(&t), qv.dot(&t), 0.0, 0.0])
            };
            // Predict: each surface's parameters moved along t's components.
            let move_on = |s2: &Surface, uv: (f64, f64)| {
                let (_, su, sv) = s2.d1(uv.0, uv.1);
                let d = t * h;
                let (du, dv) = solve2(
                    su.dot(&su),
                    su.dot(&sv),
                    su.dot(&sv),
                    sv.dot(&sv),
                    su.dot(&d),
                    sv.dot(&d),
                )
                .unwrap_or((0.0, 0.0));
                // An angle moves by less than half a turn (by a pole, as a cone's apex, where the
                // angle's derivative vanishes, the solve gives any amount: unbounded, the angle
                // grows past where its cosine resolves the step, and the march sticks).
                let turn = |x: f64, periodic: bool| {
                    if periodic && x.is_finite() {
                        super::math::wrap(x, -PI, TAU)
                    } else {
                        x
                    }
                };
                (
                    uv.0 + turn(du, s2.u_periodic()),
                    uv.1 + turn(dv, s2.v_periodic()),
                )
            };
            let pred = Pt {
                p: target,
                a: move_on(sa, s.a),
                b: move_on(sb, s.b),
            };
            let next = correct(ctx, pred, &step).filter(|n| (n.p - target).norm() < 0.5 * h);
            let Some(next) = next else {
                if h <= h_min {
                    break;
                }
                h = (h * 0.5).max(h_min);
                continue;
            };
            // Turning too fast: a shorter step.
            let t_next = tangent(ctx, &next).map(|x| if x.dot(&t) < 0.0 { -x } else { x });
            let turn = t_next.map_or(0.0, |x| angle(&x, &t));
            if turn > 0.1 && h > h_min {
                h = (h * 0.5).max(h_min);
                continue;
            }
            // Leaving a box: the end on its side.
            if ctx.a.locate(next.a, tol::ROUNDOFF).is_none()
                || ctx.bt.locate(next.b, tol::ROUNDOFF).is_none()
            {
                match boundary_end(ctx, &s, &next) {
                    Some(e) if (e.p - s.p).norm() > ctx.tol => out.push(e),
                    // The side not solved from this far, or solved where this point already is
                    // (the curve grazing a side it doesn't leave by): a shorter step first.
                    _ if h > h_min => {
                        h = (h * 0.5).max(h_min);
                        continue;
                    }
                    _ => {}
                }
                end = End::Boundary;
                break;
            }
            // Back at the start: closed.
            if side == 0
                && out.len() > 2
                && (next.p - start.p).norm() < 0.6 * h
                && (start.p - s.p).dot(&t) > 0.0
            {
                let mut pts: Vec<Pt> = std::iter::once(start).chain(out).collect();
                // Start a closed curve where it crosses the seam of a surface closed all
                // round (a boolean has a vertex there already), else where the surfaces cross
                // most steeply (far from any branch point, where its seam is best conditioned).
                match seam_crossing(ctx, &pts) {
                    Some((k, q)) => {
                        if let Some(q) = q {
                            pts.insert(k, q);
                        }
                        let n = pts.len();
                        pts.rotate_left(k % n);
                    }
                    None => {
                        let steep = |p: &Pt| {
                            ctx.a
                                .s
                                .normal(p.a.0, p.a.1)
                                .cross(&ctx.b.s.normal(p.b.0, p.b.1))
                                .norm()
                        };
                        let k = (0..pts.len())
                            .max_by(|&x, &y| steep(&pts[x]).total_cmp(&steep(&pts[y])))
                            .unwrap_or(0);
                        pts.rotate_left(k);
                    }
                }
                return Some(Traced {
                    pts,
                    closed: true,
                    ends: [End::Boundary; 2],
                    tangent: false,
                });
            }
            out.push(next);
            if let Some((i, k)) = traced_at(ctx, &next.p, Some(&t), traced) {
                // Run onto a curve traced before by its end (from a seed it didn't reach):
                // end exactly on that end, when it is within this step, so the two join.
                let c = &traced[i].0;
                let n = c.pts.len();
                if !c.closed && (k == 0 || k + 2 == n) {
                    let e = if k == 0 { c.pts[0] } else { c.pts[n - 1] };
                    if let Some(last) = out
                        .last_mut()
                        .filter(|_| (e.p - s.p).norm() <= h * 1.01 && (e.p - s.p).dot(&t) > 0.0)
                    {
                        *last = e;
                    }
                }
                end = End::Merged;
                break;
            }
            t_prev = t;
            s = next;
            if turn < 0.03 {
                h = (h * 1.5).min(h_max);
            }
        }
        halves[side] = out;
        ends[side] = end;
    }
    let [fwd, mut back] = halves;
    back.reverse();
    let pts: Vec<Pt> = back
        .into_iter()
        .chain(std::iter::once(start))
        .chain(fwd)
        .collect();
    (pts.len() >= 2).then_some(Traced {
        pts,
        closed: false,
        ends: [ends[1], ends[0]],
        tangent: false,
    })
}

fn bbox_of(pts: &[Pt], grow: f64) -> Bbox {
    let mut bb = Bbox::empty();
    for p in pts {
        bb.add(&p.p);
    }
    bb.grow(grow)
}

/// Chains touching points into tangent curves: points nearer than `link` belong to one chain,
/// ordered from one end by nearest neighbours. A chain of fewer than four points is a point of
/// contact (its middle one).
fn chain_touches(pts: Vec<Pt>, link: f64, merge: f64) -> (Vec<Traced>, Vec<Pt>) {
    use std::collections::HashMap;
    let key = |p: &P3, h: f64| {
        (
            (p.x / h).floor() as i64,
            (p.y / h).floor() as i64,
            (p.z / h).floor() as i64,
        )
    };
    let around = |k: (i64, i64, i64)| {
        (-1..=1).flat_map(move |i| {
            (-1..=1).flat_map(move |j| (-1..=1).map(move |l| (k.0 + i, k.1 + j, k.2 + l)))
        })
    };
    // Drop duplicates (a grid of cells the merge distance wide).
    let mut uniq: Vec<Pt> = Vec::new();
    let mut seen: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    let merge = merge.max(tol::TINY);
    for p in pts {
        let k = key(&p.p, merge);
        if around(k).any(|c| {
            seen.get(&c)
                .is_some_and(|v| v.iter().any(|&i| (uniq[i].p - p.p).norm() < merge))
        }) {
            continue;
        }
        seen.entry(k).or_default().push(uniq.len());
        uniq.push(p);
    }
    // Components of the points nearer than `link`.
    let link = link.max(tol::TINY);
    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, p) in uniq.iter().enumerate() {
        grid.entry(key(&p.p, link)).or_default().push(i);
    }
    let n = uniq.len();
    let mut comp = vec![usize::MAX; n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..n {
        if comp[i] != usize::MAX {
            continue;
        }
        let g = groups.len();
        let mut stack = vec![i];
        comp[i] = g;
        let mut members = Vec::new();
        while let Some(k) = stack.pop() {
            members.push(k);
            for c in around(key(&uniq[k].p, link)) {
                for &j in grid.get(&c).map(Vec::as_slice).unwrap_or(&[]) {
                    if comp[j] == usize::MAX && (uniq[j].p - uniq[k].p).norm() < link {
                        comp[j] = g;
                        stack.push(j);
                    }
                }
            }
        }
        members.sort_unstable();
        groups.push(members);
    }
    let (mut curves, mut points) = (Vec::new(), Vec::new());
    for members in groups {
        if members.len() < 4 {
            points.push(uniq[members[members.len() / 2]]);
            continue;
        }
        // Start from the member farthest from the centroid, then nearest neighbours.
        let c = members.iter().map(|&k| uniq[k].p.coords).sum::<V3>() / members.len() as f64;
        let first = members
            .iter()
            .copied()
            .max_by(|&x, &y| {
                (uniq[x].p.coords - c)
                    .norm()
                    .total_cmp(&(uniq[y].p.coords - c).norm())
            })
            .unwrap_or(members[0]);
        let mut left: Vec<usize> = members.iter().copied().filter(|&k| k != first).collect();
        let mut order = vec![first];
        while !left.is_empty() {
            let last = uniq[*order.last().unwrap_or(&first)].p;
            let (bi, _) = left
                .iter()
                .enumerate()
                .map(|(i, &k)| (i, (uniq[k].p - last).norm()))
                .fold(
                    (0, f64::INFINITY),
                    |acc, x| if x.1 < acc.1 { x } else { acc },
                );
            order.push(left.swap_remove(bi));
        }
        let pts: Vec<Pt> = order.iter().map(|&k| uniq[k]).collect();
        // A curve's chain runs on: one that folds back often is points scattered over a patch
        // where the surfaces nearly touch, not a tangent curve.
        let folds = pts
            .windows(3)
            .filter(|w| (w[1].p - w[0].p).dot(&(w[2].p - w[1].p)) < 0.0)
            .count();
        if folds * 10 > pts.len() {
            continue;
        }
        let closed = pts.len() > 4 && (pts[0].p - pts[pts.len() - 1].p).norm() < link;
        curves.push(Traced {
            pts,
            closed,
            ends: [End::Singular; 2],
            tangent: true,
        });
    }
    (curves, points)
}

/// The intersection of `a` over `ua` and `b` over `ub` inside `region`.
pub fn intersect(a: &Surface, ua: UvBox, b: &Surface, ub: UvBox, region: &Bbox) -> Intersection {
    intersect_stats(a, ua, b, ub, region).0
}

/// [`intersect`] with the search's counters.
pub fn intersect_stats(
    a: &Surface,
    ua: UvBox,
    b: &Surface,
    ub: UvBox,
    region: &Bbox,
) -> (Intersection, Stats) {
    let mut stats = Stats::default();
    if !ua.is_valid() || !ub.is_valid() || region.is_empty() {
        if std::env::var_os("CADRS_SSI_DEBUG").is_some() {
            eprintln!("ssi_numeric: nothing to search ({ua:?}, {ub:?}, region {region:?})");
        }
        return (Intersection::default(), stats);
    }
    let cells_a: Vec<Cell> = initial_cells(a, ua)
        .into_iter()
        .map(|r| cell(a, r))
        .collect();
    let cells_b: Vec<Cell> = initial_cells(b, ub)
        .into_iter()
        .map(|r| cell(b, r))
        .collect();
    // The region: the given one where both surfaces' cells can be.
    let (mut ba, mut bbx) = (Bbox::empty(), Bbox::empty());
    cells_a.iter().for_each(|c| ba.union(&c.bb));
    cells_b.iter().for_each(|c| bbx.union(&c.bb));
    let mut r = *region;
    r.min = r.min.sup(&ba.min).sup(&bbx.min);
    r.max = r.max.inf(&ba.max).inf(&bbx.max);
    if r.is_empty() || r.min.y > r.max.y || r.min.z > r.max.z {
        if std::env::var_os("CADRS_SSI_DEBUG").is_some() {
            eprintln!("ssi_numeric: the boxes don't meet in the region ({r:?})");
        }
        return (Intersection::default(), stats);
    }
    let size = r.diagonal().max(tol::ROUNDOFF);
    let tol = REL_TOL * size.max(1.0);
    let r = r.grow(tol::REL_POINT * size + tol);
    let ((pu0, pu1), (pv0, pv1)) = b.param_box();
    let bt = Dom::new(
        b,
        UvBox::new((pu0.max(-1e4), pu1.min(1e4)), (pv0.max(-1e4), pv1.min(1e4))),
    );
    let ctx = Ctx {
        a: Dom::new(a, ua),
        b: Dom::new(b, ub),
        bt,
        region: r,
        size,
        tiny: TINY * size,
        tol,
    };
    let pairs: Vec<(Cell, Cell)> = cells_a
        .iter()
        .flat_map(|ca| {
            cells_b
                .iter()
                .filter(|cb| ca.bb.grow(tol).overlaps(&cb.bb))
                .map(move |cb| (*ca, *cb))
        })
        .collect();
    let clock = web_time::Instant::now();
    super::par::start();
    let found: Vec<Found> = pairs
        .into_par_iter()
        .map(|(ca, cb)| search(&ctx, ca, cb))
        .collect();
    let mut seeds = Vec::new();
    let mut touches = Vec::new();
    let mut coin: Vec<UvBox> = Vec::new();
    for f in found {
        stats.pairs += f.pairs;
        seeds.extend(f.seeds);
        touches.extend(f.touches);
        coin.extend(f.coincident);
    }
    let coincident = !coin.is_empty();
    // Inside a cell of a lying on b the surfaces are one: a seed, a touching point or a traced
    // point there is rounding (the region's boundary is found apart).
    let in_coin = |uv: (f64, f64)| {
        let Some((ou, ov)) = ctx.a.locate(uv, tol::ROUNDOFF) else {
            return false;
        };
        let (u, v) = (uv.0 - ou, uv.1 - ov);
        coin.iter()
            .any(|c| u > c.u.0 && u < c.u.1 && v > c.v.0 && v < c.v.1)
    };
    if coincident {
        seeds.retain(|p| !in_coin(p.a));
        touches.retain(|p| !in_coin(p.a));
    }
    stats.seeds = seeds.len();
    if std::env::var_os("CADRS_SSI_SEEDS").is_some() {
        for sd in &seeds {
            eprintln!(
                "  seed {:.6} {:.6} {:.6} a {:.6} {:.6} b {:.6} {:.6}",
                sd.p.x, sd.p.y, sd.p.z, sd.a.0, sd.a.1, sd.b.0, sd.b.1
            );
        }
    }
    stats.touches = touches.len();
    let dbg = std::env::var_os("CADRS_SSI_DEBUG").is_some();
    if dbg {
        eprintln!(
            "ssi_numeric: {:?} over {ua:?} × {:?} over {ub:?} in {:?}: {} pairs, {} seeds, {} touches, coincident {coincident}, search {:?}",
            a.kind(),
            b.kind(),
            ctx.region,
            stats.pairs,
            seeds.len(),
            touches.len(),
            clock.elapsed()
        );
    }
    // Trace from each seed not on a curve already.
    let mut traced: Vec<(Traced, Bbox)> = Vec::new();
    for sd in seeds {
        // A seed where the surfaces are tangent can't be marched from: a touching point.
        if !transversal(&ctx, &sd) {
            touches.push(sd);
            continue;
        }
        let t = tangent(&ctx, &sd);
        if on_traced(&ctx, &sd.p, t.as_ref(), &traced) {
            continue;
        }
        if let Some(c) = march(&ctx, sd, &traced) {
            let bb = bbox_of(&c.pts, (ctx.size / 40.0).max(ctx.tol));
            traced.push((c, bb));
        } else if dbg {
            eprintln!("  no march from {:?} (tangent {:?})", sd, t);
        }
    }
    if coincident {
        traced.retain(|(c, _)| c.pts.iter().filter(|q| in_coin(q.a)).count() * 2 <= c.pts.len());
    }
    // A piece run onto curves traced before at both ends, and alongside them all the way (from
    // seeds by a branch point, where the curves come together), adds nothing.
    let near_other = |k: usize, p: &P3, d: f64| {
        traced.iter().enumerate().any(|(j, (c, bb))| {
            j != k
                && bb.grow(d).contains(p)
                && c.pts.windows(2).any(|w| seg_dist3(p, &w[0].p, &w[1].p) < d)
        })
    };
    let redundant: Vec<bool> = (0..traced.len())
        .map(|k| {
            let c = &traced[k].0;
            !c.closed
                && c.ends == [End::Merged; 2]
                && c.pts.iter().all(|q| near_other(k, &q.p, 0.02 * ctx.size))
        })
        .collect();
    let mut k = 0;
    traced.retain(|_| {
        k += 1;
        !redundant[k - 1]
    });
    // Touching points by the traced curves: the branch points and touching points they pass,
    // solved exactly and put on each curve passing them (so that curves meeting there share
    // the point exactly). The others: tangent curves and points of contact.
    let near_curve = |p: &P3| {
        traced.iter().any(|(c, bb)| {
            bb.contains(p)
                && c.pts
                    .windows(2)
                    .any(|w| seg_dist3(p, &w[0].p, &w[1].p) < 2.0 * ctx.tiny)
        })
    };
    // Chains of touching points are tangent curves (dropped where they run along a traced
    // curve: a crossing too shallow for the transversality test); a few together, a point.
    let (tangents, contacts) = chain_touches(touches, 3.0 * ctx.tiny, 0.5 * ctx.tiny);
    let tangents: Vec<Traced> = tangents
        .into_iter()
        .filter(|c| c.pts.iter().filter(|q| near_curve(&q.p)).count() * 2 < c.pts.len())
        .collect();
    let (by, points): (Vec<Pt>, Vec<Pt>) = contacts.into_iter().partition(|t| near_curve(&t.p));
    let mut singular: Vec<Pt> = Vec::new();
    for t in by {
        if singular.iter().any(|q| (q.p - t.p).norm() < 4.0 * ctx.tiny) {
            continue;
        }
        if let Some(q) = singular_point(&ctx, t).filter(|q| {
            !singular
                .iter()
                .any(|x| (x.p - q.p).norm() < tol::REL_POINT * ctx.size)
        }) {
            singular.push(q);
        }
    }
    if dbg {
        eprintln!(
            "  singular points {:?}",
            singular.iter().map(|q| (q.p, q.a, q.b)).collect::<Vec<_>>()
        );
    }
    // Every curve passing a singular point is put through it, whether the curves cross there
    // (a branch point) or only touch (two loops kissing): the faces a boolean splits along
    // them meet at that point.
    for q in &singular {
        for (c, _) in traced.iter_mut() {
            snap_through(&ctx, c, q);
        }
    }
    // Curves cut at the singular points they pass (a figure eight marched through its waist
    // becomes its two loops), so that the points are the pieces' ends.
    if !singular.is_empty() {
        let at_singular = |p: &P3| {
            singular
                .iter()
                .any(|q| (q.p - p).norm() <= tol::REL_FINE * (1.0 + ctx.size))
        };
        traced = traced
            .into_iter()
            .flat_map(|(c, _)| split_at(c, &at_singular))
            .map(|c| {
                let bb = bbox_of(&c.pts, (ctx.size / 40.0).max(ctx.tol));
                (c, bb)
            })
            .collect();
    }
    let mut curves: Vec<Traced> = traced.into_iter().map(|(c, _)| c).collect();
    if dbg {
        eprintln!("  traced in {:?}", clock.elapsed());
        for c in &curves {
            let steep = |p: &Pt| a.normal(p.a.0, p.a.1).cross(&b.normal(p.b.0, p.b.1)).norm();
            let (lo, hi) = c
                .pts
                .iter()
                .map(steep)
                .fold((f64::INFINITY, 0.0f64), |(l, h), x| (l.min(x), h.max(x)));
            eprintln!(
                "  traced {} points, closed {}, ends {:?}, crossing angle sin {lo:.1e}..{hi:.1e}",
                c.pts.len(),
                c.closed,
                c.ends
            );
            if std::env::var_os("CADRS_SSI_DUMP").is_some() {
                for (k, q) in c.pts.iter().enumerate() {
                    eprintln!(
                        "    {k} {:.6} {:.6} {:.6}  a {:.6} {:.6}  b {:.6} {:.6}",
                        q.p.x, q.p.y, q.p.z, q.a.0, q.a.1, q.b.0, q.b.1
                    );
                }
            }
        }
        eprintln!(
            "  {} tangent curves, {} points of contact",
            tangents.len(),
            points.len()
        );
    }
    curves.extend(tangents);
    (
        Intersection {
            curves,
            points,
            singular,
            coincident,
        },
        stats,
    )
}

/// The point near `p` where the surfaces meet and are tangent (a branch point of the
/// intersection, or a point of contact): Gauss–Newton on S_a = S_b and n_a × n_b = 0 (the
/// latter scaled by the problem's size), with central-difference derivatives.
fn singular_point(ctx: &Ctx, mut p: Pt) -> Option<Pt> {
    let (sa, sb) = (ctx.a.s, ctx.b.s);
    let w = ctx.size.max(tol::ROUNDOFF);
    let res = |x: &[f64; 4]| -> [f64; 6] {
        let d = sa.eval(x[0], x[1]) - sb.eval(x[2], x[3]);
        let c = sa.normal(x[0], x[1]).cross(&sb.normal(x[2], x[3])) * w;
        [d.x, d.y, d.z, c.x, c.y, c.z]
    };
    let mut x = [p.a.0, p.a.1, p.b.0, p.b.1];
    for _ in 0..30 {
        let r = res(&x);
        let mut j = nalgebra::Matrix6x4::zeros();
        for k in 0..4 {
            let h = 1e-7 * (1.0 + x[k].abs());
            let (mut xp, mut xm) = (x, x);
            xp[k] += h;
            xm[k] -= h;
            let (rp, rm) = (res(&xp), res(&xm));
            for i in 0..6 {
                j[(i, k)] = (rp[i] - rm[i]) / (2.0 * h);
            }
        }
        let rv = nalgebra::Vector6::from_column_slice(&r);
        let jt = j.transpose();
        let m = jt * j;
        let lambda = tol::REL_FINE * m.trace().max(tol::TINY);
        let dx = (m + nalgebra::Matrix4::identity() * lambda)
            .lu()
            .solve(&(-(jt * rv)))?;
        if !dx.iter().all(|v| v.is_finite()) {
            return None;
        }
        for k in 0..4 {
            x[k] += dx[k];
        }
        if dx.norm() < 1e-14 * (1.0 + x.iter().fold(0.0f64, |a, v| a.max(v.abs()))) {
            break;
        }
    }
    // An angle a rounding error off a seam is on it: its point then lies on the seam exactly,
    // not a hair across it (where a boolean would see it a turn away).
    for (k, periodic) in [
        sa.u_periodic(),
        sa.v_periodic(),
        sb.u_periodic(),
        sb.v_periodic(),
    ]
    .into_iter()
    .enumerate()
    {
        let seam = (x[k] / TAU).round() * TAU;
        if periodic && (x[k] - seam).abs() < tol::ROUNDOFF_FINE {
            x[k] = seam;
        }
    }
    let pa = sa.eval(x[0], x[1]);
    let tangent = sa.normal(x[0], x[1]).cross(&sb.normal(x[2], x[3])).norm() < tol::ANGULAR_POINT;
    if (pa - sb.eval(x[2], x[3])).norm() > ctx.tol || !tangent || (pa - p.p).norm() > 8.0 * ctx.tiny
    {
        return None;
    }
    p = Pt {
        p: pa,
        a: (x[0], x[1]),
        b: (x[2], x[3]),
    };
    (ctx.a.locate(p.a, tol::ROUNDOFF).is_some() && ctx.b.locate(p.b, tol::ROUNDOFF).is_some())
        .then_some(p)
}

/// The passes of a traced curve by `q`: the points nearer `q` than their neighbours, within
/// half the step there, with the curve's direction through them.
fn passes(c: &Traced, q: &P3, tol: f64) -> Vec<(usize, V3)> {
    let n = c.pts.len();
    if n < 2 {
        return vec![];
    }
    let dist = |k: usize| (c.pts[k].p - q).norm();
    let prev = |m: usize| {
        if m > 0 {
            Some(m - 1)
        } else if c.closed {
            Some(n - 1)
        } else {
            None
        }
    };
    let next = |m: usize| {
        if m + 1 < n {
            Some(m + 1)
        } else if c.closed {
            Some(0)
        } else {
            None
        }
    };
    (0..n)
        .filter_map(|m| {
            let dm = dist(m);
            let lp = prev(m).map_or(0.0, |o| (c.pts[o].p - c.pts[m].p).norm());
            let ln = next(m).map_or(0.0, |o| (c.pts[o].p - c.pts[m].p).norm());
            let near = prev(m).is_none_or(|o| dist(o) > dm)
                && next(m).is_none_or(|o| dist(o) >= dm)
                && dm <= 0.5 * lp.max(ln) + tol;
            let a = c.pts[prev(m).unwrap_or(m)].p;
            let b = c.pts[next(m).unwrap_or(m)].p;
            near.then(|| {
                (
                    m,
                    (b - a).try_normalize(tol::TINY).unwrap_or_else(V3::zeros),
                )
            })
        })
        .collect()
}

/// A traced curve cut at the points `cut` picks out: open pieces between them, or a closed
/// curve through just one of them started there; a piece that comes back to where it started
/// is closed.
fn split_at(c: Traced, cut: &dyn Fn(&P3) -> bool) -> Vec<Traced> {
    let ks: Vec<usize> = (0..c.pts.len()).filter(|&k| cut(&c.pts[k].p)).collect();
    if ks.is_empty() {
        return vec![c];
    }
    let mut pts = c.pts;
    let mut pieces: Vec<Vec<Pt>> = Vec::new();
    if c.closed {
        pts.rotate_left(ks[0]);
        let n = pts.len();
        let mut cuts: Vec<usize> = ks.iter().map(|k| k - ks[0]).collect();
        cuts.push(n);
        for w in cuts.windows(2) {
            let mut piece: Vec<Pt> = pts[w[0]..w[1]].to_vec();
            piece.push(pts[w[1] % n]);
            pieces.push(piece);
        }
    } else {
        let mut start = 0;
        for &k in &ks {
            if k > start {
                pieces.push(pts[start..=k].to_vec());
            }
            start = k;
        }
        if start + 1 < pts.len() {
            pieces.push(pts[start..].to_vec());
        }
    }
    let ends = c.ends;
    let last = pieces.len().saturating_sub(1);
    pieces
        .into_iter()
        .enumerate()
        .filter(|(_, p)| p.len() >= 2)
        .map(|(i, mut p)| {
            let closed = p.len() > 2 && (p[0].p - p[p.len() - 1].p).norm() == 0.0;
            if closed {
                p.pop();
            }
            let e0 = if !c.closed && i == 0 {
                ends[0]
            } else {
                End::Singular
            };
            let e1 = if !c.closed && i == last {
                ends[1]
            } else {
                End::Singular
            };
            Traced {
                pts: p,
                closed,
                ends: if closed { [End::Singular; 2] } else { [e0, e1] },
                tangent: c.tangent,
            }
        })
        .collect()
}

/// Puts the singular point `q` on the traced curve `c` when the curve passes it within a
/// fraction of the local step: in place of the nearest point when that is close, else between
/// the two points around it.
fn snap_through(ctx: &Ctx, c: &mut Traced, q: &Pt) {
    // (From the last pass, so inserting one leaves the indices before it alone.)
    let mut hits = passes(c, &q.p, ctx.tol);
    hits.reverse();
    let n = c.pts.len();
    let ps: Vec<P3> = c.pts.iter().map(|x| x.p).collect();
    let prev = |m: usize| {
        if m > 0 {
            Some(m - 1)
        } else if c.closed {
            Some(n - 1)
        } else {
            None
        }
    };
    let next = |m: usize| {
        if m + 1 < n {
            Some(m + 1)
        } else if c.closed {
            Some(0)
        } else {
            None
        }
    };
    for (m, _) in hits {
        let dm = (ps[m] - q.p).norm();
        let step = |o: Option<usize>| o.map_or(0.0, |o| (ps[o] - ps[m]).norm());
        let (lp, ln) = (step(prev(m)), step(next(m)));
        let short = if lp == 0.0 {
            ln
        } else if ln == 0.0 {
            lp
        } else {
            lp.min(ln)
        };
        if dm < 0.5 * short || dm < ctx.tol {
            c.pts[m] = *q;
            continue;
        }
        // Between the nearest point and the neighbour on q's side.
        let side = |o: Option<usize>| o.map(|o| (ps[o] - ps[m]).dot(&(q.p - ps[m])));
        match (side(prev(m)), side(next(m))) {
            (_, Some(x)) if x > 0.0 => c.pts.insert(m + 1, *q),
            (Some(x), _) if x > 0.0 => c.pts.insert(m, *q),
            _ => c.pts[m] = *q,
        }
    }
}
