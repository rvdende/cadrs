//! **Sheet metal Loft** (P3I.9, SM19.2; `reference/onshape/sheetmetal/raw/help-sheet_metal_loft.txt`):
//! a sheet metal part that runs from one profile to another, laid out the way Onshape's help
//! describes it: "the tessellated geometry" of the loft surface, within the **chordal
//! tolerance**, made of **planar walls** joined along the tessellation.
//!
//! - **Profiles** come in as 3D polylines (arcs already cut into pieces by the chordal tolerance,
//!   [`arc_pieces`]): closed loops, open chains or a single point (a cone's apex).
//! - **Connections** pair a point of the first profile with a point of the second (Onshape's
//!   *Connections*, dragged with manipulators). Without any, the loops are matched by proximity:
//!   the second loop starts at its point nearest the first loop's start (both measured from their
//!   centroids), and both run the same way round. Between connections the strip of triangles is
//!   the one of least area (a dynamic programme over the two point lists), so a rectangle's
//!   straight sides each meet one point of a circle and its corners fan out to the arcs between:
//!   the classic "square to round" transition.
//! - **Walls**: neighbouring triangles in one plane make one wall. Neighbouring walls meet at a
//!   **facet joint** — a [`JointKind::Tangent`] between two planar walls, with no bend region, so
//!   the flat pattern lays them edge to edge. With `bends` on, an edge where two walls meet at
//!   30° or more (and which shares no end with another such edge, so bends never fan into one
//!   point) becomes a **bend** of the model's radius instead ([`SharpBuilder`]).
//! - **Rip**: a connection marked *Rip* cuts the sheet there (the walls stop the minimal gap
//!   apart). A closed loft must be cut somewhere to lie flat: without a ripped connection it rips
//!   at its first connection (the matched start).
//! - **The folded solid** of a wall ([`wall_slab`]) is its outline thickened on its material
//!   side with **mitred** sides at facet joints (the side face lies on the plane halfway between
//!   the two walls), so neighbouring walls meet face to face with neither a gap nor an overlap.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::model::{BuildError, Joint, JointId, JointKind, JointNamer, Model, P3, RipStyle, SharpBuilder, Surface, V3, Wall, WallId};
use crate::params::Params;
use crate::poly::{self, P2, Polygon, Seg2, inward_normal};

/// Onshape's default chordal tolerance (its dialog shows 1 mm).
pub const DEFAULT_CHORDAL_TOLERANCE: f64 = 1.0;

/// Walls meeting at this angle or more may bend (with `bends` on).
pub const BEND_MIN_ANGLE: f64 = 30.0 * PI / 180.0;

/// How many straight pieces an arc of `radius` sweeping `sweep` (radians) needs so that no piece
/// strays more than `tol` from it (the chordal tolerance).
pub fn arc_pieces(radius: f64, sweep: f64, tol: f64) -> usize {
    let r = radius.abs();
    let tol = tol.max(1e-6);
    if !r.is_finite() || r <= tol {
        return ((sweep.abs() / (PI / 2.0)).ceil() as usize).max(1);
    }
    let step = 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos();
    ((sweep.abs() / step.max(1e-6)).ceil() as usize).clamp(1, 720)
}

/// One profile as a polyline: a closed loop, an open chain, or one point.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileIn {
    pub points: Vec<P3>,
    pub closed: bool,
}

impl ProfileIn {
    pub fn point(p: P3) -> ProfileIn {
        ProfileIn { points: vec![p], closed: false }
    }

    pub fn is_point(&self) -> bool {
        self.points.len() == 1
    }

    fn centroid(&self) -> P3 {
        let n = self.points.len().max(1) as f64;
        P3::from(self.points.iter().fold(V3::zeros(), |a, p| a + p.coords) / n)
    }

    /// Newell's vector area of a loop.
    fn vector_area(&self) -> V3 {
        let n = self.points.len();
        (0..n).fold(V3::zeros(), |acc, i| acc + self.points[i].coords.cross(&self.points[(i + 1) % n].coords)) / 2.0
    }
}

/// A connection: a point on each profile that the loft joins (snapped to the profiles' nearest
/// points), optionally ripped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConnectionIn {
    pub a: P3,
    pub b: P3,
    pub rip: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoftOpts {
    /// The material on the other side (the thickness arrow).
    pub flip_side: bool,
    pub connections: Vec<ConnectionIn>,
    /// Edges meeting at [`BEND_MIN_ANGLE`] or more become bends.
    pub bends: bool,
    /// A stable key for the walls' and joints' ids (the feature's).
    pub key: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LoftError {
    /// A profile has no points.
    EmptyProfile,
    /// Both profiles are points.
    TwoPoints,
    /// One profile is closed and the other open.
    OpenAndClosed,
    /// The connections cross each other.
    ConnectionsCross,
    /// The profiles make no surface (they lie on top of each other, or in one plane).
    Degenerate,
    Build(BuildError),
}

impl LoftError {
    pub fn message(&self) -> String {
        match self {
            LoftError::EmptyProfile => "A profile is empty".into(),
            LoftError::TwoPoints => "Only one profile can be a point".into(),
            LoftError::OpenAndClosed => "Both profiles must be closed, or both open".into(),
            LoftError::ConnectionsCross => "The connections cross each other".into(),
            LoftError::Degenerate => "The profiles don't make a surface".into(),
            LoftError::Build(e) => e.message(),
        }
    }
}

impl std::fmt::Display for LoftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for LoftError {}

/// An edge of the strip between two triangles (or at its ends): from the first profile to the
/// second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rung {
    pub a: P3,
    pub b: P3,
    /// The connection it is (its index in the matched connections; 0 is the matched start of a
    /// closed loft).
    pub connection: Option<usize>,
    pub rip: bool,
}

