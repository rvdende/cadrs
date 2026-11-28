//! Constructors made of the kernel's own sweeps and faces: the screw (a profile turned about an
//! axis while rising along it, [`crate::Kernel::screw`]).

use super::math::*;
use super::tol;
use crate::*;

/// The profile's points, every curve sampled finely enough to find its extent (a line by its
/// ends, an arc or a spline at 256 points).
fn profile_points(profile: &Profile) -> Vec<P3> {
    let pl = &profile.plane;
    let y = pl.normal.cross(&pl.x_dir);
    let mut out = Vec::new();
    let curves = profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .flat_map(|l| &l.curves);
    for c in curves {
        let n = if matches!(c, Curve2::Line { .. }) {
            1
        } else {
            256
        };
        for i in 0..=n {
            let q = c.point_at(i as f64 / n as f64);
            out.push(pl.origin + pl.x_dir.into_inner() * q.x + y * q.y);
        }
    }
    out
}

/// The screw of `profile` about `spec.axis` ([`crate::Kernel::screw`]): the profile swept along
/// the helix its middle follows, carried by the helix's screw ([`SweepControl::Helix`]).
pub fn screw(k: &mut super::NativeKernel, profile: &Profile, spec: &ScrewSpec) -> Result<OpResult> {
    let ScrewSpec { axis, rise, angle } = *spec;
    if !(angle > 0.0 && angle.is_finite()) {
        return Err(KernelError::InvalidParameter(format!(
            "The screw's angle must be positive (it is {angle})"
        )));
    }
    if !rise.is_finite() {
        return Err(KernelError::InvalidParameter(format!(
            "The screw's rise must be finite (it is {rise})"
        )));
    }
    if profile.regions.is_empty() {
        return Err(KernelError::InvalidProfile(
            "A screw needs a closed region to turn".into(),
        ));
    }
    let a = axis.dir.into_inner();
    let pl = &profile.plane;
    if a.dot(&pl.normal).abs() > tol::ANGULAR
        || (axis.origin - pl.origin).dot(&pl.normal).abs() > tol::POINT
    {
        return Err(KernelError::InvalidParameter(
            "The screw's axis must lie in the profile's plane".into(),
        ));
    }
    // Out from the axis in the plane, toward the profile.
    let mut out = pl.normal.cross(&a).normalize();
    let pts = profile_points(profile);
    let rho = |p: &P3, out: &V3| (p - axis.origin).dot(out);
    let (lo, hi) = pts
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| {
            let r = rho(p, &out);
            (l.min(r), h.max(r))
        });
    let (lo, hi) = if lo >= -tol::POINT {
        (lo.max(0.0), hi)
    } else if hi <= tol::POINT {
        out = -out;
        ((-hi).max(0.0), -lo)
    } else {
        return Err(KernelError::InvalidProfile(
            "The screw's profile crosses its axis".into(),
        ));
    };
    if hi <= tol::POINT {
        return Err(KernelError::InvalidProfile(
            "The screw's profile lies on its axis".into(),
        ));
    }
    let (wlo, whi) = pts
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| {
            let w = (p - axis.origin).dot(&a);
            (l.min(w), h.max(w))
        });
    let lead = TAU * rise.abs();
    if whi - wlo - lead > tol::POINT {
        return Err(KernelError::InvalidParameter(format!(
            "The screw's turns would collide: a turn rises {lead}, less than the profile's \
             extent {} along the axis",
            whi - wlo
        )));
    }
    // The helix the profile's middle follows, starting in the profile's plane.
    let helix = Helix {
        axis: Axis {
            origin: axis.origin + a * ((wlo + whi) / 2.0),
            dir: axis.dir,
        },
        x: nalgebra::Unit::new_normalize(out),
        r0: (lo + hi) / 2.0,
        r1: (lo + hi) / 2.0,
        height: rise * angle,
        turns: angle / TAU,
        start_angle: 0.0,
    };
    let mut sweep = SweepSpec::solid(helix.path());
    sweep.control = SweepControl::Helix(helix);
    k.sweep_with(profile, &sweep)
}

/// The degree of clamped knots over [0, 1] for `n` coefficients, or why they don't fit.
fn clamped_degree(knots: &[f64], n: usize, which: &str) -> Result<usize> {
    let bad = |why: &str| {
        Err(KernelError::InvalidParameter(format!(
            "The slab's knots along {which} {why}"
        )))
    };
    if n == 0 || knots.len() < n + 2 {
        return bad("are too few for its heights");
    }
    let p = knots.len() - n - 1;
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) {
        return bad("must not decrease");
    }
    if knots[..=p].iter().any(|&k| k != 0.0) || knots[n..].iter().any(|&k| k != 1.0) {
        return bad(&format!(
            "must run {} times 0 first and as many times 1 last",
            p + 1
        ));
    }
    if (p + 1..n).any(|i| knots[i + p] <= knots[i]) {
        return bad("repeat one value more than its degree");
    }
    Ok(p)
}

