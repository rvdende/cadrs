//! The solid a body fills as it moves without end about an axis ([`crate::Kernel::sweep_solid`]):
//! turned whole turns, or screwed along it.
//!
//! Either motion leaves its own family of curves in place (circles about the axis for a turn,
//! helices for a screw), so what the body fills is what its *outline* sweeps: the region it
//! covers seen along those curves. Turned, every point p of the body sweeps the circle of radius
//! r(p) at height z(p) about the axis, and the solid is the region {(r(p), z(p))} of the half
//! plane through the axis, revolved. Screwed, rising `c` per radian, every point sweeps the helix
//! through it, which meets the plane z = 0 at polar angle α(p) − z(p)/c, and the solid is that
//! plane's region {(r(p), α(p) − z(p)/c)} swept along the screw.
//!
//! The bodies taken are solids of revolution about an axis of their own whose section (the
//! meridian, in the (ρ, h) half plane of that axis) is convex: a torus (a disk section), a cone,
//! a cylinder or a wheel of cones and cylinders (a polygon). Then the body's points on any one of
//! the motion's slices (a plane z = const, or a helicoid ψ = const) form one connected region,
//! the meridian's segments where the slice cuts each half plane through the body's own axis,
//! whose distances from the axis run between their least and their greatest, both on the
//! region's edge (the meridian boundary turned). The outline is the band between those two
//! along the slices: it is traced, slice by slice, as the greatest and the least distance over
//! the section's boundary turned about its own axis (for each boundary point the turns where it
//! lies on the slice, refined to rounding by a golden-section search), from the outline's one
//! extreme slice to the other and back, and fitted by a closed cubic B-spline.

use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::Body;
use crate::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// A body of revolution: its own axis and its section's boundary in (ρ, h) about it.
struct Revolved {
    o: P3,
    a: V3,
    e1: V3,
    e2: V3,
    section: Section,
}

/// A convex meridian section: a disk (a torus's), or a convex polygon (its corners, in order).
enum Section {
    Disk { c: (f64, f64), r: f64 },
    Polygon(Vec<(f64, f64)>),
}

impl Section {
    /// The boundary's length parameter range: [0, 1).
    fn point(&self, s: f64) -> (f64, f64) {
        match self {
            Section::Disk { c, r } => {
                let t = TAU * s;
                (c.0 + r * t.cos(), c.1 + r * t.sin())
            }
            Section::Polygon(p) => {
                let n = p.len();
                let x = s.rem_euclid(1.0) * n as f64;
                let i = (x.floor() as usize).min(n - 1);
                let f = x - i as f64;
                let (a, b) = (p[i], p[(i + 1) % n]);
                (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)
            }
        }
    }

    /// Whether (ρ, h) lies inside.
    fn contains(&self, q: (f64, f64)) -> bool {
        match self {
            Section::Disk { c, r } => (q.0 - c.0).hypot(q.1 - c.1) <= *r,
            Section::Polygon(p) => {
                let n = p.len();
                let mut sign = 0.0f64;
                for i in 0..n {
                    let (a, b) = (p[i], p[(i + 1) % n]);
                    let cr = (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0);
                    if cr.abs() < 1e-14 {
                        continue;
                    }
                    if sign == 0.0 {
                        sign = cr.signum();
                    } else if cr.signum() != sign {
                        return false;
                    }
                }
                true
            }
        }
    }
}

/// The body as a solid of revolution of a convex section, if it is one: one whole torus or ball, or
/// faces on cones, cylinders and planes about one axis whose circles' (ρ, h) are the corners of
/// a convex polygon whose revolve has the body's volume.
fn revolved(b: &Body) -> Option<Revolved> {
    let frame = |o: P3, a: V3| {
        let a = a.normalize();
        let e1 = any_perpendicular(&a);
        let e2 = a.cross(&e1);
        (o, a, e1, e2)
    };
    if let [f] = b.faces.as_slice()
        && let Surface::Torus { f: fr, rmaj, rmin } = &f.surf
        && super::mass::body_volume(b) > 0.0
    {
        if !(rmin < rmaj) {
            return None;
        }
        let (o, a, e1, e2) = frame(fr.o, fr.z);
        return Some(Revolved {
            o,
            a,
            e1,
            e2,
            section: Section::Disk {
                c: (*rmaj, 0.0),
                r: *rmin,
            },
        });
    }
    // A whole ball: a disc about its centre on any axis through it.
    if let [f] = b.faces.as_slice()
        && let Surface::Sphere { f: fr, r } = &f.surf
        && super::mass::body_volume(b) > 0.0
    {
        let (o, a, e1, e2) = frame(fr.o, fr.z);
        return Some(Revolved {
            o,
            a,
            e1,
            e2,
            section: Section::Disk {
                c: (0.0, 0.0),
                r: *r,
            },
        });
    }
    // An axis from the first curved face.
    let (o, a) = b.faces.iter().find_map(|f| match &f.surf {
        Surface::Cone { f, .. } | Surface::Cylinder { f, .. } => Some((f.o, f.z.normalize())),
        _ => None,
    })?;
    let on_axis = |p: &P3, d: &V3| {
        let w = p - o;
        (w - a * w.dot(&a)).norm() < tol::REL * (1.0 + w.norm())
            && d.cross(&a).norm() < tol::ANGULAR
    };
    for f in &b.faces {
        let ok = match &f.surf {
            Surface::Cone { f, .. } | Surface::Cylinder { f, .. } => {
                on_axis(&f.o, &f.z.normalize())
            }
            Surface::Plane { f } => f.x.cross(&f.y).normalize().cross(&a).norm() < tol::ANGULAR,
            _ => false,
        };
        if !ok {
            return None;
        }
    }
    // The corners: each circle edge's (ρ, h), and points on the axis (apexes).
    let mut corners: Vec<(f64, f64)> = Vec::new();
    let mut add = |q: (f64, f64)| {
        if !corners
            .iter()
            .any(|c| (c.0 - q.0).hypot(c.1 - q.1) < 1e-9 * (1.0 + q.0.abs() + q.1.abs()))
        {
            corners.push(q);
        }
    };
    for e in &b.edges {
        match &e.curve {
            Curve::Circle { c, x, y, r } if on_axis(c, &x.cross(y)) => add((*r, (c - o).dot(&a))),
            _ => {
                for k in 0..2 {
                    let p = b.verts[e.v[k]].p;
                    let w = p - o;
                    let h = w.dot(&a);
                    add(((w - a * h).norm(), h));
                }
            }
        }
    }
    // A face square to the axis without a hole reaches the axis at its centre (a cylinder's
    // ends: its section's corners on its own axis).
    for f in &b.faces {
        if let Surface::Plane { f: fr } = &f.surf
            && f.loops.len() == 1
        {
            add((0.0, (fr.o - o).dot(&a)));
        }
    }
    if corners.len() < 3 {
        return None;
    }
    // In order round their centroid; convex.
    let (cx, cy) = (
        corners.iter().map(|c| c.0).sum::<f64>() / corners.len() as f64,
        corners.iter().map(|c| c.1).sum::<f64>() / corners.len() as f64,
    );
    corners.sort_by(|p, q| {
        (p.1 - cy)
            .atan2(p.0 - cx)
            .total_cmp(&(q.1 - cy).atan2(q.0 - cx))
    });
    let n = corners.len();
    for i in 0..n {
        let (p, q, r) = (corners[i], corners[(i + 1) % n], corners[(i + 2) % n]);
        if (q.0 - p.0) * (r.1 - q.1) - (q.1 - p.1) * (r.0 - q.0) < -1e-12 {
            return None;
        }
    }
    // Pappus: the polygon revolved has the body's volume.
    let (mut area, mut moment) = (0.0, 0.0);
    for i in 0..n {
        let (p, q) = (corners[i], corners[(i + 1) % n]);
        let c = p.0 * q.1 - q.0 * p.1;
        area += c;
        moment += c * (p.0 + q.0);
    }
    let volume = TAU * moment / 6.0;
    let v = super::mass::body_volume(b);
    let _ = area;
    if !((volume - v).abs() <= 1e-6 * v.abs().max(1e-12)) {
        return None;
    }
    let (o, a, e1, e2) = frame(o, a);
    Some(Revolved {
        o,
        a,
        e1,
        e2,
        section: Section::Polygon(corners),
    })
}

impl Revolved {
    /// The point of the section's boundary at s turned by ω about the body's axis.
    fn point(&self, s: f64, w: f64) -> P3 {
        let (rho, h) = self.section.point(s);
        self.o + (self.e1 * w.cos() + self.e2 * w.sin()) * rho + self.a * h
    }
}

/// The greatest of f over [lo, hi] near a sampled best, by golden-section search.
fn golden_max(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64) -> (f64, f64) {
    golden_max_in(f, lo, hi, 90)
}

/// [`golden_max`] in `steps` steps.
fn golden_max_in(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64, steps: usize) -> (f64, f64) {
    let g = (5f64.sqrt() - 1.0) / 2.0;
    let (mut lo, mut hi) = (lo, hi);
    let (mut x1, mut x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
    let (mut f1, mut f2) = (f(x1), f(x2));
    for _ in 0..steps {
        if f1 < f2 {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + g * (hi - lo);
            f2 = f(x2);
        } else {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - g * (hi - lo);
            f1 = f(x1);
        }
    }
    if f1 >= f2 { (x1, f1) } else { (x2, f2) }
}

/// A slice of the motion's: the body's points where `key` is `k`. For each boundary point of the
/// section (at s), its turns ω about the body's axis on the slice: `key(s, ω)` = A + B cos ω +
/// C sin ω exactly for a turn's planes (a helicoid's key is solved by Newton from those). The
/// greatest (`far`) or least distance from the motion's axis there, over s.
struct Slicer<'a> {
    body: &'a Revolved,
    /// The motion's axis.
    origin: P3,
    z: V3,
    /// Radians the screw turns per unit of height (0 for a turn).
    k: f64,
    /// Two directions square to the axis and each other, from which a screw's angles count.
    x: V3,
    y: V3,
}

impl Slicer<'_> {
    fn height(&self, p: &P3) -> f64 {
        (p - self.origin).dot(&self.z)
    }

    fn radius(&self, p: &P3) -> f64 {
        let w = p - self.origin;
        (w - self.z * w.dot(&self.z)).norm()
    }

    /// The slice's key of a point: its height for a turn; for a screw its polar angle less
    /// k × height, unwrapped near `near`.
    fn key(&self, p: &P3, near: f64) -> f64 {
        if self.k == 0.0 {
            return self.height(p);
        }
        let w = p - self.origin;
        let a = w.dot(&self.y).atan2(w.dot(&self.x)) - self.k * w.dot(&self.z);
        a + ((near - a) / TAU).round() * TAU
    }

    /// The section's boundary points (as s) in the half plane at ω on the slice `key = q`.
    fn crossings(&self, w: f64, q: f64) -> Vec<f64> {
        let b = self.body;
        if self.k == 0.0 {
            // Height is linear over the half plane: α₀ + α_ρ ρ + α_h h; its line crossing the
            // section's boundary in closed form.
            let dir = b.e1 * w.cos() + b.e2 * w.sin();
            let (a0, ar, ah) = (
                (b.o - self.origin).dot(&self.z),
                dir.dot(&self.z),
                b.a.dot(&self.z),
            );
            let g = |m: (f64, f64)| a0 + ar * m.0 + ah * m.1 - q;
            return match &b.section {
                Section::Disk { c, r } => {
                    // g(c + r(cos t, sin t)) = g(c) + r (ar cos t + ah sin t) = 0.
                    let m = r * ar.hypot(ah);
                    if m < 1e-300 {
                        return vec![];
                    }
                    let x = -g(*c) / m;
                    if !(x.abs() <= 1.0 + 1e-9) {
                        return vec![];
                    }
                    let (x, phi) = (x.clamp(-1.0, 1.0), ah.atan2(ar));
                    [phi + x.acos(), phi - x.acos()]
                        .map(|t| (t / TAU).rem_euclid(1.0))
                        .to_vec()
                }
                Section::Polygon(p) => {
                    let n = p.len();
                    let mut out = Vec::new();
                    for i in 0..n {
                        let (ga, gb) = (g(p[i]), g(p[(i + 1) % n]));
                        if (ga <= 0.0) != (gb <= 0.0) {
                            out.push((i as f64 + ga / (ga - gb)) / n as f64);
                        }
                    }
                    out
                }
            };
        }
        // A helicoid: sign changes of key − q along the boundary, refined by bisection.
        let f = |s: f64| self.key(&b.point(s, w), q) - q;
        let n = 48;
        let mut out = Vec::new();
        let mut prev = f(0.0);
        for i in 1..=n {
            let s1 = i as f64 / n as f64;
            let cur = f(s1);
            if (prev <= 0.0) != (cur <= 0.0) && (cur - prev).abs() < PI {
                // (False position, Illinois: the end kept twice running has its value halved.)
                let (mut lo, mut hi, mut flo, mut fhi) = ((i - 1) as f64 / n as f64, s1, prev, cur);
                let mut side = 0i8;
                for _ in 0..60 {
                    let m = lo + (hi - lo) * flo / (flo - fhi);
                    let fm = f(m);
                    if fm == 0.0 || hi - lo <= 1e-15 {
                        lo = m;
                        hi = m;
                        break;
                    }
                    if (fm <= 0.0) == (flo <= 0.0) {
                        lo = m;
                        flo = fm;
                        if side == -1 {
                            fhi *= 0.5;
                        }
                        side = -1;
                    } else {
                        hi = m;
                        fhi = fm;
                        if side == 1 {
                            flo *= 0.5;
                        }
                        side = 1;
                    }
                    if fm.abs() < 1e-15 {
                        lo = m;
                        hi = m;
                        break;
                    }
                }
                out.push(0.5 * (lo + hi));
            }
            prev = cur;
        }
        out
    }

    /// The greatest (`far`) or least distance from the axis of the body's points on the slice
    /// `key = q`, over the half planes through the body's axis, with the half plane's turn where
    /// it is: sampled all round and finely about `seed` (the slice before's, where a slice by an
    /// extreme one meets the body in a sliver), then refined (None when the slice misses it).
    fn extreme(&self, q: f64, far: bool, seed: f64) -> Option<(f64, f64)> {
        let sign = if far { 1.0 } else { -1.0 };
        let best_at = |w: f64| -> Option<f64> {
            self.crossings(w, q)
                .into_iter()
                .map(|s| sign * self.radius(&self.body.point(s, w)))
                .fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))))
        };
        let n = 240;
        let h = TAU / n as f64;
        let mut best: Option<(f64, f64)> = None;
        let try_at = |w: f64, best: &mut Option<(f64, f64)>| {
            if let Some(v) = best_at(w)
                && best.is_none_or(|b| v > b.1)
            {
                *best = Some((w, v));
            }
        };
        for i in 0..n {
            try_at(TAU * i as f64 / n as f64, &mut best);
        }
        for i in -16..=16 {
            try_at(seed + h * 2.0 * i as f64 / 16.0, &mut best);
        }
        let (w0, v0) = best?;
        let (w, v) = golden_max_in(
            &|w| best_at(w).unwrap_or(f64::NEG_INFINITY),
            w0 - h,
            w0 + h,
            40,
        );
        Some(if v > v0 {
            (sign * v, w)
        } else {
            (sign * v0, w0)
        })
    }
}

/// The outline's two sides in the slices' (distance, key) coordinates: the greatest distance
/// (`far`) and the least (`near`), each from the key's least to its greatest, the keys
/// clustered at the ends (q = mid + half·sin θ), where the sides meet at a point (a smooth body's
/// extreme slice touches it at one) or are joined by the extreme slice's straight stretch (a
/// flat face square to the motion's slices: a cylinder's end turned about a parallel axis).
struct Outline {
    far: Vec<(f64, f64)>,
    near: Vec<(f64, f64)>,
}

fn outline(sl: &Slicer, lo: Extreme, hi: Extreme, n: usize) -> Option<Outline> {
    let (mid, half) = (0.5 * (lo.key + hi.key), 0.5 * (hi.key - lo.key));
    let span = hi.key - lo.key;
    let scale = lo.r.abs().max(hi.r.abs()).max(span).max(tol::POINT);
    // At each end: the two sides just inside it; one point when they close on it.
    let end = |q: f64, at: Extreme| -> ((f64, f64), (f64, f64)) {
        match (sl.extreme(q, true, at.w), sl.extreme(q, false, at.w)) {
            (Some((f, _)), Some((nr, _))) if f - nr > 1e-6 * scale => ((f, at.key), (nr, at.key)),
            _ => ((at.r, at.key), (at.r, at.key)),
        }
    };
    let eps = 1e-9 * span;
    let (lo_far, lo_near) = end(lo.key + eps, lo);
    let (hi_far, hi_near) = end(hi.key - eps, hi);
    let side = |far: bool, a: (f64, f64), b: (f64, f64)| -> Option<Vec<(f64, f64)>> {
        let mut out = vec![a];
        let mut seed = lo.w;
        for i in 1..n {
            let th = -PI / 2.0 + PI * i as f64 / n as f64;
            let q = mid + half * th.sin();
            // (A slice within a hair of an end, where the ends' own keys are only found to a
            // search's precision, may miss the body: the end's point stands for it.)
            let Some((v, w)) = sl.extreme(q, far, seed) else {
                if 1.0 - th.sin().abs() < 1e-4 {
                    continue;
                }
                return None;
            };
            seed = w;
            out.push((v, q));
        }
        out.push(b);
        Some(out)
    };
    super::par::start();
    let (far, near) = rayon::join(
        || side(true, lo_far, hi_far),
        || side(false, lo_near, hi_near),
    );
    Some(Outline {
        far: far?,
        near: near?,
    })
}

/// A stretch of the body's boundary with the outward normals along it, in the half plane of its
/// section: a straight edge (one normal all along), a corner (one point, its normals fanning
/// from the edge before's to the edge after's) or an arc of a disk (the point and its normal
/// turning together). Each takes u from 0 to 1.
enum Strip {
    Edge {
        a: (f64, f64),
        b: (f64, f64),
        n: (f64, f64),
    },
    Fan {
        p: (f64, f64),
        a0: f64,
        da: f64,
    },
    Round {
        c: (f64, f64),
        r: f64,
        a0: f64,
        da: f64,
    },
}

impl Strip {
    /// The point (ρ, h) and the unit outward normal at u.
    fn at(&self, u: f64) -> ((f64, f64), (f64, f64)) {
        match *self {
            Strip::Edge { a, b, n } => ((a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u), n),
            Strip::Fan { p, a0, da } => {
                let t = a0 + da * u;
                (p, (t.cos(), t.sin()))
            }
            Strip::Round { c, r, a0, da } => {
                let t = a0 + da * u;
                let n = (t.cos(), t.sin());
                ((c.0 + r * n.0, c.1 + r * n.1), n)
            }
        }
    }
}

impl Section {
    /// The boundary as strips, in order round it, the stretches along the body's own axis
    /// left out (the boundary there is no surface of the body, and a corner on the axis no
    /// edge of it). None for a section this doesn't take apart.
    fn strips(&self) -> Option<Vec<Strip>> {
        match self {
            Section::Disk { c, r } if c.0 > *r => Some(vec![Strip::Round {
                c: *c,
                r: *r,
                a0: 0.0,
                da: TAU,
            }]),
            // A ball's: the half of its disk off the axis.
            Section::Disk { c, r } if c.0 == 0.0 => Some(vec![Strip::Round {
                c: *c,
                r: *r,
                a0: -FRAC_PI_2,
                da: PI,
            }]),
            Section::Disk { .. } => None,
            Section::Polygon(cs) => {
                let n = cs.len();
                let size = cs
                    .iter()
                    .fold(0.0f64, |m, c| m.max(c.0.abs()).max(c.1.abs()))
                    .max(tol::POINT);
                let on_axis = |i: usize| cs[i % n].0.abs() <= 1e-9 * size;
                // Edge i's outward normal (to the right of a counter-clockwise boundary).
                let normal = |i: usize| -> Option<(f64, f64)> {
                    let (p, q) = (cs[i % n], cs[(i + 1) % n]);
                    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                    let l = dx.hypot(dy);
                    (l > 1e-12 * size).then(|| (dy / l, -dx / l))
                };
                let mut out = Vec::new();
                for i in 0..n {
                    let ni = normal(i)?;
                    if !(on_axis(i) && on_axis(i + 1)) {
                        out.push(Strip::Edge {
                            a: cs[i],
                            b: cs[(i + 1) % n],
                            n: ni,
                        });
                    }
                    if !on_axis(i + 1) {
                        let nj = normal(i + 1)?;
                        let da = (ni.0 * nj.1 - ni.1 * nj.0).atan2(ni.0 * nj.0 + ni.1 * nj.1);
                        if da > 1e-12 {
                            out.push(Strip::Fan {
                                p: cs[(i + 1) % n],
                                a0: ni.1.atan2(ni.0),
                                da,
                            });
                        }
                    }
                }
                Some(out)
            }
        }
    }
}

/// A curve of the fold's image in the section's plane, by a parameter u: the fold on one
/// strip (one of its equation's two solutions), or a straight stretch between two (distance,
/// key)s (a flat face square to a turn's axis, every point of it on the fold, seen edge on).
#[derive(Clone, Copy)]
enum Trace {
    Fold { strip: usize, sign: f64 },
    Line { a: (f64, f64), b: (f64, f64) },
}

/// A [`Trace`] from u = `u0` to `u1`, with its polyline (rising u's and the points there) and
/// the vertices on it (where it ends and where others cross it), by rising u.
struct Arc {
    trace: Trace,
    u0: f64,
    u1: f64,
    us: Vec<f64>,
    xs: Vec<P2>,
    events: Vec<(f64, usize)>,
}

/// A stretch of the outline along one [`Trace`], from u = `u0` to `u1` (either way).
#[derive(Clone, Copy)]
struct Seg {
    trace: Trace,
    u0: f64,
    u1: f64,
}

/// The body's fold under the motion, in closed form: the points of its boundary where the
/// boundary's normal is square to the motion's velocity, which are the only ones the edge of
/// what the body sweeps can come from. The velocity at p is z × (p − origin) + c·z (c the
/// rise per radian turned; none for a turn). On the body's circle through the section's
/// point (ρ, h), where the section's normal is (ν_ρ, ν_h), the normal at the turn ω about the
/// body's axis a is ν_ρ e(ω) + ν_h a, and its product with the velocity is
///
///   e(ω)·[ν_ρ (z × D + c z) + (ν_ρ h − ν_h ρ)(z × a)] + ν_h a·(z × D + c z),
///
/// D from the motion's axis to the body's origin: A cos ω + B sin ω + C, zero at
/// ω = atan2(B, A) ± acos(−C/√(A² + B²)). So each circle of the body gives its two points of
/// the fold with no search, a sharp circle (a corner of the section) for each normal of its
/// fan, and the fold's image in the slices' (distance, key) is a curve given point by point.
struct Fold<'a> {
    sl: &'a Slicer<'a>,
    strips: Vec<Strip>,
    /// The key the body's keys are unwrapped about.
    near: f64,
    /// z × D + c z.
    lead: V3,
    /// z × a.
    zxa: V3,
    /// An amplitude √(A² + B²) no greater is none (every turn solves the equation, or none).
    tiny: f64,
}