/// The tessellated loft surface: triangles in order along the profiles, each between rung `k`
/// and rung `k + 1` (a closed loft's last rung is its first).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Strip {
    pub triangles: Vec<[P3; 3]>,
    pub rungs: Vec<Rung>,
    pub closed: bool,
    /// The connections as matched (first profile's point, second's, ripped), for the
    /// manipulators.
    pub connections: Vec<ConnectionIn>,
}

impl Strip {
    pub fn area(&self) -> f64 {
        self.triangles.iter().map(|t| tri_area(t)).sum()
    }
}

fn tri_area(t: &[P3; 3]) -> f64 {
    (t[1] - t[0]).cross(&(t[2] - t[0])).norm() / 2.0
}

fn dedup(points: &[P3], closed: bool, tol: f64) -> Vec<P3> {
    let mut v: Vec<P3> = Vec::with_capacity(points.len());
    for p in points {
        if v.last().is_none_or(|q: &P3| (q - p).norm() > tol) {
            v.push(*p);
        }
    }
    if closed {
        while v.len() > 2 && (v[0] - v[v.len() - 1]).norm() <= tol {
            v.pop();
        }
    }
    v
}

fn nearest(points: &[P3], p: P3) -> usize {
    (0..points.len()).min_by(|a, b| (points[*a] - p).norm().total_cmp(&(points[*b] - p).norm())).unwrap_or(0)
}

