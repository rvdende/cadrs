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
    let fine = polygon_fineness(&f.surf);
    f.loops
        .iter()
        .map(|l| loop_polygon(body, l, fine))
        .collect()
}

/// How far, in (u, v), a face's polygons may run from a B-spline p-curve: a hundredth of a
/// micron's hundred on the surface (1e-5 mm), by its metric. (Finer costs every in-face test
/// its points: at 1e-6 a raster of 112 passes took a quarter longer.)
fn polygon_fineness(s: &Surface) -> f64 {
    let (mu, mv) = s.metric();
    // (A plane's spline boundaries ten times finer: two spline outlines a few microns apart, a
    // prism's cap and its copy's moved by a hair, are told apart by their polygons, which have
    // no exact curve to fall back on as a circle's do. Only a plane's: on spline surfaces the
    // extra points cost a CNC raster a third of its time.)
    let k = if matches!(s, Surface::Plane { .. }) {
        0.1
    } else {
        1.0
    };
    k * 1e-5 / mu.max(mv).max(tol::TINY)
}

/// One of a face's loops sampled as a uv polygon ([`face_polygons`]).
fn loop_polygon(body: &Body, l: &[super::topo::Coedge], fine: f64) -> Vec<P2> {
    // (Its points counted first: a circle's are hundreds, and a vector grown to them is
    // copied over and over.)
    let steps: Vec<usize> = l
        .iter()
        .map(|c| {
            let (a, b) = co_range(&body.edges[c.edge], c.fwd);
            coedge_steps(&c.pc, a, b, fine)
        })
        .collect();
    let mut pts = Vec::with_capacity(steps.iter().sum());
    for (c, &n) in l.iter().zip(&steps) {
        let e = &body.edges[c.edge];
        let (a, b) = co_range(e, c.fwd);
        for i in 0..n {
            let t = a + (b - a) * i as f64 / n as f64;
            let p = c.pc.point(t);
            pts.push(P2::new(p.x, p.y));
        }
    }
    pts
}

/// Straight steps to a radian of a circular or elliptical p-curve: 403 round a whole circle,
/// its chords within 3e-5 of its radius of it. (A point between the chords and the curve is
/// put right by the curve itself, [`ArcPiece`]; at 256 steps a radian a circle's polygon was
/// 1609 points, and a small solid's faces' polygons most of what it allocated.)
const ARC_STEPS: f64 = 64.0;

/// How many straight steps [`loop_polygon`] takes a coedge's p-curve in over [a, b]: one for a
/// line, else by its parameter's span. (The polygon then runs up to a circle's sagitta inside
/// it, 2e-6 of its radius over a whole turn: a point between the two is put right by the
/// circle itself, [`ArcPiece`].)
fn coedge_steps(pc: &super::curve::Curve, a: f64, b: f64, fine: f64) -> usize {
    match pc {
        super::curve::Curve::Line { .. } => 1,
        // A B-spline by how it bends, not by how its parameter runs: a chord over a step h
        // of the parameter is off the curve by |C″| h²/8 at most, C″ taken at a few places
        // along it. (256 points to a unit of parameter gave a spline over [0, 1] 256 whatever
        // its length, and one over [0, 1000] the most.)
        super::curve::Curve::Nurbs(_) => {
            const LOOKS: usize = 16;
            let bend = (0..=LOOKS)
                .map(|i| pc.d2(a + (b - a) * i as f64 / LOOKS as f64).2.norm())
                .fold(0.0, f64::max);
            let steps = (b - a).abs() * (bend / (8.0 * fine.max(tol::TINY))).sqrt();
            (steps.ceil() as usize).clamp(16, 4096)
        }
        _ => (((b - a).abs() * ARC_STEPS).ceil() as usize).clamp(16, 4096),
    }
}

/// A circular p-curve of a loop, as [`loop_polygon`] stepped it: the circle, its range in the
/// loop's direction and the steps. A point between the circle and its chords is inside the
/// polygon and outside the circle, or the other way round; [`in_face_boxed`] asks the circle.
#[derive(Debug, Clone)]
struct ArcPiece {
    c: P2,
    x: V2,
    y: V2,
    r: f64,
    /// An ellipse's second radius (along `y`; `r` is the one along `x`), a circle's `r` again.
    /// An ellipse is asked as the unit circle it is with each axis measured by its radius:
    /// which side of a chord a point is on is the same there.
    r2: f64,
    t: [f64; 2],
    n: usize,
    /// How far the chords run inside the curve at most.
    sag: f64,
}

impl ArcPiece {
    fn point(&self, t: f64) -> P2 {
        self.c + self.x * (t.cos() * self.r) + self.y * (t.sin() * self.r2)
    }

    /// [`Self::amiss`] of an ellipse: in the frame its radii are the units of, where it is
    /// the unit circle (and its chords the chords of that).
    fn amiss_ellipse(&self, uv: &P2, tol: f64) -> Option<bool> {
        let w = uv - self.c;
        let q = V2::new(w.dot(&self.x) / self.r, w.dot(&self.y) / self.r2);
        let rho = q.norm();
        let least = self.r.min(self.r2);
        let step = (self.t[1] - self.t[0]).abs() / self.n as f64;
        let sag = 1.0 - (step / 2.0).cos();
        if (rho - 1.0).abs() > sag + tol / least {
            return Some(false);
        }
        let (lo, span) = (self.t[0].min(self.t[1]), (self.t[1] - self.t[0]).abs());
        let along = wrap(q.y.atan2(q.x) - lo, 0.0, TAU);
        if along > span {
            return Some(false);
        }
        if (rho - 1.0).abs() < tol / least {
            return None;
        }
        let u = if self.t[1] >= self.t[0] {
            along / span
        } else {
            1.0 - along / span
        };
        let i = ((u * self.n as f64).floor() as usize).min(self.n - 1);
        let at = |k: usize| {
            let t = self.t[0] + (self.t[1] - self.t[0]) * k as f64 / self.n as f64;
            V2::new(t.cos(), t.sin())
        };
        let (a, b) = (at(i), at(i + 1));
        let centre_side = cross2(&(b - a), &(-a));
        let own_side = cross2(&(b - a), &(q - a));
        Some((own_side * centre_side > 0.0) != (rho < 1.0))
    }

    /// Whether `uv`, in this arc's sector, is on the other side of the circle from the side
    /// of its chord it is on (`Some(true)`: the polygon has it wrong), or on the circle within
    /// `tol` (`None`). `Some(false)` off the arc's sector, or where the two agree.
    fn amiss(&self, uv: &P2, tol: f64) -> Option<bool> {
        if self.r2 != self.r {
            return self.amiss_ellipse(uv, tol);
        }
        let w = uv - self.c;
        let rho = w.norm();
        if (rho - self.r).abs() > self.sag + tol {
            return Some(false);
        }
        let (lo, span) = (self.t[0].min(self.t[1]), (self.t[1] - self.t[0]).abs());
        let along = wrap(w.dot(&self.y).atan2(w.dot(&self.x)) - lo, 0.0, TAU);
        if along > span {
            return Some(false);
        }
        if (rho - self.r).abs() < tol {
            return None;
        }
        // (The step the point's angle falls in, counted the way the loop runs.)
        let u = if self.t[1] >= self.t[0] {
            along / span
        } else {
            1.0 - along / span
        };
        let i = ((u * self.n as f64).floor() as usize).min(self.n - 1);
        let at =
            |k: usize| self.point(self.t[0] + (self.t[1] - self.t[0]) * k as f64 / self.n as f64);
        let (a, b) = (at(i), at(i + 1));
        let centre_side = cross2(&(b - a), &(self.c - a));
        let own_side = cross2(&(b - a), &(uv - a));
        Some((own_side * centre_side > 0.0) != (rho < self.r))
    }
}