impl Fold<'_> {
    /// (A, B, C) and the section's point on strip `s` at u.
    fn coefs(&self, s: &Strip, u: f64) -> (f64, f64, f64, (f64, f64)) {
        let b = self.sl.body;
        let ((rho, h), (nr, nh)) = s.at(u);
        let g = self.lead * nr + self.zxa * (nr * h - nh * rho);
        (
            b.e1.dot(&g),
            b.e2.dot(&g),
            nh * b.a.dot(&self.lead),
            (rho, h),
        )
    }

    /// The fold's point on strip `s` at u (the solution `sign`), where it has one (see
    /// [`Self::stretches`]): its distance from the motion's axis and its key.
    fn at(&self, s: &Strip, u: f64, sign: f64) -> (f64, f64) {
        let (a, b, c, (rho, h)) = self.coefs(s, u);
        let x = (-c / a.hypot(b).max(self.tiny)).clamp(-1.0, 1.0);
        let w = b.atan2(a) + sign * x.acos();
        let body = self.sl.body;
        let p = body.o + (body.e1 * w.cos() + body.e2 * w.sin()) * rho + body.a * h;
        (self.sl.radius(&p), self.sl.key(&p, self.near))
    }

    /// The stretches of strip `s` (in u) whose circles hold points of the fold: where
    /// A² + B² > C², their ends found from 64 samples' signs (256 round a disk) by bisection.
    fn stretches(&self, s: &Strip) -> Vec<(f64, f64)> {
        let n = match s {
            Strip::Round { .. } => 256,
            _ => 64,
        };
        let room = |u: f64| {
            let (a, b, c, _) = self.coefs(s, u);
            a * a + b * b - c * c - self.tiny * self.tiny
        };
        let at = |i: usize| i as f64 / n as f64;
        let rs: Vec<f64> = (0..=n).map(|i| room(at(i))).collect();
        let mut out = Vec::new();
        let mut start = (rs[0] > 0.0).then_some(0.0);
        for i in 0..n {
            if (rs[i] > 0.0) == (rs[i + 1] > 0.0) {
                continue;
            }
            // (The end kept on the stretch's side of the root.)
            let inside = rs[i] > 0.0;
            let (lo, hi) = super::math::bisect(at(i), at(i + 1), |m| (room(m) > 0.0) == inside);
            let root = if inside { lo } else { hi };
            match start.take() {
                Some(from) => out.push((from, root)),
                None => start = Some(root),
            }
        }
        if let Some(from) = start {
            out.push((from, 1.0));
        }
        out
    }

    /// Whether strip `strip`'s two solutions are one at u (the end of a stretch of it).
    fn merges(&self, strip: usize, u: f64) -> bool {
        let (a, b, c, _) = self.coefs(&self.strips[strip], u);
        let m = a.hypot(b);
        m > self.tiny && 1.0 - c.abs() / m < 1e-9
    }

    /// The (distance, key) of trace `t` at u.
    fn rq(&self, t: &Trace, u: f64) -> (f64, f64) {
        match *t {
            Trace::Fold { strip, sign } => self.at(&self.strips[strip], u, sign),
            Trace::Line { a, b } => (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u),
        }
    }

    /// The fold's traces: each stretch of each strip, both solutions; and for a strip that is
    /// all fold (a flat face square to a turn's axis: the equation vanishes all over it) the
    /// straight stretch its image is, between its outer circle's nearest and farthest points.
    fn traces(&self, turn: bool) -> Vec<(Trace, f64, f64)> {
        let mut out = Vec::new();
        for (si, s) in self.strips.iter().enumerate() {
            let stretches = self.stretches(s);
            if stretches.is_empty()
                && turn
                && let Strip::Edge { a, b, .. } = *s
            {
                let all_fold = [0.0, 0.5, 1.0].iter().all(|&u| {
                    let (ca, cb, cc, _) = self.coefs(s, u);
                    ca.hypot(cb) <= self.tiny && cc.abs() <= self.tiny
                });
                if all_fold {
                    let body = self.sl.body;
                    let (rho, h) = if a.0.abs() > b.0.abs() { a } else { b };
                    let centre = body.o + body.a * h;
                    let d = self.sl.radius(&centre);
                    let q = self.sl.key(&centre, self.near);
                    out.push((
                        Trace::Line {
                            a: ((d - rho.abs()).abs(), q),
                            b: (d + rho.abs(), q),
                        },
                        0.0,
                        1.0,
                    ));
                }
            }
            for (u0, u1) in stretches {
                if u1 > u0 {
                    for sign in [1.0, -1.0] {
                        out.push((Trace::Fold { strip: si, sign }, u0, u1));
                    }
                }
            }
        }
        out
    }

    /// The polyline of trace `t` over [u0, u1]: thirty-two stretches, each halved until its
    /// middle is within `dev` of its chord's and the chord no longer than `longest`. None
    /// past 60 000 points.
    fn polyline(
        &self,
        t: &Trace,
        (u0, u1): (f64, f64),
        p2: &dyn Fn((f64, f64)) -> P2,
        dev: f64,
        longest: f64,
    ) -> Option<(Vec<f64>, Vec<P2>)> {
        let x = |u: f64| p2(self.rq(t, u));
        const START: usize = 32;
        let (mut us, mut xs) = (vec![u0], vec![x(u0)]);
        for i in 0..START {
            let at = |j: usize| u0 + (u1 - u0) * j as f64 / START as f64;
            let (a, b) = (at(i), if i + 1 == START { u1 } else { at(i + 1) });
            let mut stack = vec![(a, x(a), b, x(b), 0u32)];
            while let Some((a, xa, b, xb, depth)) = stack.pop() {
                let m = 0.5 * (a + b);
                let xm = x(m);
                let bow = (xm - nalgebra::center(&xa, &xb)).norm();
                if depth >= 40 || (bow <= dev && (xb - xa).norm() <= longest) {
                    us.push(b);
                    xs.push(xb);
                } else {
                    stack.push((m, xm, b, xb, depth + 1));
                    stack.push((a, xa, m, xm, depth + 1));
                }
                if us.len() > 60_000 {
                    return None;
                }
            }
        }
        Some((us, xs))
    }

    /// Exact points of the piece `segs` of the outline, as many as the B-spline through them
    /// needs to keep within `tol` of the outline: six stretches a seg to start, then, round
    /// by round, the trace's point midway (in u) between two neighbours wherever the spline
    /// is farther than half of `tol` from it. `first` is the piece's first point where the
    /// piece before ends on it. None past 4000 points.
    fn sampled(
        &self,
        segs: &[Seg],
        p2: &dyn Fn((f64, f64)) -> P2,
        tol: f64,
        first: Option<(f64, f64)>,
    ) -> Option<Vec<(f64, f64)>> {
        struct Node {
            seg: usize,
            u: f64,
            rq: (f64, f64),
            x: P3,
        }
        let node = |seg: usize, u: f64| {
            let rq = self.rq(&segs[seg].trace, u);
            let x = p2(rq);
            Node {
                seg,
                u,
                rq,
                x: P3::new(x.x, x.y, 0.0),
            }
        };
        let mut nodes: Vec<Node> = Vec::new();
        for (si, sg) in segs.iter().enumerate() {
            const START: usize = 6;
            for j in (if si == 0 { 0 } else { 1 })..=START {
                let u = if j == START {
                    sg.u1
                } else {
                    sg.u0 + (sg.u1 - sg.u0) * j as f64 / START as f64
                };
                nodes.push(node(si, u));
            }
        }
        if let Some(rq) = first {
            let x = p2(rq);
            nodes[0].rq = rq;
            nodes[0].x = P3::new(x.x, x.y, 0.0);
        }
        let size = nodes.iter().fold(tol, |m, n| m.max(n.x.coords.norm()));
        let apart = 1e-12 * size;
        nodes.dedup_by(|b, a| (b.x - a.x).norm() <= apart);
        if nodes.len() < 2 {
            return None;
        }
        let debug = crate::native::env_flag!("CADRS_SWEEP_SOLID_DEBUG");
        for round in 0..24 {
            let pts: Vec<P3> = nodes.iter().map(|n| n.x).collect();
            let (curve, params) = fit_through(&pts);
            let mut add: Vec<(usize, Node)> = Vec::new();
            for k in 0..nodes.len() - 1 {
                let (a, b) = (&nodes[k], &nodes[k + 1]);
                let from = if a.seg == b.seg { a.u } else { segs[b.seg].u0 };
                let mid = node(b.seg, 0.5 * (from + b.u));
                if (mid.x - a.x).norm() <= apart || (mid.x - b.x).norm() <= apart {
                    continue;
                }
                // The spline's point nearest it, between the two.
                let (t0, t1) = (params[k], params[k + 1]);
                let mut t = 0.5 * (t0 + t1);
                for _ in 0..4 {
                    let (p, d1, d2) = curve.ders2(t);
                    let r = p - mid.x;
                    let den = d1.norm_squared() + r.dot(&d2);
                    if den <= 0.0 {
                        break;
                    }
                    t = (t - r.dot(&d1) / den).clamp(t0, t1);
                }
                if (curve.point(t) - mid.x).norm() > 0.5 * tol {
                    add.push((k + 1, mid));
                }
            }
            if debug {
                eprintln!(
                    "  fit round {round}: {} points, {} to add",
                    nodes.len(),
                    add.len()
                );
            }
            if add.is_empty() {
                break;
            }
            if nodes.len() + add.len() > 4000 {
                return None;
            }
            for (k, n) in add.into_iter().rev() {
                nodes.insert(k, n);
            }
        }
        Some(nodes.iter().map(|n| n.rq).collect())
    }
}