/// The least-area strip between chains `a` and `b` (each from one connection to the next): the
/// moves, `true` to step along `a`.
fn least_area(a: &[P3], b: &[P3]) -> Vec<bool> {
    let (na, nb) = (a.len(), b.len());
    let mut cost = vec![vec![f64::INFINITY; nb]; na];
    let mut from_a = vec![vec![false; nb]; na];
    cost[0][0] = 0.0;
    for i in 0..na {
        for j in 0..nb {
            if i == 0 && j == 0 {
                continue;
            }
            if i > 0 {
                let c = cost[i - 1][j] + tri_area(&[a[i - 1], a[i], b[j]]);
                if c < cost[i][j] {
                    cost[i][j] = c;
                    from_a[i][j] = true;
                }
            }
            if j > 0 {
                let c = cost[i][j - 1] + tri_area(&[a[i], b[j], b[j - 1]]);
                // Ties go along b, then a: deterministic.
                if c < cost[i][j] - 1e-12 {
                    cost[i][j] = c;
                    from_a[i][j] = false;
                }
            }
        }
    }
    let (mut i, mut j) = (na - 1, nb - 1);
    let mut moves = Vec::with_capacity(na + nb);
    while i > 0 || j > 0 {
        let step_a = if i == 0 { false } else if j == 0 { true } else { from_a[i][j] };
        moves.push(step_a);
        if step_a {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    moves.reverse();
    moves
}

/// The tessellated surface between two profiles (see the module docs).
pub fn strip(p1: &ProfileIn, p2: &ProfileIn, connections: &[ConnectionIn]) -> Result<Strip, LoftError> {
    if p1.points.is_empty() || p2.points.is_empty() {
        return Err(LoftError::EmptyProfile);
    }
    if p1.is_point() && p2.is_point() {
        return Err(LoftError::TwoPoints);
    }
    let size = p1.points.iter().chain(&p2.points).map(|p| p.coords.norm()).fold(1.0, f64::max);
    let tol = 1e-9 * size;
    // A point stands in for a profile like the other (closed or open).
    let closed = if p1.is_point() {
        p2.closed
    } else if p2.is_point() {
        p1.closed
    } else if p1.closed != p2.closed {
        return Err(LoftError::OpenAndClosed);
    } else {
        p1.closed
    };
    let mut a = if p1.is_point() { p1.points.clone() } else { dedup(&p1.points, closed, tol) };
    let mut b = if p2.is_point() { p2.points.clone() } else { dedup(&p2.points, closed, tol) };
    if (!p1.is_point() && a.len() < 2) || (!p2.is_point() && b.len() < 2) || (closed && (a.len().max(b.len()) < 3)) {
        return Err(LoftError::Degenerate);
    }
    // Both the same way round.
    if !p1.is_point() && !p2.is_point() {
        if closed {
            let (va, vb) = (ProfileIn { points: a.clone(), closed }.vector_area(), ProfileIn { points: b.clone(), closed }.vector_area());
            if va.dot(&vb) < 0.0 {
                b.reverse();
            }
        } else {
            let (a0, a1, b0, b1) = (a[0], a[a.len() - 1], b[0], b[b.len() - 1]);
            if (a0 - b0).norm() + (a1 - b1).norm() > (a0 - b1).norm() + (a1 - b0).norm() + tol {
                b.reverse();
            }
        }
    }
    // The connections as index pairs.
    let mut pairs: Vec<(usize, usize, bool)> = connections.iter().map(|c| (nearest(&a, c.a), nearest(&b, c.b), c.rip)).collect();
    if closed {
        if pairs.is_empty() {
            // Matched by proximity, from the centroids.
            let (ca, cb) = (ProfileIn { points: a.clone(), closed }.centroid(), ProfileIn { points: b.clone(), closed }.centroid());
            let j = (0..b.len()).min_by(|x, y| ((b[*x] - cb) - (a[0] - ca)).norm().total_cmp(&((b[*y] - cb) - (a[0] - ca)).norm())).unwrap_or(0);
            pairs.push((0, j, false));
        }
        // Start both loops at the first connection.
        let (ia, ib) = (pairs[0].0, pairs[0].1);
        if a.len() > 1 {
            a.rotate_left(ia);
        }
        if b.len() > 1 {
            b.rotate_left(ib);
        }
        let (na, nb) = (a.len(), b.len());
        for p in &mut pairs {
            p.0 = if na > 1 { (p.0 + na - ia) % na } else { 0 };
            p.1 = if nb > 1 { (p.1 + nb - ib) % nb } else { 0 };
        }
    }
    // In order along the first profile (then the second), each strictly after the last.
    let first = if closed { Some(pairs[0]) } else { None };
    let mut rest: Vec<(usize, usize, bool)> = pairs.iter().copied().skip(usize::from(closed)).collect();
    rest.sort_by_key(|p| (p.0, p.1));
    rest.dedup_by_key(|p| (p.0, p.1));
    let mut cuts: Vec<(usize, usize, bool)> = Vec::new();
    let (na, nb) = (a.len(), b.len());
    // The unrolled ends: a closed loop comes back to its start.
    let (ea, eb) = if closed { (if na > 1 { na } else { 0 }, if nb > 1 { nb } else { 0 }) } else { (na - 1, nb - 1) };
    match first {
        Some(f) => cuts.push(f),
        None => cuts.push((0, 0, false)),
    }
    for p in rest {
        if (p.0, p.1) == (0, 0) || (closed && (p.0, p.1) == (cuts[0].0, cuts[0].1)) {
            if !closed {
                cuts[0].2 |= p.2;
            }
            continue;
        }
        if (p.0, p.1) == (ea, eb) && !closed {
            continue;
        }
        let last = cuts[cuts.len() - 1];
        if p.0 < last.0 || p.1 < last.1 || (p.0 == last.0 && p.1 == last.1) {
            return Err(LoftError::ConnectionsCross);
        }
        cuts.push(p);
    }
    let end_rip = if closed { cuts[0].2 } else { false };
    cuts.push((ea, eb, end_rip));
    let at_a = |i: usize| a[if na > 1 { i % na } else { 0 }];
    let at_b = |j: usize| b[if nb > 1 { j % nb } else { 0 }];
    let mut out = Strip { closed, ..Default::default() };
    // A closed loft's connections (its matched start first); an open one's inner ones (its ends
    // aren't connections anyone placed).
    let placed = if closed { &cuts[..cuts.len() - 1] } else { &cuts[1..cuts.len() - 1] };
    out.connections = placed.iter().map(|(i, j, rip)| ConnectionIn { a: at_a(*i), b: at_b(*j), rip: *rip }).collect();
    for (k, w) in cuts.windows(2).enumerate() {
        let ((i0, j0, rip0), (i1, j1, _)) = (w[0], w[1]);
        let sa: Vec<P3> = (i0..=i1).map(at_a).collect();
        let sb: Vec<P3> = (j0..=j1).map(at_b).collect();
        let conn = if closed { Some(k) } else if k == 0 { None } else { Some(k - 1) };
        out.rungs.push(Rung { a: sa[0], b: sb[0], connection: conn, rip: rip0 });
        let (mut i, mut j) = (0, 0);
        for step_a in least_area(&sa, &sb) {
            if step_a {
                out.triangles.push([sa[i], sa[i + 1], sb[j]]);
                i += 1;
            } else {
                out.triangles.push([sa[i], sb[j + 1], sb[j]]);
                j += 1;
            }
            out.rungs.push(Rung { a: sa[i], b: sb[j], connection: None, rip: false });
        }
        // The last rung of a section is the next section's first.
        out.rungs.pop();
    }
    if !closed {
        let (i, j, _) = cuts[cuts.len() - 1];
        out.rungs.push(Rung { a: at_a(i), b: at_b(j), connection: None, rip: false });
    }
    if out.triangles.is_empty() || out.area() <= 1e-12 * size * size {
        return Err(LoftError::Degenerate);
    }
    Ok(out)
}

/// A loft built: its definition, and where each wall and joint came from.
#[derive(Clone, Debug, PartialEq)]
pub struct LoftBuilt {
    pub model: Model,
    pub strip: Strip,
    /// A key per wall and joint (from the feature's key and their place in the strip).
    pub walls: Vec<(u64, WallId)>,
    pub joints: Vec<(u64, JointId)>,
    pub warnings: Vec<String>,
}

fn mix(key: u64, what: u64, i: usize) -> u64 {
    let mut h = key ^ 0x9e37_79b9_7f4a_7c15 ^ what.rotate_left(17);
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd) ^ (i as u64).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// A wall being made: its triangles, plane and outline.
struct Group {
    tris: Vec<usize>,
    origin: P3,
    u: V3,
    v: V3,
    n: V3,
}

impl Group {
    fn local(&self, p: P3) -> P2 {
        P2::new((p - self.origin).dot(&self.u), (p - self.origin).dot(&self.v))
    }
}

/// The parameter interval `[t0, t1]` along `p → q` that the outline's boundary covers.
fn on_line(outline: &Polygon, la: P2, lb: P2, tol: f64) -> Option<(f64, f64)> {
    let d = lb - la;
    let len2 = d.norm_squared();
    if len2 < 1e-24 {
        return None;
    }
    let n = poly::perp(d / len2.sqrt());
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let l = &outline.outer;
    for i in 0..l.len() {
        let (p, q) = (l[i], l[(i + 1) % l.len()]);
        if (p - la).dot(&n).abs() < tol && (q - la).dot(&n).abs() < tol {
            for x in [p, q] {
                let s = (x - la).dot(&d) / len2;
                lo = lo.min(s);
                hi = hi.max(s);
            }
        }
    }
    (lo < hi).then(|| (lo.max(0.0), hi.min(1.0)))
}

/// The outline's edges with an end at `e`.
fn incident(outline: &Polygon, e: P2, tol: f64) -> Vec<(P2, P2)> {
    let l = &outline.outer;
    (0..l.len()).map(|i| (l[i], l[(i + 1) % l.len()])).filter(|(p, q)| (p - e).norm() < tol || (q - e).norm() < tol).collect()
}

/// Where a closed loft rips when no connection says: the rung whose corners in the triangles
/// either side are the widest (the rip's gap then trims the least from its neighbours).
fn auto_rip(s: &Strip) -> usize {
    let n = s.triangles.len();
    let angle = |t: &[P3; 3], at: P3| {
        let k = (0..3).min_by(|a, b| (t[*a] - at).norm().total_cmp(&(t[*b] - at).norm())).unwrap_or(0);
        let (u, v) = (t[(k + 1) % 3] - t[k], t[(k + 2) % 3] - t[k]);
        if u.norm() < 1e-12 || v.norm() < 1e-12 { 0.0 } else { (u.dot(&v) / (u.norm() * v.norm())).clamp(-1.0, 1.0).acos() }
    };
    (0..s.rungs.len().min(n))
        .max_by(|a, b| {
            let score = |k: usize| {
                let r = s.rungs[k];
                let (before, after) = (&s.triangles[(k + n - 1) % n], &s.triangles[k]);
                [angle(before, r.a), angle(before, r.b), angle(after, r.a), angle(after, r.b)].into_iter().fold(f64::INFINITY, f64::min)
            };
            score(*a).total_cmp(&score(*b)).then(b.cmp(a))
        })
        .unwrap_or(0)
}

/// Lays the loft out as sheet metal (see the module docs).
pub fn loft(params: Params, p1: &ProfileIn, p2: &ProfileIn, opts: &LoftOpts) -> Result<LoftBuilt, LoftError> {
    let s = strip(p1, p2, &opts.connections)?;
    let size = s.triangles.iter().flatten().map(|p| p.coords.norm()).fold(1.0, f64::max);
    let lin = 1e-7 * size;
    let tris = &s.triangles;
    let normal = |t: &[P3; 3]| (t[1] - t[0]).cross(&(t[2] - t[0]));
    let degenerate = |t: &[P3; 3]| normal(t).norm() <= 1e-10 * size * size;
    // The material side: outward for a closed loft (away from its middle), else the strip's own;
    // the thickness arrow turns it round.
    let mut sign = 1.0;
    if s.closed {
        let mid = P3::from(tris.iter().flatten().fold(V3::zeros(), |a, p| a + p.coords) / (3 * tris.len()) as f64);
        let outward: f64 = tris
            .iter()
            .map(|t| normal(t).dot(&((t[0].coords + t[1].coords + t[2].coords) / 3.0 - mid.coords)))
            .sum();
        if outward < 0.0 {
            sign = -1.0;
        }
    }
    if opts.flip_side {
        sign = -sign;
    }
    // Which rungs rip: the ripped connections; a closed loft without one rips at its start.
    let mut rips: Vec<bool> = s.rungs.iter().map(|r| r.rip).collect();
    if s.closed && !rips.iter().any(|r| *r) {
        rips[auto_rip(&s)] = true;
    }
    // Triangles into walls: neighbours in one plane, not across a rip.
    let mut groups: Vec<Group> = Vec::new();
    let plane_of = |t: &[P3; 3]| {
        let n = normal(t).normalize() * sign;
        let u = (t[1] - t[0]).normalize();
        let v = n.cross(&u);
        (t[0], u, v, n)
    };
    for (k, t) in tris.iter().enumerate() {
        let joins = groups.last().is_some_and(|g: &Group| {
            !rips[k] && (degenerate(t) || (normal(t).normalize() * sign).dot(&g.n) > 1.0 - 1e-10 && t.iter().all(|p| (p - g.origin).dot(&g.n).abs() <= lin))
        });
        if joins {
            groups.last_mut().expect("joins").tris.push(k);
        } else if degenerate(t) && groups.is_empty() {
            // A sliver at the very start: it goes with the first real wall.
            continue;
        } else {
            let (origin, u, v, n) = plane_of(t);
            groups.push(Group { tris: vec![k], origin, u, v, n });
        }
    }
    // A sliver skipped at the start belongs to the first wall.
    if let Some(g) = groups.first_mut() {
        for k in 0..g.tris[0] {
            g.tris.insert(k, k);
        }
    }
    // A closed loft's last wall runs on into its first when in one plane and not ripped there.
    if s.closed && groups.len() > 1 && !rips[0] {
        let (f, l) = (&groups[0], &groups[groups.len() - 1]);
        if f.n.dot(&l.n) > 1.0 - 1e-10 && tris[l.tris[0]].iter().all(|p| (p - f.origin).dot(&f.n).abs() <= lin) {
            let last = groups.pop().expect("two");
            groups[0].tris.splice(0..0, last.tris);
        }
    }
    if groups.is_empty() {
        return Err(LoftError::Degenerate);
    }
    let n_groups = groups.len();
    if s.closed && n_groups == 1 {
        return Err(LoftError::Degenerate);
    }
    // Outlines.
    let mut outlines: Vec<Polygon> = Vec::new();
    for g in &groups {
        let pieces: Vec<Polygon> = g.tris.iter().map(|k| Polygon::new(tris[*k].iter().map(|p| g.local(*p)).collect())).filter(|p| p.area() > 1e-14 * size * size).collect();
        let u = poly::union(&pieces);
        let exact: Vec<P2> = g.tris.iter().flat_map(|k| tris[*k].iter().map(|p| g.local(*p))).collect();
        let best = u.into_iter().max_by(|a, b| a.area().total_cmp(&b.area())).ok_or(LoftError::Degenerate)?;
        outlines.push(poly::snap_to(&best, &exact, 10.0 * poly::GRID));
    }
    // Joints: the rung between each wall and the next (and round a closed loft).
    struct J {
        a: usize,
        b: usize,
        rung: usize,
        rip: bool,
        bend: bool,
    }
    let mut joints: Vec<J> = Vec::new();
    for gi in 0..n_groups {
        let next = gi + 1;
        if next == n_groups && !s.closed {
            break;
        }
        let nb = next % n_groups;
        if nb == gi {
            break;
        }
        let rung = groups[nb].tris[0] % s.rungs.len();
        // The rung before wall nb's first triangle (a sliver may lead it).
        joints.push(J { a: gi, b: nb, rung, rip: rips[rung], bend: false });
    }
    // Bends: steep, and not fanning into a point with another.
    if opts.bends {
        let steep: Vec<usize> = (0..joints.len()).filter(|i| !joints[*i].rip && groups[joints[*i].a].n.dot(&groups[joints[*i].b].n).clamp(-1.0, 1.0).acos() >= BEND_MIN_ANGLE).collect();
        for &i in &steep {
            let r = s.rungs[joints[i].rung];
            let shares = steep.iter().any(|&k| {
                let o = s.rungs[joints[k].rung];
                k != i && [o.a, o.b].iter().any(|p| (p - r.a).norm() <= lin || (p - r.b).norm() <= lin)
            });
            joints[i].bend = !shares;
        }
    }
    let gap = params.minimal_gap;
    let untrimmed = outlines.clone();
    // Rips: each wall stops half the gap short of the rung.
    for j in joints.iter().filter(|j| j.rip) {
        let r = s.rungs[j.rung];
        for w in [j.a, j.b] {
            let g = &groups[w];
            let seg = Seg2::new(g.local(r.a), g.local(r.b));
            if let Some(into) = inward_normal(&outlines[w], seg) {
                let cut = outlines[w].clip_half_plane(seg.a + into * (gap / 2.0), into);
                if cut.is_empty() {
                    return Err(LoftError::Build(BuildError::WallTrimmedAway { joint: j.rung, wall: w }));
                }
                outlines[w] = cut;
            }
        }
    }
    // At each end of a rip, the facets that meet there lose the same small tip (a relief), so the
    // gap's trim leaves no facet sticking out past its neighbour.
    for j in joints.iter().filter(|j| j.rip) {
        let r = s.rungs[j.rung];
        for e3 in [r.a, r.b] {
            // How far along their other edges from the end the rip walls were trimmed.
            let mut rho: f64 = 0.0;
            for w in [j.a, j.b] {
                let g = &groups[w];
                let e = g.local(e3);
                for (p, q) in incident(&untrimmed[w], e, lin * 10.0) {
                    let other = if (p - e).norm() < (q - e).norm() { q } else { p };
                    let on_rung = ((other - g.local(r.a)).norm() < lin * 10.0) || ((other - g.local(r.b)).norm() < lin * 10.0);
                    if on_rung {
                        continue;
                    }
                    if let Some((t0, _)) = on_line(&outlines[w], e, other, lin * 10.0) {
                        rho = rho.max(t0 * (other - e).norm());
                    }
                }
            }
            if rho <= lin * 10.0 {
                continue;
            }
            let rho = rho * (1.0 + 1e-6);
            for (w, g) in groups.iter().enumerate() {
                let e = g.local(e3);
                if (g.origin + g.u * e.x + g.v * e.y - e3).norm() > lin * 10.0 {
                    continue;
                }
                let ends: Vec<P2> = incident(&untrimmed[w], e, lin * 10.0)
                    .into_iter()
                    .map(|(p, q)| if (p - e).norm() < (q - e).norm() { q } else { p })
                    .collect();
                if ends.len() != 2 {
                    continue;
                }
                let at = |o: P2| e + (o - e).normalize() * rho.min(0.9 * (o - e).norm());
                let (p1, p2) = (at(ends[0]), at(ends[1]));
                let mut nrm = poly::perp((p2 - p1).normalize());
                if (e - p1).dot(&nrm) > 0.0 {
                    nrm = -nrm;
                }
                let cut = outlines[w].clip_half_plane(p1, nrm);
                if !cut.is_empty() {
                    outlines[w] = cut;
                }
            }
        }
    }
    let mut warnings = Vec::new();
    let wall_ids: Vec<WallId> = (0..n_groups).map(|i| WallId(i as u32)).collect();
    // Bends through the sharp builder (it trims the walls back to the tangent lines).
    let mut model = if joints.iter().any(|j| j.bend) {
        let mut sb = SharpBuilder::new(params);
        for (i, g) in groups.iter().enumerate() {
            let w = sb.wall(g.origin, g.u, g.v, outlines[i].clone());
            sb.set_wall_id(w, wall_ids[i]);
        }
        for j in joints.iter().filter(|j| j.bend) {
            let r = s.rungs[j.rung];
            sb.bend(j.a, j.b, (r.a, r.b));
        }
        match sb.build() {
            Ok(m) => Some(m),
            Err(e) => {
                warnings.push(format!("The loft's steep edges stay sharp: {}", e.message()));
                for j in &mut joints {
                    j.bend = false;
                }
                None
            }
        }
    } else {
        None
    }
    .unwrap_or_else(|| Model {
        params,
        walls: groups
            .iter()
            .enumerate()
            .map(|(i, g)| Wall { id: wall_ids[i], surface: Surface::Planar { origin: g.origin, u: g.u, v: g.v }, outline: outlines[i].clone() })
            .collect(),
        ..Default::default()
    });
    // Rips and facet joints, on what is left of the walls.
    let mut namer = JointNamer::default();
    namer.taken = model.joints.iter().map(|j| j.name.clone()).collect();
    let mut next_id = model.joints.iter().map(|j| j.id.0 + 1).max().unwrap_or(0);
    for j in joints.iter().filter(|j| !j.bend) {
        let r = s.rungs[j.rung];
        let (Some(wa), Some(wb)) = (model.wall(wall_ids[j.a]).cloned(), model.wall(wall_ids[j.b]).cloned()) else { continue };
        let kind = if j.rip {
            let seg_on = |w: usize| {
                let g = &groups[w];
                let seg = Seg2::new(g.local(r.a), g.local(r.b));
                inward_normal(&untrimmed[w], seg).map(|i| seg.offset(i * (gap / 2.0))).unwrap_or(seg)
            };
            JointKind::Rip { on_a: seg_on(j.a), on_b: seg_on(j.b), style: RipStyle::EdgeJoint }
        } else {
            let (la, lb) = (wa.surface.local(r.a), wa.surface.local(r.b));
            let (ma, mb) = (wb.surface.local(r.a), wb.surface.local(r.b));
            let (Some((a0, a1)), Some((b0, b1))) = (on_line(&wa.outline, la, lb, lin * 10.0), on_line(&wb.outline, ma, mb, lin * 10.0)) else {
                warnings.push("A loft edge was trimmed away".into());
                continue;
            };
            let (t0, t1) = (a0.max(b0), a1.min(b1));
            if t1 - t0 < 1e-9 {
                continue;
            }
            let at = |t: f64| r.a + (r.b - r.a) * t;
            let (p, q) = (at(t0), at(t1));
            JointKind::Tangent { on_a: Seg2::new(wa.surface.local(p), wa.surface.local(q)), on_b: Seg2::new(wb.surface.local(p), wb.surface.local(q)) }
        };
        let name = namer.name(&kind);
        model.joints.push(Joint { id: JointId(next_id), name, a: wall_ids[j.a], b: wall_ids[j.b], kind });
        next_id += 1;
    }
    let walls = wall_ids.iter().enumerate().map(|(i, w)| (mix(opts.key, 1, i), *w)).collect();
    let joint_keys = model.joints.iter().enumerate().map(|(i, j)| (mix(opts.key, 2, i), j.id)).collect();
    Ok(LoftBuilt { model, strip: s, walls, joints: joint_keys, warnings })
}

/// Whether a joint is a facet joint: a tangent joint between two planar walls that meet at an
/// angle (a sheet metal loft's tessellation).
pub fn is_facet(m: &Model, j: &Joint) -> bool {
    let JointKind::Tangent { .. } = j.kind else { return false };
    match (m.wall(j.a).map(|w| w.surface), m.wall(j.b).map(|w| w.surface)) {
        (Some(a @ Surface::Planar { .. }), Some(b @ Surface::Planar { .. })) => {
            let (na, nb) = (a.normal().unwrap_or_default(), b.normal().unwrap_or_default());
            na.dot(&nb) < 1.0 - 1e-12
        }
        _ => false,
    }
}

/// Ear clipping of a simple polygon (counter-clockwise): triangles as index triples.
pub fn triangulate(l: &[P2]) -> Vec<[usize; 3]> {
    let n = l.len();
    if n < 3 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = (0..n).collect();
    if poly::signed_area(l) < 0.0 {
        idx.reverse();
    }
    let cross = |a: P2, b: P2, c: P2| (b - a).perp(&(c - a));
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 && guard < 10 * n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (i0, i1, i2) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (l[i0], l[i1], l[i2]);
            if cross(a, b, c) <= 1e-14 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                j != i0 && j != i1 && j != i2 && {
                    let p = l[j];
                    cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
                }
            });
            if inside {
                continue;
            }
            out.push([i0, i1, i2]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            // Collinear leftovers: drop the flattest corner.
            let m = idx.len();
            let k = (0..m)
                .min_by(|x, y| {
                    let f = |k: usize| cross(l[idx[(k + m - 1) % m]], l[idx[k]], l[idx[(k + 1) % m]]).abs();
                    f(*x).total_cmp(&f(*y))
                })
                .unwrap_or(0);
            idx.remove(k);
        }
    }
    if idx.len() == 3 && cross(l[idx[0]], l[idx[1]], l[idx[2]]).abs() > 0.0 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// A planar wall's folded solid as a closed triangle mesh (outward facing): its outline on the