/// The circular and elliptical p-curves of a loop ([`ArcPiece`]), on a surface whose (u, v) is measured the
/// same both ways (a circle there is one in the metric too).
fn loop_arcs(body: &Body, l: &[super::topo::Coedge]) -> Vec<ArcPiece> {
    l.iter()
        .filter_map(|c| {
            let (o, x, y, r, r2) = match &c.pc {
                super::curve::Curve::Circle { c: o, x, y, r } => (o, x, y, *r, *r),
                super::curve::Curve::Ellipse { c: o, x, y, a, b } => (o, x, y, *a, *b),
                _ => return None,
            };
            let (a, b) = co_range(&body.edges[c.edge], c.fwd);
            let n = coedge_steps(&c.pc, a, b, 0.0);
            Some(ArcPiece {
                c: P2::new(o.x, o.y),
                x: V2::new(x.x, x.y),
                y: V2::new(y.x, y.y),
                r,
                r2,
                t: [a, b],
                n,
                sag: r.max(r2) * (1.0 - ((b - a).abs() / (2.0 * n as f64)).cos()),
            })
        })
        .collect()
}

/// A face's [`face_polygons`] and their [`poly_boxes`].
pub type KeptPolygons = std::sync::Arc<(Vec<Vec<P2>>, PolyBoxes)>;

/// [`face_polygons`] and their [`poly_boxes`], kept by what they are made from (the face's
/// surface and each p-curve over its range): a boolean takes every face of both its bodies (in
/// its import and again to classify), most of them as the bodies before it had them. Each loop
/// is kept too, so a face that gained a loop since (a plate's face, a hole cut in it one after
/// another) samples only the new one.
pub fn face_polygons_kept(body: &Body, f: &Face) -> KeptPolygons {
    use std::hash::Hasher;
    /// Two generations of at most this many points each (16 bytes a point, and the boxes).
    const KEPT: usize = 1 << 21;
    type KeptLoop = std::sync::Arc<(Vec<P2>, PolyBox)>;
    #[derive(Default)]
    struct Kept {
        new: rustc_hash::FxHashMap<u128, KeptPolygons>,
        new_loops: rustc_hash::FxHashMap<u128, KeptLoop>,
        new_points: usize,
        old: rustc_hash::FxHashMap<u128, KeptPolygons>,
        old_loops: rustc_hash::FxHashMap<u128, KeptLoop>,
    }
    impl Kept {
        /// Counts `points` more kept, starting a new generation first when they don't fit.
        fn room(&mut self, points: usize) {
            if self.new_points + points > KEPT {
                let new = std::mem::take(&mut self.new);
                super::par::discard(std::mem::replace(&mut self.old, new));
                let loops = std::mem::take(&mut self.new_loops);
                super::par::discard(std::mem::replace(&mut self.old_loops, loops));
                self.new_points = 0;
            }
            self.new_points += points;
        }
    }
    // (Read by every face of both bodies of every boolean, on as many threads as run
    // booleans at once, and nearly always found: readers don't wait for one another.)
    static KEPT_POLYS: std::sync::LazyLock<std::sync::RwLock<Kept>> =
        std::sync::LazyLock::new(|| {
            // (Emptied by [`crate::native::clear_caches`].)
            super::par::kept_cache(|| {
                *KEPT_POLYS.write().unwrap_or_else(|e| e.into_inner()) = Default::default();
            });
            Default::default()
        });
    // Each loop's key (the surface and the loop's p-curves over their ranges), and the face's
    // (the loops' in order).
    let loop_keys: Vec<u128> = f
        .loops
        .iter()
        .map(|l| {
            let mut h = super::fingerprint::Key128::new();
            h.surface(&f.surf);
            h.write_usize(l.len());
            for c in l {
                let (a, b) = co_range(&body.edges[c.edge], c.fwd);
                h.f64s(&[a, b]);
                h.curve(&c.pc);
            }
            h.key()
        })
        .collect();
    let key = {
        let mut h = super::fingerprint::Key128::new();
        h.write_usize(loop_keys.len());
        for k in &loop_keys {
            h.write_u128(*k);
        }
        h.key()
    };
    let points = |p: &KeptPolygons| p.0.iter().map(Vec::len).sum::<usize>() + 1;
    if let Some(p) = KEPT_POLYS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .new
        .get(&key)
        .cloned()
    {
        return p;
    }
    let mut k = KEPT_POLYS.write().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = k.new.get(&key).cloned() {
        return p;
    }
    if let Some(p) = k.old.remove(&key) {
        k.room(points(&p));
        k.new.insert(key, p.clone());
        return p;
    }
    // The face's loops, each kept or sampled now (the lock let go meanwhile: sampling a long
    // loop takes a while).
    let mut kept: Vec<Option<KeptLoop>> = loop_keys
        .iter()
        .map(|lk| {
            k.new_loops.get(lk).cloned().or_else(|| {
                let p = k.old_loops.remove(lk)?;
                k.room(p.0.len() + 1);
                k.new_loops.insert(*lk, p.clone());
                Some(p)
            })
        })
        .collect();
    drop(k);
    let mut made: Vec<(u128, KeptLoop)> = Vec::new();
    for (i, slot) in kept.iter_mut().enumerate() {
        if slot.is_none() {
            let poly = loop_polygon(body, &f.loops[i], polygon_fineness(&f.surf));
            let mut pb = poly_boxes(std::slice::from_ref(&poly), &f.surf)
                .pop()
                .expect("a box per polygon");
            let (mu, mv) = f.surf.metric();
            if mu == mv {
                pb.arcs = loop_arcs(body, &f.loops[i]);
                pb.bulge = pb.arcs.iter().fold(0.0, |m, a| m.max(a.sag));
            }
            let l: KeptLoop = std::sync::Arc::new((poly, pb));
            made.push((loop_keys[i], l.clone()));
            *slot = Some(l);
        }
    }
    let (polys, boxes): (Vec<Vec<P2>>, PolyBoxes) = kept
        .into_iter()
        .flatten()
        .map(|l| (l.0.clone(), l.1.clone()))
        .unzip();
    let p: KeptPolygons = std::sync::Arc::new((polys, boxes));
    let mut k = KEPT_POLYS.write().unwrap_or_else(|e| e.into_inner());
    for (lk, l) in made {
        k.room(l.0.len() + 1);
        k.new_loops.insert(lk, l);
    }
    k.room(points(&p));
    k.new.insert(key, p.clone());
    p
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
    boxes: std::borrow::Cow<'a, PolyBoxes>,
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
    /// The loop's circular p-curves, where it was made from a loop ([`face_polygons_kept`]),
    /// and how far the polygon runs inside them at most.
    arcs: Vec<ArcPiece>,
    bulge: f64,
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

impl PolyBox {
    /// The polygon's (u, v) box: (min u, max u, min v, max v).
    pub fn uv_box(&self) -> [f64; 4] {
        self.b
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
            PolyBox {
                b,
                m,
                rows,
                arcs: Vec::new(),
                bulge: 0.0,
            }
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
        // A point between a circular edge and the chords it was stepped in is on the wrong
        // side of the polygon (which may not even reach it): the circle says which.
        if !pb.arcs.is_empty() {
            let reach = pb.bulge + tol / mu.max(tol::TINY) + gb;
            if uv.x >= b[0] - reach
                && uv.x <= b[1] + reach
                && uv.y >= b[2] - reach
                && uv.y <= b[3] + reach
            {
                for arc in &pb.arcs {
                    match arc.amiss(&uv, tol / mu.max(tol::TINY)) {
                        None => return InFace::Boundary,
                        Some(true) => inside = !inside,
                        Some(false) => {}
                    }
                }
            }
        }
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
            boxes: std::borrow::Cow::Owned(boxes),
            range: uv_range(polys),
        }
    }

    /// [`Self::with_boxes`] of polygons kept elsewhere, with their range.
    fn kept(k: &'a KeptPolygons, range: ((f64, f64), (f64, f64))) -> Self {
        FacePolys {
            polys: &k.0,
            boxes: std::borrow::Cow::Borrowed(&k.1),
            range,
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
    ray_surface_with(s, o, d, tmin, tmax, None)
}

/// [`ray_surface`], with a parametric surface's grid of points ([`RayGrid`]) kept in `grid`
/// for the next ray at the same surface, where given.
fn ray_surface_with(
    s: &Surface,
    o: &P3,
    d: &V3,
    tmin: f64,
    tmax: f64,
    grid: Option<&std::sync::OnceLock<RayGrid>>,
) -> Vec<(f64, f64, f64)> {
    let mut ts: Vec<f64> = Vec::new();
    let mut hits: Vec<(f64, f64, f64)> = Vec::new();
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
        Surface::Torus { f, rmaj, rmin } => ts = ray_torus(s, f, *rmaj, *rmin, o, d, tmin, tmax),
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
            let us = match c {
                super::curve::Curve::Nurbs(n) => nurbs_plane_crossings(n, &m.normalize(), o),
                _ => super::ssi::curve_surface(c, a, b, &pl),
            };
            let exact = matches!(c, super::curve::Curve::Nurbs(_));
            for u in us {
                let q = c.point(u);
                // o + t d = q + v dir: solve in least squares.
                let w = q - o;
                let (dd, dr, rr) = (d.dot(d), d.dot(dir), dir.dot(dir));
                let (wd, wr) = (w.dot(d), w.dot(dir));
                if let Some((t, v)) = solve2(dd, -dr, dr, -rr, wd, wr) {
                    if exact {
                        // (The surface's own parameters: no projection.)
                        hits.push((t, u, v));
                    } else {
                        ts.push(t);
                    }
                }
            }
        }
        Surface::Nurbs(_) | Surface::Revolution { .. } => {
            if let Surface::Nurbs(ns) = s
                && let Some(hits) = ray_ruled(ns, o, d).or_else(|| ray_graph(ns, o, d))
            {
                return hits;
            }
            if let Surface::Revolution {
                f,
                m: super::curve::Curve::Line { o: lo, d: ld },
            } = s
                && let Some(hits) = ray_revolved_line(f, lo, ld, o, d)
            {
                return hits;
            }
            return match grid {
                Some(g) => ray_parametric(s, g.get_or_init(|| RayGrid::new(s)), o, d),
                None => ray_parametric(s, &RayGrid::new(s), o, d),
            };
        }
    }
    let mut out = hits;
    for t in ts {
        let p = o + d * t;
        let (u, v) = s.project(&p, None);
        out.push((t, u, v));
    }
    out
}

