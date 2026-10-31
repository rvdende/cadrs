//! Ray–face intersection, point-in-face and point-in-solid classification.

use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Face, co_range};

/// Where a (u, v) point lies relative to a face's trimmed domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InFace {
    Inside,
    Outside,
    Boundary,
}

/// A face's loops sampled as uv polygons (for in-face tests).
pub fn face_polygons(body: &Body, f: &Face) -> Vec<Vec<P2>> {
    f.loops
        .iter()
        .map(|l| {
            let mut pts = Vec::new();
            for c in l {
                let e = &body.edges[c.edge];
                let (a, b) = co_range(e, c.fwd);
                let n = match &c.pc {
                    super::curve::Curve::Line { .. } => 1,
                    _ => {
                        // Enough points that the polygon is within ~1e-6 of the p-curve.
                        let span = (b - a).abs();
                        ((span * 256.0).ceil() as usize).clamp(16, 4096)
                    }
                };
                for i in 0..n {
                    let t = a + (b - a) * i as f64 / n as f64;
                    let p = c.pc.point(t);
                    pts.push(P2::new(p.x, p.y));
                }
            }
            pts
        })
        .collect()
}

/// A face's [`face_polygons`] and their [`poly_boxes`].
pub type KeptPolygons = std::sync::Arc<(Vec<Vec<P2>>, PolyBoxes)>;

/// [`face_polygons`] and their [`poly_boxes`], kept by what they are made from (the face's
/// surface and each p-curve over its range): a boolean takes every face of both its bodies (in
/// its import and again to classify), most of them as the bodies before it had them.
pub fn face_polygons_kept(body: &Body, f: &Face) -> KeptPolygons {
    use std::hash::Hasher;
    /// Two generations of at most this many points each (16 bytes a point, and the boxes).
    const KEPT: usize = 1 << 21;
    #[derive(Default)]
    struct Kept {
        new: rustc_hash::FxHashMap<u128, KeptPolygons>,
        new_points: usize,
        old: rustc_hash::FxHashMap<u128, KeptPolygons>,
    }
    static KEPT_POLYS: std::sync::LazyLock<std::sync::Mutex<Kept>> =
        std::sync::LazyLock::new(Default::default);
    let mut h = super::fingerprint::Key128::new();
    h.surface(&f.surf);
    for l in &f.loops {
        h.write_usize(l.len());
        for c in l {
            let (a, b) = co_range(&body.edges[c.edge], c.fwd);
            h.f64s(&[a, b]);
            h.curve(&c.pc);
        }
    }
    let key = h.key();
    let points = |p: &KeptPolygons| p.0.iter().map(Vec::len).sum::<usize>() + 1;
    let kept = {
        let mut k = KEPT_POLYS.lock().unwrap_or_else(|e| e.into_inner());
        match k.new.get(&key).cloned() {
            Some(p) => Some(p),
            None => k.old.remove(&key).inspect(|p| {
                k.new_points += points(p);
                k.new.insert(key, p.clone());
            }),
        }
    };
    kept.unwrap_or_else(|| {
        let polys = face_polygons(body, f);
        let boxes = poly_boxes(&polys, &f.surf);
        let p: KeptPolygons = std::sync::Arc::new((polys, boxes));
        let mut k = KEPT_POLYS.lock().unwrap_or_else(|e| e.into_inner());
        if k.new_points + points(&p) > KEPT {
            k.old = std::mem::take(&mut k.new);
            k.new_points = 0;
        }
        k.new_points += points(&p);
        k.new.insert(key, p.clone());
        p
    })
}

/// Point in a face's domain given its sampled loops; `tol` in model units (scaled by the
/// surface metric).
pub fn in_face(polys: &[Vec<P2>], surf: &Surface, uv: P2, tol: f64) -> InFace {
    let (mu, mv) = surf.metric();
    let mut inside = false;
    for p in polys {
        let (lo, hi) = uv_range(std::slice::from_ref(p));
        let uv = shifted_into(surf, uv, lo, hi);
        let q = P2::new(uv.x * mu, uv.y * mv);
        let n = p.len();
        for i in 0..n {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let (a, b) = (P2::new(a.x * mu, a.y * mv), P2::new(b.x * mu, b.y * mv));
            if seg_dist2(&q, &a, &b) < tol {
                return InFace::Boundary;
            }
        }
        if point_in_polygon(&uv, p) {
            inside = !inside;
        }
    }
    if inside {
        InFace::Inside
    } else {
        InFace::Outside
    }
}

/// A face's polygons with their boxes, for [`in_face`] on many points: in (u, v) and in the
/// surface's metric (as [`in_face`] scales them), each polygon's and all of them together.
pub struct FacePolys<'a> {
    polys: &'a [Vec<P2>],
    /// Per polygon: its (u, v) box and its scaled box ([`PolyBoxes`]).
    boxes: PolyBoxes,
    range: ((f64, f64), (f64, f64)),
}

/// A face's polygon prepared for point tests: its (u, v) box and its box in the
/// metric-scaled (u, v), as (min x, max x, min y, max y), and, for a long polygon, its sides
/// bucketed by rows of v (a point's tests then look at the sides of its rows only).
#[derive(Debug, Clone)]
pub struct PolyBox {
    b: [f64; 4],
    m: [f64; 4],
    rows: Option<Rows>,
}