/// The B-spline of degree five through `pts` (of as high a degree as fewer points allow), with
/// the points' parameters: the lengths along the curve between them. Chords for lengths keep
/// an interpolant only to the fourth power of the points' spacing whatever its degree, so the
/// lengths are taken along interpolants instead: along the cubic by chords, then along the
/// quintic by those (each gains two powers; Floater and Surazhsky, "Parameterization for
/// curve interpolation", 2006).
fn fit_through(pts: &[P3]) -> (super::nurbs::NurbsCurve, Vec<f64>) {
    let fit = |params: &[f64], degree: usize| {
        super::nurbs::NurbsCurve::interpolate(pts, params, degree.min(pts.len() - 1), None, None)
    };
    let mut params = vec![0.0];
    for w in pts.windows(2) {
        params.push(params.last().expect("a parameter") + (w[1] - w[0]).norm());
    }
    if pts.len() < 4 {
        return (fit(&params, 5), params);
    }
    // The lengths of a curve's stretches between the parameters (five-point Gauss-Legendre).
    const GAUSS: [(f64, f64); 5] = [
        (0.0, 0.568_888_888_888_888_9),
        (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
        (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
        (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
        (0.906_179_845_938_664, 0.236_926_885_056_189_1),
    ];
    let lengths = |c: &super::nurbs::NurbsCurve, params: &[f64]| -> Vec<f64> {
        let mut out = vec![0.0];
        for w in params.windows(2) {
            let (mid, half) = (0.5 * (w[0] + w[1]), 0.5 * (w[1] - w[0]));
            let len: f64 = GAUSS
                .iter()
                .map(|&(x, wt)| wt * c.ders2(mid + half * x).1.norm())
                .sum::<f64>()
                * half;
            out.push(out.last().expect("a length") + len.max(f64::MIN_POSITIVE));
        }
        out
    };
    let cubic = fit(&params, 3);
    let params = lengths(&cubic, &params);
    let quintic = fit(&params, 5);
    let params = lengths(&quintic, &params);
    (fit(&params, 5), params)
}

/// The signed angle from `a` to `b` (counter-clockwise positive), in (−π, π].
fn turn_from(a: &V2, b: &V2) -> f64 {
    (a.x * b.y - a.y * b.x).atan2(a.dot(b))
}

/// The outline in closed form ([`Fold`]): the outer edge of the fold's image in the section's
/// plane, as its closed loop of curves, counter-clockwise. The fold's traces are exact curves
/// there; where two cross (found on their polylines, then solved on the curves themselves)
/// the edge passes from one to the other, and it is followed from its point farthest along
/// the plane's x, keeping what the body doesn't sweep on its right: at each crossing or
/// meeting of traces the way that turns most to the right. So nothing is sliced, and the
/// edge may double back in the key (a notch where one stretch of the fold crosses another,
/// which the least distance on each slice would cut straight across). Each stretch between
/// two corners is then exact points dense enough for its B-spline to keep within 2e-11 of the
/// body's reach of the outline.
///
/// `c` is the motion's rise per radian (0 for a turn), `near` the key the body's keys are
/// unwrapped about, `p2` the plane's point of a (distance, key), `keys` the body's keys' range
/// as found by search (the outline's own must agree with it). None where the section isn't
/// taken apart ([`Section::strips`]) or the edge can't be followed (the caller slices
/// instead). With the curves, the greatest distance from the axis.
fn exact_outline(
    sl: &Slicer,
    near: f64,
    c: f64,
    p2: &dyn Fn((f64, f64)) -> P2,
    keys: (f64, f64),
) -> Option<(Vec<Curve2>, f64)> {
    let debug = crate::native::env_flag!("CADRS_SWEEP_SOLID_DEBUG");
    let b = sl.body;
    let strips = b.section.strips()?;
    let lead = sl.z.cross(&(b.o - sl.origin)) + sl.z * c;
    let size = strips.iter().fold(0.0f64, |m, s| {
        [0.0, 0.5, 1.0].iter().fold(m, |m, &u| {
            let ((rho, h), _) = s.at(u);
            m.max(rho.abs()).max(h.abs())
        })
    });
    let fold = Fold {
        sl,
        strips,
        near,
        lead,
        zxa: sl.z.cross(&b.a),
        tiny: 1e-12 * (lead.norm() + size),
    };
    let x_at = |t: &Trace, u: f64| p2(fold.rq(t, u));
    // The traces as arcs with their polylines; one whose image is a point goes.
    let reach = (b.o - sl.origin).norm() + 2.0 * size;
    let (dev, longest) = (1e-6 * reach, reach / 64.0);
    let vertex_gap = 1e-8 * reach;
    let mut arcs: Vec<Arc> = Vec::new();
    for (trace, u0, u1) in fold.traces(c == 0.0) {
        let (us, xs) = fold.polyline(&trace, (u0, u1), p2, dev, longest)?;
        let length: f64 = xs.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
        if length > vertex_gap {
            arcs.push(Arc {
                trace,
                u0,
                u1,
                us,
                xs,
                events: Vec::new(),
            });
        }
    }
    if arcs.is_empty() {
        return None;
    }
    // A trace's derivative by u (differenced over a thousandth of its polyline's step there).
    let ddu = |a: &Arc, u: f64| -> V2 {
        let k = a.us.partition_point(|&x| x < u).clamp(1, a.us.len() - 1);
        let h = 1e-3 * (a.us[k] - a.us[k - 1]);
        let (lo, hi) = ((u - h).max(a.u0), (u + h).min(a.u1));
        (x_at(&a.trace, hi) - x_at(&a.trace, lo)) / (hi - lo)
    };
    // Where two arcs cross: every pair of polyline spans whose boxes (grown by the
    // polylines' bow) meet and whose chords' lines meet near both, solved on the curves by
    // Newton; kept where they cross at an angle.
    struct Cross {
        i: usize,
        u: f64,
        j: usize,
        v: f64,
        p: P2,
    }
    let mut spans: Vec<(f64, f64, f64, f64, usize, usize)> = Vec::new();
    for (ai, a) in arcs.iter().enumerate() {
        for k in 0..a.xs.len() - 1 {
            let (p, q) = (a.xs[k], a.xs[k + 1]);
            spans.push((
                p.x.min(q.x) - 2.0 * dev,
                p.x.max(q.x) + 2.0 * dev,
                p.y.min(q.y) - 2.0 * dev,
                p.y.max(q.y) + 2.0 * dev,
                ai,
                k,
            ));
        }
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut crosses: Vec<Cross> = Vec::new();
    for (n, s) in spans.iter().enumerate() {
        for t in &spans[n + 1..] {
            if t.0 > s.1 {
                break;
            }
            if t.2 > s.3 || t.3 < s.2 {
                continue;
            }
            let ((i, k), (j, l)) = ((s.4, s.5), (t.4, t.5));
            if i == j && k.abs_diff(l) <= 1 {
                continue;
            }
            let (a, c2) = (&arcs[i], &arcs[j]);
            let (p0, p1, q0, q1) = (a.xs[k], a.xs[k + 1], c2.xs[l], c2.xs[l + 1]);
            let (d, e) = (p1 - p0, q1 - q0);
            let den = d.x * e.y - d.y * e.x;
            if den.abs() <= 1e-14 * d.norm() * e.norm() {
                continue;
            }
            let w = q0 - p0;
            let (ss, tt) = ((w.x * e.y - w.y * e.x) / den, (w.x * d.y - w.y * d.x) / den);
            if !(-0.5..=1.5).contains(&ss) || !(-0.5..=1.5).contains(&tt) {
                continue;
            }
            // Newton on the curves, within a span either side.
            let win =
                |a: &Arc, k: usize| (a.us[k.saturating_sub(1)], a.us[(k + 2).min(a.us.len() - 1)]);
            let ((ulo, uhi), (vlo, vhi)) = (win(a, k), win(c2, l));
            let mut u = (a.us[k] + (a.us[k + 1] - a.us[k]) * ss).clamp(ulo, uhi);
            let mut v = (c2.us[l] + (c2.us[l + 1] - c2.us[l]) * tt).clamp(vlo, vhi);
            let mut found = None;
            for _ in 0..40 {
                let f = x_at(&a.trace, u) - x_at(&c2.trace, v);
                let (du, dv) = (ddu(a, u), ddu(c2, v));
                let det = -du.x * dv.y + du.y * dv.x;
                if f.norm() <= 1e-12 * reach {
                    let sin = det.abs() / (du.norm() * dv.norm()).max(f64::MIN_POSITIVE);
                    if sin > 1e-7 {
                        found = Some(x_at(&a.trace, u));
                    }
                    break;
                }
                if det.abs() <= 1e-300 {
                    break;
                }
                // [du, −dv] (δu, δv) = −f.
                let su = (f.x * dv.y - f.y * dv.x) / det;
                let sv = (du.x * -f.y + du.y * f.x) / det;
                u = (u + su).clamp(ulo, uhi);
                v = (v + sv).clamp(vlo, vhi);
            }
            let Some(p) = found else {
                continue;
            };
            // (One trace through one point at two of its own parameters, not at one.)
            if i == j && (u - v).abs() <= 1e-9 * (a.u1 - a.u0) {
                continue;
            }
            if crosses.iter().any(|x| {
                (x.p - p).norm() <= vertex_gap && ((x.i, x.j) == (i, j) || (x.i, x.j) == (j, i))
            }) {
                continue;
            }
            crosses.push(Cross { i, u, j, v, p });
        }
    }
    // The vertices: the arcs' ends, those at one point together, and the crossings.
    let mut vertices: Vec<(P2, Vec<(usize, f64)>)> = Vec::new();
    for ai in 0..arcs.len() {
        for u in [arcs[ai].u0, arcs[ai].u1] {
            let p = x_at(&arcs[ai].trace, u);
            // Where a strip's two solutions merge (its stretch's end), its two arcs end on
            // one point of one smooth curve; found there each is only good to the square
            // root of the rounding (along the curve, not off it), so they are joined by what
            // they are, not by where they come out.
            let twin = match arcs[ai].trace {
                Trace::Fold { strip, sign } if fold.merges(strip, u) => {
                    vertices.iter().position(|(_, at)| {
                        at.iter().any(|&(aj, uj)| {
                            uj == u
                                && matches!(arcs[aj].trace, Trace::Fold { strip: s2, sign: g2 }
                                    if s2 == strip && g2 == -sign)
                        })
                    })
                }
                _ => None,
            };
            let near_by = || {
                vertices
                    .iter()
                    .position(|(q, _)| (q - p).norm() <= vertex_gap)
            };
            let v = match twin.or_else(near_by) {
                Some(v) => v,
                None => {
                    vertices.push((p, Vec::new()));
                    vertices.len() - 1
                }
            };
            vertices[v].1.push((ai, u));
            arcs[ai].events.push((u, v));
        }
    }
    // (A crossing found where arcs end on each other is that meeting, already a vertex: only
    // an arc passing through it that isn't yet there is added to it.)
    for x in &crosses {
        let v = match vertices
            .iter()
            .position(|(q, _)| (q - x.p).norm() <= vertex_gap)
        {
            Some(v) => v,
            None => {
                vertices.push((x.p, Vec::new()));
                vertices.len() - 1
            }
        };
        for (ai, u) in [(x.i, x.u), (x.j, x.v)] {
            let there = vertices[v].1.iter().any(|&(aj, uj)| {
                aj == ai
                    && (x_at(&arcs[ai].trace, uj) - x_at(&arcs[ai].trace, u)).norm() <= vertex_gap
            });
            if !there {
                vertices[v].1.push((ai, u));
                arcs[ai].events.push((u, v));
            }
        }
    }
    for a in arcs.iter_mut() {
        a.events.sort_by(|p, q| p.0.total_cmp(&q.0));
    }
    if debug {
        for (k, a) in arcs.iter().enumerate() {
            let (p, q) = (a.xs[0], a.xs[a.xs.len() - 1]);
            eprintln!(
                "  arc {k}: {} u {:.6}..{:.6} from ({:.6}, {:.6}) to ({:.6}, {:.6}), {} points, events {:?}",
                match a.trace {
                    Trace::Fold { strip, sign } => format!("strip {strip} sign {sign}"),
                    Trace::Line { .. } => "line".to_string(),
                },
                a.u0,
                a.u1,
                p.x,
                p.y,
                q.x,
                q.y,
                a.us.len(),
                a.events.iter().map(|e| e.1).collect::<Vec<_>>()
            );
        }
        eprintln!(
            "exact outline: {} arcs ({} polyline points), {} crossings, {} vertices",
            arcs.len(),
            arcs.iter().map(|a| a.us.len()).sum::<usize>(),
            crosses.len(),
            vertices.len()
        );
    }
    // The next event along an arc from u, the way `way` (±1): its parameter and its vertex.
    let next = |ai: usize, u: f64, way: f64| -> Option<(f64, usize)> {
        let ev = &arcs[ai].events;
        if way > 0.0 {
            ev.iter().copied().find(|e| e.0 > u)
        } else {
            ev.iter().rev().copied().find(|e| e.0 < u)
        }
    };
    // The direction an arc leaves u in, the way `way`: to a point of it a little ahead (no
    // farther than half way to its next event, nor than a hundred-thousandth of the reach).
    let ahead = 1e-5 * reach;
    let leaving = |ai: usize, u: f64, way: f64| -> Option<V2> {
        let (un, _) = next(ai, u, way)?;
        let a = &arcs[ai];
        let from = x_at(&a.trace, u);
        let mut ul = u + 0.5 * (un - u);
        for _ in 0..80 {
            if (x_at(&a.trace, ul) - from).norm() <= ahead {
                break;
            }
            ul = u + 0.5 * (ul - u);
        }
        (x_at(&a.trace, ul) - from).try_normalize(1e-300)
    };
    // From the point farthest along x, up it (what isn't swept on the right).
    let (mut start_arc, mut start_k, mut best) = (0usize, 0usize, f64::NEG_INFINITY);
    for (ai, a) in arcs.iter().enumerate() {
        for (k, p) in a.xs.iter().enumerate() {
            if p.x > best {
                (start_arc, start_k, best) = (ai, k, p.x);
            }
        }
    }
    let sa = &arcs[start_arc];
    let (klo, khi) = (
        start_k.saturating_sub(1),
        (start_k + 1).min(sa.us.len() - 1),
    );
    let start_u = golden_max(&|u| x_at(&sa.trace, u).x, sa.us[klo], sa.us[khi]).0;
    // (Off its events: a start on one would be a vertex to leave, not a point to pass.)
    let start_u = {
        let clear = |u: f64| {
            sa.events
                .iter()
                .all(|e| (e.0 - u).abs() > 1e-9 * (sa.u1 - sa.u0))
        };
        if clear(start_u) {
            start_u
        } else {
            let (lo, hi) = (sa.us[klo], sa.us[khi]);
            [0.5 * (start_u + lo), 0.5 * (start_u + hi)]
                .into_iter()
                .find(|&u| clear(u))?
        }
    };
    if debug {
        eprintln!(
            "exact outline: starts on arc {start_arc} at u {start_u:.6}, ({:.6}, {:.6})",
            x_at(&sa.trace, start_u).x,
            x_at(&sa.trace, start_u).y
        );
    }
    let up = V2::new(0.0, 1.0);
    let start_way = [1.0, -1.0]
        .into_iter()
        .filter_map(|w| Some((w, leaving(start_arc, start_u, w)?.dot(&up))))
        .max_by(|p, q| p.1.total_cmp(&q.1))?
        .0;
    // The walk: runs along arcs, each with whether it starts at a corner.
    let mut runs: Vec<(Seg, bool)> = Vec::new();
    let (mut ai, mut u, mut way) = (start_arc, start_u, start_way);
    let mut corner = false;
    let limit = 8 * (vertices.len() + arcs.len()) + 16;
    let mut closed = false;
    for step in 0..limit {
        let (un, v) = next(ai, u, way)?;
        // Back at the start, on its arc the way it was left?
        let passes = ai == start_arc
            && way == start_way
            && step > 0
            && (start_u - u) * way > 0.0
            && (un - start_u) * way >= 0.0;
        let to = if passes { start_u } else { un };
        runs.push((
            Seg {
                trace: arcs[ai].trace,
                u0: u,
                u1: to,
            },
            corner,
        ));
        if passes {
            closed = true;
            break;
        }
        // At the vertex: every way out of it but the way back, the one turning most to the
        // right of the way in.
        let coming = -leaving(ai, un, -way)?;
        let mut pick: Option<(f64, usize, f64, f64)> = None;
        for &(aj, uj) in &vertices[v].1 {
            for w in [1.0, -1.0] {
                if aj == ai && uj == un && w == -way {
                    continue;
                }
                let Some(d) = leaving(aj, uj, w) else {
                    continue;
                };
                let turn = turn_from(&coming, &d);
                // (Not straight back along a trace lying on this one.)
                if turn.abs() > PI - 1e-6 {
                    continue;
                }
                if pick.is_none_or(|p| turn < p.0) {
                    pick = Some((turn, aj, uj, w));
                }
            }
        }
        let Some((turn, aj, uj, w)) = pick else {
            if debug {
                eprintln!("exact outline: a dead end at {:?}", vertices[v].0);
            }
            return None;
        };
        if debug && step < 60 {
            eprintln!(
                "  walk {step}: arc {ai} {u:.6} -> {un:.6} to vertex {v} at ({:.6}, {:.6}); turns {turn:.4} onto arc {aj} at {uj:.6} way {w}",
                vertices[v].0.x, vertices[v].0.y
            );
        }
        corner = turn.abs() > 0.02;
        (ai, u, way) = (aj, uj, w);
    }
    if !closed {
        if debug {
            eprintln!("exact outline: the edge doesn't close in {limit} steps");
        }
        return None;
    }
    // Pieces between corners: from a corner where there is one, else cut at the start and
    // at the run half way round (a smooth closed outline: two splines).
    runs.retain(|(s, _)| s.u0 != s.u1);
    if runs.is_empty() {
        return None;
    }
    match runs.iter().position(|r| r.1) {
        Some(k) => runs.rotate_left(k),
        None => {
            runs[0].1 = true;
            let half = runs.len() / 2;
            if half > 0 {
                runs[half].1 = true;
            } else {
                // One run all the way round: halved.
                let (s, _) = runs[0];
                let mid = 0.5 * (s.u0 + s.u1);
                runs = vec![(Seg { u1: mid, ..s }, true), (Seg { u0: mid, ..s }, true)];
            }
        }
    }
    let mut pieces: Vec<Vec<Seg>> = Vec::new();
    for (s, corner) in &runs {
        if *corner || pieces.is_empty() {
            pieces.push(Vec::new());
        }
        pieces.last_mut()?.push(*s);
    }
    let tol = 2e-11 * reach;
    let mut sides: Vec<Vec<(f64, f64)>> = Vec::new();
    for segs in &pieces {
        let first = sides.last().and_then(|p| p.last().copied());
        sides.push(fold.sampled(segs, p2, tol, first)?);
    }
    // Closed: the last piece ends on the first's first point.
    let head = *sides.first()?.first()?;
    *sides.last_mut()?.last_mut()? = head;
    // The outline's keys must be the body's, and it must run counter-clockwise.
    let all = || sides.iter().flatten();
    let (qlo, qhi) = all().fold((f64::INFINITY, f64::NEG_INFINITY), |m, p| {
        (m.0.min(p.1), m.1.max(p.1))
    });
    let span = keys.1 - keys.0;
    let pts: Vec<P2> = all().map(|&rq| p2(rq)).collect();
    let area: f64 = (0..pts.len())
        .map(|k| {
            let (p, q) = (pts[k], pts[(k + 1) % pts.len()]);
            0.5 * (p.x * q.y - q.x * p.y)
        })
        .sum();
    if debug {
        eprintln!(
            "exact outline: {} pieces of {:?} points; keys {qlo}..{qhi} (the body's {keys:?}); area {area}",
            sides.len(),
            sides.iter().map(|p| p.len()).collect::<Vec<_>>()
        );
    }
    if (qlo - keys.0).abs() > 1e-4 * span || (qhi - keys.1).abs() > 1e-4 * span || !(area > 0.0) {
        return None;
    }
    if debug {
        // Against the slices' own search at fifteen keys: where the outline's points (as a
        // polygon in distance and key) cross each, their greatest and least distance.
        let poly: Vec<(f64, f64)> = all().copied().collect();
        let (mut far_off, mut near_off) = (0.0f64, 0.0f64);
        for i in 1..16 {
            let q = keys.0 + span * i as f64 / 16.0;
            let (mut lo_r, mut hi_r) = (f64::INFINITY, f64::NEG_INFINITY);
            for k in 0..poly.len() {
                let (p, n) = (poly[k], poly[(k + 1) % poly.len()]);
                if (p.1 <= q) != (n.1 <= q) {
                    let r = p.0 + (n.0 - p.0) * (q - p.1) / (n.1 - p.1);
                    lo_r = lo_r.min(r);
                    hi_r = hi_r.max(r);
                }
            }
            if let Some((v, _)) = sl.extreme(q, true, 0.0) {
                far_off = far_off.max((hi_r - v).abs());
            }
            if let Some((v, _)) = sl.extreme(q, false, 0.0) {
                near_off = near_off.max((lo_r - v).abs());
            }
        }
        eprintln!(
            "exact outline: off the slices' search by {far_off:.3e} (far) and {near_off:.3e} (near), the polygon's chords' sag included"
        );
    }
    let far = all().map(|p| p.0).fold(0.0, f64::max);
    let curves = sides
        .iter()
        .map(|side| {
            // (A straight stretch stays a line.)
            let ps: Vec<P3> = side
                .iter()
                .map(|&rq| {
                    let p = p2(rq);
                    P3::new(p.x, p.y, 0.0)
                })
                .collect();
            let (a, z) = (ps[0], ps[ps.len() - 1]);
            let chord = z - a;
            let straight = ps.iter().all(|p| {
                let w = p - a;
                (w - chord * (w.dot(&chord) / chord.norm_squared().max(f64::MIN_POSITIVE))).norm()
                    <= 1e-13 * reach
            });
            if straight {
                return Curve2::Line {
                    a: P2::new(a.x, a.y),
                    b: P2::new(z.x, z.y),
                    source: None,
                };
            }
            let c = fit_through(&ps).0;
            Curve2::Spline {
                degree: c.degree,
                knots: c.knots.clone(),
                poles: c
                    .cps
                    .iter()
                    .map(|h| P2::new(h.x / h.w, h.y / h.w))
                    .collect(),
                weights: vec![],
                source: None,
            }
        })
        .collect();
    Some((curves, far))
}

/// An extreme slice: its key, and where it touches the body (its distance from the axis and the
/// turn about the body's axis there).
#[derive(Clone, Copy)]
struct Extreme {
    key: f64,
    r: f64,
    w: f64,
}

/// The least and greatest key over the body (its extreme slices), by golden-section search over
/// the section's boundary and the turns.
fn key_range(sl: &Slicer, near: f64) -> (Extreme, Extreme) {
    let f = |s: f64, w: f64| sl.key(&sl.body.point(s, w), near);
    let n = 256;
    let mut best = [(f64::INFINITY, 0.0, 0.0), (f64::NEG_INFINITY, 0.0, 0.0)];
    for i in 0..n {
        for j in 0..n {
            let (s, w) = (i as f64 / n as f64, TAU * j as f64 / n as f64);
            let v = f(s, w);
            if v < best[0].0 {
                best[0] = (v, s, w);
            }
            if v > best[1].0 {
                best[1] = (v, s, w);
            }
        }
    }
    let mut out = [Extreme {
        key: 0.0,
        r: 0.0,
        w: 0.0,
    }; 2];
    for (k, sign) in [(0usize, -1.0), (1, 1.0)] {
        let (_, s0, w0) = best[k];
        let (hs, hw) = (1.0 / n as f64, TAU / n as f64);
        // Alternate searches along s and ω.
        let (mut s, mut w) = (s0, w0);
        for _ in 0..6 {
            s = golden_max(&|x| sign * f(x, w), s - hs, s + hs).0;
            w = golden_max(&|x| sign * f(s, x), w - hw, w + hw).0;
        }
        out[k] = Extreme {
            key: f(s, w),
            r: sl.radius(&sl.body.point(s, w)),
            w,
        };
    }
    (out[0], out[1])
}

/// Whether `b` is a whole torus and `steps` one slide of it aslant: neither along its axis nor
/// square to it (either of which its leading faces sweep in closed form, and soundly).
fn slid_torus(b: &Body, steps: &[RigidStep]) -> bool {
    let ([RigidStep::Slide { from, to }], [f]) = (steps, b.faces.as_slice()) else {
        return false;
    };
    let super::surface::Surface::Torus { f, .. } = &f.surf else {
        return false;
    };
    let Some(d) = (to - from).try_normalize(tol::POINT) else {
        return false;
    };
    let along = d.dot(&f.z).abs();
    along > tol::ANGULAR && along < 1.0 - tol::ANGULAR
}

/// `target` less the whole sweep of the torus `b` by `steps` ([`super::sweep_tube::torus`]: its
/// envelope trimmed by what it covers of itself); None where `b` isn't one, or the sweep or
/// the cut isn't made soundly.
fn cut_torus_whole(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    steps: &[RigidStep],
) -> Option<OpResult> {
    let r = super::sweep_tube::torus(k, b, &super::sweep_tube::Motion::Steps(steps))?.ok()?;
    let [s] = r.bodies.as_slice() else {
        return None;
    };
    let cut = k.boolean(crate::BoolOp::Subtract, target, &[*s]);
    k.release(*s);
    if let Ok(c) = cut
        && c.bodies.iter().all(|&id| k.sound(id))
    {
        return Some(c);
    }
    if crate::native::env_flag!("CADRS_SWEEP_DEBUG") {
        eprintln!("swept cut: the torus's whole sweep wouldn't cut from the target");
    }
    None
}

/// [`crate::Kernel::sweep_solid`].
/// A solid of revolution grown by a ball of radius `d`, exactly: its section grown by `d` in its
/// half plane, then revolved. A ball grows into a ball, a torus into a torus; a convex polygon
/// section with one edge along the axis (a cylinder's, a cone's, a frustum's) into its other
/// edges moved out by `d`, a round of radius `d` at each corner (at the axis's ends only the
/// part off the axis), and the axis between them. None for any other body.
pub(crate) fn grown_revolved(b: &Body, d: f64) -> Option<Body> {
    if !(d > 0.0) {
        return None;
    }
    let rev = revolved(b)?;
    let pt = |x: f64, y: f64| nalgebra::Point2::new(x, y);
    let curves: Vec<Curve2> = match &rev.section {
        Section::Disk { c, r } => {
            // A ball's disc stays one about the axis; a torus's must stay off it.
            if c.0 != 0.0 && c.0 <= r + d {
                return None;
            }
            if c.0 == 0.0 {
                // The half disc against the axis.
                vec![
                    Curve2::Arc {
                        center: pt(0.0, c.1),
                        radius: r + d,
                        start_angle: -FRAC_PI_2,
                        sweep: PI,
                        source: None,
                    },
                    Curve2::Line {
                        a: pt(0.0, c.1 + r + d),
                        b: pt(0.0, c.1 - r - d),
                        source: None,
                    },
                ]
            } else {
                vec![Curve2::Circle {
                    center: pt(c.0, c.1),
                    radius: r + d,
                    source: None,
                }]
            }
        }
        Section::Polygon(cs) => {
            let n = cs.len();
            let size = cs
                .iter()
                .fold(1.0f64, |m, c| m.max(c.0.abs()).max(c.1.abs()));
            let on_axis = |i: usize| cs[i % n].0.abs() <= 1e-9 * size;
            // The one edge along the axis, i → i + 1 (counter-clockwise: running down it).
            let axis: Vec<usize> = (0..n).filter(|&i| on_axis(i) && on_axis(i + 1)).collect();
            let &[i] = axis.as_slice() else {
                return None;
            };
            if (0..n).filter(|&j| on_axis(j)).count() != 2 {
                return None;
            }
            let c = |j: usize| cs[j % n];
            // Edge j's outward normal (to the right of a counter-clockwise boundary).
            let normal = |j: usize| {
                let (p, q) = (c(j), c(j + 1));
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                let l = dx.hypot(dy);
                (dy / l, -dx / l)
            };
            // The axis edge runs down it (the section to its left, off the axis): c(i) is its
            // top, c(i + 1) its bottom. The edges either side face away from the axis.
            let (before, after) = (normal(i + n - 1), normal(i + 1));
            if before.0 < -1e-12 || after.0 < -1e-12 {
                return None;
            }
            let (top, bottom) = (c(i), c(i + 1));
            if !(top.1 > bottom.1) {
                return None;
            }
            let arc = |at: (f64, f64), a0: f64, a1: f64| {
                let sweep = (a1 - a0).rem_euclid(TAU);
                (sweep > 1e-12 && sweep < PI).then(|| Curve2::Arc {
                    center: pt(at.0, at.1),
                    radius: d,
                    start_angle: a0,
                    sweep,
                    source: None,
                })
            };
            let mut out = Vec::new();
            // Round the bottom from straight down to the edge after it.
            out.extend(arc(bottom, -FRAC_PI_2, after.1.atan2(after.0)));
            // Each edge moved out, a round at each corner between them.
            for k in 1..n {
                let j = i + k;
                let (nx, ny) = normal(j);
                let (p, q) = (c(j), c(j + 1));
                out.push(Curve2::Line {
                    a: pt(p.0 + nx * d, p.1 + ny * d),
                    b: pt(q.0 + nx * d, q.1 + ny * d),
                    source: None,
                });
                if k < n - 1 {
                    let (mx, my) = normal(j + 1);
                    out.extend(arc(q, ny.atan2(nx), my.atan2(mx)));
                }
            }
            // Round the top from the edge before it to straight up, then down the axis.
            out.extend(arc(top, before.1.atan2(before.0), FRAC_PI_2));
            out.push(Curve2::Line {
                a: pt(0.0, top.1 + d),
                b: pt(0.0, bottom.1 - d),
                source: None,
            });
            out
        }
    };
    let region = Region {
        outer: Loop { curves },
        holes: vec![],
        source: Some(0),
    };
    let plane = Plane {
        origin: rev.o,
        x_dir: nalgebra::Unit::new_normalize(rev.e1),
        normal: nalgebra::Unit::new_normalize(rev.e1.cross(&rev.a)),
    };
    let axis = Axis {
        origin: rev.o,
        dir: nalgebra::Unit::new_normalize(rev.a),
    };
    let mut k = NativeKernel::new();
    let r = k
        .revolve_with(
            &Profile::new(plane, vec![region]),
            &RevolveSpec::solid(axis, None),
        )
        .ok()?;
    let body = (**k.get(*r.bodies.first()?).ok()?).clone();
    (body.check().is_empty() && super::mass::body_volume(&body) > 0.0).then_some(body)
}

/// A torus or a ball turned a whole turn about an axis that misses it (a torus's own axis along
/// the turn's): exactly the solid of revolution of its shadow on the half plane through the
/// axis. The ball's shadow is its great circle's disc; the torus's a stadium, the band between
/// its tube's nearest and farthest reach ended by the tube's half circles. None for any other
/// body or motion.
fn whole_turn_of_round(
    k: &mut NativeKernel,
    b: &Body,
    steps: &[RigidStep],
) -> Option<Result<OpResult>> {
    let [RigidStep::Turn { axis, from, to }] = steps else {
        return None;
    };
    if (to - from).abs() < TAU - tol::ANGULAR_FINE {
        return None;
    }
    let [face] = b.faces.as_slice() else {
        return None;
    };
    let z = axis.dir.into_inner();
    let (o, round) = match &face.surf {
        Surface::Torus { f, rmaj, rmin } if f.z.cross(&z).norm() < tol::ANGULAR_FINE => {
            (f.o, (*rmaj, *rmin))
        }
        Surface::Sphere { f, r } => (f.o, (0.0, *r)),
        _ => return None,
    };
    let (rmaj, rmin) = round;
    let w = o - axis.origin;
    let h = w.dot(&z);
    let radial = w - z * h;
    let d = radial.norm();
    // The axis must clear the body by a margin (else the shadow reaches it, or wraps it).
    if !(d > rmaj + rmin + tol::POINT.max(1e-9 * d)) {
        return None;
    }
    let x = radial / d;
    let pt = |a: f64, b: f64| nalgebra::Point2::new(a, b);
    let curves = if rmaj == 0.0 {
        vec![Curve2::Circle {
            center: pt(d, h),
            radius: rmin,
            source: None,
        }]
    } else {
        let (near, far) = (d - rmaj, d + rmaj);
        vec![
            Curve2::Line {
                a: pt(near, h - rmin),
                b: pt(far, h - rmin),
                source: None,
            },
            Curve2::Arc {
                center: pt(far, h),
                radius: rmin,
                start_angle: -FRAC_PI_2,
                sweep: PI,
                source: None,
            },
            Curve2::Line {
                a: pt(far, h + rmin),
                b: pt(near, h + rmin),
                source: None,
            },
            Curve2::Arc {
                center: pt(near, h),
                radius: rmin,
                start_angle: FRAC_PI_2,
                sweep: PI,
                source: None,
            },
        ]
    };
    let region = Region {
        outer: Loop { curves },
        holes: vec![],
        source: Some(0),
    };
    // The half plane through the axis: sketch x out from it, y along it.
    let plane = Plane {
        origin: axis.origin,
        x_dir: nalgebra::Unit::new_normalize(x),
        normal: nalgebra::Unit::new_normalize(x.cross(&z)),
    };
    let whole = Axis {
        origin: axis.origin,
        dir: axis.dir,
    };
    Some(k.revolve_with(
        &Profile::new(plane, vec![region]),
        &RevolveSpec::solid(whole, None),
    ))
}

/// The ellipsoid `body` is, as its centre and the linear map `m` taking the unit ball onto it
/// about that centre (`c + m·x`, `m` symmetric), when its one face lies on an ellipsoid: a
/// quadric fitted through points of the face, then every one of a finer grid of its points on
/// it to 1e-10. (An affine map of a ball keeps no record of the ball: its face is a NURBS.)
fn ellipsoid_of(b: &Body) -> Option<(P3, nalgebra::Matrix3<f64>)> {
    let [face] = b.faces.as_slice() else {
        return None;
    };
    if !matches!(face.surf, Surface::Nurbs(_)) || b.shells.len() != 1 || !b.shells[0].closed {
        return None;
    }
    let ((u0, u1), (v0, v1)) = face.surf.param_box();
    let grid = |n: usize| -> Vec<P3> {
        let mut ps = Vec::with_capacity(n * n);
        for i in 0..n {
            for j in 0..n {
                let u = u0 + (u1 - u0) * (i as f64 + 0.5) / n as f64;
                let v = v0 + (v1 - v0) * (j as f64 + 0.37) / n as f64;
                ps.push(face.surf.eval(u, v));
            }
        }
        ps
    };
    let pts = grid(7);
    // Scaled about the points' middle for conditioning.
    let mid = P3::from(pts.iter().map(|p| p.coords).sum::<V3>() / pts.len() as f64);
    let size = pts.iter().map(|p| (p - mid).norm()).fold(0.0, f64::max);
    if !(size > tol::POINT) {
        return None;
    }
    let row = |p: &P3| {
        let q = (p - mid) / size;
        [
            q.x * q.x,
            q.y * q.y,
            q.z * q.z,
            q.x * q.y,
            q.y * q.z,
            q.z * q.x,
            q.x,
            q.y,
            q.z,
            1.0,
        ]
    };
    let a = nalgebra::DMatrix::from_fn(pts.len(), 10, |i, j| row(&pts[i])[j]);
    let svd = (a.transpose() * &a).symmetric_eigen();
    let (k, _) = svd
        .eigenvalues
        .iter()
        .enumerate()
        .min_by(|x, y| x.1.abs().total_cmp(&y.1.abs()))?;
    let q = svd.eigenvectors.column(k);
    let sym = nalgebra::Matrix3::new(
        q[0],
        q[3] / 2.0,
        q[5] / 2.0,
        q[3] / 2.0,
        q[1],
        q[4] / 2.0,
        q[5] / 2.0,
        q[4] / 2.0,
        q[2],
    );
    let lin = V3::new(q[6], q[7], q[8]);
    // (q - c)ᵀ A (q - c) = r in the scaled coordinates.
    let c = -0.5 * sym.try_inverse()? * lin;
    let r = c.dot(&(sym * c)) - q[9];
    let a_n = sym / r;
    let eig = a_n.symmetric_eigen();
    if eig.eigenvalues.iter().any(|&l| !(l > 1e-12)) {
        return None;
    }
    // m = A^(-1/2), back in model units.
    let inv_sqrt = eig.eigenvectors
        * nalgebra::Matrix3::from_diagonal(&eig.eigenvalues.map(|l| 1.0 / l.sqrt()))
        * eig.eigenvectors.transpose();
    let m = inv_sqrt * size;
    let centre = mid + c * size;
    let m_inv = m.try_inverse()?;
    grid(13)
        .iter()
        .all(|p| ((m_inv * (p - centre)).norm() - 1.0).abs() < 1e-10)
        .then_some((centre, m))
}

/// An ellipsoid slid: the unit ball slid along the slide mapped back (sweeping commutes with
/// the ellipsoid's linear map), a capsule, mapped onto it exactly.
fn slid_ellipsoid(k: &mut NativeKernel, b: &Body, steps: &[RigidStep]) -> Option<Result<OpResult>> {
    let [RigidStep::Slide { from, to }] = steps else {
        return None;
    };
    let (c, m) = ellipsoid_of(b)?;
    let m_inv = m.try_inverse()?;
    let ball = super::blend::ball_body(P3::origin(), 1.0).ok()?;
    let id = k.insert(ball);
    let slide = SolidMotion::Rigid {
        steps: vec![RigidStep::Slide {
            from: m_inv * from,
            to: m_inv * to,
        }],
    };
    let r = match sweep_solid(k, id, &slide) {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };
    let capsule = (**k.get(*r.bodies.first()?).ok()?).clone();
    let mapped = super::affine::affine_body(&capsule, &Affine { m, t: c.coords });
    Some(mapped.map(|body| {
        let id = k.insert(body);
        OpResult {
            bodies: vec![id],
            history: History::default(),
        }
    }))
}

pub fn sweep_solid(k: &mut NativeKernel, body: BodyId, motion: &SolidMotion) -> Result<OpResult> {
    // An endless turn or screw of a revolved body: its outline as fine as [`FINE_SLICES`], and
    // where that builds nothing, as [`COARSE_SLICES`].
    if matches!(
        motion,
        SolidMotion::Planar { .. }
            | SolidMotion::Rigid { .. }
            | SolidMotion::Climb { .. }
            | SolidMotion::Path { .. }
    ) {
        return sweep_solid_at(k, body, motion);
    }
    OUTLINES
        .into_iter()
        .map(|n| with_slices(n, || sweep_solid_at(k, body, motion)))
        .reduce(|done, next| done.or(next))
        .expect("an outline to try")
}

/// An endless sweep's outline in closed form ([`exact_outline`]), where [`OUTLINE_SLICES`]
/// asks for no slices.
const EXACT: usize = 0;
/// The outlines an endless sweep tries in turn, until one builds a solid (and, for a cut, one
/// its boolean takes): in closed form, traced finely, traced coarsely.
const OUTLINES: [usize; 3] = [EXACT, FINE_SLICES, COARSE_SLICES];

/// How many slices an endless sweep's outline is traced on where it isn't had in closed form:
/// the cubic B-spline through them is off the outline by the fourth power of their spacing, so
/// four times the slices is 256 times nearer (a wheel turned round a rod: its volume to 1e-8,
/// not 1e-6).
const FINE_SLICES: usize = 1280;
/// The slices of an outline whose finer trace builds no solid, or one a boolean can't take.
const COARSE_SLICES: usize = 160;

thread_local! {
    static OUTLINE_SLICES: std::cell::Cell<usize> = const { std::cell::Cell::new(COARSE_SLICES) };
}

/// `f` with endless sweeps' outlines traced on `n` slices.
fn with_slices<R>(n: usize, f: impl FnOnce() -> R) -> R {
    let was = OUTLINE_SLICES.with(|s| s.replace(n));
    let r = f();
    OUTLINE_SLICES.with(|s| s.set(was));
    r
}

/// A ball screwed about an axis (a turn and a slide along the turn's axis at once): everything
/// within its radius of the arc of a helix its centre runs along, exactly, where the tube about
/// the helix folds through itself (the helix curving tighter than the ball) or its turns
/// overlap.
///
/// The solid's faces lie on the tube about the helix (the ball's great circle square to the
/// helix, screwed along it) and on the ball at each end, and a point of those surfaces is on
/// the solid exactly where nothing of the arc is nearer it than the ball's radius. So: the
/// tube in short stretches and the two spheres, each cut along where the others cross it
/// ([`super::boolean::imprint`]), the pieces kept whose inside point is the radius from the
/// arc, and those sewn.
fn screwed_ball(k: &mut NativeKernel, b: &Body, steps: &[RigidStep]) -> Option<Result<OpResult>> {
    let (centre, r) = match b.faces.first().map(|f| &f.surf) {
        Some(Surface::Sphere { f, r }) => (f.o, *r),
        _ => return None,
    };
    let same = |s: &Surface| {
        matches!(s, Surface::Sphere { f, r: q }
            if (f.o - centre).norm() <= tol::POINT && (q - r).abs() <= tol::POINT)
    };
    if !b.faces.iter().all(|f| same(&f.surf)) {
        return None;
    }
    let (axis, turn, slide) = match steps {
        [
            RigidStep::Turn { axis, from, to },
            RigidStep::Slide { from: s0, to: s1 },
        ]
        | [
            RigidStep::Slide { from: s0, to: s1 },
            RigidStep::Turn { axis, from, to },
        ] => (*axis, to - from, s1 - s0),
        _ => return None,
    };
    let z = axis.dir.into_inner();
    let rise = slide.dot(&z);
    if turn <= 1e-9 || (slide - z * rise).norm() > tol::POINT {
        return None;
    }
    let pose = |t: f64| {
        let (q, d) = rigid_at(steps, t);
        Affine {
            m: q.into_inner(),
            t: d,
        }
    };
    let first = pose(0.0);
    let start = first.point(&centre);
    let foot = axis.origin + z * (start - axis.origin).dot(&z);
    let a = (start - foot).norm();
    if a <= 1e-6 * r {
        return None;
    }
    // The helix: its radius a, rising c a radian.
    let c = rise / turn;
    let e = (start - foot) / a;
    let ahead = z.cross(&e);
    let tangent = (ahead * a + z * c).normalize();
    let helix_at = |w: f64| foot + (e * w.cos() + ahead * w.sin()) * a + z * (c * w);
    // How far a point is from the arc: the least over its samples, then by Newton on the
    // distance's derivative from each sample that is a least among its neighbours.
    let from_arc = |p: &P3| -> f64 {
        const N: usize = 1440;
        let d2 = |w: f64| (helix_at(w) - p).norm_squared();
        let ds: Vec<f64> = (0..=N).map(|i| d2(turn * i as f64 / N as f64)).collect();
        let mut best = ds[0].min(ds[N]);
        for i in 0..=N {
            let lo = if i == 0 { f64::INFINITY } else { ds[i - 1] };
            let hi = if i == N { f64::INFINITY } else { ds[i + 1] };
            if ds[i] <= lo && ds[i] <= hi {
                let mut w = turn * i as f64 / N as f64;
                for _ in 0..12 {
                    let q = helix_at(w);
                    let d1 = (-e * w.sin() + ahead * w.cos()) * a + z * c;
                    let dd = -(e * w.cos() + ahead * w.sin()) * a;
                    let (g, h) = ((q - p).dot(&d1), d1.norm_squared() + (q - p).dot(&dd));
                    if h.abs() < 1e-14 {
                        break;
                    }
                    w = (w - g / h).clamp(0.0, turn);
                }
                best = best.min(d2(w));
            }
        }
        best.sqrt()
    };
    // The ball's great circle square to the helix at its start, in four arcs.
    let plane = Plane {
        origin: start,
        x_dir: nalgebra::Unit::new_normalize(-e),
        normal: nalgebra::Unit::new_normalize(tangent),
    };
    let o2 = nalgebra::Point2::new(0.0, 0.0);
    // Where the helix curves tighter than the ball the tube folds: along the two lines where
    // the circle is the helix's radius of curvature toward its centre the tube turns back, and
    // the strip between them (nearer that centre) is inside the solid all along. The circle is
    // cut there, and that strip left out.
    let rho = (a * a + c * c) / a;
    let fold = (rho < r * (1.0 - 1e-9)).then(|| (rho / r).acos());
    let curves: Vec<Curve2> = match fold {
        Some(f) => std::iter::once(Curve2::arc(o2, r, -f, 2.0 * f))
            .chain((0..4).map(|i| {
                let q = (TAU - 2.0 * f) / 4.0;
                Curve2::arc(o2, r, f + q * i as f64, q)
            }))
            .collect(),
        None => (0..4)
            .map(|i| Curve2::arc(o2, r, FRAC_PI_2 * (i as f64 - 0.5), FRAC_PI_2))
            .collect(),
    };
    // (In stretches of thirty degrees: short enough that one meets itself nowhere.)
    let n = (turn / 30f64.to_radians()).ceil().max(1.0) as usize;
    let step = turn / n as f64;
    let debug = crate::native::env_flag!("CADRS_SWEEP_SOLID_DEBUG");
    let made = (|| -> Result<Body> {
        let region = Region {
            outer: Loop { curves },
            holes: vec![],
            source: Some(0),
        };
        let path = helix_path(foot, z, e, ahead, a, TAU * c, step / TAU);
        let prof = Profile::new(plane, vec![region]);
        let regions = super::profile::regions(&prof)?;
        let faces = super::profile::merge_regions(&regions)?;
        let segs = super::sweep::path::path_segments(k, &path)?;
        let helix = super::sweep::frames::Helix::new(foot, z, e, ahead, a, c, 0.0);
        let (built, _) = super::sweep::frames::sweep_seg(
            &faces,
            &[],
            plane,
            &segs,
            super::sweep::Frames::Control(SweepControl::None),
            true,
            Some(helix),
        )?;
        let wall: Vec<usize> = (0..built.body.faces.len())
            .filter(|&f| !matches!(built.body.faces[f].surf, Surface::Plane { .. }))
            .filter(|&f| {
                let Some(((u0, u1), _)) = super::heal_split::face_uv_box(&built.body, f) else {
                    return true;
                };
                let mid = (0.5 * (u0 + u1)).rem_euclid(TAU);
                fold.is_none_or(|w| mid.min(TAU - mid) > w)
            })
            .collect();
        let stretch = super::kernel::subset(&built.body, &wall, false);
        let on = |i: usize| {
            let w = step * i as f64;
            let q = nalgebra::Rotation3::from_axis_angle(&axis.dir, w).into_inner();
            Affine {
                m: q,
                t: axis.origin.coords - q * axis.origin.coords + z * (c * w),
            }
        };
        // The sheets, a face each: the tube's stretches' and the two spheres'.
        let all = |x: &Body| -> Vec<usize> { (0..x.faces.len()).collect() };
        let mut sheets: Vec<Body> = Vec::new();
        // (The stretch each of the tube's sheets is of.)
        let mut of: Vec<Option<usize>> = Vec::new();
        // (The end each of the spheres' sheets is at: whether the start.)
        let mut end_of: Vec<Option<bool>> = Vec::new();
        for i in 0..n {
            let s = stretch.transformed(&on(i));
            for f in 0..s.faces.len() {
                sheets.push(super::kernel::subset(&s, &[f], false));
                of.push(Some(i));
                end_of.push(None);
            }
        }
        // The balls' halves beyond the arc's ends: behind the helix's plane at its start, and
        // that ball's other half screwed on to the end. (The halves toward the arc are inside
        // the tube, which touches the spheres along the circles between.)
        {
            let disc = Region {
                outer: Loop {
                    curves: vec![
                        Curve2::arc(o2, 1.5 * r, 0.0, PI),
                        Curve2::arc(o2, 1.5 * r, PI, PI),
                    ],
                },
                holes: vec![],
                source: Some(0),
            };
            let mut scratch = NativeKernel::new();
            let behind = scratch.extrude_with(
                &Profile::new(plane, vec![disc]),
                &ExtrudeSpec::blind(nalgebra::Unit::new_normalize(-tangent), 1.5 * r),
            )?;
            let behind = (**scratch.get(behind.bodies[0])?).clone();
            let ball = b.transformed(&first);
            let opts = super::boolean::BoolOptions::default();
            let mut halves = Vec::new();
            for op in [BoolOp::Intersect, BoolOp::Subtract] {
                halves.push(super::boolean::boolean_bodies_with(&ball, &behind, op, opts)?.body);
            }
            for (place, at_start) in [(on(0), true), (on(n), false)] {
                for half in &halves {
                    for f in all(half) {
                        if matches!(half.faces[f].surf, Surface::Sphere { .. }) {
                            sheets
                                .push(super::kernel::subset(half, &[f], false).transformed(&place));
                            of.push(None);
                            end_of.push(Some(at_start));
                        }
                    }
                }
            }
        }
        // The angle about the helix, d radians of its turn in from an end, of the point of
        // the tube's circle there that is on that end's ball (and the circle's centre and
        // axes): see below.
        let leaving = |at_start: bool, branch: f64, d: f64| -> Option<(f64, P3, V3, V3)> {
            let (w0, dir) = if at_start { (0.0, 1.0) } else { (turn, -1.0) };
            let w = w0 + dir * d;
            let es = e * w.cos() + ahead * w.sin();
            let t = ((ahead * w.cos() - e * w.sin()) * a + z * c).normalize();
            let (nn, bn) = (-es, t.cross(&-es));
            let delta = helix_at(w) - helix_at(w0);
            let (ca, cb) = (delta.dot(&nn), delta.dot(&bn));
            let x = -delta.norm_squared() / (2.0 * r * ca.hypot(cb));
            (x.abs() <= 1.0).then(|| {
                let phi = cb.atan2(ca) + branch * x.acos();
                ((phi + PI).rem_euclid(TAU) - PI, helix_at(w), nn, bn)
            })
        };
        // The stretch at each end, where the fold shows: its face beside the fold's line ends
        // not on that line but on the curve where the tube leaves the ball, which starts along
        // the line (the two touch there), too near it to be cut from the face. So that face
        // is made on the tube between the curve and its far side.
        let mut made_whole = vec![false; sheets.len()];
        if let Some(f) = fold {
            let q = (TAU - 2.0 * f) / 4.0;
            let per = stretch.faces.len();
            for at_start in [true, false] {
                for branch in [1.0, -1.0] {
                    // (The branch that shows: its points no nearer the rest of the arc.)
                    let shows =
                        leaving(at_start, branch, 0.5 * step).is_some_and(|(phi, o, nn, bn)| {
                            from_arc(&(o + (nn * phi.cos() + bn * phi.sin()) * r)) >= r - 1e-6
                        });
                    let Some((near, ..)) = leaving(at_start, branch, 1e-3) else {
                        continue;
                    };
                    if !shows {
                        continue;
                    }
                    let side = near.signum();
                    let far = side * (f + q);
                    const ACROSS: usize = 24;
                    const ALONG: usize = 32;
                    let across: Vec<f64> = (0..=ACROSS).map(|i| i as f64 / ACROSS as f64).collect();
                    let mut rows = Vec::new();
                    let mut at = Vec::new();
                    for j in 0..=ALONG {
                        let d = step * j as f64 / ALONG as f64;
                        let Some((phi, o, nn, bn)) = leaving(at_start, branch, d.max(1e-9)) else {
                            break;
                        };
                        let phi = if j == 0 { side * f } else { phi };
                        let pts: Vec<P3> = across
                            .iter()
                            .map(|x| {
                                let u = phi + (far - phi) * x;
                                o + (nn * u.cos() + bn * u.sin()) * r
                            })
                            .collect();
                        rows.push(super::nurbs::NurbsCurve::interpolate(
                            &pts, &across, 3, None, None,
                        ));
                        at.push(d);
                    }
                    if rows.len() <= ALONG {
                        continue;
                    }
                    let surf =
                        Surface::Nurbs(std::sync::Arc::new(super::loft_build::skin(&rows, &at)));
                    let Ok(sheet) = super::face_build::face_from_surface(&surf, None) else {
                        continue;
                    };
                    // (The face it stands in for: the end's stretch's, on the fold's side.)
                    let i = if at_start { 0 } else { n - 1 };
                    let which = (0..per).find(|&x| {
                        super::heal_split::face_uv_box(&stretch, x).is_some_and(|((u0, u1), _)| {
                            let mid = (0.5 * (u0 + u1) + PI).rem_euclid(TAU) - PI;
                            mid * side > 0.0 && mid.abs() < f + q
                        })
                    });
                    if let Some(x) = which {
                        if debug {
                            eprintln!(
                                "screwed ball: sheet {} made whole, from {:.4} to {far:.4}",
                                i * per + x,
                                side * f
                            );
                        }
                        sheets[i * per + x] = sheet;
                        made_whole[i * per + x] = true;
                    }
                }
            }
        }
        // Where the tube leaves an end's ball the two cross along a curve that starts where
        // they touch, too flat there for the search to find: but in closed form. The tube's
        // circle at w is c(w) + r (cos φ N + sin φ B); its points r from the end's centre c₀
        // are where Δ·N cos φ + Δ·B sin φ = −|Δ|²/2r, Δ = c(w) − c₀. The first two stretches
        // from an end and that end's sphere are cut along it by a sheet of the rays from c₀
        // through the curve, which crosses both squarely, in place of one another.
        const GUIDED: usize = 2;
        let mut guides: Vec<(Body, bool)> = Vec::new();
        for at_start in [true, false] {
            let c0 = helix_at(if at_start { 0.0 } else { turn });
            let reach = (GUIDED.min(n) as f64) * step;
            for branch in [1.0, -1.0] {
                const ALONG: usize = 48;
                // (From a little before the end to a little past the stretches, and not at
                // the end itself, where every point of the circle is on the ball.)
                let pts: Vec<P3> = (0..=ALONG)
                    .map(|i| -0.06 + (reach + 0.12) * i as f64 / ALONG as f64)
                    .filter(|d| d.abs() >= 2e-3)
                    .filter_map(|d| leaving(at_start, branch, d))
                    .map(|(phi, o, nn, bn)| o + (nn * phi.cos() + bn * phi.sin()) * r)
                    .collect();
                if pts.len() < 8 {
                    continue;
                }
                let mut params = vec![0.0];
                for w in pts.windows(2) {
                    params.push(params.last().unwrap() + (w[1] - w[0]).norm());
                }
                let curve = super::nurbs::NurbsCurve::interpolate(&pts, &params, 3, None, None);
                let ray = |h: &super::nurbs::H, f: f64| {
                    let q = c0 + (super::nurbs::dehom(h) - c0) * f;
                    super::nurbs::H::new(q.x, q.y, q.z, 1.0)
                };
                let surf = Surface::Nurbs(std::sync::Arc::new(super::nurbs::NurbsSurface {
                    du: curve.degree,
                    dv: 1,
                    ku: curve.knots.clone(),
                    kv: vec![0.0, 0.0, 1.0, 1.0],
                    cps: curve
                        .cps
                        .iter()
                        .map(|h| vec![ray(h, 0.75), ray(h, 1.25)])
                        .collect(),
                }));
                if let Ok(sheet) = super::face_build::face_from_surface(&surf, None) {
                    guides.push((sheet, at_start));
                }
            }
        }
        if debug {
            eprintln!("screwed ball: {} guide sheets", guides.len());
        }
        // Whether sheet i is one an end's guides stand in for against sheet j: that end's
        // sphere against the stretches next to it.
        let ball_at = |i: usize| -> Option<bool> { end_of[i] };
        let near_end = |i: usize, at_start: bool| -> bool {
            of[i].is_some_and(|x| {
                if at_start {
                    x < GUIDED
                } else {
                    x + GUIDED >= n
                }
            })
        };
        let guided = |i: usize, j: usize| -> Option<bool> {
            match (ball_at(i), ball_at(j)) {
                (Some(end), None) if near_end(j, end) => Some(end),
                (None, Some(end)) if near_end(i, end) => Some(end),
                _ => None,
            }
        };
        // A sheet's point inside a face of it.
        let inside = |x: &Body, f: usize| -> Option<P3> {
            let polys = super::classify::face_polygons(x, &x.faces[f]);
            let uv = super::boolean::interior_points_of_polys(&polys)
                .into_iter()
                .next()?;
            Some(x.faces[f].surf.eval(uv.x, uv.y))
        };
        // The sheets wholly inside the solid (every sample of theirs deeper than the samples
        // lie apart: the distance changes no faster than the point moves) cut nothing seen
        // and are left out.
        let buried = |x: &Body| -> bool {
            const M: usize = 10;
            x.faces.iter().all(|f| {
                let polys = super::classify::face_polygons(x, f);
                let mut uv = Bbox::empty();
                for q in polys.iter().flatten() {
                    uv.add(&P3::new(q.x, q.y, 0.0));
                }
                if uv.is_empty() {
                    return false;
                }
                let at = |i: usize, j: usize| {
                    f.surf.eval(
                        uv.min.x + (uv.max.x - uv.min.x) * i as f64 / M as f64,
                        uv.min.y + (uv.max.y - uv.min.y) * j as f64 / M as f64,
                    )
                };
                let mut gap: f64 = 0.0;
                let mut shallow: f64 = 0.0;
                for i in 0..=M {
                    for j in 0..=M {
                        let p = at(i, j);
                        shallow = shallow.max(from_arc(&p));
                        if i < M {
                            gap = gap.max((at(i + 1, j) - p).norm());
                        }
                        if j < M {
                            gap = gap.max((at(i, j + 1) - p).norm());
                        }
                    }
                }
                shallow < r - gap
            })
        };
        let seen: Vec<bool> = sheets.iter().map(|x| !buried(x)).collect();
        let boxes: Vec<Bbox> = sheets.iter().map(|x| x.bbox().grow(1e-6)).collect();
        if debug {
            eprintln!(
                "screwed ball: {n} stretches, {} sheets, {} not buried",
                sheets.len(),
                seen.iter().filter(|s| **s).count()
            );
        }
        let mut kept: Vec<Body> = Vec::new();
        for i in 0..sheets.len() {
            if !seen[i] {
                continue;
            }
            // (By all the others it reaches at once: a crease crosses a face against two
            // stretches of the turn beside it, and a cut by one alone ends inside the face.)
            let mut others = Body::default();
            for j in 0..sheets.len() {
                // (Not the tube's own stretch or the ones next to it: the surface goes on
                // through them, and the tube meets itself only a turn on or at its ends.)
                let beside = matches!((of[i], of[j]), (Some(x), Some(y)) if x.abs_diff(y) <= 1);
                if j != i
                    && seen[j]
                    && !beside
                    && guided(i, j).is_none()
                    && !(end_of[i].is_some() && end_of[i] == end_of[j])
                    && boxes[i].overlaps(&boxes[j])
                {
                    others.append(&sheets[j]);
                }
            }
            for (g, at_start) in &guides {
                let mine = match ball_at(i) {
                    Some(end) => end == *at_start,
                    None => near_end(i, *at_start),
                };
                if mine && !made_whole[i] {
                    others.append(g);
                }
            }
            let cur = if others.faces.is_empty() {
                sheets[i].clone()
            } else {
                match super::boolean::imprint(&sheets[i], &others, None) {
                    Ok(out) => out.body,
                    Err(e) => {
                        if debug {
                            eprintln!(
                                "screwed ball: sheet {i} ({:?}) by {} faces: {e}",
                                of[i],
                                others.faces.len()
                            );
                        }
                        sheets[i].clone()
                    }
                }
            };
            let on_it: Vec<usize> = (0..cur.faces.len())
                .filter(|&f| inside(&cur, f).is_some_and(|p| from_arc(&p) >= r - 1e-6))
                .collect();
            if debug {
                eprintln!(
                    "screwed ball: sheet {i}: {} pieces, {} kept",
                    cur.faces.len(),
                    on_it.len()
                );
            }
            if !on_it.is_empty() {
                kept.push(super::kernel::subset(&cur, &on_it, false));
            }
        }
        let mut whole = Body::default();
        for x in &kept {
            whole.append(x);
        }
        super::surfacing::sewn_solid(whole, 1e-5)
    })();
    if debug {
        match &made {
            Ok(t) => eprintln!("screwed ball: {} faces", t.faces.len()),
            Err(e) => eprintln!("screwed ball: failed: {e}"),
        }
    }
    Some(made.map(|body| OpResult {
        bodies: vec![k.insert(body)],
        history: History::default(),
    }))
}

/// [`sweep_solid`], an endless sweep's outline on the slices [`with_slices`] set.
fn sweep_solid_at(k: &mut NativeKernel, body: BodyId, motion: &SolidMotion) -> Result<OpResult> {
    let b = std::sync::Arc::clone(k.get(body)?);
    if let SolidMotion::Planar { normal, steps } = motion {
        return super::sweep_planar::sweep_planar(k, &b, normal.into_inner(), steps);
    }
    // One slide, turn or screw of any solid: exactly, from its leading faces' sweeps (split
    // where its faces meet the motion); else meshed.
    if let SolidMotion::Rigid { steps } = motion {
        if let Some(r) = whole_turn_of_round(k, &b, steps) {
            return r;
        }
        if let Some(r) = slid_ellipsoid(k, &b, steps) {
            return r;
        }
        let mut exact = None;
        // A pipe slid: exactly, as everything within its radius of the strip its spine sweeps.
        if let [RigidStep::Slide { from, to }] = steps.as_slice()
            && from.norm() == 0.0
            && let Some(pipe) = k.pipes.get(&body.0).cloned()
        {
            match super::sweep_pipe::slide(k, &pipe, *to) {
                Some(Ok(r)) => return Ok(r),
                Some(Err(e)) => exact = Some(e),
                None => {}
            }
        }
        // (A torus by two steps or more first as a torus: its faces' own sweeps, which make
        // one step's exactly in closed form, seldom join for two, and are slow to find so.)
        // (And a torus slid: where it leans far enough that its outline folds, its leading
        // faces' sweeps cross one another and still sew into a solid, one too large.)
        let mut as_torus = None;
        if steps.len() >= 2 || slid_torus(&b, steps) || crate::native::env_flag!("CADRS_TUBE_FIRST")
        {
            as_torus = super::sweep_tube::torus(k, &b, &super::sweep_tube::Motion::Steps(steps));
            if let Some(Ok(r)) = as_torus {
                return Ok(r);
            }
        }
        if (1..=2).contains(&steps.len()) {
            match super::sweep_faces::by_faces(k, &b, steps) {
                Ok(r) => return Ok(r),
                Err(e) => exact = Some(e),
            }
        }
        // A ball screwed: exactly, its tube and end balls trimmed where they pass through
        // one another.
        if exact.is_some()
            && let Some(Ok(r)) = screwed_ball(k, &b, steps)
        {
            return Ok(r);
        }
        let place = |t: f64| rigid_at(steps, t);
        // A convex polyhedron whose contacts move over its faces: exactly, from its leading
        // polygons' sweeps.
        match super::sweep_moving::polyhedron(k, &b, &place) {
            Some(Ok(r)) => return Ok(r),
            Some(Err(e)) => exact = exact.or(Some(e)),
            None => {}
        }
        // A torus: exactly, from the offsets of its core circle's sweep.
        match as_torus
            .or_else(|| super::sweep_tube::torus(k, &b, &super::sweep_tube::Motion::Steps(steps)))
        {
            Some(Ok(r)) => return Ok(r),
            Some(Err(e)) => exact = exact.or(Some(e)),
            None => {}
        }
        return match revolved(&b) {
            Some(rev) => meshed(k, &b, &|p| rev.distance(p), &place),
            None => meshed_body(k, &b, &place).map_err(|e| exact.unwrap_or(e)),
        };
    }
    if let SolidMotion::Climb {
        axis,
        from,
        to,
        rise,
    } = motion
    {
        let place = |t: f64| climb_at(axis, *from, *to, rise, t);
        return match revolved(&b) {
            Some(rev) => meshed(k, &b, &|p| rev.distance(p), &place),
            None => meshed_body(k, &b, &place),
        };
    }
    if let SolidMotion::Path { path, up } = motion {
        if let Some(r) = path_sweep_exact(k, body, &b, path, up.into_inner()) {
            return Ok(r);
        }
        // A torus or a bull-nose mill riding one curve: exactly, its envelope trimmed by what
        // it covers of itself.
        if let Some(Ok(r)) = torus_along(k, &b, path, up.into_inner()) {
            return Ok(r);
        }
        let frames = PathFrames::new(k, path, up.into_inner())?;
        let place = |t: f64| frames.place(t);
        return match revolved(&b) {
            Some(rev) => meshed(k, &b, &|p| rev.distance(p), &place),
            None => meshed_body(k, &b, &place),
        };
    }
    let rev = revolved(&b).ok_or(KernelError::Unsupported(
        "sweeping solids other than a torus or a convex section revolved",
    ))?;
    let (axis, kk, span) = match *motion {
        SolidMotion::Planar { .. }
        | SolidMotion::Rigid { .. }
        | SolidMotion::Path { .. }
        | SolidMotion::Climb { .. } => {
            unreachable!()
        }
        SolidMotion::Turn { axis } => (axis, 0.0, None),
        SolidMotion::Screw {
            axis,
            rise,
            from,
            to,
        } => {
            if !(rise.abs() > tol::POINT && to > from) {
                return Err(KernelError::InvalidParameter(
                    "A screw needs a rise and a span along its axis".into(),
                ));
            }
            (axis, TAU / rise, Some((from, to)))
        }
    };
    let z = axis.dir.into_inner();
    let sl = Slicer {
        body: &rev,
        origin: axis.origin,
        z,
        k: kk,
        x: any_perpendicular(&z),
        y: z.cross(&any_perpendicular(&z)),
    };
    // The body's centre's key, about which a screw's keys are unwrapped.
    let near = sl.key(&rev.o, 0.0);
    // The motion's axis must miss the body (an outline touching it would need a side along it):
    // its points across the body's reach, each tested in the body's section.
    {
        let size = b.bbox().diagonal();
        let tc = (rev.o - axis.origin).dot(&z);
        let n = 2048;
        for i in 0..=n {
            let t = tc - size + 2.0 * size * i as f64 / n as f64;
            let w = axis.origin + z * t - rev.o;
            let h = w.dot(&rev.a);
            let rho = (w - rev.a * h).norm();
            if rev.section.contains((rho, h)) {
                return Err(KernelError::Unsupported(
                    "sweeping a solid its axis passes through",
                ));
            }
        }
    }
    let (lo, hi) = key_range(&sl, near);
    if kk != 0.0 && hi.key - lo.key >= PI {
        return Err(KernelError::Unsupported(
            "screwing a solid that reaches half way round the screw's axis",
        ));
    }
    let n = OUTLINE_SLICES.with(|s| s.get());
    // The section's plane: through the axis for a turn (x the distance, y the height); the plane
    // square to it at height 0 for a screw (polar: distance, angle).
    let x = any_perpendicular(&z);
    let y = z.cross(&x);
    let p2 = |(r, q): (f64, f64)| -> nalgebra::Point2<f64> {
        if kk == 0.0 {
            nalgebra::Point2::new(r, q)
        } else {
            nalgebra::Point2::new(r * q.cos(), r * q.sin())
        }
    };
    // The outline in closed form where it can be had (and the finer trace is asked for: the
    // coarser is what a boolean falls back on); else traced slice by slice.
    let rise = if kk == 0.0 { 0.0 } else { 1.0 / kk };
    let exact = (n == EXACT && !crate::native::env_flag!("CADRS_OUTLINE_SLICED"))
        .then(|| exact_outline(&sl, near, rise, &p2, (lo.key, hi.key)))
        .flatten();
    let n = if n == EXACT { FINE_SLICES } else { n };
    let (curves, reach) = match exact {
        Some(found) => found,
        None => {
            let ol = outline(&sl, lo, hi, n).ok_or_else(|| KernelError::InvalidResult {
                op: "solid sweep",
                why: "the body's outline has a slice it misses".into(),
            })?;
            sliced_outline(&sl, &ol, &p2)
        }
    };
    let region = Region {
        outer: Loop { curves },
        holes: vec![],
        source: Some(0),
    };
    match span {
        None => {
            // The half plane through the axis: sketch x out from it, y along it.
            let plane = Plane {
                origin: axis.origin,
                x_dir: nalgebra::Unit::new_normalize(x),
                normal: nalgebra::Unit::new_normalize(x.cross(&z)),
            };
            k.revolve_with(
                &Profile::new(plane, vec![region]),
                &RevolveSpec::solid(axis, None),
            )
        }
        Some((from, to)) => {
            // The plane square to the axis at `from`, its sketch axes x and y; the region turned
            // to where the screw has it there, then swept up the helix to `to`.
            let turn = kk * from;
            let (c, s) = (turn.cos(), turn.sin());
            let (xr, yr) = (x * c + y * s, y * c - x * s);
            let plane = Plane {
                origin: axis.origin + z * from,
                x_dir: nalgebra::Unit::new_normalize(xr),
                normal: nalgebra::Unit::new_normalize(xr.cross(&yr)),
            };
            // The helix through a point of the region (its first pole's distance, at angle 0
            // of the plane), rising to `to`.
            let r0 = reach.max(tol::POINT);
            let turns = (to - from) * kk / TAU;
            let path = helix_path(plane.origin, z, xr, yr, r0, TAU / kk, turns);
            // Swept by the screw's own motion (a path this short is too little of a helix for
            // the sweep to recognise one).
            let prof = Profile::new(plane, vec![region]);
            let regions = super::profile::regions(&prof)?;
            let faces = super::profile::merge_regions(&regions)?;
            let segs = super::sweep::path::path_segments(k, &path)?;
            let helix =
                super::sweep::frames::Helix::new(plane.origin, z, xr, yr, r0, TAU / kk / TAU, 0.0);
            let (built, _) = super::sweep::frames::sweep_seg(
                &faces,
                &[],
                plane,
                &segs,
                super::sweep::Frames::Control(SweepControl::None),
                true,
                Some(helix),
            )?;
            Ok(super::features::Tagged::from_built(built).into_result(k))
        }
    }
}

/// The traced outline ([`outline`]) as its closed loop of curves in the section's plane, with
/// its greatest distance from the axis: each side a cubic B-spline through its points (by the
/// chords' square roots), piece by piece between its corners, the sides joined at the ends by
/// the extreme slices' stretches where they don't meet.
fn sliced_outline(
    sl: &Slicer,
    ol: &Outline,
    p2: &dyn Fn((f64, f64)) -> nalgebra::Point2<f64>,
) -> (Vec<Curve2>, f64) {
    if crate::native::env_flag!("CADRS_SWEEP_SOLID_DEBUG") {
        for (name, side) in [("far", &ol.far), ("near", &ol.near)] {
            let mut turns: Vec<(f64, usize)> = Vec::new();
            for i in 1..side.len() - 1 {
                let (a, b, c) = (side[i - 1], side[i], side[i + 1]);
                let (u, v) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
                let ang = (u.0 * v.1 - u.1 * v.0).atan2(u.0 * v.0 + u.1 * v.1).abs();
                turns.push((ang, i));
            }
            turns.sort_by(|a, b| b.0.total_cmp(&a.0));
            eprintln!(
                "{name}: {} points, sharpest turns {:?}",
                side.len(),
                &turns[..4.min(turns.len())]
            );
        }
    }
    let fit = |pts: &[(f64, f64)]| -> Curve2 {
        let ps: Vec<P3> = pts
            .iter()
            .map(|&q| {
                let p = p2(q);
                P3::new(p.x, p.y, 0.0)
            })
            .collect();
        let mut params = vec![0.0];
        for w in ps.windows(2) {
            params.push(params.last().unwrap() + (w[1] - w[0]).norm().sqrt());
        }
        let c = super::nurbs::NurbsCurve::interpolate(&ps, &params, 3, None, None);
        Curve2::Spline {
            degree: c.degree,
            knots: c.knots.clone(),
            poles: c
                .cps
                .iter()
                .map(|h| nalgebra::Point2::new(h.x / h.w, h.y / h.w))
                .collect(),
            weights: vec![],
            source: None,
        }
    };
    // A side's corners (where the extreme passes from one part of the body to another: a sharp
    // edge's image meeting a face's fold, a V's tip): it is fitted piece by piece between them,
    // each corner placed where the side's distance is extreme (a tip) or where its two
    // neighbouring chords' lines meet.
    let pieces = |side: &[(f64, f64)], far: bool| -> Vec<Vec<(f64, f64)>> {
        let turn = |i: usize| {
            let (a, b, c) = (side[i - 1], side[i], side[i + 1]);
            let (u, v) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
            (u.0 * v.1 - u.1 * v.0).atan2(u.0 * v.0 + u.1 * v.1).abs()
        };
        let mut cuts: Vec<usize> = Vec::new();
        for i in 2..side.len().saturating_sub(2) {
            if turn(i) > 0.2 && turn(i) >= turn(i - 1) && turn(i) >= turn(i + 1) {
                cuts.push(i);
            }
        }
        let mut out = Vec::new();
        let mut start = 0;
        let mut first = side[0];
        for &i in &cuts {
            let (a, b, c) = (side[i - 1], side[i], side[i + 1]);
            let sign = if far { 1.0 } else { -1.0 };
            let corner = if sign * b.0 >= sign * a.0 && sign * b.0 >= sign * c.0 {
                // A tip: the extreme of the side's distance over the slices about it.
                let f = |q: f64| {
                    sl.extreme(q, far, 0.0)
                        .map_or(f64::NEG_INFINITY, |e| sign * e.0)
                };
                let (q, v) = golden_max(&f, a.1, c.1);
                (sign * v, q)
            } else {
                // Two branches crossing: their chords' lines extended.
                let (p, d) = (side[i - 2], side[i - 1]);
                let (e, g) = (side[i + 1], side[i + 2]);
                let (u, v) = ((d.0 - p.0, d.1 - p.1), (g.0 - e.0, g.1 - e.1));
                let den = u.0 * v.1 - u.1 * v.0;
                if den.abs() < 1e-300 {
                    b
                } else {
                    let t = ((e.0 - p.0) * v.1 - (e.1 - p.1) * v.0) / den;
                    (p.0 + u.0 * t, p.1 + u.1 * t)
                }
            };
            let mut piece = vec![first];
            piece.extend_from_slice(&side[start + 1..i]);
            piece.push(corner);
            out.push(piece);
            start = i;
            first = corner;
        }
        let mut last = vec![first];
        last.extend_from_slice(&side[start + 1..]);
        out.push(last);
        out
    };
    let mut curves: Vec<Curve2> = pieces(&ol.far, true).iter().map(|p| fit(p)).collect();
    let (fe, ne) = (*ol.far.last().unwrap(), *ol.near.last().unwrap());
    if fe != ne {
        curves.push(Curve2::Line {
            a: p2(fe),
            b: p2(ne),
            source: None,
        });
    }
    let mut near: Vec<Curve2> = pieces(&ol.near, false)
        .iter()
        .rev()
        .map(|p| {
            let back: Vec<(f64, f64)> = p.iter().rev().copied().collect();
            fit(&back)
        })
        .collect();
    curves.append(&mut near);
    let (fs, ns) = (ol.far[0], ol.near[0]);
    if fs != ns {
        curves.push(Curve2::Line {
            a: p2(ns),
            b: p2(fs),
            source: None,
        });
    }
    let reach = ol.far.iter().map(|p| p.0).fold(0.0, f64::max);
    (curves, reach)
}

/// A helix as rational quadratic pieces (sixteen a turn) about the axis through `o` along `z`,
/// starting at radius `r` toward `x` (`y` a quarter turn on), rising `pitch` a turn, `turns`
/// turns (negative: left-handed).
fn helix_path(o: P3, z: V3, x: V3, y: V3, r: f64, pitch: f64, turns: f64) -> Vec<PathCurve> {
    let n = ((turns.abs() * 16.0).ceil() as usize).max(1);
    let at = |t: f64, rr: f64, h: f64| o + (x * t.cos() + y * t.sin()) * rr + z * h;
    let total = TAU * turns;
    (0..n)
        .map(|i| {
            let (t0, t1) = (
                total * i as f64 / n as f64,
                total * (i + 1) as f64 / n as f64,
            );
            let half = (t1 - t0) / 2.0;
            let (h0, h1) = (pitch * t0 / TAU, pitch * t1 / TAU);
            PathCurve::Bezier3 {
                poles: vec![
                    at(t0, r, h0),
                    at(t0 + half, r / half.cos(), 0.5 * (h0 + h1)),
                    at(t1, r, h1),
                ],
                weights: vec![1.0, half.cos(), 1.0],
            }
        })
        .collect()
}

impl Section {
    /// The signed distance of (ρ, h) from the section (negative inside).
    fn distance(&self, q: (f64, f64)) -> f64 {
        match self {
            Section::Disk { c, r } => (q.0 - c.0).hypot(q.1 - c.1) - r,
            Section::Polygon(p) => {
                let n = p.len();
                let mut d = f64::INFINITY;
                for i in 0..n {
                    let (a, b) = (p[i], p[(i + 1) % n]);
                    let (ab0, ab1) = (b.0 - a.0, b.1 - a.1);
                    let t = (((q.0 - a.0) * ab0 + (q.1 - a.1) * ab1) / (ab0 * ab0 + ab1 * ab1))
                        .clamp(0.0, 1.0);
                    d = d.min((a.0 + ab0 * t - q.0).hypot(a.1 + ab1 * t - q.1));
                }
                if self.contains(q) { -d } else { d }
            }
        }
    }
}

impl Revolved {
    /// The body's signed distance at p (negative inside): its section's in the half plane
    /// through its axis and p.
    fn distance(&self, p: &P3) -> f64 {
        let w = p - self.o;
        let h = w.dot(&self.a);
        self.section.distance(((w - self.a * h).norm(), h))
    }
}

/// A rigid motion's place at t: the rotation and translation taking the body's points there.
/// The rise of [`SolidMotion::Climb`] at the turn θ (its polynomial's coefficients, constant
/// first).
fn climb_rise(rise: &[f64], a: f64) -> f64 {
    rise.iter().rev().fold(0.0, |acc, c| acc * a + c)
}

/// The place of [`SolidMotion::Climb`] at t in [0, 1]: turned by θ about the axis and risen
/// along it, θ from `from` to `to`.
fn climb_at(
    axis: &Axis,
    from: f64,
    to: f64,
    rise: &[f64],
    t: f64,
) -> (nalgebra::Rotation3<f64>, V3) {
    let a = from + (to - from) * t;
    let r = nalgebra::Rotation3::from_axis_angle(&axis.dir, a);
    let o = axis.origin.coords;
    (r, o - r * o + axis.dir.into_inner() * climb_rise(rise, a))
}

pub(super) fn rigid_at(steps: &[RigidStep], t: f64) -> (nalgebra::Rotation3<f64>, V3) {
    let (mut r, mut d) = (nalgebra::Rotation3::identity(), V3::zeros());
    for s in steps {
        match *s {
            RigidStep::Turn { axis, from, to } => {
                let q = nalgebra::Rotation3::from_axis_angle(&axis.dir, from + (to - from) * t);
                let c = axis.origin.coords;
                // x ↦ q (x − c) + c after the steps before.
                r = q * r;
                d = q * (d - c) + c;
            }
            RigidStep::Slide { from, to } => d += from + (to - from) * t,
        }
    }
    (r, d)
}

/// How far (in grid steps) the rigid sweep's mesh may move from the planes of the triangles
/// marching tetrahedra made: 0, none (the solid's coplanar triangles merge into faces anyway; a
/// coarser mesh's long slivers leave booleans with it open).
const DECIMATE: f64 = 0.0;

/// The closed mesh, made solid, of where `depth` (positive inside, changing no faster than the
/// distance) is positive, on the grid of `n` points `h` apart from `lo`: the depth found from
/// coarse to fine, marching tetrahedra (see [`meshed`]).
fn contour(
    k: &mut NativeKernel,
    lo: P3,
    n: [usize; 3],
    h: f64,
    depth: &(dyn Fn(&P3) -> f64 + Sync),
    t0: web_time::Instant,
) -> Result<OpResult> {
    use rayon::prelude::*;
    let prof = crate::native::env_flag!("CADRS_PROFILE");
    let at = |i: usize, j: usize, l: usize| lo + V3::new(i as f64 * h, j as f64 * h, l as f64 * h);
    let idx = |i: usize, j: usize, l: usize| (l * n[1] + j) * n[0] + i;
    let ijl = |g: usize| (g % n[0], (g / n[0]) % n[1], g / (n[0] * n[1]));
    // The depth from coarse to fine (steps of 8, 4, 2 and 1): a point whose nearest coarser
    // node lies deeper (or farther out) than their distance and a step more takes its sign
    // from it (the depth changes no faster than the distance), so the depth is found only near
    // the boundary, and every grid edge it crosses has its ends' found.
    let mut field = vec![f64::NAN; n[0] * n[1] * n[2]];
    super::par::start();
    for k in (0..=3).rev() {
        let s = 1usize << k;
        let mut pts: Vec<usize> = Vec::new();
        for l in (0..n[2]).step_by(s) {
            for j in (0..n[1]).step_by(s) {
                for i in (0..n[0]).step_by(s) {
                    let g = idx(i, j, l);
                    if field[g].is_nan() {
                        pts.push(g);
                    }
                }
            }
        }
        let field_ref = &field;
        let vals: Vec<f64> = pts
            .par_iter()
            .map(|&g| {
                let (i, j, l) = ijl(g);
                if k < 3 {
                    let up = |x: usize| (x + s) / (2 * s) * 2 * s;
                    let (ci, cj, cl) = (up(i), up(j), up(l));
                    let v = field_ref[idx(ci, cj, cl)];
                    let gap = |a: usize, c: usize| (a as f64 - c as f64).powi(2);
                    let dist = h * (gap(i, ci) + gap(j, cj) + gap(l, cl)).sqrt();
                    if v.abs() - dist > 1.05 * h {
                        return v.signum() * (v.abs() - dist);
                    }
                }
                depth(&at(i, j, l))
            })
            .collect();
        for (&g, v) in pts.iter().zip(vals) {
            field[g] = v;
        }
    }
    if prof {
        eprintln!(
            "rigid sweep: field of {:?} in {:.0} ms",
            n,
            t0.elapsed().as_secs_f64() * 1e3
        );
    }
    // Marching tetrahedra: each cube cut into six about its main diagonal (so neighbouring
    // cubes cut their shared face alike); a vertex on each grid edge where the depth changes
    // sign, shared by the tetrahedra about it; triangles turned from the inside corners out.
    let tets: [[usize; 4]; 6] = [
        [0, 1, 3, 7],
        [0, 3, 2, 7],
        [0, 2, 6, 7],
        [0, 6, 4, 7],
        [0, 4, 5, 7],
        [0, 5, 1, 7],
    ];
    let corner = |c: usize| (c & 1, (c >> 1) & 1, (c >> 2) & 1);
    let pos = |g: usize| {
        let (i, j, l) = ijl(g);
        at(i, j, l)
    };
    // Each triangle by its corners' grid edges (inside end, outside end).
    let mut tris: Vec<[(usize, usize); 3]> = Vec::new();
    let mut emit = |t: [(usize, usize); 3]| {
        // (Its normal, from the edges' middles, points from inside to outside.)
        let m = t.map(|(a, b)| P3::from((pos(a).coords + pos(b).coords) * 0.5));
        let normal = (m[1] - m[0]).cross(&(m[2] - m[0]));
        let (a, b) = t[0];
        tris.push(if normal.dot(&(pos(b) - pos(a))) >= 0.0 {
            t
        } else {
            [t[0], t[2], t[1]]
        });
    };
    for l in 0..n[2] - 1 {
        for j in 0..n[1] - 1 {
            for i in 0..n[0] - 1 {
                let g: [usize; 8] = std::array::from_fn(|c| {
                    let (a, b2, e) = corner(c);
                    idx(i + a, j + b2, l + e)
                });
                let inside = g.iter().filter(|&&q| field[q] > 0.0).count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                for t in &tets {
                    let (mut ins, mut outs) = ([0usize; 4], [0usize; 4]);
                    let (mut ni, mut no) = (0, 0);
                    for &c in t {
                        if field[g[c]] > 0.0 {
                            ins[ni] = g[c];
                            ni += 1;
                        } else {
                            outs[no] = g[c];
                            no += 1;
                        }
                    }
                    match (ni, no) {
                        (1, 3) => emit([(ins[0], outs[0]), (ins[0], outs[1]), (ins[0], outs[2])]),
                        (3, 1) => emit([(ins[0], outs[0]), (ins[1], outs[0]), (ins[2], outs[0])]),
                        (2, 2) => {
                            let (a, b2, c, d) = (ins[0], ins[1], outs[0], outs[1]);
                            emit([(a, c), (a, d), (b2, d)]);
                            emit([(a, c), (b2, d), (b2, c)]);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    if prof {
        eprintln!(
            "rigid sweep: marched in {:.0} ms",
            t0.elapsed().as_secs_f64() * 1e3
        );
    }
    let key = |(a, b): (usize, usize)| (a.min(b), a.max(b));
    let keys: Vec<(usize, usize)> = {
        let mut ks: Vec<(usize, usize)> = tris.iter().flatten().map(|&e| key(e)).collect();
        ks.sort_unstable();
        ks.dedup();
        ks
    };
    // Each vertex where the depth is 0 along its edge (false position, Illinois: the end kept
    // twice running has its value halved).
    let placed: Vec<P3> = keys
        .par_iter()
        .map(|&(a, b2)| {
            let (pa, pb) = (pos(a), pos(b2));
            let (mut fa, mut fb) = (field[a], field[b2]);
            let (mut xa, mut xb) = (0.0f64, 1.0f64);
            let mut x = fa / (fa - fb);
            let mut side = 0i8;
            for _ in 0..12 {
                let fx = depth(&(pa + (pb - pa) * x));
                if fx.abs() < 1e-6 * h {
                    break;
                }
                if (fx > 0.0) == (fa > 0.0) {
                    xa = x;
                    fa = fx;
                    if side == -1 {
                        fb *= 0.5;
                    }
                    side = -1;
                } else {
                    xb = x;
                    fb = fx;
                    if side == 1 {
                        fa *= 0.5;
                    }
                    side = 1;
                }
                x = xa + (xb - xa) * fa / (fa - fb);
            }
            pa + (pb - pa) * x
        })
        .collect();
    let mesh: Vec<[u32; 3]> = tris
        .par_iter()
        .map(|t| t.map(|e| keys.binary_search(&key(e)).unwrap_or(0) as u32))
        .collect();
    if prof {
        eprintln!(
            "rigid sweep: {} triangles in {:.0} ms",
            mesh.len(),
            t0.elapsed().as_secs_f64() * 1e3
        );
    }
    // Coarser where the grid made more triangles than the surface needs (within `DECIMATE`
    // steps of the planes they replace).
    let (pts, mesh) = if DECIMATE <= 0.0 {
        (placed, mesh)
    } else {
        super::decimate::decimate(&placed, &mesh, DECIMATE * h)
    };
    // (Slivers a corner lying on the opposite side made, cut out: the triangle across that
    // side split at the corner instead.)
    let mut mesh = mesh;
    unsliver(&pts, &mut mesh, 1e-4 * h);
    let out: Vec<[P3; 3]> = mesh.iter().map(|t| t.map(|i| pts[i as usize])).collect();
    if out.is_empty() {
        return Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: "the motion fills nothing".into(),
        });
    }
    if prof {
        eprintln!(
            "rigid sweep: {} triangles kept in {:.0} ms",
            out.len(),
            t0.elapsed().as_secs_f64() * 1e3
        );
    }
    // (Corners within a ten-thousandth of a step welded: a sliver they make would round to
    // nothing in a single-precision mesh of the solid, or tessellate open.)
    let id = k.mesh_solid(&out, 1e-4 * h)?;
    if prof {
        eprintln!(
            "rigid sweep: solid in {:.0} ms",
            t0.elapsed().as_secs_f64() * 1e3
        );
    }
    Ok(OpResult {
        bodies: vec![id],
        history: History::default(),
    })
}

/// [`crate::Kernel::offset_meshed`].
pub fn offset_meshed(k: &mut NativeKernel, body: BodyId, distance: f64) -> Result<OpResult> {
    if !distance.is_finite() {
        return Err(KernelError::InvalidParameter(
            "The offset distance isn't finite".into(),
        ));
    }
    let b = std::sync::Arc::clone(k.get(body)?);
    let md =
        super::mesh_distance::MeshDistance::new(&b, 1e-3 * b.bbox().diagonal().max(tol::POINT))
            .ok_or(KernelError::Unsupported(
                "offsetting a solid whose mesh doesn't close",
            ))?;
    let bb = b.bbox().grow(distance.max(0.0));
    let ext = bb.max - bb.min;
    let h = ext.max() / 100.0;
    let lo = bb.min - V3::repeat(2.0 * h);
    let n = [0, 1, 2].map(|i| 8 * ((ext[i] + 4.0 * h) / h / 8.0).ceil() as usize + 1);
    // Inside the offset where the body's signed distance is below the offset's.
    contour(
        k,
        lo,
        n,
        h,
        &|p: &P3| distance - md.distance(p),
        web_time::Instant::now(),
    )
}

/// The endless motion that cuts `target` as the rigid `motion` cuts it, where its ends keep clear
/// of the target: a turn whose missing part (from its end round to its start) never carries the
/// tool's points at the target's distances from the axis and heights along it into the target's
/// angles about the axis, as a whole turn; a screw (a turn and a slide along its axis, the slide
/// through 0 at a turn of 0) that starts with the tool wholly past the target's one end along the
/// axis and ends wholly past the other, as the screw without end between planes just past the
/// target's ends. Measured on the bodies' meshes, with margins past their deflection.
fn endless_within(target: &Body, tool: &Body, motion: &SolidMotion) -> Option<SolidMotion> {
    let SolidMotion::Rigid { steps } = motion else {
        return None;
    };
    let (mut turn, mut slide) = (None, None);
    for st in steps {
        match *st {
            RigidStep::Turn { axis, from, to } if turn.is_none() => turn = Some((axis, from, to)),
            RigidStep::Slide { from, to } if slide.is_none() => slide = Some((from, to)),
            _ => return None,
        }
    }
    let (axis, a, z_end) = turn?;
    if (z_end - a).abs() <= tol::ANGULAR {
        return None;
    }
    let z = axis.dir.into_inner().normalize();
    let o = axis.origin;
    let size = target
        .bbox()
        .diagonal()
        .max(tool.bbox().diagonal())
        .max(tol::POINT);
    let defl = 1e-3 * size;
    let points = |b: &Body| -> Vec<P3> {
        let (meshes, _) = super::tess::mesh_body(b, defl, 0.3);
        meshes
            .iter()
            .flat_map(|m| m.positions.iter().copied())
            .chain(b.verts.iter().map(|v| v.p))
            .collect()
    };
    let pad = 4.0 * defl + tol::POINT;
    let height = |p: &P3| (p - o).dot(&z);
    let x = any_perpendicular(&z);
    let y = z.cross(&x);
    let polar = |p: &P3| {
        let d = p - o;
        (d.dot(&x).hypot(d.dot(&y)), d.dot(&y).atan2(d.dot(&x)))
    };
    // A body's reach along the axis, within its box's (no farther, so clearance by it is sure).
    let reach = |b: &Body| {
        let bb = b.bbox();
        (0..8)
            .map(|i| {
                height(&P3::new(
                    if i & 1 == 0 { bb.min.x } else { bb.max.x },
                    if i & 2 == 0 { bb.min.y } else { bb.max.y },
                    if i & 4 == 0 { bb.min.z } else { bb.max.z },
                ))
            })
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                (l.min(v), h.max(v))
            })
    };
    match slide {
        Some((s0, s1)) => {
            // A screw: the slide along the axis, through 0 at a turn of 0.
            let d = s1 - s0;
            if d.cross(&z).norm() > tol::ANGULAR * d.norm().max(1.0) || d.norm() <= tol::POINT {
                return None;
            }
            let per = d.dot(&z) / (z_end - a);
            if (s0.dot(&z) - per * a).abs() > tol::POINT || s0.cross(&z).norm() > tol::POINT {
                return None;
            }
            let (h0, h1) = reach(target);
            let (w0, w1) = reach(tool);
            // (Turning leaves heights along the axis as they are: the tool's range only rises.)
            let (ra, rb) = (s0.dot(&z), s1.dot(&z));
            let (lo, hi) = if rb > ra { (ra, rb) } else { (rb, ra) };
            let clear = w1 + lo < h0 - pad && w0 + hi > h1 + pad;
            clear.then_some(SolidMotion::Screw {
                axis,
                rise: per * TAU,
                from: h0 - 2.0 * pad,
                to: h1 + 2.0 * pad,
            })
        }
        None => {
            let tp = points(target);
            let (h0, h1) = tp
                .iter()
                .map(height)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                    (l.min(v), h.max(v))
                });
            // A turn: angles about the axis of the target and of the tool's points at the
            // target's distances and heights (a turn keeps both).
            let (r0, r1) = tp
                .iter()
                .map(|p| polar(p).0)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                    (l.min(v), h.max(v))
                });
            if r0 <= pad {
                return None;
            }
            let wp: Vec<(f64, f64)> = points(tool)
                .iter()
                .filter(|p| {
                    let (r, _) = polar(p);
                    let h = height(p);
                    r >= r0 - pad && r <= r1 + pad && h >= h0 - pad && h <= h1 + pad
                })
                .map(polar)
                .collect();
            if wp.is_empty() {
                return Some(SolidMotion::Turn { axis });
            }
            // An arc of angles holding all of them (the complement of the widest gap between
            // neighbours), widened by what a mesh can miss at the least distance: (start, length).
            let arc = |angles: &mut Vec<f64>, rmin: f64| -> Option<(f64, f64)> {
                angles.sort_by(f64::total_cmp);
                let n = angles.len();
                let (mut gap, mut at) = (angles[0] + TAU - angles[n - 1], 0);
                for k in 1..n {
                    let g = angles[k] - angles[k - 1];
                    if g > gap {
                        gap = g;
                        at = k;
                    }
                }
                let widen = 4.0 * defl / rmin.max(tol::POINT) + 1e-3;
                let len = TAU - gap + 2.0 * widen;
                (len < TAU).then_some((angles[at] - widen, len))
            };
            let rmin = wp.iter().map(|q| q.0).fold(f64::INFINITY, f64::min).min(r0);
            let mut ta: Vec<f64> = tp.iter().map(|p| polar(p).1).collect();
            let mut wa: Vec<f64> = wp.iter().map(|q| q.1).collect();
            let (ts, tl) = arc(&mut ta, r0)?;
            let (ws, wl) = arc(&mut wa, rmin)?;
            // The turn's missing part: from its end on round to its start (right-handed).
            let (lo, hi) = if z_end > a { (a, z_end) } else { (z_end, a) };
            let missing = TAU - (hi - lo);
            if missing <= 0.0 {
                return Some(SolidMotion::Turn { axis });
            }
            // The tool's arc over the missing turns: from ws + hi, ws + wl + hi + missing long.
            let (ms, ml) = (ws + hi, wl + missing);
            if ml + tl >= TAU {
                return None;
            }
            // Disjoint on the circle: the target's arc starts past the swept arc's end and ends
            // before it starts again.
            let off = (ts - ms).rem_euclid(TAU);
            (off > ml && off + tl < TAU).then_some(SolidMotion::Turn { axis })
        }
    }
}

/// Whether a screw's tool may meet its own sweep a turn on (a thread whose groove is wider than
/// its lead): unless the motion is a turn and a slide along its axis, rising farther a turn than
/// the tool's box reaches along the axis.
fn turns_may_overlap(b: &Body, steps: &[RigidStep]) -> bool {
    let (mut turn, mut slide) = (None, None);
    for s in steps {
        match *s {
            RigidStep::Turn { axis, from, to } => turn = Some((axis, to - from)),
            RigidStep::Slide { from, to } => slide = Some(to - from),
        }
    }
    let (Some((axis, angle)), Some(d)) = (turn, slide) else {
        return true;
    };
    let z = axis.dir.into_inner();
    if d.cross(&z).norm() > tol::ANGULAR * d.norm().max(1.0) || angle.abs() <= tol::ANGULAR {
        return true;
    }
    let lead = d.dot(&z).abs() / angle.abs() * TAU;
    let bb = b.bbox();
    let along: Vec<f64> = (0..8)
        .map(|i| {
            let c = V3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            c.dot(&z)
        })
        .collect();
    let reach = along.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - along.iter().copied().fold(f64::INFINITY, f64::min);
    reach >= 0.9 * lead
}

/// The turn's axis, its angle and what it slides by, of steps that are a turn alone or a screw
/// (a turn and a slide along its axis): a motion that carries what it has swept so far along
/// itself.
fn screw_of(steps: &[RigidStep]) -> Option<(crate::Axis, f64, V3)> {
    let (mut turn, mut slide) = (None, V3::zeros());
    for s in steps {
        match *s {
            RigidStep::Turn { axis, from, to } if turn.is_none() => turn = Some((axis, to - from)),
            RigidStep::Turn { .. } => return None,
            RigidStep::Slide { from, to } => slide += to - from,
        }
    }
    let (axis, angle) = turn?;
    (slide.cross(&axis.dir).norm() <= tol::ANGULAR * slide.norm().max(1.0))
        .then_some((axis, angle, slide))
}

/// `target` less the tool `b` swept by `steps` (turning more than half a turn) in pieces of at
/// most half a turn, cut one after another: None where a piece isn't swept or cut soundly.
fn cut_in_pieces(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    steps: &[RigidStep],
) -> Option<OpResult> {
    cut_in_pieces_where(k, target, b, steps, None)
}

/// [`cut_in_pieces`], by the pieces `only` picks alone (by their number, the first 0).
fn cut_in_pieces_where(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    steps: &[RigidStep],
    only: Option<&dyn Fn(usize) -> bool>,
) -> Option<OpResult> {
    let turn = steps
        .iter()
        .map(|s| match *s {
            RigidStep::Turn { from, to, .. } => (to - from).abs(),
            RigidStep::Slide { .. } => 0.0,
        })
        .fold(0.0, f64::max);
    let n = (turn / PI).ceil() as usize;
    let part = |i: usize| -> Vec<RigidStep> {
        let (a, z) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        steps
            .iter()
            .map(|s| match *s {
                RigidStep::Turn { axis, from, to } => RigidStep::Turn {
                    axis,
                    from: from + (to - from) * a,
                    to: from + (to - from) * z,
                },
                RigidStep::Slide { from, to } => RigidStep::Slide {
                    from: from + (to - from) * a,
                    to: from + (to - from) * z,
                },
            })
            .collect()
    };
    // A turn alone, or a screw (its slide along its axis): the motion carries each piece onto
    // the next, so the first is swept and the others are it, moved.
    let onward = screw_of(steps).map(|(axis, angle, rise)| {
        move |i: usize| -> Affine {
            let part = i as f64 / n as f64;
            let q = nalgebra::Rotation3::from_axis_angle(&axis.dir, angle * part);
            let c = axis.origin.coords;
            Affine {
                m: *q.matrix(),
                t: c - q * c + rise * part,
            }
        }
    });
    let mut first: Option<(Option<Body>, Vec<Body>)> = None;
    // (With only some pieces wanted, the first is swept all the same: the others are it.)
    if only.is_some() && onward.is_some() {
        let mut pieces: Vec<Body> = Vec::new();
        let r = super::sweep_faces::by_faces_or_pieces(k, b, &part(0), Some(&mut pieces))
            .ok()
            .map(|r| r.bodies);
        let whole = match r.as_deref() {
            Some([s]) => k.get(*s).ok().map(|b| (**b).clone()),
            _ => None,
        };
        for id in r.into_iter().flatten() {
            k.release(id);
        }
        first = Some((whole, pieces));
    }
    // The cuts one after another, the body checked once they are all made; if it isn't sound
    // then, again with a check after each (a cut that goes wrong is then made piece by piece).
    for checked in [false, true] {
        let mut acc = vec![target];
        let mut made = true;
        for i in 0..n {
            if only.is_some_and(|wanted| !wanted(i)) {
                continue;
            }
            let mut pieces: Vec<Body> = Vec::new();
            let swept: Option<Vec<BodyId>> = match (&first, &onward) {
                (Some((whole, parts)), Some(onward)) => {
                    let by = onward(i);
                    pieces = parts.iter().map(|p| p.transformed(&by)).collect();
                    whole.as_ref().map(|w| vec![k.insert(w.transformed(&by))])
                }
                _ => {
                    let r =
                        super::sweep_faces::by_faces_or_pieces(k, b, &part(i), Some(&mut pieces))
                            .ok()
                            .map(|r| r.bodies);
                    if onward.is_some() {
                        let whole = match r.as_deref() {
                            Some([s]) => k.get(*s).ok().map(|b| (**b).clone()),
                            _ => None,
                        };
                        first = Some((whole, pieces.clone()));
                    }
                    r
                }
            };
            let t_swept = web_time::Instant::now();
            let cut = match swept.as_deref() {
                Some([s]) if acc.len() == 1 => {
                    let cut = k.boolean(crate::BoolOp::Subtract, acc[0], &[*s]);
                    k.release(*s);
                    let t_cut = web_time::Instant::now();
                    let r = cut
                        .ok()
                        .filter(|c| c.bodies.len() == 1 && (!checked || k.sound(c.bodies[0])))
                        .map(|c| c.bodies);
                    if crate::native::env_flag!("CADRS_SWEEP_DEBUG") {
                        eprintln!(
                            "cut in pieces: piece {i}: cut {:.1} ms, checked {:.1} ms",
                            (t_cut - t_swept).as_secs_f64() * 1e3,
                            t_cut.elapsed().as_secs_f64() * 1e3
                        );
                    }
                    r
                }
                _ => None,
            };
            // Else the part's pieces cut one at a time (no union of them needed).
            let cut = cut.or_else(|| {
                if pieces.is_empty() {
                    cut_by_face_sweeps(k, &acc, b, &part(i))
                } else {
                    cut_by_pieces(k, &acc, pieces)
                }
            });
            let Some(c) = cut else {
                release_unless(k, &acc, target);
                if checked {
                    return None;
                }
                made = false;
                break;
            };
            release_unless(k, &acc, target);
            acc = c;
        }
        if !made {
            continue;
        }
        if checked || acc.iter().all(|&id| k.sound(id)) {
            return Some(OpResult {
                bodies: acc,
                history: History::default(),
            });
        }
        release_unless(k, &acc, target);
    }
    None
}

/// `target` less a screw's sweep, a lead at a time. Where the target is a prism along the
/// screw's axis (every face a plane across it, or a plane, a cylinder or an extrusion along it)
/// and the screw starts short of it and ends past it, what the screw cuts is the same in every
/// lead of the target's length: one lead's slab is cut, by the few half turns that reach it,
/// then copied along the axis and the copies glued face to face (a thread of twenty turns is
/// otherwise forty cuts, each from a body the ones before have grown). None where that isn't
/// so, or a step of it fails.
fn cut_by_leads(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    steps: &[RigidStep],
) -> Option<OpResult> {
    use super::surface::Surface;
    let debug = crate::native::env_flag!("CADRS_SWEEP_DEBUG");
    // (`CADRS_SWEEP_LEADS=0`: every half turn cut in turn, as without this, to compare.)
    if std::env::var("CADRS_SWEEP_LEADS").is_ok_and(|v| v == "0") {
        return None;
    }
    let clock = web_time::Instant::now();
    let (axis, angle, rise) = screw_of(steps)?;
    let travel = rise.norm();
    if travel <= tol::POINT || angle.abs() < 2.0 * TAU {
        return None;
    }
    // (Along the axis the way the screw advances.)
    let z = rise / travel;
    let lead = travel * TAU / angle.abs();
    let o = axis.origin;
    let height = |p: &P3| (p - o).dot(&z);
    let t = std::sync::Arc::clone(k.get(target).ok()?);
    let size = t.bbox().diagonal().max(tol::POINT);
    let near = 1e-9 * size;
    if t.solids.len() != 1 || t.verts.is_empty() {
        return None;
    }
    let (t0, t1) = t
        .verts
        .iter()
        .map(|v| height(&v.p))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), h| {
            (a.min(h), z.max(h))
        });
    let level = |h: f64| (h - t0).abs() <= near || (h - t1).abs() <= near;
    if !t.verts.iter().all(|v| level(height(&v.p))) {
        return None;
    }
    let along = |d: &V3| d.cross(&z).norm() <= 1e-9 * d.norm();
    let across = |d: &V3| d.dot(&z).abs() <= 1e-9 * d.norm();
    let prism = t.faces.iter().all(|f| match &f.surf {
        Surface::Plane { f } => {
            let n = f.x.cross(&f.y);
            along(&n) || across(&n)
        }
        Surface::Cylinder { f, .. } => along(&f.z),
        Surface::Extrusion { d, .. } => along(d),
        _ => false,
    });
    if !prism {
        return None;
    }
    // The screw's ends clear of the target: the tool wholly short of it where it starts and
    // past it where it stops (by its box, which holds it at any turn about the axis).
    let (d0, d1) = steps.iter().find_map(|s| match s {
        RigidStep::Slide { from, to } => Some((from.dot(&z), to.dot(&z))),
        RigidStep::Turn { .. } => None,
    })?;
    let bb = b.bbox();
    let (hb0, hb1) = (0..8)
        .map(|i| {
            height(&P3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            ))
        })
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), h| {
            (a.min(h), z.max(h))
        });
    if !(hb1 + d0 < t0 - near && hb0 + d1 > t1 + near) {
        return None;
    }
    let whole = ((t1 - t0) / lead + 1e-9).floor() as usize;
    if whole < 2 {
        return None;
    }
    let rest = (t1 - t0) - whole as f64 * lead;
    // Everything past a height along the axis, as a block to cut away.
    let reach = t.verts.iter().map(|v| (v.p - o).norm()).fold(0.0, f64::max) + size;
    let past = |k: &mut NativeKernel, h: f64, depth: f64| -> Option<BodyId> {
        let other = if z.x.abs() < 0.9 { V3::x() } else { V3::y() };
        let x = (other - z * other.dot(&z)).normalize();
        let plane = crate::Plane {
            origin: o + z * h,
            x_dir: nalgebra::Unit::new_normalize(x),
            normal: nalgebra::Unit::new_normalize(z),
        };
        let corners = [
            (-reach, -reach),
            (reach, -reach),
            (reach, reach),
            (-reach, reach),
        ];
        let outer = crate::Loop {
            curves: (0..4)
                .map(|i| {
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    crate::Curve2::Line {
                        a: nalgebra::Point2::new(a.0, a.1),
                        b: nalgebra::Point2::new(b.0, b.1),
                        source: None,
                    }
                })
                .collect(),
        };
        let region = crate::Region {
            outer,
            holes: Vec::new(),
            source: None,
        };
        let spec = crate::ExtrudeSpec::blind(nalgebra::Unit::new_normalize(z), depth);
        k.extrude_with(&crate::Profile::new(plane, vec![region]), &spec)
            .ok()?
            .bodies
            .first()
            .copied()
    };
    let less = |k: &mut NativeKernel, body: BodyId, h: f64, depth: f64| -> Option<Body> {
        let block = past(k, h, depth)?;
        let cut = k.boolean(crate::BoolOp::Subtract, body, &[block]);
        k.release(block);
        let cut = cut.ok()?;
        let one = match cut.bodies.as_slice() {
            [one] => k.get(*one).ok().map(|b| (**b).clone()),
            _ => None,
        };
        for id in cut.bodies {
            k.release(id);
        }
        one
    };
    // One lead's slab of the target, and the half turns that reach it.
    let slab = less(k, target, t0 + lead, (t1 - t0 - lead) + size)?;
    let slab = k.insert(slab);
    let n = (angle.abs() / PI).ceil() as usize;
    let reaches = |i: usize| {
        let (a, e) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        let (lo, hi) = (hb0 + d0 + (d1 - d0) * a, hb1 + d0 + (d1 - d0) * e);
        hi > t0 - near && lo < t0 + lead + near
    };
    let cut = cut_in_pieces_where(k, slab, b, steps, Some(&reaches));
    let unit = match cut.as_ref().map(|c| c.bodies.as_slice()) {
        Some([one]) => k.get(*one).ok().map(|b| (**b).clone()),
        _ => None,
    };
    for id in cut.into_iter().flat_map(|c| c.bodies) {
        if id != slab {
            k.release(id);
        }
    }
    k.release(slab);
    let unit = unit?;
    if debug {
        eprintln!(
            "cut by leads: one lead of {whole} (and {rest:.3e} over) cut by {} half turns in {:.0} ms: {} faces",
            (0..n).filter(|&i| reaches(i)).count(),
            clock.elapsed().as_secs_f64() * 1e3,
            unit.faces.len()
        );
    }
    let moved = |j: usize| {
        unit.transformed(&Affine {
            m: nalgebra::Matrix3::identity(),
            t: z * (lead * j as f64),
        })
    };
    let mut all: Vec<Body> = (0..whole).map(moved).collect();
    if rest > 1e-7 * size {
        let copy = k.insert(moved(whole));
        let last = less(k, copy, t1, lead + size);
        k.release(copy);
        all.push(last?);
    }
    let r = super::sweep_faces::glued_with(k, &all, size, true);
    if debug {
        eprintln!(
            "cut by leads: {} slabs glued by {:.0} ms: {:?}",
            all.len(),
            clock.elapsed().as_secs_f64() * 1e3,
            r.as_ref().map(|r| r.bodies.len())
        );
    }
    r
}

