//! Set-back fillet corners ([`Kernel::fillet_setback`]): where three filleted edges meet at a
//! square corner of three planes, each fillet stops `s` (the setback) short of the corner, on its
//! section square to its edge, and one smooth patch closes the corner.
//!
//! After springrep's `blend::fillet_setback` (MIT). In the corner's own axes, the depths
//! `u, v, w` in from its faces, a round's ball has its centre on the lines `r` in from two faces.
//! Set back, the centres near the corner lie on the sphere about `(s, s, s)` through each line's
//! point `s` along it, `√2·(s − r)` in radius, which meets each line there tangentially; where the
//! sphere passes beneath a face the centres run along its circle `r` above that face. The patch
//! the ball sweeps is that sphere grown by `r`, cut from the corner's wedge, its rims on the
//! faces rounded at `r`: a sphere and three tori, exact, tangent to each other, to the faces and,
//! on the cube of side `s` at the corner, to the fillets' end sections. Within that cube the
//! rounded wedge is the solid; outside it the filleted body.
//!
//! [`Kernel::fillet_setback`]: crate::Kernel::fillet_setback

use super::blend::{Corner2, FaceSrc, SectionKind, Tracked, section_loop};
use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::profile::{MergedFace, Src};
use super::surface::Surface;
use super::sweep::{self, Revolve};
use super::topo::Body;
use crate::*;

fn unsupported() -> KernelError {
    KernelError::Unsupported(
        "set-back corners other than three straight edges meeting square between planes",
    )
}

/// The cube from `o` along the right-handed unit directions `e`, `size` on a side.
fn cube(o: P3, e: [V3; 3], size: f64) -> Result<Body> {
    let at = |i: f64, j: f64, k: f64| o + (e[0] * i + e[1] * j + e[2] * k) * size;
    super::blend::hexahedron(&[
        at(0.0, 0.0, 0.0),
        at(1.0, 0.0, 0.0),
        at(1.0, 1.0, 0.0),
        at(0.0, 1.0, 0.0),
        at(0.0, 0.0, 1.0),
        at(1.0, 0.0, 1.0),
        at(1.0, 1.0, 1.0),
        at(0.0, 1.0, 1.0),
    ])
}

/// A corner to set back: its vertex and its edges' unit directions away from it (right-handed).
struct Corner {
    v: usize,
    e: [V3; 3],
}

/// The corners of `b` where three of `all` meet, each checked to be square between planes.
fn corners(b: &Body, all: &[usize]) -> Result<Vec<Corner>> {
    let mut out = Vec::new();
    for v in 0..b.verts.len() {
        let at: Vec<usize> = all
            .iter()
            .copied()
            .filter(|&e| b.edges[e].v.contains(&v) && b.edges[e].v[0] != b.edges[e].v[1])
            .collect();
        if at.len() < 3 {
            continue;
        }
        let incident = (0..b.edges.len())
            .filter(|&e| b.edges[e].v.contains(&v))
            .count();
        if at.len() != 3 || incident != 3 {
            return Err(unsupported());
        }
        let p = b.verts[v].p;
        let mut e = [V3::zeros(); 3];
        for (i, &x) in at.iter().enumerate() {
            let ed = &b.edges[x];
            if !matches!(ed.curve, Curve::Line { .. }) {
                return Err(unsupported());
            }
            let other = if ed.v[0] == v { ed.v[1] } else { ed.v[0] };
            e[i] = (b.verts[other].p - p).normalize();
        }
        if (0..3).any(|i| e[i].dot(&e[(i + 1) % 3]).abs() > 1e-9) {
            return Err(unsupported());
        }
        let flat = b.faces.iter().all(|f| {
            !f.loops
                .iter()
                .flatten()
                .any(|c| b.edges[c.edge].v.contains(&v))
                || matches!(f.surf, Surface::Plane { .. })
        });
        if !flat {
            return Err(unsupported());
        }
        if e[0].cross(&e[1]).dot(&e[2]) < 0.0 {
            e.swap(0, 1);
        }
        out.push(Corner { v, e });
    }
    Ok(out)
}

