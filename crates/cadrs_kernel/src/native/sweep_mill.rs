//! What a bull-nose mill fills under a rigid motion, exactly (a leaning mill carried along a
//! spline).
//!
//! The mill is a solid of revolution with a flat end, its corner rounded, a cylinder's side and
//! a flat top with a sharp rim: convex, and smooth but for that rim. Where neither flat end
//! touches what it sweeps (the end leaning forward and trailing all over, the top leading all
//! over: a mill carried along a path gentler than its own radius), it touches along one closed
//! curve at each instant: up the cylinder where its axis's point moves square to its normal,
//! round the trailing half of the top rim, down the cylinder's other side, and round the
//! leading half of the corner. Those four stretches sweep the four sheets of its envelope;
//! the mill's own faces where the motion starts (what trails) and stops (what leads) close
//! them. The sheets are fitted, the mill's faces are as they are, they are joined along the
//! contact curves at the two ends, and what is boundary is told by the mill's own distance
//! over the motion, as [`super::sweep_tube`] tells a torus's.

use super::kernel::NativeKernel;
use super::math::*;
use super::surface::Surface;
use super::sweep_pipe::fit_patch;
use super::sweep_tube::Motion;
use super::topo::Body;
use crate::*;
use std::f64::consts::{PI, TAU};

/// A bull-nose mill in its own frame: its flat end's centre, its axis from there, the end's
/// radius, the corner's, and its height.
struct Mill {
    c: P3,
    a: V3,
    r1: f64,
    rc: f64,
    h: f64,
}

impl Mill {
    /// The mill a body is, if it is one: a plane, a torus's quarter, a cylinder and a plane
    /// about one axis, with the indices of its end's face and its top's.
    fn of(b: &Body) -> Option<(Mill, usize, usize)> {
        if b.faces.len() != 4 || b.solids.len() != 1 {
            return None;
        }
        let torus = b.faces.iter().find_map(|f| match &f.surf {
            Surface::Torus { f, rmaj, rmin } => Some((f.o, f.z, *rmaj, *rmin)),
            _ => None,
        })?;
        let cyl = b.faces.iter().find_map(|f| match &f.surf {
            Surface::Cylinder { f, r } => Some((f.o, f.z, *r)),
            _ => None,
        })?;
        let planes: Vec<(usize, P3, V3)> = b
            .faces
            .iter()
            .enumerate()
            .filter_map(|(i, f)| match &f.surf {
                Surface::Plane { f } => Some((i, f.o, f.z)),
                _ => None,
            })
            .collect();
        let [p, q] = planes.as_slice() else {
            return None;
        };
        let (o, z, r1, rc) = torus;
        let near = 1e-9 * (1.0 + r1);
        let along = |x: &P3| (x - o).dot(&z);
        let off = |x: &P3| ((x - o) - z * along(x)).norm();
        if !(rc < r1)
            || (cyl.2 - (r1 + rc)).abs() > near
            || cyl.1.cross(&z).norm() > 1e-9
            || off(&cyl.0) > near
            || p.2.cross(&z).norm() > 1e-9
            || q.2.cross(&z).norm() > 1e-9
        {
            return None;
        }
        // (The end is the plane the corner's tube touches; the top the other.)
        let (hp, hq) = (along(&p.1), along(&q.1));
        let (end, top, he, ht) = if (hp.abs() - rc).abs() <= near {
            (p.0, q.0, hp, hq)
        } else if (hq.abs() - rc).abs() <= near {
            (q.0, p.0, hq, hp)
        } else {
            return None;
        };
        let a = if ht > he { z } else { -z };
        let h = (ht - he).abs();
        if !(h > rc + near) {
            return None;
        }
        Some((
            Mill {
                c: o + z * he,
                a,
                r1,
                rc,
                h,
            },
            end,
            top,
        ))
    }

    /// How far a point of the mill's own frame lies outside it (negative inside; exact on it
    /// and inside, never more than the distance outside).
    fn outside(&self, q: &P3) -> f64 {
        let w = q - self.c;
        let h = w.dot(&self.a);
        let rho = (w - self.a * h).norm();
        let (dx, dy) = (rho - self.r1, self.rc - h);
        let round = if dx <= 0.0 && dy <= 0.0 {
            dx.max(dy)
        } else {
            dx.max(0.0).hypot(dy.max(0.0))
        } - self.rc;
        round.max(h - self.h)
    }
}

