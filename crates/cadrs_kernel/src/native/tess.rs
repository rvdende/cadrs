//! Tessellation: every edge is discretised once (so neighbouring faces share exactly the same
//! boundary points and the mesh is watertight), then each face is triangulated in its (u, v)
//! domain by a constrained Delaunay triangulation, seeded with interior points spaced by the
//! surface's curvature and refined until every triangle meets the controls ([`TessParams`]:
//! chord deviation measured against the surface itself, normal angle, maximum edge length and
//! aspect ratio). Refinement only ever inserts interior points: the boundary stays the shared
//! edge discretisation.
//!
//! The (u, v) boundary of a face is built from its p-curves, corrected where they stray from the
//! edges (re-projected onto the surface) and unwrapped on closed surfaces: each coedge is placed
//! on the copy of the period that continues the previous one, and a loop that only closes by
//! running along a pole (a sphere's, an apex, a closed B-spline's collapsed row) without a
//! degenerate edge there gets one (see [`chain_loop`]).
//!
//! Faces are meshed in parallel and kept as separate chunks (`FaceMesh`) so callers can cache and
//! re-upload only the faces an operation changed; the result is deterministic (the same body and
//! controls give bit-identical meshes on any number of threads).

use std::collections::VecDeque;

use rustc_hash::FxHashMap as HashMap;
use std::f64::consts::{FRAC_PI_2, TAU};

use rayon::prelude::*;
use spade::handles::FixedVertexHandle;
use spade::{
    AngleLimit, ConstrainedDelaunayTriangulation, HasPosition, Point2 as SPoint,
    PositionInTriangulation, RefinementParameters, Triangulation,
};

use super::kernel::{NativeKernel, vertex_ids};
use super::math::*;
use super::surface::{D2, Surface};
use super::tol;
use super::topo::{Body, Coedge, Face, co_range};
use crate::{BodyId, EdgeId, FaceId, KernelError, Result, Tessellation, TriMesh, VertexId};

/// A fast hasher (FxHash's mix) for the mesher's sets of vertex indices and coordinates, probed
/// for every triangle in every refinement round: SipHash's guard against chosen keys isn't
/// needed there, and the sets are only probed, never iterated, so no order depends on it.
#[derive(Default, Clone, Copy)]
struct FxHasher(u64);

impl FxHasher {
    #[inline]
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for FxHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut b = [0u8; 8];
            b[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(b));
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }
}

type FastSet<T> = std::collections::HashSet<T, std::hash::BuildHasherDefault<FxHasher>>;

/// A triangle the refinement still has to check: its corners' ids sorted, its index in the
/// triangulation, its corners, and which of its sides are on the boundary.
type Unchecked = ([usize; 3], usize, [Vx; 3], [bool; 3]);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceMesh {
    pub positions: Vec<P3>,
    pub normals: Vec<V3>,
    /// Each vertex's (u, v) on the face's surface (in the surface's own range).
    pub uvs: Vec<P2>,
    pub indices: Vec<[u32; 3]>,
}

/// Each edge's sample parameters and points.
pub type EdgeSamples = Vec<(Vec<f64>, Vec<P3>)>;

/// The aspect ratio of an equilateral triangle (the smallest there is) in [`TessParams`]'s
/// measure, the longest side over the height onto it.
pub const MIN_ASPECT: f64 = 1.154_700_538_379_251_7; // 2 / √3

/// The mesher's controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TessParams {
    /// The largest distance from a triangle or an edge segment to the exact surface or curve
    /// (model units). Positive.
    pub chord: f64,
    /// The largest angle between the surface normals at a triangle's corners, and the largest
    /// turn of an edge segment (radians). Positive; angles over π/2 act as π/2.
    pub angle: f64,
    /// The longest a triangle side or edge segment may be (model units); 0 for no limit.
    pub max_edge: f64,
    /// The largest aspect ratio of a triangle, its longest side over the height onto it (an
    /// equilateral triangle's is [`MIN_ASPECT`]); 0 for no limit. Met by splitting interior
    /// sides only: a triangle whose longest side is on the face's boundary keeps it, as the
    /// boundary is shared with the neighbouring face.
    pub max_aspect: f64,
}

impl TessParams {
    /// Chord deviation and angle, no edge length or aspect limit.
    ///
    /// # Errors
    /// [`KernelError::InvalidInput`] when the chord or the angle isn't positive and finite (a
    /// chord of 0 isn't "angle only": it would ask for an infinitely fine mesh).
    pub fn new(chord: f64, angle: f64) -> Result<Self> {
        let p = Self {
            chord,
            angle,
            max_edge: 0.0,
            max_aspect: 0.0,
        };
        p.validate()?;
        Ok(p)
    }

    /// The same with a maximum edge length and aspect ratio (each 0 for no limit).
    ///
    /// # Errors
    /// [`KernelError::InvalidInput`] for a negative or non-finite limit, or an aspect ratio
    /// below [`MIN_ASPECT`].
    pub fn with_limits(self, max_edge: f64, max_aspect: f64) -> Result<Self> {
        let p = Self {
            max_edge,
            max_aspect,
            ..self
        };
        p.validate()?;
        Ok(p)
    }

    /// Checks the controls (see the fields).
    ///
    /// # Errors
    /// [`KernelError::InvalidInput`] naming the bad control.
    pub fn validate(&self) -> Result<()> {
        let bad = |what: &str, v: f64| {
            Err(KernelError::InvalidInput(format!(
                "tessellation {what} {v} is invalid"
            )))
        };
        if !(self.chord.is_finite() && self.chord > 0.0) {
            return bad("chord deviation (must be positive)", self.chord);
        }
        if !(self.angle.is_finite() && self.angle > 0.0) {
            return bad("angle (must be positive)", self.angle);
        }
        if !(self.max_edge.is_finite() && self.max_edge >= 0.0) {
            return bad("maximum edge length (must be 0 or positive)", self.max_edge);
        }
        if !(self.max_aspect.is_finite()
            && (self.max_aspect == 0.0 || self.max_aspect >= MIN_ASPECT))
        {
            return bad(
                "maximum aspect ratio (must be 0 or at least 2/√3)",
                self.max_aspect,
            );
        }
        Ok(())
    }

    /// For the internal callers that pass bare numbers: a chord or angle that isn't positive and
    /// finite takes [`Tessellation::default`]'s.
    pub fn lenient(chord: f64, angle: f64) -> Self {
        let d = Tessellation::default();
        Self {
            chord: if chord.is_finite() && chord > 0.0 {
                chord
            } else {
                d.deflection
            },
            angle: if angle.is_finite() && angle > 0.0 {
                angle
            } else {
                d.angle
            },
            max_edge: 0.0,
            max_aspect: 0.0,
        }
    }

    fn ang(&self) -> f64 {
        self.angle.min(FRAC_PI_2)
    }
}

impl TryFrom<Tessellation> for TessParams {
    type Error = KernelError;
    fn try_from(q: Tessellation) -> Result<Self> {
        Self::new(q.deflection, q.angle)
    }
}

// ---------------------------------------------------------------------------------------------
// Edges.

/// Each edge's sample parameters and points (see [`edge_samples_with`]); a chord or angle that
/// isn't positive takes the default's.
pub fn edge_samples(body: &Body, defl: f64, ang: f64) -> EdgeSamples {
    edge_samples_with(body, &TessParams::lenient(defl, ang))
}

/// Each edge's sample parameters and points: the curve's own sampling for the angle, then
/// segments bisected until each lies within the chord of the curve (checked at its quarter
/// points) and is no longer than the maximum edge. The ends are exactly the edge's vertices.
pub fn edge_samples_with(body: &Body, p: &TessParams) -> EdgeSamples {
    body.edges
        .par_iter()
        .map(|e| {
            if e.degenerate {
                return (
                    vec![e.t[0], e.t[1]],
                    vec![body.verts[e.v[0]].p, body.verts[e.v[1]].p],
                );
            }
            let ts = e.curve.sample_params(e.t[0], e.t[1], p.chord, p.ang());
            let ts = refine_polyline(|t| e.point(t), &ts, p);
            let mut pts: Vec<P3> = ts.iter().map(|&t| e.point(t)).collect();
            let n = pts.len();
            pts[0] = body.verts[e.v[0]].p;
            pts[n - 1] = body.verts[e.v[1]].p;
            // Samples bunched within the edge's tolerance (a curve with knots piled up at an end,
            // or ending a hair off its vertex) are one: they only make zero-area slivers.
            let gap = e.tol.max(super::topo::TOL_MIN);
            let (mut kt, mut kp) = (vec![ts[0]], vec![pts[0]]);
            for i in 1..n - 1 {
                if (pts[i] - kp[kp.len() - 1]).norm() > gap && (pts[i] - pts[n - 1]).norm() > gap {
                    kt.push(ts[i]);
                    kp.push(pts[i]);
                }
            }
            kt.push(ts[n - 1]);
            kp.push(pts[n - 1]);
            (kt, kp)
        })
        .collect()
}

/// Bisects the segments of a sampled curve that stray more than the chord from it, or are
/// longer than the maximum edge.
fn refine_polyline(c: impl Fn(f64) -> P3, ts: &[f64], p: &TessParams) -> Vec<f64> {
    // Enough for a 1 km edge at 0.1 mm segments; past it the curve's sampling stands.
    const MAX_SAMPLES: usize = 1 << 16;
    fn split(
        c: &impl Fn(f64) -> P3,
        p: &TessParams,
        (a, pa): (f64, P3),
        (b, pb): (f64, P3),
        depth: u32,
        out: &mut Vec<f64>,
    ) {
        let long = p.max_edge > 0.0 && (pb - pa).norm() > p.max_edge;
        let off = || {
            [0.25, 0.5, 0.75]
                .iter()
                .any(|&s| seg_dist3(&c(a + (b - a) * s), &pa, &pb) > p.chord)
        };
        if depth < 24 && out.len() < MAX_SAMPLES && (long || off()) {
            let m = 0.5 * (a + b);
            let pm = c(m);
            split(c, p, (a, pa), (m, pm), depth + 1, out);
            split(c, p, (m, pm), (b, pb), depth + 1, out);
        } else {
            out.push(b);
        }
    }
    let Some(&t0) = ts.first() else {
        return Vec::new();
    };
    let mut out = vec![t0];
    let mut prev = (t0, c(t0));
    for &t in &ts[1..] {
        let cur = (t, c(t));
        split(&c, p, prev, cur, 0, &mut out);
        prev = cur;
    }
    out
}

/// Distance from `q` to the segment `a`–`b`.
fn seg_dist3(q: &P3, a: &P3, b: &P3) -> f64 {
    let ab = b - a;
    let l = ab.norm_squared();
    let t = if l < tol::TINY {
        0.0
    } else {
        ((q - a).dot(&ab) / l).clamp(0.0, 1.0)
    };
    (a + ab * t - q).norm()
}

// ---------------------------------------------------------------------------------------------
// The face's parameter domain.

/// A face's surface with its closed directions (start and period per direction). The mesher
/// works in unwrapped (u, v), which can run past a closed B-spline's range; evaluation wraps
/// back into it.
struct Domain<'a> {
    s: &'a Surface,
    per: [Option<(f64, f64)>; 2],
}

