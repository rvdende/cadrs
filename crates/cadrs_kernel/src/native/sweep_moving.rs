//! What a convex polyhedron fills under any rigid motion, exactly: its contacts may move over its
//! faces (a cube tumbling about two axes at once), which the sweep by faces
//! ([`super::sweep_faces`]) can't follow.
//!
//! A face's velocity field in the body's own frame is ω(t) × p + u(t), so where a plane face
//! leads (g = n · (ω × p + u) > 0) is the face cut by a straight line, its contact, which moves
//! as t does: at each instant the leading part is a convex polygon. Its corners are the face's
//! corners and the points where the contact line crosses its edges (each edge also cut where the
//! neighbouring face's contact crosses it, so that both faces' pieces of an edge agree). Between
//! the instants where that polygon changes (its line through a corner, two contacts meeting on an
//! edge), and in chunks of at most a few degrees of turn, each side of the polygon sweeps a ruled
//! surface between the paths of its two corners (each path a B-spline through its exact points,
//! all of a chunk's at one set of parameters, refined to a millionth of a millionth of the size).
//!
//! The body's trailing polygons at the start, the sides that one face's polygon sweeps (a side
//! two faces' polygons share is inside, and so is the polygon between two chunks) and the leading
//! polygons at the end close a surface round the swept solid, but not its boundary: where the
//! motion carries the body back over where it has been, the surface runs inside and crosses
//! itself. Which of it bounds the solid is decided exactly ([`Oracle`]: a point is swept when, at
//! some instant, it lies behind every face's plane of the body placed then). Each face is judged
//! at points over it: wholly on the boundary it is kept, wholly inside dropped; the others are
//! cut along the curves where the boundary crosses them, which are followed from points between
//! a judged pair, across the faces that meet along them (on to a face's edge, or to where a third
//! face covers the curve and two others start). Their pieces are judged again, and the faces
//! kept are joined along the edges they share.

use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::{H, NurbsCurve, NurbsSurface};
use super::surface::Surface;
use super::tol;
use super::topo::Body;
use crate::*;
use std::collections::HashMap;

/// A motion: the rotation and translation that place the body at t in [0, 1].
pub type Place<'a> = &'a (dyn Fn(f64) -> (nalgebra::Rotation3<f64>, V3) + Sync);

/// What a face of a chunk's piece is: a side, or a cap at the chunk's start or end (for the
/// body at the start: its leading polygons, and its trailing ones).
const SIDE: u8 = 0;
const CAP_START: u8 = 1;
const CAP_END: u8 = 2;

/// A chunk's piece, with what each of its faces is.
type Piece = (Body, Vec<u8>);

/// A side of a polygon: its face, and its corners in the face's order.
type Side = (usize, Node, Node);

/// A function of the body's velocity whose sign changes where a polygon does.
type Signed<'a> = Box<dyn Fn((V3, V3)) -> f64 + 'a>;

/// The most a chunk may turn (radians).
const CHUNK_TURN: f64 = 0.25;

/// A corner of a face's leading polygon: a body vertex, or where face `f`'s contact line crosses
/// edge `e`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
enum Node {
    Vert(usize),
    Cross(usize, usize),
}

/// A plane face of the body: its outward normal, its corners in loop order (body vertices), each
/// side's edge, and the face across it.
struct PFace {
    n: V3,
    corners: Vec<usize>,
    edges: Vec<usize>,
    across: Vec<usize>,
}

/// The body's velocity in its own frame at t: (ω, u), so that a point p moves at ω × p + u.
/// (By central differences, of second order at the ends too, where they look one way only: a
/// first-order end would put the contacts' corners a hair off their paths there.)
fn velocity(place: Place, t: f64) -> (V3, V3) {
    let h = 1e-6;
    let (r, _) = place(t);
    let (rdot, ddot) = if t - h < 0.0 || t + h > 1.0 {
        let s = if t - h < 0.0 { 1.0 } else { -1.0 };
        let ((r0, d0), (r1, d1), (r2, d2)) = (place(t), place(t + s * h), place(t + 2.0 * s * h));
        (
            (r1.matrix() * 4.0 - r0.matrix() * 3.0 - r2.matrix()) / (2.0 * s * h),
            (d1 * 4.0 - d0 * 3.0 - d2) / (2.0 * s * h),
        )
    } else {
        let ((ra, da), (rb, db)) = (place(t - h), place(t + h));
        (
            (rb.matrix() - ra.matrix()) / (2.0 * h),
            (db - da) / (2.0 * h),
        )
    };
    let w = r.matrix().transpose() * rdot;
    let omega = V3::new(
        0.5 * (w[(2, 1)] - w[(1, 2)]),
        0.5 * (w[(0, 2)] - w[(2, 0)]),
        0.5 * (w[(1, 0)] - w[(0, 1)]),
    );
    (omega, r.matrix().transpose() * ddot)
}

/// A convex polyhedron (`b`) and its motion.
struct Sweep<'a> {
    b: &'a Body,
    faces: Vec<PFace>,
    place: Place<'a>,
    /// Each edge's two faces.
    edge_faces: Vec<Vec<usize>>,
}

impl Sweep<'_> {
    /// g of face `f` at body point p, time t.
    fn g(&self, f: usize, p: &P3, (w, u): (V3, V3)) -> f64 {
        self.faces[f].n.dot(&(w.cross(&p.coords) + u))
    }

    /// Where on edge `e` (0 at its first vertex, 1 at its second) face `f`'s contact crosses it.
    fn cross_s(&self, e: usize, f: usize, vel: (V3, V3)) -> Option<f64> {
        let ed = &self.b.edges[e];
        let (a, z) = (self.b.verts[ed.v[0]].p, self.b.verts[ed.v[1]].p);
        let (ga, gz) = (self.g(f, &a, vel), self.g(f, &z, vel));
        if ga == gz {
            return None;
        }
        let s = ga / (ga - gz);
        (s > 0.0 && s < 1.0).then_some(s)
    }

    /// A node's point in the body's frame at t.
    fn at(&self, node: Node, t: f64) -> P3 {
        match node {
            Node::Vert(v) => self.b.verts[v].p,
            Node::Cross(e, f) => {
                let vel = velocity(self.place, t);
                let ed = &self.b.edges[e];
                let (a, z) = (self.b.verts[ed.v[0]].p, self.b.verts[ed.v[1]].p);
                let (ga, gz) = (self.g(f, &a, vel), self.g(f, &z, vel));
                let s = if ga == gz { 0.5 } else { ga / (ga - gz) };
                a + (z - a) * s.clamp(0.0, 1.0)
            }
        }
    }

    /// A node's place in the world at t.
    fn world(&self, node: Node, t: f64) -> P3 {
        let (r, d) = (self.place)(t);
        P3::from(r * self.at(node, t).coords + d)
    }

    /// Face `f`'s polygon at t where g has `sign` (+1 leading, −1 trailing), its corners in the
    /// face's loop order, each side of the face cut also where the neighbour's contact crosses
    /// it. Empty where the face has no such part.
    fn polygon(&self, f: usize, t: f64, sign: f64) -> Vec<Node> {
        let vel = velocity(self.place, t);
        let pf = &self.faces[f];
        let k = pf.corners.len();
        let mut out = Vec::new();
        for i in 0..k {
            let (va, vz) = (pf.corners[i], pf.corners[(i + 1) % k]);
            let e = pf.edges[i];
            let (a, z) = (self.b.verts[va].p, self.b.verts[vz].p);
            let ga = sign * self.g(f, &a, vel);
            if ga >= 0.0 {
                out.push(Node::Vert(va));
            }
            // The crossings inside this side, in its order from va.
            let ed = &self.b.edges[e];
            let forward = ed.v[0] == va;
            let mut xs: Vec<(f64, Node)> = Vec::new();
            for g in [f, pf.across[i]] {
                if let Some(s) = self.cross_s(e, g, vel) {
                    let s = if forward { s } else { 1.0 - s };
                    let p = a + (z - a) * s;
                    // Our own contact always; the neighbour's where it falls in our part.
                    if g == f || sign * self.g(f, &p, vel) > 0.0 {
                        xs.push((s, Node::Cross(e, g)));
                    }
                }
            }
            xs.sort_by(|x, y| x.0.total_cmp(&y.0));
            out.extend(xs.into_iter().map(|x| x.1));
        }
        out
    }

    /// The instants in (0, 1) where some face's polygon changes: its contact line through a
    /// corner, or two contacts meeting on an edge.
    fn events(&self) -> Vec<f64> {
        let n = 2048;
        let ts: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
        let vels: Vec<(V3, V3)> = ts.iter().map(|&t| velocity(self.place, t)).collect();
        let mut fns: Vec<Signed<'_>> = Vec::new();
        for (f, pf) in self.faces.iter().enumerate() {
            for &v in &pf.corners {
                let p = self.b.verts[v].p;
                fns.push(Box::new(move |vel| self.g(f, &p, vel)));
            }
        }
        for (e, fs) in self.edge_faces.iter().enumerate() {
            if let [f1, f2] = fs[..] {
                let ed = &self.b.edges[e];
                let (a, z) = (self.b.verts[ed.v[0]].p, self.b.verts[ed.v[1]].p);
                fns.push(Box::new(move |vel| {
                    self.g(f1, &a, vel) * self.g(f2, &z, vel)
                        - self.g(f2, &a, vel) * self.g(f1, &z, vel)
                }));
            }
        }
        let mut out = Vec::new();
        for f in &fns {
            let vals: Vec<f64> = vels.iter().map(|&v| f(v)).collect();
            for i in 0..n {
                if (vals[i] > 0.0) == (vals[i + 1] > 0.0) || vals[i] == 0.0 {
                    continue;
                }
                let (mut lo, mut hi) = (ts[i], ts[i + 1]);
                let pos = vals[i] > 0.0;
                for _ in 0..60 {
                    let m = 0.5 * (lo + hi);
                    if (f(velocity(self.place, m)) > 0.0) == pos {
                        lo = m;
                    } else {
                        hi = m;
                    }
                }
                out.push(0.5 * (lo + hi));
            }
        }
        out.sort_by(f64::total_cmp);
        out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        out
    }
}

/// Whether points lie in the swept solid, exactly: a point is in it when, at some instant, it
/// lies behind every face's plane of the body placed then. [`Oracle::depth`] is the least over
/// the motion of the most a point lies in front of one of those planes (negative inside).
pub struct Oracle<'a> {
    /// How far a point (in the body's own frame) lies outside the body: positive outside.
    measure: Box<dyn Fn(&V3) -> f64 + Send + Sync + 'a>,
    ts: Vec<f64>,
    /// The place at each sample: its rotation and translation.
    places: Vec<(nalgebra::Rotation3<f64>, V3)>,
    place: Place<'a>,
    /// Which candidate face (by its index among those given to [`boundary_of`]) holds a point
    /// the body's surface touches at an instant, with the point's (u, v) on it, when known.
    locate: Option<Locate<'a>>,
    /// How far a point moves in the body's frame from the first sample to each: its turn's
    /// part (to be multiplied by the point's distance from the origin) and its shift's, summed
    /// along the samples. With a measure that changes no faster than the point moves (a
    /// distance), they bound how far it can dip between two samples read.
    travel: Option<Travel>,
}

/// [`Oracle::travel`]: the sums up to each sample, and the most between two neighbours.
struct Travel {
    turn: Vec<f64>,
    shift: Vec<f64>,
    step: (f64, f64),
}

/// Every how many samples the measure is read before the rest are, where they may matter.
const STRIDE: usize = 8;

/// (instant, point) ↦ the candidate faces the body's surface passes through the point on then
/// (several where they share an edge there), each with the (u, v) there.
pub type Locate<'a> = Box<dyn Fn(f64, &P3) -> Vec<(usize, P2)> + Send + Sync + 'a>;

impl<'a> Oracle<'a> {
    /// The test of a convex polyhedron, from its faces' planes (n · p = d, n outward).
    pub fn new(planes: Vec<(V3, f64)>, place: Place<'a>, samples: usize) -> Oracle<'a> {
        // (By unit normals the measure is a distance from the planes: it changes no faster
        // than the point moves.)
        let unit = planes.iter().all(|(n, _)| (n.norm() - 1.0).abs() <= 1e-9);
        let mut oracle = Self::planes_measure(planes, place, samples);
        if unit {
            oracle.travel = Some(oracle.travelled());
        }
        oracle
    }

    fn planes_measure(planes: Vec<(V3, f64)>, place: Place<'a>, samples: usize) -> Oracle<'a> {
        Self::with_measure(
            Box::new(move |q: &V3| {
                planes
                    .iter()
                    .map(|(n, c)| n.dot(q) - c)
                    .fold(f64::NEG_INFINITY, f64::max)
            }),
            place,
            samples,
        )
    }

    /// The test of any body by `measure`: how far a point in its own frame lies outside it.
    pub fn with_measure(
        measure: Box<dyn Fn(&V3) -> f64 + Send + Sync + 'a>,
        place: Place<'a>,
        samples: usize,
    ) -> Oracle<'a> {
        let ts: Vec<f64> = (0..=samples).map(|i| i as f64 / samples as f64).collect();
        let places = ts.iter().map(|&t| place(t)).collect();
        Oracle {
            measure,
            ts,
            places,
            place,
            locate: None,
            travel: None,
        }
    }

    /// [`Oracle::travel`] of the samples as they are.
    fn travelled(&self) -> Travel {
        let n = self.places.len();
        let (mut turn, mut shift) = (vec![0.0; n], vec![0.0; n]);
        let mut step = (0.0f64, 0.0f64);
        for k in 0..n.saturating_sub(1) {
            let ((ra, da), (rb, db)) = (&self.places[k], &self.places[k + 1]);
            let (qa, qb) = (ra.transpose(), rb.transpose());
            // (A turn's change of a rotation moves a point by its largest singular value
            // times the point's distance: the Frobenius norm over √2, for two rotations.)
            let rho = (qb.matrix() - qa.matrix()).norm() / 2f64.sqrt() * (1.0 + 1e-9);
            let tau = (qb * db - qa * da).norm();
            turn[k + 1] = turn[k] + rho;
            shift[k + 1] = shift[k] + tau;
            step = (step.0.max(rho), step.1.max(tau));
        }
        Travel { turn, shift, step }
    }

    /// The measure at the samples for `x`, read only where a local least no higher than
    /// `limit` (None: than the lowest of all, by a sample's travel) may lie; infinity at the
    /// rest. Every [`STRIDE`]-th first, then the stretches between whose ends leave room for
    /// one, the lowest bound first.
    /// With them, how high a sample beside ones unread may be and still be a least (those
    /// between it and the next read are bounded above it): infinity where all were read.
    fn sampled(&self, x: &P3, limit: Option<f64>) -> (Vec<f64>, f64) {
        let n = self.places.len();
        let travel = match &self.travel {
            Some(t) if n > 4 * STRIDE && (n - 1).is_multiple_of(STRIDE) => t,
            _ => {
                return (
                    self.places.iter().map(|pl| self.at(pl, x)).collect(),
                    f64::INFINITY,
                );
            }
        };
        let far = x.coords.norm();
        let mut vals = vec![f64::INFINITY; n];
        let mut best = f64::INFINITY;
        for i in (0..n).step_by(STRIDE) {
            vals[i] = self.at(&self.places[i], x);
            best = best.min(vals[i]);
        }
        // (Between two samples read, no sample dips below half their sum less the travel.)
        let stretches = (n - 1) / STRIDE;
        let bound = |k: usize| {
            let (i, j) = (k * STRIDE, (k + 1) * STRIDE);
            let moved =
                (travel.turn[j] - travel.turn[i]) * far + (travel.shift[j] - travel.shift[i]);
            0.5 * (vals[i] + vals[j] - moved)
        };
        let mut order: Vec<(f64, usize)> = (0..stretches).map(|k| (bound(k), k)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        // (A least refined between its neighbours comes no lower than a sample's travel.)
        let slack = travel.step.0 * far + travel.step.1;
        for (low, k) in order {
            if low > limit.unwrap_or(best) + slack {
                break;
            }
            for i in k * STRIDE + 1..(k + 1) * STRIDE {
                vals[i] = self.at(&self.places[i], x);
                best = best.min(vals[i]);
            }
        }
        (vals, limit.unwrap_or(best) + slack)
    }

    fn at(&self, (r, d): &(nalgebra::Rotation3<f64>, V3), x: &P3) -> f64 {
        (self.measure)(&(r.transpose() * (x.coords - d)))
    }

    /// How far `x` lies outside the body placed at instant `t` (negative inside).
    pub fn at_instant(&self, t: f64, x: &P3) -> f64 {
        self.at(&(self.place)(t), x)
    }

    /// The least over the motion of how far `x` lies outside the placed body (by its faces'
    /// planes), with the instant: the samples' local least values refined by golden-section
    /// search.
    pub fn depth(&self, x: &P3) -> (f64, f64) {
        self.leasts_of(x, self.sampled(x, None))
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or((f64::INFINITY, 0.0))
    }

    /// The local leasts over the motion (the four lowest of the samples', refined) of how far
    /// `x` lies outside the placed body, each with its instant.
    pub fn leasts(&self, x: &P3) -> Vec<(f64, f64)> {
        let vals = self.places.iter().map(|pl| self.at(pl, x)).collect();
        self.leasts_of(x, (vals, f64::INFINITY))
    }

    /// [`Self::leasts`], those no higher than `limit` alone (and others, perhaps, that are).
    pub fn leasts_within(&self, x: &P3, limit: f64) -> Vec<(f64, f64)> {
        self.leasts_of(x, self.sampled(x, Some(limit)))
    }

    /// The local leasts of the samples' values for `x` ([`Self::sampled`]'s), the four lowest,
    /// refined.
    fn leasts_of(&self, x: &P3, (vals, unread): (Vec<f64>, f64)) -> Vec<(f64, f64)> {
        let n = vals.len();
        // (Beside a sample left unread, a sample is a least where it is low enough that what
        // was left unread is bounded above it; higher, it is none that matters.)
        let below = |i: usize, o: usize| {
            if vals[o].is_finite() {
                vals[i] <= vals[o]
            } else {
                vals[i] <= unread
            }
        };
        let mut mins: Vec<usize> = (0..n)
            .filter(|&i| {
                vals[i].is_finite()
                    && (i == 0 || below(i, i - 1))
                    && (i + 1 == n || below(i, i + 1))
            })
            .collect();
        mins.sort_by(|&a, &b| vals[a].total_cmp(&vals[b]));
        let mut out = Vec::with_capacity(4);
        for &i in mins.iter().take(4) {
            let mut best = (f64::INFINITY, 0.0);
            let (mut a, mut b) = (self.ts[i.saturating_sub(1)], self.ts[(i + 1).min(n - 1)]);
            let f = |t: f64| self.at(&(self.place)(t), x);
            let g = 0.5 * (5f64.sqrt() - 1.0);
            let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
            let (mut fc, mut fd) = (f(c), f(d));
            // (Forty halvings by the golden ratio: the instant to a billionth of the samples'
            // spacing, where the least is flat to its square.)
            for _ in 0..40 {
                if fc < fd {
                    b = d;
                    d = c;
                    fd = fc;
                    c = b - g * (b - a);
                    fc = f(c);
                } else {
                    a = c;
                    c = d;
                    fc = fd;
                    d = a + g * (b - a);
                    fd = f(d);
                }
            }
            for (v, t) in [(vals[i], self.ts[i]), (fc, c), (fd, d)] {
                if v < best.0 {
                    best = (v, t);
                }
            }
            out.push(best);
        }
        out
    }
}

/// The B-splines (cubic, at one set of parameters over [ta, tb]) through the paths of `nodes`,
/// refined until each strays at most `tol` from its path between its points.
fn traces(sw: &Sweep, nodes: &[Node], ta: f64, tb: f64, tol: f64) -> HashMap<Node, NurbsCurve> {
    let mut n = 8;
    loop {
        let ts: Vec<f64> = (0..=n)
            .map(|i| ta + (tb - ta) * i as f64 / n as f64)
            .collect();
        let mut out = HashMap::new();
        let mut worst: f64 = 0.0;
        for &node in nodes {
            let pts: Vec<P3> = ts.iter().map(|&t| sw.world(node, t)).collect();
            let c = NurbsCurve::interpolate(&pts, &ts, 3, None, None);
            for w in ts.windows(2) {
                let m = 0.5 * (w[0] + w[1]);
                worst = worst.max((c.point(m) - sw.world(node, m)).norm());
            }
            out.insert(node, c);
        }
        if worst <= tol || n >= 4096 {
            return out;
        }
        n *= 2;
    }
}

/// The ruled surface from path `a` to path `z` (u along t, v from a to z).
fn ruled(a: &NurbsCurve, z: &NurbsCurve) -> NurbsSurface {
    NurbsSurface {
        du: a.degree,
        dv: 1,
        ku: a.knots.clone(),
        kv: vec![0.0, 0.0, 1.0, 1.0],
        cps: a
            .cps
            .iter()
            .zip(&z.cps)
            .map(|(p, q)| vec![*p, *q])
            .collect::<Vec<Vec<H>>>(),
    }
}

/// The body where the motion starts, each face split along its contact into the polygon that
/// leads and the one that trails, tagged [`CAP_START`] and [`CAP_END`] (the trailing ones are
/// where the swept solid's boundary starts; the leading ones, the first chunk's start).
fn start_body(sw: &Sweep, size: f64) -> Result<(Body, Vec<u8>)> {
    use super::topo::{Coedge, Face};
    let mut b = Body::default();
    let mut tags = Vec::new();
    let mut vert: HashMap<P3Key, usize> = HashMap::new();
    let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
    let (r, _) = (sw.place)(0.0);
    for f in 0..sw.faces.len() {
        let n = r * sw.faces[f].n;
        for sign in [1.0, -1.0] {
            let poly = sw.polygon(f, 0.0, sign);
            if poly.len() < 3 {
                continue;
            }
            let mut vs: Vec<usize> = Vec::new();
            for nd in &poly {
                let p = sw.world(*nd, 0.0);
                let v = *vert
                    .entry(P3Key::of(&p, 1e-9 * size))
                    .or_insert_with(|| b.add_vertex(p));
                if vs.last() != Some(&v) {
                    vs.push(v);
                }
            }
            while vs.len() > 1 && vs[0] == vs[vs.len() - 1] {
                vs.pop();
            }
            if vs.len() < 3 {
                continue;
            }
            let o = b.verts[vs[0]].p;
            let x = (b.verts[vs[1]].p - o).normalize();
            let x = (x - n * x.dot(&n)).normalize();
            let y = n.cross(&x);
            let uv = |p: P3| P3::new((p - o).dot(&x), (p - o).dot(&y), 0.0);
            let mut coedges = Vec::new();
            for i in 0..vs.len() {
                let (va, vz) = (vs[i], vs[(i + 1) % vs.len()]);
                let key = (va.min(vz), va.max(vz));
                let e = *edges.entry(key).or_insert_with(|| {
                    let (c, len) = Curve::line(b.verts[key.0].p, b.verts[key.1].p);
                    b.add_edge(c, 0.0, len, key.0, key.1)
                });
                let ed = &b.edges[e];
                let (p0, p1) = (uv(b.verts[ed.v[0]].p), uv(b.verts[ed.v[1]].p));
                let len = ed.t[1] - ed.t[0];
                coedges.push(Coedge {
                    edge: e,
                    fwd: ed.v[0] == va,
                    pc: Curve::Line {
                        o: p0,
                        d: (p1 - p0) / len,
                    },
                });
            }
            tags.push(if sign > 0.0 { CAP_START } else { CAP_END });
            b.faces.push(Face {
                surf: Surface::Plane {
                    f: Frame { o, x, y, z: n },
                },
                sense: true,
                loops: vec![coedges],
            });
        }
    }
    super::boolean::assemble_shells(&mut b)?;
    if !b.is_solid() || !b.check().is_empty() {
        return Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: format!(
                "the body split along its contacts isn't sound ({:?})",
                b.check().first()
            ),
        });
    }
    Ok((b, tags))
}