/// A polygon's sides (by their first point's index) in rows of equal height over its v range:
/// row k's at `sides[start[k]..start[k + 1]]`, in index order.
#[derive(Debug, Clone)]
struct Rows {
    y0: f64,
    dy: f64,
    start: Vec<u32>,
    sides: Vec<u32>,
}

impl Rows {
    /// Row k's sides.
    fn row(&self, k: usize) -> &[u32] {
        &self.sides[self.start[k] as usize..self.start[k + 1] as usize]
    }

    /// The rows [y0, y1] reaches, clamped to the polygon's.
    fn span(&self, y0: f64, y1: f64) -> std::ops::RangeInclusive<usize> {
        let n = (self.start.len() - 1) as f64;
        let at = |y: f64| ((y - self.y0) / self.dy).floor().clamp(0.0, n - 1.0) as usize;
        at(y0)..=at(y1)
    }
}

/// Per polygon of a face, a [`PolyBox`]: what [`in_face_boxed`] culls by.
pub type PolyBoxes = Vec<PolyBox>;

/// The [`PolyBoxes`] of a face's polygons.
pub fn poly_boxes(polys: &[Vec<P2>], surf: &Surface) -> PolyBoxes {
    let (mu, mv) = surf.metric();
    polys
        .iter()
        .map(|p| {
            let mut b = [
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ];
            let mut m = b;
            for q in p {
                b = [b[0].min(q.x), b[1].max(q.x), b[2].min(q.y), b[3].max(q.y)];
                let (x, y) = (q.x * mu, q.y * mv);
                m = [m[0].min(x), m[1].max(x), m[2].min(y), m[3].max(y)];
            }
            let n = p.len();
            let rows = (n > 64 && b[3] > b[2]).then(|| {
                let count = (n / 8).clamp(1, 256);
                let mut r = Rows {
                    y0: b[2],
                    dy: (b[3] - b[2]) / count as f64,
                    start: vec![0; count + 1],
                    sides: Vec::new(),
                };
                let spans: Vec<std::ops::RangeInclusive<usize>> = (0..n)
                    .map(|i| {
                        let (a, c) = (p[i], p[(i + 1) % n]);
                        r.span(a.y.min(c.y), a.y.max(c.y))
                    })
                    .collect();
                for sp in &spans {
                    for k in sp.clone() {
                        r.start[k + 1] += 1;
                    }
                }
                for k in 0..count {
                    r.start[k + 1] += r.start[k];
                }
                let mut fill = r.start.clone();
                r.sides = vec![0; r.start[count] as usize];
                for (i, sp) in spans.into_iter().enumerate() {
                    for k in sp {
                        r.sides[fill[k] as usize] = i as u32;
                        fill[k] += 1;
                    }
                }
                r
            });
            PolyBox { b, m, rows }
        })
        .collect()
}

/// [`in_face`], the same answer: a polygon whose boxes (`boxes`, [`poly_boxes`] of `polys`)
/// the point lies well outside (in the metric beyond `tol` of every side, and in (u, v)
/// outside, where the crossings to its right are even) can neither hold it on its boundary nor
/// flip its parity, and is skipped; of a long polygon only the sides in the point's rows (those
/// that come within `tol` of it, and those that cross its v) are looked at.
pub fn in_face_boxed(
    polys: &[Vec<P2>],
    boxes: &[PolyBox],
    surf: &Surface,
    uv: P2,
    tol: f64,
) -> InFace {
    let (mu, mv) = surf.metric();
    let mut inside = false;
    for (p, pb) in polys.iter().zip(boxes) {
        let (b, m) = (&pb.b, &pb.m);
        let uv = shifted_into(surf, uv, (b[0], b[1]), (b[2], b[3]));
        let q = P2::new(uv.x * mu, uv.y * mv);
        let gm = tol * (1.0 + tol::REL)
            + tol::REL_FINE
                * (1.0
                    + q.x.abs().max(q.y.abs())
                    + m[1].abs().max(m[0].abs()).max(m[3].abs()).max(m[2].abs()));
        let gb = tol::REL_FINE
            * (1.0
                + uv.x.abs().max(uv.y.abs())
                + b[1].abs().max(b[0].abs()).max(b[3].abs()).max(b[2].abs()));
        let far_m = q.x < m[0] - gm || q.x > m[1] + gm || q.y < m[2] - gm || q.y > m[3] + gm;
        let far_b = uv.x < b[0] - gb || uv.x > b[1] + gb || uv.y < b[2] - gb || uv.y > b[3] + gb;
        if far_m && far_b {
            continue;
        }
        let n = p.len();
        let side = |i: usize| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            (P2::new(a.x * mu, a.y * mv), P2::new(b.x * mu, b.y * mv))
        };
        let Some(rows) = &pb.rows else {
            for i in 0..n {
                let (a, b) = side(i);
                if seg_dist2(&q, &a, &b) < tol {
                    return InFace::Boundary;
                }
            }
            if point_in_polygon(&uv, p) {
                inside = !inside;
            }
            continue;
        };
        // (The rows within `tol` of the point in v, a little more for rounding: a side outside
        // them is farther than `tol` everywhere.)
        let reach = gm / mv.max(tol::TINY) + gb;
        for k in rows.span(uv.y - reach, uv.y + reach) {
            for &i in rows.row(k) {
                let (a, b) = side(i as usize);
                if seg_dist2(&q, &a, &b) < tol {
                    return InFace::Boundary;
                }
            }
        }
        // Parity: the sides crossing the point's v, all in its row.
        let mut odd = false;
        for k in rows.span(uv.y, uv.y) {
            for &i in rows.row(k) {
                let (a, c) = (p[i as usize], p[(i as usize + 1) % n]);
                if (a.y > uv.y) != (c.y > uv.y) {
                    let x = a.x + (uv.y - a.y) / (c.y - a.y) * (c.x - a.x);
                    if uv.x < x {
                        odd = !odd;
                    }
                }
            }
        }
        if odd {
            inside = !inside;
        }
    }
    if inside {
        InFace::Inside
    } else {
        InFace::Outside
    }
}