/// Releases `ids` but `keep`.
fn release_unless(k: &mut NativeKernel, ids: &[BodyId], keep: BodyId) {
    for &id in ids {
        if id != keep {
            k.release(id);
        }
    }
}

/// The bodies `targets` less the sweep of `b` by one slide, turn or screw: less the solid where
/// it starts and each of its leading faces' sweeps in turn ([`super::sweep_faces::face_sweeps`]:
/// their union is the sweep, but where they overlap that union is a harder boolean than these
/// cuts). None when a cut fails or leaves a body that isn't sound.
fn cut_by_face_sweeps(
    k: &mut NativeKernel,
    targets: &[BodyId],
    b: &Body,
    steps: &[RigidStep],
) -> Option<Vec<BodyId>> {
    let pieces = super::sweep_faces::face_sweeps(k, b, steps).ok()?;
    cut_by_pieces(k, targets, pieces)
}

/// The bodies `targets` less each of `pieces` in turn; None when a cut fails or leaves a body
/// that isn't sound.
fn cut_by_pieces(
    k: &mut NativeKernel,
    targets: &[BodyId],
    pieces: Vec<Body>,
) -> Option<Vec<BodyId>> {
    // (Pieces that only touch glued first: one cut each group, none along a tangency.)
    let pieces = super::sweep_faces::glued_groups(k, pieces);
    if crate::native::env_flag!("CADRS_SWEEP_DEBUG") {
        eprintln!("swept cut: {} groups of pieces", pieces.len());
    }
    let keep: Vec<BodyId> = targets.to_vec();
    let mut acc: Vec<BodyId> = targets.to_vec();
    for piece in pieces {
        let pb = piece.bbox();
        let tool = k.insert(piece);
        let mut next = Vec::new();
        let mut ok = true;
        for &a in &acc {
            let overlaps = k.get(a).is_ok_and(|t| {
                let tb = t.bbox();
                tb.min.x <= pb.max.x
                    && pb.min.x <= tb.max.x
                    && tb.min.y <= pb.max.y
                    && pb.min.y <= tb.max.y
                    && tb.min.z <= pb.max.z
                    && pb.min.z <= tb.max.z
            });
            if !overlaps {
                next.push(a);
                continue;
            }
            match k.boolean(crate::BoolOp::Subtract, a, &[tool]) {
                Ok(c) if c.bodies.iter().all(|&id| k.sound(id)) => {
                    next.extend(c.bodies);
                }
                r => {
                    if crate::native::env_flag!("CADRS_SWEEP_DEBUG") {
                        match &r {
                            Ok(c) => eprintln!(
                                "swept cut: a piece's cut isn't sound: {:?}",
                                c.bodies
                                    .iter()
                                    .filter_map(|&id| k.get(id).ok()?.check().into_iter().next())
                                    .next()
                            ),
                            Err(e) => eprintln!("swept cut: a piece's cut failed: {e}"),
                        }
                    }
                    ok = false;
                    break;
                }
            }
        }
        k.release(tool);
        for &a in &acc {
            if !next.contains(&a) && !keep.contains(&a) {
                k.release(a);
            }
        }
        if !ok {
            for &a in &next {
                if !keep.contains(&a) {
                    k.release(a);
                }
            }
            return None;
        }
        acc = next;
    }
    Some(acc)
}