/// A point as a key, rounded to a grid of `step`.
#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct P3Key([i64; 3]);

impl P3Key {
    fn of(p: &P3, step: f64) -> P3Key {
        P3Key([p.x, p.y, p.z].map(|c| (c / step).round() as i64))
    }
}

/// The solid one chunk sweeps: each face's leading polygon at `ta` (facing back) and at `tb`,
/// and the ruled surface each side of them sweeps that one polygon bounds (a side two share is
/// inside), its topology built as it is: each corner's path one edge, each cap side one line
/// edge (shared by the caps and the sides that end there), a side that comes to a point at an
/// end a point edge there. None when no face leads.
fn chunk_piece(sw: &Sweep, ta: f64, tb: f64, fit: f64, size: f64) -> Result<Option<Piece>> {
    use super::topo::{Coedge, Face};
    let mut tags = Vec::new();
    let tm = 0.5 * (ta + tb);
    let polys: Vec<Vec<Node>> = (0..sw.faces.len())
        .map(|f| sw.polygon(f, tm, 1.0))
        .collect();
    if polys.iter().all(|p| p.len() < 3) {
        return Ok(None);
    }
    let mut nodes: Vec<Node> = polys.iter().flatten().copied().collect();
    nodes.sort();
    nodes.dedup();
    let mut paths = traces(sw, &nodes, ta, tb, fit);
    let mut b = Body::default();
    // Each node's vertex at each end: corners that meet there (within a billionth of the size)
    // one vertex, at the least node's place, their paths' end poles moved onto it.
    let mut vert: HashMap<(Node, bool), usize> = HashMap::new();
    for start in [true, false] {
        let at = |c: &NurbsCurve| {
            super::nurbs::dehom(&if start {
                c.cps[0]
            } else {
                c.cps[c.cps.len() - 1]
            })
        };
        let ps: Vec<(Node, P3)> = nodes.iter().map(|n| (*n, at(&paths[n]))).collect();
        let mut made: HashMap<Node, usize> = HashMap::new();
        for &(n, p) in &ps {
            let (lead, canon) = ps
                .iter()
                .filter(|(_, q)| (q - p).norm() < 1e-9 * size)
                .min_by_key(|(m, _)| *m)
                .copied()
                .unwrap_or((n, p));
            let v = *made.entry(lead).or_insert_with(|| b.add_vertex(canon));
            vert.insert((n, start), v);
            let c = paths.get_mut(&n).unwrap();
            let k = if start { 0 } else { c.cps.len() - 1 };
            c.cps[k] = super::nurbs::hom(&canon, 1.0);
        }
    }
    // Each corner's path: one edge, t from ta to tb (made when a side first runs along it).
    let mut path_edges: HashMap<Node, usize> = HashMap::new();
    let mut path_edge = |b: &mut Body, n: Node| -> usize {
        *path_edges.entry(n).or_insert_with(|| {
            let (v0, v1) = (vert[&(n, true)], vert[&(n, false)]);
            let c = Curve::Nurbs(std::sync::Arc::new(paths[&n].clone()));
            b.add_edge(c, ta, tb, v0, v1)
        })
    };
    // Each cap side (unordered corners, end): a line edge from its first corner, or none where
    // they meet.
    let mut cap_edge: HashMap<(Node, Node, bool), (usize, Node, f64)> = HashMap::new();
    let mut side_edge =
        |b: &mut Body, a: Node, z: Node, start: bool| -> Option<(usize, Node, f64)> {
            let key = if a < z { (a, z, start) } else { (z, a, start) };
            if let Some(e) = cap_edge.get(&key) {
                return Some(*e);
            }
            let (va, vz) = (vert[&(key.0, start)], vert[&(key.1, start)]);
            if va == vz {
                return None;
            }
            let (pa, pz) = (b.verts[va].p, b.verts[vz].p);
            let (c, len) = Curve::line(pa, pz);
            let e = (b.add_edge(c, 0.0, len, va, vz), key.0, len);
            cap_edge.insert(key, e);
            Some(e)
        };
    // The caps.
    for (f, poly) in polys.iter().enumerate() {
        if poly.len() < 3 {
            continue;
        }
        for start in [true, false] {
            let t = if start { ta } else { tb };
            let (r, _) = (sw.place)(t);
            let n = r * sw.faces[f].n * if start { -1.0 } else { 1.0 };
            // Counter-clockwise about the outward normal: the face's own loop order at the end,
            // reversed at the start.
            let mut order = poly.clone();
            if start {
                order.reverse();
            }
            let mut loop_: Vec<(usize, bool)> = Vec::new();
            for i in 0..order.len() {
                let (a, z) = (order[i], order[(i + 1) % order.len()]);
                if let Some((e, from, _)) = side_edge(&mut b, a, z, start) {
                    loop_.push((e, from == a));
                }
            }
            if loop_.len() < 3 {
                continue;
            }
            let o = b.verts[b.edges[loop_[0].0].v[0]].p;
            let first = &b.edges[loop_[0].0];
            let x = (b.verts[first.v[1]].p - b.verts[first.v[0]].p).normalize();
            let x = (x - n * x.dot(&n)).normalize();
            let y = n.cross(&x);
            let uv = |p: P3| P3::new((p - o).dot(&x), (p - o).dot(&y), 0.0);
            let coedges = loop_
                .iter()
                .map(|&(e, fwd)| {
                    let ed = &b.edges[e];
                    let (p0, p1) = (uv(b.verts[ed.v[0]].p), uv(b.verts[ed.v[1]].p));
                    let len = ed.t[1] - ed.t[0];
                    Coedge {
                        edge: e,
                        fwd,
                        pc: Curve::Line {
                            o: p0,
                            d: (p1 - p0) / len,
                        },
                    }
                })
                .collect();
            tags.push(if start { CAP_START } else { CAP_END });
            b.faces.push(Face {
                surf: Surface::Plane {
                    f: Frame { o, x, y, z: n },
                },
                sense: true,
                loops: vec![coedges],
            });
        }
    }
    // The sides.
    // (Each side: the faces whose polygons have it, with its corners in their order.)
    let mut sides: HashMap<(Node, Node), Vec<Side>> = HashMap::new();
    for (f, poly) in polys.iter().enumerate() {
        if poly.len() < 3 {
            continue;
        }
        for i in 0..poly.len() {
            let (a, z) = (poly[i], poly[(i + 1) % poly.len()]);
            let key = if a < z { (a, z) } else { (z, a) };
            sides.entry(key).or_default().push((f, a, z));
        }
    }
    let mut keys: Vec<(Node, Node)> = sides.keys().copied().collect();
    keys.sort();
    for key in keys {
        let users = &sides[&key];
        if users.len() != 1 {
            continue;
        }
        let (f, a0, z0) = users[0];
        let (a, z) = key;
        let (pa, pz) = (&paths[&a], &paths[&z]);
        // (A side of no width all along: nothing to sweep.)
        if (0..=8).all(|i| {
            let t = ta + (tb - ta) * i as f64 / 8.0;
            (pa.point(t) - pz.point(t)).norm() < 1e-9 * size
        }) {
            continue;
        }
        let surf = Surface::Nurbs(std::sync::Arc::new(ruled(pa, pz)));
        // Counter-clockwise in (u, v): a's path (v = 0), the end (u = tb, v up), z's path back
        // (v = 1), the start (u = ta, v down).
        let line = |o: (f64, f64), d: (f64, f64)| Curve::Line {
            o: P3::new(o.0, o.1, 0.0),
            d: V3::new(d.0, d.1, 0.0),
        };
        let ea = path_edge(&mut b, a);
        let ez = path_edge(&mut b, z);
        let mut coedges = vec![Coedge {
            edge: ea,
            fwd: true,
            pc: line((0.0, 0.0), (1.0, 0.0)),
        }];
        for (start, u) in [(false, tb), (true, ta)] {
            // Up at the end (a to z), down at the start (z to a).
            let from_a = !start;
            match side_edge(&mut b, a, z, start) {
                Some((e, from, len)) => {
                    let pc = if from == a {
                        line((u, 0.0), (0.0, 1.0 / len))
                    } else {
                        line((u, 1.0), (0.0, -1.0 / len))
                    };
                    let fwd = (from == a) == from_a;
                    if start {
                        coedges.push(Coedge {
                            edge: ez,
                            fwd: false,
                            pc: line((0.0, 1.0), (1.0, 0.0)),
                        });
                    }
                    coedges.push(Coedge { edge: e, fwd, pc });
                }
                None => {
                    let v = vert[&(a, start)];
                    let p = b.verts[v].p;
                    let e = b.add_degenerate(p, 0.0, 1.0, v);
                    if start {
                        coedges.push(Coedge {
                            edge: ez,
                            fwd: false,
                            pc: line((0.0, 1.0), (1.0, 0.0)),
                        });
                    }
                    coedges.push(Coedge {
                        edge: e,
                        fwd: from_a,
                        pc: line((u, 0.0), (0.0, 1.0)),
                    });
                }
            }
        }
        let mut face = Face {
            surf,
            sense: true,
            loops: vec![coedges],
        };
        // Outward: away from the polygon it bounds.
        let (r, _) = (sw.place)(tm);
        let poly = &polys[f];
        let centre =
            P3::from(poly.iter().map(|&nd| sw.at(nd, tm).coords).sum::<V3>() / poly.len() as f64);
        let side_mid = P3::from((sw.at(a0, tm).coords + sw.at(z0, tm).coords) * 0.5);
        let inward = r * (centre - side_mid);
        let (_, su, sv) = face.surf.d1(tm, 0.5);
        if su.cross(&sv).dot(&inward) > 0.0 {
            Body::reverse_face(&mut face);
        }
        tags.push(SIDE);
        b.faces.push(face);
    }
    // The sides turned to agree with the caps (whose outward normals are known): across each
    // edge, a face uses it the other way from its neighbour.
    {
        let caps = b
            .faces
            .iter()
            .take_while(|f| matches!(f.surf, Surface::Plane { .. }))
            .count();
        let mut fixed = vec![false; b.faces.len()];
        fixed[..caps].iter_mut().for_each(|x| *x = true);
        let mut changed = true;
        while changed {
            changed = false;
            let mut uses: HashMap<usize, Vec<(usize, bool)>> = HashMap::new();
            for (fi, f) in b.faces.iter().enumerate() {
                for c in f.loops.iter().flatten() {
                    if !b.edges[c.edge].degenerate {
                        uses.entry(c.edge).or_default().push((fi, c.fwd));
                    }
                }
            }
            for fi in 0..b.faces.len() {
                if fixed[fi] {
                    continue;
                }
                let verdict = b.faces[fi].loops.iter().flatten().find_map(|c| {
                    uses.get(&c.edge)?
                        .iter()
                        .find(|(g, _)| *g != fi && fixed[*g])
                        .map(|(_, fwd)| *fwd == c.fwd)
                });
                if let Some(same) = verdict {
                    if same {
                        Body::reverse_face(&mut b.faces[fi]);
                    }
                    fixed[fi] = true;
                    changed = true;
                    // (The table of uses again, now that this face may have turned.)
                    break;
                }
            }
        }
    }
    super::boolean::assemble_shells(&mut b)?;
    if super::mass::body_volume(&b) < 0.0 {
        for f in &mut b.faces {
            Body::reverse_face(f);
        }
        b.shells.clear();
        b.solids.clear();
        super::boolean::assemble_shells(&mut b)?;
    }
    if !b.is_solid() || !b.check().is_empty() {
        return Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: format!(
                "a chunk of the swept polyhedron isn't a sound solid ({:?})",
                b.check().first()
            ),
        });
    }
    Ok(Some((b, tags)))
}

/// A face that may bound the swept solid (the one face of its own body), with its loops
/// sampled in (u, v) and its box.
struct Cand {
    body: Body,
    polys: Vec<Vec<P2>>,
    bbox: Bbox,
    /// The size of its (u, v) box.
    uv_span: V2,
}

impl Cand {
    fn new(body: Body) -> Cand {
        let polys = super::classify::face_polygons(&body, &body.faces[0]);
        let bbox = body.bbox();
        let mut c = Cand {
            body,
            polys,
            bbox,
            uv_span: V2::zeros(),
        };
        let (lo, hi) = grid_box(&c);
        c.uv_span = hi - lo;
        c
    }

    fn surf(&self) -> &Surface {
        &self.body.faces[0].surf
    }

    /// The outward normal at (u, v).
    fn normal(&self, uv: P2) -> V3 {
        let f = &self.body.faces[0];
        f.surf.normal(uv.x, uv.y) * if f.sense { 1.0 } else { -1.0 }
    }

    /// The (u, v) of the point of the face's surface nearest `x`: a plane's exactly, a ruled
    /// surface's by the nearest point of each ruling (sampled along u, then refined).
    fn project(&self, x: &P3) -> P2 {
        let Surface::Nurbs(ns) = self.surf() else {
            let (u, v) = self.surf().project(x, None);
            // (Into the face's own box, by whole turns: a surface's own angles may run
            // over another range than the face's.)
            let (lo, hi) = grid_box(self);
            let into = |w: f64, lo: f64, hi: f64| -> f64 {
                if w < lo - 1e-12 || w > hi + 1e-12 {
                    let up = w + TAU * ((lo - w) / TAU).ceil().max(0.0);
                    let down = w - TAU * ((w - hi) / TAU).ceil().max(0.0);
                    if (lo..=hi).contains(&up) {
                        up
                    } else if (lo..=hi).contains(&down) {
                        down
                    } else {
                        w
                    }
                } else {
                    w
                }
            };
            return P2::new(into(u, lo.x, hi.x), into(v, lo.y, hi.y));
        };
        if ns.dv != 1 || ns.kv.len() != 4 {
            // A B-spline patch: the nearest of a grid, then Gauss-Newton steps (kept in range).
            let ((u0, u1), (v0, v1)) = ns.range();
            let n = 24;
            let mut best = (f64::INFINITY, u0, v0);
            for i in 0..=n {
                for j in 0..=n {
                    let (u, v) = (
                        u0 + (u1 - u0) * i as f64 / n as f64,
                        v0 + (v1 - v0) * j as f64 / n as f64,
                    );
                    let d = (ns.point(u, v) - x).norm_squared();
                    if d < best.0 {
                        best = (d, u, v);
                    }
                }
            }
            let (mut u, mut v) = (best.1, best.2);
            for _ in 0..30 {
                let (p, su, sv) = self.surf().d1(u, v);
                let r = p - x;
                let (a, b, c) = (su.dot(&su), su.dot(&sv), sv.dot(&sv));
                let (gu, gv) = (su.dot(&r), sv.dot(&r));
                let det = a * c - b * b;
                if det.abs() < 1e-300 {
                    break;
                }
                let du = -(c * gu - b * gv) / det;
                let dv = -(a * gv - b * gu) / det;
                let (nu, nv) = ((u + du).clamp(u0, u1), (v + dv).clamp(v0, v1));
                let moved = (nu - u).abs() + (nv - v).abs();
                (u, v) = (nu, nv);
                if moved < 1e-15 * (1.0 + u.abs() + v.abs()) {
                    break;
                }
            }
            return P2::new(u, v);
        }
        let (u0, u1) = (ns.ku[0], ns.ku[ns.ku.len() - 1]);
        let at = |u: f64| -> (f64, f64) {
            let a = self.surf().eval(u, 0.0);
            let d = self.surf().eval(u, 1.0) - a;
            let dd = d.norm_squared();
            let v = if dd > 0.0 {
                ((x - a).dot(&d) / dd).clamp(0.0, 1.0)
            } else {
                0.0
            };
            ((a + d * v - x).norm(), v)
        };
        let n = 64;
        let best = (0..=n)
            .map(|i| u0 + (u1 - u0) * i as f64 / n as f64)
            .min_by(|&p, &q| at(p).0.total_cmp(&at(q).0))
            .unwrap_or(u0);
        let h = (u1 - u0) / n as f64;
        let (mut lo, mut hi) = ((best - h).max(u0), (best + h).min(u1));
        let g = 0.5 * (5f64.sqrt() - 1.0);
        for _ in 0..80 {
            let (c, d) = (hi - g * (hi - lo), lo + g * (hi - lo));
            if at(c).0 < at(d).0 {
                hi = d;
            } else {
                lo = c;
            }
        }
        let u = 0.5 * (lo + hi);
        P2::new(u, at(u).1)
    }

    /// Whether (u, v) is on the face (within `tol`, model units).
    fn holds(&self, uv: P2, tol: f64) -> bool {
        super::classify::in_face(&self.polys, self.surf(), uv, tol)
            != super::classify::InFace::Outside
    }
}

/// A point where two surfaces meet: (u, v) on the first, then on the second.
type Q = [f64; 4];