impl<'a> FacePolys<'a> {
    pub fn new(polys: &'a [Vec<P2>], surf: &Surface) -> Self {
        Self::with_boxes(polys, poly_boxes(polys, surf))
    }

    /// [`Self::new`] with the polygons' boxes made already ([`poly_boxes`]).
    pub fn with_boxes(polys: &'a [Vec<P2>], boxes: PolyBoxes) -> Self {
        FacePolys {
            polys,
            boxes,
            range: uv_range(polys),
        }
    }

    /// [`periodic_candidates`] for these polygons.
    pub fn candidates(&self, s: &Surface, u: f64, v: f64) -> Vec<P2> {
        periodic_candidates_in(s, u, v, self.range)
    }

    /// [`in_face`], the same answer, culling by the polygons' boxes ([`in_face_boxed`]).
    pub fn in_face(&self, surf: &Surface, uv: P2, tol: f64) -> InFace {
        in_face_boxed(self.polys, &self.boxes, surf, uv, tol)
    }
}

/// Parameters t where the line o + t·d meets the surface (all of them, any sign), with the
/// (u, v) there.
pub fn ray_surface(s: &Surface, o: &P3, d: &V3, tmin: f64, tmax: f64) -> Vec<(f64, f64, f64)> {
    let mut ts: Vec<f64> = Vec::new();
    match s {
        Surface::Plane { f } => {
            let n = f.x.cross(&f.y);
            let den = n.dot(d);
            if den.abs() > 1e-15 {
                ts.push((f.o - o).dot(&n) / den);
            }
        }
        Surface::Cylinder { f, r } => {
            let l = f.local(o);
            let ld = V3::new(d.dot(&f.x), d.dot(&f.y), d.dot(&f.z));
            ts = quadratic(
                ld.x * ld.x + ld.y * ld.y,
                2.0 * (l.x * ld.x + l.y * ld.y),
                l.x * l.x + l.y * l.y - r * r,
            );
            if ld.x * ld.x + ld.y * ld.y < 1e-24 {
                ts.clear();
            }
        }
        Surface::Sphere { f, r } => {
            let l = o - f.o;
            ts = quadratic(d.norm_squared(), 2.0 * l.dot(d), l.norm_squared() - r * r);
        }
        Surface::Cone { f, r, a } => {
            // ρ cos a − z sin a − r cos a = 0 ⇔ ρ = r + z tan a (on the cone's sheet).
            let l = f.local(o);
            let ld = V3::new(d.dot(&f.x), d.dot(&f.y), d.dot(&f.z));
            let ta = a.tan();
            // x² + y² = (r + z tan a)²
            let (c0, c1) = (*r + l.z * ta, ld.z * ta);
            let qa = ld.x * ld.x + ld.y * ld.y - c1 * c1;
            let qb = 2.0 * (l.x * ld.x + l.y * ld.y - c0 * c1);
            let qc = l.x * l.x + l.y * l.y - c0 * c0;
            for t in quadratic(qa, qb, qc) {
                // Keep the sheet where r + z tan a >= 0.
                if c0 + c1 * t >= -tol::ROUNDOFF {
                    ts.push(t);
                }
            }
        }
        Surface::Extrusion { c, d: dir } => {
            // The plane through the ray parallel to the extrusion cuts it in rulings where the
            // base curve crosses that plane; each ruling meets the ray at one point.
            let m = d.cross(dir);
            if m.norm() < tol::DEGENERATE {
                return Vec::new();
            }
            let fr = Frame::from_z(*o, m.normalize());
            let pl = Surface::Plane { f: fr };
            let (a, b) = c.nurbs_range().unwrap_or((0.0, TAU));
            for u in super::ssi::curve_surface(c, a, b, &pl) {
                let q = c.point(u);
                // o + t d = q + v dir: solve in least squares.
                let w = q - o;
                let (dd, dr, rr) = (d.dot(d), d.dot(dir), dir.dot(dir));
                let (wd, wr) = (w.dot(d), w.dot(dir));
                if let Some((t, _v)) = solve2(dd, -dr, dr, -rr, wd, wr) {
                    ts.push(t);
                }
            }
        }
        Surface::Nurbs(_) | Surface::Revolution { .. } => {
            return ray_parametric(s, o, d);
        }
        _ => {
            // Implicit along the ray where available, else sampled distance with projection.
            let n = 400;
            let f = |t: f64| -> f64 {
                let p = o + d * t;
                match s.signed_distance(&p) {
                    Some(x) => x,
                    None => {
                        let (u, v) = s.project(&p, None);
                        let (q, su, sv) = s.d1(u, v);
                        (p - q).dot(&su.cross(&sv).normalize())
                    }
                }
            };
            let (lo, hi) = (tmin, tmax);
            let mut prev = (lo, f(lo));
            for i in 1..=n {
                let t = lo + (hi - lo) * i as f64 / n as f64;
                let ft = f(t);
                if prev.1 == 0.0 {
                    ts.push(prev.0);
                } else if prev.1.signum() != ft.signum() {
                    // Bisect.
                    let (mut a, mut b, mut fa) = (prev.0, t, prev.1);
                    for _ in 0..80 {
                        let m = 0.5 * (a + b);
                        let fm = f(m);
                        if fm.signum() == fa.signum() {
                            a = m;
                            fa = fm;
                        } else {
                            b = m;
                        }
                    }
                    let m = 0.5 * (a + b);
                    // Reject sign flips across the far side of a projection jump.
                    let p = o + d * m;
                    if s.distance(&p) < tol::POINT {
                        ts.push(m);
                    }
                }
                prev = (t, ft);
            }
        }
    }
    let mut out = Vec::new();
    for t in ts {
        let p = o + d * t;
        let (u, v) = s.project(&p, None);
        out.push((t, u, v));
    }
    // Torus: the quadric cases above miss it; handled by the sampled branch.
    out
}