impl<'a> Domain<'a> {
    /// `tol`: how close a closed surface's two ends must be.
    fn new(s: &'a Surface, tol: f64) -> Self {
        let ((u0, u1), (v0, v1)) = s.param_box();
        let mut per = [None, None];
        if s.u_periodic() {
            per[0] = Some((u0, u1 - u0));
        }
        if s.v_periodic() {
            per[1] = Some((v0, v1 - v0));
        }
        if let Surface::Nurbs(n) = s {
            // A B-spline closed on itself (its two ends the same row of points).
            let at = |k: usize, a: f64, b: f64| a + (b - a) * k as f64 / 6.0;
            if (0..=6)
                .all(|k| (n.point(u0, at(k, v0, v1)) - n.point(u1, at(k, v0, v1))).norm() <= tol)
            {
                per[0] = Some((u0, u1 - u0));
            }
            if (0..=6)
                .all(|k| (n.point(at(k, u0, u1), v0) - n.point(at(k, u0, u1), v1)).norm() <= tol)
            {
                per[1] = Some((v0, v1 - v0));
            }
        }
        // A period must be a real length.
        for p in &mut per {
            if p.is_some_and(|(_, w)| !(w.is_finite() && w > tol::ROUNDOFF_FINE)) {
                *p = None;
            }
        }
        Self { s, per }
    }

    fn wrap(&self, u: f64, v: f64) -> (f64, f64) {
        let w = |x: f64, p: Option<(f64, f64)>| match p {
            Some((a, w)) if matches!(self.s, Surface::Nurbs(_) | Surface::Extrusion { .. }) => {
                a + (x - a).rem_euclid(w)
            }
            _ => x,
        };
        (w(u, self.per[0]), w(v, self.per[1]))
    }

    fn eval(&self, q: P2) -> P3 {
        let (u, v) = self.wrap(q.x, q.y);
        self.s.eval(u, v)
    }

    fn d1(&self, q: P2) -> (P3, V3, V3) {
        let (u, v) = self.wrap(q.x, q.y);
        self.s.d1(u, v)
    }

    fn d2(&self, u: f64, v: f64) -> D2 {
        let (u, v) = self.wrap(u, v);
        self.s.d2(u, v)
    }

    fn normal(&self, q: P2) -> V3 {
        let (u, v) = self.wrap(q.x, q.y);
        self.s.normal(u, v)
    }

    /// `q` moved onto a seam (a whole number of periods from the start) or a B-spline's range
    /// end when within rounding of it.
    fn snap(&self, mut q: P2) -> P2 {
        let range = match self.s {
            Surface::Nurbs(n) => {
                let ((u0, u1), (v0, v1)) = n.range();
                [Some((u0, u1)), Some((v0, v1))]
            }
            _ => [None, None],
        };
        for d in 0..2 {
            if let Some((a, w)) = self.per[d] {
                let k = ((q[d] - a) / w).round();
                if (q[d] - (a + k * w)).abs() <= tol::REL * w {
                    q[d] = a + k * w;
                }
            } else if let Some((a, b)) = range[d] {
                let eps = tol::REL * (b - a).abs();
                for end in [a, b] {
                    if (q[d] - end).abs() <= eps {
                        q[d] = end;
                    }
                }
            }
        }
        q
    }

    /// Whether the iso-line through `q` along closed direction `dir` is a single point (a pole).
    fn is_pole(&self, q: P2, dir: usize, tol: f64) -> bool {
        let Some((_, w)) = self.per[dir] else {
            return false;
        };
        let p = self.eval(q);
        [0.25, 0.5, 0.75].iter().all(|&s| {
            let mut r = q;
            r[dir] += w * s;
            (self.eval(r) - p).norm() <= tol
        })
    }
}

// ---------------------------------------------------------------------------------------------
// The boundary.

/// A boundary point: where it is in the face's (u, v) and in space (an edge sample, shared with
/// the neighbouring face).
#[derive(Debug, Clone, Copy)]
struct BPt {
    uv: P2,
    p: P3,
}

/// The p-curve's (u, v) for a sample, or the surface's projection of the sample where the
/// p-curve strays more than `tol` from it (an approximated p-curve) and the projection is better
/// and nearby.
fn fix_uv(dom: &Domain, uv: P2, p: &P3, tol: f64) -> P2 {
    let (q, su, sv) = dom.d1(uv);
    let d0 = (q - p).norm();
    if d0 <= tol || !d0.is_finite() {
        return uv;
    }
    let (u, v) = dom.s.project(p, Some((uv.x, uv.y)));
    let mut w = P2::new(u, v);
    // The copy of a closed direction nearest the p-curve's.
    for d in 0..2 {
        if let Some((_, per)) = dom.per[d] {
            w[d] -= ((w[d] - uv[d]) / per).round() * per;
        }
    }
    if !(w.x.is_finite() && w.y.is_finite()) {
        return uv;
    }
    let d1 = (dom.eval(w) - p).norm();
    let moved = (w.x - uv.x).abs() * su.norm() + (w.y - uv.y).abs() * sv.norm();
    if d1 < d0 && moved <= 4.0 * d0 + tol {
        w
    } else {
        uv
    }
}

/// A coedge's boundary points in its direction, both ends included, unwrapped along closed
/// directions so consecutive points are less than half a period apart.
fn coedge_points(
    body: &Body,
    c: &Coedge,
    es: &[(Vec<f64>, Vec<P3>)],
    dom: &Domain,
    p: &TessParams,
    fix_tol: f64,
) -> Vec<BPt> {
    let e = &body.edges[c.edge];
    let (ts, ps) = &es[c.edge];
    let uv_at = |t: f64| {
        let q = c.pc.point(t);
        P2::new(q.x, q.y)
    };
    // A point edge, flagged or not (an apex's or a pole's edge sampled down to its two ends at
    // one point): along the p-curve across the pole, all at its point (the last at the end
    // vertex, should it be a hair off).
    let collapsed = e.degenerate
        || (ps.len() == 2 && (ps[0] - ps[1]).norm() <= e.tol.max(super::topo::TOL_MIN));
    // The points with the p-curve's own (u, v) at each.
    let (mut out, own): (Vec<BPt>, Vec<P2>) = if collapsed {
        let (a, b) = co_range(e, c.fwd);
        let (pa, pb) = if c.fwd {
            (ps[0], ps[ps.len() - 1])
        } else {
            (ps[ps.len() - 1], ps[0])
        };
        let (qa, qb) = (uv_at(a), uv_at(b));
        let turns = (0..2)
            .filter_map(|d| dom.per[d].map(|(_, per)| ((qb[d] - qa[d]) / per).abs()))
            .fold(0.0, f64::max);
        let n = pole_points(turns, (b - a).abs() / p.ang().max(0.05));
        (0..=n)
            .map(|i| {
                let uv = uv_at(a + (b - a) * i as f64 / n as f64);
                (
                    BPt {
                        uv,
                        p: if i == n { pb } else { pa },
                    },
                    uv,
                )
            })
            .unzip()
    } else {
        let at = |i: usize| {
            let uv = uv_at(ts[i]);
            (
                BPt {
                    uv: fix_uv(dom, uv, &ps[i], fix_tol),
                    p: ps[i],
                },
                uv,
            )
        };
        if c.fwd {
            (0..ts.len()).map(at).unzip()
        } else {
            (0..ts.len()).rev().map(at).unzip()
        }
    };
    // Exactly on a seam or a B-spline's range end when within rounding of it (a projection's
    // 1e-13 off a seam makes slivers along it).
    for b in &mut out {
        b.uv = dom.snap(b.uv);
    }
    // Each point on the copy of the period that steps from the one before as the p-curve does
    // (a projection lands on any copy). Mass properties integrate along the p-curve as it is: a
    // meridian through a sphere's pole whose p-curve steps half a turn there (trj3/4/5_s1's
    // feet) must turn the same way here, or the mesh covers another part of the sphere. Where
    // the p-curve itself jumps by about a whole period (a curve wrapped round a seam), the
    // nearest copy.
    for d in 0..2 {
        if let Some((_, per)) = dom.per[d] {
            for i in 1..out.len() {
                let jump = out[i].uv[d] - out[i - 1].uv[d];
                let step = own[i][d] - own[i - 1][d];
                let along = if step.abs() < 0.75 * per { step } else { 0.0 };
                if (jump - along).abs() > 0.5 * per {
                    out[i].uv[d] -= ((jump - along) / per).round() * per;
                }
            }
        }
    }
    out
}

/// How many segments along a pole spanning `turns` periods (`by_angle` for the angle): many, so
/// the points by the pole each see one run of the pole's (u, v) copies (else an interior point
/// joined to two copies apart pinches the mesh there when they become one point).
fn pole_points(turns: f64, by_angle: f64) -> usize {
    ((turns * 512.0).max(by_angle).ceil() as usize).clamp(1, 1024)
}

/// Joins a loop's coedges into one closed (u, v) polygon (its last point not repeated).
///
/// On a closed direction each coedge moves to the copy of the period that starts where the last
/// one ended. A loop that then still doesn't close by whole periods must turn around the
/// surface at a pole; where the model has no degenerate edge for it (a sphere-like closed
/// B-spline whose loop runs down a seam to the pole and back up the same seam), the loop is
/// opened at its first pole junction and the gap is bridged along the pole, its points all the
/// pole's point. Without a pole the loop is left as it is.
fn chain_loop(mut parts: Vec<Vec<BPt>>, dom: &Domain, tol: f64, ang: f64) -> Vec<BPt> {
    parts.retain(|c| !c.is_empty());
    let n = parts.len();
    if n == 0 {
        return Vec::new();
    }
    let last = |c: &Vec<BPt>| c[c.len() - 1];
    for d in 0..2 {
        let Some((_, per)) = dom.per[d] else { continue };
        let shift = |c: &mut Vec<BPt>, by: f64| c.iter_mut().for_each(|b| b.uv[d] += by);
        for k in 1..n {
            let m = ((last(&parts[k - 1]).uv[d] - parts[k][0].uv[d]) / per).round();
            if m != 0.0 {
                shift(&mut parts[k], m * per);
            }
        }
        let m = ((parts[0][0].uv[d] - last(&parts[n - 1]).uv[d]) / per).round();
        if m != 0.0 && !dom.is_pole(last(&parts[n - 1]).uv, d, tol) {
            // Open the loop at a pole junction inside it instead (the closing one isn't).
            if let Some(j) = (1..n).find(|&k| dom.is_pole(last(&parts[k - 1]).uv, d, tol)) {
                for c in &mut parts[j..] {
                    shift(c, m * per);
                }
            }
        }
    }
    let mut out = Vec::new();
    for k in 0..n {
        let c = &parts[k];
        out.extend_from_slice(&c[..c.len() - 1]);
        let (a, b) = (last(c), parts[(k + 1) % n][0]);
        // A jump between coedges at one point in space: along a pole.
        let gap = b.uv - a.uv;
        if gap.norm() > tol::REL_FINE * (1.0 + a.uv.coords.norm()) && (a.p - b.p).norm() <= tol {
            let turns: f64 = (0..2)
                .filter_map(|d| dom.per[d].map(|(_, per)| (gap[d] / per).abs()))
                .fold(0.0, f64::max);
            let m = pole_points(turns, turns * TAU / ang.max(0.05));
            out.extend((1..m).map(|i| BPt {
                uv: a.uv + gap * (i as f64 / m as f64),
                p: a.p,
            }));
        }
    }
    // Neighbours at one point in space and a hair apart in (u, v) are one point: a degenerate
    // edge across a gap within a file's tolerance at a plain point (trj9_ap209's blocks: an edge
    // ending 1.5e-6 short of the next on a plane) runs back along the edge before it, and the
    // triangulation then lost half the face. (Not a pole's run: its points are far apart in
    // (u, v).)
    let (mu, mv) = dom.s.metric();
    let near = |a: &BPt, b: &BPt| {
        (a.p - b.p).norm() <= tol && ((a.uv.x - b.uv.x) * mu).hypot((a.uv.y - b.uv.y) * mv) <= tol
    };
    out.dedup_by(|b, a| near(a, b));
    while out.len() > 3 && near(&out[out.len() - 1], &out[0]) {
        out.pop();
    }
    out
}