fn uv_of(q: &Q, k: usize) -> P2 {
    P2::new(q[2 * k], q[2 * k + 1])
}

/// The (u, v) of the point of `s` nearest `x` near `uv`, by Gauss-Newton steps from it.
fn nearest_uv(s: &Surface, mut uv: P2, x: &P3) -> P2 {
    for _ in 0..20 {
        let (p, su, sv) = s.d1(uv.x, uv.y);
        let r = p - x;
        let (a, b, c) = (su.dot(&su), su.dot(&sv), sv.dot(&sv));
        let (gu, gv) = (su.dot(&r), sv.dot(&r));
        let det = a * c - b * b;
        if det.abs() < 1e-300 {
            break;
        }
        let du = -(c * gu - b * gv) / det;
        let dv = -(a * gv - b * gu) / det;
        uv = P2::new(uv.x + du, uv.y + dv);
        if du.abs() + dv.abs() < 1e-15 * (1.0 + uv.x.abs() + uv.y.abs()) {
            break;
        }
    }
    uv
}

/// About how far apart on face `c` the places (u, v) and (u', v') are, in model units (the
/// surface's first derivatives at the first, times the difference).
fn metric(c: &Cand, uv: P2, wv: P2) -> f64 {
    let (_, su, sv) = c.surf().d1(uv.x, uv.y);
    (su * (wv.x - uv.x) + sv * (wv.y - uv.y)).norm()
}

/// The gap between the surfaces' points at q, and its derivatives in q's four parameters.
fn pair_eval(a: &Surface, b: &Surface, q: &Q) -> (V3, [V3; 4]) {
    let (pa, au, av) = a.d1(q[0], q[1]);
    let (pb, bu, bv) = b.d1(q[2], q[3]);
    (pa - pb, [au, av, -bu, -bv])
}

/// q moved onto the curve where the surfaces meet (Newton's, by the least change), if it gets
/// within `tol`.
fn pair_correct(a: &Surface, b: &Surface, mut q: Q, tol: f64) -> Option<Q> {
    for _ in 0..40 {
        let (f, j) = pair_eval(a, b, &q);
        if f.norm() <= tol {
            return Some(q);
        }
        let m = nalgebra::Matrix3::from_fn(|r, c| (0..4).map(|k| j[k][r] * j[k][c]).sum::<f64>());
        let y = m.lu().solve(&f)?;
        for k in 0..4 {
            q[k] -= j[k].dot(&y);
        }
        if q.iter().any(|x| !x.is_finite()) {
            return None;
        }
    }
    None
}

/// The direction of the curve where the surfaces meet at q, in q's parameters scaled to unit
/// speed in space, and that speed's direction.
fn pair_tangent(a: &Surface, b: &Surface, q: &Q) -> Option<(Q, V3)> {
    let (_, j) = pair_eval(a, b, q);
    let det = |x: V3, y: V3, z: V3| x.dot(&y.cross(&z));
    let n = [
        det(j[1], j[2], j[3]),
        -det(j[0], j[2], j[3]),
        det(j[0], j[1], j[3]),
        -det(j[0], j[1], j[2]),
    ];
    let vel = j[0] * n[0] + j[1] * n[1];
    let s = vel.norm();
    (s > 1e-300).then(|| (n.map(|x| x / s), vel / s))
}

/// A stretch of a curve along which two candidate faces meet: the faces, and points on it with
/// their (u, v) on each.
struct Seg {
    a: usize,
    b: usize,
    pts: Vec<(P3, P2, P2)>,
}

/// How a walk along a curve ended.
#[derive(Debug, Clone, Copy, PartialEq)]
enum End {
    /// At a face's boundary where no candidate face continues it.
    Edge,
    /// Where it goes inside the swept solid (a third face's cover).
    Inside,
    /// Back where it started.
    Closed,
    /// Stuck: no step along it found.
    Failed,
}

/// Follows the curves along which the candidate faces meet on the boundary of the swept solid
/// (where one face goes inside the region another bounds).
struct Tracer<'a> {
    cands: &'a [Cand],
    oracle: &'a Oracle<'a>,
    size: f64,
    eps: f64,
    /// The pairs of faces whose curves are given whole (never traced, nor walked onto).
    given: &'a std::collections::HashSet<(usize, usize)>,
    /// How deep inside a point may lie and still count as on the boundary (a face another
    /// grazes, dipping inside it by less, is kept whole: the curve between two faces nearly
    /// tangent is ill-defined, and leaving the dip changes the solid by less than this).
    shallow: f64,
}

impl Tracer<'_> {
    fn point(&self, a: usize, q: &Q) -> P3 {
        self.cands[a].surf().eval(q[0], q[1])
    }

    fn entry(&self, a: usize, q: &Q) -> (P3, P2, P2) {
        (self.point(a, q), uv_of(q, 0), uv_of(q, 1))
    }

    /// Whether the point of q, where faces a and b meet, is on the swept solid's boundary
    /// rather than inside it (covered by the body at some other instant): no instant puts it
    /// inside the body by more than the faces' fit.
    fn on_boundary(&self, a: usize, q: &Q) -> bool {
        self.oracle.depth(&self.point(a, q)).0 > -self.shallow
    }

    /// Whether `x` lies on one of face `c`'s edges (within `tol`).
    fn on_edge(&self, c: usize, x: &P3, tol: f64) -> bool {
        self.edge_dir_at(c, x, tol).is_some()
    }

    /// The direction of the edge of face `c` that `x` lies on (within `tol`), if any.
    fn edge_dir_at(&self, c: usize, x: &P3, tol: f64) -> Option<V3> {
        let b = &self.cands[c].body;
        b.edges.iter().filter(|e| !e.degenerate).find_map(|e| {
            let s = e.curve.closest(x, e.t[0], e.t[1]);
            ((e.curve.point(s) - x).norm() <= tol).then(|| e.curve.tangent(s).normalize())
        })
    }

    fn inside(&self, a: usize, b: usize, q: &Q, tol: f64) -> bool {
        self.cands[a].holds(uv_of(q, 0), tol) && self.cands[b].holds(uv_of(q, 1), tol)
    }

    /// The candidate faces other than `not` that `x` lies on, with its (u, v) on each: those
    /// the oracle locates at the instants the body touches `x` (the right place on a face whose
    /// surface passes through `x` twice, where projection finds the nearer), then any other by
    /// projection.
    fn faces_at(&self, x: &P3, not: &[usize], tol: f64) -> Vec<(usize, P2)> {
        let mut out: Vec<(usize, P2)> = Vec::new();
        if let Some(locate) = &self.oracle.locate {
            for (v, t) in self.oracle.leasts(x) {
                if v.abs() > tol {
                    continue;
                }
                for (i, uv) in locate(t, x) {
                    // (A face once for each place it has the point at.)
                    let again = out.iter().any(|(j, w)| {
                        *j == i && {
                            let c = &self.cands[i];
                            (uv.x - w.x).abs() <= 1e-6 * c.uv_span.x
                                && (uv.y - w.y).abs() <= 1e-6 * c.uv_span.y
                        }
                    });
                    if !not.contains(&i)
                        && !again
                        && (self.cands[i].surf().eval(uv.x, uv.y) - x).norm() < tol
                        && self.cands[i].holds(uv, tol)
                    {
                        out.push((i, uv));
                    }
                }
            }
        }
        for (i, c) in self.cands.iter().enumerate() {
            if not.contains(&i) || out.iter().any(|(j, _)| *j == i) || !c.bbox.grow(tol).contains(x)
            {
                continue;
            }
            let uv = c.project(x);
            if (c.surf().eval(uv.x, uv.y) - x).norm() < tol && c.holds(uv, tol) {
                out.push((i, uv));
            }
        }
        out
    }

    /// Where the curve of faces a and b near q leaves face `k`'s domain (0: a's, 1: b's),
    /// exactly: on one of its boundary edges (the nearest that works), the point where the
    /// edge meets the other face.
    fn exit_on_edge(&self, a: usize, b: usize, q: &Q, k: usize) -> Option<Q> {
        let (leave, other) = if k == 0 { (a, b) } else { (b, a) };
        let lc = &self.cands[leave];
        let x = self.point(a, q);
        let uo = uv_of(q, 1 - k);
        let mut edges: Vec<(f64, usize, f64, Curve)> = Vec::new();
        for co in lc.body.faces[0].loops.iter().flatten() {
            let e = &lc.body.edges[co.edge];
            if e.degenerate {
                continue;
            }
            let s = e.curve.closest(&x, e.t[0], e.t[1]);
            edges.push(((e.curve.point(s) - x).norm(), co.edge, s, co.pc.clone()));
        }
        edges.sort_by(|p, q| p.0.total_cmp(&q.0));
        let so = self.cands[other].surf();
        for (_, ei, s0, pc) in edges.into_iter().take(3) {
            let e = &lc.body.edges[ei];
            let (mut s, mut u, mut v) = (s0, uo.x, uo.y);
            for _ in 0..40 {
                let (p, dp, _) = e.curve.d2(s);
                let (po, su, sv) = so.d1(u, v);
                let f = p - po;
                if f.norm() <= 1e-13 * self.size {
                    let span = e.t[1] - e.t[0];
                    if s < e.t[0] - 1e-9 * span || s > e.t[1] + 1e-9 * span {
                        break;
                    }
                    let w = pc.point(s.clamp(e.t[0], e.t[1]));
                    let q = if k == 0 {
                        [w.x, w.y, u, v]
                    } else {
                        [u, v, w.x, w.y]
                    };
                    return Some(q);
                }
                let m = nalgebra::Matrix3::from_columns(&[dp, -su, -sv]);
                let Some(d) = m.lu().solve(&f) else { break };
                s -= d[0];
                u -= d[1];
                v -= d[2];
                if !(s.is_finite() && u.is_finite() && v.is_finite()) {
                    break;
                }
            }
        }
        None
    }

    /// A corner both faces have (where their curve may end, the faces meeting there, as at an
    /// event, often touching) within `reach` of `x`, with its (u, v) on each.
    fn shared_corner(&self, a: usize, b: usize, x: &P3, reach: f64) -> Option<(P3, Q)> {
        if a == b {
            return None;
        }
        let tol = 1e-9 * self.size;
        let (ca, cb) = (&self.cands[a].body, &self.cands[b].body);
        ca.verts
            .iter()
            .map(|v| v.p)
            .filter(|p| (p - x).norm() <= reach)
            .find(|p| cb.verts.iter().any(|w| (w.p - p).norm() <= tol))
            .map(|p| {
                let (ua, ub) = (self.cands[a].project(&p), self.cands[b].project(&p));
                (p, [ua.x, ua.y, ub.x, ub.y])
            })
    }

    /// The point of the boundary of both faces nearest the point of `q` (within `reach` on
    /// each; the nearest edge point of `a`, with its (u, v) on each face), where a curve that
    /// can't go on ends.
    fn boundary_end(&self, a: usize, b: usize, q: &Q, reach: f64) -> Option<Q> {
        let x = self.point(a, q);
        let nearest = |c: usize| -> Option<(f64, P3, P2)> {
            let body = &self.cands[c].body;
            let mut best: Option<(f64, P3, P2)> = None;
            for co in body.faces[0].loops.iter().flatten() {
                let e = &body.edges[co.edge];
                if e.degenerate {
                    continue;
                }
                let s = e.curve.closest(&x, e.t[0], e.t[1]);
                let p = e.curve.point(s);
                let d = (p - x).norm();
                if d <= reach && best.is_none_or(|(bd, ..)| d < bd) {
                    let w = co.pc.point(s);
                    best = Some((d, p, P2::new(w.x, w.y)));
                }
            }
            best
        };
        let (_, _, ua) = nearest(a)?;
        let (_, _, ub) = nearest(b)?;
        Some([ua.x, ua.y, ub.x, ub.y])
    }

    /// Walks from q (faces a and b meeting there) one way (`dir`), across the faces that
    /// continue either, until the curve leaves the boundary or comes back to `start`.
    fn walk(&self, a0: usize, b0: usize, q0: Q, dir: f64, start: &P3) -> (Vec<Seg>, End) {
        let (mut a, mut b, mut q) = (a0, b0, q0);
        let mut segs = vec![Seg {
            a,
            b,
            pts: vec![self.entry(a, &q)],
        }];
        let hmax = 4e-3 * self.size;
        let hmin = 1e-10 * self.size;
        let ctol = 1e-12 * self.size;
        let mut h = 0.25 * hmax;
        let Some((_, v0)) = pair_tangent(self.cands[a].surf(), self.cands[b].surf(), &q) else {
            return (segs, End::Failed);
        };
        let mut prev = v0 * dir;
        let mut length = 0.0;
        for _ in 0..100_000 {
            let (sa, sb) = (self.cands[a].surf(), self.cands[b].surf());
            let Some((dq, vel)) = pair_tangent(sa, sb, &q) else {
                return (segs, End::Failed);
            };
            let s = if vel.dot(&prev) >= 0.0 { 1.0 } else { -1.0 };
            let (dq, vel) = (dq.map(|x| x * s), vel * s);
            let ahead = |f: f64| -> Option<Q> {
                pair_correct(sa, sb, std::array::from_fn(|k| q[k] + dq[k] * h * f), ctol)
            };
            let x = self.point(a, &q);
            let next = ahead(1.0).and_then(|qn| {
                let xn = self.point(a, &qn);
                let step = (xn - x).norm();
                let (_, vn) = pair_tangent(sa, sb, &qn)?;
                // (Small on each face too: a thin face's (u, v) moves fast.)
                let small = (0..2).all(|k| {
                    let c = &self.cands[[a, b][k]];
                    let d = uv_of(&qn, k) - uv_of(&q, k);
                    d.x.abs() <= 0.02 * c.uv_span.x && d.y.abs() <= 0.02 * c.uv_span.y
                });
                (step > 0.5 * h && step < 1.5 * h && vn.dot(&vel).abs() > 0.99 && small)
                    .then_some(qn)
            });
            let Some(qn) = next else {
                h *= 0.5;
                if h < hmin {
                    // (Stuck where the faces touch: at a corner they share, its end.)
                    if let Some((c, qc)) = self.shared_corner(a, b, &x, 1e-4 * self.size) {
                        segs.last_mut()
                            .unwrap()
                            .pts
                            .push((c, uv_of(&qc, 0), uv_of(&qc, 1)));
                        return (segs, End::Edge);
                    }
                    // (Stuck where the faces become tangent at their boundaries, the two
                    // branches of a sheet crossing itself merging at the tip of a fold: the
                    // curve ends on the edges there.)
                    let merging = a == b && {
                        let c = &self.cands[a];
                        let (p, r) = (uv_of(&q, 0), uv_of(&q, 1));
                        (p.x - r.x).abs() < 1e-3 * c.uv_span.x
                            && (p.y - r.y).abs() < 1e-3 * c.uv_span.y
                    };
                    let reach = if merging { 1e-3 } else { 1e-4 } * self.size;
                    if let Some(qe) = self.boundary_end(a, b, &q, reach) {
                        segs.last_mut().unwrap().pts.push(self.entry(a, &qe));
                        return (segs, End::Edge);
                    }
                    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                        eprintln!(
                            "  walk of faces ({a}, {b}) stuck at {x:?} ({:?}, {:?}; merging {merging})",
                            uv_of(&q, 0),
                            uv_of(&q, 1)
                        );
                    }
                    return (segs, End::Failed);
                }
                continue;
            };
            // (A face crossing itself, its two places coming together: the crossing ends there,
            // at the tip of a fold, on the face's boundary.)
            if a == b {
                let c = &self.cands[a];
                let (p, r) = (uv_of(&qn, 0), uv_of(&qn, 1));
                if (p.x - r.x).abs() < 1e-3 * c.uv_span.x && (p.y - r.y).abs() < 1e-3 * c.uv_span.y
                {
                    if let Some(qe) = self.boundary_end(a, b, &q, 1e-2 * self.size) {
                        segs.last_mut().unwrap().pts.push(self.entry(a, &qe));
                        return (segs, End::Edge);
                    }
                    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                        eprintln!(
                            "  walk of face {a} with itself: its places merge at {p:?}, {r:?}, away from its boundary"
                        );
                    }
                    return (segs, End::Failed);
                }
            }
            let (ina, inb) = (
                self.cands[a].holds(uv_of(&qn, 0), 0.0),
                self.cands[b].holds(uv_of(&qn, 1), 0.0),
            );
            if !(ina && inb) {
                // Where it leaves a face (by bisection on the step, then exactly onto the
                // edge it crosses), and the face that goes on.
                let (mut lo, mut hi) = (0.0, 1.0);
                let mut qe = qn;
                for _ in 0..60 {
                    let m = 0.5 * (lo + hi);
                    match ahead(m) {
                        Some(qm) if self.inside(a, b, &qm, 0.0) => lo = m,
                        Some(qm) => {
                            hi = m;
                            qe = qm;
                        }
                        None => hi = m,
                    }
                }
                let (ina, inb) = (
                    self.cands[a].holds(uv_of(&qe, 0), 0.0),
                    self.cands[b].holds(uv_of(&qe, 1), 0.0),
                );
                let x0 = self.point(a, &q);
                let exits: Vec<(Q, usize)> = [(!ina, 0), (!inb, 1)]
                    .into_iter()
                    .filter(|e| e.0)
                    .filter_map(|(_, k)| self.exit_on_edge(a, b, &qe, k).map(|q| (q, k)))
                    .collect();
                // (Leaving both at once: the one it meets first.)
                let first = exits
                    .iter()
                    .min_by(|p, r| {
                        let dp = (self.point(a, &p.0) - x0).norm();
                        let dr = (self.point(a, &r.0) - x0).norm();
                        dp.total_cmp(&dr)
                    })
                    .copied();
                let (ina, inb) = match first {
                    Some((qx, k)) => {
                        qe = qx;
                        (k != 0, k != 1)
                    }
                    None => (ina, inb),
                };
                segs.last_mut().unwrap().pts.push(self.entry(a, &qe));
                let xe = self.point(a, &qe);
                let tol = 1e-6 * self.size;
                // (The face that goes on may be the other face of the pair, at another place
                // on it: a face crossing itself.)
                let mut options: Vec<(usize, usize, Q)> = Vec::new();
                let near = self.faces_at(&xe, &[], tol);
                let elsewhere = |c: usize, uv: P2, k: usize| -> bool {
                    let f = [a, b][k];
                    c != f || {
                        let cand = &self.cands[c];
                        let w = uv_of(&qe, k);
                        (uv.x - w.x).abs() > 1e-6 * cand.uv_span.x
                            || (uv.y - w.y).abs() > 1e-6 * cand.uv_span.y
                    }
                };
                if !ina {
                    for &(c, uv) in &near {
                        if c != a && elsewhere(c, uv, 1) {
                            options.push((c, b, [uv.x, uv.y, qe[2], qe[3]]));
                        }
                    }
                }
                if !inb {
                    for &(c, uv) in &near {
                        if c != b && elsewhere(c, uv, 0) {
                            options.push((a, c, [qe[0], qe[1], uv.x, uv.y]));
                        }
                    }
                }
                if !ina && !inb {
                    for &(c, uc) in &near {
                        for &(d, ud) in &near {
                            if c != a && d != b && (c != d || (uc - ud).norm() > 0.0) {
                                options.push((c, d, [uc.x, uc.y, ud.x, ud.y]));
                            }
                        }
                    }
                }
                // The pair that goes on: corrected onto its curve, a step along it (either way,
                // that along the way it came first) on both faces.
                let mut found = None;
                let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
                'options: for (na, nb, nq) in options {
                    if self.given.contains(&(na.min(nb), na.max(nb))) {
                        // (Given whole: the curve on from here is in the network already.)
                        continue;
                    }
                    let (ta, tb) = (self.cands[na].surf(), self.cands[nb].surf());
                    let Some(nq) = pair_correct(ta, tb, nq, ctol) else {
                        if debug {
                            eprintln!("    option ({na}, {nb}): no crossing found");
                        }
                        continue;
                    };
                    // (Faces that only touch there, along an edge they share, folded back on
                    // each other: no crossing to follow.)
                    let (n1, n2) = (
                        self.cands[na].normal(uv_of(&nq, 0)),
                        self.cands[nb].normal(uv_of(&nq, 1)),
                    );
                    let sin = n1.cross(&n2).norm();
                    let _ = &nq;
                    if sin < 1e-6 {
                        if debug {
                            eprintln!("    option ({na}, {nb}): tangent (sin {sin:.2e})");
                        }
                        continue;
                    }
                    let Some((ndq, nv)) = pair_tangent(ta, tb, &nq) else {
                        if debug {
                            eprintln!("    option ({na}, {nb}): no direction");
                        }
                        continue;
                    };

                    let first = if nv.dot(&vel) >= 0.0 { 1.0 } else { -1.0 };
                    let mut why = String::new();
                    for sg in [first, -first] {
                        for h1 in [1e-6, 1e-5, 1e-4] {
                            let h1 = h1 * self.size;
                            let Some(q1) = pair_correct(
                                ta,
                                tb,
                                std::array::from_fn(|k| nq[k] + ndq[k] * sg * h1),
                                ctol,
                            ) else {
                                why += "no step; ";
                                continue;
                            };
                            // (Not along an edge the two faces share.)
                            let x1 = self.point(na, &q1);
                            let etol = 1e-9 * self.size;
                            let shared_edge = na != nb
                                && self.on_edge(na, &x1, etol)
                                && self.on_edge(nb, &x1, etol);
                            if self.inside(na, nb, &q1, 0.0)
                                && self.on_boundary(na, &q1)
                                && !shared_edge
                            {
                                found = Some((na, nb, nq, nv * sg));
                                break 'options;
                            }
                            if debug {
                                why += &format!(
                                    "{}{}{}; ",
                                    if self.inside(na, nb, &q1, 0.0) {
                                        ""
                                    } else {
                                        "outside "
                                    },
                                    if self.on_boundary(na, &q1) {
                                        ""
                                    } else {
                                        "covered "
                                    },
                                    if shared_edge { "on a shared edge" } else { "" }
                                );
                            }
                        }
                    }
                    if debug {
                        eprintln!(
                            "    option ({na}, {nb}) at {:?}: no step on the boundary ({why})",
                            (uv_of(&nq, 0), uv_of(&nq, 1))
                        );
                    }
                }
                let Some((na, nb, nq, nvel)) = found else {
                    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                        let close: Vec<String> = self
                            .cands
                            .iter()
                            .enumerate()
                            .filter(|(c, g)| *c != a && *c != b && g.bbox.grow(1e-3 * self.size).contains(&xe))
                            .map(|(c, g)| {
                                let uv = g.project(&xe);
                                let (lo, hi) = grid_box(g);
                                format!(
                                    "{c} ({:.4}..{:.4}, {:.4}..{:.4}) at {uv:?} off {:.2e} holds {}",
                                    lo.x, hi.x, lo.y, hi.y,
                                    (g.surf().eval(uv.x, uv.y) - xe).norm(),
                                    g.holds(uv, tol)
                                )
                            })
                            .collect();
                        eprintln!(
                            "  curve of faces ({a}, {b}) left {} at {xe:?} ({:?} on a, {:?} on b; in a {ina}, in b {inb}); faces there: {:?}; faces near: {close:?}",
                            if !ina && !inb {
                                "both"
                            } else if !ina {
                                "the first"
                            } else {
                                "the second"
                            },
                            uv_of(&qe, 0),
                            uv_of(&qe, 1),
                            near.iter().map(|(c, _)| *c).collect::<Vec<_>>()
                        );
                    }
                    return (segs, End::Edge);
                };
                a = na;
                b = nb;
                q = nq;
                segs.push(Seg {
                    a,
                    b,
                    pts: vec![self.entry(a, &q)],
                });
                prev = nvel;
                continue;
            }
            if !self.on_boundary(a, &qn) {
                // Where it goes inside.
                let (mut lo, mut hi) = (0.0, 1.0);
                let mut qt = q;
                for _ in 0..60 {
                    let m = 0.5 * (lo + hi);
                    let Some(qm) = ahead(m) else { break };
                    if self.on_boundary(a, &qm) {
                        lo = m;
                        qt = qm;
                    } else {
                        hi = m;
                    }
                }
                segs.last_mut().unwrap().pts.push(self.entry(a, &qt));
                return (segs, End::Inside);
            }
            let xn = self.point(a, &qn);
            // Coming to a corner both faces have: the curve ends there (where the faces, meeting
            // at an event, may touch and the curve be hard to follow).
            if let Some((c, qc)) = self.shared_corner(a, b, &xn, 1e-4 * self.size)
                && (c - xn).norm() < (c - x).norm()
            {
                segs.last_mut()
                    .unwrap()
                    .pts
                    .push((c, uv_of(&qc, 0), uv_of(&qc, 1)));
                return (segs, End::Edge);
            }
            length += (xn - x).norm();
            segs.last_mut().unwrap().pts.push(self.entry(a, &qn));
            q = qn;
            prev = vel;
            if length > 4.0 * hmax && (xn - start).norm() < h {
                return (segs, End::Closed);
            }
            h = (h * 1.5).min(hmax);
        }
        (segs, End::Failed)
    }
}