/// Line–surface hits on a parametric surface: the ray against a triangulated grid of the
/// surface's parameter box, each hit refined by Newton on S(u, v) = o + t·d.
fn ray_parametric(s: &Surface, o: &P3, d: &V3) -> Vec<(f64, f64, f64)> {
    let ((u0, u1), (v0, v1)) = match s {
        Surface::Revolution { m, .. } => ((0.0, TAU), m.nurbs_range().unwrap_or((-1e3, 1e3))),
        _ => s.param_box(),
    };
    let n = 32;
    let at = |i: usize, j: usize| {
        (
            u0 + (u1 - u0) * i as f64 / n as f64,
            v0 + (v1 - v0) * j as f64 / n as f64,
        )
    };
    let mut grid = vec![P3::origin(); (n + 1) * (n + 1)];
    for i in 0..=n {
        for j in 0..=n {
            let (u, v) = at(i, j);
            grid[i * (n + 1) + j] = s.eval(u, v);
        }
    }
    let mut out: Vec<(f64, f64, f64)> = Vec::new();
    let tri = |a: &P3, b: &P3, c: &P3| -> Option<(f64, f64, f64)> {
        // Möller–Trumbore with a small margin; returns (t, β, γ).
        let (e1, e2) = (b - a, c - a);
        let h = d.cross(&e2);
        let det = e1.dot(&h);
        if det.abs() < tol::TINY {
            return None;
        }
        let f = 1.0 / det;
        let sv = o - a;
        let bu = f * sv.dot(&h);
        let q = sv.cross(&e1);
        let bv = f * d.dot(&q);
        let m = 0.05;
        if bu < -m || bv < -m || bu + bv > 1.0 + m {
            return None;
        }
        Some((f * e2.dot(&q), bu, bv))
    };
    for i in 0..n {
        for j in 0..n {
            let (p00, p10, p01, p11) = (
                grid[i * (n + 1) + j],
                grid[(i + 1) * (n + 1) + j],
                grid[i * (n + 1) + j + 1],
                grid[(i + 1) * (n + 1) + j + 1],
            );
            let (ua, va) = at(i, j);
            let (ub, vb) = at(i + 1, j + 1);
            for (k, (a, b, c)) in [(p00, p10, p11), (p00, p11, p01)].into_iter().enumerate() {
                let Some((t, bu, bv)) = tri(&a, &b, &c) else {
                    continue;
                };
                // Seed (u, v) inside the cell.
                let (mut u, mut v) = if k == 0 {
                    (ua + (ub - ua) * (bu + bv), va + (vb - va) * bv)
                } else {
                    (ua + (ub - ua) * bu, va + (vb - va) * (bu + bv))
                };
                let mut tt = t;
                let mut ok = false;
                for _ in 0..30 {
                    let (q, su, sv) = s.d1(u, v);
                    let r = q - (o + d * tt);
                    if r.norm() < tol::DEGENERATE {
                        ok = true;
                        break;
                    }
                    let m = nalgebra::Matrix3::from_columns(&[su, sv, -d]);
                    let Some(inv) = m.try_inverse() else { break };
                    let dx = inv * (-r);
                    u += dx.x;
                    v += dx.y;
                    tt += dx.z;
                    if dx.norm() < 1e-14 {
                        ok = (s.eval(u, v) - (o + d * tt)).norm() < tol::ROUNDOFF;
                        break;
                    }
                }
                if ok && !out.iter().any(|(x, ..)| (x - tt).abs() < tol::ROUNDOFF) {
                    out.push((tt, u, v));
                }
            }
        }
    }
    out
}