/// [`crate::Kernel::cut_swept`].
/// [`cut_swept`] at each of `copies` places of the tool a whole turn apart about the target's
/// axis (the second turn's): a gear's every tooth space, cut by a motion that doesn't repeat
/// (a face mill's roll). Generated exactly, one plug made and its copies subtracted together,
/// or an error at once where its contacts can't be followed.
pub fn cut_swept_indexed(
    k: &mut NativeKernel,
    target: BodyId,
    tool: BodyId,
    motion: &SolidMotion,
    copies: usize,
) -> Result<OpResult> {
    let SolidMotion::Rigid { steps } = motion else {
        return Err(KernelError::Unsupported(
            "an indexed cut by a motion of two turns",
        ));
    };
    let Some(RigidStep::Turn { axis, .. }) = steps.get(1).copied() else {
        return Err(KernelError::Unsupported(
            "an indexed cut by a motion of two turns",
        ));
    };
    let b = std::sync::Arc::clone(k.get(tool)?);
    let _ = axis;
    match super::sweep_generate::cut(k, target, &b, steps, (copies > 1).then_some(copies)) {
        Some(r) => r,
        // (Not cut a tooth at a time by the meshed sweep: seconds a tooth, its faces a mesh's,
        // and it may not close. The caller is told at once.)
        None => Err(KernelError::Unsupported(
            "a generated cut the tool's contacts can't be followed for exactly",
        )),
    }
}

