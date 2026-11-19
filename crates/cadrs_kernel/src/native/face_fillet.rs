//! A fillet between two faces of a solid that need not share an edge ([`Kernel::fillet_faces`]):
//! the ball of the radius rolls touching both faces, and what lies outside its path between its
//! two contact lines goes, any faces between them with it (a chamfer narrower than the round is
//! consumed, leaving the round of the sharp edge).
//!
//! Between two planes the ball's path is exact: the planes meet in a line (the virtual edge), and
//! square to it the section is the corner, the two contacts `r·cot(θ/2)` from it along the faces
//! and the arc between them (the circular fillet's [`section_loop`]). Swept along that line past
//! the body's extent it is the tool the body loses.
//!
//! [`Kernel::fillet_faces`]: crate::Kernel::fillet_faces

use super::blend::{Corner2, FaceSrc, SectionKind, Tracked, section_loop};
use super::kernel::NativeKernel;
use super::math::*;
use super::profile::{MergedFace, Src};
use super::surface::Surface;
use super::sweep::{self, Prism};
use super::topo::Body;
use crate::*;

fn invalid(why: impl Into<String>) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

/// The plane of face `f` as a point on it and its outward unit normal, if it is flat.
fn flat(b: &Body, f: usize) -> Option<(P3, V3)> {
    let face = &b.faces[f];
    let Surface::Plane { f: fr } = &face.surf else {
        return None;
    };
    let n = face.surf.normal(0.0, 0.0).normalize() * if face.sense { 1.0 } else { -1.0 };
    Some((fr.o, n))
}

/// Points along the boundary of face `f`: its edges' ends and a few points between.
fn boundary(b: &Body, f: usize) -> Vec<P3> {
    let mut out = Vec::new();
    for c in b.faces[f].loops.iter().flatten() {
        let e = &b.edges[c.edge];
        if e.degenerate {
            continue;
        }
        for k in 0..=8 {
            out.push(e.point(e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 8.0));
        }
    }
    out
}

/// The fillet of radius `r` between the faces `fa` and `fb` (see the module docs).
pub fn fillet_faces(
    k: &mut NativeKernel,
    body: BodyId,
    fa: FaceId,
    fb: FaceId,
    r: f64,
) -> Result<OpResult> {
    if !(r.is_finite() && r > 0.0) {
        return Err(invalid("The fillet's radius must be positive"));
    }
    let b = std::sync::Arc::clone(k.get(body)?);
    let (ia, ib) = (fa.0 as usize, fb.0 as usize);
    if ia >= b.faces.len() || ib >= b.faces.len() {
        return Err(invalid("A face to fillet between isn't the body's"));
    }
    if ia == ib {
        return Err(invalid("A fillet between faces needs two different faces"));
    }
    let (Some((oa, na)), Some((ob, nb))) = (flat(&b, ia), flat(&b, ib)) else {
        return Err(KernelError::Unsupported(
            "fillets between faces other than planes",
        ));
    };
    // The line the planes meet in.
    let t = na.cross(&nb);
    if t.norm() < 1e-9 {
        return Err(invalid(
            "The faces are parallel: no ball rolls touching both",
        ));
    }
    let t = t.normalize();
    // Its point nearest the planes' points: n·x = n·o for both planes, and along t from oa.
    let m = nalgebra::Matrix3::from_rows(&[na.transpose(), nb.transpose(), t.transpose()]);
    let rhs = V3::new(na.dot(&oa.coords), nb.dot(&ob.coords), t.dot(&oa.coords));
    let p = P3::from(
        m.try_inverse()
            .ok_or_else(|| invalid("The faces' planes don't meet"))?
            * rhs,
    );
    // Into each face, square to the line: where the other face's material lies. Both faces must
    // run that way from the line (the solid is convex across the virtual edge).
    let u1 = (-nb + na * na.dot(&nb)).normalize();
    let u2 = (-na + nb * nb.dot(&na)).normalize();
    let tol = 1e-7 * (1.0 + b.bbox().diagonal());
    let (pa, pb) = (boundary(&b, ia), boundary(&b, ib));
    let reach = |pts: &[P3], u: &V3| {
        pts.iter()
            .map(|q| (q - p).dot(u))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            })
    };
    let ((lo_a, hi_a), (lo_b, hi_b)) = (reach(&pa, &u1), reach(&pb, &u2));
    if lo_a < -tol || lo_b < -tol {
        return Err(KernelError::Unsupported(
            "fillets between faces that meet in a concave corner, or run across where their planes meet",
        ));
    }
    let theta = u1.dot(&u2).clamp(-1.0, 1.0).acos();
    if !(theta > 1e-9 && theta < PI - 1e-9) {
        return Err(invalid("The faces are tangent: nothing to round"));
    }
    // The contacts' distance from the line: the ball must touch each face on the face.
    let d = r / (theta / 2.0).tan();
    if d > hi_a + tol || d > hi_b + tol {
        return Err(KernelError::TooLarge {
            op: "fillet",
            why: format!("a ball of radius {r} touching both planes misses the faces"),
        });
    }
    // The section in the plane square to the line, the line at its origin.
    let y = t.cross(&u1);
    let to2 = |v: &V3| V2::new(v.dot(&u1), v.dot(&y));
    let corner = Corner2 {
        u1: to2(&u1).normalize(),
        u2: to2(&u2).normalize(),
        convex: true,
    };
    let src = Src::Curve {
        region: fa.0,
        curve: fb.0,
    };
    let (lp, _, _) = section_loop(&corner, SectionKind::Circle { r }, src)?;
    // Swept along the line over all the body spans, and a little more.
    let bb = b.bbox();
    let along: Vec<f64> = (0..8)
        .map(|i| {
            let c = P3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            (c - p).dot(&t)
        })
        .collect();
    let margin = 1.0 + 0.1 * bb.diagonal();
    let s0 = along.iter().copied().fold(f64::INFINITY, f64::min) - margin;
    let s1 = along.iter().copied().fold(f64::NEG_INFINITY, f64::max) + margin;
    let plane = Plane {
        origin: p,
        x_dir: nalgebra::Unit::new_normalize(u1),
        normal: nalgebra::Unit::new_normalize(t),
    };
    let mf = MergedFace {
        outer: lp,
        holes: vec![],
        regions: vec![u64::MAX],
    };
    let built = sweep::sweep_full(&[mf], &[], &Prism { plane, dir: t }, s0, s1, false, true)?;
    let mut tool_src = vec![Vec::new(); built.body.faces.len()];
    for (f, o) in &built.origins {
        if *o == src.origin() {
            tool_src[*f].push(FaceSrc::New(*o));
        }
    }
    let solids = b.solids.len();
    let out = Tracked::of((*b).clone())
        .apply(&built.body, &tool_src, BoolOp::Subtract)?
        .with_solids_of_input(solids)?;
    Ok(out.into_result(k, body))
}