/// Rays cast (parity votes and [`ray_hits`]), and points classified, since the start: the counts
/// `CADRS_PROFILE` reports per boolean (research/algorithms/booleans.md, "Measure first").
pub(crate) static RAYS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static POINTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Every crossing of the line with the faces of the body: (t, face, point), sorted by t.
pub fn ray_hits(body: &Body, polys: &[Vec<Vec<P2>>], o: &P3, d: &V3) -> Vec<(f64, usize, P3)> {
    let all: Vec<usize> = (0..body.faces.len()).collect();
    ray_hits_among(body, polys, &all, None, o, d)
}

/// [`ray_hits`] with the faces `among` only, each skipped where the line keeps off its box
/// (`boxes`, the faces' [`Body::face_bbox`]es by face index, when given).
pub fn ray_hits_among(
    body: &Body,
    polys: &[Vec<Vec<P2>>],
    among: &[usize],
    boxes: Option<&[Bbox]>,
    o: &P3,
    d: &V3,
) -> Vec<(f64, usize, P3)> {
    RAYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let bb = body.bbox().grow(1.0);
    let span = bb.diagonal() + (bb.center() - o).norm();
    let mut out = Vec::new();
    for &fi in among {
        let f = &body.faces[fi];
        if let Some(bx) = boxes
            && !line_meets_box(o, d, &bx[fi].grow(tol::POINT + tol::REL * span))
        {
            continue;
        }
        for (t, u, v) in ray_surface(&f.surf, o, d, -span, span) {
            let uvs = periodic_candidates(&f.surf, u, v, &polys[fi]);
            for uv in uvs {
                if in_face(&polys[fi], &f.surf, uv, tol::ROUNDOFF) != InFace::Outside {
                    out.push((t, fi, o + d * t));
                    break;
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Hits on a shared edge are reported once per face; keep them (OCCT reports both too) but
    // drop exact duplicates on the same face.
    out.dedup_by(|a, b| a.1 == b.1 && (a.0 - b.0).abs() < tol::ROUNDOFF);
    out
}

/// Whether the whole line through `o` along `d` meets the box.
fn line_meets_box(o: &P3, d: &V3, b: &Bbox) -> bool {
    if b.is_empty() {
        return false;
    }
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for k in 0..3 {
        if d[k].abs() < tol::TINY {
            if o[k] < b.min[k] || o[k] > b.max[k] {
                return false;
            }
            continue;
        }
        let (t0, t1) = ((b.min[k] - o[k]) / d[k], (b.max[k] - o[k]) / d[k]);
        lo = lo.max(t0.min(t1));
        hi = hi.min(t0.max(t1));
        if lo > hi {
            return false;
        }
    }
    true
}

/// The (u, v) shifted by whole periods into the face's uv box.
pub fn periodic_candidates(s: &Surface, u: f64, v: f64, polys: &[Vec<P2>]) -> Vec<P2> {
    periodic_candidates_in(s, u, v, uv_range(polys))
}

/// [`periodic_candidates`] for the polygons of `boxes` ([`poly_boxes`]): their range from
/// their boxes (the same as from their points, for none of the points' passes).
pub fn periodic_candidates_boxed(s: &Surface, u: f64, v: f64, boxes: &[PolyBox]) -> Vec<P2> {
    let mut r = (
        (f64::INFINITY, f64::NEG_INFINITY),
        (f64::INFINITY, f64::NEG_INFINITY),
    );
    for pb in boxes {
        r = (
            (r.0.0.min(pb.b[0]), r.0.1.max(pb.b[1])),
            (r.1.0.min(pb.b[2]), r.1.1.max(pb.b[3])),
        );
    }
    periodic_candidates_in(s, u, v, r)
}

/// `uv` moved by whole turns along the surface's periodic directions into the box (lo: the u
/// range, hi: the v range) of one loop's polygon, where a turn brings it there: a loop of a
/// periodic face may lie across the u range of another (a hole straddling the seam, its polygon
/// running below u = 0 while the outer loop's runs from 0 to 2π), and a point is in it when any
/// of its turns is.
fn shifted_into(s: &Surface, uv: P2, (ulo, uhi): (f64, f64), (vlo, vhi): (f64, f64)) -> P2 {
    let fit = |x: f64, lo: f64, hi: f64, periodic: bool| {
        if !periodic || (x >= lo - tol::ROUNDOFF && x <= hi + tol::ROUNDOFF) {
            return x;
        }
        (-2..=2)
            .map(|k| x + TAU * k as f64)
            .find(|y| *y >= lo - tol::ROUNDOFF && *y <= hi + tol::ROUNDOFF)
            .unwrap_or(x)
    };
    P2::new(
        fit(uv.x, ulo, uhi, s.u_periodic()),
        fit(uv.y, vlo, vhi, s.v_periodic()),
    )
}

/// The (u, v) ranges the polygons span.
fn uv_range(polys: &[Vec<P2>]) -> ((f64, f64), (f64, f64)) {
    let mut ub = (f64::INFINITY, f64::NEG_INFINITY);
    let mut vb = (f64::INFINITY, f64::NEG_INFINITY);
    for p in polys {
        for q in p {
            ub = (ub.0.min(q.x), ub.1.max(q.x));
            vb = (vb.0.min(q.y), vb.1.max(q.y));
        }
    }
    (ub, vb)
}

/// [`periodic_candidates`] with the polygons' (u, v) ranges known.
fn periodic_candidates_in(
    s: &Surface,
    u: f64,
    v: f64,
    (ub, vb): ((f64, f64), (f64, f64)),
) -> Vec<P2> {
    let us: Vec<f64> = if s.u_periodic() {
        let mut v = Vec::new();
        for k in -2..=2 {
            let x = u + TAU * k as f64;
            if x >= ub.0 - tol::ROUNDOFF && x <= ub.1 + tol::ROUNDOFF {
                v.push(x);
            }
        }
        if v.is_empty() {
            v.push(u);
        }
        v
    } else {
        vec![u]
    };
    let vs: Vec<f64> = if s.v_periodic() {
        let mut w = Vec::new();
        for k in -2..=2 {
            let x = v + TAU * k as f64;
            if x >= vb.0 - tol::ROUNDOFF && x <= vb.1 + tol::ROUNDOFF {
                w.push(x);
            }
        }
        if w.is_empty() {
            w.push(v);
        }
        w
    } else {
        vec![v]
    };
    let mut out = Vec::new();
    for &a in &us {
        for &b in &vs {
            out.push(P2::new(a, b));
        }
    }
    out
}

/// Inside / outside / on the boundary of the body's solids. (Many points against one body:
/// [`Classifier`].)
pub fn classify_point(body: &Body, polys: &[Vec<Vec<P2>>], p: &P3, tol: f64) -> crate::PointClass {
    Classifier::new(body, polys).classify(p, tol)
}

/// Ray parity, by the majority of a few rays (two that agree, else the first side to three),
/// with retries in other directions when a ray grazes an edge or a face. (Many points against
/// one body: [`Classifier`].)
pub fn parity_inside(body: &Body, polys: &[Vec<Vec<P2>>], p: &P3) -> bool {
    Classifier::new(body, polys).parity_inside(p)
}

/// A body prepared for classifying many points: its faces' boxes (rays and points only meet
/// the faces whose boxes they reach), its own box, and a start inside each face's domain for
/// the projections.
pub struct Classifier<'a> {
    body: &'a Body,
    polys: Vec<FacePolys<'a>>,
    boxes: Vec<Bbox>,
    bbox: Bbox,
    starts: Vec<Option<(f64, f64)>>,
}

impl<'a> Classifier<'a> {
    /// The body with its faces' sampled loops ([`face_polygons`], one per face).
    pub fn new(body: &'a Body, polys: &'a [Vec<Vec<P2>>]) -> Self {
        let faces = polys
            .iter()
            .zip(&body.faces)
            .map(|(p, f)| FacePolys::new(p, &f.surf))
            .collect();
        Self::with(body, polys.iter().map(Vec::as_slice), faces)
    }

    /// [`Self::new`] with the polygons and their boxes made already ([`face_polygons_kept`]).
    pub fn with_kept(body: &'a Body, kept: &'a [KeptPolygons]) -> Self {
        let faces = kept
            .iter()
            .map(|k| FacePolys::with_boxes(&k.0, k.1.clone()))
            .collect();
        Self::with(body, kept.iter().map(|k| k.0.as_slice()), faces)
    }

    fn with(
        body: &'a Body,
        polys: impl Iterator<Item = &'a [Vec<P2>]>,
        faces: Vec<FacePolys<'a>>,
    ) -> Self {
        let boxes: Vec<Bbox> = (0..body.faces.len()).map(|fi| body.face_bbox(fi)).collect();
        let mut bbox = Bbox::empty();
        for b in &boxes {
            bbox.union(b);
        }
        for v in &body.verts {
            bbox.add(&v.p);
        }
        let starts = polys
            .map(|ps| {
                let mut uv = Bbox::empty();
                ps.iter()
                    .flatten()
                    .for_each(|q| uv.add(&P3::new(q.x, q.y, 0.0)));
                (!uv.is_empty()).then(|| (uv.center().x, uv.center().y))
            })
            .collect();
        Classifier {
            body,
            polys: faces,
            boxes,
            bbox,
            starts,
        }
    }

    /// The (u, v) on face `fi`'s surface nearest `p`: from the face's start, and from the
    /// surface's own search when that stops more than `tol` away.
    fn project(&self, fi: usize, p: &P3, tol: f64) -> (f64, f64) {
        let s = &self.body.faces[fi].surf;
        if let Some(h) = self.starts[fi] {
            let (u, v) = s.project(p, Some(h));
            if (s.eval(u, v) - p).norm() <= tol {
                return (u, v);
            }
        }
        s.project(p, None)
    }

    /// Inside / outside / on the boundary of the body's solids (on a face within `tol`).
    pub fn classify(&self, p: &P3, tol: f64) -> crate::PointClass {
        POINTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tol = tol.max(tol::ROUNDOFF);
        for (fi, f) in self.body.faces.iter().enumerate() {
            if !self.boxes[fi].grow(tol).contains(p) {
                continue;
            }
            let (u, v) = self.project(fi, p, tol);
            if (f.surf.eval(u, v) - p).norm() <= tol {
                for uv in self.polys[fi].candidates(&f.surf, u, v) {
                    if self.polys[fi].in_face(&f.surf, uv, tol) != InFace::Outside {
                        return crate::PointClass::OnBoundary;
                    }
                }
            }
        }
        if self.parity_inside(p) {
            crate::PointClass::Inside
        } else {
            crate::PointClass::Outside
        }
    }

    /// Ray parity, by the majority of a few rays (two that agree, else the first side to
    /// three), with retries in other directions when a ray grazes an edge or a face.
    #[allow(
        clippy::approx_constant,
        reason = "ray directions picked off the axes and diagonals, not constants"
    )]
    pub fn parity_inside(&self, p: &P3) -> bool {
        // Five fixed directions, then 32 more spread over the sphere (a golden-angle spiral)
        // while every ray so far grazed a face or ran through an edge.
        let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
        let dirs: Vec<V3> = [
            V3::new(0.5773, 0.5774, 0.5772),
            V3::new(-0.3014, 0.8123, 0.4993),
            V3::new(0.7071, -0.1213, -0.6966),
            V3::new(0.1234, 0.2345, -0.9641),
            V3::new(-0.8, -0.35, 0.487),
        ]
        .into_iter()
        .chain((0..32).map(|i| {
            let z = 1.0 - (i as f64 + 0.5) / 16.0;
            let r = (1.0 - z * z).max(0.0).sqrt();
            let a = golden * i as f64 + 0.1;
            V3::new(r * a.cos(), r * a.sin(), z)
        }))
        .collect();
        let bb = self.bbox.grow(1.0);
        let span = bb.diagonal() + (bb.center() - p).norm();
        let mut votes = (0, 0);
        for d in dirs {
            RAYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let d = d.normalize();
            let mut count = 0;
            let mut bad = false;
            'faces: for (fi, f) in self.body.faces.iter().enumerate() {
                if !ray_meets_box(p, &d, &self.boxes[fi].grow(tol::REL * (1.0 + span))) {
                    continue;
                }
                for (t, u, v) in ray_surface(&f.surf, p, &d, -span, span) {
                    if t <= tol::ROUNDOFF_FINE {
                        continue;
                    }
                    // Grazing: the ray nearly tangent to the surface.
                    let n = f.surf.normal(u, v);
                    if n.dot(&d).abs() < tol::ANGULAR_POINT {
                        bad = true;
                        break 'faces;
                    }
                    for uv in self.polys[fi].candidates(&f.surf, u, v) {
                        match self.polys[fi].in_face(&f.surf, uv, 1e-7) {
                            InFace::Inside => {
                                count += 1;
                                break;
                            }
                            InFace::Boundary => {
                                bad = true;
                                break 'faces;
                            }
                            InFace::Outside => {}
                        }
                    }
                }
            }
            if bad {
                continue;
            }
            if count % 2 == 1 {
                votes.0 += 1;
            } else {
                votes.1 += 1;
            }
            // Two rays that agree, else the first side to three: a ray can miss a crossing
            // (near the boundary of a face's polygon, nearly along a B-spline face) and so count
            // the wrong parity.
            if (votes.0.min(votes.1) == 0 && votes.0.max(votes.1) >= 2) || votes.0.max(votes.1) >= 3
            {
                break;
            }
        }
        if votes.0 + votes.1 == 0 {
            // Every ray grazed: by the side of the nearest face.
            return self.nearest_face_inside(p);
        }
        votes.0 > votes.1
    }

    /// Whether `p` lies behind the nearest face (against its outward normal there).
    fn nearest_face_inside(&self, p: &P3) -> bool {
        let mut best: Option<(f64, V3, P3)> = None;
        for (fi, f) in self.body.faces.iter().enumerate() {
            let (u, v) = self.project(fi, p, 0.0);
            let q = f.surf.eval(u, v);
            let d = (q - p).norm();
            if best.is_none_or(|b| d < b.0) {
                let n = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
                best = Some((d, n, q));
            }
        }
        best.is_some_and(|(_, n, q)| (p - q).dot(&n) < 0.0)
    }
}