/// A slab's body ([`crate::Kernel::slab`]), built face by face: the base, the four sides (each a
/// plane bounded below by a line and above by the top's boundary, a B-spline whose p-curve on
/// the side is its own poles read in the side's plane) and the top (the height field's
/// B-spline surface whole, its sides lines in its parameters).
pub fn slab(spec: &SlabSpec) -> Result<super::topo::Body> {
    use super::curve::Curve;
    use super::nurbs::{H, NurbsCurve, NurbsSurface};
    use super::surface::Surface;
    use super::topo::{BodyBuilder, Coedge};
    let [sx, sy] = spec.size;
    if !(sx > 0.0 && sy > 0.0 && sx.is_finite() && sy.is_finite()) {
        return Err(KernelError::InvalidParameter(
            "The slab's sides must be positive".into(),
        ));
    }
    let h = &spec.heights;
    let nu = h.len();
    let nv = h.first().map_or(0, Vec::len);
    if nv == 0 || h.iter().any(|r| r.len() != nv) {
        return Err(KernelError::InvalidParameter(
            "The slab's heights must be rows of one length".into(),
        ));
    }
    if h.iter().flatten().any(|&z| !(z > 0.0 && z.is_finite())) {
        return Err(KernelError::InvalidParameter(
            "The slab's heights must be positive".into(),
        ));
    }
    if spec.x.dot(&spec.y).abs() > tol::ANGULAR {
        return Err(KernelError::InvalidParameter(
            "The slab's sides must be square to each other".into(),
        ));
    }
    let pu = clamped_degree(&spec.knots_u, nu, "u")?;
    let pv = clamped_degree(&spec.knots_v, nv, "v")?;
    // The Greville abscissae: where the poles stand so that x and y run linearly.
    let greville = |k: &[f64], p: usize, i: usize| k[i + 1..=i + p].iter().sum::<f64>() / p as f64;
    let gu: Vec<f64> = (0..nu)
        .map(|i| greville(&spec.knots_u, pu, i) * sx)
        .collect();
    let gv: Vec<f64> = (0..nv)
        .map(|j| greville(&spec.knots_v, pv, j) * sy)
        .collect();
    let top = NurbsSurface {
        screw: None,
        du: pu,
        dv: pv,
        ku: spec.knots_u.clone(),
        kv: spec.knots_v.clone(),
        cps: (0..nu)
            .map(|i| {
                (0..nv)
                    .map(|j| H::new(gu[i], gv[j], h[i][j], 1.0))
                    .collect()
            })
            .collect(),
    };
    // A curve through poles (a, b) in a plane's coordinates (or space's, with c).
    let nurbs = |deg: usize, knots: &[f64], pts: Vec<H>| {
        Curve::nurbs(NurbsCurve {
            degree: deg,
            knots: knots.to_vec(),
            cps: pts,
        })
    };
    let line = |o: [f64; 3], d: [f64; 3]| Curve::Line {
        o: P3::new(o[0], o[1], o[2]),
        d: V3::new(d[0], d[1], d[2]),
    };
    let co = |edge: usize, fwd: bool, pc: Curve| Coedge { edge, fwd, pc };
    let plane = |o: [f64; 3], x: [f64; 3], y: [f64; 3]| {
        let (x, y) = (V3::new(x[0], x[1], x[2]), V3::new(y[0], y[1], y[2]));
        Surface::Plane {
            f: Frame {
                o: P3::new(o[0], o[1], o[2]),
                x,
                y,
                z: x.cross(&y),
            },
        }
    };
    let (h00, h10, h11, h01) = (h[0][0], h[nu - 1][0], h[nu - 1][nv - 1], h[0][nv - 1]);
    let mut bb = BodyBuilder::new();
    let b = [
        bb.vertex(P3::new(0.0, 0.0, 0.0)),
        bb.vertex(P3::new(sx, 0.0, 0.0)),
        bb.vertex(P3::new(sx, sy, 0.0)),
        bb.vertex(P3::new(0.0, sy, 0.0)),
    ];
    let t = [
        bb.vertex(P3::new(0.0, 0.0, h00)),
        bb.vertex(P3::new(sx, 0.0, h10)),
        bb.vertex(P3::new(sx, sy, h11)),
        bb.vertex(P3::new(0.0, sy, h01)),
    ];
    // The base's sides, each the way its coordinate grows.
    let eb_front = bb.edge(line([0.0; 3], [1.0, 0.0, 0.0]), 0.0, sx, b[0], b[1]);
    let eb_back = bb.edge(line([0.0, sy, 0.0], [1.0, 0.0, 0.0]), 0.0, sx, b[3], b[2]);
    let eb_left = bb.edge(line([0.0; 3], [0.0, 1.0, 0.0]), 0.0, sy, b[0], b[3]);
    let eb_right = bb.edge(line([sx, 0.0, 0.0], [0.0, 1.0, 0.0]), 0.0, sy, b[1], b[2]);
    // The uprights at the corners.
    let up = |x: f64, y: f64| line([x, y, 0.0], [0.0, 0.0, 1.0]);
    let v0 = bb.edge(up(0.0, 0.0), 0.0, h00, b[0], t[0]);
    let v1 = bb.edge(up(sx, 0.0), 0.0, h10, b[1], t[1]);
    let v2 = bb.edge(up(sx, sy), 0.0, h11, b[2], t[2]);
    let v3 = bb.edge(up(0.0, sy), 0.0, h01, b[3], t[3]);
    // The top's sides: its boundary curves, in its parameters.
    let row_v = |j: usize| -> Vec<H> { (0..nu).map(|i| top.cps[i][j]).collect() };
    let row_u = |i: usize| -> Vec<H> { top.cps[i].clone() };
    let (ku, kv) = (&spec.knots_u, &spec.knots_v);
    let et_front = bb.edge(nurbs(pu, ku, row_v(0)), 0.0, 1.0, t[0], t[1]);
    let et_back = bb.edge(nurbs(pu, ku, row_v(nv - 1)), 0.0, 1.0, t[3], t[2]);
    let et_left = bb.edge(nurbs(pv, kv, row_u(0)), 0.0, 1.0, t[0], t[3]);
    let et_right = bb.edge(nurbs(pv, kv, row_u(nu - 1)), 0.0, 1.0, t[1], t[2]);
    // A boundary curve's p-curve on a side: its poles' (along, up) in the side's plane.
    let side_pc = |deg: usize, knots: &[f64], pts: Vec<H>, along: usize| {
        nurbs(
            deg,
            knots,
            pts.iter()
                .map(|q| H::new(q[along], q.z, 0.0, 1.0))
                .collect(),
        )
    };
    let l2 = |o: [f64; 2], d: [f64; 2]| line([o[0], o[1], 0.0], [d[0], d[1], 0.0]);
    let top_f = bb.face(
        Surface::Nurbs(std::sync::Arc::new(top.clone())),
        true,
        vec![vec![
            co(et_front, true, l2([0.0, 0.0], [1.0, 0.0])),
            co(et_right, true, l2([1.0, 0.0], [0.0, 1.0])),
            co(et_back, false, l2([0.0, 1.0], [1.0, 0.0])),
            co(et_left, false, l2([0.0, 0.0], [0.0, 1.0])),
        ]],
    );
    let bottom = bb.face(
        plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        false,
        vec![vec![
            co(eb_left, true, l2([0.0, 0.0], [0.0, 1.0])),
            co(eb_back, true, l2([0.0, sy], [1.0, 0.0])),
            co(eb_right, false, l2([sx, 0.0], [0.0, 1.0])),
            co(eb_front, false, l2([0.0, 0.0], [1.0, 0.0])),
        ]],
    );
    let (xz, yz) = (
        ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    );
    let front = bb.face(
        plane([0.0; 3], xz.0, xz.1),
        true,
        vec![vec![
            co(eb_front, true, l2([0.0, 0.0], [1.0, 0.0])),
            co(v1, true, l2([sx, 0.0], [0.0, 1.0])),
            co(et_front, false, side_pc(pu, ku, row_v(0), 0)),
            co(v0, false, l2([0.0, 0.0], [0.0, 1.0])),
        ]],
    );
    let back = bb.face(
        plane([0.0, sy, 0.0], xz.0, xz.1),
        false,
        vec![vec![
            co(eb_back, false, l2([0.0, 0.0], [1.0, 0.0])),
            co(v3, true, l2([0.0, 0.0], [0.0, 1.0])),
            co(et_back, true, side_pc(pu, ku, row_v(nv - 1), 0)),
            co(v2, false, l2([sx, 0.0], [0.0, 1.0])),
        ]],
    );
    let left = bb.face(
        plane([0.0; 3], yz.0, yz.1),
        false,
        vec![vec![
            co(eb_left, false, l2([0.0, 0.0], [1.0, 0.0])),
            co(v0, true, l2([0.0, 0.0], [0.0, 1.0])),
            co(et_left, true, side_pc(pv, kv, row_u(0), 1)),
            co(v3, false, l2([sy, 0.0], [0.0, 1.0])),
        ]],
    );
    let right = bb.face(
        plane([sx, 0.0, 0.0], yz.0, yz.1),
        true,
        vec![vec![
            co(eb_right, true, l2([0.0, 0.0], [1.0, 0.0])),
            co(v2, true, l2([sy, 0.0], [0.0, 1.0])),
            co(et_right, false, side_pc(pv, kv, row_u(nu - 1), 1)),
            co(v1, false, l2([0.0, 0.0], [0.0, 1.0])),
        ]],
    );
    let shell = bb.shell(vec![bottom, front, right, back, left, top_f], true);
    bb.solid(vec![shell]);
    let body = bb.build()?;
    // Placed on the frame.
    let (x, y) = (spec.x.into_inner(), spec.y.into_inner());
    let m = nalgebra::Matrix3::from_columns(&[x, y, x.cross(&y)]);
    Ok(body.transformed(&Affine {
        m,
        t: spec.origin.coords,
    }))
}