/// A ray against a ruled B-spline (degree 1 and one span along v, its weights the same along
/// v: a ruled loft's sides), in closed form: the ruling at u, A(u) + s·D(u), meets the line
/// o + t·d where the two are coplanar, f(u) = (A(u) − o)·(D(u) × d) = 0, and f·w² is a B-spline
/// of the two rows' poles (its degree twice u's): sampled a span at a time, each sign change
/// bisected, then s and t from the two lines. Hits with s outside the ruling are dropped. None
/// for any other surface, or a ray nearly along a ruling somewhere (the general search then).
fn ray_ruled(ns: &super::nurbs::NurbsSurface, o: &P3, d: &V3) -> Option<Vec<(f64, f64, f64)>> {
    use super::nurbs::H;
    if ns.dv != 1 || ns.kv.len() != 4 || ns.cps.iter().any(|r| r.len() != 2) {
        return None;
    }
    if ns
        .cps
        .iter()
        .any(|r| (r[0].w - r[1].w).abs() > 1e-14 * r[0].w.abs() || !(r[0].w > 0.0))
    {
        return None;
    }
    let p = ns.du;
    let k = &ns.ku;
    let n = ns.cps.len();
    let (v0, v1) = (ns.kv[1], ns.kv[2]);
    // A row's homogeneous point at u in span j (de Boor).
    let row = |j: usize, u: f64, side: usize| -> H {
        let mut buf = [H::new(0.0, 0.0, 0.0, 0.0); 16];
        let mut heap: Vec<H>;
        let q: &mut [H] = if p < 16 {
            &mut buf[..=p]
        } else {
            heap = vec![H::new(0.0, 0.0, 0.0, 0.0); p + 1];
            &mut heap
        };
        for (r, x) in q.iter_mut().enumerate() {
            *x = ns.cps[j - p + r][side];
        }
        for r in 1..=p {
            for i in (r..=p).rev() {
                let lo = k[j - p + i];
                let hi = k[j + 1 + i - r];
                let a = if hi > lo { (u - lo) / (hi - lo) } else { 0.0 };
                q[i] = q[i - 1] * (1.0 - a) + q[i] * a;
            }
        }
        q[p]
    };
    let dn = d.norm();
    // The ruling at u: its start and direction.
    let ruling = |j: usize, u: f64| -> (P3, V3) {
        let (a, b) = (row(j, u, 0), row(j, u, 1));
        let w = a.w;
        let pa = P3::new(a.x / w, a.y / w, a.z / w);
        (pa, V3::new(b.x - a.x, b.y - a.y, b.z - a.z) / w)
    };
    let f = |j: usize, u: f64| -> Option<f64> {
        let (a, dr) = ruling(j, u);
        let m = dr.cross(d);
        if m.norm() <= 1e-9 * dr.norm() * dn {
            return None;
        }
        Some((a - o).dot(&m))
    };
    let mut out = Vec::new();
    for j in p..n {
        let (u0, u1) = (k[j], k[j + 1]);
        if u1 <= u0 {
            continue;
        }
        let m = 4 * (2 * p + 1);
        let at = |i: usize| u0 + (u1 - u0) * i as f64 / m as f64;
        let mut prev = (at(0), f(j, at(0))?);
        for i in 1..=m {
            let u = at(i);
            let cur = (u, f(j, u)?);
            let last = j + 1 == n && i == m;
            let crosses = (prev.1 < 0.0 && cur.1 > 0.0)
                || (prev.1 > 0.0 && cur.1 < 0.0)
                || prev.1 == 0.0
                || (last && cur.1 == 0.0);
            if crosses {
                let root = if prev.1 == 0.0 {
                    prev.0
                } else if cur.1 == 0.0 {
                    cur.0
                } else {
                    let (mut a, mut b, mut fa) = (prev.0, cur.0, prev.1);
                    for _ in 0..60 {
                        let mid = 0.5 * (a + b);
                        let fm = f(j, mid)?;
                        if fm == 0.0 || (b - a) <= 1e-15 * (1.0 + mid.abs()) {
                            a = mid;
                            b = mid;
                            break;
                        }
                        if (fm < 0.0) == (fa < 0.0) {
                            a = mid;
                            fa = fm;
                        } else {
                            b = mid;
                        }
                    }
                    0.5 * (a + b)
                };
                // o + t d = A + s D.
                let (a, dr) = ruling(j, root);
                let w = a - o;
                let (dd, ds, ss) = (d.dot(d), d.dot(&dr), dr.dot(&dr));
                let (wd, ws) = (w.dot(d), w.dot(&dr));
                if let Some((t, sr)) = solve2(dd, -ds, ds, -ss, wd, ws)
                    && (-1e-12..=1.0 + 1e-12).contains(&sr)
                    && out.last().is_none_or(|l: &(f64, f64, f64)| {
                        (root - l.1).abs() > 1e-14 * (1.0 + root.abs())
                    })
                {
                    out.push((t, root, v0 + (v1 - v0) * sr));
                }
            }
            prev = cur;
        }
    }
    Some(out)
}