/// The face's boundary loops in (u, v), inner loops moved to the copy of the period inside the
/// outer one.
fn boundary(
    body: &Body,
    f: &Face,
    es: &[(Vec<f64>, Vec<P3>)],
    dom: &Domain,
    p: &TessParams,
    tol: f64,
) -> Vec<Vec<BPt>> {
    let fix_tol = (1e-3 * p.chord).max(tol);
    let mut loops: Vec<Vec<BPt>> = f
        .loops
        .iter()
        .map(|l| {
            chain_loop(
                l.iter()
                    .map(|c| coedge_points(body, c, es, dom, p, fix_tol))
                    .collect(),
                dom,
                tol,
                p.ang(),
            )
        })
        .collect();
    if loops.len() > 1 && dom.per.iter().any(Option::is_some) {
        let bx = |l: &[BPt]| {
            l.iter().fold(
                [
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                ],
                |b, q| {
                    [
                        b[0].min(q.uv.x),
                        b[1].max(q.uv.x),
                        b[2].min(q.uv.y),
                        b[3].max(q.uv.y),
                    ]
                },
            )
        };
        let area = |b: [f64; 4]| (b[1] - b[0]).max(0.0) * (b[3] - b[2]).max(0.0);
        let outer = (0..loops.len())
            .max_by(|&a, &b| area(bx(&loops[a])).total_cmp(&area(bx(&loops[b]))))
            .unwrap_or(0);
        let ob = bx(&loops[outer]);
        for (k, l) in loops.iter_mut().enumerate() {
            if k == outer || l.is_empty() {
                continue;
            }
            let b = bx(l);
            for d in 0..2 {
                if let Some((_, per)) = dom.per[d] {
                    let m = (((ob[2 * d] + ob[2 * d + 1]) - (b[2 * d] + b[2 * d + 1])) * 0.5 / per)
                        .round();
                    l.iter_mut().for_each(|q| q.uv[d] += m * per);
                }
            }
        }
    }
    loops
}

/// Which of the face's loops stand for its whole closed surface: those of only seams, slits and
/// degenerate edges (each edge twice in the face) that enclose no area in (u, v) (a sphere read
/// with its seam's two sides on the same meridian), around which the rest are holes.
fn surface_loops(body: &Body, f: &Face, loops: &[Vec<BPt>], dom: &Domain) -> Vec<bool> {
    let mut uses: HashMap<usize, usize> = HashMap::default();
    for c in f.loops.iter().flatten() {
        *uses.entry(c.edge).or_insert(0) += 1;
    }
    let ((u0, u1), (v0, v1)) = dom.s.param_box();
    let full =
        (dom.per[0].map_or(u1 - u0, |(_, w)| w) * dom.per[1].map_or(v1 - v0, |(_, w)| w)).abs();
    f.loops
        .iter()
        .zip(loops)
        .map(|(l, pts)| {
            let n = pts.len();
            let area = 0.5
                * (0..n)
                    .map(|i| {
                        pts[i].uv.x * pts[(i + 1) % n].uv.y - pts[i].uv.y * pts[(i + 1) % n].uv.x
                    })
                    .sum::<f64>()
                    .abs();
            (dom.per[0].is_some() || dom.per[1].is_some())
                && l.iter()
                    .all(|c| body.edges[c.edge].degenerate || uses[&c.edge] == 2)
                && area <= tol::REL_POINT * full
        })
        .collect()
}