/// Whether the ray from `o` along `d` (t > 0) meets the box.
fn ray_meets_box(o: &P3, d: &V3, b: &Bbox) -> bool {
    if b.is_empty() {
        return false;
    }
    let (mut lo, mut hi) = (0.0f64, f64::INFINITY);
    for k in 0..3 {
        if d[k].abs() < tol::TINY {
            if o[k] < b.min[k] || o[k] > b.max[k] {
                return false;
            }
            continue;
        }
        let (t0, t1) = ((b.min[k] - o[k]) / d[k], (b.max[k] - o[k]) / d[k]);
        lo = lo.max(t0.min(t1));
        hi = hi.min(t0.max(t1));
        if lo > hi {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boxed polygons answer as [`in_face`] does, on points inside, outside, near and on the
    /// boundary of an outer polygon with a hole, on a plane and a (scaled) cylinder.
    #[test]
    fn boxed_polygons_match_in_face() {
        let outer: Vec<P2> = (0..40)
            .map(|i| {
                let a = i as f64 * TAU / 40.0;
                P2::new(
                    2.0 + 1.5 * a.cos() * (1.0 + 0.3 * (3.0 * a).sin()),
                    0.5 + a.sin(),
                )
            })
            .collect();
        let hole: Vec<P2> = (0..12)
            .map(|i| {
                let a = -(i as f64) * TAU / 12.0;
                P2::new(2.2 + 0.3 * a.cos(), 0.6 + 0.2 * a.sin())
            })
            .collect();
        let polys = vec![outer, hole];
        let f = Frame::world();
        for surf in [Surface::Plane { f }, Surface::Cylinder { f, r: 7.0 }] {
            let fp = FacePolys::new(&polys, &surf);
            let mut seed = 12345u64;
            let mut rnd = || {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (seed >> 11) as f64 / (1u64 << 53) as f64
            };
            let mut pts: Vec<P2> = (0..4000)
                .map(|_| P2::new(-1.0 + 6.0 * rnd(), -2.0 + 5.0 * rnd()))
                .collect();
            // On and just off the polygons' corners and sides.
            for p in &polys {
                for w in p.windows(2) {
                    let m = P2::from((w[0].coords + w[1].coords) * 0.5);
                    pts.extend([w[0], m, m + V2::new(1e-8, 0.0), m + V2::new(0.0, -3e-8)]);
                }
            }
            for uv in pts {
                for tol in [1e-7, 1e-3] {
                    assert_eq!(
                        fp.in_face(&surf, uv, tol),
                        in_face(&polys, &surf, uv, tol),
                        "{uv:?} tol {tol}"
                    );
                }
                assert_eq!(
                    fp.candidates(&surf, uv.x, uv.y),
                    periodic_candidates(&surf, uv.x, uv.y, &polys)
                );
            }
        }
    }

    /// A hole of a cylinder's face straddling its seam: its polygon runs from u = −0.5 to 0.5
    /// while the outer loop's runs from 0 to 2π. Points in the hole on either side of the seam
    /// are outside the face, by either in-face test; points away from it inside.
    #[test]
    fn hole_across_the_seam() {
        let surf = Surface::Cylinder {
            f: Frame::world(),
            r: 1.0,
        };
        let outer = vec![
            P2::new(0.0, 0.0),
            P2::new(TAU, 0.0),
            P2::new(TAU, 1.0),
            P2::new(0.0, 1.0),
        ];
        let hole = vec![
            P2::new(-0.5, 0.4),
            P2::new(-0.5, 0.6),
            P2::new(0.5, 0.6),
            P2::new(0.5, 0.4),
        ];
        let polys = vec![outer, hole];
        let fp = FacePolys::new(&polys, &surf);
        for (uv, want) in [
            ((0.2, 0.5), InFace::Outside),
            ((TAU - 0.2, 0.5), InFace::Outside),
            ((PI, 0.5), InFace::Inside),
            ((0.2, 0.2), InFace::Inside),
            ((TAU - 0.2, 0.8), InFace::Inside),
        ] {
            let uv = P2::new(uv.0, uv.1);
            assert_eq!(in_face(&polys, &surf, uv, 1e-9), want, "{uv:?}");
            assert_eq!(fp.in_face(&surf, uv, 1e-9), want, "{uv:?}");
        }
    }
}

#[cfg(test)]
mod boxed_tests {
    use super::*;

    /// The boxed and row-bucketed point test answers as the plain one, over a long polygon (a
    /// wavy outline of 2000 points) with small holes, at points inside, outside, on and near the
    /// boundary.
    #[test]
    fn boxed_in_face_matches_in_face() {
        let s = Surface::Plane { f: Frame::world() };
        let outer: Vec<P2> = (0..2000)
            .map(|i| {
                let a = TAU * i as f64 / 2000.0;
                let r = 10.0 + 0.5 * (7.0 * a).sin();
                P2::new(r * a.cos(), r * a.sin())
            })
            .collect();
        let mut polys = vec![outer];
        for k in 0..6 {
            let c = P2::new(-6.0 + 2.4 * k as f64, 0.5);
            polys.push(
                (0..100)
                    .rev()
                    .map(|i| {
                        let a = TAU * i as f64 / 100.0;
                        P2::new(c.x + 0.8 * a.cos(), c.y + 0.8 * a.sin())
                    })
                    .collect(),
            );
        }
        let boxes = poly_boxes(&polys, &s);
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut pts: Vec<P2> = (0..4000)
            .map(|_| P2::new(-13.0 + 26.0 * rnd(), -13.0 + 26.0 * rnd()))
            .collect();
        // On and a hair off the sides.
        pts.extend(polys.iter().flat_map(|p| {
            p.iter()
                .step_by(37)
                .flat_map(|q| [*q, P2::new(q.x + 1e-8, q.y), P2::new(q.x, q.y - 3e-7)])
        }));
        for q in pts {
            for tol in [1e-7, 1e-3] {
                assert_eq!(
                    in_face_boxed(&polys, &boxes, &s, q, tol),
                    in_face(&polys, &s, q, tol),
                    "at {q:?}, tol {tol}"
                );
            }
        }
    }
}