/// Where three surfaces meet near a start: (u, v) on each, by Newton's method (two gaps, six
/// parameters), if it gets within `tol`.
fn triple_point(s: [&Surface; 3], start: [P2; 3], tol: f64) -> Option<[P2; 3]> {
    let mut q: [f64; 6] = [
        start[0].x, start[0].y, start[1].x, start[1].y, start[2].x, start[2].y,
    ];
    for _ in 0..40 {
        let d: Vec<(P3, V3, V3)> = (0..3).map(|k| s[k].d1(q[2 * k], q[2 * k + 1])).collect();
        let f1 = d[0].0 - d[1].0;
        let f2 = d[0].0 - d[2].0;
        if f1.norm().max(f2.norm()) <= tol {
            return Some(std::array::from_fn(|k| P2::new(q[2 * k], q[2 * k + 1])));
        }
        let mut j = nalgebra::SMatrix::<f64, 6, 6>::zeros();
        let mut r = nalgebra::SVector::<f64, 6>::zeros();
        for i in 0..3 {
            r[i] = f1[i];
            r[3 + i] = f2[i];
            j[(i, 0)] = d[0].1[i];
            j[(i, 1)] = d[0].2[i];
            j[(i, 2)] = -d[1].1[i];
            j[(i, 3)] = -d[1].2[i];
            j[(3 + i, 0)] = d[0].1[i];
            j[(3 + i, 1)] = d[0].2[i];
            j[(3 + i, 4)] = -d[2].1[i];
            j[(3 + i, 5)] = -d[2].2[i];
        }
        let dx = j.lu().solve(&r)?;
        for k in 0..6 {
            q[k] -= dx[k];
        }
        if q.iter().any(|x| !x.is_finite()) {
            return None;
        }
    }
    None
}

/// How near two ends of curves given whole are one vertex, in the measure of the size.
const GIVEN_VERTEX: f64 = 1e-7;

/// A stretch of the network: where faces `a` and `b` meet between two of its vertices, the
/// points traced along it (with their (u, v) on each face).
struct Stretch {
    a: usize,
    b: usize,
    pts: Vec<(P3, P2, P2)>,
    v: [usize; 2],
    /// Given by construction, its points as dense as it needs: taken as it is.
    given: bool,
}

/// The curves along which the candidate faces meet on the swept solid's boundary: stretches
/// between vertices (where a curve passes from one face to the next, ends on a face's edge, or
/// meets two others where a third face covers it).
#[derive(Default)]
struct Network {
    verts: Vec<P3>,
    stretches: Vec<Stretch>,
    /// Vertices where three curves meet, with the faces.
    triples: Vec<(usize, [usize; 3])>,
    /// Problems met.
    trouble: Vec<String>,
}

impl Network {
    fn vertex(&mut self, p: P3, tol: f64) -> usize {
        if let Some(i) = self.verts.iter().position(|q| (q - p).norm() <= tol) {
            return i;
        }
        self.verts.push(p);
        self.verts.len() - 1
    }

    /// Whether `p` lies on a stretch traced already (within `tol` of its polyline).
    fn near(&self, p: &P3, tol: f64) -> bool {
        self.near_by(p, tol, |_| true)
    }

    /// Whether `p` is within `tol` of a stretch of faces `a` and `b`.
    fn near_pair(&self, p: &P3, tol: f64, a: usize, b: usize) -> bool {
        self.near_by(p, tol, |s| (s.a, s.b) == (a, b) || (s.a, s.b) == (b, a))
    }