/// The boundary of a whole closed surface's domain: its (u, v) rectangle, the seam's points
/// shared by its two sides and a pole row's all one point. None unless each direction is
/// closed or ends at poles.
fn whole_boundary(dom: &Domain, p: &TessParams, tol: f64) -> Option<Vec<BPt>> {
    let ((pu0, pu1), (pv0, pv1)) = dom.s.param_box();
    let (u0, wu) = dom.per[0].unwrap_or((pu0, pu1 - pu0));
    let (v0, wv) = dom.per[1].unwrap_or((pv0, pv1 - pv0));
    if !(wu.is_finite() && wv.is_finite() && wu.abs() < 1e3 && wv.abs() < 1e3) {
        return None;
    }
    let (hu, hv) = steps(dom, (u0, u0 + wu), (v0, v0 + wv), p.chord, p.ang());
    let count = |w: f64, h: f64, lo: usize| {
        if h.is_finite() && h > 0.0 {
            ((w.abs() / h).ceil() as usize).clamp(lo, 400)
        } else {
            lo
        }
    };
    let (nu, nv) = (count(wu, hu, 8), count(wv, hv, 4));
    let (u, v) = (
        |i: usize| u0 + wu * i as f64 / nu as f64,
        |j: usize| v0 + wv * j as f64 / nv as f64,
    );
    // Each side not closed must be a pole row.
    let pole = |j: usize, d: usize| dom.is_pole(P2::new(u(0), v(j)), d, tol);
    let (pole0, pole1) = (
        dom.per[1].is_none() && pole(0, 0),
        dom.per[1].is_none() && pole(nv, 0),
    );
    if dom.per[0].is_none() || (dom.per[1].is_none() && !(pole0 && pole1)) {
        return None;
    }
    // The point of grid corner (i, j): one evaluation per point of the surface.
    let at = |i: usize, j: usize| {
        let i = i % nu;
        let j = if dom.per[1].is_some() { j % nv } else { j };
        let q = if (j == 0 && pole0) || (j == nv && pole1) {
            P2::new(u(0), v(j))
        } else {
            P2::new(u(i), v(j))
        };
        dom.eval(q)
    };
    let mut out = Vec::with_capacity(2 * (nu + nv));
    out.extend((0..nu).map(|i| BPt {
        uv: P2::new(u(i), v(0)),
        p: at(i, 0),
    }));
    out.extend((0..nv).map(|j| BPt {
        uv: P2::new(u(nu), v(j)),
        p: at(nu, j),
    }));
    out.extend((1..=nu).rev().map(|i| BPt {
        uv: P2::new(u(i), v(nv)),
        p: at(i, nv),
    }));
    out.extend((1..=nv).rev().map(|j| BPt {
        uv: P2::new(u(0), v(j)),
        p: at(0, j),
    }));
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// Interior seeding.

/// Parameter steps in u and v so the mesh stays within `defl` and `ang` of the surface,
/// estimated over a sample grid of the face's uv box.
fn steps(dom: &Domain, ub: (f64, f64), vb: (f64, f64), defl: f64, ang: f64) -> (f64, f64) {
    if matches!(dom.s, Surface::Plane { .. }) {
        return (f64::INFINITY, f64::INFINITY);
    }
    let n = 6;
    let (mut ku, mut kv): (f64, f64) = (0.0, 0.0); // max |S_uu·n| / |S_u| style rates
    let (mut cu, mut cv): (f64, f64) = (0.0, 0.0); // max |S_uu·n|
    for i in 0..=n {
        for j in 0..=n {
            let u = ub.0 + (ub.1 - ub.0) * i as f64 / n as f64;
            let v = vb.0 + (vb.1 - vb.0) * j as f64 / n as f64;
            let d = dom.d2(u, v);
            let nn = d[1].cross(&d[2]);
            if nn.norm() < tol::DEGENERATE {
                continue;
            }
            let nn = nn.normalize();
            let suu = d[3].dot(&nn).abs();
            let svv = d[5].dot(&nn).abs();
            cu = cu.max(suu);
            cv = cv.max(svv);
            let lu = d[1].norm().max(tol::DEGENERATE);
            let lv = d[2].norm().max(tol::DEGENERATE);
            ku = ku.max(suu / lu);
            kv = kv.max(svv / lv);
        }
    }
    let step = |c: f64, k: f64| -> f64 {
        if c < tol::DEGENERATE {
            return f64::INFINITY;
        }
        let by_defl = (8.0 * defl / c).sqrt();
        // The same angular step as the edges (`curve::arc_step`): half of `ang`.
        let by_ang = if k > tol::DEGENERATE {
            0.5 * ang / k
        } else {
            f64::INFINITY
        };
        by_defl.min(by_ang)
    };
    (step(cu, ku), step(cv, kv))
}

/// The mean |S_u| and |S_v| over a sample grid of the box.
fn mean_speeds(dom: &Domain, ub: (f64, f64), vb: (f64, f64)) -> Option<(f64, f64)> {
    let n = 6;
    let (mut su, mut sv, mut k) = (0.0, 0.0, 0usize);
    for i in 0..=n {
        for j in 0..=n {
            let q = P2::new(
                ub.0 + (ub.1 - ub.0) * i as f64 / n as f64,
                vb.0 + (vb.1 - vb.0) * j as f64 / n as f64,
            );
            let (_, du, dv) = dom.d1(q);
            su += du.norm();
            sv += dv.norm();
            k += 1;
        }
    }
    let (su, sv) = (su / k as f64, sv / k as f64);
    (su > tol::DEGENERATE && sv > tol::DEGENERATE && su.is_finite() && sv.is_finite())
        .then_some((su, sv))
}

/// Interior points on a grid stepped by the surface's curvature, inside the loops and clear of
/// the boundary. `pts` is the boundary, `loops` ranges into it; `(mu, mv)` scales (u, v) to
/// about true lengths.
#[allow(clippy::too_many_arguments)]
fn seed_grid(
    dom: &Domain,
    pts: &[P2],
    loops: &[(usize, usize)],
    ubox: (f64, f64),
    vbox: (f64, f64),
    (mu, mv): (f64, f64),
    p: &TessParams,
) -> Vec<P2> {
    let (hu, hv) = steps(dom, ubox, vbox, p.chord, p.ang());
    // A step longer than the face is no step (straight to rounding: a loft along a line).
    let hu = if hu >= ubox.1 - ubox.0 {
        f64::INFINITY
    } else {
        hu
    };
    let hv = if hv >= vbox.1 - vbox.0 {
        f64::INFINITY
    } else {
        hv
    };
    let mut grid: Vec<P2> = Vec::new();
    if !(hu.is_finite() || hv.is_finite()) {
        return grid;
    }
    let nu = if hu.is_finite() {
        (((ubox.1 - ubox.0) / hu).ceil() as usize).clamp(1, 400)
    } else {
        1
    };
    let mut nv = if hv.is_finite() {
        (((vbox.1 - vbox.0) / hv).ceil() as usize).clamp(1, 400)
    } else {
        1
    };
    // Straight along v (cylinders, cones, extrusions): long triangles from one end to the other
    // are exact there and keep the rulings whole; only very long ones are cut.
    if !hv.is_finite() && hu.is_finite() {
        let du = (ubox.1 - ubox.0) / nu as f64 * mu;
        let ext = (vbox.1 - vbox.0) * mv;
        // Rows no further apart than half the face's width, or Delaunay bridges a narrow
        // strip from rail to rail across its curve.
        let width = (ubox.1 - ubox.0) * mu;
        nv = ((ext / (du * 64.0).min(0.5 * width)).ceil() as usize).clamp(1, 400);
    }
    let nu = if !hu.is_finite() && hv.is_finite() {
        let dv = (vbox.1 - vbox.0) / nv as f64 * mv;
        let ext = (ubox.1 - ubox.0) * mu;
        let width = (vbox.1 - vbox.0) * mv;
        ((ext / (dv * 8.0).min(0.5 * width)).ceil() as usize).clamp(1, 400)
    } else {
        nu
    };
    // Rows (columns) no further apart than half the face's width across them, or Delaunay
    // bridges a narrow curved strip from side to side.
    let (wu, wv) = ((ubox.1 - ubox.0) * mu, (vbox.1 - vbox.0) * mv);
    if nu > 1 {
        nv = nv.max(((wv / (0.5 * wu)).ceil() as usize).clamp(1, 400));
    }
    let nu = if nv > 1 {
        nu.max(((wu / (0.5 * wv)).ceil() as usize).clamp(1, 400))
    } else {
        nu
    };
    if nu <= 1 && nv <= 1 {
        return grid;
    }
    let du = (ubox.1 - ubox.0) / nu as f64;
    let dv = (vbox.1 - vbox.0) / nv as f64;
    let min_d = 0.4 * (du * mu).min(dv * mv);
    let segs: Vec<(P2, P2)> = loops
        .iter()
        .flat_map(|&(a, b)| {
            (a..b).map(move |i| {
                let j = if i + 1 == b { a } else { i + 1 };
                (
                    P2::new(pts[i].x * mu, pts[i].y * mv),
                    P2::new(pts[j].x * mu, pts[j].y * mv),
                )
            })
        })
        .collect();
    // The boundary's segments bucketed by grid cell (scaled space), for the distance test.
    let (cw, chh) = (du * mu, dv * mv);
    let (ox, oy) = (ubox.0 * mu, vbox.0 * mv);
    let cell = |x: f64, y: f64| -> (i64, i64) {
        (
            ((x - ox) / cw).floor() as i64,
            ((y - oy) / chh).floor() as i64,
        )
    };
    let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::default();
    let mut always: Vec<usize> = Vec::new();
    for (k, (a, b)) in segs.iter().enumerate() {
        let (c0, c1) = (
            cell(a.x.min(b.x) - min_d, a.y.min(b.y) - min_d),
            cell(a.x.max(b.x) + min_d, a.y.max(b.y) + min_d),
        );
        if (c1.0 - c0.0 + 1) * (c1.1 - c0.1 + 1) > 4096 {
            // A very long segment: in every cell of its rows would be too many; test it
            // against every point instead.
            always.push(k);
            continue;
        }
        for cx in c0.0..=c1.0 {
            for cy in c0.1..=c1.1 {
                buckets.entry((cx, cy)).or_default().push(k);
            }
        }
    }
    // Per row of the grid, the inside intervals along u (one scanline pass over the boundary
    // instead of a point-in-polygon test per point).
    for j in 1..nv {
        let v = vbox.0 + dv * j as f64;
        let mut xs: Vec<f64> = Vec::new();
        for &(a, b) in loops {
            for i in a..b {
                let k = if i + 1 == b { a } else { i + 1 };
                let (p, q) = (pts[i], pts[k]);
                if (p.y > v) != (q.y > v) {
                    xs.push(p.x + (v - p.y) * (q.x - p.x) / (q.y - p.y));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for i in 1..nu {
            let u = ubox.0 + du * i as f64;
            if xs.partition_point(|x| *x < u) % 2 == 0 {
                continue;
            }
            let qs = P2::new(u * mu, v * mv);
            let c = cell(qs.x, qs.y);
            let near = buckets.get(&c).is_some_and(|ks| {
                ks.iter()
                    .any(|&k| seg_dist2(&qs, &segs[k].0, &segs[k].1) < min_d)
            }) || always
                .iter()
                .any(|&k| seg_dist2(&qs, &segs[k].0, &segs[k].1) < min_d);
            if !near {
                grid.push(P2::new(u, v));
            }
        }
    }
    grid
}

// ---------------------------------------------------------------------------------------------
// Triangulation and refinement.

/// A vertex of a face's triangulation.
#[derive(Debug, Clone, Copy)]
struct Vx {
    /// (u, v) scaled to about true lengths (the triangulation's coordinates).
    at: SPoint<f64>,
    uv: P2,
    /// The mesh point: the edge sample on the boundary, the surface point inside.
    p: P3,
    /// The surface normal (Su × Sv, unit).
    n: V3,
    /// Whether the normal is well defined (not at a pole or apex).
    smooth: bool,
    /// Added by the triangulation's own refinement, the rest still to fill in.
    fresh: bool,
}

impl From<SPoint<f64>> for Vx {
    fn from(at: SPoint<f64>) -> Self {
        Self {
            at,
            uv: P2::origin(),
            p: P3::origin(),
            n: V3::z(),
            smooth: false,
            fresh: true,
        }
    }
}

impl HasPosition for Vx {
    type Scalar = f64;
    fn position(&self) -> SPoint<f64> {
        self.at
    }
}

type Cdt = ConstrainedDelaunayTriangulation<Vx>;

/// A triangulation vertex at `uv`; `p` the mesh point when it isn't the surface's.
fn vx(dom: &Domain, uv: P2, p: Option<P3>, (mu, mv): (f64, f64)) -> Vx {
    let (q, su, sv) = dom.d1(uv);
    let n = su.cross(&sv);
    let l = n.norm();
    // At a pole or an apex a derivative vanishes, but only to rounding: a cone's Su there is
    // 1e-15 long and may point either way, its normal the other nappe's (turned by π against its
    // neighbours', which the angle refinement then splits for ever). Against the face's mean
    // speeds, such a point has no normal of its own.
    let smooth = l > 1e-10 * su.norm() * sv.norm()
        && l > tol::TINY
        && su.norm() > tol::REL * mu
        && sv.norm() > tol::REL * mv;
    let n = if smooth { n / l } else { dom.normal(uv) };
    Vx {
        at: SPoint::new(tiny(uv.x * mu), tiny(uv.y * mv)),
        uv,
        p: p.unwrap_or(q),
        n,
        smooth,
        fresh: false,
    }
}

fn tiny(x: f64) -> f64 {
    if x.abs() < 1e-30 { 0.0 } else { x }
}

fn pair(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// Which inner faces of the triangulation are inside the face: by parity of the boundary
/// segments crossed from the hull (`flip` holds the segments that bound; a slit, crossed both
/// ways, doesn't).
fn classify(cdt: &Cdt, flip: &FastSet<(usize, usize)>) -> Vec<bool> {
    let mut state: Vec<i8> = vec![-1; cdt.num_inner_faces() + 1];
    let mut queue = VecDeque::new();
    for face in cdt.inner_faces() {
        for e in face.adjacent_edges() {
            if e.rev().face().is_outer() {
                let idx = face.fix().index();
                let s = i8::from(
                    e.is_constraint_edge()
                        && flip.contains(&pair(e.from().fix().index(), e.to().fix().index())),
                );
                if state[idx] == -1 {
                    state[idx] = s;
                    queue.push_back(face.fix());
                }
            }
        }
    }
    while let Some(fh) = queue.pop_front() {
        let face = cdt.face(fh);
        let s = state[fh.index()];
        for e in face.adjacent_edges() {
            if let Some(o) = e.rev().face().as_inner() {
                let idx = o.fix().index();
                if state[idx] == -1 {
                    let crosses = e.is_constraint_edge()
                        && flip.contains(&pair(e.from().fix().index(), e.to().fix().index()));
                    state[idx] = if crosses { 1 - s } else { s };
                    queue.push_back(o.fix());
                }
            }
        }
    }
    state.into_iter().map(|s| s == 1).collect()
}

/// Which inner faces of the triangulation are inside the face: those around whose centroid the
/// boundary segments `cons` (between `verts`) wind.
fn classify_winding(cdt: &Cdt, verts: &[Vx], cons: &[[usize; 2]]) -> Vec<bool> {
    let mut inside = vec![false; cdt.num_inner_faces() + 1];
    // The segments by the band of y they span, in buckets: a centroid's count runs over the
    // segments of its bucket (only a segment spanning its y can count), not every segment.
    let (ylo, yhi) = cons
        .iter()
        .flat_map(|c| c.map(|i| verts[i].at.y))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), y| {
            (l.min(y), h.max(y))
        });
    let nb = (cons.len() / 2).clamp(1, 1024);
    let width = (yhi - ylo) / nb as f64;
    let bucket = |y: f64| -> usize {
        if width > 0.0 {
            (((y - ylo) / width).floor().max(0.0) as usize).min(nb - 1)
        } else {
            0
        }
    };
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); nb];
    for (k, &[i, j]) in cons.iter().enumerate() {
        let (a, b) = (verts[i].at.y, verts[j].at.y);
        // (A segment with a coordinate that isn't a number counts for no point.)
        if a.is_nan() || b.is_nan() {
            continue;
        }
        for bk in buckets
            .iter_mut()
            .take(bucket(a.max(b)) + 1)
            .skip(bucket(a.min(b)))
        {
            bk.push(k);
        }
    }
    for face in cdt.inner_faces() {
        let [a, b, c] = face.positions();
        let (x, y) = ((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0);
        if !(y >= ylo && y <= yhi) {
            continue;
        }
        let mut w = 0i32;
        for &k in &buckets[bucket(y)] {
            let [i, j] = cons[k];
            let (p, q) = (verts[i].at, verts[j].at);
            let side = (q.x - p.x) * (y - p.y) - (x - p.x) * (q.y - p.y);
            if p.y <= y && q.y > y && side > 0.0 {
                w += 1;
            } else if p.y > y && q.y <= y && side < 0.0 {
                w -= 1;
            }
        }
        inside[face.fix().index()] = w != 0;
    }
    inside
}

/// Where to split a triangle that misses the chord, angle or edge length, or None when it meets
/// them all (the aspect ratio is the triangulation's own refinement's, see [`mesh_face_with`]). `v` its
/// corners, `cons[i]` whether side i (corner i to i + 1) is on the boundary (never split: it
/// is the neighbouring face's too). Interior sides are split at their middle (both triangles on
/// them are inside), else the centroid is added.
fn split_point(
    dom: &Domain,
    v: &[Vx; 3],
    cons: [bool; 3],
    p: &TessParams,
    min_len: f64,
) -> Option<P2> {
    let len = |i: usize| (v[(i + 1) % 3].p - v[i].p).norm();
    let l = [len(0), len(1), len(2)];
    let longest = (0..3).max_by(|&a, &b| l[a].total_cmp(&l[b])).unwrap_or(0);
    if l[longest] < min_len {
        return None;
    }
    // A sliver in (u, v) (three points along a boundary): splitting it only makes more; the
    // triangles beside its long side split that side if it needs it.
    let (a, b, c) = (v[0].at, v[1].at, v[2].at);
    let area2 = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let side2 = [(a, b), (b, c), (c, a)]
        .iter()
        .map(|(x, y)| (x.x - y.x).powi(2) + (x.y - y.y).powi(2))
        .fold(0.0, f64::max);
    if !(area2.abs() > tol::REL * side2) {
        return None;
    }
    let mid = |i: usize| P2::from((v[i].uv.coords + v[(i + 1) % 3].uv.coords) * 0.5);
    let interior_longest = (0..3)
        .filter(|&i| !cons[i])
        .max_by(|&a, &b| l[a].total_cmp(&l[b]));
    if p.max_edge > 0.0 && l[longest] > p.max_edge && !cons[longest] {
        return Some(mid(longest));
    }
    if matches!(dom.s, Surface::Plane { .. }) {
        return None;
    }
    // Chord: how far the triangle is from the surface at its centroid and its interior sides'
    // middles, along the surface normal there (a B-spline's (u, v) middle is not the 3D middle:
    // the offset along the surface is no deviation).
    let off = |q: P2, t: P3| {
        let (s, su, sv) = dom.d1(q);
        let n = su.cross(&sv);
        let l = n.norm();
        if l > tol::TINY {
            (t - s).dot(&n).abs() / l
        } else {
            (t - s).norm()
        }
    };
    let centroid = P2::from((v[0].uv.coords + v[1].uv.coords + v[2].uv.coords) / 3.0);
    let mut worst = (
        off(
            centroid,
            P3::from((v[0].p.coords + v[1].p.coords + v[2].p.coords) / 3.0),
        ),
        centroid,
    );
    for i in (0..3).filter(|&i| !cons[i]) {
        let q = mid(i);
        let d = off(q, nalgebra::center(&v[i].p, &v[(i + 1) % 3].p));
        if d > worst.0 {
            worst = (d, q);
        }
    }
    if worst.0 > p.chord {
        return Some(worst.1);
    }
    // Angle between the corners' normals, on triangles over a tenth of the chord (the normals
    // swing round without end by a pole or an apex, where the triangles are tiny anyway).
    if v.iter().all(|x| x.smooth) && l[longest] > 0.1 * p.chord {
        let turn = (0..3)
            .map(|i| v[i].n.angle(&v[(i + 1) % 3].n))
            .fold(0.0, f64::max);
        if turn > p.ang() {
            return Some(interior_longest.map(mid).unwrap_or(centroid));
        }
    }
    None
}

/// Triangulates one face (see [`mesh_face_with`]); a chord or angle that isn't positive takes
/// the default's.
pub fn mesh_face(
    body: &Body,
    f: &Face,
    es: &[(Vec<f64>, Vec<P3>)],
    defl: f64,
    ang: f64,
) -> FaceMesh {
    mesh_face_with(body, f, es, &TessParams::lenient(defl, ang))
}

/// Triangulates one face. `es` holds each edge's samples ([`edge_samples_with`] with the same
/// controls); the face's boundary vertices are exactly those samples, so faces meshed from the
/// same samples join without gaps.
pub fn mesh_face_with(
    body: &Body,
    f: &Face,
    es: &[(Vec<f64>, Vec<P3>)],
    p: &TessParams,
) -> FaceMesh {
    let t_start = web_time::Instant::now();
    let prof = std::env::var("CADRS_TESS_PROFILE").is_ok();
    // The face's size, for its tolerances.
    let mut bb = (
        P3::from(V3::repeat(f64::INFINITY)),
        P3::from(V3::repeat(f64::NEG_INFINITY)),
    );
    for c in f.loops.iter().flatten() {
        for q in &es[c.edge].1 {
            bb = (bb.0.inf(q), bb.1.sup(q));
        }
    }
    let extent = (bb.1 - bb.0).norm();
    if !extent.is_finite() {
        return FaceMesh::default();
    }
    let tol = (tol::REL_POINT * extent).max(1e-10);
    let dom = Domain::new(&f.surf, tol);
    let mut bloops = boundary(body, f, es, &dom, p, tol);
    let whole = surface_loops(body, f, &bloops, &dom);
    let covers =
        whole.iter().any(|&w| w) || (f.loops.is_empty() && dom.per.iter().any(Option::is_some));
    if let Some(rect) = covers.then(|| whole_boundary(&dom, p, tol)).flatten() {
        // The holes into the rectangle's copy of the periods.
        let mut kept: Vec<Vec<BPt>> = bloops
            .into_iter()
            .zip(whole)
            .filter(|(l, w)| !w && l.len() >= 3)
            .map(|(l, _)| l)
            .collect();
        for l in &mut kept {
            for d in 0..2 {
                if let Some((a, w)) = dom.per[d] {
                    let (lo, hi) = l
                        .iter()
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| {
                            (lo.min(q.uv[d]), hi.max(q.uv[d]))
                        });
                    let k = ((0.5 * (lo + hi) - a) / w).floor();
                    l.iter_mut().for_each(|q| q.uv[d] -= k * w);
                }
            }
        }
        kept.insert(0, rect);
        bloops = kept;
    }
    let mut pts: Vec<BPt> = Vec::new();
    let mut loops: Vec<(usize, usize)> = Vec::new();
    for l in bloops {
        let start = pts.len();
        pts.extend(l);
        loops.push((start, pts.len()));
    }
    let nb = pts.len();
    if nb < 3 {
        return FaceMesh::default();
    }
    let (ubox, vbox) = pts.iter().fold(
        (
            (f64::INFINITY, f64::NEG_INFINITY),
            (f64::INFINITY, f64::NEG_INFINITY),
        ),
        |(u, v), q| {
            (
                (u.0.min(q.uv.x), u.1.max(q.uv.x)),
                (v.0.min(q.uv.y), v.1.max(q.uv.y)),
            )
        },
    );
    // B-spline parameters carry no length: scale them by the surface's mean speeds over the
    // face's box, so the grid's steps and the triangulation work in about true lengths (a long
    // straight-along-v fillet strip with v in [0, 1] would otherwise get no interior rows).
    // Likewise an extrusion's (its curve's parameter and its direction's length) and a surface
    // of revolution's along its meridian: unscaled, a 0.02 mm extrusion of a spline over a
    // parameter range of 0.0016 took a 14 000-point grid.
    let scale = f.surf.metric();
    let scale = if matches!(
        f.surf,
        Surface::Nurbs(_) | Surface::Extrusion { .. } | Surface::Revolution { .. }
    ) {
        mean_speeds(&dom, ubox, vbox).unwrap_or(scale)
    } else {
        scale
    };
    let pts_uv: Vec<P2> = pts.iter().map(|b| b.uv).collect();
    let grid = seed_grid(&dom, &pts_uv, &loops, ubox, vbox, scale, p);
    if prof {
        eprintln!(
            "tess face: boundary {nb}, grid {} in {:?}, scale {scale:?}, box {ubox:?} {vbox:?}",
            grid.len(),
            t_start.elapsed()
        );
    }
    // Constrained Delaunay in scaled uv, bulk loaded (boundary points at the same (u, v) merged
    // first, so the triangulation's vertices are ours in order).
    let mut verts: Vec<Vx> = Vec::with_capacity(nb + grid.len());
    let mut seen: HashMap<(u64, u64), usize> =
        HashMap::with_capacity_and_hasher(nb, Default::default());
    let mut handle: Vec<usize> = Vec::with_capacity(nb);
    for b in &pts {
        let v = vx(&dom, b.uv, Some(b.p), scale);
        if !(v.at.x.is_finite() && v.at.y.is_finite()) {
            return FaceMesh::default();
        }
        let k = *seen
            .entry((v.at.x.to_bits(), v.at.y.to_bits()))
            .or_insert_with(|| {
                verts.push(v);
                verts.len() - 1
            });
        handle.push(k);
    }
    let n_boundary = verts.len();
    // The grid keeps clear of the boundary, so its points are new.
    verts.extend(
        grid.iter()
            .map(|&q| vx(&dom, q, None, scale))
            .filter(|v| v.at.x.is_finite() && v.at.y.is_finite()),
    );
    let mut cons: Vec<[usize; 2]> = Vec::new();
    let mut count: HashMap<(usize, usize), usize> = HashMap::default();
    for &(a, b) in &loops {
        for i in a..b {
            let j = if i + 1 == b { a } else { i + 1 };
            let (x, y) = (handle[i], handle[j]);
            if x != y {
                cons.push([x, y]);
                *count.entry(pair(x, y)).or_insert(0) += 1;
            }
        }
    }
    let flip: FastSet<(usize, usize)> = count
        .into_iter()
        .filter(|&(_, n)| n % 2 == 1)
        .map(|(k, _)| k)
        .collect();
    let setup = Setup {
        dom: &dom,
        f,
        verts,
        cons,
        flip,
        n_boundary,
        scale,
        extent,
        tol,
        prof: prof.then_some(t_start),
    };
    let m = triangulate(&setup, p, true);
    if prof {
        eprintln!(
            "tess face: {} triangles at {:?}",
            m.indices.len(),
            t_start.elapsed()
        );
    }
    let bad = check_faces(std::slice::from_ref(&m)).bad;
    if bad == 0 {
        return m;
    }
    // A fold the refinement made (an interior point joined to a pole at two places, say): the
    // unrefined triangulation instead when it has fewer.
    let plain = triangulate(&setup, p, false);
    if check_faces(std::slice::from_ref(&plain)).bad < bad {
        plain
    } else {
        m
    }
}

/// A face's triangulation input: its domain, the triangulation's starting vertices (the first
/// `n_boundary` on the boundary) and boundary segments (`flip`: those that bound, see
/// [`classify`]).
struct Setup<'a> {
    dom: &'a Domain<'a>,
    f: &'a Face,
    verts: Vec<Vx>,
    cons: Vec<[usize; 2]>,
    flip: FastSet<(usize, usize)>,
    n_boundary: usize,
    scale: (f64, f64),
    extent: f64,
    tol: f64,
    prof: Option<web_time::Instant>,
}