/// `target` less what any tool fills carried upright along a path whose every move is clear
/// above the target or straight along `up` (a drill's program: down a hole in pecks, up, over
/// to the next): the moves on one line that follow one another fill what one slide does, the
/// tool drawn from the lowest of them to the highest, and each such slide is cut on its own.
/// None for a path with another move that reaches the target.
fn cut_plunges(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    path: &[crate::PathCurve],
    up: V3,
) -> Option<OpResult> {
    let up = up.try_normalize(tol::TINY)?;
    let segs = super::sweep::path::path_segments(k, path).ok()?;
    let p0 = segs.first()?.start();
    let near = tol::REL_POINT * b.bbox().diagonal().max(tol::POINT);
    let height = |p: &P3| p.coords.dot(&up);
    let corners = |bb: &Bbox| {
        let bb = *bb;
        (0..8).map(move |i| {
            let pick = |k: usize| {
                if i >> k & 1 == 0 {
                    bb.min[k]
                } else {
                    bb.max[k]
                }
            };
            P3::new(pick(0), pick(1), pick(2))
        })
    };
    // (The tool's lowest point below its place on the path, and the target's top.)
    let bottom = corners(&b.bbox())
        .map(|c| (c - p0).dot(&up))
        .fold(f64::INFINITY, f64::min);
    let over = corners(&k.get(target).ok()?.bbox())
        .map(|c| height(&c))
        .fold(f64::NEG_INFINITY, f64::max);
    // Each hole: where the tool stands lowest in it, and how far up it is drawn from there.
    let mut holes: Vec<(P3, f64)> = Vec::new();
    let mut open = false;
    for s in &segs {
        let (a, z) = (s.start(), s.end());
        let (ha, hz) = (height(&a), height(&z));
        let lowest = (0..=8)
            .map(|i| height(&s.c.point(s.t0 + (s.t1 - s.t0) * i as f64 / 8.0)))
            .fold(ha.min(hz), f64::min);
        if lowest + bottom >= over - near {
            open = false;
            continue;
        }
        let d = z - a;
        let straight = (0..=4).all(|i| {
            let f = i as f64 / 4.0;
            let t = if s.rev {
                s.t1 + (s.t0 - s.t1) * f
            } else {
                s.t0 + (s.t1 - s.t0) * f
            };
            (s.c.point(t) - (a + d * f)).norm() <= near
        });
        if !straight || (d - up * d.dot(&up)).norm() > near {
            return None;
        }
        let (low, high) = if ha <= hz { (a, hz) } else { (z, ha) };
        match holes.last_mut().filter(|_| open) {
            Some(h) => {
                let top = (height(&h.0) + h.1).max(high);
                if height(&low) < height(&h.0) {
                    h.0 = low;
                }
                h.1 = top - height(&h.0);
            }
            None => holes.push((low, high - height(&low))),
        }
        open = true;
    }
    if holes.is_empty() {
        return None;
    }
    let mut at = target;
    let mut cut: Option<OpResult> = None;
    for (low, rise) in holes {
        let tool = k.insert(b.transformed(&Affine::translation(low - p0)));
        let step = cut_swept(k, at, tool, &SolidMotion::slide(up * rise));
        k.release(tool);
        if at != target {
            k.release(at);
        }
        match step {
            Ok(r) if r.bodies.len() == 1 => {
                at = r.bodies[0];
                cut = Some(r);
            }
            Ok(r) => {
                for id in r.bodies {
                    k.release(id);
                }
                return None;
            }
            Err(_) => return None,
        }
    }
    cut
}