/// Whether a body is a bull-nose mill ([`mill`] takes it).
pub(crate) fn is_mill(b: &Body) -> bool {
    Mill::of(b).is_some()
}

/// The mill and its motion, with the samples its inside test reads.
struct Swept<'a> {
    mill: Mill,
    motion: &'a Motion<'a>,
    size: f64,
    ts: Vec<f64>,
    places: Vec<(nalgebra::Matrix3<f64>, V3)>,
}

/// The four sheets: the cylinder's two sides (h up it), the top rim's trailing half and the
/// corner's leading half (each by an angle from the first side round to the second).
#[derive(Clone, Copy, PartialEq)]
enum Sheet {
    Side(f64),
    Rim,
    Corner,
}

impl Swept<'_> {
    /// A point of the mill's frame, its velocity in that frame at t.
    fn speed(&self, j: &super::sweep_tube::Jet, q: &P3) -> V3 {
        j.r[0].tr_mul(&(j.r[1] * q.coords + j.d[1]))
    }

    /// At t, by the axis's point h up it: that point, the unit way it moves across the axis,
    /// and the normal square to both (the first side's).
    fn across(&self, j: &super::sweep_tube::Jet, h: f64) -> Option<(P3, V3, V3)> {
        let m = &self.mill;
        let p = m.c + m.a * h;
        let w = self.speed(j, &p);
        let w =
            (w - m.a * w.dot(&m.a)).try_normalize(1e-9 * self.speed(j, &p).norm().max(1e-300))?;
        Some((p, w, m.a.cross(&w)))
    }

    /// A sheet's point in the mill's frame at (t, s), with the mill's normal there.
    fn body_point(&self, sheet: Sheet, t: f64, s: f64) -> Option<(P3, V3)> {
        let m = &self.mill;
        let j = self.motion.jet(t);
        match sheet {
            Sheet::Side(side) => {
                let (p, _, n) = self.across(&j, s)?;
                Some((p + n * (side * (m.r1 + m.rc)), n * side))
            }
            Sheet::Rim => {
                let (p, w, n) = self.across(&j, m.h)?;
                let out = n * s.cos() - w * s.sin();
                // (Between the side's normal and the top's: the rim's own.)
                Some((p + out * (m.r1 + m.rc), (out + m.a).normalize()))
            }
            Sheet::Corner => {
                let (p, w, n) = self.across(&j, m.rc)?;
                let e1 = n * s.cos() + w * s.sin();
                let k = p + e1 * m.r1;
                let wk = self.speed(&j, &k);
                let (wa, we) = (wk.dot(&m.a), wk.dot(&e1));
                // (The tube's normal square to the core's velocity, on the quarter that is
                // the corner: outward and toward the end.)
                if !(wa > 0.0) {
                    return None;
                }
                let out = (e1 * wa - m.a * we) / wa.hypot(we);
                Some((k + out * m.rc, out))
            }
        }
    }

    fn place(&self, t: f64, q: &P3) -> P3 {
        let (r, d) = self.motion.place(t);
        P3::from(r * q.coords + d)
    }

    /// A sheet's point at (t, s).
    fn point(&self, sheet: Sheet, t: f64, s: f64) -> P3 {
        let q = self.body_point(sheet, t, s).map_or(self.mill.c, |q| q.0);
        self.place(t, &q)
    }

    fn at_instant(&self, pl: &(nalgebra::Matrix3<f64>, V3), x: &P3) -> f64 {
        self.mill
            .outside(&P3::from(pl.0.tr_mul(&(x.coords - pl.1))))
    }

    /// The least over the motion of how far `x` lies outside the mill (negative: inside what
    /// it sweeps).
    fn least(&self, x: &P3) -> f64 {
        let vals: Vec<f64> = self
            .places
            .iter()
            .map(|pl| self.at_instant(pl, x))
            .collect();
        let n = vals.len();
        let mut best = f64::INFINITY;
        for i in 0..n {
            if (i > 0 && vals[i] > vals[i - 1]) || (i + 1 < n && vals[i] > vals[i + 1]) {
                continue;
            }
            if vals[i] > self.size {
                continue;
            }
            // (Golden section between the samples either side.)
            let g = 0.5 * (5f64.sqrt() - 1.0);
            let f = |t: f64| self.at_instant(&self.motion.place(t), x);
            let (mut a, mut b) = (self.ts[i.saturating_sub(1)], self.ts[(i + 1).min(n - 1)]);
            let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
            let (mut fc, mut fd) = (f(c), f(d));
            for _ in 0..48 {
                if b - a <= 1e-9 {
                    break;
                }
                if fc < fd {
                    (b, d, fd) = (d, c, fc);
                    c = b - g * (b - a);
                    fc = f(c);
                } else {
                    (a, c, fc) = (c, d, fd);
                    d = a + g * (b - a);
                    fd = f(d);
                }
            }
            best = best.min(fc.min(fd)).min(vals[i]);
        }
        best
    }
}