/// The corner's patch solid: the wedge on `vp` along `e` cut by the sphere about the point `s`
/// in from all three faces, of radius `R = √2·(s − r) + r`, its three rims on the faces rounded
/// at `r`.
///
/// Each rim is a circle on its face about the foot of the sphere's centre, and its round is the
/// torus about the axis through that centre square to the face: in the half-plane through the
/// axis (`ρ` out from it, `h` in from the face, the centre at `(0, s)`) the round is the circle of
/// radius `r` about `(√((R − r)² − (s − r)²), r)`, tangent to the face and to the sphere. What it
/// takes off is the section of a circular fillet between the face's line and the sphere's
/// tangent line at its contact, turned about the axis.
fn patch(vp: P3, e: [V3; 3], r: f64, s: f64) -> Result<Body> {
    let diag = e[0] + e[1] + e[2];
    let centre = vp + diag * s;
    let big = std::f64::consts::SQRT_2 * (s - r) + r;
    // Poles across the corner's diagonal and the seam on its far side: both off the rims.
    let ball = super::blend::ball_frame(centre, big, diag.normalize(), (e[0] - e[1]).normalize())?;
    let mut out =
        super::boolean::boolean_bodies(&ball, &cube(vp, e, s + big + 1.0)?, BoolOp::Intersect)?
            .body;
    let rho0 = ((big - r).powi(2) - (s - r).powi(2)).sqrt();
    let c2 = V2::new(0.0, s);
    let t = V2::new(rho0, r);
    let contact_a = V2::new(rho0, 0.0);
    let contact_b = c2 + (t - c2) * (big / (big - r));
    let nb = (contact_b - c2) / big;
    // Where the face's line meets the sphere's tangent at its contact.
    let x = V2::new(contact_b.x + contact_b.y * nb.y / nb.x, 0.0);
    let corner = Corner2 {
        u1: (contact_a - x).normalize(),
        u2: (contact_b - x).normalize(),
        convex: true,
    };
    let src = Src::Curve {
        region: u64::MAX,
        curve: u64::MAX,
    };
    for k in 0..3 {
        let axis = e[k];
        let foot = vp + (diag - axis) * s;
        let radial = (e[(k + 1) % 3] + e[(k + 2) % 3]).normalize();
        let plane = Plane {
            origin: foot + radial * x.x,
            x_dir: nalgebra::Unit::new_normalize(radial),
            normal: nalgebra::Unit::new_normalize(radial.cross(&axis)),
        };
        let (lp, _, _) = section_loop(&corner, SectionKind::Circle { r }, src)?;
        let mf = MergedFace {
            outer: lp,
            holes: vec![],
            regions: vec![u64::MAX],
        };
        let f = Frame {
            o: foot,
            x: radial,
            y: axis.cross(&radial),
            z: axis,
        };
        let tool = sweep::sweep_full(&[mf], &[], &Revolve { plane, f }, 0.0, TAU, true, true)?;
        out = super::boolean::boolean_bodies(&out, &tool.body, BoolOp::Subtract)?.body;
    }
    Ok(out)
}