/// The constrained Delaunay triangulation of a face, refined to the controls when `refine`.
fn triangulate(s: &Setup, p: &TessParams, refine: bool) -> FaceMesh {
    let Setup {
        dom,
        f,
        ref verts,
        ref cons,
        ref flip,
        n_boundary,
        scale,
        extent,
        tol,
        ..
    } = *s;
    let mut conflicts: Vec<[usize; 2]> = Vec::new();
    let Ok(mut cdt): std::result::Result<Cdt, _> =
        ConstrainedDelaunayTriangulation::try_bulk_load_cdt(verts.clone(), cons.clone(), |c| {
            conflicts.push(c)
        })
    else {
        return FaceMesh::default();
    };
    // The refinement's budget: as many points again as the seeding gave, at least 10 000 and at
    // most 100 000 (a chord far below the face's size would ask for millions; the seeding is
    // already within a few times of the chord, so the bound is only reached there).
    // Inside by parity across the boundary, or, when boundary segments cross (a p-curve astray)
    // and one is missing from the triangulation so parity would leak, by the boundary's winding
    // number at each triangle's centroid. Likewise when a segment runs through another boundary
    // vertex (a chamfer as wide as the wall it cuts leaves the wall's end face pinched to no
    // width along it, two segments along one line, one running on past the other's ends): the
    // triangulation splits it there, and parity, counted by the whole segments, would miss its
    // pieces.
    let split: Vec<[usize; 2]> = cons
        .iter()
        .filter(|&&[a, b]| {
            cdt.get_edge_from_neighbors(
                FixedVertexHandle::from_index(a),
                FixedVertexHandle::from_index(b),
            )
            .is_none()
        })
        .copied()
        .collect();
    let sides = |cdt: &Cdt| {
        if conflicts.is_empty() && split.is_empty() {
            classify(cdt, flip)
        } else {
            classify_winding(cdt, verts, cons)
        }
    };
    if s.prof.is_some() && !conflicts.is_empty() {
        eprintln!(
            "tess face: boundary segments crossing: {:?}",
            conflicts
                .iter()
                .map(|c| (c, verts[c[0]].uv, verts[c[1]].uv))
                .collect::<Vec<_>>()
        );
    }
    let max_verts = cdt.num_vertices() + cdt.num_vertices().clamp(10_000, 100_000);
    // Aspect ratio: Ruppert/Chew refinement to the matching smallest angle (a needle's aspect
    // ratio is about 1 / sin of its smallest angle), in the scaled (u, v), keeping the boundary
    // segments whole and leaving triangles under a chord's size alone.
    if refine && p.max_aspect > 0.0 {
        let angle = (1.0 / p.max_aspect).asin().to_degrees().min(30.0);
        let n0 = cdt.num_vertices();
        let k0 = cdt.num_constraints();
        let size = p.chord.max(1e-3 * extent);
        cdt.refine(
            RefinementParameters::new()
                .exclude_outer_faces(true)
                .keep_constraint_edges()
                .with_angle_limit(AngleLimit::from_deg(angle))
                .with_min_required_area(0.5 * size * size)
                .with_max_additional_vertices(max_verts.saturating_sub(n0)),
        );
        if cdt.num_constraints() != k0 {
            // A boundary segment was split after all: start again without it.
            match ConstrainedDelaunayTriangulation::try_bulk_load_cdt(
                verts.clone(),
                cons.clone(),
                |_| {},
            ) {
                Ok(c) => cdt = c,
                Err(_) => return FaceMesh::default(),
            }
        } else {
            for i in n0..cdt.num_vertices() {
                let h = FixedVertexHandle::from_index(i);
                let d = *cdt.vertex(h).data();
                if d.fresh {
                    *cdt.vertex_data_mut(h) = vx(
                        dom,
                        P2::new(d.at.x / scale.0, d.at.y / scale.1),
                        None,
                        scale,
                    );
                }
            }
        }
    }
    // Refinement: split the triangles that miss a control, a round at a time, until none do.
    // Only interior points are added (inside triangles or on interior sides).
    let min_len = (1e-3 * p.chord).min(1e-4 * extent).max(tol);
    let mut ok: FastSet<[usize; 3]> = FastSet::default();
    // The same by the triangle's index in the triangulation, where it was found good: most
    // triangles keep their index from round to round, so most are told good by one look-up in
    // this array instead of the set (a triangle at that index now with other corners isn't).
    let mut ok_at: Vec<[usize; 3]> = Vec::new();
    for _ in 0..if refine { 64 } else { 0 } {
        if cdt.num_vertices() >= max_verts {
            break;
        }
        let inside = sides(&cdt);
        // The triangles still to check (corners, which sides are boundary), checked in parallel.
        ok_at.resize(cdt.num_inner_faces() + 1, [usize::MAX; 3]);
        let todo: Vec<Unchecked> = cdt
            .inner_faces()
            .filter(|face| inside[face.fix().index()])
            .filter_map(|face| {
                let vs = face.vertices();
                let ids = vs.map(|v| v.fix().index());
                let mut key = ids;
                key.sort_unstable();
                let fi = face.fix().index();
                if ok_at[fi] == key {
                    return None;
                }
                if ok.contains(&key) {
                    ok_at[fi] = key;
                    return None;
                }
                let mut cons = [false; 3];
                for e in face.adjacent_edges() {
                    if e.is_constraint_edge() {
                        let (x, y) = (e.from().fix().index(), e.to().fix().index());
                        if let Some(i) =
                            (0..3).find(|&i| pair(ids[i], ids[(i + 1) % 3]) == pair(x, y))
                        {
                            cons[i] = true;
                        }
                    }
                }
                Some((key, fi, vs.map(|v| *v.data()), cons))
            })
            .collect();
        let splits: Vec<Option<P2>> = todo
            .par_iter()
            .map(|(_, _, v, cons)| split_point(dom, v, *cons, p, min_len))
            .collect();
        let mut proposed: FastSet<(u64, u64)> = FastSet::default();
        let mut at: Vec<P2> = Vec::new();
        for ((key, fi, _, _), q) in todo.iter().zip(splits) {
            match q {
                Some(q) => {
                    if proposed.insert((q.x.to_bits(), q.y.to_bits())) {
                        at.push(q);
                    }
                }
                None => {
                    ok.insert(*key);
                    ok_at[*fi] = *key;
                }
            }
        }
        let mut adds: Vec<Vx> = at.par_iter().map(|&q| vx(dom, q, None, scale)).collect();
        if let Some(t) = s.prof {
            eprintln!(
                "tess face: refinement round at {} vertices, {} to add, {:?}",
                cdt.num_vertices(),
                adds.len(),
                t.elapsed()
            );
        }
        if adds.is_empty() {
            break;
        }
        let snap = tol::REL * extent;
        // In rows across the face, so each insertion's walk starts next to it.
        let row = (extent * 1e-2).max(tol::DEGENERATE);
        adds.sort_by(|a, b| {
            let (ra, rb) = ((a.at.y / row).floor(), (b.at.y / row).floor());
            ra.total_cmp(&rb).then(if ra as i64 % 2 == 0 {
                a.at.x.total_cmp(&b.at.x)
            } else {
                b.at.x.total_cmp(&a.at.x)
            })
        });
        for v in adds {
            if !(v.at.x.is_finite() && v.at.y.is_finite()) || cdt.num_vertices() >= max_verts {
                continue;
            }
            // Never onto a vertex or a boundary segment (that would split the shared boundary).
            // Nor within rounding of one: the point would sit on the boundary in space.
            let clear = |f: spade::handles::FixedFaceHandle<spade::handles::InnerTag>| {
                cdt.face(f).adjacent_edges().iter().all(|e| {
                    let [a, b] = e.positions();
                    !e.is_constraint_edge()
                        || seg_dist2(
                            &P2::new(v.at.x, v.at.y),
                            &P2::new(a.x, a.y),
                            &P2::new(b.x, b.y),
                        ) > snap
                })
            };
            let free = match cdt.locate(v.at) {
                PositionInTriangulation::OnFace(f) => clear(f),
                PositionInTriangulation::OnEdge(e) => {
                    let e = cdt.directed_edge(e);
                    !e.is_constraint_edge()
                        && [e.face(), e.rev().face()]
                            .iter()
                            .all(|f| f.as_inner().is_none_or(|f| clear(f.fix())))
                }
                _ => false,
            };
            if free {
                let _ = cdt.insert(v);
            }
        }
    }
    // Out: boundary vertices at one point in space (along a pole) become one, and the triangles
    // that collapse with them go.
    let inside = sides(&cdt);
    let sign = if f.sense { 1.0 } else { -1.0 };
    let mut out = FaceMesh::default();
    let mut map: Vec<u32> = vec![u32::MAX; cdt.num_vertices()];
    let mut weld: HashMap<[u64; 3], u32> = HashMap::default();
    let mut index = |hi: usize| {
        if map[hi] == u32::MAX {
            let d = *cdt.vertex(FixedVertexHandle::from_index(hi)).data();
            let mut push = || {
                let (u, v) = dom.wrap(d.uv.x, d.uv.y);
                out.positions.push(d.p);
                out.normals.push(d.n * sign);
                out.uvs.push(P2::new(u, v));
                (out.positions.len() - 1) as u32
            };
            map[hi] = if hi < n_boundary {
                *weld
                    .entry([d.p.x.to_bits(), d.p.y.to_bits(), d.p.z.to_bits()])
                    .or_insert_with(push)
            } else {
                push()
            };
        }
        map[hi]
    };
    let distinct = |t: &[u32; 3]| t[0] != t[1] && t[1] != t[2] && t[2] != t[0];
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for face in cdt.inner_faces() {
        if !inside[face.fix().index()] {
            continue;
        }
        let mut tri = face.vertices().map(|v| index(v.fix().index()));
        if !distinct(&tri) {
            continue;
        }
        if !f.sense {
            tri.swap(1, 2);
        }
        tris.push(tri);
    }
    drop_pillows(&mut tris);
    // A segment split where another boundary vertex lies on it is still one side of the
    // neighbouring face's triangles (that face's boundary has no vertex there): a fan of
    // triangles of no area from its start over its pieces joins that side to this face's
    // triangles along the pieces, and to the faces along the line on the pieces no triangle of
    // this face is on (a pinch). Boundary sides run the loop's way in the triangles out, so the
    // fan runs the segment's way along it and back along the pieces.
    for &[a, b] in &split {
        let Some(along) = chain(&cdt, a, b) else {
            continue;
        };
        for w in along[1..].windows(2) {
            let tri = [index(a), index(w[1]), index(w[0])];
            if distinct(&tri) {
                tris.push(tri);
            }
        }
    }
    out.indices = tris;
    out
}