/// A height field: a non-rational B-spline whose poles stand over its Greville abscissae on a
/// plane, `c + g_u·a + g_v·b + h·n` (a, b in the plane, n square to it), so that its point at
/// (u, v) lies over `c + u·a + v·b` (linear precision) at height n·S(u, v). Its frame `(c, a, b,
/// n)`, or None for any other surface (or a flat one).
fn graph_frame(ns: &super::nurbs::NurbsSurface) -> Option<(P3, V3, V3, V3)> {
    let (nu, nv) = (ns.cps.len(), ns.cps.first()?.len());
    if nu < 2 || nv < 2 || ns.cps.iter().flatten().any(|h| h.w != 1.0) {
        return None;
    }
    let greville = |k: &[f64], p: usize, n: usize| -> Vec<f64> {
        (0..n)
            .map(|i| k[i + 1..=i + p].iter().sum::<f64>() / p.max(1) as f64)
            .collect()
    };
    let pt = |i: usize, j: usize| P3::new(ns.cps[i][j].x, ns.cps[i][j].y, ns.cps[i][j].z);
    // A quick test first (every ray and projection asks, of every B-spline face): the affine
    // part cancels in P(i, j) − P(i, 0) − P(0, j) + P(0, 0), which must lie along n for every
    // pole; three spread over the grid, all along one line, or this is no height field.
    {
        let dd = |i: usize, j: usize| pt(i, j) - pt(i, 0) - (pt(0, j) - pt(0, 0));
        let probes = [dd(nu - 1, nv - 1), dd(nu / 2, nv - 1), dd(nu - 1, nv / 2)];
        let size = (pt(nu - 1, nv - 1) - pt(0, 0)).norm().max(tol::TINY);
        let far = pt(0, 0).coords.abs().max();
        let e = 1e-9 * (size + far);
        let big = probes.iter().max_by(|a, b| a.norm().total_cmp(&b.norm()))?;
        if big.norm() > e {
            let n = big.normalize();
            if probes.iter().any(|d| d.cross(&n).norm() > e) {
                return None;
            }
        }
    }
    let (gu, gv) = (greville(&ns.ku, ns.du, nu), greville(&ns.kv, ns.dv, nv));
    // The affine part by least squares over the grid (separable: means, then slopes).
    let m = (nu * nv) as f64;
    let (mu, mv) = (
        gu.iter().sum::<f64>() / nu as f64,
        gv.iter().sum::<f64>() / nv as f64,
    );
    let mut c = V3::zeros();
    let (mut a, mut b) = (V3::zeros(), V3::zeros());
    let (mut suu, mut svv) = (0.0, 0.0);
    for (i, x) in gu.iter().enumerate() {
        for (j, y) in gv.iter().enumerate() {
            let q = pt(i, j).coords;
            c += q / m;
            a += q * (x - mu);
            b += q * (y - mv);
            suu += (x - mu).powi(2);
            svv += (y - mv).powi(2);
        }
    }
    if !(suu > 0.0 && svv > 0.0) {
        return None;
    }
    let (a, b) = (a / suu, b / svv);
    let c = P3::from(c - a * mu - b * mv);
    // What the affine part leaves must all lie along one direction n.
    let size = (pt(nu - 1, nv - 1) - pt(0, 0)).norm().max(tol::TINY);
    let res: Vec<V3> = (0..nu)
        .flat_map(|i| (0..nv).map(move |j| (i, j)))
        .map(|(i, j)| pt(i, j) - (c + a * gu[i] + b * gv[j]))
        .collect();
    let big = res.iter().max_by(|x, y| x.norm().total_cmp(&y.norm()))?;
    if big.norm() <= 1e-9 * size {
        return None;
    }
    let n = big.normalize();
    // (Rounding: a moved surface's poles carry their place's size in their last digits.)
    let far = ns.cps.iter().flatten().fold(0.0f64, |m, h| {
        m.max(h.x.abs()).max(h.y.abs()).max(h.z.abs())
    });
    if res
        .iter()
        .any(|r| r.cross(&n).norm() > 1e-12 * (size + far))
    {
        return None;
    }
    // a and b in the plane square to n (their parts along n are height, linear in u and v).
    let (a, b) = (a - n * a.dot(&n), b - n * b.dot(&n));
    if a.cross(&b).norm() <= 1e-12 * a.norm() * b.norm() {
        return None;
    }
    let c = c - n * (c - P3::origin()).dot(&n);
    Some((c, a, b, n))
}