    fn near_by(&self, p: &P3, tol: f64, which: impl Fn(&Stretch) -> bool) -> bool {
        self.stretches.iter().filter(|s| which(s)).any(|s| {
            s.pts.windows(2).any(|w| {
                let (a, b) = (w[0].0, w[1].0);
                let d = b - a;
                let t = if d.norm_squared() > 0.0 {
                    ((p - a).dot(&d) / d.norm_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (a + d * t - p).norm() <= tol
            })
        })
    }

    /// The walk's stretches added (their ends made vertices: `first` at its start when given).
    fn add(&mut self, segs: Vec<Seg>, first: Option<usize>, tol: f64) -> Option<usize> {
        let mut prev = first;
        let mut last = None;
        for mut sg in segs {
            if sg.pts.len() < 2 {
                continue;
            }
            let v0 = match prev {
                Some(v) => v,
                None => self.vertex(sg.pts[0].0, tol),
            };
            sg.pts[0].0 = self.verts[v0];
            let n = sg.pts.len();
            let v1 = self.vertex(sg.pts[n - 1].0, tol);
            sg.pts[n - 1].0 = self.verts[v1];
            // (A stretch no longer than the vertices' tolerance, back where it started: nothing.)
            if v0 == v1 {
                let len: f64 = sg.pts.windows(2).map(|w| (w[1].0 - w[0].0).norm()).sum();
                if len < 4.0 * tol {
                    continue;
                }
            }
            self.stretches.push(Stretch {
                a: sg.a,
                b: sg.b,
                pts: sg.pts,
                v: [v0, v1],
                given: false,
            });
            prev = Some(v1);
            last = Some(v1);
        }
        last
    }
}

/// The network of curves along which the mixed faces go inside the swept solid: from points
/// between a sample of a face outside and one inside, onto the face that covers it there, each
/// curve followed across the faces; where one goes inside (a third face covering it), the two
/// curves that meet it there followed from that point.
fn network(tr: &Tracer, mixed: &[usize], m: usize, given: Vec<Given>, complete: bool) -> Network {
    let size = tr.size;
    // (Curves' ends within this of each other are one vertex: walks from different seeds end
    // at one place only to about the step they were bisected in, and a curve's end where it
    // goes inside is placed by the depth, which changes slowly where it grazes the third face.)
    // (Curves given whole end where their makers put them, each end found as finely as the
    // curve: only ends that are one point come within a ten-millionth of the size.)
    let vtol = if complete {
        GIVEN_VERTEX * size
    } else {
        1e-5 * size
    };
    let mut net = Network::default();
    // The curves known by construction first: seeds on them are passed over, and no curve of
    // two faces given one is traced (the given one is the whole of it).
    let given_pairs = tr.given;
    // (Each given curve cut where it goes inside the solid, under a third face: the parts
    // inside left out, and each end there a triple point, as a traced curve's is.)
    let mut given_ends: Vec<(usize, usize)> = Vec::new();
    let mut given = given;
    given.retain(|g| g.pts.len() >= 2);
    if complete {
        // The whole boundary's curves, each the part of it on the boundary: as they are. Two
        // ends are one vertex where they are one place on a face they share (each end is
        // placed exactly on its two faces; where a face folds, places far apart on it are a
        // hair apart in space, and are not one).
        let extent: Vec<f64> = tr
            .cands
            .iter()
            .map(|c| {
                let (lo, hi) = grid_box(c);
                (hi - lo).norm()
            })
            .collect();
        let end = |g: &Given, e: usize| {
            if e == 0 {
                g.pts[0]
            } else {
                g.pts[g.pts.len() - 1]
            }
        };
        let mut root: Vec<usize> = (0..2 * given.len()).collect();
        fn find(root: &mut [usize], mut i: usize) -> usize {
            while root[i] != i {
                root[i] = root[root[i]];
                i = root[i];
            }
            i
        }
        // (An end's places: (end, face, (u, v), point).)
        let mut places: Vec<(usize, usize, P2, P3)> = Vec::new();
        for (gi, g) in given.iter().enumerate() {
            for e in 0..2 {
                let p = end(g, e);
                places.push((2 * gi + e, g.a, p.1, p.0));
                places.push((2 * gi + e, g.b, p.2, p.0));
            }
        }
        places.sort_by_key(|p| p.1);
        for (i, p) in places.iter().enumerate() {
            for q in places[i + 1..].iter().take_while(|q| q.1 == p.1) {
                if (p.2 - q.2).norm() <= 1e-7 * extent[p.1] && (p.3 - q.3).norm() <= 1e-4 * size {
                    let (a, b) = (find(&mut root, p.0), find(&mut root, q.0));
                    root[a.max(b)] = a.min(b);
                }
            }
        }
        let mut vertex_of: HashMap<usize, usize> = HashMap::new();
        for (gi, g) in std::mem::take(&mut given).into_iter().enumerate() {
            let mut v = [0; 2];
            for (e, slot) in v.iter_mut().enumerate() {
                let r = find(&mut root, 2 * gi + e);
                *slot = *vertex_of.entry(r).or_insert_with(|| {
                    net.verts.push(end(&g, e).0);
                    net.verts.len() - 1
                });
            }
            let mut pts = g.pts;
            let n = pts.len();
            pts[0].0 = net.verts[v[0]];
            pts[n - 1].0 = net.verts[v[1]];
            // (Back where it started over no length: nothing.)
            let len: f64 = pts.windows(2).map(|w| (w[1].0 - w[0].0).norm()).sum();
            if v[0] == v[1] && len < 1e-9 * size {
                continue;
            }
            net.stretches.push(Stretch {
                a: g.a,
                b: g.b,
                pts,
                v,
                given: true,
            });
        }
    }
    for g in given {
        let on: Vec<bool> = g
            .pts
            .iter()
            .map(|p| tr.oracle.depth(&p.0).0 > -1e-6 * size)
            .collect();
        let mut run: Vec<(P3, P2, P2)> = Vec::new();
        let mut run_ends: [bool; 2] = [false, false];
        let mut flush = |run: &mut Vec<(P3, P2, P2)>, ends: [bool; 2], net: &mut Network| {
            if run.len() >= 2 {
                let n0 = net.stretches.len();
                net.add(
                    vec![Seg {
                        a: g.a,
                        b: g.b,
                        pts: std::mem::take(run),
                    }],
                    None,
                    vtol,
                );
                if net.stretches.len() > n0 {
                    let si = net.stretches.len() - 1;
                    net.stretches[si].given = true;
                    for (k, inside) in ends.iter().enumerate() {
                        if *inside {
                            given_ends.push((si, k));
                        }
                    }
                }
            }
            run.clear();
        };
        for i in 0..g.pts.len() {
            if on[i] {
                if run.is_empty() && i > 0 && !on[i - 1] {
                    // From inside to the boundary between the points: the point where.
                    let (mut lo, mut hi) = (g.pts[i - 1], g.pts[i]);
                    for _ in 0..40 {
                        let m = (
                            P3::from(0.5 * (lo.0.coords + hi.0.coords)),
                            P2::from(0.5 * (lo.1.coords + hi.1.coords)),
                            P2::from(0.5 * (lo.2.coords + hi.2.coords)),
                        );
                        if tr.oracle.depth(&m.0).0 > -1e-9 * size {
                            hi = m;
                        } else {
                            lo = m;
                        }
                    }
                    run.push(hi);
                    run_ends = [true, false];
                }
                run.push(g.pts[i]);
            } else if !run.is_empty() {
                let (mut lo, mut hi) = (g.pts[i - 1], g.pts[i]);
                for _ in 0..40 {
                    let m = (
                        P3::from(0.5 * (lo.0.coords + hi.0.coords)),
                        P2::from(0.5 * (lo.1.coords + hi.1.coords)),
                        P2::from(0.5 * (lo.2.coords + hi.2.coords)),
                    );
                    if tr.oracle.depth(&m.0).0 > -1e-9 * size {
                        lo = m;
                    } else {
                        hi = m;
                    }
                }
                run.push(lo);
                run_ends[1] = true;
                flush(&mut run, run_ends, &mut net);
                run_ends = [false, false];
            }
        }
        flush(&mut run, run_ends, &mut net);
    }
    // Curve ends to follow from: (faces, q, the way along it as a 3D direction, start vertex).
    let mut queue: Vec<(usize, usize, Q, V3, usize)> = Vec::new();
    // A curve's end where it goes inside: the third face, the exact point where the three
    // meet, and the two curves that meet it there followed on from it.
    let end_inside = |net: &mut Network,
                      queue: &mut Vec<(usize, usize, Q, V3, usize)>,
                      si: usize,
                      k: usize| {
        let s = &net.stretches[si];
        let (a, b, lv) = (s.a, s.b, s.v[k]);
        let (x, ua, ub) = if k == 0 {
            s.pts[0]
        } else {
            s.pts[s.pts.len() - 1]
        };
        if net.triples.iter().any(|(v, _)| *v == lv) {
            // (Found before, from another curve: this one's end put on it.)
            let p = net.verts[lv];
            let st = &mut net.stretches[si];
            let at = if k == 0 { 0 } else { st.pts.len() - 1 };
            st.pts[at] = (p, tr.cands[a].project(&p), tr.cands[b].project(&p));
            return;
        }
        let mut best: Option<(f64, usize, [P2; 3])> = None;
        // (The third face may be one of the two, at another place on it: a face crossing
        // itself.)
        let elsewhere = |h: usize, uh: P2| -> bool {
            [(a, ua), (b, ub)].iter().all(|&(f, uf)| {
                h != f || {
                    let c = &tr.cands[h];
                    (uh.x - uf.x).abs() > 1e-4 * c.uv_span.x
                        || (uh.y - uf.y).abs() > 1e-4 * c.uv_span.y
                }
            })
        };
        for (h, uh) in tr.faces_at(&x, &[], 1e-5 * size) {
            if !elsewhere(h, uh) {
                continue;
            }
            let sf = [tr.cands[a].surf(), tr.cands[b].surf(), tr.cands[h].surf()];
            let Some(t) = triple_point(sf, [ua, ub, uh], 1e-12 * size) else {
                continue;
            };
            let d = (sf[0].eval(t[0].x, t[0].y) - x).norm();
            if !elsewhere(h, t[2]) {
                continue;
            }
            // (Where the curve grazes the third face, the point it was found to go inside
            // at stands well along it from where the three meet.)
            if d < 1e-3 * size && best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, h, t));
            }
        }
        // (No point where three meet: two of them tangent there, a face and the one it
        // runs on into smoothly across an edge. The curve goes on with the nearest other
        // face from where it stands.)
        let best = best.or_else(|| {
            tr.faces_at(&x, &[], 1e-5 * size)
                .into_iter()
                .filter(|(h, uh)| *h != a && *h != b && elsewhere(*h, *uh))
                .map(|(h, uh)| {
                    let d = (tr.cands[h].surf().eval(uh.x, uh.y) - x).norm();
                    (d, h, [ua, ub, uh])
                })
                .min_by(|p, q| p.0.total_cmp(&q.0))
        });
        let Some((_, h, t)) = best else {
            if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                let ls: Vec<String> = tr
                    .oracle
                    .leasts(&x)
                    .iter()
                    .map(|(v, t)| {
                        let loc = tr.oracle.locate.as_ref().map(|l| l(*t, &x));
                        format!("t {t:.6} f {v:.3e} located {loc:?}")
                    })
                    .collect();
                eprintln!(
                    "  no third face for faces ({a}, {b}) at {x:?} ({ua:?}, {ub:?}): instants {ls:?}; faces there {:?}",
                    tr.faces_at(&x, &[], 1e-5 * size)
                );
            }
            net.trouble
                .push(format!("no third face where a curve goes inside at {x:?}"));
            return;
        };
        let p = tr.cands[a].surf().eval(t[0].x, t[0].y);
        net.verts[lv] = p;
        for st in &mut net.stretches {
            for e in 0..2 {
                if st.v[e] == lv {
                    let n = st.pts.len();
                    let at = if e == 0 { 0 } else { n - 1 };
                    let (ua, ub) = if (st.a, st.b) == (a, b) {
                        (t[0], t[1])
                    } else {
                        (st.pts[at].1, st.pts[at].2)
                    };
                    st.pts[at] = (p, ua, ub);
                }
            }
        }
        net.triples.push((lv, [a, b, h]));
        for (f, g, uf, ug) in [(a, h, t[0], t[2]), (b, h, t[1], t[2])] {
            let q: Q = [uf.x, uf.y, ug.x, ug.y];
            let (sf, sg) = (tr.cands[f].surf(), tr.cands[g].surf());
            let Some((dq, vel)) = pair_tangent(sf, sg, &q) else {
                net.trouble.push(format!("no direction at {p:?}"));
                continue;
            };
            // Each way a little: the one on the boundary.
            for sgn in [1.0, -1.0] {
                let h1 = 1e-4 * size;
                if let Some(q1) = pair_correct(
                    sf,
                    sg,
                    std::array::from_fn(|k| q[k] + dq[k] * sgn * h1),
                    1e-12 * size,
                ) && tr.inside(f, g, &q1, 0.0)
                    && tr.on_boundary(f, &q1)
                {
                    queue.push((f, g, q, vel * sgn, lv));
                }
            }
        }
    };
    let note = |net: &mut Network, end: End, segs: &[Seg]| {
        if matches!(end, End::Failed) {
            net.trouble.push("a curve's walk failed".into());
        }
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some()
            && let Some(last) = segs.last()
            && let Some(p) = last.pts.last()
        {
            eprintln!(
                "  walk of faces ({}, {}) ended {end:?} at {:?} ({:?} on the first, {:?} on the second) after {} points in {} stretches",
                last.a,
                last.b,
                p.0,
                p.1,
                p.2,
                last.pts.len(),
                segs.len()
            );
        }
    };
    // The curves queued from where others end, followed.
    macro_rules! drain {
        () => {
            while let Some((f, g, q, way, v)) = queue.pop() {
                let p = tr.point(f, &q);
                if given_pairs.contains(&(f.min(g), f.max(g))) {
                    // (Given whole: the curve on through here is in the network already.)
                    continue;
                }
                if net.stretches.iter().any(|s| {
                    (s.v[0] == v || s.v[1] == v) && ((s.a, s.b) == (f, g) || (s.a, s.b) == (g, f))
                }) {
                    continue;
                }
                // (A curve of these two faces already followed through there, from
                // elsewhere: a little way along, on it.)
                if let Some(dir) = way.try_normalize(1e-300)
                    && net.near_pair(&(p + dir * (1e-3 * size)), 2e-4 * size, f, g)
                {
                    continue;
                }
                let (sf, sg) = (tr.cands[f].surf(), tr.cands[g].surf());
                let dir = match pair_tangent(sf, sg, &q) {
                    Some((_, vel)) if vel.dot(&way) >= 0.0 => 1.0,
                    _ => -1.0,
                };
                if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                    eprintln!("  continuing faces ({f}, {g}) from vertex {v} at {p:?}");
                }
                let (segs, end) = tr.walk(f, g, q, dir, &p);
                let n0 = net.stretches.len();
                note(&mut net, end, &segs);
                net.add(segs, Some(v), vtol);
                if end == End::Inside && net.stretches.len() > n0 {
                    let last = net.stretches.len() - 1;
                    end_inside(&mut net, &mut queue, last, 1);
                }
            }
        };
    }
    for (si, k) in given_ends {
        end_inside(&mut net, &mut queue, si, k);
    }
    drain!();
    for &i in mixed.iter().filter(|_| !complete) {
        let c = &tr.cands[i];
        let samples = face_grid(c, m);
        let out_at = |uv: P2| {
            let x = c.surf().eval(uv.x, uv.y);
            tr.oracle.depth(&(x + c.normal(uv) * tr.eps)).0 > -tr.shallow
        };
        let class: Vec<Option<bool>> = samples.iter().map(|s| s.map(out_at)).collect();
        for k in 0..m * m {
            let (gi, gj) = (k / m, k % m);
            for (di, dj) in [(1, 0), (0, 1)] {
                if gi + di >= m || gj + dj >= m {
                    continue;
                }
                let l = (gi + di) * m + gj + dj;
                let (Some(ok), Some(ol)) = (class[k], class[l]) else {
                    continue;
                };
                if ok == ol {
                    continue;
                }
                let (mut uo, mut ui) = if ok {
                    (samples[k].unwrap(), samples[l].unwrap())
                } else {
                    (samples[l].unwrap(), samples[k].unwrap())
                };
                let (uo0, ui0) = (uo, ui);
                for _ in 0..50 {
                    let mid = P2::new(0.5 * (uo.x + ui.x), 0.5 * (uo.y + ui.y));
                    if out_at(mid) {
                        uo = mid;
                    } else {
                        ui = mid;
                    }
                }
                // Onto the exact crossing with the body at the instant that covers the inside
                // sample: where its own signed distance is zero along the samples' segment (a
                // face another grazes dips inside it slowly, and the point where the dip reaches
                // `shallow` can be far along the face from where it begins).
                let x_in = c.surf().eval(ui.x, ui.y);
                let at = |uv: P2| c.surf().eval(uv.x, uv.y);
                if let Some((_, t2)) = Some(tr.oracle.depth(&(x_in + c.normal(ui) * tr.eps)))
                    .filter(|d| d.0.is_finite())
                    && tr.oracle.at_instant(t2, &at(uo0)) > 0.0
                    && tr.oracle.at_instant(t2, &at(ui0)) < 0.0
                {
                    let (mut po, mut pi) = (uo0, ui0);
                    for _ in 0..60 {
                        let mid = P2::new(0.5 * (po.x + pi.x), 0.5 * (po.y + pi.y));
                        if tr.oracle.at_instant(t2, &at(mid)) > 0.0 {
                            po = mid;
                        } else {
                            pi = mid;
                        }
                    }
                    uo = po;
                }
                let x = c.surf().eval(uo.x, uo.y);
                if net.near(&x, 1e-4 * size) {
                    continue;
                }
                // The face covering it there: the one the oracle locates at an instant that
                // touches the point (the face itself, somewhere else on it, where it crosses
                // itself), else the nearest that works.
                let reach = 1e-3 * size;
                let mut best: Option<(f64, usize, Q)> = None;
                let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
                let consider = |j: usize, ug: P2, best: &mut Option<(f64, usize, Q)>| {
                    let g = &tr.cands[j];
                    let why: &str;
                    'c: {
                        let same = |p: P2, r: P2| {
                            (p.x - r.x).abs() < 1e-6 * c.uv_span.x
                                && (p.y - r.y).abs() < 1e-6 * c.uv_span.y
                        };
                        if j == i && same(uo, ug) {
                            why = "the same place";
                            break 'c;
                        }
                        let Some(q) = pair_correct(
                            c.surf(),
                            g.surf(),
                            [uo.x, uo.y, ug.x, ug.y],
                            1e-12 * size,
                        ) else {
                            why = "no crossing found";
                            break 'c;
                        };
                        if j == i && same(uv_of(&q, 0), uv_of(&q, 1)) {
                            why = "the same place after correction";
                            break 'c;
                        }
                        // (Not a face it only touches, folded back on it along an edge they
                        // share; a grazing crossing elsewhere is a crossing.)
                        let xq = tr.point(i, &q);
                        let sin = c.normal(uv_of(&q, 0)).cross(&g.normal(uv_of(&q, 1))).norm();
                        if sin < 1e-6 {
                            why = "tangent";
                            break 'c;
                        }
                        // (Not along an edge the two faces share: a step along the crossing
                        // still on both faces' edges.)
                        if i != j
                            && let Some((dq, _)) = pair_tangent(c.surf(), g.surf(), &q)
                            && let Some(q1) = pair_correct(
                                c.surf(),
                                g.surf(),
                                std::array::from_fn(|k| q[k] + dq[k] * 1e-5 * size),
                                1e-12 * size,
                            )
                        {
                            let x1 = tr.point(i, &q1);
                            if tr.on_edge(i, &x1, 1e-9 * size) && tr.on_edge(j, &x1, 1e-9 * size) {
                                why = "along an edge they share";
                                break 'c;
                            }
                        }
                        let d = (xq - x).norm();
                        if d >= reach {
                            why = "too far";
                            break 'c;
                        }
                        if !tr.inside(i, j, &q, 1e-9 * size) {
                            why = "outside a face";
                            break 'c;
                        }
                        if !tr.on_boundary(i, &q) {
                            why = "inside the solid";
                            break 'c;
                        }
                        if best.is_none_or(|(bd, ..)| d < bd) {
                            *best = Some((d, j, q));
                        }
                        return;
                    }
                    if debug {
                        eprintln!("  face {i} at {uo:?}: face {j} at {ug:?} doesn't cover: {why}");
                    }
                };
                if let Some(locate) = &tr.oracle.locate {
                    for (v, t) in tr.oracle.leasts_within(&x, 1e-5 * size) {
                        if v.abs() > 1e-5 * size {
                            continue;
                        }
                        for (j, ug) in locate(t, &x) {
                            consider(j, ug, &mut best);
                        }
                    }
                }
                if best.is_none() {
                    for (j, g) in tr.cands.iter().enumerate() {
                        if j == i || !g.bbox.grow(reach).contains(&x) {
                            continue;
                        }
                        let ug = g.project(&x);
                        if (g.surf().eval(ug.x, ug.y) - x).norm() > reach {
                            continue;
                        }
                        consider(j, ug, &mut best);
                    }
                }
                if let Some((_, j, _)) = best
                    && given_pairs.contains(&(i.min(j), i.max(j)))
                {
                    continue;
                }
                let Some((_, j, q)) = best else {
                    net.trouble.push(format!("face {i}: no face covers {x:?}"));
                    if debug {
                        let ls: Vec<String> = tr
                            .oracle
                            .leasts(&x)
                            .iter()
                            .map(|(v, t)| {
                                let loc = tr.oracle.locate.as_ref().map(|l| l(*t, &x));
                                format!("t {t:.6} f {v:.3e} located {loc:?}")
                            })
                            .collect();
                        eprintln!("  face {i}: nothing covers {x:?} at {uo:?}: {ls:?}");
                    }
                    continue;
                };
                // (On a curve already followed: within the sag of its chords.)
                if net.near(&tr.point(i, &q), 2e-4 * size) {
                    continue;
                }
                let start = tr.point(i, &q);
                if debug {
                    let nearest = net
                        .stretches
                        .iter()
                        .flat_map(|s| s.pts.iter().map(|p| (p.0 - start).norm()))
                        .fold(f64::INFINITY, f64::min);
                    eprintln!(
                        "  seed on face {i} with face {j} at {start:?} (nearest network point {nearest:.2e})"
                    );
                }
                let (fwd, ef) = tr.walk(i, j, q, 1.0, &start);
                let (back, eb) = if ef == End::Closed {
                    (Vec::new(), End::Closed)
                } else {
                    tr.walk(i, j, q, -1.0, &start)
                };
                // The way back reversed, then on.
                let mut segs: Vec<Seg> = back
                    .into_iter()
                    .rev()
                    .map(|mut s| {
                        s.pts.reverse();
                        s
                    })
                    .collect();
                let mut fwd = fwd.into_iter();
                if let Some(first) = fwd.next() {
                    match segs.last_mut() {
                        Some(l) => l.pts.extend(first.pts.into_iter().skip(1)),
                        None => segs.push(first),
                    }
                }
                segs.extend(fwd);
                let n0 = net.stretches.len();
                note(&mut net, eb, &segs);
                note(&mut net, ef, &segs);
                net.add(segs, None, vtol);
                let n1 = net.stretches.len();
                if n1 > n0 {
                    if eb == End::Inside {
                        end_inside(&mut net, &mut queue, n0, 0);
                    }
                    if ef == End::Inside {
                        end_inside(&mut net, &mut queue, n1 - 1, 1);
                    }
                }
                drain!();
            }
        }
    }
    // A curve ending on another (at a triple point on a given curve, say) splits it there,
    // so that the two meet at a vertex. (Curves given whole end where they meet.)
    let mut vi = if complete { net.verts.len() } else { 0 };
    while vi < net.verts.len() {
        let p = net.verts[vi];
        let mut split: Option<(usize, usize)> = None;
        'find: for (si, st) in net.stretches.iter().enumerate() {
            if st.v.contains(&vi) || st.pts.len() < 3 {
                continue;
            }
            let n = st.pts.len();
            if let Some(w) = (1..n - 1).find(|&w| (st.pts[w].0 - p).norm() <= vtol) {
                split = Some((si, w));
                break 'find;
            }
            for w in 0..st.pts.len() - 1 {
                let (a, b) = (st.pts[w].0, st.pts[w + 1].0);
                let d = b - a;
                let t = if d.norm_squared() > 0.0 {
                    ((p - a).dot(&d) / d.norm_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                if (0.02..=0.98).contains(&t) && (a + d * t - p).norm() <= vtol {
                    split = Some((si, w + 1));
                    // (The point on the curve nearest the vertex put in before w + 1.)
                    break 'find;
                }
            }
        }
        if let Some((si, w)) = split {
            let st = &mut net.stretches[si];
            let exact = (st.pts[w].0 - p).norm() <= vtol;
            let mut tail: Vec<(P3, P2, P2)> = if exact {
                st.pts.split_off(w)
            } else {
                // The vertex on both sides, its (u, v) on each face refined from the
                // neighbours' mean.
                let (q0, q1) = (st.pts[w - 1], st.pts[w]);
                let mid = (
                    p,
                    nearest_uv(
                        tr.cands[st.a].surf(),
                        P2::from(0.5 * (q0.1.coords + q1.1.coords)),
                        &p,
                    ),
                    nearest_uv(
                        tr.cands[st.b].surf(),
                        P2::from(0.5 * (q0.2.coords + q1.2.coords)),
                        &p,
                    ),
                );
                let mut tail = st.pts.split_off(w);
                tail.insert(0, mid);
                st.pts.push(mid);
                tail
            };
            if exact {
                let mut head_end = tail[0];
                head_end.0 = p;
                tail[0].0 = p;
                st.pts.push(head_end);
            }
            let v1 = st.v[1];
            st.v[1] = vi;
            let (a, b, given) = (st.a, st.b, st.given);
            net.stretches.push(Stretch {
                a,
                b,
                pts: tail,
                v: [vi, v1],
                given,
            });
        }
        vi += 1;
    }
    // A stretch traced twice (from seeds apart on it, each far from the other's curve where the
    // faces graze): the later copy dropped.
    let same_tol = 1e-5 * size;
    let mut drop = vec![false; net.stretches.len()];
    for i in (0..net.stretches.len()).filter(|_| !complete) {
        for j in 0..i {
            if drop[j] {
                continue;
            }
            let (p, q) = (&net.stretches[i], &net.stretches[j]);
            let pair = (p.a, p.b) == (q.a, q.b) || (p.a, p.b) == (q.b, q.a);
            if !pair {
                continue;
            }
            let (p0, p1) = (p.pts[0].0, p.pts[p.pts.len() - 1].0);
            let (q0, q1) = (q.pts[0].0, q.pts[q.pts.len() - 1].0);
            let ends = ((p0 - q0).norm() <= same_tol && (p1 - q1).norm() <= same_tol)
                || ((p0 - q1).norm() <= same_tol && (p1 - q0).norm() <= same_tol);
            if !ends {
                continue;
            }
            let on = |pts: &[(P3, P2, P2)], other: &Stretch| {
                pts.iter().step_by((pts.len() / 16).max(1)).all(|r| {
                    other.pts.windows(2).any(|w| {
                        let (a, b) = (w[0].0, w[1].0);
                        let d = b - a;
                        let t = if d.norm_squared() > 0.0 {
                            ((r.0 - a).dot(&d) / d.norm_squared()).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        (a + d * t - r.0).norm() <= same_tol
                    })
                })
            };
            if on(&p.pts, q) {
                drop[i] = true;
                break;
            }
        }
    }
    if drop.iter().any(|d| *d) {
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!(
                "  {} stretches traced twice dropped",
                drop.iter().filter(|d| **d).count()
            );
        }
        let mut k = 0;
        net.stretches.retain(|_| {
            k += 1;
            !drop[k - 1]
        });
    }
    // A curve's end a hair from a corner of one of its faces (where an event brings corners
    // together) put on the corner, so that every face meeting there ends it at one vertex.
    let snap = 1e-6 * size;
    for vi in (0..net.verts.len()).filter(|_| !complete) {
        let p = net.verts[vi];
        let mut best: Option<(f64, P3)> = None;
        for st in net.stretches.iter().filter(|st| st.v.contains(&vi)) {
            for f in [st.a, st.b] {
                for v in &tr.cands[f].body.verts {
                    let d = (v.p - p).norm();
                    if d <= snap && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, v.p));
                    }
                }
            }
        }
        let Some((d, c)) = best else { continue };
        if d == 0.0 {
            continue;
        }
        net.verts[vi] = c;
        for st in net.stretches.iter_mut() {
            for e in 0..2 {
                if st.v[e] != vi {
                    continue;
                }
                let at = if e == 0 { 0 } else { st.pts.len() - 1 };
                let (ua, ub) = (tr.cands[st.a].project(&c), tr.cands[st.b].project(&c));
                st.pts[at] = (c, ua, ub);
            }
        }
    }
    // (Points crowding together, where a walk slowed to a stop or a curve's end was moved onto
    // a corner, thinned: a curve's points a hair apart make slivers of its faces' meshes.)
    let gap = 1e-6 * size;
    for st in &mut net.stretches {
        let n = st.pts.len();
        let last = st.pts[n - 1];
        let mut kept = vec![st.pts[0]];
        for p in &st.pts[1..n - 1] {
            if (p.0 - st.pts[0].0).norm() > gap && (p.0 - last.0).norm() > gap {
                kept.push(*p);
            }
        }
        kept.push(last);
        st.pts = kept;
    }
    // Each stretch's points made dense enough that the curves fitted through them (and their
    // p-curves) stay on both faces: a point added between two where the fit strays.
    let ftol = 1e-9 * size;
    for st in &mut net.stretches {
        if st.given {
            continue;
        }
        for _ in 0..12 {
            let curves = stretch_curves(st);
            let params = stretch_params(st);
            let (sa, sb) = (tr.cands[st.a].surf(), tr.cands[st.b].surf());
            let mut add: Vec<(usize, (P3, P2, P2))> = Vec::new();
            for i in 0..st.pts.len() - 1 {
                let (p, r) = (&st.pts[i], &st.pts[i + 1]);
                // (Not between points close already: near where the faces touch, the curve
                // is ill-defined to within its fit, and points there only crowd.)
                if (r.0 - p.0).norm() <= 10.0 * gap {
                    continue;
                }
                let q: Q = [
                    0.5 * (p.1.x + r.1.x),
                    0.5 * (p.1.y + r.1.y),
                    0.5 * (p.2.x + r.2.x),
                    0.5 * (p.2.y + r.2.y),
                ];
                let Some(qm) = pair_correct(sa, sb, q, 1e-13 * size) else {
                    continue;
                };
                let xm = sa.eval(qm[0], qm[1]);
                let tm = 0.5 * (params[i] + params[i + 1]);
                let off = (curves.c.point(tm) - xm).norm();
                let w0 = curves.pc[0].point(tm);
                let w1 = curves.pc[1].point(tm);
                let off0 = (sa.eval(w0.x, w0.y) - xm).norm();
                let off1 = (sb.eval(w1.x, w1.y) - xm).norm();
                if off.max(off0).max(off1) > ftol {
                    add.push((i + 1, (xm, uv_of(&qm, 0), uv_of(&qm, 1))));
                }
            }
            if add.is_empty() {
                break;
            }
            for (k, (i, p)) in add.into_iter().enumerate() {
                st.pts.insert(i + k, p);
            }
        }
    }
    // (Where the faces graze, a point added slides along the curve onto its neighbour: points
    // within a millionth of the size of the one before thinned, the ends kept.)
    let least = 1e-6 * size;
    for st in &mut net.stretches {
        let n = st.pts.len();
        let len: f64 = st.pts.windows(2).map(|w| (w[1].0 - w[0].0).norm()).sum();
        if n <= 2 || len < 100.0 * least {
            continue;
        }
        let last = st.pts[n - 1];
        let mut kept = vec![st.pts[0]];
        for p in &st.pts[1..n - 1] {
            if (p.0 - kept[kept.len() - 1].0).norm() >= least && (p.0 - last.0).norm() >= least {
                kept.push(*p);
            }
        }
        kept.push(last);
        st.pts = kept;
    }
    net
}