/// The vertices along boundary segment `a`–`b` in the triangulation, `a` first and `b` last:
/// its constraint edges through the boundary vertices on it, where it was split. None when it
/// has no such path (a segment lost to a crossing one).
fn chain(cdt: &Cdt, a: usize, b: usize) -> Option<Vec<usize>> {
    let at = |i: usize| cdt.vertex(FixedVertexHandle::from_index(i)).position();
    let (pa, pb) = (at(a), at(b));
    let d = (pb.x - pa.x, pb.y - pa.y);
    let len2 = d.0 * d.0 + d.1 * d.1;
    if !(len2 > 0.0) {
        return None;
    }
    let mut path = vec![a];
    // How far along the segment the path has come (0 at `a`, 1 at `b`).
    let mut reach = 0.0;
    while path.last() != Some(&b) {
        if path.len() > cdt.num_vertices() {
            return None;
        }
        let cur = path[path.len() - 1];
        let (s, next) = cdt
            .vertex(FixedVertexHandle::from_index(cur))
            .out_edges()
            .filter(|e| e.is_constraint_edge())
            .filter_map(|e| {
                let t = e.to().fix().index();
                let q = at(t);
                let (x, y) = (q.x - pa.x, q.y - pa.y);
                let s = (x * d.0 + y * d.1) / len2;
                let off = (x * d.1 - y * d.0).abs() / len2;
                (s > reach && s <= 1.0 + tol::REL && off <= tol::REL).then_some((s, t))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))?;
        reach = s;
        path.push(next);
    }
    Some(path)
}