/// A ray against a height field ([`graph_frame`]) exactly: seen along n the ray is a line in
/// (u, v), where its height and the surface's differ by f(t), a polynomial between the points
/// it crosses the knot lines; each sign change is bisected. A ray along n meets it once, over
/// its foot. None for any other surface.
fn ray_graph(ns: &super::nurbs::NurbsSurface, o: &P3, d: &V3) -> Option<Vec<(f64, f64, f64)>> {
    let (c, a, b, n) = graph_frame(ns)?;
    let ((u0, u1), (v0, v1)) = ns.range();
    // (u, v) of a point's foot: solve c + u a + v b = foot (a 2 × 2 in the plane).
    let (aa, ab, bb) = (a.dot(&a), a.dot(&b), b.dot(&b));
    let det = aa * bb - ab * ab;
    let uv = |w: V3| -> (f64, f64) {
        let (wa, wb) = (w.dot(&a), w.dot(&b));
        ((wa * bb - wb * ab) / det, (wb * aa - wa * ab) / det)
    };
    let (q0, q1) = (uv(o - c), uv(*d - n * d.dot(&n)));
    let height = |u: f64, v: f64| (ns.point(u, v) - c).dot(&n);
    let mut out = Vec::new();
    let span = (u1 - u0).max(v1 - v0);
    if q1.0.abs().max(q1.1.abs()) <= 1e-12 * span {
        // Along n: the one point over the ray's foot.
        let (u, v) = q0;
        let dn = d.dot(&n);
        if dn == 0.0 || u < u0 || u > u1 || v < v0 || v > v1 {
            return Some(out);
        }
        out.push(((height(u, v) - (o - c).dot(&n)) / dn, u, v));
        return Some(out);
    }
    // The t range over the domain, then its cuts at the knot lines.
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for (p0, p1, a0, a1) in [(q0.0, q1.0, u0, u1), (q0.1, q1.1, v0, v1)] {
        if p1 == 0.0 {
            if p0 < a0 || p0 > a1 {
                return Some(out);
            }
            continue;
        }
        let (ta, tb) = ((a0 - p0) / p1, (a1 - p0) / p1);
        lo = lo.max(ta.min(tb));
        hi = hi.min(ta.max(tb));
    }
    if !(lo < hi) {
        return Some(out);
    }
    let mut ts = vec![lo, hi];
    for (p0, p1, k) in [(q0.0, q1.0, &ns.ku), (q0.1, q1.1, &ns.kv)] {
        if p1 != 0.0 {
            ts.extend(
                k.iter()
                    .map(|&x| (x - p0) / p1)
                    .filter(|&t| t > lo && t < hi),
            );
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup();
    let at = |t: f64| (q0.0 + q1.0 * t, q0.1 + q1.1 * t);
    let f = |t: f64| {
        let (u, v) = at(t);
        let (u, v) = (u.clamp(u0, u1), v.clamp(v0, v1));
        height(u, v) - (o + d * t - c).dot(&n)
    };
    let m = 2 * (ns.du + ns.dv) + 2;
    for w in ts.windows(2) {
        let (ta, tb) = (w[0], w[1]);
        let mut prev = (ta, f(ta));
        for i in 1..=m {
            let t = ta + (tb - ta) * i as f64 / m as f64;
            let cur = (t, f(t));
            let crosses = (prev.1 < 0.0) != (cur.1 < 0.0) || prev.1 == 0.0;
            if crosses {
                // Regula falsi, Illinois style (the end kept twice running has its value
                // halved): bracketed like bisection, near Newton's speed on a smooth root.
                let (mut x, mut y, mut fx, mut fy) = (prev.0, cur.0, prev.1, cur.1);
                let mut side = 0i8;
                let mut t = x;
                for _ in 0..100 {
                    if fx == 0.0 {
                        t = x;
                        break;
                    }
                    if fy == 0.0 {
                        t = y;
                        break;
                    }
                    t = (x * fy - y * fx) / (fy - fx);
                    if !(t > x.min(y) && t < x.max(y)) {
                        t = 0.5 * (x + y);
                    }
                    if (y - x).abs() <= 1e-15 * (1.0 + t.abs()) {
                        break;
                    }
                    let ft = f(t);
                    if ft == 0.0 {
                        break;
                    }
                    if (ft < 0.0) == (fx < 0.0) {
                        x = t;
                        fx = ft;
                        if side == -1 {
                            fy *= 0.5;
                        }
                        side = -1;
                    } else {
                        y = t;
                        fy = ft;
                        if side == 1 {
                            fx *= 0.5;
                        }
                        side = 1;
                    }
                    if ft.abs() <= 1e-15 * (1.0 + t.abs()) {
                        break;
                    }
                }
                if out
                    .last()
                    .is_none_or(|l: &(f64, f64, f64)| (t - l.0).abs() > 1e-12 * (1.0 + t.abs()))
                {
                    let (u, v) = at(t);
                    out.push((t, u.clamp(u0, u1), v.clamp(v0, v1)));
                }
            }
            prev = cur;
        }
    }
    Some(out)
}

/// Where a B-spline curve crosses the plane through `o` with unit normal `n`: the sign changes
/// of g(u) = n·(C(u) − o)·w(u), itself a B-spline whose coefficients are the control points'
/// (homogeneous) distances. Only a knot span whose coefficients change sign can hold one
/// (the variation-diminishing property); there g is sampled, and each sign change bisected to
/// rounding. A touch without a crossing is not one (a ray's parity is unchanged by it).
fn nurbs_plane_crossings(c: &super::nurbs::NurbsCurve, n: &V3, o: &P3) -> Vec<f64> {
    let p = c.degree;
    let k = &c.knots;
    let no = n.dot(&o.coords);
    let g: Vec<f64> = c
        .cps
        .iter()
        .map(|h| n.x * h.x + n.y * h.y + n.z * h.z - h.w * no)
        .collect();
    let scale = g.iter().fold(0.0f64, |m, x| m.max(x.abs())).max(tol::TINY);
    // g at u in span j (de Boor on the scalar coefficients).
    // (On the stack up to degree 15: this runs for every ray against every extruded spline.)
    let eval = |j: usize, u: f64| -> f64 {
        let mut buf = [0.0f64; 16];
        let mut heap: Vec<f64>;
        let d: &mut [f64] = if p < 16 {
            &mut buf[..=p]
        } else {
            heap = vec![0.0; p + 1];
            &mut heap
        };
        for (r, x) in d.iter_mut().enumerate() {
            *x = g[j - p + r];
        }
        for r in 1..=p {
            for i in (r..=p).rev() {
                let lo = k[j - p + i];
                let hi = k[j + 1 + i - r];
                let a = if hi > lo { (u - lo) / (hi - lo) } else { 0.0 };
                d[i] = (1.0 - a) * d[i - 1] + a * d[i];
            }
        }
        d[p]
    };
    let mut out: Vec<f64> = Vec::new();
    let n_cp = c.cps.len();
    for j in p..n_cp {
        let (u0, u1) = (k[j], k[j + 1]);
        if u1 <= u0 {
            continue;
        }
        let co = &g[j - p..=j];
        let (mn, mx) = co
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
                (a.min(x), b.max(x))
            });
        if mn > 0.0 || mx < 0.0 {
            continue;
        }
        let m = 4 * (p + 1);
        let at = |i: usize| u0 + (u1 - u0) * i as f64 / m as f64;
        let mut prev = (at(0), eval(j, at(0)));
        for i in 1..=m {
            let u = at(i);
            let cur = (u, eval(j, u));
            // A sign change in [prev, cur): a root at its left end is its own; one at the
            // span's right end belongs to the next span (or to the curve's end, kept here).
            let last = j + 1 == n_cp && i == m;
            let crosses = (prev.1 < 0.0 && cur.1 > 0.0)
                || (prev.1 > 0.0 && cur.1 < 0.0)
                || prev.1 == 0.0
                || (last && cur.1 == 0.0);
            if crosses {
                let root = if prev.1 == 0.0 {
                    prev.0
                } else if cur.1 == 0.0 {
                    cur.0
                } else {
                    // Regula falsi, Illinois style: bracketed, near Newton's speed.
                    let (mut a, mut b, mut fa, mut fb) = (prev.0, cur.0, prev.1, cur.1);
                    let mut side = 0i8;
                    let mut t = 0.5 * (a + b);
                    for _ in 0..100 {
                        t = (a * fb - b * fa) / (fb - fa);
                        if !(t > a && t < b) {
                            t = 0.5 * (a + b);
                        }
                        if (b - a) <= 1e-15 * (1.0 + t.abs()) {
                            break;
                        }
                        let ft = eval(j, t);
                        if ft == 0.0 || ft.abs() <= 1e-15 * scale {
                            break;
                        }
                        if (ft < 0.0) == (fa < 0.0) {
                            a = t;
                            fa = ft;
                            if side == -1 {
                                fb *= 0.5;
                            }
                            side = -1;
                        } else {
                            b = t;
                            fb = ft;
                            if side == 1 {
                                fa *= 0.5;
                            }
                            side = 1;
                        }
                    }
                    t
                };
                if out
                    .last()
                    .is_none_or(|&l| root - l > 1e-14 * (1.0 + root.abs()))
                {
                    out.push(root);
                }
            }
            prev = cur;
        }
    }
    out
}

/// Parameters t in [tmin, tmax] where the line o + t·d meets the torus of `s` (frame `f`,
/// radii `rmaj` and `rmin`): the roots of the quartic (|p|² + R² − r²)² = 4R²(x² + y²) along
/// it (in the torus's frame, from the line's point nearest its centre), each then settled by
/// Newton on the torus's own distance, and kept where the point lies on it.
#[allow(clippy::too_many_arguments)]
fn ray_torus(
    s: &Surface,
    f: &Frame,
    rmaj: f64,
    rmin: f64,
    o: &P3,
    d: &V3,
    tmin: f64,
    tmax: f64,
) -> Vec<f64> {
    let e = V3::new(d.dot(&f.x), d.dot(&f.y), d.dot(&f.z));
    let a = e.norm_squared();
    if !(a > 0.0) {
        return Vec::new();
    }
    let q0 = f.local(o);
    let t0 = -q0.dot(&e) / a;
    let q = q0 + e * t0;
    let (b, c) = (
        2.0 * q.dot(&e),
        q.norm_squared() + rmaj * rmaj - rmin * rmin,
    );
    let (pa, pb, pc) = (
        e.x * e.x + e.y * e.y,
        2.0 * (q.x * e.x + q.y * e.y),
        q.x * q.x + q.y * q.y,
    );
    let r4 = 4.0 * rmaj * rmaj;
    let coeffs = [
        c * c - r4 * pc,
        2.0 * b * c - r4 * pb,
        b * b + 2.0 * a * c - r4 * pa,
        2.0 * a * b,
        a * a,
    ];
    let dist = |t: f64| s.signed_distance(&(o + d * t));
    let mut ts = Vec::new();
    for x in poly_roots(&coeffs, tmin - t0, tmax - t0) {
        let mut t = x + t0;
        // (The quartic's root holds only to the square root of its rounding where the line
        // nearly grazes the torus: a few Newton steps on the distance, taken while they help.)
        if let Some(mut g) = dist(t) {
            for _ in 0..3 {
                // (The distance's slope along the line: its gradient, the torus's normal at
                // the foot, along the line's direction.)
                let Some(slope) = s.signed_distance_gradient(&(o + d * t)).map(|n| n.dot(d)) else {
                    break;
                };
                if !(slope.abs() > 1e-6) {
                    break;
                }
                let nt = t - g / slope;
                match dist(nt) {
                    Some(ng) if ng.abs() < g.abs() => (t, g) = (nt, ng),
                    _ => break,
                }
            }
        }
        if s.distance(&(o + d * t)) < tol::POINT {
            ts.push(t);
        }
    }
    ts
}

