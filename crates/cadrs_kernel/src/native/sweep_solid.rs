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
            let (v, w) = sl.extreme(q, far, seed)?;
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
    // One slide, turn or screw of any solid: exactly, from its leading faces' sweeps (split
    // where its faces meet the motion); else meshed.
    if let SolidMotion::Rigid { steps } = motion {
        let mut exact = None;
        if (1..=2).contains(&steps.len()) {
            match super::sweep_faces::by_faces(k, &b, steps) {
                Ok(r) => return Ok(r),
                Err(e) => exact = Some(e),
            }
        }
        let place = |t: f64| rigid_at(steps, t);
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
    let prof = std::env::var("CADRS_PROFILE").is_ok();
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
    let b = (**k.get(body)?).clone();
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

/// `target` less the tool `b` swept by `steps` (turning more than half a turn) in pieces of at
/// most half a turn, cut one after another: None where a piece isn't swept or cut soundly.
fn cut_in_pieces(
    k: &mut NativeKernel,
    target: BodyId,
    b: &Body,
    steps: &[RigidStep],
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
    let mut acc = vec![target];
    for i in 0..n {
        let mut pieces: Vec<Body> = Vec::new();
        let swept = super::sweep_faces::by_faces_or_pieces(k, b, &part(i), Some(&mut pieces)).ok();
        let cut = match swept.as_ref().map(|r| r.bodies.as_slice()) {
            Some([s]) if acc.len() == 1 => {
                let cut = k.boolean(crate::BoolOp::Subtract, acc[0], &[*s]);
                k.release(*s);
                cut.ok()
                    .filter(|c| {
                        c.bodies.len() == 1
                            && k.get(c.bodies[0]).is_ok_and(|b| b.check().is_empty())
                    })
                    .map(|c| c.bodies)
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
            return None;
        };
        release_unless(k, &acc, target);
        acc = c;
    }
    Some(OpResult {
        bodies: acc,
        history: History::default(),
    })
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
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
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
                Ok(c)
                    if c.bodies
                        .iter()
                        .all(|&id| k.get(id).is_ok_and(|b| b.check().is_empty())) =>
                {
                    next.extend(c.bodies);
                }
                r => {
                    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
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
pub fn cut_swept(
    k: &mut NativeKernel,
    target: BodyId,
    tool: BodyId,
    motion: &SolidMotion,
) -> Result<OpResult> {
    let b = (**k.get(tool)?).clone();
    // A motion whose ends keep clear of the target: within it the endless motion (a whole turn,
    // a screw without end) fills what this one does, and is made exactly as a whole (the solid
    // of revolution, the screw's helical sweep).
    if let Some(endless) = endless_within(&**k.get(target)?, &b, motion) {
        let swept = sweep_solid(k, tool, &endless).ok();
        if let Some(r) = swept
            && let [s] = r.bodies.as_slice()
        {
            let cut = k.boolean(crate::BoolOp::Subtract, target, &[*s]);
            k.release(*s);
            if let Ok(c) = cut
                && c.bodies
                    .iter()
                    .all(|&id| k.get(id).is_ok_and(|b| b.check().is_empty()))
            {
                return Ok(c);
            }
        }
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
            && c.bodies
                .iter()
                .all(|&id| k.get(id).is_ok_and(|b| b.check().is_empty()))
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
    // Else meshed: inside the target, in none of the tool's places.
    let t = (**k.get(target)?).clone();
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
    let size = b.bbox().diagonal().max(1.0);
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
            if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
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
                if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
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
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some()
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