/// Edge `e` of `b` split at `t` by vertex `w`: the coedges using it use both pieces, in order.
fn split_edge(b: &mut Body, e: usize, t: f64, w: usize) {
    use super::topo::Coedge;
    let old = b.edges[e].clone();
    b.edges[e].t = [old.t[0], t];
    b.edges[e].v = [old.v[0], w];
    let mut second = old.clone();
    second.t = [t, old.t[1]];
    second.v = [w, old.v[1]];
    b.edges.push(second);
    let e2 = b.edges.len() - 1;
    for f in &mut b.faces {
        for l in &mut f.loops {
            let mut nl = Vec::with_capacity(l.len() + 1);
            for c in l.drain(..) {
                if c.edge == e {
                    let other = Coedge {
                        edge: e2,
                        fwd: c.fwd,
                        pc: c.pc.clone(),
                    };
                    if c.fwd {
                        nl.push(c);
                        nl.push(other);
                    } else {
                        nl.push(other);
                        nl.push(c);
                    }
                } else {
                    nl.push(c);
                }
            }
            *l = nl;
        }
    }
}

/// Coedge `c` (on surface `surf`, along edge `from`) moved onto edge `to`, which runs along the
/// same points: its p-curve put in `to`'s parameter (exactly where both are lines, else
/// projected again), its direction turned where `to` runs the other way. `near`: how far the
/// surface may be off `to` (a fitted surface's fit).
fn moved_coedge(
    c: &super::topo::Coedge,
    from: &super::topo::Edge,
    to: &super::topo::Edge,
    surf: &Surface,
    near: f64,
) -> super::topo::Coedge {
    use super::topo::Coedge;
    let reversed = from.v[0] != to.v[0] || (from.v[0] == from.v[1]);
    let fwd = if reversed { !c.fwd } else { c.fwd };
    if !reversed && from.t == to.t && {
        let m = 0.5 * (to.t[0] + to.t[1]);
        (from.curve.point(m) - to.curve.point(m)).norm()
            <= 1e-9 * (1.0 + to.curve.point(m).coords.norm())
    } {
        return Coedge {
            edge: c.edge,
            fwd,
            pc: c.pc.clone(),
        };
    }
    if let (Curve::Line { o: fo, d: fd }, Curve::Line { o: to_o, d: td }, Curve::Line { o, d }) =
        (&from.curve, &to.curve, &c.pc)
    {
        // s (from's parameter) = α + β t (to's).
        let dd = fd.norm_squared();
        let alpha = (to_o - fo).dot(fd) / dd;
        let beta = td.dot(fd) / dd;
        return Coedge {
            edge: c.edge,
            fwd,
            pc: Curve::Line {
                o: o + d * alpha,
                d: d * beta,
            },
        };
    }
    // The face's own p-curve of the old edge, read at the old edge's parameter of each of the
    // new edge's points (the two lie on each other): exact where a projection onto the surface
    // is not (a fitted surface's edge, a surface nearly folding).
    {
        let at = |t: f64| -> P3 {
            let s = from.curve.closest(&to.curve.point(t), from.t[0], from.t[1]);
            c.pc.point(s)
        };
        let scale = 1.0 + to.curve.point(to.t[0]).coords.norm();
        let mut n = 16;
        loop {
            let ts: Vec<f64> = (0..=n)
                .map(|i| to.t[0] + (to.t[1] - to.t[0]) * i as f64 / n as f64)
                .collect();
            let pts: Vec<P3> = ts.iter().map(|&t| at(t)).collect();
            let pc = NurbsCurve::interpolate(&pts, &ts, 3.min(n), None, None);
            let worst = ts
                .windows(2)
                .map(|w| {
                    let t = 0.5 * (w[0] + w[1]);
                    let uv = pc.point(t);
                    (surf.eval(uv.x, uv.y) - to.curve.point(t)).norm()
                })
                .fold(0.0, f64::max);
            if worst <= (1e-9 * scale).max(near) {
                return Coedge {
                    edge: c.edge,
                    fwd,
                    pc: Curve::Nurbs(std::sync::Arc::new(pc)),
                };
            }
            if n >= 512 {
                break;
            }
            n *= 2;
        }
    }
    // (Near the old p-curve: in the same turn of a closed surface's parameters.)
    let near = {
        let m = 0.5 * (from.t[0] + from.t[1]);
        let w = c.pc.point(m);
        Some(P2::new(w.x, w.y))
    };
    Coedge {
        edge: c.edge,
        fwd,
        pc: super::pcurve::pcurve(surf, &to.curve, to.t[0], to.t[1], near),
    }
}

/// Whether faces on a surface may be made one where they lie side by side on it (pieces a
/// surface was cut into only to be trimmed).
pub(crate) type Whole<'a> = dyn Fn(&Surface) -> bool + 'a;

/// The kept faces (each a body of one face, facing out) joined into a body: vertices within
/// `tol` one, an edge with another's vertex lying on it (within `near`) split there, and the
/// two edges between the same vertices that lie on each other (within `near`) one, each face
/// keeping its own p-curve.
fn assemble(
    faces: &[Body],
    near: f64,
    tol: f64,
    whole: Option<(&Whole, f64)>,
) -> Result<(Body, Option<f64>, bool)> {
    // (An edge lies on another, or a vertex on an edge, within the faces' own looseness.)
    let near = near.max(tol);
    let clock = web_time::Instant::now();
    let lap = |what: &str| {
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!(
                "  joining: {what} by {:.0} ms",
                clock.elapsed().as_secs_f64() * 1e3
            );
        }
    };
    let mut b = Body::default();
    for f in faces {
        b.append(f);
    }
    b.shells.clear();
    b.solids.clear();
    // Vertices.
    let nv = b.verts.len();
    let mut vmap: Vec<usize> = (0..nv).collect();
    let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    let key = |p: &P3| [p.x, p.y, p.z].map(|c| (c / (4.0 * tol)).floor() as i64);
    for i in 0..nv {
        let p = b.verts[i].p;
        let k = key(&p);
        let mut found = None;
        'cells: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz])
                        && let Some(&j) = list.iter().find(|&&j| (b.verts[j].p - p).norm() <= tol)
                    {
                        found = Some(j);
                        break 'cells;
                    }
                }
            }
        }
        match found {
            Some(j) => vmap[i] = j,
            None => grid.entry(k).or_default().push(i),
        }
    }
    for e in &mut b.edges {
        e.v = [vmap[e.v[0]], vmap[e.v[1]]];
    }
    // T-junctions.
    loop {
        let ef = b.edge_faces();
        let free: Vec<usize> = (0..b.edges.len())
            .filter(|&e| ef[e].len() == 1 && !b.edges[e].degenerate)
            .collect();
        let mut ends: Vec<usize> = free.iter().flat_map(|&e| b.edges[e].v).collect();
        ends.sort_unstable();
        ends.dedup();
        let mut hits: Vec<(usize, f64, usize)> = Vec::new();
        for &e in &free {
            let ed = &b.edges[e];
            let bb = super::boolean::curve_bbox(&ed.curve, ed.t[0], ed.t[1]).grow(near);
            let mut best: Option<(f64, f64, usize)> = None;
            for &w in &ends {
                if ed.v.contains(&w) || !bb.contains(&b.verts[w].p) {
                    continue;
                }
                let p = b.verts[w].p;
                let t = ed.curve.closest(&p, ed.t[0], ed.t[1]);
                let q = ed.curve.point(t);
                let d = (q - p).norm();
                if d <= near
                    && (q - ed.point(ed.t[0])).norm() > near
                    && (q - ed.point(ed.t[1])).norm() > near
                    && best.is_none_or(|(bd, ..)| d < bd)
                {
                    best = Some((d, t, w));
                }
            }
            if let Some((_, t, w)) = best {
                hits.push((e, t, w));
            }
        }
        if hits.is_empty() {
            break;
        }
        for (e, t, w) in hits {
            split_edge(&mut b, e, t, w);
        }
    }
    lap("vertices and T-junctions");
    // Edges.
    let ef = b.edge_faces();
    let mut by_ends: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (e, ed) in b.edges.iter().enumerate() {
        if ed.degenerate || ef[e].len() != 1 {
            continue;
        }
        by_ends
            .entry((ed.v[0].min(ed.v[1]), ed.v[0].max(ed.v[1])))
            .or_default()
            .push(e);
    }
    let mut onto: HashMap<usize, usize> = HashMap::new();
    let mut keys: Vec<(usize, usize)> = by_ends.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        let list = &by_ends[&k];
        let mut taken = vec![false; list.len()];
        for x in 0..list.len() {
            if taken[x] {
                continue;
            }
            let ex = &b.edges[list[x]];
            for y in x + 1..list.len() {
                if taken[y] {
                    continue;
                }
                let ey = &b.edges[list[y]];
                let close = [0.25, 0.5, 0.75].iter().all(|f| {
                    let m = ey.curve.point(ey.t[0] + (ey.t[1] - ey.t[0]) * f);
                    let t = ex.curve.closest(&m, ex.t[0], ex.t[1]);
                    (ex.curve.point(t) - m).norm() <= near
                });
                if close {
                    taken[x] = true;
                    taken[y] = true;
                    onto.insert(list[y], list[x]);
                    break;
                }
            }
        }
    }
    for fi in 0..b.faces.len() {
        let surf = b.faces[fi].surf.clone();
        for li in 0..b.faces[fi].loops.len() {
            for ci in 0..b.faces[fi].loops[li].len() {
                let c = b.faces[fi].loops[li][ci].clone();
                if let Some(&x) = onto.get(&c.edge) {
                    let mut nc = moved_coedge(&c, &b.edges[c.edge], &b.edges[x], &surf, near);
                    nc.edge = x;
                    b.faces[fi].loops[li][ci] = nc;
                }
            }
        }
    }
    super::unify::compact(&mut b);
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        // (The edges left with one face: where the faces don't close.)
        let ef = b.edge_faces();
        for (e, ed) in b.edges.iter().enumerate() {
            if ed.degenerate || ef[e].len() == 2 {
                continue;
            }
            let (p, q) = (ed.point(ed.t[0]), ed.point(ed.t[1]));
            let places: Vec<String> = b
                .faces
                .iter()
                .enumerate()
                .flat_map(|(fi, f)| {
                    f.loops
                        .iter()
                        .flatten()
                        .filter(|c| c.edge == e)
                        .map(move |c| {
                            let (a, z) = (c.pc.point(ed.t[0]), c.pc.point(ed.t[1]));
                            format!(
                                "face {fi} ({:.5}, {:.5}) -> ({:.5}, {:.5})",
                                a.x, a.y, z.x, z.y
                            )
                        })
                })
                .collect();
            eprintln!(
                "  edge {e} has {} faces: {:.3e} long, [{:.6}, {:.6}, {:.6}] -> [{:.6}, {:.6}, {:.6}]; {places:?}",
                ef[e].len(),
                (q - p).norm(),
                p.x,
                p.y,
                p.z,
                q.x,
                q.y,
                q.z
            );
        }
    }
    lap("edges");
    // Faces cut apart only to be trimmed made whole again, if the body is as sound for it
    // (before its shells are made out, which is the dear part, and needn't be done twice).
    if let Some((whole, searched)) = whole {
        let mut merged = b.clone();
        let from: Vec<Vec<(usize, usize)>> =
            (0..merged.faces.len()).map(|i| vec![(0, i)]).collect();
        let can: Vec<bool> = merged.faces.iter().map(|f| whole(&f.surf)).collect();
        let from = super::unify::unify_faces(&mut merged, from, &|f, g| can[f] && can[g]);
        lap("pieces made whole");
        // (What each face adds to the volume, integrated once: a face left as it was adds
        // what it did.)
        let shift = b.bbox().center().coords;
        let terms = super::mass::faces_volume_terms(&b, &shift);
        let before: f64 = terms.iter().sum();
        lap("volume before");
        let after: Vec<f64> = merged
            .faces
            .iter()
            .zip(&from)
            .map(|(f, origin)| match origin.as_slice() {
                [(_, i)] => terms[*i],
                _ => super::mass::face_integrals_of::<false>(&merged, f, &shift)[4],
            })
            .collect();
        let volume: f64 = after.iter().sum();
        let sound = merged.faces.len() < b.faces.len()
            && (volume - before).abs() <= 1e-7 * before.abs()
            && {
                lap("volume after");
                super::boolean::assemble_shells_with(&mut merged, Some(&after)).is_ok()
            }
            && {
                lap("shells");
                // (Its edges' tolerances fitted as the caller will fit them.)
                merged.fit_tolerances_searched(searched);
                lap("tolerances");
                merged.check().is_empty()
            };
        lap("checked");
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!(
                "moving sweep: {} faces, {} with the pieces of one surface made whole ({})",
                b.faces.len(),
                merged.faces.len(),
                if sound { "kept" } else { "left" }
            );
        }
        if sound {
            // (Checked, with its tolerances fitted as the caller fits them: not again.)
            return Ok((merged, Some(volume), true));
        }
        super::boolean::assemble_shells_with(&mut b, Some(&terms))?;
        return Ok((b, Some(before), false));
    }
    super::boolean::assemble_shells(&mut b)?;
    Ok((b, None, false))
}

/// A stretch's curves: in space, and on each of its two faces (z = 0), over [0, its length].
struct StretchCurves {
    c: Curve,
    pc: [Curve; 2],
}

fn stretch_curves(s: &Stretch) -> StretchCurves {
    let params = stretch_params(s);
    let pts: Vec<P3> = s.pts.iter().map(|p| p.0).collect();
    let fit = |pts: &[P3]| {
        let deg = if pts.len() >= 4 { 3 } else { 1 };
        Curve::Nurbs(std::sync::Arc::new(NurbsCurve::interpolate(
            pts, &params, deg, None, None,
        )))
    };
    let ua: Vec<P3> = s.pts.iter().map(|p| P3::new(p.1.x, p.1.y, 0.0)).collect();
    let ub: Vec<P3> = s.pts.iter().map(|p| P3::new(p.2.x, p.2.y, 0.0)).collect();
    StretchCurves {
        c: fit(&pts),
        pc: [fit(&ua), fit(&ub)],
    }
}

/// A curve along which two candidate faces `a` and `b` meet on the swept solid's boundary,
/// known by construction (traced in a face's own parameters where the two graze and the
/// tracer would lose it): its points with their (u, v) on each; closed when the last repeats
/// the first.
pub(crate) struct Given {
    pub a: usize,
    pub b: usize,
    pub pts: Vec<(P3, P2, P2)>,
}

/// A cut through a face: the curve along it in space, the points it was traced through with
/// their (u, v) on the face, and the ids (not the face's vertices') of its ends.
pub(crate) struct FaceCut {
    pub c: Curve,
    pub pts: Vec<(P3, P2)>,
    pub v: [usize; 2],
    /// The curve's parameter at each point, where it is not the chord lengths summed.
    pub params: Option<Vec<f64>>,
}

/// The chord lengths of a polyline, summed from nothing.
fn chord_params<T>(pts: &[T], at: impl Fn(&T) -> P3) -> Vec<f64> {
    let mut params = vec![0.0];
    for w in pts.windows(2) {
        params.push(params.last().unwrap() + (at(&w[1]) - at(&w[0])).norm());
    }
    params
}

/// The parameter of each point of a stretch's curves: the chord lengths summed, and for a
/// face's crossing of itself its run on the face at each of its two places as well (each in
/// the measure of its length in space). Such a curve may end at the tip of a fold, where its
/// two places come together: there it stands still in space while its places still move, and
/// curves through their points over chord length alone swing wide of them.
fn stretch_params(s: &Stretch) -> Vec<f64> {
    let chords = chord_params(&s.pts, |p| p.0);
    if s.a != s.b {
        return chords;
    }
    let run = |k: usize| -> Vec<f64> {
        chord_params(&s.pts, |p| {
            let w = if k == 0 { p.1 } else { p.2 };
            P3::new(w.x, w.y, 0.0)
        })
    };
    let (r0, r1) = (run(0), run(1));
    let len = *chords.last().unwrap();
    let part = |r: &[f64], i: usize| {
        let whole = *r.last().unwrap();
        if whole > 0.0 { len * r[i] / whole } else { 0.0 }
    };
    (0..chords.len())
        .map(|i| chords[i] + part(&r0, i) + part(&r1, i))
        .collect()
}

/// Candidate face `fi` split along the network's stretches on it: its pieces, each a body of
/// one face.
fn split_by(
    c: &Cand,
    fi: usize,
    net: &Network,
    curves: &[StretchCurves],
    size: f64,
    on: f64,
    exact: Option<f64>,
) -> std::result::Result<Vec<Body>, String> {
    let nb = c.body.verts.len();
    // Vertex ids: the body's, then the network's (offset by nb), then further ids for the
    // other places a network vertex has on the face (where the face crosses itself, a point of
    // it is two places in (u, v)).
    let mut next_id = nb + net.verts.len();
    // The places network vertices have on the face: (network vertex, (u, v), id).
    let mut places: Vec<(usize, P2, usize)> = Vec::new();
    let mut pos: HashMap<usize, P3> = HashMap::new();
    let mut cuts: Vec<FaceCut> = Vec::new();
    for (si, st) in net.stretches.iter().enumerate() {
        for k in 0..2 {
            if [st.a, st.b][k] != fi {
                continue;
            }
            let mut v = [0; 2];
            for (e, &gv) in st.v.iter().enumerate() {
                let pt = if e == 0 {
                    st.pts[0]
                } else {
                    st.pts[st.pts.len() - 1]
                };
                let uv = if k == 0 { pt.1 } else { pt.2 };
                let id = match places.iter().find(|(w, wuv, _)| {
                    *w == gv
                        && match exact {
                            Some(part) => {
                                let (lo, hi) = grid_box(c);
                                (*wuv - uv).norm() <= part * (hi - lo).norm()
                            }
                            None => metric(c, *wuv, uv) <= 1e-5 * size,
                        }
                }) {
                    Some(&(_, _, id)) => id,
                    None => {
                        let id = if places.iter().any(|(w, ..)| *w == gv) {
                            next_id += 1;
                            next_id - 1
                        } else {
                            nb + gv
                        };
                        places.push((gv, uv, id));
                        pos.insert(id, net.verts[gv]);
                        id
                    }
                };
                v[e] = id;
            }
            cuts.push(FaceCut {
                c: curves[si].c.clone(),
                pts: st
                    .pts
                    .iter()
                    .map(|p| (p.0, if k == 0 { p.1 } else { p.2 }))
                    .collect(),
                v,
                params: Some(stretch_params(st)),
            });
        }
    }
    cut_face(&c.body, fi, cuts, &pos, size, on, exact)
}