/// Line–surface hits on the surface a line `lo + v·ld` sweeps turning about the z axis of `f` (a
/// cylinder, a cone or a hyperboloid of one sheet: a quadric), as (t, u, v) with u in [0, 2π).
/// None where the line runs nearly square to the axis (a flat ring, which the turn covers twice:
/// v is then not the height's), for [`ray_parametric`].
///
/// A point at height z on the surface is as far from the axis as the line's point at that height,
/// v = (z − a_z) / b_z (the line a + v·b in the frame), so the surface is ρ² = (a_x + v b_x)² +
/// (a_y + v b_y)², quadratic in z, and along the ray quadratic in t.
fn ray_revolved_line(f: &Frame, lo: &P3, ld: &V3, o: &P3, d: &V3) -> Option<Vec<(f64, f64, f64)>> {
    let (a, b) = (
        f.local(lo),
        V3::new(ld.dot(&f.x), ld.dot(&f.y), ld.dot(&f.z)),
    );
    if b.z.abs() < 1e-3 * b.norm() {
        return None;
    }
    let (q, e) = (f.local(o), V3::new(d.dot(&f.x), d.dot(&f.y), d.dot(&f.z)));
    // The line's point at the ray's height, g0 + t·g1 in x and y.
    let v0 = (q.z - a.z) / b.z;
    let v1 = e.z / b.z;
    let (gx0, gx1) = (a.x + v0 * b.x, v1 * b.x);
    let (gy0, gy1) = (a.y + v0 * b.y, v1 * b.y);
    let roots = quadratic(
        e.x * e.x + e.y * e.y - gx1 * gx1 - gy1 * gy1,
        2.0 * (q.x * e.x + q.y * e.y - gx0 * gx1 - gy0 * gy1),
        q.x * q.x + q.y * q.y - gx0 * gx0 - gy0 * gy0,
    );
    let mut out: Vec<(f64, f64, f64)> = Vec::with_capacity(2);
    for t in roots {
        if out.iter().any(|(x, ..)| (x - t).abs() < tol::ROUNDOFF) {
            continue;
        }
        let v = v0 + t * v1;
        let (px, py) = (q.x + t * e.x, q.y + t * e.y);
        let (mx, my) = (a.x + v * b.x, a.y + v * b.y);
        // The turn from the line's point to the ray's (any, on the axis).
        let u = (py.atan2(px) - my.atan2(mx)).rem_euclid(TAU);
        out.push((t, if u < TAU { u } else { 0.0 }, v));
    }
    Some(out)
}

/// A parametric surface's points over a grid of its parameter box, for rays against it
/// ([`ray_parametric`]): the same for every ray, so made once for the many a classifier casts.
pub(crate) struct RayGrid {
    u: (f64, f64),
    v: (f64, f64),
    n: usize,
    pts: Vec<P3>,
}

impl RayGrid {
    fn new(s: &Surface) -> Self {
        let (u, v) = match s {
            Surface::Revolution { m, .. } => ((0.0, TAU), m.nurbs_range().unwrap_or((-1e3, 1e3))),
            _ => s.param_box(),
        };
        let mut g = RayGrid {
            u,
            v,
            n: 32,
            pts: Vec::new(),
        };
        let pts = surface_grid(s, g.n, |i, j| g.at(i, j));
        g.pts = pts;
        g
    }

    /// The (u, v) of the grid's point (i, j).
    fn at(&self, i: usize, j: usize) -> (f64, f64) {
        let ((u0, u1), (v0, v1), n) = (self.u, self.v, self.n);
        (
            u0 + (u1 - u0) * i as f64 / n as f64,
            v0 + (v1 - v0) * j as f64 / n as f64,
        )
    }
}