/// `target` less what a flat end mill fills carried upright through a program: a path of
/// level moves (lines and arcs at one height), moves straight along the mill's axis (plunges
/// and rises) and moves clear above the target. At each depth the mill reaches, everything
/// within its radius of a level run there ([`super::offset::prism::wire_region`], exact in
/// lines and arcs) and the disc under each plunge are one flat figure, united in the plane;
/// that figure extruded from the depth to above the target is all the mill fills there
/// within the target, and is cut from it in one boolean. (A plunge or a rise fills only the
/// disc at its foot and a column above the mill's own top; a move's sweep cut on its own cuts
/// again most of what the move before it cut.) None for another tool or path, where the mill's
/// top would come below the target's (the column would then count), or the cuts aren't sound.
fn cut_end_mill_program(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    path: &[crate::PathCurve],
    up: V3,
) -> Option<OpResult> {
    use crate::{Curve2, Loop, Region};
    let debug = crate::native::env_flag!("CADRS_SWEEP_DEBUG");
    let up = up.try_normalize(tol::TINY)?;
    let segs = super::sweep::path::path_segments(k, path).ok()?;
    let p0 = segs.first()?.start();
    // The mill: a cylinder about the line through the path's start along `up`, between two
    // flat ends.
    if b.faces.len() != 3 || b.solids.len() != 1 {
        return None;
    }
    let size = b.bbox().diagonal().max(tol::POINT);
    let near = tol::REL_POINT * size;
    let along = |v: &V3| v.normalize().cross(&up).norm() < 1e-9;
    let (mut ends, mut radius) = (Vec::new(), None);
    for f in &b.faces {
        match &f.surf {
            Surface::Plane { f } if along(&f.x.cross(&f.y)) => ends.push((f.o - p0).dot(&up)),
            Surface::Cylinder { f, r } if along(&f.z) => {
                let d = f.o - p0;
                if (d - up * d.dot(&up)).norm() > near {
                    return None;
                }
                radius = Some(*r);
            }
            _ => return None,
        }
    }
    let (radius, [e0, e1]) = (radius?, <[f64; 2]>::try_from(ends).ok()?);
    let (bottom, top) = (e0.min(e1), e0.max(e1));
    let height = |p: &P3| p.coords.dot(&up);
    let bb = k.get(target).ok()?.bbox();
    let over = (0..8)
        .map(|i| {
            let pick = |k: usize| {
                if i >> k & 1 == 0 {
                    bb.min[k]
                } else {
                    bb.max[k]
                }
            };
            height(&P3::new(pick(0), pick(1), pick(2)))
        })
        .fold(f64::NEG_INFINITY, f64::max);
    // The plane's frame: x any way square to `up`.
    let x = any_perpendicular(&up);
    let y = up.cross(&x);
    let rows = nalgebra::Matrix3::from_rows(&[x.transpose(), y.transpose(), up.transpose()]);
    let flat = |p: &P3| nalgebra::Point2::new(p.coords.dot(&x), p.coords.dot(&y));
    // Each depth's regions, by the height of the mill's flat end there.
    let mut depths: Vec<(f64, Vec<Region>)> = Vec::new();
    let mut add = |h: f64, r: Region| match depths.iter_mut().find(|d| (d.0 - h).abs() <= near) {
        Some(d) => d.1.push(r),
        None => depths.push((h, vec![r])),
    };
    let disc = |c: nalgebra::Point2<f64>, r: f64| Loop {
        curves: vec![Curve2::circle(c, r)],
    };
    // The level run gathered so far: its moves in the plane's frame, at one height.
    let mut run: Vec<(Curve, f64, f64)> = Vec::new();
    let mut run_at = 0.0;
    let mut reaches = false;
    let flush = |run: &mut Vec<(Curve, f64, f64)>,
                 h: f64,
                 add: &mut dyn FnMut(f64, Region)|
     -> Option<()> {
        if run.is_empty() {
            return Some(());
        }
        let wire = std::mem::take(run);
        add(h, super::offset::prism::wire_region(&wire, radius).ok()?);
        Some(())
    };
    for s in &segs {
        let (a, z) = (s.start(), s.end());
        // (A straight move given as a spline of two poles is a line from end to end.)
        let straight = !matches!(s.c, Curve::Circle { .. })
            && (z - a).norm() > near
            && (0..=4).all(|i| {
                let f = i as f64 / 4.0;
                let t = if s.rev {
                    s.t1 + (s.t0 - s.t1) * f
                } else {
                    s.t0 + (s.t1 - s.t0) * f
                };
                (s.c.point(t) - (a + (z - a) * f)).norm() <= near
            });
        let line;
        let s = if straight {
            let (c, l) = Curve::line(a, z);
            line = super::sweep::path::Seg {
                c,
                t0: 0.0,
                t1: l,
                rev: false,
            };
            &line
        } else {
            s
        };
        let (ha, hz) = (height(&a), height(&z));
        let level = (0..=8).all(|i| {
            let t = s.t0 + (s.t1 - s.t0) * i as f64 / 8.0;
            (height(&s.c.point(t)) - ha).abs() <= near
        });
        // (The mill's flat end above the target all along: nothing cut.)
        let clear = ha.min(hz) + bottom >= over - near;
        if !clear && ha.min(hz) + top < over - near {
            // The mill's top under the target's: its plunges' columns would cut too.
            return None;
        }
        if level && !clear {
            let foot = ha + bottom;
            if !run.is_empty() && (run_at - foot).abs() > near {
                flush(&mut run, run_at, &mut add)?;
            }
            run_at = foot;
            reaches = true;
            // A whole circle: the ring within the radius of it (its hole where the circle is
            // wider than the mill).
            if let Curve::Circle { c, r, .. } = &s.c
                && ((s.t1 - s.t0).abs() - TAU).abs() <= tol::ANGULAR
            {
                flush(&mut run, run_at, &mut add)?;
                let centre = flat(c);
                add(
                    foot,
                    Region {
                        outer: disc(centre, r + radius),
                        holes: if *r > radius + near {
                            vec![disc(centre, r - radius)]
                        } else {
                            vec![]
                        },
                        source: None,
                    },
                );
                continue;
            }
            if !matches!(s.c, Curve::Line { .. } | Curve::Circle { .. }) {
                return None;
            }
            // The move in the plane's frame at its own height (z = 0 there).
            let to_flat = Affine {
                m: rows,
                t: -(rows * (up * ha)),
            };
            let c = s.c.transformed(&to_flat);
            run.push(if s.rev {
                let (c, t0, t1) = c.reversed(s.t0, s.t1);
                (c, t0, t1)
            } else {
                (c, s.t0, s.t1)
            });
            continue;
        }
        flush(&mut run, run_at, &mut add)?;
        if clear {
            continue;
        }
        // A move along the axis: the disc at its foot. Any other isn't this program's.
        let d = z - a;
        if !matches!(s.c, Curve::Line { .. }) || (d - up * d.dot(&up)).norm() > near {
            return None;
        }
        reaches = true;
        add(
            ha.min(hz) + bottom,
            Region {
                outer: disc(flat(&a), radius),
                holes: vec![],
                source: None,
            },
        );
    }
    flush(&mut run, run_at, &mut add)?;
    if !reaches {
        return None;
    }
    // Each depth's figure extruded to a little above the target, cut from it: deepest first,
    // and a depth whose figure lies within the deeper ones' not at all (what it would cut is
    // cut already: a pocket stepped down over one outline is its last step's cut).
    depths.sort_by(|p, q| p.0.total_cmp(&q.0));
    let above = over + 0.1 * radius;
    let mut at = target;
    let mut cut: Option<OpResult> = None;
    let mut deeper: Vec<Region> = Vec::new();
    for (h, regions) in depths {
        let n = regions.len();
        let united = super::flat_union::union(&regions, None).and_then(|(own, _)| {
            let m = deeper.len();
            let mut all = std::mem::take(&mut deeper);
            all.extend(own.iter().cloned());
            let (both, adds) = super::flat_union::union(&all, Some(m))?;
            deeper = both;
            Ok((own, adds))
        });
        let regions = match united {
            Ok((_, false)) => {
                if debug {
                    eprintln!("end mill program: depth {h}: {n} regions within the deeper ones");
                }
                continue;
            }
            Ok((own, true)) => own,
            Err(e) => {
                if debug {
                    eprintln!("end mill program: depth {h}: its figure: {e}");
                }
                return None;
            }
        };
        let prisms =
            match super::offset::prism::regions_prisms(regions, P3::origin(), x, up, h, above) {
                Ok(p) if !p.is_empty() => p,
                Ok(_) => continue,
                Err(e) => {
                    if debug {
                        eprintln!("end mill program: depth {h}: {e}");
                    }
                    return None;
                }
            };
        if debug {
            eprintln!(
                "end mill program: depth {h}: {n} regions in {} solid(s)",
                prisms.len()
            );
        }
        let tools: Vec<BodyId> = prisms.into_iter().map(|p| k.insert(p)).collect();
        let step = k.boolean(crate::BoolOp::Subtract, at, &tools);
        for t in tools {
            k.release(t);
        }
        if at != target {
            k.release(at);
        }
        match step {
            Ok(r) if r.bodies.len() == 1 => {
                at = r.bodies[0];
                cut = Some(r);
            }
            Ok(r) => {
                for id in r.bodies {
                    k.release(id);
                }
                return None;
            }
            Err(e) => {
                if debug {
                    eprintln!("end mill program: depth {h}: its cut: {e}");
                }
                return None;
            }
        }
    }
    let r = cut?;
    r.bodies.iter().all(|&id| k.sound(id)).then_some(r)
}

/// `target` less what a ball-end mill fills carried upright along a pass of straight moves
/// ([`super::sweep_ball_pass`]): the pass's solid up to a little above the target (and above
/// the ball's highest centre), where that is below the lowest the mill's top comes. None for
/// another tool or path, or where the cut isn't sound.
fn cut_ball_pass(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    path: &[crate::PathCurve],
    up: V3,
) -> Option<OpResult> {
    let up = up.try_normalize(tol::TINY)?;
    let segs = super::sweep::path::path_segments(k, path).ok()?;
    let p0 = segs.first()?.start();
    let (centre, r, length) = super::sweep_ball_pass::ball_mill(b, &p0, &up)?;
    let near = tol::REL_POINT * b.bbox().diagonal().max(tol::POINT);
    let mut centres = vec![p0 + up * centre];
    for s in &segs {
        let (a, z) = (s.start(), s.end());
        let mid = s.c.point(0.5 * (s.t0 + s.t1));
        if (mid - (a + (z - a) * 0.5)).norm() > near
            || (a + up * centre - centres[centres.len() - 1]).norm() > near
        {
            return None;
        }
        centres.push(z + up * centre);
    }
    let height = |p: &P3| p.coords.dot(&up);
    let bb = k.get(target).ok()?.bbox();
    let over = (0..8)
        .map(|i| {
            let pick = |k: usize| {
                if i >> k & 1 == 0 {
                    bb.min[k]
                } else {
                    bb.max[k]
                }
            };
            height(&P3::new(pick(0), pick(1), pick(2)))
        })
        .fold(f64::NEG_INFINITY, f64::max);
    let (low, high) = centres
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |m, c| {
            (m.0.min(height(c)), m.1.max(height(c)))
        });
    let top = over.max(high) + 0.1 * r;
    if top > low + length {
        return None;
    }
    let pass = super::sweep_ball_pass::ball_pass(&centres, r, &up, top)?;
    let id = k.insert(pass);
    let cut = k.boolean(crate::BoolOp::Subtract, target, &[id]);
    k.release(id);
    match cut {
        Ok(c) if c.bodies.iter().all(|&id| k.sound(id)) => Some(c),
        Ok(_) => None,
        Err(e) => {
            if crate::native::env_flag!("CADRS_SWEEP_DEBUG") {
                eprintln!("ball pass: its cut: {e}");
            }
            None
        }
    }
}

pub fn cut_swept(
    k: &mut NativeKernel,
    target: BodyId,
    tool: BodyId,
    motion: &SolidMotion,
) -> Result<OpResult> {
    let b = std::sync::Arc::clone(k.get(tool)?);
    // A ball-end mill carried upright along straight moves in one upright plane (a raster
    // pass): what it fills below the target's top, made face by face, cut from the target.
    if let SolidMotion::Path { path, up } = motion
        && let Some(r) = cut_ball_pass(k, target, &b, path, up.into_inner())
    {
        return Ok(r);
    }
    // A flat end mill carried upright through a program of level moves and plunges: what it
    // fills at each depth, united in the plane and extruded, cut once a depth.
    if let SolidMotion::Path { path, up } = motion
        && let Some(r) = cut_end_mill_program(k, target, &b, path, up.into_inner())
    {
        return Ok(r);
    }
    // A motion whose ends keep clear of the target: within it the endless motion (a whole turn,
    // a screw without end) fills what this one does, and is made exactly as a whole (the solid
    // of revolution, the screw's helical sweep).
    // Any tool carried through moves along its own line alone where it reaches the target (a
    // drill's pecks): each hole's moves are one slide, from its lowest to its highest.
    if let SolidMotion::Path { path, up } = motion
        && let Some(r) = cut_plunges(k, target, &b, path, up.into_inner())
    {
        return Ok(r);
    }
    if let Some(endless) = endless_within(&**k.get(target)?, &b, motion) {
        // (Its outline exact, then traced finely, then coarsely: a boolean may take the one and
        // not the other.)
        for n in OUTLINES {
            let swept = with_slices(n, || sweep_solid_at(k, tool, &endless));
            let debug = crate::native::env_flag!("CADRS_SWEEP_SOLID_DEBUG");
            if debug && let Err(e) = &swept {
                eprintln!("endless sweep on {n} slices: {e}");
            }
            if let Ok(r) = swept
                && let [s] = r.bodies.as_slice()
            {
                let cut = k.boolean(crate::BoolOp::Subtract, target, &[*s]);
                k.release(*s);
                if debug {
                    match &cut {
                        Err(e) => eprintln!("endless sweep on {n} slices: its cut: {e}"),
                        Ok(c) => eprintln!(
                            "endless sweep on {n} slices: cut, sound {:?}",
                            c.bodies.iter().map(|&id| k.sound(id)).collect::<Vec<_>>()
                        ),
                    }
                }
                if let Ok(c) = cut
                    && c.bodies.iter().all(|&id| k.sound(id))
                {
                    return Ok(c);
                }
            }
        }
    }
    // Two turns about different lines (a gear cut by its mate): by the contacts' sheets.
    if let SolidMotion::Rigid { steps } = motion
        && let Some(Ok(r)) = super::sweep_generate::cut(k, target, &b, steps, None)
    {
        return Ok(r);
    }
    // A rigid motion turning more than half a turn: exactly, in pieces of at most half a turn
    // cut one after another (a tool's sweep over a whole turn can overlap itself, a thread whose
    // groove is wider than its lead; the pieces don't), and whole. The pieces first where the
    // tool's turns may overlap; else the whole first (one sweep and one cut), the pieces after.
    let long = match motion {
        SolidMotion::Rigid { steps } if (1..=2).contains(&steps.len()) => {
            let turn = steps
                .iter()
                .map(|s| match *s {
                    RigidStep::Turn { from, to, .. } => (to - from).abs(),
                    RigidStep::Slide { .. } => 0.0,
                })
                .fold(0.0, f64::max);
            (turn > PI + tol::ANGULAR).then_some(steps.as_slice())
        }
        _ => None,
    };
    let overlapping = long.is_some_and(|steps| turns_may_overlap(&b, steps));
    if let Some(steps) = long
        && overlapping
        && let Some(r) = cut_by_leads(k, target, &b, steps)
    {
        return Ok(r);
    }
    if let Some(steps) = long
        && overlapping
        && let Some(r) = cut_in_pieces(k, target, &b, steps)
    {
        return Ok(r);
    }
    // Exactly: the sweep, where it is made without meshing, cut from the target.
    // A climb (a screw whose lead changes): half a turn at a time.
    if let SolidMotion::Climb {
        axis,
        from,
        to,
        rise,
    } = motion
    {
        let (r, d) = climb_at(axis, *from, *to, rise, 0.0);
        let start = b.transformed(&Affine {
            m: *r.matrix(),
            t: d,
        });
        let (coef, a0) = (rise.clone(), *from);
        let base = climb_rise(&coef, a0);
        let rel: super::sweep_faces::Rise =
            std::sync::Arc::new(move |s: f64| climb_rise(&coef, a0 + s) - base);
        if let Some(r) = cut_rising_screw(
            k,
            target,
            &start,
            axis.origin,
            axis.dir.into_inner(),
            to - from,
            rel,
        ) {
            return Ok(r);
        }
    }
    // Carried along a helix (about a fixed axis along `up`, its lead changing or not): a screw.
    if let SolidMotion::Path { path, up } = motion
        && let Ok(frames) = PathFrames::new(k, path, up.into_inner())
        && let Some((o, angle, rise)) = path_screw(&frames, b.bbox().diagonal().max(frames.total))
        && let Some(r) = cut_rising_screw(k, target, &b, o, frames.up, angle, rise)
    {
        return Ok(r);
    }
    // A torus slid: by its envelope first (its leading faces' sweeps cross one another where
    // its outline folds).
    let torus_first = matches!(motion, SolidMotion::Rigid { steps } if slid_torus(&b, steps));
    if let SolidMotion::Rigid { steps } = motion
        && torus_first
        && let Some(r) = cut_torus_whole(k, target, &b, steps)
    {
        return Ok(r);
    }
    let mut pieces: Vec<Body> = Vec::new();
    let exact = match motion {
        SolidMotion::Rigid { steps } if (1..=2).contains(&steps.len()) => {
            super::sweep_faces::by_faces_or_pieces(k, &b, steps, Some(&mut pieces)).ok()
        }
        SolidMotion::Planar { .. } | SolidMotion::Turn { .. } | SolidMotion::Screw { .. } => {
            sweep_solid(k, tool, motion).ok()
        }
        SolidMotion::Path { path, up } => path_sweep_exact(k, tool, &b, path, up.into_inner()),
        _ => None,
    };
    if let Some(r) = exact
        && let [s] = r.bodies.as_slice()
    {
        let cut = k.boolean(crate::BoolOp::Subtract, target, &[*s]);
        k.release(*s);
        if let Ok(c) = cut
            && c.bodies.iter().all(|&id| k.sound(id))
        {
            return Ok(c);
        }
    }
    // Else, for a motion of at most half a turn, the sweep's pieces cut one at a time.
    if long.is_none()
        && let SolidMotion::Rigid { steps } = motion
        && (1..=2).contains(&steps.len())
        && let Some(bodies) = if pieces.is_empty() {
            cut_by_face_sweeps(k, &[target], &b, steps)
        } else {
            cut_by_pieces(k, &[target], std::mem::take(&mut pieces))
        }
    {
        return Ok(OpResult {
            bodies,
            history: History::default(),
        });
    }
    if let Some(steps) = long
        && !overlapping
        && let Some(r) = cut_in_pieces(k, target, &b, steps)
    {
        return Ok(r);
    }
    // A torus: its sweep trimmed whole by what it covers of itself (where its leading faces
    // fold, their sweeps can't be cut one at a time), cut from the target.
    if let SolidMotion::Rigid { steps } = motion
        && !torus_first
        && let Some(r) = cut_torus_whole(k, target, &b, steps)
    {
        return Ok(r);
    }
    // Else meshed: inside the target, in none of the tool's places.
    let t = std::sync::Arc::clone(k.get(target)?);
    let tdist =
        super::mesh_distance::MeshDistance::new(&t, 1e-3 * t.bbox().diagonal().max(tol::POINT))
            .ok_or(KernelError::Unsupported(
                "cutting from a target whose mesh doesn't close",
            ))?;
    let td = |p: &P3| -tdist.distance(p);
    let within = Some((t.bbox(), &td as &(dyn Fn(&P3) -> f64 + Sync)));
    let rev = revolved(&b);
    let md;
    let dist: &(dyn Fn(&P3) -> f64 + Sync) = match &rev {
        Some(r) => &|p: &P3| r.distance(p),
        None => {
            md = super::mesh_distance::MeshDistance::new(
                &b,
                1e-3 * b.bbox().diagonal().max(tol::POINT),
            )
            .ok_or(KernelError::Unsupported(
                "sweeping a solid whose mesh doesn't close",
            ))?;
            &|p: &P3| md.distance(p)
        }
    };
    match motion {
        SolidMotion::Rigid { steps } => meshed_within(k, &b, dist, &|t| rigid_at(steps, t), within),
        SolidMotion::Path { path, up } => {
            let frames = PathFrames::new(k, path, up.into_inner())?;
            meshed_within(k, &b, dist, &|t| frames.place(t), within)
        }
        SolidMotion::Climb {
            axis,
            from,
            to,
            rise,
        } => meshed_within(
            k,
            &b,
            dist,
            &|t| climb_at(axis, *from, *to, rise, t),
            within,
        ),
        _ => Err(KernelError::Unsupported(
            "cutting by a swept solid meshed for a motion other than a rigid one or a path",
        )),
    }
}

/// An end mill standing upright (a cylinder about the line through the path's start along `up`,
/// between two flat ends square to it, the bottom edge sharp or rounded: a flat or a bull-nose
/// cutter) carried along a path in a plane square to `up`: what it passes through is everything
/// within its radius of the path, between its ends ([`super::offset::prism::wire_slot`]: the
/// path's offset, exact and trimmed where the path bends tighter than the radius or loops),
/// its bottom edge rounded as the cutter's is. None for another body or path.
fn end_mill_along(
    k: &mut NativeKernel,
    b: &Body,
    path: &[crate::PathCurve],
    up: V3,
) -> Option<OpResult> {
    let debug = crate::native::env_flag!("CADRS_SWEEP_DEBUG");
    let up = up.try_normalize(tol::TINY)?;
    let segs = super::sweep::path::path_segments(k, path).ok()?;
    let p0 = segs.first()?.start();
    let size = b.bbox().diagonal().max(crate::native::tol::POINT);
    let near = tol::REL_POINT * size.max((segs.last()?.end() - p0).norm());
    for s in &segs {
        for i in 0..=16 {
            let t = s.t0 + (s.t1 - s.t0) * i as f64 / 16.0;
            if (s.c.point(t) - p0).dot(&up).abs() > near {
                return None;
            }
        }
    }
    let height = |p: &P3| (p - p0).dot(&up);
    let off_axis = |p: &P3| {
        let d = p - p0;
        (d - up * d.dot(&up)).norm()
    };
    let parallel = |v: &V3| v.normalize().cross(&up).norm() < 1e-9;
    // Its faces: two flat ends, the cylinder, and at most the corner's torus.
    let (mut flats, mut radius, mut corner) = (Vec::new(), None, None);
    for f in &b.faces {
        match &f.surf {
            Surface::Plane { f } if parallel(&f.x.cross(&f.y)) => flats.push(height(&f.o)),
            Surface::Cylinder { f, r } if parallel(&f.z) && off_axis(&f.o) <= near => {
                if radius.replace(*r).is_some() {
                    return None;
                }
            }
            Surface::Torus { f, rmaj, rmin } if parallel(&f.z) && off_axis(&f.o) <= near => {
                if corner.replace((height(&f.o), *rmaj, *rmin)).is_some() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    let radius = radius?;
    let [h0, h1] = flats[..] else {
        return None;
    };
    let (bottom, top) = (h0.min(h1), h0.max(h1));
    if let Some((h, a, c)) = corner
        && ((a + c - radius).abs() > near || (h - c - bottom).abs() > near)
    {
        return None;
    }
    // The path in the frame it lies flat in.
    let x = {
        let d = segs.first()?.dir(0.0);
        (d - up * d.dot(&up)).try_normalize(1e-12)?
    };
    let y = up.cross(&x);
    let to_flat = Affine {
        m: nalgebra::Matrix3::from_rows(&[x.transpose(), y.transpose(), up.transpose()]),
        t: -nalgebra::Matrix3::from_rows(&[x.transpose(), y.transpose(), up.transpose()])
            * p0.coords,
    };
    let wire: Vec<(Curve, f64, f64)> = segs
        .iter()
        .map(|s| {
            let c = s.c.transformed(&to_flat);
            if s.rev {
                let (c, t0, t1) = c.reversed(s.t0, s.t1);
                (c, t0, t1)
            } else {
                (c, s.t0, s.t1)
            }
        })
        .collect();
    let slot = match super::offset::prism::wire_slot(&wire, radius, p0, x, up, bottom, top) {
        Ok(s) => s,
        Err(e) => {
            if debug {
                eprintln!("end mill: no slot: {e}");
            }
            return None;
        }
    };
    let body = match corner {
        None => slot,
        Some((_, _, c)) => {
            // Its bottom edges (all in the bottom plane), rounded.
            let mut kk = NativeKernel::new();
            let edges: Vec<crate::EdgeId> = slot
                .edges
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    !e.degenerate
                        && (0..=4).all(|i| {
                            let t = e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 4.0;
                            (height(&e.curve.point(t)) - bottom).abs() <= near
                        })
                })
                .map(|(i, _)| crate::EdgeId(i as u64))
                .collect();
            let id = kk.insert(slot);
            match crate::Kernel::fillet(&mut kk, id, &edges, c) {
                Ok(r) => (**kk.get(*r.bodies.first()?).ok()?).clone(),
                Err(e) => {
                    if debug {
                        eprintln!(
                            "end mill: the slot's bottom edges ({}) wouldn't round: {e}",
                            edges.len()
                        );
                    }
                    return None;
                }
            }
        }
    };
    if !body.check().is_empty() || body.solids.len() != 1 {
        if debug {
            eprintln!("end mill: the slot isn't sound: {:?}", body.check().first());
        }
        return None;
    }
    let id = k.insert(body);
    Some(OpResult {
        bodies: vec![id],
        history: History::default(),
    })
}

/// A whole torus, or a bull-nose mill, carried along a path of one curve
/// ([`SolidMotion::Path`]): what it passes through, exactly ([`super::sweep_tube::torus`],
/// [`super::sweep_mill::mill`]). The frame rides the curve by the curve's own
/// parameter, which is smooth (by its length, as [`PathFrames`] has it, the same places come in
/// another order of time, which is all the same to what is swept). None for another body or a
/// path of several curves.
fn torus_along(
    k: &mut NativeKernel,
    b: &Body,
    path: &[crate::PathCurve],
    up: V3,
) -> Option<Result<OpResult>> {
    let torus = matches!(b.faces.as_slice(), [f] if matches!(f.surf, super::surface::Surface::Torus { .. }));
    if !torus && !super::sweep_mill::is_mill(b) {
        return None;
    }
    let segs = super::sweep::path::path_segments(k, path).ok()?;
    let [s] = segs.as_slice() else {
        return None;
    };
    let up = up.normalize();
    let (ta, tb) = if s.rev { (s.t1, s.t0) } else { (s.t0, s.t1) };
    let frame = |t: f64| -> Option<(nalgebra::Matrix3<f64>, P3)> {
        let (p, d1, _) = s.c.d2(ta + (tb - ta) * t);
        let d1 = d1 * (tb - ta).signum();
        let x = (d1 - up * d1.dot(&up)).try_normalize(1e-12)?;
        Some((nalgebra::Matrix3::from_columns(&[x, up.cross(&x), up]), p))
    };
    // (Every frame along it, and a little past its ends, must stand.)
    if (0..=64).any(|i| frame(-0.01 + 1.02 * i as f64 / 64.0).is_none()) {
        return None;
    }
    let (m0, o0) = frame(0.0)?;
    let place = |t: f64| {
        let (m, o) = frame(t).unwrap_or((m0, o0));
        let r = m * m0.transpose();
        (r, o.coords - r * o0.coords)
    };
    let motion = super::sweep_tube::Motion::Place(&place);
    if torus {
        super::sweep_tube::torus(k, b, &motion)
    } else {
        super::sweep_mill::mill(k, b, &motion)
    }
}

/// [`meshed`] with the body's distance from its own mesh (closed, within a thousandth of its
/// reach).
fn meshed_body(k: &mut NativeKernel, b: &Body, place: &Place) -> Result<OpResult> {
    let md = super::mesh_distance::MeshDistance::new(b, 1e-3 * b.bbox().diagonal().max(tol::POINT))
        .ok_or(KernelError::Unsupported(
            "sweeping a solid whose mesh doesn't close (for its distance)",
        ))?;
    meshed(k, b, &|p| md.distance(p), place)
}

/// A body of revolution about the line through the path's start along `up` (every face a plane
/// square to it, or a cylinder, cone, torus, sphere or revolution about it; every edge a circle
/// about it or in a plane through it), carried along a path lying in a plane square to `up`
/// ([`SolidMotion::Path`], its frame upright): each place is the body moved so its axis stands on
/// the path (its turn about its own axis changes nothing), so what it passes through is its section
/// through the axis swept along the path, square to it, with the body where it starts and where
/// it ends. A single Bézier path is swept in quarters (a path looping over itself sweeps no
/// quarter through itself), all united. None where that isn't
/// so, or the sweep fails.
#[allow(clippy::too_many_lines)]
fn upright_revolution_along(
    k: &mut NativeKernel,
    tool: BodyId,
    b: &Body,
    path: &[crate::PathCurve],
    up: V3,
) -> Option<OpResult> {
    use crate::PathCurve;
    let up = up.try_normalize(tol::TINY)?;
    let segs = super::sweep::path::path_segments(k, path).ok()?;
    let (first, last) = (segs.first()?, segs.last()?);
    let p0 = first.start();
    let size = b.bbox().diagonal().max(crate::native::tol::POINT);
    let near = tol::REL_POINT * size.max((last.end() - p0).norm());
    // The path in the plane square to `up` through its start.
    for s in &segs {
        for i in 0..=16 {
            let t = s.t0 + (s.t1 - s.t0) * i as f64 / 16.0;
            if (s.c.point(t) - p0).dot(&up).abs() > near {
                return None;
            }
        }
    }
    // Symmetric about the line L through p0 along `up`.
    let off_axis = |p: &P3| {
        let d = p - p0;
        (d - up * d.dot(&up)).norm()
    };
    let parallel = |v: &V3| v.normalize().cross(&up).norm() < 1e-9;
    let about = |f: &Frame| parallel(&f.z) && off_axis(&f.o) <= near;
    for f in &b.faces {
        let ok = match &f.surf {
            Surface::Plane { f } => parallel(&f.x.cross(&f.y)),
            Surface::Cylinder { f, .. } | Surface::Cone { f, .. } | Surface::Torus { f, .. } => {
                about(f)
            }
            Surface::Sphere { f, .. } => off_axis(&f.o) <= near,
            Surface::Revolution { f, .. } => about(f),
            _ => false,
        };
        if !ok {
            return None;
        }
    }
    for e in &b.edges {
        if e.degenerate {
            continue;
        }
        let pts: Vec<P3> = (0..=8)
            .map(|i| e.curve.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0))
            .collect();
        // A circle about L (one height, one distance), or in one plane through L.
        let h0 = (pts[0] - p0).dot(&up);
        let r0 = off_axis(&pts[0]);
        let circle = pts
            .iter()
            .all(|p| ((p - p0).dot(&up) - h0).abs() <= near && (off_axis(p) - r0).abs() <= near);
        let horiz = |p: &P3| {
            let d = p - p0;
            d - up * d.dot(&up)
        };
        let meridian = match pts.iter().map(horiz).find(|h| h.norm() > near) {
            None => true,
            Some(h) => {
                let n = h.cross(&up).normalize();
                pts.iter().all(|p| (p - p0).dot(&n).abs() <= near)
            }
        };
        if !circle && !meridian {
            return None;
        }
    }
    // The pieces of the path: a single Bézier curve in quarters.
    let pieces: Vec<Vec<PathCurve>> = match path {
        [PathCurve::Bezier3 { poles, weights }] if poles.len() >= 2 => {
            let w: Vec<f64> = if weights.is_empty() {
                vec![1.0; poles.len()]
            } else {
                weights.clone()
            };
            let hom: Vec<nalgebra::Vector4<f64>> = poles
                .iter()
                .zip(&w)
                .map(|(p, w)| nalgebra::Vector4::new(p.x * w, p.y * w, p.z * w, *w))
                .collect();
            // De Casteljau: the poles of the part from s0 to s1.
            let part = |s0: f64, s1: f64| -> Vec<nalgebra::Vector4<f64>> {
                let split = |q: &[nalgebra::Vector4<f64>], t: f64| {
                    let mut left = vec![q[0]];
                    let mut right = vec![q[q.len() - 1]];
                    let mut row = q.to_vec();
                    while row.len() > 1 {
                        row = row.windows(2).map(|w| w[0] + (w[1] - w[0]) * t).collect();
                        left.push(row[0]);
                        right.push(row[row.len() - 1]);
                    }
                    right.reverse();
                    (left, right)
                };
                let (_, r) = split(&hom, s0);
                let (l, _) = split(&r, ((s1 - s0) / (1.0 - s0)).clamp(0.0, 1.0));
                l
            };
            [0.0, 0.25, 0.5, 0.75]
                .iter()
                .map(|&s0| {
                    let q = part(s0, s0 + 0.25);
                    vec![PathCurve::Bezier3 {
                        poles: q
                            .iter()
                            .map(|h| P3::new(h.x / h.w, h.y / h.w, h.z / h.w))
                            .collect(),
                        weights: if weights.is_empty() {
                            Vec::new()
                        } else {
                            q.iter().map(|h| h.w).collect()
                        },
                    }]
                })
                .collect()
        }
        _ => vec![path.to_vec()],
    };
    // The body's place where a piece starts or ends: turned about `up` from the path's start
    // tangent to the tangent there, moved to the point there.
    let flat = |v: V3| (v - up * v.dot(&up)).try_normalize(tol::TINY);
    let t0 = flat(first.dir(0.0))?;
    let place = |q: P3, t: V3| -> Option<(nalgebra::Matrix3<f64>, V3)> {
        let t = flat(t)?;
        let ang = t0.cross(&t).dot(&up).atan2(t0.dot(&t));
        let r =
            *nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(up), ang).matrix();
        Some((r, q.coords - r * p0.coords))
    };
    // The section: the body split by the plane through p0 square to the path there; the cut face.
    let plane = Plane {
        origin: p0,
        x_dir: nalgebra::Unit::new_normalize(up.cross(&t0)),
        normal: nalgebra::Unit::new_normalize(t0),
    };
    let halves = super::ops::split(k, tool, &crate::SplitTool::Plane(plane), 0).ok()?;
    let half = *halves.bodies.first()?;
    let hb = k.get(half).ok()?.as_ref().clone();
    let face = (0..hb.faces.len()).find(|&fi| match &hb.faces[fi].surf {
        Surface::Plane { f } => {
            f.x.cross(&f.y).normalize().cross(&t0).norm() < 1e-9
                && (f.o - p0).dot(&t0).abs() <= near
        }
        _ => false,
    })?;
    let mut made: Vec<BodyId> = halves.bodies.clone();
    let result = (|| -> Option<Body> {
        let mut all: Option<Body> = None;
        let add = |all: &mut Option<Body>, x: Body| -> Option<()> {
            *all = Some(match all.take() {
                None => x,
                Some(a) => {
                    super::boolean::boolean_bodies(&a, &x, crate::BoolOp::Union)
                        .ok()?
                        .body
                }
            });
            Some(())
        };
        let tb = k.get(tool).ok()?.as_ref().clone();
        add(&mut all, tb)?;
        let mut end = None;
        for piece in &pieces {
            // The piece's start place: the section moved there, then swept along the piece.
            let psegs = super::sweep::path::path_segments(k, piece).ok()?;
            let (ps, pe) = (psegs.first()?, psegs.last()?);
            let (m, d) = place(ps.start(), ps.dir(0.0))?;
            let moved = k.transform_affine(half, m, d).ok()?;
            let mh = *moved.bodies.first()?;
            made.push(mh);
            let profile_plane = Plane {
                origin: P3::from(m * plane.origin.coords + d),
                x_dir: nalgebra::Unit::new_normalize(m * plane.x_dir.into_inner()),
                normal: nalgebra::Unit::new_normalize(m * plane.normal.into_inner()),
            };
            let spec = crate::SweepSpec {
                body: crate::BodyKind::Solid,
                path: piece.clone(),
                control: crate::SweepControl::None,
                corners: crate::SweepCorners::Mitre,
                faces: vec![crate::FaceInput {
                    body: mh,
                    face: crate::FaceId(face as u64),
                    source: 0,
                }],
            };
            let swept =
                crate::Kernel::sweep_with(k, &Profile::new(profile_plane, vec![]), &spec).ok()?;
            let sid = *swept.bodies.first()?;
            made.push(sid);
            add(&mut all, k.get(sid).ok()?.as_ref().clone())?;
            end = Some((pe.end(), pe.dir(1.0)));
        }
        // The body where the path ends (at the joints between pieces their sweeps meet in the
        // section, and cover it).
        let (q, t) = end?;
        let (m, d) = place(q, t)?;
        let at = k.transform_affine(tool, m, d).ok()?;
        let aid = *at.bodies.first()?;
        made.push(aid);
        add(&mut all, k.get(aid).ok()?.as_ref().clone())?;
        all
    })();
    for id in made {
        k.release(id);
    }
    let body = result?;
    (body.is_solid() && body.check().is_empty()).then(|| OpResult {
        bodies: vec![k.insert(body)],
        history: History::default(),
    })
}