/// The fillet of `edges` by `spec`, set back `setback` at every corner where three of them meet
/// (see the module docs).
pub fn fillet_setback(
    k: &mut NativeKernel,
    body: BodyId,
    edges: &[EdgeId],
    spec: &FilletSpec,
    setback: f64,
) -> Result<OpResult> {
    let FilletSize::Radius(r) = spec.size else {
        return Err(KernelError::Unsupported(
            "set-back corners of fillets not sized by radius",
        ));
    };
    let g2 = match spec.profile {
        FilletProfile::Circular => false,
        FilletProfile::Thumbweight { center: None } => true,
        _ => {
            return Err(KernelError::Unsupported(
                "set-back corners of fillets other than circular or the equal-area G2 section",
            ));
        }
    };
    if !(r > 0.0) {
        return Err(KernelError::InvalidParameter(
            "The fillet's radius must be positive".into(),
        ));
    }
    if !(setback.is_finite() && setback > r) {
        return Err(KernelError::InvalidParameter(format!(
            "The setback ({setback}) must be greater than the fillet's radius ({r})"
        )));
    }
    let b0 = std::sync::Arc::clone(k.get(body)?);
    let mut all: Vec<usize> = Vec::new();
    for &e in edges {
        let chain = if spec.propagate {
            k.tangent_chain(body, e)?
        } else {
            vec![e]
        };
        for c in chain {
            if !all.contains(&(c.0 as usize)) {
                all.push(c.0 as usize);
            }
        }
    }
    let corners = corners(&b0, &all)?;
    for c in &corners {
        for &x in &all {
            let ed = &b0.edges[x];
            if !ed.v.contains(&c.v) {
                continue;
            }
            let len = ed.curve.length(ed.t[0], ed.t[1]);
            let far = if ed.v[0] == c.v { ed.v[1] } else { ed.v[0] };
            let need = if corners.iter().any(|o| o.v == far) {
                2.0 * setback
            } else {
                setback + r
            };
            if len <= need {
                return Err(KernelError::TooLarge {
                    op: "fillet",
                    why: format!("the setback {setback} runs past an edge {len:.4} long"),
                });
            }
        }
    }
    if g2 {
        return setback_g2(k, body, &b0, &all, &corners, spec, r, setback);
    }
    let plain = super::blend::fillet(k, body, edges, spec)?;
    if corners.is_empty() {
        return Ok(plain);
    }
    let fb = std::sync::Arc::clone(k.get(plain.bodies[0])?);
    let mut src: Vec<Vec<FaceSrc>> = vec![Vec::new(); fb.faces.len()];
    for (f, o) in &plain.history.generated {
        src[f.0 as usize].push(FaceSrc::New(*o));
    }
    for (f, i) in &plain.history.modified {
        src[f.0 as usize].push(FaceSrc::Input(i.face.0 as usize));
    }
    k.release(plain.bodies[0]);
    let mut cur = Tracked {
        body: (*fb).clone(),
        src,
    };
    for c in &corners {
        let vp = b0.verts[c.v].p;
        let mark = FaceSrc::New(Origin::FromVertex {
            body,
            vertex: VertexId(c.v as u64),
        });
        // The cube of side s at the corner, grown out past the corner's faces so none of its
        // faces lies on them.
        let m = 1.0 + setback;
        let keep = cube(vp - (c.e[0] + c.e[1] + c.e[2]) * m, c.e, setback + m)?;
        let none = vec![Vec::new(); keep.faces.len()];
        let p = patch(vp, c.e, r, setback)?;
        let p = Tracked {
            src: vec![vec![mark.clone()]; p.faces.len()],
            body: p,
        }
        .apply(&keep, &none, BoolOp::Intersect)?;
        cur = cur
            .apply(&keep, &none, BoolOp::Subtract)?
            .apply(&p.body, &p.src, BoolOp::Union)?;
    }
    Ok(cur.into_result(k, body))
}

/// The cube of side `s` at the corner `c` of `b`, grown out past the corner's faces by `m` so
/// none of its faces lies on them.
fn notch(b: &Body, c: &Corner, s: f64) -> Result<Body> {
    let m = 1.0 + s;
    cube(b.verts[c.v].p - (c.e[0] + c.e[1] + c.e[2]) * m, c.e, s + m)
}