/// definition surface, the same outline `thickness` along the material normal, and the sides —
/// square to the wall, except at facet joints, where the side lies on the plane halfway between
/// the two walls so neighbours meet face to face. `None` for a rolled wall or one with holes (the
/// caller extrudes those).
pub fn wall_slab(m: &Model, wall: WallId, removed: &[Polygon]) -> Option<Vec<[P3; 3]>> {
    let w = m.wall(wall)?;
    let Surface::Planar { u, v, .. } = w.surface else { return None };
    if !w.outline.holes.is_empty() || !removed.is_empty() {
        return None;
    }
    let t = m.params.thickness;
    let n = u.cross(&v).normalize();
    let size = w.outline.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0).max(1e-9);
    let tol = 1e-6 * size;
    // No repeated points and no points in the middle of a straight edge (they'd make side
    // faces of no area).
    let mut pts: Vec<P2> = w.outline.outer.clone();
    loop {
        let n = pts.len();
        if n < 3 {
            return None;
        }
        let drop = (0..n).find(|&i| {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            (b - a).norm() < tol || ((c - a).norm() > tol && (b - a).perp(&(c - a)).abs() / (c - a).norm() < tol && (b - a).dot(&(c - b)) > 0.0)
        });
        match drop {
            Some(i) => {
                pts.remove(i);
            }
            None => break,
        }
    }
    let l = &pts;
    let k = l.len();
    // Each edge's side direction (in the side plane, away from the definition surface).
    let pt = |q: P2| w.surface.point(q);
    let mut side: Vec<V3> = vec![n; k];
    for j in m.joints.iter().filter(|j| (j.a == wall || j.b == wall) && is_facet(m, j)) {
        let Some(seg) = j.segment_on(wall) else { continue };
        let Some(other) = m.wall(j.other(wall)).and_then(|o| o.surface.normal()) else { continue };
        let d = seg.b - seg.a;
        let len = d.norm();
        if len < tol {
            continue;
        }
        let nn = poly::perp(d / len);
        for (i, sd) in side.iter_mut().enumerate() {
            let (p, q) = (l[i], l[(i + 1) % k]);
            let on = (p - seg.a).dot(&nn).abs() < tol && (q - seg.a).dot(&nn).abs() < tol;
            let overlap = {
                let (s0, s1) = ((p - seg.a).dot(&d) / len, (q - seg.a).dot(&d) / len);
                s0.max(s1).min(len) - s0.min(s1).max(0.0)
            };
            if on && overlap > tol {
                let mid = (n + other).normalize();
                if mid.dot(&n) > 1e-3 {
                    *sd = mid;
                }
            }
        }
    }
    let bottom: Vec<P3> = l.iter().map(|q| pt(*q)).collect();
    let top: Vec<P3> = (0..k)
        .map(|i| {
            let (prev, next) = ((i + k - 1) % k, i);
            let p = bottom[i];
            let plane = |e: usize| {
                let d = (bottom[(e + 1) % k] - bottom[e]).normalize();
                d.cross(&side[e]).normalize()
            };
            let (s0, s1) = (plane(prev), plane(next));
            let mtx = nalgebra::Matrix3::from_rows(&[n.transpose(), s0.transpose(), s1.transpose()]);
            let rhs = nalgebra::Vector3::new(n.dot(&p.coords) + t, s0.dot(&p.coords), s1.dot(&p.coords));
            let fallback = || {
                // In line: along the side direction the edges share.
                let sd = if side[prev] != n { side[prev] } else { side[next] };
                p + sd * (t / sd.dot(&n))
            };
            if mtx.determinant().abs() < 1e-9 {
                return fallback();
            }
            match mtx.try_inverse() {
                Some(inv) => {
                    let x = P3::from(inv * rhs);
                    // A mitre this far out is a numerical accident.
                    if (x - p).norm() > 50.0 * t { fallback() } else { x }
                }
                None => fallback(),
            }
        })
        .collect();
    let mut out = Vec::new();
    for [a, b, c] in triangulate(l) {
        out.push([bottom[a], bottom[c], bottom[b]]);
        out.push([top[a], top[b], top[c]]);
    }
    for i in 0..k {
        let j = (i + 1) % k;
        out.push([bottom[i], bottom[j], top[j]]);
        out.push([bottom[i], top[j], top[i]]);
    }
    Some(out)
}