/// [`crate::Kernel::sweep_solid`] of a bull-nose mill by a rigid motion: None when `b` isn't
/// one, or its flat ends touch what it sweeps (a motion this doesn't take yet); an error when
/// its boundary isn't made out.
pub(crate) fn mill(k: &mut NativeKernel, b: &Body, motion: &Motion) -> Option<Result<OpResult>> {
    use rayon::prelude::*;
    let (mill, end_face, top_face) = Mill::of(b)?;
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    let t_start = web_time::Instant::now();
    let refused = |why: String| -> Option<Result<OpResult>> {
        if debug {
            eprintln!("mill sweep: refused: {why}");
        }
        Some(Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: format!("the swept mill: {why}"),
        }))
    };
    let size = b.bbox().diagonal().max(super::tol::POINT);
    let n = 2048;
    let ts: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
    let places = ts.iter().map(|&t| motion.place(t)).collect();
    let (rad, rc, r1, height) = (mill.r1 + mill.rc, mill.rc, mill.r1, mill.h);
    let (c, a) = (mill.c, mill.a);
    let sw = Swept {
        mill,
        motion,
        size,
        ts,
        places,
    };
    // The contact as this takes it, all the motion through: the axis's points moving across
    // it, the end trailing and the top leading all over, the corner's contact on its quarter.
    for i in 0..=256 {
        let t = i as f64 / 256.0;
        let j = motion.jet(t);
        for h in [rc, 0.5 * (rc + height), height] {
            sw.across(&j, h)?;
        }
        let (_, w, nn) = sw.across(&j, rc)?;
        for q in 0..32 {
            let ang = TAU * q as f64 / 32.0;
            let e = nn * ang.cos() + w * ang.sin();
            if sw.speed(&j, &(c + e * r1)).dot(&a) <= 0.0
                || sw.speed(&j, &(c + a * height + e * rad)).dot(&a) <= 0.0
            {
                return None;
            }
        }
    }
    let delta = 1e-11 * size;
    let fit_tol = 1e-7 * size;
    // The sheets: whole on the boundary (a mill that sweeps back over its own sheets is not
    // taken yet).
    let sheets = [
        (Sheet::Side(1.0), (rc, height)),
        (Sheet::Rim, (0.0, PI)),
        (Sheet::Side(-1.0), (rc, height)),
        (Sheet::Corner, (0.0, PI)),
    ];
    let clear = sheets.par_iter().all(|&(sheet, (s0, s1))| {
        (0..=96).all(|i| {
            (0..=8).all(|jj| {
                let x = sw.point(sheet, i as f64 / 96.0, s0 + (s1 - s0) * jj as f64 / 8.0);
                sw.least(&x) >= -1e-9 * size
            })
        })
    });
    if !clear {
        return refused("it sweeps back over what it has swept".into());
    }
    super::par::start();
    let fitted: Vec<Option<super::nurbs::NurbsSurface>> = sheets
        .par_iter()
        .map(|&(sheet, (s0, s1))| {
            let at = |t: f64, s: f64| sw.point(sheet, t, s);
            fit_patch(&at, ((0.0, 1.0), (s0, s1)), (16, 4), fit_tol)
        })
        .collect();
    let mut faces: Vec<Body> = Vec::new();
    for ((sheet, (s0, s1)), fit) in sheets.iter().zip(fitted) {
        let Some(fit) = fit else {
            return refused("a sheet of its envelope won't fit".into());
        };
        let surf = Surface::Nurbs(std::sync::Arc::new(fit));
        let mut f =
            match super::face_build::face_from_surface(&surf, Some(((0.0, 1.0), (*s0, *s1)))) {
                Ok(f) => f,
                Err(e) => return Some(Err(e)),
            };
        // (Facing as the mill does there.)
        let (t, s) = (0.5, 0.5 * (s0 + s1));
        let Some((_, out)) = sw.body_point(*sheet, t, s) else {
            return refused("a sheet of its envelope isn't there midway".into());
        };
        let out = motion.place(t).0 * out;
        let face = &mut f.faces[0];
        let (_, su, sv) = face.surf.d1(t, s);
        if (su.cross(&sv) * if face.sense { 1.0 } else { -1.0 }).dot(&out) < 0.0 {
            Body::reverse_face(face);
        }
        faces.push(f);
    }
    // The mill's own faces at each end of the motion, in quarters round its axis begun an
    // eighth of a turn before the first side's contact (the contacts fall in quarters'
    // middles), each cut along the contact there, so that the sheets' edges are its pieces'
    // edges; the pieces the motion covers are judged so.
    let mut uv_point: Vec<Box<dyn Fn(P2) -> P3 + Sync + '_>> = Vec::new();
    for &(sheet, _) in sheets.iter() {
        let sw = &sw;
        uv_point.push(Box::new(move |w: P2| sw.point(sheet, w.x, w.y)));
    }
    for (pose, t) in [(0usize, 0.0), (1, 1.0)] {
        let j = motion.jet(t);
        let (_, w, nn) = sw.across(&j, rc)?;
        let (rot, shift) = motion.place(t);
        let to_world = |q: &P3| P3::from(rot * q.coords + shift);
        let frame = |o: P3| Frame {
            o: to_world(&o),
            x: rot * nn,
            y: rot * a.cross(&nn),
            z: rot * a,
        };
        let cyl = Surface::Cylinder {
            f: frame(c),
            r: rad,
        };
        let tor = Surface::Torus {
            f: frame(c + a * rc),
            rmaj: r1,
            rmin: rc,
        };
        // The contact's stretches, each with the quarter it crosses: (on the torus, the
        // quarter, its points in the quarter's (u, v)).
        let mut joins: Vec<(bool, usize, Vec<P2>)> = Vec::new();
        let angle = |v: &V3| v.dot(&a.cross(&nn)).atan2(v.dot(&nn));
        for (side, quarter, about) in [(1.0, 0usize, 0.0), (-1.0, 2, PI)] {
            let mut pts = Vec::new();
            for i in 0..=48 {
                let h = rc + (height - rc) * i as f64 / 48.0;
                let (_, out) = sw.body_point(Sheet::Side(side), t, h)?;
                let u = about + wrap_pi(angle(&out) - about);
                if (u - about).abs() > 0.2 * PI {
                    return None;
                }
                pts.push(P2::new(u, h));
            }
            joins.push((false, quarter, pts));
        }
        for (b0, b1, quarter, turn) in [
            (0.0, 0.25 * PI, 0usize, 0.0),
            (0.25 * PI, 0.75 * PI, 3, TAU),
            (0.75 * PI, PI, 2, TAU),
        ] {
            let mut pts = Vec::new();
            for i in 0..=32 {
                let beta = b0 + (b1 - b0) * i as f64 / 32.0;
                let (_, out) = sw.body_point(Sheet::Corner, t, beta)?;
                let e1 = nn * beta.cos() + w * beta.sin();
                let theta = out.dot(&a).atan2(out.dot(&e1));
                pts.push(P2::new(turn - beta, theta.clamp(-0.5 * PI, 0.0)));
            }
            joins.push((true, quarter, pts));
        }
        for (on_torus, surf, v) in [(false, &cyl, (rc, height)), (true, &tor, (-0.5 * PI, 0.0))] {
            for q in 0..4 {
                let u0 = -0.25 * PI + 0.5 * PI * q as f64;
                let mut f = match super::face_build::face_from_surface(
                    surf,
                    Some(((u0, u0 + 0.5 * PI), v)),
                ) {
                    Ok(f) => f,
                    Err(e) => return Some(Err(e)),
                };
                let (um, vm) = (u0 + 0.25 * PI, 0.5 * (v.0 + v.1));
                let face = &mut f.faces[0];
                let (p, su, sv) = face.surf.d1(um, vm);
                // (Away from the axis, or from the corner's core.)
                let from = match surf {
                    Surface::Cylinder { f, .. } => f.o + f.z * (p - f.o).dot(&f.z),
                    Surface::Torus { f, rmaj, .. } => {
                        f.o + (f.x * um.cos() + f.y * um.sin()) * *rmaj
                    }
                    _ => p,
                };
                if (su.cross(&sv) * if face.sense { 1.0 } else { -1.0 }).dot(&(p - from)) < 0.0 {
                    Body::reverse_face(face);
                }
                let mut cuts = Vec::new();
                let mut pos: std::collections::HashMap<usize, P3> = Default::default();
                let mut next_id = f.verts.len();
                for (_, _, uvs) in joins.iter().filter(|j| j.0 == on_torus && j.1 == q) {
                    let run: Vec<(P3, P2)> =
                        uvs.iter().map(|w| (surf.eval(w.x, w.y), *w)).collect();
                    let mut params = vec![0.0];
                    for w in run.windows(2) {
                        params.push(params.last().unwrap() + (w[1].0 - w[0].0).norm());
                    }
                    let p3: Vec<P3> = run.iter().map(|p| p.0).collect();
                    let curve = super::nurbs::NurbsCurve::interpolate(&p3, &params, 5, None, None);
                    let v = [next_id, next_id + 1];
                    next_id += 2;
                    pos.insert(v[0], run[0].0);
                    pos.insert(v[1], run[run.len() - 1].0);
                    cuts.push(super::sweep_moving::FaceCut {
                        c: super::curve::Curve::Nurbs(std::sync::Arc::new(curve)),
                        pts: run,
                        v,
                        params: None,
                    });
                }
                let pieces = if cuts.is_empty() {
                    vec![f]
                } else {
                    match super::sweep_moving::cut_face(&f, 0, cuts, &pos, size, 1e-7 * size, None)
                    {
                        Ok(ps) => ps,
                        Err(why) => {
                            return refused(format!("its own faces along the contact: {why}"));
                        }
                    }
                };
                for piece in pieces {
                    faces.push(piece);
                    let s = surf.clone();
                    uv_point.push(Box::new(move |w: P2| s.eval(w.x, w.y)));
                }
            }
        }
        // (The flat end where the motion starts, the top where it stops: as the mill has it.)
        let moved = b.transformed(&Affine { m: rot, t: shift });
        let flat = if pose == 0 { end_face } else { top_face };
        let disc = super::kernel::subset(&moved, &[flat], false);
        let dsurf = disc.faces[0].surf.clone();
        faces.push(disc);
        uv_point.push(Box::new(move |w: P2| dsurf.eval(w.x, w.y)));
    }
    let given: Vec<super::sweep_moving::Given> = Vec::new();
    if debug {
        eprintln!(
            "mill sweep: {} faces made by {:.0} ms",
            faces.len(),
            t_start.elapsed().as_secs_f64() * 1e3
        );
    }
    let member = |i: usize, uv: P2| sw.least(&uv_point[i](uv)) < -delta;
    let still = |_: f64| (nalgebra::Rotation3::identity(), V3::zeros());
    let oracle = super::sweep_moving::Oracle::with_measure(Box::new(|_: &V3| 1.0), &still, 2);
    let on_mill = |s: &Surface| matches!(s, Surface::Cylinder { .. } | Surface::Torus { .. });
    let solid = match super::sweep_moving::boundary_of_some(
        faces,
        &oracle,
        size,
        0.0,
        f64::INFINITY,
        given,
        Some(&member),
        fit_tol,
        true,
        Some(&on_mill),
    ) {
        Ok(s) => s,
        Err(e) => {
            if debug {
                eprintln!("mill sweep: refused: {e}");
            }
            return Some(Err(e));
        }
    };
    if debug {
        eprintln!(
            "mill sweep: {} faces by {:.0} ms",
            solid.faces.len(),
            t_start.elapsed().as_secs_f64() * 1e3
        );
    }
    let id = k.insert(solid);
    Some(Ok(OpResult {
        bodies: vec![id],
        history: History::default(),
    }))
}

/// An angle brought within half a turn of nothing.
fn wrap_pi(a: f64) -> f64 {
    (a + PI).rem_euclid(TAU) - PI
}