/// The set-back corners of curvature-continuous fillets ([`super::blend_setback_g2`], after
/// springrep's `blend::fillet_setback_g2`, MIT): each corner's cube goes first, then the edges
/// are filleted from inside the cubes (what the fillets would cut from each other lies within
/// the cubes, and went with them), then each corner is closed by its surface.
#[allow(clippy::too_many_arguments)]
fn setback_g2(
    k: &mut NativeKernel,
    body: BodyId,
    b0: &Body,
    all: &[usize],
    corners: &[Corner],
    spec: &FilletSpec,
    r: f64,
    setback: f64,
) -> Result<OpResult> {
    let mut cur = Tracked::of(b0.clone());
    let marks: Vec<FaceSrc> = corners
        .iter()
        .map(|c| {
            FaceSrc::New(Origin::FromVertex {
                body,
                vertex: VertexId(c.v as u64),
            })
        })
        .collect();
    for (c, mark) in corners.iter().zip(&marks) {
        let cut = notch(b0, c, setback)?;
        let cut_src = vec![vec![mark.clone()]; cut.faces.len()];
        cur = cur
            .apply(&cut, &cut_src, BoolOp::Subtract)
            .inspect_err(|e| debug(&format!("cutting the corner back: {e}")))?;
    }
    // Each edge again, by a point on what is left of it.
    let mut picked: Vec<(EdgeId, usize)> = Vec::new();
    for &x in all {
        let ed = &b0.edges[x];
        let (p0, p1) = (b0.verts[ed.v[0]].p, b0.verts[ed.v[1]].p);
        let cut0 = corners.iter().any(|c| c.v == ed.v[0]);
        let cut1 = corners.iter().any(|c| c.v == ed.v[1]);
        let len = (p1 - p0).norm();
        let (lo, hi) = (
            if cut0 { setback / len } else { 0.0 },
            if cut1 { 1.0 - setback / len } else { 1.0 },
        );
        let q = p0 + (p1 - p0) * (0.5 * (lo + hi));
        let found = (0..cur.body.edges.len()).find(|&e| {
            let ce = &cur.body.edges[e];
            let (a, b) = (cur.body.verts[ce.v[0]].p, cur.body.verts[ce.v[1]].p);
            let d = b - a;
            let t = (q - a).dot(&d) / d.norm_squared();
            matches!(ce.curve, Curve::Line { .. })
                && (0.0..=1.0).contains(&t)
                && (a + d * t - q).norm() < 1e-7 * (1.0 + len)
        });
        let Some(e) = found else {
            return Err(KernelError::InvalidGeometry {
                op: "set-back corner",
                why: "an edge is lost cutting its corner back".into(),
            });
        };
        picked.push((EdgeId(e as u64), x));
    }
    let id = k.insert(cur.body.clone());
    let ids: Vec<EdgeId> = picked.iter().map(|p| p.0).collect();
    let spec = FilletSpec {
        propagate: false,
        ..*spec
    };
    let res = super::blend::fillet(k, id, &ids, &spec);
    k.release(id);
    let res = res.inspect_err(|e| debug(&format!("the fillets: {e}")))?;
    let fb = (**k.get(res.bodies[0])?).clone();
    let mut src: Vec<Vec<FaceSrc>> = vec![Vec::new(); fb.faces.len()];
    for (f, i) in &res.history.modified {
        src[f.0 as usize].extend(cur.src[i.face.0 as usize].iter().cloned());
    }
    for (f, o) in &res.history.generated {
        let o = match *o {
            Origin::FromEdge { edge, .. } => {
                picked
                    .iter()
                    .find(|p| p.0 == edge)
                    .map_or(*o, |p| Origin::FromEdge {
                        body,
                        edge: EdgeId(p.1 as u64),
                    })
            }
            o => o,
        };
        src[f.0 as usize].push(FaceSrc::New(o));
    }
    k.release(res.bodies[0]);
    let mut cur = Tracked { body: fb, src };
    let w = super::gn_blend::equal_area_w(PI / 2.0)?;
    for (c, mark) in corners.iter().zip(&marks) {
        let vp = b0.verts[c.v].p;
        cur = super::blend_setback_g2::close_corner(cur, mark, vp, c.e, r, setback, w)
            .inspect_err(|e| debug(&format!("closing the corner: {e}")))?;
    }
    Ok(cur.into_result(k, body))
}

fn debug(what: &str) {
    if crate::native::env_flag!("CADRS_BLEND_DEBUG") {
        eprintln!("set-back corner: {what}");
    }
}
