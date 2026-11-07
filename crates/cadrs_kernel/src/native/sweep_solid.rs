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
use std::f64::consts::{PI, TAU};

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

/// The body as a solid of revolution of a convex section, if it is one: one whole torus, or
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
            let (v, w) = sl.extreme(q, far, seed)?;
            seed = w;
            out.push((v, q));
        }
        out.push(b);
        Some(out)
    };
    let (far, near) = rayon::join(
        || side(true, lo_far, hi_far),
        || side(false, lo_near, hi_near),
    );
    Some(Outline {
        far: far?,
        near: near?,
    })
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

/// [`crate::Kernel::sweep_solid`].
pub fn sweep_solid(k: &mut NativeKernel, body: BodyId, motion: &SolidMotion) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    if let SolidMotion::Planar { normal, steps } = motion {
        return super::sweep_planar::sweep_planar(k, &b, normal.into_inner(), steps);
    }
    let rev = revolved(&b).ok_or(KernelError::Unsupported(
        "sweeping solids other than a torus or a convex section revolved",
    ))?;
    if let SolidMotion::Rigid { steps } = motion {
        return rigid(k, &b, &rev, steps);
    }
    let (axis, kk, span) = match *motion {
        SolidMotion::Planar { .. } | SolidMotion::Rigid { .. } => unreachable!(),
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
    let n = 160;
    let ol = outline(&sl, lo, hi, n).ok_or_else(|| KernelError::InvalidResult {
        op: "solid sweep",
        why: "the body's outline has a slice it misses".into(),
    })?;
    if std::env::var_os("CADRS_SWEEP_SOLID_DEBUG").is_some() {
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
    // Each side a cubic B-spline through its points (by chord length), the sides joined at the
    // ends by the extreme slices' stretches where they don't meet.
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
            let r0 = ol
                .far
                .iter()
                .map(|p| p.0)
                .fold(0.0, f64::max)
                .max(tol::POINT);
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
fn rigid_at(steps: &[RigidStep], t: f64) -> (nalgebra::Rotation3<f64>, V3) {
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
/// marching tetrahedra made.
const DECIMATE: f64 = 0.02;

/// [`crate::Kernel::sweep_solid`] for a rigid motion: the region some instant of the body holds,
/// meshed by marching tetrahedra and made solid.
fn rigid(k: &mut NativeKernel, b: &Body, rev: &Revolved, steps: &[RigidStep]) -> Result<OpResult> {
    use rayon::prelude::*;
    let prof = std::env::var("CADRS_PROFILE").is_ok();
    let t0 = web_time::Instant::now();
    // The places sampled, and their inverses (world to body: x ↦ R x − e).
    let nt = 160;
    let places: Vec<(nalgebra::Rotation3<f64>, V3)> = (0..=nt)
        .map(|i| rigid_at(steps, i as f64 / nt as f64))
        .collect();
    let inverse: Vec<(nalgebra::Rotation3<f64>, V3)> = places
        .iter()
        .map(|(r, d)| {
            let ri = r.inverse();
            (ri, ri * d)
        })
        .collect();
    // The grid over the places' boxes, its sides a multiple of eight steps.
    let bb0 = b.bbox();
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
    let ext = bb.max - bb.min;
    let h = ext.max() / 100.0;
    let lo = bb.min - V3::repeat(2.0 * h);
    let n = [0, 1, 2].map(|i| 8 * ((ext[i] + 4.0 * h) / h / 8.0).ceil() as usize + 1);
    let at = |i: usize, j: usize, l: usize| lo + V3::new(i as f64 * h, j as f64 * h, l as f64 * h);
    let idx = |i: usize, j: usize, l: usize| (l * n[1] + j) * n[0] + i;
    let ijl = |g: usize| (g % n[0], (g / n[0]) % n[1], g / (n[0] * n[1]));
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
    let depth = |x: &P3| -> f64 {
        let mut best = (f64::NEG_INFINITY, 0usize);
        for (i, (r, e)) in inverse.iter().enumerate() {
            let v = -rev.distance(&P3::from(r * x.coords - e));
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
            let (r, d) = rigid_at(steps, t);
            -rev.distance(&P3::from(r.inverse() * (x.coords - d)))
        };
        let dt = 1.0 / nt as f64;
        let c = best.1 as f64 * dt;
        let (_, v) = golden_max_in(&f, (c - dt).max(0.0), (c + dt).min(1.0), 24);
        v.max(best.0)
    };
    // The depth from coarse to fine (steps of 8, 4, 2 and 1): a point whose nearest coarser
    // node lies deeper (or farther out) than their distance and a step more takes its sign
    // from it (the depth changes no faster than the distance), so the depth is found only near
    // the boundary, and every grid edge it crosses has its ends' found.
    let mut field = vec![f64::NAN; n[0] * n[1] * n[2]];
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
    let (pts, mesh) = super::decimate::decimate(&placed, &mesh, DECIMATE * h);
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
    let id = k.mesh_solid(&out, 1e-9 * h.max(1.0))?;
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