/// Removes pairs of triangles on the same three vertices the opposite ways round. Where an
/// interior point by a pole is joined to two of the pole's (u, v) copies with a point between,
/// the welded pole leaves such a pair (a fold of no area pinching the mesh there).
fn drop_pillows(tris: &mut Vec<[u32; 3]>) {
    let mut by: HashMap<[u32; 3], Vec<usize>> = HashMap::default();
    for (k, t) in tris.iter().enumerate() {
        let mut key = *t;
        key.sort_unstable();
        by.entry(key).or_default().push(k);
    }
    // A triangle's orientation: whether its corners in order are an even rotation of sorted.
    let even = |t: &[u32; 3]| (t[0] < t[1]) as u8 + (t[1] < t[2]) as u8 + (t[2] < t[0]) as u8 == 2;
    let mut uses: HashMap<(u32, u32), usize> = HashMap::default();
    for t in tris.iter() {
        for i in 0..3 {
            *uses
                .entry((t[i].min(t[(i + 1) % 3]), t[i].max(t[(i + 1) % 3])))
                .or_insert(0) += 1;
        }
    }
    let mut gone = vec![false; tris.len()];
    for (key, ks) in &by {
        if let [a, b] = ks[..] {
            // Only where that opens nothing: each side left with no triangle or two.
            let sides = [(key[0], key[1]), (key[1], key[2]), (key[0], key[2])];
            if even(&tris[a]) != even(&tris[b]) && sides.iter().all(|s| uses[s] != 3) {
                gone[a] = true;
                gone[b] = true;
            }
        }
    }
    if gone.iter().any(|&g| g) {
        let mut k = 0;
        tris.retain(|_| {
            k += 1;
            !gone[k - 1]
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Bodies.

/// The whole body's mesh, face by face (see [`mesh`]); a chord or angle that isn't positive
/// takes the default's.
pub fn mesh_body(body: &Body, defl: f64, ang: f64) -> (Vec<FaceMesh>, EdgeSamples) {
    mesh_all(body, &TessParams::lenient(defl, ang))
}

/// The whole body's mesh, face by face, with each edge's samples (the face meshes' shared
/// boundary points).
///
/// # Errors
/// [`KernelError::InvalidInput`] for invalid controls ([`TessParams::validate`]).
pub fn mesh(body: &Body, p: &TessParams) -> Result<(Vec<FaceMesh>, EdgeSamples)> {
    p.validate()?;
    Ok(mesh_all(body, p))
}

/// The boundary points (edge samples) up to which a plane is meshed without [`MeshCache`].
const PLANE_UNKEPT: usize = 16;

fn mesh_all(body: &Body, p: &TessParams) -> (Vec<FaceMesh>, EdgeSamples) {
    let es = edge_samples_with(body, p);
    let faces: Vec<FaceMesh> = body
        .faces
        .par_iter()
        .map(|f| {
            // (A plane with few boundary points meshes in about the time its key takes: a box's
            // side, mostly new. One with many is kept too: a box's face round a few hundred
            // holes' rounded edges took 9 ms each time a feature elsewhere changed the body.)
            let few = || {
                f.loops
                    .iter()
                    .flatten()
                    .map(|c| es[c.edge].1.len())
                    .sum::<usize>()
                    <= PLANE_UNKEPT
            };
            if matches!(f.surf, Surface::Plane { .. }) && few() {
                return mesh_face_with(body, f, &es, p);
            }
            let key = face_mesh_key(body, f, &es, p);
            let kept = mesh_cache().get(key);
            if let Some(m) = kept {
                return (*m).clone();
            }
            let m = mesh_face_with(body, f, &es, p);
            mesh_cache().insert(key, std::sync::Arc::new(m.clone()));
            m
        })
        .collect();
    (faces, es)
}

/// What a face's mesh ([`mesh_face_with`]) is made from, hashed to 128 bits
/// ([`super::fingerprint`]): the controls, the face's surface and sense, and each coedge's
/// p-curve with its edge's range, tolerance, samples and whether it is a point (and which
/// coedges share an edge: a seam's two sides).
fn face_mesh_key(body: &Body, f: &Face, es: &[(Vec<f64>, Vec<P3>)], p: &TessParams) -> u128 {
    use std::hash::Hasher;
    let mut h = super::fingerprint::Key128::new();
    h.f64s(&[p.chord, p.angle, p.max_edge, p.max_aspect]);
    h.surface(&f.surf);
    h.write_u8(f.sense as u8);
    let mut first: HashMap<usize, usize> = HashMap::default();
    for l in &f.loops {
        h.write_usize(l.len());
        for c in l {
            let e = &body.edges[c.edge];
            let n = first.len();
            h.write_usize(*first.entry(c.edge).or_insert(n));
            h.write_u8(c.fwd as u8 | (e.degenerate as u8) << 1);
            h.f64s(&e.t);
            h.f64s(&[e.tol]);
            h.curve(&c.pc);
            let (ts, ps) = &es[c.edge];
            h.write_usize(ts.len());
            h.f64s(ts);
            for q in ps {
                h.f64s(q.coords.as_slice());
            }
        }
    }
    h.key()
}

/// The faces' meshes last made, by [`face_mesh_key`]: a body a feature changes keeps most of
/// its faces as they were, and each body a rebuild makes is meshed whole for display. Two
/// generations of at most [`Self::KEPT`] bytes each, a hit in the older moved to the newer.
#[derive(Default)]
struct MeshCache {
    new: HashMap<u128, std::sync::Arc<FaceMesh>>,
    new_bytes: usize,
    old: HashMap<u128, std::sync::Arc<FaceMesh>>,
}

impl MeshCache {
    const KEPT: usize = 32 << 20;

    fn get(&mut self, k: u128) -> Option<std::sync::Arc<FaceMesh>> {
        if let Some(m) = self.new.get(&k) {
            return Some(m.clone());
        }
        let m = self.old.remove(&k)?;
        self.insert(k, m.clone());
        Some(m)
    }

    fn insert(&mut self, k: u128, m: std::sync::Arc<FaceMesh>) {
        let bytes = 64
            + m.positions.len() * size_of::<P3>()
            + m.normals.len() * size_of::<V3>()
            + m.uvs.len() * size_of::<P2>()
            + m.indices.len() * size_of::<[u32; 3]>();
        if self.new_bytes + bytes > Self::KEPT {
            self.old = std::mem::take(&mut self.new);
            self.new_bytes = 0;
        }
        self.new_bytes += bytes;
        self.new.insert(k, m);
    }
}

fn mesh_cache() -> std::sync::MutexGuard<'static, MeshCache> {
    static CACHE: std::sync::LazyLock<std::sync::Mutex<MeshCache>> =
        std::sync::LazyLock::new(Default::default);
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

impl NativeKernel {
    /// [`crate::Kernel::tessellate`] with all the mesher's controls.
    ///
    /// # Errors
    /// [`KernelError::UnknownBody`], or [`KernelError::InvalidInput`] for invalid controls.
    pub fn tessellate_with(&self, body: BodyId, p: &TessParams) -> Result<TriMesh> {
        let b = self.get(body)?;
        let (faces, es) = mesh(b, p)?;
        let mut m = TriMesh::default();
        for (fi, fm) in faces.into_iter().enumerate() {
            let base = m.positions.len() as u32;
            m.positions.extend(fm.positions);
            m.normals.extend(fm.normals);
            for t in fm.indices {
                m.indices.push([t[0] + base, t[1] + base, t[2] + base]);
                m.triangle_faces.push(FaceId(fi as u64));
            }
        }
        for (ei, (_, pts)) in es.into_iter().enumerate() {
            if b.edges[ei].degenerate {
                continue;
            }
            m.edges.push((EdgeId(ei as u64), pts));
        }
        if !b.points.is_empty() {
            let ids = vertex_ids(b);
            m.points = b
                .points
                .iter()
                .map(|&v| (VertexId(ids[v].unwrap_or(0) as u64), b.verts[v].p))
                .collect();
        }
        Ok(m)
    }
}

// ---------------------------------------------------------------------------------------------
// Checks.

/// How a mesh's triangles join, its vertices welded where they are at exactly the same point
/// (the mesher shares boundary points bit for bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeshCheck {
    /// Distinct sides (unordered vertex pairs).
    pub edges: usize,
    /// Sides with one triangle (a hole, or a sheet's border).
    pub open: usize,
    /// Sides with more than two triangles, or two the same way round.
    pub bad: usize,
}

impl MeshCheck {
    /// Every side shared by exactly two triangles, once each way: a closed, consistently
    /// oriented surface.
    pub fn is_watertight(&self) -> bool {
        self.open == 0 && self.bad == 0
    }
}

/// [`MeshCheck`] of a body's face meshes.
pub fn check_faces(meshes: &[FaceMesh]) -> MeshCheck {
    check_triangles(
        meshes
            .iter()
            .flat_map(|m| m.indices.iter().map(|t| t.map(|i| m.positions[i as usize]))),
    )
}

/// [`MeshCheck`] of a [`TriMesh`].
pub fn check_tri_mesh(m: &TriMesh) -> MeshCheck {
    check_triangles(m.indices.iter().map(|t| t.map(|i| m.positions[i as usize])))
}

/// [`MeshCheck`] of triangles given by their corners.
pub fn check_triangles(tris: impl IntoIterator<Item = [P3; 3]>) -> MeshCheck {
    let mut ids: HashMap<[u64; 3], usize> = HashMap::default();
    // Per side (lower id, higher id): uses along it and against it.
    let mut sides: HashMap<(usize, usize), (u32, u32)> = HashMap::default();
    for t in tris {
        let v = t.map(|p| {
            let n = ids.len();
            *ids.entry([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()])
                .or_insert(n)
        });
        for i in 0..3 {
            let (a, b) = (v[i], v[(i + 1) % 3]);
            if a == b {
                continue;
            }
            let s = sides.entry(pair(a, b)).or_insert((0, 0));
            if a < b {
                s.0 += 1;
            } else {
                s.1 += 1;
            }
        }
    }
    let mut c = MeshCheck {
        edges: sides.len(),
        ..MeshCheck::default()
    };
    for &(f, b) in sides.values() {
        match (f, b) {
            (1, 1) => {}
            (1, 0) | (0, 1) => c.open += 1,
            _ => c.bad += 1,
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (W12): the closed B-spline cones of SMLib's `box_cone_grid.smp`, read as cones,
    /// put their apex row a rounding error past the apex, where Su × Sv points the other nappe's
    /// way. Taken as smooth, those normals turned π against their neighbours' and the angle
    /// refinement split the triangles by the apex for ever: 20 000 triangles at any chord and
    /// 0.3 to 0.7 s a face. A point whose derivative vanishes has no normal of its own.
    #[test]
    fn cone_apex_rows_dont_refine_for_ever() {
        let p = format!(
            "{}/tests/data/smlib/booleans/box_cone_grid.smp",
            env!("CARGO_MANIFEST_DIR")
        );
        let part = super::super::smb::read_part(&std::fs::read(p).unwrap()).unwrap();
        let mut cones = 0;
        for b in &part.breps {
            let es = edge_samples(&b.body, 0.1, 0.5);
            for f in b
                .body
                .faces
                .iter()
                .filter(|f| matches!(f.surf, Surface::Cone { .. }))
            {
                cones += 1;
                let m = mesh_face(&b.body, f, &es, 0.1, 0.5);
                assert!(
                    !m.indices.is_empty() && m.indices.len() < 500,
                    "{} triangles on a cone of radius 10 at most",
                    m.indices.len()
                );
            }
        }
        assert!(cones > 0);
    }

    /// A hemisphere bounded by a meridian circle through both poles, closed by its disc: the
    /// half of the sphere left of the loop in (u, v) (y ≤ 0), or the other half (`far`: the loop
    /// turned round). The meridian is one closed edge whose p-curve steps half a turn at each
    /// pole, as fitted meridians of STEP files come (trj3/4/5_s1's feet), and a hair more: the
    /// nearest copy of the period is then the other way round the pole.
    fn hemisphere(r: f64, far: bool) -> Body {
        use super::super::curve::Curve;
        use super::super::nurbs::{NurbsCurve, hom};
        use super::super::topo::BodyBuilder;
        use std::f64::consts::PI;
        let h = PI / 2.0;
        let circle = Curve::Circle {
            c: P3::origin(),
            x: V3::x(),
            y: V3::z(),
            r,
        };
        // The meridian's leg at u = 0 is on the copy of the period that makes the loop run
        // counter-clockwise in (u, v) about the material (u in (π, 2π) for the near half).
        let u0 = if far { -1e-7 } else { 2.0 * PI + 1e-7 };
        let mut bb = BodyBuilder::new();
        let v = bb.vertex(circle.point(0.0));
        let e = bb.edge(circle, 0.0, 4.0 * h, v, v);
        let cps = [(u0, 0.0), (u0, h), (PI, h), (PI, -h), (u0, -h), (u0, 0.0)]
            .map(|(u, v)| hom(&P3::new(u, v, 0.0), 1.0));
        let pc = Curve::nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, h, h, 3.0 * h, 3.0 * h, 4.0 * h, 4.0 * h],
                cps.to_vec(),
            )
            .unwrap(),
        );
        // The disc's plane: its normal out of the solid, its p-curve the circle in its (u, v).
        let (n, y) = if far {
            (-V3::y(), 1.0)
        } else {
            (V3::y(), -1.0)
        };
        let disc = Surface::Plane {
            f: Frame::from_xz(P3::origin(), V3::x(), n),
        };
        let disc_pc = Curve::Circle {
            c: P3::origin(),
            x: V3::x(),
            y: V3::new(0.0, y, 0.0),
            r,
        };
        let f0 = bb.face(
            Surface::Sphere {
                f: Frame::world(),
                r,
            },
            true,
            vec![vec![Coedge {
                edge: e,
                fwd: !far,
                pc,
            }]],
        );
        let f1 = bb.face(
            disc,
            true,
            vec![vec![Coedge {
                edge: e,
                fwd: far,
                pc: disc_pc,
            }]],
        );
        let sh = bb.shell(vec![f0, f1], true);
        bb.solid(vec![sh]);
        bb.build().unwrap()
    }

    /// A tiny extrusion (a 0.02 mm spline over a parameter range of 0.0016, swept 0.01 along a
    /// direction of length 0.01) meshes in a few hundred triangles at a 0.05 chord: its (u, v)
    /// are scaled by the surface's speeds, as a B-spline's are (unscaled, a 14 000-point grid
    /// and 28 000 triangles: Raspberry Pi Pico 2's pins, 300 000 a body).
    #[test]
    fn tiny_extrusion_meshes_coarsely() {
        use super::super::curve::Curve;
        use super::super::nurbs::{NurbsCurve, hom};
        use super::super::topo::BodyBuilder;
        let (a, b) = (-0.0016, 0.0);
        let pts = [
            (6.0823, -26.3369),
            (6.0841, -26.3348),
            (6.0876, -26.3307),
            (6.0931, -26.3301),
            (6.0958, -26.3298),
        ];
        let cps: Vec<_> = pts
            .iter()
            .map(|&(x, y)| hom(&P3::new(x, y, 1.95), 1.0))
            .collect();
        let c =
            Curve::nurbs(NurbsCurve::new(3, vec![a, a, a, a, -0.0008, b, b, b, b], cps).unwrap());
        let d = V3::new(0.0, 0.0, 0.01);
        let top = c.transformed(&Affine::translation(d));
        let mut bb = BodyBuilder::new();
        let [v0, v1, v2, v3] =
            [c.point(a), c.point(b), top.point(b), top.point(a)].map(|p| bb.vertex(p));
        let e0 = bb.edge(c.clone(), a, b, v0, v1);
        let (s1, _) = Curve::line(c.point(b), top.point(b));
        let e1 = bb.edge(s1, 0.0, 0.01, v1, v2);
        let (s3, _) = Curve::line(c.point(a), top.point(a));
        let e2 = bb.edge(top, a, b, v3, v2);
        let e3 = bb.edge(s3, 0.0, 0.01, v0, v3);
        let line = |o: (f64, f64), dir: (f64, f64)| Curve::Line {
            o: P3::new(o.0, o.1, 0.0),
            d: V3::new(dir.0, dir.1, 0.0),
        };
        let lp = vec![
            Coedge {
                edge: e0,
                fwd: true,
                pc: line((0.0, 0.0), (1.0, 0.0)),
            },
            Coedge {
                edge: e1,
                fwd: true,
                pc: line((b, 0.0), (0.0, 100.0)),
            },
            Coedge {
                edge: e2,
                fwd: false,
                pc: line((0.0, 1.0), (1.0, 0.0)),
            },
            Coedge {
                edge: e3,
                fwd: false,
                pc: line((a, 0.0), (0.0, 100.0)),
            },
        ];
        let f = bb.face(Surface::Extrusion { c, d }, true, vec![lp]);
        bb.shell(vec![f], false);
        let body = bb.build().unwrap();
        let (fm, _) = mesh_body(&body, 0.05, 0.08726646259971647);
        let n = fm[0].indices.len();
        assert!(n > 0 && n < 2000, "{n} triangles");
    }

    /// Mass properties and the mesh take the same half of a sphere whose loop crosses its poles
    /// (the mesh took three quarters of the sphere for trj5_s1's feet, or the other half).
    #[test]
    fn hemisphere_through_the_poles_meshes_as_its_mass_says() {
        use std::f64::consts::PI;
        let r = 10.0;
        for far in [false, true] {
            let b = hemisphere(r, far);
            let m = super::super::mass::body_mass(&b);
            assert!(
                (m.volume - 2.0 / 3.0 * PI * r.powi(3)).abs() < 1e-6 * m.volume,
                "far {far}: volume {}",
                m.volume
            );
            assert!(
                (m.surface_area - 3.0 * PI * r * r).abs() < 1e-6 * m.surface_area,
                "far {far}: area {}",
                m.surface_area
            );
            // The centre of mass is on the solid's side: y = ∓3r/8.
            let cy = if far { 3.0 * r / 8.0 } else { -3.0 * r / 8.0 };
            assert!(
                (m.center_of_mass.y - cy).abs() < 1e-6 * r,
                "far {far}: centre {:?}",
                m.center_of_mass
            );
            let (fm, _) = mesh_body(&b, 0.01, 0.1);
            let tris: Vec<[P3; 3]> = fm
                .iter()
                .flat_map(|f| f.indices.iter().map(|t| t.map(|i| f.positions[i as usize])))
                .collect();
            let vol: f64 = tris
                .iter()
                .map(|[a, b, c]| a.coords.dot(&b.coords.cross(&c.coords)) / 6.0)
                .sum();
            let area: f64 = tris
                .iter()
                .map(|[a, b, c]| (b - a).cross(&(c - a)).norm() / 2.0)
                .sum();
            let cy_mesh: f64 = tris
                .iter()
                .map(|[a, b, c]| {
                    a.coords.dot(&b.coords.cross(&c.coords)) / 6.0 * (a.y + b.y + c.y) / 4.0
                })
                .sum::<f64>()
                / vol;
            assert!(
                (vol - m.volume).abs() < 1e-2 * m.volume,
                "far {far}: mesh volume {vol} vs {}",
                m.volume
            );
            assert!(
                (area - m.surface_area).abs() < 1e-2 * m.surface_area,
                "far {far}: mesh area {area} vs {}",
                m.surface_area
            );
            assert!(
                (cy_mesh - cy).abs() < 1e-2 * r,
                "far {far}: mesh centre y {cy_mesh}"
            );
            assert!(
                check_triangles(tris).is_watertight(),
                "far {far}: mesh not watertight"
            );
        }
    }

    /// A rectangle whose right side starts 1.5e-6 below its corner (within the corner's
    /// tolerance), the gap spanned by a degenerate edge, as trj9_ap209's blocks are read: the
    /// boundary runs down the gap and back up through the corner, and the mesh covered half the
    /// face.
    #[test]
    fn a_hair_of_degenerate_edge_on_a_plane_keeps_the_face() {
        use super::super::curve::Curve;
        use super::super::topo::BodyBuilder;
        let (w, h, gap) = (5000.0, 2000.0, 1.5e-6);
        let mut bb = BodyBuilder::new();
        let c = [
            P3::new(0.0, 0.0, 0.0),
            P3::new(w, 0.0, 0.0),
            P3::new(w, h, 0.0),
            P3::new(0.0, h, 0.0),
        ];
        let v: Vec<usize> = c.iter().map(|&p| bb.vertex_with_tol(p, 1e-5)).collect();
        let mut lp = Vec::new();
        for k in 0..4 {
            let a = if k == 1 { P3::new(w, -gap, 0.0) } else { c[k] };
            let (l, len) = Curve::line(a, c[(k + 1) % 4]);
            let e = bb.edge(l.clone(), 0.0, len, v[k], v[(k + 1) % 4]);
            if k == 1 {
                let de = bb.degenerate_edge(c[1], 0.0, gap, v[1]);
                lp.push(Coedge {
                    edge: de,
                    fwd: true,
                    pc: Curve::Line {
                        o: c[1],
                        d: -V3::y(),
                    },
                });
            }
            lp.push(Coedge {
                edge: e,
                fwd: true,
                pc: l,
            });
        }
        let f = bb.face(Surface::Plane { f: Frame::world() }, true, vec![lp]);
        bb.shell(vec![f], false);
        let b = bb.build().unwrap();
        let (fm, _) = mesh_body(&b, 1.0, 0.1);
        let area: f64 = fm
            .iter()
            .flat_map(|f| f.indices.iter().map(|t| t.map(|i| f.positions[i as usize])))
            .map(|[a, b, c]| (b - a).cross(&(c - a)).norm() / 2.0)
            .sum();
        assert!((area - w * h).abs() < 1e-6 * w * h, "mesh area {area}");
        assert!((super::super::mass::face_area(&b, 0).0 - w * h).abs() < 1e-6 * w * h);
    }

    #[test]
    fn params_are_checked() {
        assert!(TessParams::new(0.0, 0.5).is_err());
        assert!(TessParams::new(-1.0, 0.5).is_err());
        assert!(TessParams::new(f64::NAN, 0.5).is_err());
        assert!(TessParams::new(0.1, 0.0).is_err());
        let p = TessParams::new(0.1, 0.5).unwrap();
        assert!(p.with_limits(-1.0, 0.0).is_err());
        assert!(p.with_limits(1.0, 1.0).is_err());
        assert!(p.with_limits(1.0, f64::INFINITY).is_err());
        assert_eq!(p.with_limits(2.0, 4.0).unwrap().max_aspect, 4.0);
        assert!(
            TessParams::try_from(Tessellation {
                deflection: 0.0,
                angle: 0.1
            })
            .is_err()
        );
        // The lenient form (internal callers) falls back to the default instead.
        assert_eq!(
            TessParams::lenient(0.0, 0.0).chord,
            Tessellation::default().deflection
        );
    }

    #[test]
    fn polyline_meets_chord_and_length() {
        // A circle of radius 10 sampled at only its ends and middle.
        let c = |t: f64| P3::new(10.0 * t.cos(), 10.0 * t.sin(), 0.0);
        let p = TessParams::new(0.01, 1.0)
            .unwrap()
            .with_limits(0.5, 0.0)
            .unwrap();
        let ts = refine_polyline(c, &[0.0, 1.5, 3.0], &p);
        for w in ts.windows(2) {
            let (a, b) = (c(w[0]), c(w[1]));
            assert!((b - a).norm() <= 0.5 + 1e-12);
            assert!(seg_dist3(&c(0.5 * (w[0] + w[1])), &a, &b) <= 0.01);
        }
        assert_eq!((ts[0], ts[ts.len() - 1]), (0.0, 3.0));
    }

    #[test]
    fn mesh_check_counts() {
        let p = |x: f64, y: f64, z: f64| P3::new(x, y, z);
        // A tetrahedron: closed.
        let (a, b, c, d) = (
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(0.0, 1.0, 0.0),
            p(0.0, 0.0, 1.0),
        );
        let tet = [[a, c, b], [a, b, d], [b, c, d], [c, a, d]];
        let k = check_triangles(tet);
        assert_eq!((k.edges, k.open, k.bad), (6, 0, 0));
        assert!(k.is_watertight());
        // One face missing: three open sides.
        let k = check_triangles(tet[..3].iter().copied());
        assert_eq!((k.open, k.bad), (3, 0));
        // One face flipped: its three sides run the same way as their neighbours'.
        let k = check_triangles([tet[0], tet[1], tet[2], [a, c, d]]);
        assert_eq!(k.bad, 3);
    }
}