/// Face `fi` (the one face of `b`) split along `cuts` (whose end ids, at least the body's vertex
/// count, are placed by `pos`): its pieces, each a body of one face. A cut's end on the face's
/// boundary (in space and in (u, v) both: where the face crosses itself, a point of its edge is
/// also inside it) is the boundary's vertex there, or splits its edge; `on`: how near the
/// boundary in space is on it. With `exact`, the cuts' ends are placed on the face exactly, and
/// one is on the boundary where its (u, v) is, within that fraction of the face's extent (where
/// the face folds, points a hair apart in space are far apart on it); any of the ends at a
/// vertex being there puts the vertex there.
pub(crate) fn cut_face(
    b: &Body,
    fi: usize,
    cuts: Vec<FaceCut>,
    pos: &HashMap<usize, P3>,
    size: f64,
    on: f64,
    exact: Option<f64>,
) -> std::result::Result<Vec<Body>, String> {
    use super::split::Arc;
    use super::topo::{Coedge, Face};
    let f = &b.faces[0];
    if f.loops.len() != 1 {
        return Err(format!("face {fi} has {} loops", f.loops.len()));
    }
    let nb = b.verts.len();
    let mut pos = pos.clone();
    for (i, v) in b.verts.iter().enumerate() {
        pos.insert(i, v.p);
    }
    // A closed cut (a curve round a pocket, back where it started) in two halves, each from
    // the start to a vertex half way round: the splitter takes arcs between two vertices.
    let mut cuts = cuts;
    let mut next_id = pos.keys().copied().max().map_or(nb, |m| m + 1).max(nb);
    for ci in 0..cuts.len() {
        if cuts[ci].v[0] != cuts[ci].v[1] || cuts[ci].pts.len() < 4 {
            continue;
        }
        let mid = cuts[ci].pts.len() / 2;
        let second: Vec<(P3, P2)> = cuts[ci].pts[mid..].to_vec();
        cuts[ci].pts.truncate(mid + 1);
        // (Each half over the whole's parameters, the second's from nothing.)
        let halves = cuts[ci].params.take().map(|p| {
            let at = p[mid];
            (
                p[..=mid].to_vec(),
                p[mid..].iter().map(|t| t - at).collect::<Vec<f64>>(),
            )
        });
        let v = next_id;
        next_id += 1;
        pos.insert(v, second[0].0);
        let end = cuts[ci].v[1];
        cuts[ci].v[1] = v;
        // (Each half's curve through its own points, over their chord lengths from zero, as
        // the arcs below take it.)
        let fit = |pts: &[(P3, P2)], params: Option<&Vec<f64>>| -> Curve {
            let params = params
                .cloned()
                .unwrap_or_else(|| chord_params(pts, |p| p.0));
            let p3: Vec<P3> = pts.iter().map(|p| p.0).collect();
            let deg = if p3.len() >= 4 { 3 } else { 1 };
            Curve::Nurbs(std::sync::Arc::new(NurbsCurve::interpolate(
                &p3, &params, deg, None, None,
            )))
        };
        let (first_params, second_params) = match halves {
            Some((a, b)) => (Some(a), Some(b)),
            None => (None, None),
        };
        cuts[ci].c = fit(&cuts[ci].pts, first_params.as_ref());
        cuts[ci].params = first_params;
        let c = fit(&second, second_params.as_ref());
        cuts.push(FaceCut {
            c,
            pts: second,
            v: [v, end],
            params: second_params,
        });
    }
    let cuts = cuts;
    let metric = |uv: P2, wv: P2| -> f64 {
        let (_, su, sv) = f.surf.d1(uv.x, uv.y);
        (su * (wv.x - uv.x) + sv * (wv.y - uv.y)).norm()
    };
    // The cuts' ends: (id, point, (u, v)).
    let mut ends: Vec<(usize, P3, P2)> = Vec::new();
    for cut in &cuts {
        ends.push((cut.v[0], cut.pts[0].0, cut.pts[0].1));
        let last = cut.pts[cut.pts.len() - 1];
        ends.push((cut.v[1], last.0, last.1));
    }
    ends.sort_by_key(|e| e.0);
    if exact.is_none() {
        ends.dedup_by_key(|e| e.0);
    }
    let near_uv = 1e-5 * size;
    let uv_of_vertex = |v: usize| -> Option<P2> {
        f.loops[0].iter().find_map(|co| {
            let e = &b.edges[co.edge];
            let (t0, t1) = super::topo::co_range(e, co.fwd);
            let (s0, s1) = if co.fwd {
                (e.v[0], e.v[1])
            } else {
                (e.v[1], e.v[0])
            };
            let t = if s0 == v {
                t0
            } else if s1 == v {
                t1
            } else {
                return None;
            };
            let w = co.pc.point(t);
            Some(P2::new(w.x, w.y))
        })
    };
    // (How near in (u, v) is on the boundary: by the surface's stretch there, or, the ends
    // being exact, by the face's extent.)
    let extent = {
        let (mut lo, mut hi) = (
            P2::new(f64::INFINITY, f64::INFINITY),
            P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for co in &f.loops[0] {
            let e = &b.edges[co.edge];
            for t in [e.t[0], e.t[1]] {
                let w = co.pc.point(t);
                lo = P2::new(lo.x.min(w.x), lo.y.min(w.y));
                hi = P2::new(hi.x.max(w.x), hi.y.max(w.y));
            }
        }
        (hi - lo).norm()
    };
    let there = |uv: P2, w: P2| -> bool {
        match exact {
            Some(part) => (uv - w).norm() <= part * extent,
            None => metric(uv, w) <= near_uv,
        }
    };
    let mut alias: HashMap<usize, usize> = HashMap::new();
    let mut splits: HashMap<usize, Vec<(f64, usize)>> = HashMap::new();
    for &(v, p, uv) in &ends {
        if alias.contains_key(&v) || splits.values().any(|l| l.iter().any(|(_, w)| *w == v)) {
            continue;
        }
        if let Some(bv) = (0..nb).find(|&i| {
            (b.verts[i].p - p).norm() <= on && uv_of_vertex(i).is_some_and(|w| there(uv, w))
        }) {
            alias.insert(v, bv);
            continue;
        }
        let mut best: Option<(f64, usize, f64)> = None;
        for co in &f.loops[0] {
            let e = &b.edges[co.edge];
            if e.degenerate {
                continue;
            }
            // (The ends exact: the place on the edge nearest in the face's own (u, v).)
            let t = if exact.is_some() {
                co.pc.closest(&P3::new(uv.x, uv.y, 0.0), e.t[0], e.t[1])
            } else {
                e.curve.closest(&p, e.t[0], e.t[1])
            };
            let d = (e.curve.point(t) - p).norm();
            let w = co.pc.point(t);
            if d <= on && there(uv, P2::new(w.x, w.y)) && best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, co.edge, t));
            }
        }
        if let Some((_, e, t)) = best {
            splits.entry(e).or_default().push((t, v));
        } else if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            // (A cut's end inside the face: fine where another cut ends there too.)
            let others = ends
                .iter()
                .filter(|(w, q, _)| *w != v && (q - p).norm() <= on)
                .count();
            let n_at = cuts.iter().filter(|c| c.v.contains(&v)).count();
            if others == 0 && n_at < 2 {
                let nearest = f.loops[0]
                    .iter()
                    .filter(|co| !b.edges[co.edge].degenerate)
                    .map(|co| {
                        let e = &b.edges[co.edge];
                        let t = e.curve.closest(&p, e.t[0], e.t[1]);
                        let w = co.pc.point(t);
                        (
                            (e.curve.point(t) - p).norm(),
                            metric(uv, P2::new(w.x, w.y)),
                            (uv - P2::new(w.x, w.y)).norm() / extent,
                        )
                    })
                    .min_by(|x, y| x.0.total_cmp(&y.0));
                eprintln!(
                    "    face {fi}: a cut ends inside it at {uv:?} (nearest edge {nearest:?} in space, in (u, v) by the surface's stretch, and by the face's extent)"
                );
            }
        }
    }
    let uv_on_edge = |v: usize| -> Option<P2> {
        splits.iter().find_map(|(e, list)| {
            let &(t, _) = list.iter().find(|(_, w)| *w == v)?;
            let co = f.loops[0].iter().find(|co| co.edge == *e)?;
            let w = co.pc.point(t);
            Some(P2::new(w.x, w.y))
        })
    };
    // The cut arcs, each p-curve ending where the boundary has its ends (on a thin face a hair
    // in space is far in (u, v)): at the vertex it is, or the point of the edge it splits.
    let mut arcs: Vec<Arc> = Vec::new();
    let mut curve_of: HashMap<usize, usize> = HashMap::new();
    for (ci, cut) in cuts.iter().enumerate() {
        let id = 1_000_000 + ci;
        curve_of.insert(id, ci);
        let v = cut.v.map(|x| alias.get(&x).copied().unwrap_or(x));
        let params = cut
            .params
            .clone()
            .unwrap_or_else(|| chord_params(&cut.pts, |p| p.0));
        let len = *params.last().unwrap();
        let mut uvs: Vec<P2> = cut.pts.iter().map(|p| p.1).collect();
        let n = uvs.len();
        for (end, at) in [(0usize, 0usize), (1, n - 1)] {
            let target = if v[end] < nb {
                uv_of_vertex(v[end])
            } else {
                uv_on_edge(v[end])
            };
            if let Some(w) = target {
                uvs[at] = w;
            }
        }
        let pts: Vec<P3> = uvs.iter().map(|w| P3::new(w.x, w.y, 0.0)).collect();
        let deg = if pts.len() >= 4 { 3 } else { 1 };
        let pc = Curve::Nurbs(std::sync::Arc::new(NurbsCurve::interpolate(
            &pts, &params, deg, None, None,
        )));
        arcs.push(Arc {
            edge: id,
            fwd: true,
            pc,
            t: [0.0, len],
            v,
            cut: true,
            deg: false,
        });
    }
    let mut boundary: Vec<Arc> = Vec::new();
    for co in &f.loops[0] {
        let e = &b.edges[co.edge];
        let mut cutsat: Vec<(f64, usize)> = splits.get(&co.edge).cloned().unwrap_or_default();
        cutsat.sort_by(|x, y| x.0.total_cmp(&y.0));
        // Pieces along the edge's curve, from v[0].
        let mut bounds = vec![(e.t[0], e.v[0])];
        bounds.extend(cutsat);
        bounds.push((e.t[1], e.v[1]));
        let mut pieces: Vec<Arc> = bounds
            .windows(2)
            .map(|w| Arc {
                edge: co.edge * 1000,
                fwd: co.fwd,
                pc: co.pc.clone(),
                t: [w[0].0, w[1].0],
                v: if co.fwd {
                    [w[0].1, w[1].1]
                } else {
                    [w[1].1, w[0].1]
                },
                cut: false,
                deg: e.degenerate,
            })
            .collect();
        for (k, a) in pieces.iter_mut().enumerate() {
            a.edge = co.edge * 1000 + k;
        }
        if !co.fwd {
            pieces.reverse();
        }
        boundary.extend(pieces);
    }
    let r = super::split::split_face(&f.surf, !f.sense, std::slice::from_ref(&boundary), &arcs);
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() && !arcs.is_empty() && r.faces.len() <= 1 {
        let desc = |a: &Arc| {
            let (p, q) = (a.uv_at(0.0), a.uv_at(1.0));
            format!(
                "edge {} v {:?} ({:.5}, {:.5})->({:.5}, {:.5}) t {:?}",
                a.edge, a.v, p.x, p.y, q.x, q.y, a.t
            )
        };
        eprintln!(
            "    face {fi} not split ({} faces of {:?} loops): boundary {:?}; cuts {:?}; aliases {alias:?}; splits {splits:?}",
            r.faces.len(),
            r.faces
                .iter()
                .map(|l| l.iter().map(|a| a.len()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            boundary.iter().map(desc).collect::<Vec<_>>(),
            arcs.iter().map(desc).collect::<Vec<_>>()
        );
    }

    let mut out = Vec::new();
    for loops in r.faces {
        let mut body = Body::default();
        let mut vmap: HashMap<usize, usize> = HashMap::new();
        let mut vert = |body: &mut Body, id: usize| -> usize {
            *vmap.entry(id).or_insert_with(|| body.add_vertex(pos[&id]))
        };
        let mut new_loops = Vec::new();
        for l in &loops {
            let mut coedges = Vec::new();
            for a in l {
                // The arc's ends along its edge's direction.
                let (v0, v1) = if a.fwd {
                    (a.v[0], a.v[1])
                } else {
                    (a.v[1], a.v[0])
                };
                let (v0, v1) = (vert(&mut body, v0), vert(&mut body, v1));
                let e = if let Some(&ci) = curve_of.get(&a.edge) {
                    body.add_edge(cuts[ci].c.clone(), a.t[0], a.t[1], v0, v1)
                } else {
                    let src = &b.edges[a.edge / 1000];
                    if src.degenerate {
                        body.add_degenerate(body.verts[v0].p, a.t[0], a.t[1], v0)
                    } else {
                        body.add_edge(src.curve.clone(), a.t[0], a.t[1], v0, v1)
                    }
                };
                coedges.push(Coedge {
                    edge: e,
                    fwd: a.fwd,
                    pc: a.pc.clone(),
                });
            }
            new_loops.push(coedges);
        }
        body.faces.push(Face {
            surf: f.surf.clone(),
            sense: f.sense,
            loops: new_loops,
        });
        body.shells.push(super::topo::Shell {
            faces: vec![0],
            closed: false,
        });
        out.push(body);
    }
    Ok(out)
}

/// A candidate face's (u, v) box.
fn grid_box(c: &Cand) -> (P2, P2) {
    let (mut lo, mut hi) = (
        P2::new(f64::INFINITY, f64::INFINITY),
        P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for q in c.polys.iter().flatten() {
        lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    (lo, hi)
}

/// An m × m grid over a candidate face's (u, v) box: each point on the face, or None.
fn face_grid(c: &Cand, m: usize) -> Vec<Option<P2>> {
    let (mut lo, mut hi) = (
        P2::new(f64::INFINITY, f64::INFINITY),
        P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for q in c.polys.iter().flatten() {
        lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    (0..m * m)
        .map(|k| {
            let (i, j) = (k / m, k % m);
            let uv = P2::new(
                lo.x + (hi.x - lo.x) * (i as f64 + 0.5) / m as f64,
                lo.y + (hi.y - lo.y) * (j as f64 + 0.5) / m as f64,
            );
            (super::classify::in_face(&c.polys, c.surf(), uv, 0.0)
                == super::classify::InFace::Inside)
                .then_some(uv)
        })
        .collect()
}

/// Points on face `fi` of `b` with its outward normal there: an m × m grid over its (u, v) box,
/// those inside it (or a few interior points where none are).
fn face_samples(b: &Body, fi: usize, m: usize) -> Vec<(P3, V3)> {
    let f = &b.faces[fi];
    let sign = if f.sense { 1.0 } else { -1.0 };
    face_sample_uvs(b, fi, m)
        .into_iter()
        .map(|uv| (f.surf.eval(uv.x, uv.y), f.surf.normal(uv.x, uv.y) * sign))
        .collect()
}

/// [`face_samples`]' places in the face's (u, v).
fn face_sample_uvs(b: &Body, fi: usize, m: usize) -> Vec<P2> {
    let f = &b.faces[fi];
    let polys = super::classify::face_polygons(b, f);
    let (mut lo, mut hi) = (
        P2::new(f64::INFINITY, f64::INFINITY),
        P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for q in polys.iter().flatten() {
        lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    // (Samples clear of the boundary by a fiftieth of the box: the loops are polygons of the
    // p-curves, whose chords stray from them, and a sample nearer than that may be across a cut.)
    let margin = 0.02 * ((hi.x - lo.x).powi(2) + (hi.y - lo.y).powi(2)).sqrt();
    let near_boundary = |uv: P2| {
        polys.iter().any(|poly| {
            poly.iter().enumerate().any(|(k, a)| {
                let b = poly[(k + 1) % poly.len()];
                let d = b - a;
                let t = if d.norm_squared() > 0.0 {
                    ((uv - a).dot(&d) / d.norm_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (a + d * t - uv).norm() < margin
            })
        })
    };
    let mut uvs = Vec::new();
    for i in 0..m {
        for j in 0..m {
            let uv = P2::new(
                lo.x + (hi.x - lo.x) * (i as f64 + 0.5) / m as f64,
                lo.y + (hi.y - lo.y) * (j as f64 + 0.5) / m as f64,
            );
            if super::classify::in_face(&polys, &f.surf, uv, 0.0) == super::classify::InFace::Inside
                && !near_boundary(uv)
            {
                uvs.push(uv);
            }
        }
    }
    if uvs.is_empty() {
        uvs = super::boolean::interior_points_of_polys(&polys);
    }
    uvs
}

/// [`crate::Kernel::sweep_solid`] of a convex polyhedron by the motion `place` (see the module
/// docs): None when `b` isn't one; an error when its boundary isn't made out exactly (a piece of
/// a face judged both inside and out, faces that don't join into one sound solid, a volume the
/// body and the chunks don't bound).
pub fn polyhedron(k: &mut NativeKernel, b: &Body, place: Place) -> Option<Result<OpResult>> {
    if b.solids.len() != 1 {
        return None;
    }
    let ef = b.edge_faces();
    let mut faces = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        let Surface::Plane { f: fr } = &f.surf else {
            return None;
        };
        if f.loops.len() != 1 {
            return None;
        }
        let n = fr.z * if f.sense { 1.0 } else { -1.0 };
        let mut corners = Vec::new();
        let mut edges = Vec::new();
        let mut across = Vec::new();
        for c in &f.loops[0] {
            let e = &b.edges[c.edge];
            if !matches!(e.curve, Curve::Line { .. }) || ef[c.edge].len() != 2 {
                return None;
            }
            corners.push(if c.fwd { e.v[0] } else { e.v[1] });
            edges.push(c.edge);
            across.push(*ef[c.edge].iter().find(|&&g| g != fi)?);
        }
        faces.push(PFace {
            n,
            corners,
            edges,
            across,
        });
    }
    // Convex: every vertex on or behind every face's plane.
    for (fi, pf) in faces.iter().enumerate() {
        let o = b.verts[pf.corners[0]].p;
        if b.verts
            .iter()
            .any(|v| (v.p - o).dot(&pf.n) > 1e-9 * b.bbox().diagonal())
        {
            return None;
        }
        let _ = fi;
    }
    let sw = Sweep {
        b,
        faces,
        place,
        edge_faces: ef,
    };
    let size = b.bbox().diagonal().max(tol::POINT);
    let r = swept(&sw, size);
    if let Err(e) = &r {
        // With CADRS_DUMP_DIR set, the polyhedron whose sweep is refused is left there.
        super::kernel::dump("moving-sweep", b, &e.to_string());
    }
    Some(r.map(|solid| OpResult {
        bodies: vec![k.insert(solid)],
        history: History::default(),
    }))
}

/// The refusal of a moving-contact sweep, saying why.
fn refused(why: String) -> KernelError {
    KernelError::InvalidResult {
        op: "solid sweep",
        why: format!("the swept polyhedron: {why}"),
    }
}

/// The solid `sw`'s polyhedron sweeps (see the module docs).
fn swept(sw: &Sweep, size: f64) -> Result<Body> {
    use rayon::prelude::*;
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    let t0 = web_time::Instant::now();
    let place = sw.place;
    // The chunks: between events, each turning at most CHUNK_TURN.
    let mut cuts = vec![0.0];
    cuts.extend(sw.events());
    cuts.push(1.0);
    let mut chunks: Vec<(f64, f64)> = Vec::new();
    for w in cuts.windows(2) {
        let (ra, _) = place(w[0]);
        let (rb, _) = place(w[1]);
        let m = ((ra.inverse() * rb).angle() / CHUNK_TURN).ceil().max(1.0) as usize;
        for i in 0..m {
            chunks.push((
                w[0] + (w[1] - w[0]) * i as f64 / m as f64,
                w[0] + (w[1] - w[0]) * (i + 1) as f64 / m as f64,
            ));
        }
    }
    // The faces the boundary is made from: the body's trailing polygons at the start, every
    // chunk's sides, and the last chunk's leading polygons at the end. (The body at the start
    // and the chunks' pieces, each sound, bound the volume: theirs summed counts what the motion
    // passes over more than once that many times.)
    let (start, start_tags) = start_body(sw, size)?;
    let mut most = super::mass::body_volume(&start);
    let least = most;
    let mut faces: Vec<Body> = start_tags
        .iter()
        .enumerate()
        .filter(|(_, t)| **t == CAP_END)
        .map(|(fi, _)| super::kernel::subset(&start, &[fi], false))
        .collect();
    super::par::start();
    let fit = 1e-9 * size;
    let pieces: Vec<Result<Option<Piece>>> = chunks
        .par_iter()
        .map(|&(ta, tb)| chunk_piece(sw, ta, tb, fit, size))
        .collect();
    let n = pieces.len();
    for (ci, r) in pieces.into_iter().enumerate() {
        let Some((p, tags)) = r? else { continue };
        most += super::mass::body_volume(&p);
        for (fi, &tag) in tags.iter().enumerate() {
            if tag == SIDE || (tag == CAP_END && ci + 1 == n) {
                faces.push(super::kernel::subset(&p, &[fi], false));
            }
        }
    }
    let planes: Vec<(V3, f64)> = sw
        .faces
        .iter()
        .map(|f| (f.n, f.n.dot(&sw.b.verts[f.corners[0]].p.coords)))
        .collect();
    let oracle = Oracle::new(planes, place, 2048);
    if debug {
        eprintln!(
            "moving sweep: {} chunks, candidates in {:.0} ms",
            chunks.len(),
            (web_time::Instant::now() - t0).as_secs_f64() * 1e3
        );
    }
    boundary_of(faces, &oracle, size, least, most, Vec::new(), None, 0.0)
}

/// The boundary of what a motion fills, from `faces` (sheets of one face each) that close a
/// surface round it (running inside it where the motion passes over the same place again):
/// each judged by `oracle` at points over it (a point just off it along its outward normal
/// swept or not), kept whole when it is wholly on the boundary and dropped when wholly inside,
/// the others cut along the curves the boundary crosses them by and their pieces judged again,
/// the kept faces joined edge to edge (`given`: curves known by construction, taken before any
/// are traced; with `member` they are all of them, each on the boundary all along: none is
/// traced, and the faces and their pieces are judged by it, at places over them; `loose`: how
/// far a face's surface may be off the curves and edges it is cut and joined along, a fitted
/// face's fit). Refused unless that makes one sound solid whose volume is between `least` and
/// `most`.
/// Whether a candidate face's place (the face's index, its (u, v)) is inside the swept solid:
/// read exactly, where the faces' maker can (a face fitted to a surface it knows).
pub(crate) type Member<'a> = dyn Fn(usize, P2) -> bool + Sync + 'a;

#[allow(clippy::too_many_arguments)]
pub(crate) fn boundary_of(
    faces: Vec<Body>,
    oracle: &Oracle,
    size: f64,
    least: f64,
    most: f64,
    given: Vec<Given>,
    member: Option<&Member>,
    loose: f64,
) -> Result<Body> {
    boundary_of_some(
        faces, oracle, size, least, most, given, member, loose, true, None,
    )
}

/// [`boundary_of`], of what may be several solids (`one` false: a boolean's result), with the
/// faces of a surface `whole` picks out made one where they lie side by side.
#[allow(clippy::too_many_arguments)]
pub(crate) fn boundary_of_some(
    faces: Vec<Body>,
    oracle: &Oracle,
    size: f64,
    least: f64,
    most: f64,
    given: Vec<Given>,
    member: Option<&Member>,
    loose: f64,
    one: bool,
    whole: Option<&Whole>,
) -> Result<Body> {
    use rayon::prelude::*;
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    let t0 = web_time::Instant::now();
    let faces: Vec<Cand> = faces.into_iter().map(Cand::new).collect();
    let t_built = web_time::Instant::now();
    let eps = 1e-7 * size;
    let shallow = 1e-8 * size;
    // A face's samples outside (or inside by less than `shallow`) and inside; one inside only
    // by a dip of less than twenty times `shallow` counts as outside: a pocket that shallow
    // changes the solid by nothing measurable, and the curve round it is ill-defined.
    // (A pocket shallower than a millionth of the size is left as it lies: faces dipping
    // inside by less are boundary.)
    let pocket = 1e-6 * size;
    let judge = |i: usize, c: &Body, m: usize| -> (usize, usize) {
        if let Some(member) = member {
            let inn = face_sample_uvs(c, 0, m)
                .into_iter()
                .filter(|uv| member(i, *uv))
                .count();
            return (face_sample_uvs(c, 0, m).len() - inn, inn);
        }
        let (mut out, mut inn, mut band, mut deepest) = (0, 0, 0, 0.0f64);
        for (p, n) in face_samples(c, 0, m) {
            let d = oracle.depth(&(p + n * eps)).0;
            deepest = deepest.min(d);
            // (A sample moved `eps` out stands that far outside when nothing else covers it:
            // less, and another instant reaches it, if only to touch.)
            if d >= 0.5 * eps {
                out += 1;
            } else if d < -shallow {
                inn += 1;
            } else {
                band += 1;
            }
        }
        // (Samples within the band count with the rest: outside when nothing is clearly
        // inside.)
        if inn == 0 || deepest > -pocket {
            (out + inn + band, 0)
        } else if out == 0 {
            (0, inn + band)
        } else {
            (out, inn)
        }
    };
    let classes: Vec<(usize, usize)> = faces
        .par_iter()
        .enumerate()
        .map(|(i, c)| judge(i, &c.body, 9))
        .collect();
    if debug && let Ok(from) = std::env::var("CADRS_SWEEP_CLASSES") {
        let from: usize = from.parse().unwrap_or(0);
        for (i, c) in classes.iter().enumerate().skip(from) {
            let (lo, hi) = grid_box(&faces[i]);
            let ds: Vec<f64> = face_samples(&faces[i].body, 0, 9)
                .iter()
                .map(|(p, n)| oracle.depth(&(p + n * eps)).0)
                .collect();
            eprintln!(
                "  face {i} (u {:.4}..{:.4}, v {:.4}..{:.4}): {} out {} in; depths {:.2e}..{:.2e}",
                lo.x,
                hi.x,
                lo.y,
                hi.y,
                c.0,
                c.1,
                ds.iter().copied().fold(f64::INFINITY, f64::min),
                ds.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            );
        }
    }
    let mixed: Vec<usize> = (0..faces.len())
        .filter(|&i| classes[i].0 > 0 && classes[i].1 > 0)
        .collect();
    let t_judged = web_time::Instant::now();
    // The curves the boundary crosses them along.
    let given_pairs: std::collections::HashSet<(usize, usize)> =
        given.iter().map(|g| (g.a.min(g.b), g.a.max(g.b))).collect();
    let tracer = Tracer {
        cands: &faces,
        oracle,
        size,
        eps,
        shallow,
        given: &given_pairs,
    };
    let net = network(&tracer, &mixed, 9, given, member.is_some());
    let t_net = web_time::Instant::now();
    if debug && let Ok(which) = std::env::var("CADRS_SWEEP_STRETCHES") {
        let wanted: Vec<usize> = which
            .split(',')
            .filter_map(|w| w.trim().parse().ok())
            .collect();
        for (si, st) in net.stretches.iter().enumerate() {
            if !wanted.is_empty() && !wanted.contains(&st.a) && !wanted.contains(&st.b) {
                continue;
            }
            let (p, q) = (st.pts[0].0, st.pts[st.pts.len() - 1].0);
            {
                let n = st.pts.len();
                let gaps = |r: std::ops::Range<usize>| -> Vec<String> {
                    r.map(|i| format!("{:.2e}", (st.pts[i + 1].0 - st.pts[i].0).norm()))
                        .collect()
                };
                eprintln!(
                    "  stretch {si} ({}, {}) {n} points: {p:?} -> {q:?} vertices {:?}; first gaps {:?}, last gaps {:?}; last (u, v) {:?} {:?} | {:?} {:?}",
                    st.a,
                    st.b,
                    st.v,
                    gaps(0..4.min(n - 1)),
                    gaps(n.saturating_sub(5)..n - 1),
                    st.pts[n - 2].1,
                    st.pts[n - 1].1,
                    st.pts[n - 2].2,
                    st.pts[n - 1].2
                );
            }
        }
    }
    if debug {
        // Stretches whose (u, v) run outside their faces' boxes.
        for (si, st) in net.stretches.iter().enumerate() {
            for (k, f) in [st.a, st.b].into_iter().enumerate() {
                let (lo, hi) = grid_box(&faces[f]);
                let uvs = st.pts.iter().map(|p| if k == 0 { p.1 } else { p.2 });
                let (mut l, mut h) = (
                    P2::new(f64::INFINITY, f64::INFINITY),
                    P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
                );
                for uv in uvs {
                    l = P2::new(l.x.min(uv.x), l.y.min(uv.y));
                    h = P2::new(h.x.max(uv.x), h.y.max(uv.y));
                }
                let slack = 1e-6 * (hi - lo).norm();
                if l.x < lo.x - slack
                    || l.y < lo.y - slack
                    || h.x > hi.x + slack
                    || h.y > hi.y + slack
                {
                    eprintln!(
                        "  stretch {si} ({}, {}) on face {f}: (u, v) over ({:.4}, {:.4})..({:.4}, {:.4}), the face's box ({:.4}, {:.4})..({:.4}, {:.4}); {} points, vertices {:?}",
                        st.a,
                        st.b,
                        l.x,
                        l.y,
                        h.x,
                        h.y,
                        lo.x,
                        lo.y,
                        hi.x,
                        hi.y,
                        st.pts.len(),
                        st.v
                    );
                }
            }
        }
    }
    let curves: Vec<StretchCurves> = net.stretches.iter().map(stretch_curves).collect();
    let mut on_net = vec![false; faces.len()];
    for st in &net.stretches {
        on_net[st.a] = true;
        on_net[st.b] = true;
    }
    // The faces cut along them, and their pieces judged again: each must be clearly one or the
    // other.
    let kept: Vec<std::result::Result<Vec<Body>, String>> = (0..faces.len())
        .into_par_iter()
        .map(|i| {
            if !on_net[i] {
                return match classes[i] {
                    (_, 0) => Ok(vec![faces[i].body.clone()]),
                    (0, _) => Ok(Vec::new()),
                    _ => {
                        if debug {
                            let (lo, hi) = grid_box(&faces[i]);
                            let ds: Vec<String> = face_samples(&faces[i].body, 0, 5)
                                .iter()
                                .map(|(p, n)| {
                                    let uv = faces[i].project(p);
                                    format!("({:.4}, {:.4}) {:.2e}", uv.x, uv.y, oracle.depth(&(p + n * eps)).0)
                                })
                                .collect();
                            eprintln!(
                                "  face {i} (u {:.4}..{:.4}, v {:.4}..{:.4}) mixed without a curve: {ds:?}",
                                lo.x, hi.x, lo.y, hi.y
                            );
                        }
                        Err(format!(
                            "face {i} goes inside, but no curve was found on it"
                        ))
                    }
                };
            }
            let mut kept = Vec::new();
            // (Curves given whole end on a face's boundary exactly, in its own (u, v).)
            let (on, exact) = if member.is_some() {
                ((1e-6 * size).max(8.0 * loose), Some(1e-7))
            } else {
                (1e-7 * size, None)
            };
            let pieces = split_by(&faces[i], i, &net, &curves, size, on, exact)?;
            if pieces.is_empty() {
                return Err(format!("face {i} came to nothing when cut along its curves"));
            }
            if debug {
                let (lo, hi) = grid_box(&faces[i]);
                let js: Vec<String> = pieces
                    .iter()
                    .map(|p| {
                        let (o, n) = judge(i, p, 5);
                        let polys = super::classify::face_polygons(p, &p.faces[0]);
                        let inner = super::boolean::interior_points_of_polys(&polys);
                        format!("{o} out {n} in at {:?}", inner.first())
                    })
                    .collect();
                eprintln!(
                    "  face {i} (u {:.4}..{:.4}, v {:.4}..{:.4}) cut into {} pieces: {js:?}",
                    lo.x, hi.x, lo.y, hi.y,
                    pieces.len()
                );
                if pieces.len() <= 1 && net.stretches.iter().any(|st| st.a == i || st.b == i) {
                    for st in net.stretches.iter().filter(|st| st.a == i || st.b == i) {
                        for k in 0..2 {
                            if [st.a, st.b][k] != i {
                                continue;
                            }
                            let uv = |p: &(P3, P2, P2)| if k == 0 { p.1 } else { p.2 };
                            eprintln!(
                                "    stretch ({}, {}) side {k}: {} points from {:?} to {:?} (vertices {:?})",
                                st.a, st.b, st.pts.len(), uv(&st.pts[0]), uv(&st.pts[st.pts.len() - 1]), st.v
                            );
                        }
                    }
                }
            }
            for (g, piece) in pieces.into_iter().enumerate() {
                match judge(i, &piece, 5) {
                    (out, inn) if out > 2 * inn && out > 0 => kept.push(piece),
                    (out, inn) if inn > 2 * out && inn > 0 => {}
                    (out, inn) => {
                        if debug {
                            let (lo, hi) = grid_box(&faces[i]);
                            let sts: Vec<String> = net
                                .stretches
                                .iter()
                                .filter(|st| st.a == i || st.b == i)
                                .map(|st| {
                                    let k = usize::from(st.a != i);
                                    let uv = |p: &(P3, P2, P2)| if k == 0 { p.1 } else { p.2 };
                                    format!(
                                        "({}, {}) {} points from {:?} to {:?}",
                                        st.a,
                                        st.b,
                                        st.pts.len(),
                                        uv(&st.pts[0]),
                                        uv(&st.pts[st.pts.len() - 1])
                                    )
                                })
                                .collect();
                            let ps: Vec<String> = face_samples(&piece, 0, 5)
                                .iter()
                                .filter_map(|(p, n)| {
                                    let d = oracle.depth(&(p + n * eps)).0;
                                    (d <= -shallow).then(|| {
                                        let uv = faces[i].project(p);
                                        format!("{uv:?} {d:.2e}")
                                    })
                                })
                                .collect();
                            eprintln!(
                                "  face {i} (u {:.4}..{:.4}, v {:.4}..{:.4}) piece {g} both: stretches {sts:?}; samples inside {ps:?}",
                                lo.x, hi.x, lo.y, hi.y
                            );
                        }
                        return Err(format!(
                            "face {i}'s piece {g} is both ({out} out, {inn} in)"
                        ));
                    }
                }
            }
            Ok(kept)
        })
        .collect();
    let mut keep: Vec<Body> = Vec::new();
    for r in kept {
        keep.extend(
            r.map_err(|why| refused(format!("{why} (the curves' trouble: {:?})", net.trouble)))?,
        );
    }
    let t_cut = web_time::Instant::now();
    // The kept faces joined, their edges' tolerances covering the gaps their curves leave
    // (wider only where faces meet nearly touching, and the curve between them is ill-defined).
    // (Vertices merged within three times the cutter's tolerance for an end on an edge: two
    // faces place a shared vertex each at its own edge's point.)
    let (mut solid, known, checked) = assemble(
        &keep,
        // (Curves given whole put their ends, and so the pieces' corners, where they are to
        // far within what makes two of them one vertex.)
        (8.0 * loose).max(1e-9 * size),
        if member.is_some() {
            0.05 * GIVEN_VERTEX * size
        } else {
            3e-7 * size
        },
        whole.map(|w| (w, 1e-5 * size)),
    )?;
    let t_tol = web_time::Instant::now();
    solid.fit_tolerances_searched(1e-5 * size);
    if debug {
        eprintln!(
            "moving sweep: tolerances fitted in {:.0} ms",
            t_tol.elapsed().as_secs_f64() * 1e3
        );
    }
    let volume = match known {
        Some(v) if solid.is_solid() => v,
        _ => super::mass::body_volume(&solid),
    };
    if debug && !volume.is_finite() {
        for (vi, v) in solid.verts.iter().enumerate() {
            if !v.p.coords.iter().all(|c| c.is_finite()) {
                eprintln!("moving sweep: vertex {vi} is not finite");
            }
        }
        for (ei, e) in solid.edges.iter().enumerate() {
            let mid = e.curve.point(0.5 * (e.t[0] + e.t[1]));
            if !mid.coords.iter().all(|c| c.is_finite()) || !e.t.iter().all(|t| t.is_finite()) {
                eprintln!("moving sweep: edge {ei} is not finite (t {:?})", e.t);
            }
        }
        let shift = solid.bbox().center().coords;
        for (fi, f) in solid.faces.iter().enumerate() {
            let v = super::mass::face_integrals_of::<false>(&solid, f, &shift)[4];
            if !v.is_finite() {
                let (lo, hi) = {
                    let polys = super::classify::face_polygons(&solid, f);
                    let mut lo = P2::new(f64::INFINITY, f64::INFINITY);
                    let mut hi = P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
                    for q in polys.iter().flatten() {
                        lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
                        hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
                    }
                    (lo, hi)
                };
                eprintln!(
                    "moving sweep: face {fi} has no finite volume: {} loops of {:?} coedges, uv {lo:?}..{hi:?}",
                    f.loops.len(),
                    f.loops.iter().map(|l| l.len()).collect::<Vec<_>>()
                );
            }
        }
    }
    if debug {
        let ms = |a: web_time::Instant, b: web_time::Instant| (b - a).as_secs_f64() * 1e3;
        eprintln!(
            "moving sweep: {} faces ({:.0} ms); judged ({:.0} ms): {} outside, {} \
             mixed; {} curves' stretches ({:.0} ms, trouble {:?}); cut ({:.0} ms) and joined \
             ({:.0} ms): {} faces, volume {volume} (between {least} and {most})",
            faces.len(),
            ms(t0, t_built),
            ms(t_built, t_judged),
            classes.iter().filter(|c| c.1 == 0).count(),
            mixed.len(),
            net.stretches.len(),
            ms(t_judged, t_net),
            net.trouble,
            ms(t_net, t_cut),
            ms(t_cut, web_time::Instant::now()),
            solid.faces.len(),
        );
    }
    let mut problems = if checked { Vec::new() } else { solid.check() };
    if debug && let Some(first) = problems.first() {
        // The face named first, its loops' p-curve ends.
        let msg = format!("{first:?}");
        if let Some(fi) = msg
            .split("face ")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse::<usize>().ok())
            && let Some(f) = solid.faces.get(fi)
        {
            for (li, l) in f.loops.iter().enumerate() {
                let ends: Vec<String> = l
                    .iter()
                    .map(|co| {
                        let e = &solid.edges[co.edge];
                        let (t0, t1) = super::topo::co_range(e, co.fwd);
                        let (a, b) = (co.pc.point(t0), co.pc.point(t1));
                        let (v0, v1) = if co.fwd { (e.v[0], e.v[1]) } else { (e.v[1], e.v[0]) };
                        let off = |uv: P3, v: usize| {
                            (f.surf.eval(uv.x, uv.y) - solid.verts[v].p).norm()
                        };
                        format!(
                            "edge {} t {:?} fwd {}: ({:.6}, {:.6}) -> ({:.6}, {:.6}); p-curve ends off their vertices {:.1e}, {:.1e}; curve ends off them {:.1e}, {:.1e}",
                            co.edge, e.t, co.fwd, a.x, a.y, b.x, b.y,
                            off(a, v0), off(b, v1),
                            (e.curve.point(t0) - solid.verts[v0].p).norm(),
                            (e.curve.point(t1) - solid.verts[v1].p).norm()
                        )
                    })
                    .collect();
                eprintln!("moving sweep: face {fi} loop {li}: {ends:?}");
            }
        }
    }
    if problems.is_empty() {
        // (Faces crossing: a face kept whole that went inside, its curve missed.)
        problems = super::check_faces::face_crossings(&solid);
    }
    if debug && solid.solids.len() != 1 {
        let ef = solid.edge_faces();
        let odd: Vec<usize> = (0..solid.edges.len())
            .filter(|&e| !solid.edges[e].degenerate && ef[e].len() != 2)
            .collect();
        eprintln!(
            "moving sweep: {} pieces kept, {} faces joined",
            keep.len(),
            solid.faces.len()
        );
        for &e in &odd {
            let ed = &solid.edges[e];
            let m = ed.curve.point(0.5 * (ed.t[0] + ed.t[1]));
            // The nearest other edge midway: how far, and whose.
            let mut nearest: Option<(f64, usize)> = None;
            for (o, od) in solid.edges.iter().enumerate() {
                if o == e || od.degenerate {
                    continue;
                }
                let s = od.curve.closest(&m, od.t[0], od.t[1]);
                let d = (od.curve.point(s) - m).norm();
                if nearest.is_none_or(|(nd, _)| d < nd) {
                    nearest = Some((d, o));
                }
            }
            let near = nearest.map(|(d, o)| {
                let od = &solid.edges[o];
                format!(
                    "edge {o} of faces {:?} is {d:.2e} off midway (its ends {:.2e}, {:.2e} from this one's)",
                    ef[o],
                    (solid.verts[od.v[0]].p - solid.verts[ed.v[0]].p).norm().min((solid.verts[od.v[1]].p - solid.verts[ed.v[0]].p).norm()),
                    (solid.verts[od.v[0]].p - solid.verts[ed.v[1]].p).norm().min((solid.verts[od.v[1]].p - solid.verts[ed.v[1]].p).norm()),
                )
            });
            eprintln!(
                "moving sweep: edge {e} of {} faces {:?}: {:?} -> {:?}; nearest {near:?}",
                ef[e].len(),
                ef[e],
                solid.verts[ed.v[0]].p,
                solid.verts[ed.v[1]].p
            );
        }
    }
    if solid.solids.is_empty() || (one && solid.solids.len() != 1) || !problems.is_empty() {
        return Err(refused(format!(
            "its faces don't join into one sound solid ({} solids; {:?})",
            solid.solids.len(),
            problems.first()
        )));
    }
    if !(volume >= least * (1.0 - 1e-9) && volume <= most * (1.0 + 1e-9)) {
        return Err(refused(format!(
            "volume {volume} outside what the body and the chunks bound ({least}, {most})"
        )));
    }
    Ok(solid)
}