/// A path's frames turning about one fixed axis along `up` as they rise (a helix, its lead
/// changing or not): a point on the axis, the turn from the path's start to its end (radians),
/// and the rise as the turn goes (a function of the turn from the start, 0 there). None when the
/// frames turn about an axis that moves (more than a millionth of the size).
fn path_screw(frames: &PathFrames, size: f64) -> Option<(P3, f64, super::sweep_faces::Rise)> {
    let up = frames.up;
    let (ex, ey) = {
        let x = any_perpendicular(&up).normalize();
        (x, up.cross(&x))
    };
    let n = 4096;
    let mut turns: Vec<f64> = Vec::with_capacity(n + 1);
    let mut moves: Vec<V3> = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let (r, d) = frames.place(i as f64 / n as f64);
        let x = r * ex;
        let a = x.dot(&ey).atan2(x.dot(&ex));
        let a = match turns.last() {
            Some(&p) => p + (a - p - TAU * ((a - p) / TAU).round()),
            None => a,
        };
        turns.push(a);
        moves.push(d);
    }
    let total = turns[n] - turns[0];
    // Turning one way all along.
    if total.abs() < 1e-6
        || turns
            .windows(2)
            .any(|w| (w[1] - w[0]) * total.signum() <= 0.0)
    {
        return None;
    }
    // The axis: the point o (in the plane square to up) each place's shift (I − R)o is, by least
    // squares.
    let (mut a11, mut a12, mut a22, mut b1, mut b2) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..=n {
        let (c, s_) = (turns[i].cos(), turns[i].sin());
        // I − R in the plane: [[1 − c, s], [−s, 1 − c]].
        let (m11, m12, m21, m22) = (1.0 - c, s_, -s_, 1.0 - c);
        let (dx, dy) = (moves[i].dot(&ex), moves[i].dot(&ey));
        a11 += m11 * m11 + m21 * m21;
        a12 += m11 * m12 + m21 * m22;
        a22 += m12 * m12 + m22 * m22;
        b1 += m11 * dx + m21 * dy;
        b2 += m12 * dx + m22 * dy;
    }
    let det = a11 * a22 - a12 * a12;
    if det.abs() < 1e-12 {
        return None;
    }
    let (ox, oy) = ((a22 * b1 - a12 * b2) / det, (a11 * b2 - a12 * b1) / det);
    for i in 0..=n {
        let (c, s_) = (turns[i].cos(), turns[i].sin());
        let (dx, dy) = (moves[i].dot(&ex), moves[i].dot(&ey));
        let off = ((1.0 - c) * ox + s_ * oy - dx).hypot(-s_ * ox + (1.0 - c) * oy - dy);
        if off > 1e-6 * size {
            if crate::native::env_flag!("CADRS_SWEEP_DEBUG") {
                eprintln!("path screw: the axis moves ({off:.2e} at {i} of {n})");
            }
            return None;
        }
    }
    let o = P3::from(ex * ox + ey * oy);
    // The rise against the turn, interpolated (a cubic through every place).
    let pts: Vec<P3> = (0..=n)
        .map(|i| P3::new(0.0, moves[i].dot(&up) - moves[0].dot(&up), 0.0))
        .collect();
    let params: Vec<f64> = turns.iter().map(|a| a - turns[0]).collect();
    let curve = super::nurbs::NurbsCurve::interpolate(&pts, &params, 3, None, None);
    let rise: super::sweep_faces::Rise = std::sync::Arc::new(move |a: f64| curve.point(a).y);
    Some((o, total, rise))
}

/// `target` less what `b` passes through screwed about the axis through `o` along `up` by
/// `angle`, rising `rise(θ)` at the turn θ: half a turn at a time, each the target less the
/// solid where it starts and each of its leading faces' sweeps. None when that fails.
fn cut_rising_screw(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    o: P3,
    up: V3,
    angle: f64,
    rise: super::sweep_faces::Rise,
) -> Option<OpResult> {
    let n = (angle.abs() / PI).ceil().max(1.0) as usize;
    // Every half turn's pieces, then the target less them (glued where they only touch: the
    // half turns meet along the solid where one ends and the next starts).
    let mut all: Vec<Body> = Vec::new();
    for i in 0..n {
        let (a0, a1) = (
            angle * i as f64 / n as f64,
            angle * (i + 1) as f64 / n as f64,
        );
        let r = nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(up), a0);
        let m = *r.matrix();
        let place = Affine {
            m,
            t: o.coords - m * o.coords + up * rise(a0),
        };
        let here = b.transformed(&place);
        let (f, base) = (rise.clone(), rise(a0));
        let part: super::sweep_faces::Rise = std::sync::Arc::new(move |s: f64| f(a0 + s) - base);
        match super::sweep_faces::rising_screw_sweeps(k, &here, o, up, a1 - a0, part) {
            // (After the first, the solid where a half turn starts is inside the one before.)
            Ok(p) => all.extend(p.into_iter().skip(usize::from(i > 0))),
            Err(e) => {
                if crate::native::env_flag!("CADRS_SWEEP_DEBUG") {
                    eprintln!("path screw: part {i} of {n}: {e}");
                }
                return None;
            }
        }
    }
    let bodies = cut_by_pieces(k, &[target], all)?;
    Some(OpResult {
        bodies,
        history: History::default(),
    })
}

/// A solid carried along a path, exactly where it can be: an upright solid of revolution along a
/// planar path ([`upright_revolution_along`]); a prism along `up` carried along a path in a plane
/// square to `up`, whose frames then only turn about `up` (a planar motion: the prism's
/// polygon's swept region extruded, [`super::sweep_planar::sweep_planar_by`]).
fn path_sweep_exact(
    k: &mut NativeKernel,
    tool: BodyId,
    b: &Body,
    path: &[crate::PathCurve],
    up: V3,
) -> Option<OpResult> {
    if let Some(r) = upright_revolution_along(k, tool, b, path, up) {
        return Some(r);
    }
    if let Some(r) = end_mill_along(k, b, path, up) {
        return Some(r);
    }
    let frames = PathFrames::new(k, path, up).ok()?;
    let up = up.normalize();
    let o0 = frames.frame(0.0).ok()?.1;
    let size = b.bbox().diagonal().max(frames.total);
    let planar = (0..=256).all(|i| {
        frames
            .frame(i as f64 / 256.0)
            .is_ok_and(|(_, o)| (o - o0).dot(&up).abs() <= 1e-9 * size)
    });
    if !planar {
        return None;
    }
    let r = super::sweep_planar::sweep_planar_by(k, b, up, &|t| frames.place(t));
    if crate::native::env_flag!("CADRS_SWEEP_DEBUG")
        && let Err(e) = &r
    {
        eprintln!("path sweep: the planar sweep failed: {e}");
    }
    r.ok()
}

/// The frames riding a path ([`SolidMotion::Path`]): by its length, t from 0 to 1, the path's
/// point and its tangent made square to `up`.
struct PathFrames {
    segs: Vec<super::sweep::path::Seg>,
    /// Each segment's samples (its parameter, the length from the path's start), 256 of them.
    table: Vec<Vec<(f64, f64)>>,
    total: f64,
    up: V3,
    start: (nalgebra::Matrix3<f64>, P3),
}

impl PathFrames {
    fn new(k: &NativeKernel, path: &[crate::PathCurve], up: V3) -> Result<Self> {
        let segs = super::sweep::path::path_segments(k, path)?;
        if segs.is_empty() {
            return Err(KernelError::InvalidParameter("The path is empty".into()));
        }
        let mut table = Vec::with_capacity(segs.len());
        let mut at = 0.0;
        for s in &segs {
            let n = 256;
            let mut row = Vec::with_capacity(n + 1);
            // (A reversed segment runs from its end parameter back.)
            let (ta, tb) = if s.rev { (s.t1, s.t0) } else { (s.t0, s.t1) };
            let mut prev = s.c.point(ta);
            for i in 0..=n {
                let t = ta + (tb - ta) * i as f64 / n as f64;
                let p = s.c.point(t);
                at += (p - prev).norm();
                prev = p;
                row.push((t, at));
            }
            table.push(row);
        }
        let up = up.normalize();
        let mut me = PathFrames {
            segs,
            table,
            total: at.max(tol::POINT),
            up,
            start: (nalgebra::Matrix3::identity(), P3::origin()),
        };
        me.start = me.frame(0.0)?;
        Ok(me)
    }

    /// The frame (its axes as columns) and origin at t.
    fn frame(&self, t: f64) -> Result<(nalgebra::Matrix3<f64>, P3)> {
        let l = t.clamp(0.0, 1.0) * self.total;
        let si = self
            .table
            .iter()
            .position(|row| row.last().is_some_and(|r| r.1 >= l))
            .unwrap_or(self.table.len() - 1);
        let row = &self.table[si];
        let j = row.partition_point(|r| r.1 < l).clamp(1, row.len() - 1);
        let (a, b) = (row[j - 1], row[j]);
        let f = if b.1 > a.1 {
            (l - a.1) / (b.1 - a.1)
        } else {
            0.0
        };
        let s = &self.segs[si];
        let u = a.0 + (b.0 - a.0) * f.clamp(0.0, 1.0);
        let (p, d1, _) = s.c.d2(u);
        // (The way of travel: the samples' parameters' way.)
        let d1 = d1 * (row[row.len() - 1].0 - row[0].0).signum();
        let x = (d1 - self.up * d1.dot(&self.up))
            .try_normalize(1e-12)
            .ok_or_else(|| {
                KernelError::InvalidParameter("The path runs along the frame's up direction".into())
            })?;
        let y = self.up.cross(&x);
        Ok((nalgebra::Matrix3::from_columns(&[x, y, self.up]), p))
    }

    /// The place at t: the rotation and translation from the start frame to t's.
    fn place(&self, t: f64) -> (nalgebra::Rotation3<f64>, V3) {
        let (m0, o0) = self.start;
        let (m, o) = self.frame(t).unwrap_or(self.start);
        let r = nalgebra::Rotation3::from_matrix_unchecked(m * m0.transpose());
        (r, o.coords - r * o0.coords)
    }
}

/// A closed mesh's slivers removed: a triangle whose corner lies within `eps` of its opposite
/// side, inside it, is dropped and the triangle across that side split at the corner (the two
/// covered the same area; the split keeps every side matched). A few passes.
fn unsliver(pts: &[P3], tris: &mut Vec<[u32; 3]>, eps: f64) {
    use std::collections::HashMap;
    for _ in 0..4 {
        let mut half: HashMap<(u32, u32), usize> = HashMap::with_capacity(tris.len() * 3);
        for (ti, t) in tris.iter().enumerate() {
            for k in 0..3 {
                half.insert((t[k], t[(k + 1) % 3]), ti);
            }
        }
        let mut dead = vec![false; tris.len()];
        let mut added: Vec<[u32; 3]> = Vec::new();
        let mut changed = false;
        for ti in 0..tris.len() {
            if dead[ti] {
                continue;
            }
            let t = tris[ti];
            // The longest side and the corner opposite it.
            let side = |k: usize| (pts[t[(k + 1) % 3] as usize] - pts[t[k] as usize]).norm();
            let k = (0..3)
                .max_by(|&x, &y| side(x).total_cmp(&side(y)))
                .unwrap_or(0);
            let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            let (pa, pb, pc) = (pts[a as usize], pts[b as usize], pts[c as usize]);
            let ab = pb - pa;
            let l2 = ab.norm_squared();
            if l2 <= 0.0 {
                continue;
            }
            let f = (pc - pa).dot(&ab) / l2;
            if !(f > 0.0 && f < 1.0) || (pa + ab * f - pc).norm() > eps {
                continue;
            }
            let Some(&ui) = half.get(&(b, a)) else {
                continue;
            };
            if dead[ui] || ui == ti {
                continue;
            }
            let u = tris[ui];
            let d = u.iter().copied().find(|&v| v != a && v != b);
            let Some(d) = d else { continue };
            if d == c {
                continue;
            }
            dead[ti] = true;
            dead[ui] = true;
            added.push([b, c, d]);
            added.push([c, a, d]);
            changed = true;
        }
        if !changed {
            return;
        }
        let mut next: Vec<[u32; 3]> = tris
            .iter()
            .zip(&dead)
            .filter(|(_, d)| !**d)
            .map(|(t, _)| *t)
            .collect();
        next.extend(added);
        *tris = next;
    }
}

/// How far `p` lies outside the box (0 inside it).
fn box_distance(bb: &Bbox, p: &P3) -> f64 {
    let d = V3::new(
        (bb.min.x - p.x).max(p.x - bb.max.x).max(0.0),
        (bb.min.y - p.y).max(p.y - bb.max.y).max(0.0),
        (bb.min.z - p.z).max(p.z - bb.max.z).max(0.0),
    );
    d.norm()
}

/// A place of a motion at t ∈ [0, 1]: the rotation and translation taking the body's points
/// there.
pub(crate) type Place<'a> = dyn Fn(f64) -> (nalgebra::Rotation3<f64>, V3) + Sync + 'a;

/// What body `b` fills as it moves through `place(t)`, t from 0 to 1, meshed: `dist` its signed
/// distance (negative inside; a bound below it, of the right sign, will do). See
/// [`crate::SolidMotion::Rigid`].
pub(crate) fn meshed(
    k: &mut NativeKernel,
    b: &Body,
    dist: &(dyn Fn(&P3) -> f64 + Sync),
    place: &Place,
) -> Result<OpResult> {
    meshed_within(k, b, dist, place, None)
}

/// A target's box and its depth (positive inside), for [`meshed_within`].
type Within<'a> = Option<(Bbox, &'a (dyn Fn(&P3) -> f64 + Sync))>;

/// [`meshed`]; with `within` (a target's box and its depth, positive inside), the target less
/// what the body fills: the grid over the target's box, the field the lesser of its depth and
/// the negated fill's.
fn meshed_within(
    k: &mut NativeKernel,
    b: &Body,
    dist: &(dyn Fn(&P3) -> f64 + Sync),
    place: &Place,
    within: Within,
) -> Result<OpResult> {
    let t0 = web_time::Instant::now();
    // The places sampled, and their inverses (world to body: x ↦ R x − e): 160, or more for a
    // motion that moves the body's box farther than a fiftieth of its reach between them.
    let bb0 = b.bbox();
    let reach = bb0.diagonal().max(tol::POINT);
    let mut nt = 160usize;
    let step_of = |nt: usize| {
        (0..nt)
            .map(|i| {
                let (a, b2) = (
                    place(i as f64 / nt as f64),
                    place((i + 1) as f64 / nt as f64),
                );
                [bb0.min, bb0.max, bb0.center()]
                    .iter()
                    .map(|c| ((a.0 * c.coords + a.1) - (b2.0 * c.coords + b2.1)).norm())
                    .fold(0.0, f64::max)
            })
            .fold(0.0, f64::max)
    };
    while nt < 1024 && step_of(nt) > reach / 50.0 {
        nt *= 2;
    }
    let places: Vec<(nalgebra::Rotation3<f64>, V3)> =
        (0..=nt).map(|i| place(i as f64 / nt as f64)).collect();
    let inverse: Vec<(nalgebra::Rotation3<f64>, V3)> = places
        .iter()
        .map(|(r, d)| {
            let ri = r.inverse();
            (ri, ri * d)
        })
        .collect();
    // The grid over the places' boxes, its sides a multiple of eight steps.
    let corners = |bb: &Bbox| -> [P3; 8] {
        std::array::from_fn(|i| {
            P3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            )
        })
    };
    let mut bb = Bbox::empty();
    for (r, d) in &places {
        for c in corners(&bb0) {
            bb.add(&P3::from(r * c.coords + d));
        }
    }
    if let Some((tb, _)) = &within {
        bb = *tb;
    }
    let ext = bb.max - bb.min;
    let h = ext.max() / 100.0;
    let lo = bb.min - V3::repeat(2.0 * h);
    let n = [0, 1, 2].map(|i| 8 * ((ext[i] + 4.0 * h) / h / 8.0).ceil() as usize + 1);
    let at = |i: usize, j: usize, l: usize| lo + V3::new(i as f64 * h, j as f64 * h, l as f64 * h);
    // How far a grid point's place in the body moves between samples, at most (its map is
    // affine, so at a corner of the grid's box), over two: what the samples' best can miss.
    let mut gb = Bbox::empty();
    gb.add(&lo);
    gb.add(&at(n[0] - 1, n[1] - 1, n[2] - 1));
    let mut miss = 0.0f64;
    for w in inverse.windows(2) {
        for c in corners(&gb) {
            miss = miss.max(((w[0].0 * c.coords - w[0].1) - (w[1].0 * c.coords - w[1].1)).norm());
        }
    }
    let miss = 0.6 * miss;
    // How deep x lies in the body's places (its signed distance from the region, positive
    // inside): the best sampled instant, refined by golden-section search about it near the
    // boundary. Far from it the value is a bound: of the right sign and no deeper than the
    // point lies.
    let fill = |x: &P3| -> f64 {
        let mut best = (f64::NEG_INFINITY, 0usize);
        for (i, (r, e)) in inverse.iter().enumerate() {
            let q = P3::from(r * x.coords - e);
            // (Outside the body's box it lies no deeper than that box's distance.)
            let off = box_distance(&bb0, &q);
            if off > 0.0 && -off <= best.0 {
                continue;
            }
            let v = -dist(&q);
            if v > best.0 {
                best = (v, i);
            }
        }
        if best.0 >= 2.0 * h {
            return best.0;
        }
        if best.0 <= -miss - 2.0 * h {
            return best.0 + miss;
        }
        let f = |t: f64| {
            let (r, d) = place(t);
            -dist(&P3::from(r.inverse() * (x.coords - d)))
        };
        let dt = 1.0 / nt as f64;
        let c = best.1 as f64 * dt;
        let (_, v) = golden_max_in(&f, (c - dt).max(0.0), (c + dt).min(1.0), 24);
        v.max(best.0)
    };
    let depth = |x: &P3| -> f64 {
        match &within {
            Some((_, td)) => td(x).min(-fill(x)),
            None => fill(x),
        }
    };
    contour(k, lo, n, h, &depth, t0)
}