/// Line–surface hits on a parametric surface: the ray against a triangulated grid of the
/// surface's parameter box (`g`, of `s`), each hit refined by Newton on S(u, v) = o + t·d.
fn ray_parametric(s: &Surface, g: &RayGrid, o: &P3, d: &V3) -> Vec<(f64, f64, f64)> {
    let (n, grid) = (g.n, &g.pts);
    let at = |i: usize, j: usize| g.at(i, j);
    // The grid seen along the ray: each point's place in the plane square to it, about the ray.
    // A triangle the ray goes through (within the margin below) has the ray's point in its
    // shadow there, so only cells whose shadow's box (grown for the margin) holds it are tried.
    let e1 = any_perpendicular(d).normalize();
    let e2 = d.cross(&e1).normalize();
    let shadow: Vec<[f64; 2]> = grid
        .iter()
        .map(|q| {
            let w = q - o;
            [w.dot(&e1), w.dot(&e2)]
        })
        .collect();
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
            let corners = [
                i * (n + 1) + j,
                (i + 1) * (n + 1) + j,
                i * (n + 1) + j + 1,
                (i + 1) * (n + 1) + j + 1,
            ];
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for &k in &corners {
                for a in 0..2 {
                    lo[a] = lo[a].min(shadow[k][a]);
                    hi[a] = hi[a].max(shadow[k][a]);
                }
            }
            // (Barycentric margins of m reach at most 3m of the triangle's edges past it; the
            // cell's box, its width and height, bounds them, and doubling that covers rounding.)
            let grow = 0.3 * ((hi[0] - lo[0]) + (hi[1] - lo[1])) + tol::ROUNDOFF;
            if lo[0] > grow || hi[0] < -grow || lo[1] > grow || hi[1] < -grow {
                continue;
            }
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

/// The surface's points at `at(i, j)` for i, j in 0..=n, row by row of i. A surface of revolution
/// is evaluated as [`Surface::eval`] does, to the bit, but with its meridian's points and the
/// turns' sines found once per row and column rather than once per point.
fn surface_grid(s: &Surface, n: usize, at: impl Fn(usize, usize) -> (f64, f64)) -> Vec<P3> {
    let mut grid = Vec::with_capacity((n + 1) * (n + 1));
    if let Surface::Revolution { f, m } = s {
        let ls: Vec<V3> = (0..=n).map(|j| f.local(&m.point(at(0, j).1))).collect();
        for i in 0..=n {
            let (s, c) = at(i, 0).0.sin_cos();
            grid.extend(
                ls.iter()
                    .map(|l| f.world_point(&V3::new(l.x * c - l.y * s, l.x * s + l.y * c, l.z))),
            );
        }
    } else {
        for i in 0..=n {
            grid.extend((0..=n).map(|j| {
                let (u, v) = at(i, j);
                s.eval(u, v)
            }));
        }
    }
    grid
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
    ray_hits_kept(body, polys, among, boxes, None, o, d)
}

/// [`ray_hits_among`] with each parametric face's [`RayGrid`] kept in `grids` (by face index)
/// for the next ray: a body asked about many rays makes each face's grid once.
pub(crate) fn ray_hits_kept(
    body: &Body,
    polys: &[Vec<Vec<P2>>],
    among: &[usize],
    boxes: Option<&[Bbox]>,
    grids: Option<&[std::sync::OnceLock<RayGrid>]>,
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
        let grid = grids.and_then(|g| g.get(fi));
        for (t, u, v) in ray_surface_with(&f.surf, o, d, -span, span, grid) {
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

/// The whole periods k (at least −2 to 2) with x + 2πk in the range `b`, or about it.
fn periods(x: f64, b: (f64, f64)) -> (i64, i64) {
    if !(b.0.is_finite() && b.1.is_finite() && x.is_finite()) || b.1 - b.0 > 1e4 {
        return (-2, 2);
    }
    let k0 = ((b.0 - x) / TAU).floor() as i64;
    let k1 = ((b.1 - x) / TAU).ceil() as i64;
    (k0.min(-2), k1.max(2))
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
        // (Every period the face's box reaches: a face may wind round its axis several times.)
        let (k0, k1) = periods(u, ub);
        for k in k0..=k1 {
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
        let (k0, k1) = periods(v, vb);
        for k in k0..=k1 {
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
    /// Each face's polygons: given, or ([`Self::lazy`]) made when a ray or a point first
    /// reaches the face.
    polys: Vec<FacePolys<'a>>,
    lazy: Vec<std::sync::OnceLock<LazyFace>>,
    boxes: Vec<Bbox>,
    /// The boxes in a tree (each grown by the margin a ray's or a point's test allows it), so
    /// a ray or a point meets only the faces near it, not every face of the body.
    tree: super::bvh::Bvh,
    bbox: Bbox,
    /// The box past which a point is outside whatever the tolerance asked (the body's box
    /// grown by its largest edge or vertex tolerance and the tree's margin).
    hold: Bbox,
    starts: Vec<Option<(f64, f64)>>,
    /// Each parametric face's grid for rays, made at the first ray that reaches the face.
    grids: Vec<std::sync::OnceLock<RayGrid>>,
    /// For a convex body of planes, balls, cylinders and frustums: each face and the sign that
    /// makes its surface's signed distance positive outside the body ([`Self::convex_faces`]).
    convex: std::sync::OnceLock<Option<Vec<(usize, f64)>>>,
}

/// The sign that turns `f`'s surface's signed distance positive outside the solid it bounds:
/// the face's outward normal (its surface's own, turned by its sense) against the distance's
/// gradient at a point of the face (a mirrored ball's frame turns its normal round, not its
/// distance). None where either can't be had there.
fn outward_sign(f: &Face) -> Option<f64> {
    let ((u0, u1), (v0, v1)) = f.surf.param_box();
    let (u, v) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
    let (u, v) = if u.is_finite() && v.is_finite() {
        (u, v)
    } else {
        (0.0, 0.0)
    };
    let p = f.surf.eval(u, v);
    let n = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
    let g = f.surf.signed_distance_gradient(&p)?;
    let c = n.dot(&g);
    (c.abs() > 0.5).then_some(c.signum())
}

/// Whether one closed solid `b` is convex as a closed surface convex everywhere is (Hadamard):
/// no edge concave (faces meeting at a convex edge or tangentially) and every curved face of
/// a plane, ball, cylinder or cone bulging outward ([`outward_sign`]). Unlike a classifier's
/// test, a rounded box is convex here (its rounds' cylinders don't hold the rest of it).
pub(crate) fn locally_convex(b: &Body) -> bool {
    b.solids.len() == 1
        && b.shells.len() == 1
        && b.shells[0].closed
        && b.faces.iter().all(|f| match &f.surf {
            Surface::Plane { .. } => true,
            Surface::Sphere { .. } | Surface::Cylinder { .. } | Surface::Cone { .. } => {
                outward_sign(f) == Some(1.0)
            }
            // A torus's outer half (at its major radius or beyond from its axis) curves like a
            // ball: a puck's rounded rim.
            Surface::Torus { f: fr, rmaj, rmin } if rmin < rmaj => {
                let polys = face_polygons(b, f);
                let outer = |u: f64, v: f64| {
                    let l = fr.local(&f.surf.eval(u, v));
                    l.x.hypot(l.y) >= rmaj * (1.0 - 1e-9)
                };
                // (Its loops on the outer half, and the middle of the face's own range of v:
                // a band whose circles are both outside may still run round the inside.)
                let (lo, hi) = polys
                    .iter()
                    .flatten()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                        (a.min(q.y), b.max(q.y))
                    });
                let u = polys.iter().flatten().next().map_or(0.0, |q| q.x);
                outward_sign(f) == Some(1.0)
                    && lo.is_finite()
                    && outer(u, 0.5 * (lo + hi))
                    && polys.iter().flatten().all(|q| outer(q.x, q.y))
            }
            _ => false,
        })
        && (0..b.edges.len())
            .all(|e| b.edges[e].degenerate || super::fillet::edge_convexity(b, e) != Some(false))
}

/// A face's polygons made on demand: kept, their (u, v) range and the start for projections.
type LazyFace = (KeptPolygons, ((f64, f64), (f64, f64)), Option<(f64, f64)>);

impl<'a> Classifier<'a> {
    /// The body, each face's sampled loops made only when a ray or a point reaches the face:
    /// a few points classified against a body of thousands of faces (a tool against the plate
    /// it has been cutting) sample the few faces their rays cross, not all of them.
    pub fn lazy(body: &'a Body) -> Self {
        let mut c = Self::with(body, std::iter::empty(), Vec::new());
        c.lazy = (0..body.faces.len()).map(|_| Default::default()).collect();
        c.starts = vec![None; body.faces.len()];
        c
    }

    fn lazy_face(&self, fi: usize) -> &LazyFace {
        self.lazy[fi].get_or_init(|| {
            let k = face_polygons_kept(self.body, &self.body.faces[fi]);
            let range = uv_range(&k.0);
            let mut uv = Bbox::empty();
            k.0.iter()
                .flatten()
                .for_each(|q| uv.add(&P3::new(q.x, q.y, 0.0)));
            let start = (!uv.is_empty()).then(|| (uv.center().x, uv.center().y));
            (k, range, start)
        })
    }

    /// Face `fi`'s polygons, ready for point tests.
    fn face(&self, fi: usize) -> FacePolys<'_> {
        if self.lazy.is_empty() {
            let f = &self.polys[fi];
            FacePolys {
                polys: f.polys,
                boxes: std::borrow::Cow::Borrowed(&*f.boxes),
                range: f.range,
            }
        } else {
            let (k, range, _) = self.lazy_face(fi);
            FacePolys::kept(k, *range)
        }
    }

    /// The start of projections onto face `fi`: the middle of its loops' (u, v) box.
    fn start(&self, fi: usize) -> Option<(f64, f64)> {
        if self.lazy.is_empty() {
            self.starts[fi]
        } else {
            self.lazy_face(fi).2
        }
    }

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
        // (A margin over any the tests below grow a box by: they test the face's own box
        // again.)
        let margin = tol::POINT.max(tol::REL_POINT * (1.0 + bbox.diagonal()));
        let tree = super::bvh::Bvh::new(boxes.iter().map(|b| b.grow(margin)).collect());
        let worst = body
            .edges
            .iter()
            .map(|e| e.tol)
            .chain(body.verts.iter().map(|v| v.tol))
            .fold(0.0f64, f64::max);
        let hold = bbox.grow(2.0 * worst + 2.0 * margin);
        Classifier {
            body,
            polys: faces,
            lazy: Vec::new(),
            boxes,
            tree,
            bbox,
            hold,
            starts,
            grids: (0..body.faces.len()).map(|_| Default::default()).collect(),
            convex: std::sync::OnceLock::new(),
        }
    }

    /// The body's faces with signs ([`Self::convex`]) where it is convex: one solid of planes,
    /// spheres, cylinders and cones (no apex) whose curved faces bulge outward, and every
    /// sampled point of every face's loops on or inside every face's surface (at a concave edge
    /// one face's points lie outside the other's). Then a point is inside the body exactly
    /// where it is inside every face's surface. None otherwise, or for a lazy classifier.
    fn convex_faces(&self) -> Option<&[(usize, f64)]> {
        self.convex
            .get_or_init(|| {
                let b = self.body;
                // (A cone's apex is the one point edge whose surface's signed distance runs on
                // past it: a ball's poles lie on it.)
                let apex = b.edges.iter().any(|e| e.degenerate)
                    && b.faces
                        .iter()
                        .any(|f| matches!(f.surf, Surface::Cone { .. }));
                if !self.lazy.is_empty()
                    || b.solids.len() != 1
                    || b.shells.len() != 1
                    || b.faces.is_empty()
                    || apex
                {
                    return None;
                }
                let mut sides = Vec::with_capacity(b.faces.len());
                for (fi, f) in b.faces.iter().enumerate() {
                    let sign = outward_sign(f)?;
                    // (A curved face must bulge outward: its distance outside grows outward.)
                    match &f.surf {
                        Surface::Plane { .. } => {}
                        Surface::Sphere { .. }
                        | Surface::Cylinder { .. }
                        | Surface::Cone { .. }
                            if sign > 0.0 => {}
                        _ => return None,
                    }
                    sides.push((fi, sign));
                }
                let size = self.bbox.diagonal().max(tol::POINT);
                let slack = 1e-9 * (size + self.bbox.center().coords.norm());
                let outside = |q: &P3| {
                    sides.iter().any(|&(fi, sign)| {
                        b.faces[fi]
                            .surf
                            .signed_distance(q)
                            .is_none_or(|d| d * sign > slack)
                    })
                };
                if b.verts.iter().any(|v| outside(&v.p)) {
                    return None;
                }
                for (fi, f) in b.faces.iter().enumerate() {
                    for ring in self.face(fi).polys {
                        if ring.iter().any(|q| outside(&f.surf.eval(q.x, q.y))) {
                            return None;
                        }
                    }
                }
                Some(sides)
            })
            .as_deref()
    }

    /// The (u, v) on face `fi`'s surface nearest `p`: from the face's start, and from the
    /// surface's own search when that stops more than `tol` away.
    fn project(&self, fi: usize, p: &P3, tol: f64) -> (f64, f64) {
        let s = &self.body.faces[fi].surf;
        // A height field: Newton from the point's foot, which lies over the nearest point of
        // any point within the tolerance of it (no grid search wanted).
        if let Surface::Nurbs(ns) = s
            && let Some((c, a, b, _)) = graph_frame(ns)
        {
            let (aa, ab, bb) = (a.dot(&a), a.dot(&b), b.dot(&b));
            let det = aa * bb - ab * ab;
            let w = p - c;
            let (wa, wb) = (w.dot(&a), w.dot(&b));
            let ((u0, u1), (v0, v1)) = ns.range();
            let foot = (
                ((wa * bb - wb * ab) / det).clamp(u0, u1),
                ((wb * aa - wa * ab) / det).clamp(v0, v1),
            );
            return s.project(p, Some(foot));
        }
        if let Some(h) = self.start(fi) {
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
        // Past the body's box (and the tolerance asked): outside, no ray needed.
        if !self.hold.grow(tol).contains(p) {
            return crate::PointClass::Outside;
        }
        // A whole ball or ring torus (one closed face, the whole surface): its signed distance
        // settles every point, on the boundary within the tolerance.
        if let [f] = self.body.faces.as_slice()
            && self.body.shells.len() == 1
            && self.body.shells[0].closed
            && let Some(d) = match &f.surf {
                Surface::Sphere { .. } => f.surf.signed_distance(p),
                Surface::Torus { rmaj, rmin, .. } if rmin < rmaj => f.surf.signed_distance(p),
                _ => None,
            }
            && let Some(sign) = outward_sign(f)
        {
            let d = d * sign;
            return if d > tol {
                crate::PointClass::Outside
            } else if d < -tol {
                crate::PointClass::Inside
            } else {
                crate::PointClass::OnBoundary
            };
        }
        // A convex body: inside every face's surface is inside, outside any is outside; only a
        // point within the tolerance of the boundary is settled face by face below.
        if let Some(sides) = self.convex_faces() {
            let d = sides.iter().fold(f64::NEG_INFINITY, |m, &(fi, sign)| {
                m.max(
                    self.body.faces[fi]
                        .surf
                        .signed_distance(p)
                        .map_or(f64::INFINITY, |d| d * sign),
                )
            });
            if d > tol {
                return crate::PointClass::Outside;
            }
            if d < -tol {
                return crate::PointClass::Inside;
            }
        }
        let mut near = {
            let mut at = Bbox::empty();
            at.add(p);
            self.tree.query(&at.grow(tol))
        };
        near.sort_unstable();
        for fi in near {
            let f = &self.body.faces[fi];
            if !self.boxes[fi].grow(tol).contains(p) {
                continue;
            }
            let (u, v) = self.project(fi, p, tol);
            if (f.surf.eval(u, v) - p).norm() <= tol {
                let polys = self.face(fi);
                for uv in polys.candidates(&f.surf, u, v) {
                    if polys.in_face(&f.surf, uv, tol) != InFace::Outside {
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
        let dirs = [
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
        }));
        let bb = self.bbox.grow(1.0);
        let span = bb.diagonal() + (bb.center() - p).norm();
        let mut votes = (0, 0);
        for d in dirs {
            RAYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let d = d.normalize();
            let mut count = 0;
            let mut bad = false;
            // (The faces in their own order, whatever the tree's.)
            let mut reached = self.tree.ray(p, &d, 0.0, f64::INFINITY);
            reached.sort_unstable();
            'faces: for fi in reached {
                let f = &self.body.faces[fi];
                if !ray_meets_box(p, &d, &self.boxes[fi].grow(tol::REL * (1.0 + span))) {
                    continue;
                }
                let grid = Some(&self.grids[fi]);
                for (t, u, v) in ray_surface_with(&f.surf, p, &d, -span, span, grid) {
                    if t <= tol::ROUNDOFF_FINE {
                        continue;
                    }
                    // Grazing: the ray nearly tangent to the surface.
                    let n = f.surf.normal(u, v);
                    if n.dot(&d).abs() < tol::ANGULAR_POINT {
                        bad = true;
                        break 'faces;
                    }
                    let polys = self.face(fi);
                    for uv in polys.candidates(&f.surf, u, v) {
                        match polys.in_face(&f.surf, uv, 1e-7) {
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

    /// The torus's crossings along a line are where its distance changes sign along it,
    /// found by sampling finely; each lies on the torus.
    #[test]
    fn torus_hits_match_sampling() {
        let f = Frame::from_xz(
            P3::new(0.5, -1.0, 2.0),
            V3::new(1.0, 0.3, 0.2),
            V3::new(-0.2, 0.4, 1.0),
        );
        let s = Surface::Torus {
            f,
            rmaj: 6.0,
            rmin: 2.0,
        };
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        let mut hits = 0;
        for _ in 0..300 {
            let o = f.o + V3::new(10.0 * rnd(), 10.0 * rnd(), 4.0 * rnd());
            let d = V3::new(rnd(), rnd(), 0.4 * rnd()).normalize();
            let ts: Vec<f64> = ray_surface(&s, &o, &d, -30.0, 30.0)
                .into_iter()
                .map(|h| h.0)
                .collect();
            let n = 60_000;
            let g = |t: f64| s.signed_distance(&(o + d * t)).unwrap();
            let mut flips = Vec::new();
            for i in 0..n {
                let (a, b) = (
                    -30.0 + 60.0 * i as f64 / n as f64,
                    -30.0 + 60.0 * (i + 1) as f64 / n as f64,
                );
                if g(a).signum() != g(b).signum() {
                    flips.push(0.5 * (a + b));
                }
            }
            assert_eq!(ts.len(), flips.len(), "{ts:?} {flips:?}");
            for (t, x) in ts.iter().zip(&flips) {
                assert!((t - x).abs() < 1e-3, "{ts:?} {flips:?}");
                assert!(g(*t).abs() < 1e-9, "{t}: {}", g(*t));
            }
            hits += ts.len();
        }
        assert!(hits > 200, "{hits}");
    }

    /// The closed form for a turned line finds the points the grid search does, and each (t, u,
    /// v) it gives is the surface's point on the ray: on a hyperboloid (the line skew to the
    /// axis), a cone (through it) and a cylinder (beside it), in a tilted frame.
    #[test]
    fn turned_line_hits_match_the_grid_search() {
        let f = Frame::from_xz(
            P3::new(0.3, -0.2, 0.7),
            V3::new(1.0, 0.2, -0.1),
            V3::new(0.2, 0.3, 1.0),
        );
        let lines = [
            (P3::new(1.0, 0.5, 0.2), V3::new(0.3, -0.8, 0.6)),
            (P3::new(0.9, 0.0, 0.6), V3::new(-0.5, 0.4, -1.2)),
            (P3::new(1.5, -0.7, 0.0), f.z * 2.0),
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        let mut found = 0;
        for (lo, ld) in lines {
            let s = Surface::Revolution {
                f,
                m: super::super::curve::Curve::Line { o: lo, d: ld },
            };
            for _ in 0..200 {
                let o = P3::new(3.0 * rnd(), 3.0 * rnd(), 3.0 * rnd());
                let d = V3::new(rnd(), rnd(), rnd()).normalize();
                let exact = ray_revolved_line(&f, &lo, &ld, &o, &d).expect("not a flat ring");
                for &(t, u, v) in &exact {
                    assert!((0.0..TAU).contains(&u));
                    assert!((s.eval(u, v) - (o + d * t)).norm() < 1e-9, "{t} {u} {v}");
                }
                for (t, ..) in ray_parametric(&s, &RayGrid::new(&s), &o, &d) {
                    found += 1;
                    assert!(
                        exact.iter().any(|(x, ..)| (x - t).abs() < 1e-7),
                        "{t} not in {exact:?}"
                    );
                }
            }
        }
        assert!(found > 300, "{found}");
    }
}