/// The volume a closed, outward-facing triangle mesh encloses.
pub fn mesh_volume(tris: &[[P3; 3]]) -> f64 {
    tris.iter().map(|t| t[0].coords.dot(&t[1].coords.cross(&t[2].coords))).sum::<f64>() / 6.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flat::flatten;

    fn rect(z: f64, w: f64, h: f64) -> ProfileIn {
        ProfileIn {
            points: vec![P3::new(-w / 2.0, -h / 2.0, z), P3::new(w / 2.0, -h / 2.0, z), P3::new(w / 2.0, h / 2.0, z), P3::new(-w / 2.0, h / 2.0, z)],
            closed: true,
        }
    }

    fn circle(z: f64, r: f64, tol: f64) -> ProfileIn {
        let n = arc_pieces(r, std::f64::consts::TAU, tol);
        ProfileIn {
            points: (0..n).map(|i| {
                let a = i as f64 / n as f64 * std::f64::consts::TAU + std::f64::consts::FRAC_PI_4;
                P3::new(r * a.cos(), r * a.sin(), z)
            }).collect(),
            closed: true,
        }
    }

    #[test]
    fn arc_pieces_follow_the_chordal_tolerance() {
        let n = arc_pieces(40.0, std::f64::consts::TAU, 1.0);
        let step = std::f64::consts::TAU / n as f64;
        assert!(40.0 * (1.0 - (step / 2.0).cos()) <= 1.0 + 1e-9);
        assert!(arc_pieces(40.0, std::f64::consts::TAU, 0.1) > n);
    }

    #[test]
    fn square_to_round_has_one_triangle_per_side_and_fans_at_corners() {
        let s = strip(&rect(0.0, 100.0, 100.0), &circle(60.0, 40.0, 0.5), &[]).unwrap();
        // Each rectangle side (long, straight) meets a single circle point.
        let long = s.triangles.iter().filter(|t| (t[0] - t[1]).norm() > 99.0 || (t[1] - t[2]).norm() > 99.0 || (t[0] - t[2]).norm() > 99.0).count();
        assert_eq!(long, 4);
        assert_eq!(s.triangles.len(), 4 + circle(60.0, 40.0, 0.5).points.len());
    }

    #[test]
    fn rectangle_to_circle_flattens_and_its_slabs_match_the_flat() {
        let p = Params { thickness: 1.0, ..Default::default() };
        let b = loft(p, &rect(0.0, 100.0, 80.0), &circle(60.0, 30.0, 1.0), &LoftOpts::default()).unwrap();
        let flat = flatten(&b.model);
        assert!(flat.is_ok(), "{:?}", flat.errors);
        assert_eq!(flat.parts.len(), 1);
        let rips = b.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Rip { .. })).count();
        assert_eq!(rips, 1);
        let area = flat.parts[0].area();
        let volume: f64 = b.model.walls.iter().map(|w| mesh_volume(&wall_slab(&b.model, w.id, &[]).unwrap())).sum();
        assert!(((area * p.thickness) - volume).abs() / volume < 0.02, "flat {area} × T vs folded {volume}");
        // Mitred slabs meet face to face: every slab is a proper solid.
        for w in &b.model.walls {
            assert!(mesh_volume(&wall_slab(&b.model, w.id, &[]).unwrap()) > 0.0);
        }
    }

    #[test]
    fn a_frustum_bends_at_its_corners() {
        let p = Params { thickness: 1.0, bend_radius: 2.0, ..Default::default() };
        let b = loft(p, &rect(0.0, 100.0, 100.0), &rect(50.0, 50.0, 50.0), &LoftOpts { bends: true, ..Default::default() }).unwrap();
        assert_eq!(b.model.walls.len(), 4);
        let bends = b.model.joints.iter().filter(|j| j.bend().is_some()).count();
        assert_eq!(bends, 3, "{:?}", b.model.joints.iter().map(|j| &j.name).collect::<Vec<_>>());
        assert!(b.model.validate().is_empty(), "{:?}", b.model.validate());
        assert!(flatten(&b.model).is_ok());
    }

    #[test]
    fn open_profiles_and_a_ripped_connection() {
        let a = ProfileIn { points: vec![P3::new(0.0, 0.0, 0.0), P3::new(50.0, 0.0, 0.0), P3::new(50.0, 50.0, 0.0)], closed: false };
        let b = ProfileIn { points: vec![P3::new(0.0, 0.0, 40.0), P3::new(30.0, 0.0, 40.0), P3::new(30.0, 30.0, 40.0)], closed: false };
        let c = ConnectionIn { a: P3::new(50.0, 0.0, 0.0), b: P3::new(30.0, 0.0, 40.0), rip: true };
        let built = loft(Params::default(), &a, &b, &LoftOpts { connections: vec![c], ..Default::default() }).unwrap();
        let flat = flatten(&built.model);
        assert!(flat.is_ok());
        assert_eq!(flat.parts.len(), 2, "the rip cuts it in two");
    }

    #[test]
    fn crossing_connections_are_refused() {
        let a = ProfileIn { points: (0..5).map(|i| P3::new(i as f64 * 10.0, 0.0, 0.0)).collect(), closed: false };
        let b = ProfileIn { points: (0..5).map(|i| P3::new(i as f64 * 10.0, 0.0, 30.0)).collect(), closed: false };
        let cs = vec![
            ConnectionIn { a: P3::new(10.0, 0.0, 0.0), b: P3::new(30.0, 0.0, 30.0), rip: false },
            ConnectionIn { a: P3::new(30.0, 0.0, 0.0), b: P3::new(10.0, 0.0, 30.0), rip: false },
        ];
        assert_eq!(strip(&a, &b, &cs).unwrap_err(), LoftError::ConnectionsCross);
    }

    #[test]
    fn a_cone_to_a_point() {
        let b = loft(Params::default(), &circle(0.0, 30.0, 0.5), &ProfileIn::point(P3::new(0.0, 0.0, 40.0)), &LoftOpts::default()).unwrap();
        assert!(flatten(&b.model).is_ok());
    }
    #[test]
    fn rectangle_to_circle_with_bends_on_flattens() {
        for (offset, tol, bends) in [(0.0, 1.0, true), (std::f64::consts::FRAC_PI_4, 1.0, true), (0.0, 4.0, false), (0.0, 4.0, true)] {
            let n = arc_pieces(30.0, std::f64::consts::TAU, tol);
            let c = ProfileIn {
                points: (0..n).map(|i| {
                    let a = i as f64 / n as f64 * std::f64::consts::TAU + offset;
                    P3::new(30.0 * a.cos(), 30.0 * a.sin(), 60.0)
                }).collect(),
                closed: true,
            };
            let p = Params { thickness: 1.5, bend_radius: 2.0, ..Default::default() };
            let b = loft(p, &rect(0.0, 100.0, 80.0), &c, &LoftOpts { bends, ..Default::default() }).unwrap();
            let flat = flatten(&b.model);
            let names: Vec<_> = b.model.joints.iter().filter(|j| !matches!(j.kind, JointKind::Tangent { .. })).map(|j| j.name.clone()).collect();
            assert!(flat.is_ok(), "offset {offset} tol {tol} bends {bends}: {:?} joints {names:?} warnings {:?}", flat.errors.iter().map(|e| e.message()).collect::<Vec<_>>(), b.warnings);
        }
    }
}
