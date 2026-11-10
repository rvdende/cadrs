//! Sharp offsets: every face moved along its normal by its distance (the surfaces' offsets), the
//! topology kept where the moved faces still meet as they did (a local operation,
//! [`crate::native::local::replace_surfaces`]); a prism's offset made as its profile's 2D offset
//! extruded where faces vanish or the profile splits; a face its neighbours cross over collapsed.

use crate::native::curve::Curve;
use crate::native::kernel::NativeKernel;
use crate::native::math::*;
use crate::native::nurbs::NurbsCurve;
use crate::native::surface::Surface;
use crate::native::tol;
use crate::native::topo::{Body, Coedge};
use crate::*;

use crate::native::local::replace_surfaces;

/// A surface moved `d` along its outward normal (sense applied).
pub fn offset_surface(s: &Surface, sense: bool, d: f64) -> Option<Surface> {
    // A B-spline that is a plane, cylinder, cone or sphere: its exact offset (a fit of a
    // cone's offset would have to pass its apex).
    if let Surface::Nurbs(n) = s
        && let Some(a) = crate::native::recognize::surface_through_poles(n)
    {
        let ((u0, u1), (v0, v1)) = n.range();
        let (u, v) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        let (ua, va) = a.project(&s.eval(u, v), None);
        let same = s.normal(u, v).dot(&a.normal(ua, va)) > 0.0;
        return offset_surface(&a, sense == same, d);
    }
    let d = if sense { d } else { -d };
    Some(match s {
        Surface::Plane { f } => {
            let n = f.x.cross(&f.y);
            Surface::Plane {
                f: Frame {
                    o: f.o + n * d,
                    ..*f
                },
            }
        }
        Surface::Cylinder { f, r } => {
            let r2 = r + d * if f.right_handed() { 1.0 } else { -1.0 };
            if r2 <= tol::ROUNDOFF {
                return None;
            }
            Surface::Cylinder { f: *f, r: r2 }
        }
        Surface::Sphere { f, r } => {
            let r2 = r + d * if f.right_handed() { 1.0 } else { -1.0 };
            if r2 <= tol::ROUNDOFF {
                return None;
            }
            Surface::Sphere { f: *f, r: r2 }
        }
        Surface::Torus { f, rmaj, rmin } => {
            let r2 = rmin + d * if f.right_handed() { 1.0 } else { -1.0 };
            if r2 <= tol::ROUNDOFF {
                return None;
            }
            Surface::Torus {
                f: *f,
                rmaj: *rmaj,
                rmin: r2,
            }
        }
        Surface::Cone { f, r, a } => {
            // Moving along the normal (cos a, −sin a) in the (ρ, h) meridian.
            let k = d * if f.right_handed() { 1.0 } else { -1.0 };
            Surface::Cone {
                f: Frame {
                    o: f.o - f.z * (k * a.sin()),
                    ..*f
                },
                r: r + k * a.cos(),
                a: *a,
            }
        }
        // An extrusion: its curve moved square to the direction (the normal is), extruded the
        // same way.
        Surface::Extrusion { c, d: dir } => {
            let off = offset_curve_along(c, &|t| s.normal(t, 0.0), d)?;
            Surface::Extrusion { c: off, d: *dir }
        }
        // A revolution: its meridian (at u = 0) moved along the normal there, revolved.
        Surface::Revolution { f, m } => {
            let off = offset_curve_along(m, &|t| s.normal(0.0, t), d)?;
            Surface::Revolution { f: *f, m: off }
        }
        // A B-spline: a surface through a grid of offset points (exact offsets aren't
        // B-splines).
        Surface::Nurbs(n) => {
            // The grid doubled until the surface holds the offset at the cells' middles too.
            // It spans a fifth more of the surface past each side where the surface is open
            // there (its polynomial continuation): a face moved in must reach its neighbours'
            // offsets past its old edges (SMLib extends the surfaces too,
            // `GetCreateExtendedSurface`). Not past a side the surface closes up across, nor
            // past a pole (a side collapsed to a point).
            let ((a0, a1), (b0, b1)) = n.range();
            let size = 1.0
                + n.cps
                    .iter()
                    .flatten()
                    .map(|h| (h.xyz() / h.w).norm())
                    .fold(0.0, f64::max);
            // Points along each side: u = a0, u = a1, v = b0, v = b1.
            let side = |k: usize| -> Vec<P3> {
                (0..=4)
                    .map(|j| {
                        let (fu, fv) = (
                            a0 + (a1 - a0) * j as f64 / 4.0,
                            b0 + (b1 - b0) * j as f64 / 4.0,
                        );
                        match k {
                            0 => s.eval(a0, fv),
                            1 => s.eval(a1, fv),
                            2 => s.eval(fu, b0),
                            _ => s.eval(fu, b1),
                        }
                    })
                    .collect()
            };
            let sides: Vec<Vec<P3>> = (0..4).map(side).collect();
            let same = |p: &P3, q: &P3| (p - q).norm() < tol::REL * size;
            let closed = |k: usize| sides[k].iter().zip(&sides[k + 1]).all(|(p, q)| same(p, q));
            let pole = |k: usize| sides[k].iter().all(|p| same(p, &sides[k][0]));
            // (Nor a rational one: past its domain its weights can reach zero.)
            let w0 = n.cps[0][0].w;
            let rational = n
                .cps
                .iter()
                .flatten()
                .any(|h| (h.w - w0).abs() > tol::REL_FINE * w0.abs());
            let grow = |(a, b): (f64, f64), k: usize| {
                if rational {
                    return (a, b);
                }
                let w = 0.2 * (b - a);
                (
                    if closed(k) || pole(k) { a } else { a - w },
                    if closed(k) || pole(k + 1) { b } else { b + w },
                )
            };
            let ((u0, u1), (v0, v1)) = (grow((a0, a1), 0), grow((b0, b1), 2));
            // At a pole the normal is the limit from inside the domain.
            let (uc, vc) = ((a0 + a1) / 2.0, (b0 + b1) / 2.0);
            let normal = |u: f64, v: f64| {
                let dd = s.d2(u, v);
                if dd[1].cross(&dd[2]).norm()
                    > tol::REL_FINE * (dd[1].norm_squared() + dd[2].norm_squared())
                {
                    return s.normal(u, v);
                }
                s.normal(u + (uc - u) * tol::DIFF_STEP, v + (vc - v) * tol::DIFF_STEP)
            };
            // A smooth pole (its normals from all round agree) offsets to a point: every grid
            // point on it that one point, so the offset surface collapses there too (a loop
            // round it then goes round a pole, not along a tiny closed edge). A cone-like apex
            // (normals turning round) offsets to a curve and stays as it is.
            let pole_point = |k: usize| -> Option<P3> {
                if !pole(k) {
                    return None;
                }
                // (Just inside the pole: on it the normal is rounding noise.)
                let (du, dv) = (1e-7 * (a1 - a0), 1e-7 * (b1 - b0));
                let ns: Vec<V3> = (0..16)
                    .map(|j| {
                        let f = j as f64 / 16.0;
                        match k {
                            0 => s.normal(a0 + du, b0 + (b1 - b0) * f),
                            1 => s.normal(a1 - du, b0 + (b1 - b0) * f),
                            2 => s.normal(a0 + (a1 - a0) * f, b0 + dv),
                            _ => s.normal(a0 + (a1 - a0) * f, b1 - dv),
                        }
                    })
                    .collect();
                let mean = ns.iter().fold(V3::zeros(), |m, n| m + n).normalize();
                ns.iter()
                    .all(|n| (n - mean).norm() < 1e-4)
                    .then(|| sides[k][0] + mean * d)
            };
            let poles: Vec<Option<P3>> = (0..4).map(pole_point).collect();
            let at = |u: f64, v: f64| {
                let (eu, ev) = (tol::REL_FINE * (a1 - a0), tol::REL_FINE * (b1 - b0));
                let on = [
                    (u - a0).abs() <= eu,
                    (u - a1).abs() <= eu,
                    (v - b0).abs() <= ev,
                    (v - b1).abs() <= ev,
                ];
                match (0..4).find_map(|k| if on[k] { poles[k] } else { None }) {
                    Some(q) => q,
                    None => s.eval(u, v) + normal(u, v) * d,
                }
            };
            let mut k = 32;
            loop {
                let us: Vec<f64> = (0..=k)
                    .map(|i| u0 + (u1 - u0) * i as f64 / k as f64)
                    .collect();
                let vs: Vec<f64> = (0..=k)
                    .map(|j| v0 + (v1 - v0) * j as f64 / k as f64)
                    .collect();
                let rows: Vec<NurbsCurve> = us
                    .iter()
                    .map(|&u| {
                        let pts: Vec<P3> = vs.iter().map(|&v| at(u, v)).collect();
                        NurbsCurve::interpolate(&pts, &vs, 3, None, None)
                    })
                    .collect();
                let surf =
                    crate::native::nurbs::NurbsSurface::through_curves(&rows, &us, 3, None, None);
                // through_curves runs its rows along v and interpolates them along u: transpose
                // to keep the parameters' roles.
                let cps: Vec<Vec<crate::native::nurbs::H>> = (0..surf.cps[0].len())
                    .map(|j| surf.cps.iter().map(|r| r[j]).collect())
                    .collect();
                let off = Surface::Nurbs(std::sync::Arc::new(crate::native::nurbs::NurbsSurface {
                    du: surf.dv,
                    dv: surf.du,
                    ku: surf.kv.clone(),
                    kv: surf.ku.clone(),
                    cps,
                }));
                let err = us
                    .windows(2)
                    .flat_map(|a| {
                        vs.windows(2)
                            .map(move |b| (0.5 * (a[0] + a[1]), 0.5 * (b[0] + b[1])))
                    })
                    .map(|(u, v)| (off.eval(u, v) - at(u, v)).norm())
                    .fold(0.0, f64::max);
                if err < OFFSET_FIT * (1.0 + d.abs()) * 1e3 || k >= 32 {
                    break off;
                }
                k *= 2;
            }
        }
    })
}

/// A curve moved by `d` along the unit field `n(t)`: exact for a line moved by a constant
/// vector and a circle moved square to itself in its plane (concentric), else a cubic through
/// 65 offset points on its parameters. None when a circle would shrink to nothing.
fn offset_curve_along(c: &Curve, n: &dyn Fn(f64) -> V3, d: f64) -> Option<Curve> {
    let (t0, t1) = c.nurbs_range().unwrap_or(match c {
        Curve::Circle { .. } | Curve::Ellipse { .. } => (0.0, TAU),
        _ => (-1.0, 1.0),
    });
    match c {
        Curve::Line { o, d: dir } => {
            let (a, b) = (n(t0), n(t1));
            if (a - b).norm() < tol::ROUNDOFF {
                return Some(Curve::Line {
                    o: o + a * d,
                    d: *dir,
                });
            }
        }
        Curve::Circle { c: cc, x, y, r } => {
            // Normal along the radius (in the plane): a concentric circle.
            let p = c.point(0.0);
            let radial = (p - cc) / *r;
            let nn = n(0.0);
            let along = nn.dot(&radial);
            if (along.abs() - 1.0).abs() < tol::ANGULAR {
                let r2 = r + d * along;
                return (r2 > tol::ROUNDOFF).then_some(Curve::Circle {
                    c: *cc,
                    x: *x,
                    y: *y,
                    r: r2,
                });
            }
            // Normal across the plane: the circle moved along it.
            let z = x.cross(y);
            if (nn.dot(&z).abs() - 1.0).abs() < tol::ANGULAR {
                return Some(Curve::Circle {
                    c: cc + nn * d,
                    x: *x,
                    y: *y,
                    r: *r,
                });
            }
        }
        _ => {}
    }
    // A cubic through offset points, their number doubled until it holds the offset between
    // them too (the curve's knots kink the offset).
    let at = |t: f64| c.point(t) + n(t) * d;
    let mut k = 64;
    loop {
        let ts: Vec<f64> = (0..=k)
            .map(|i| t0 + (t1 - t0) * i as f64 / k as f64)
            .collect();
        let pts: Vec<P3> = ts.iter().map(|&t| at(t)).collect();
        let fit = Curve::Nurbs(std::sync::Arc::new(NurbsCurve::interpolate(
            &pts, &ts, 3, None, None,
        )));
        let err = ts
            .windows(2)
            .map(|w| (fit.point(0.5 * (w[0] + w[1])) - at(0.5 * (w[0] + w[1]))).norm())
            .fold(0.0, f64::max);
        if err < OFFSET_FIT * (1.0 + d.abs()) || k >= 1024 {
            return Some(fit);
        }
        k *= 2;
    }
}

/// How closely an approximated offset (curve or surface) follows the true one (mm, relative to
/// the distance).
const OFFSET_FIT: f64 = 1e-9;

/// Sharp offset: every face moved by its distance, the edges where the moved faces meet.
pub fn offset_sharp(b: &Body, dist: &[f64]) -> Result<Body> {
    let surfs: Vec<Surface> = b
        .faces
        .iter()
        .zip(dist)
        .map(|(f, d)| {
            if *d == 0.0 {
                Some(f.surf.clone())
            } else {
                offset_surface(&f.surf, f.sense, *d)
            }
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(KernelError::Unsupported("offsetting this kind of face"))?;
    if let Some(r) = prism_offset(b, dist) {
        return r;
    }
    // A prism with a curved side shrunk past its curvature: its sides' offsets fold over
    // themselves, and its outline's trimmed offset extruded is the inset.
    if let Some(r) = super::prism::curved_prism_inset(b, dist, false, true) {
        return r;
    }
    let out = match replace_surfaces(b, &surfs) {
        Ok(out) => out,
        // Faces or corners the moves swallow (a pyramid shrunk past its apex, which becomes a
        // ridge): of a convex polyhedron, the solid its moved planes bound; of a prism with a
        // curved side, its outline's offset extruded.
        Err(e) => {
            return match crate::native::local::planes_bound(b, &surfs) {
                Some(Ok((out, _))) => Ok(out),
                Some(Err(_)) | None => {
                    match super::prism::curved_prism_inset(b, dist, false, false) {
                        Some(Ok(out)) => Ok(out),
                        _ => Err(e),
                    }
                }
            };
        }
    };
    // A face its moved neighbours cross over vanishes, as a prism's side does in
    // `prism_offset`: they meet instead, and the offset is made again without it (when that
    // fails, the faces as they moved).
    match crossed_face(b, &out) {
        Some((f, k)) if b.faces.len() > 4 => {
            let (nb, nd) = collapse_face(b, dist, f, k);
            Ok(offset_sharp(&nb, &nd).unwrap_or(out))
        }
        _ => Ok(out),
    }
}

/// A face of `b` that its offset `out` (the faces moved, the topology kept) turned inside out:
/// a four-sided face between two parallel straight edges whose moved lines crossed over each
/// other (the face's moved neighbours on either side meet before reaching it: a notch's short
/// slope between its floor and wall, filled by their moves). The face and the index k of its
/// coedge on one of those edges (the other is coedge k + 2); of several, the one crossed most.
fn crossed_face(b: &Body, out: &Body) -> Option<(usize, usize)> {
    let ef = b.edge_faces();
    let line = |body: &Body, e: usize| match body.edges[e].curve {
        Curve::Line { o, d } if !body.edges[e].degenerate => {
            d.try_normalize(tol::DEGENERATE).map(|d| (o, d))
        }
        _ => None,
    };
    // The part of q − o square to d.
    let across = |o: P3, q: P3, d: V3| {
        let w = q - o;
        w - d * w.dot(&d)
    };
    let mut worst: Option<(f64, usize, usize)> = None;
    for (fi, f) in b.faces.iter().enumerate() {
        let [l] = f.loops.as_slice() else { continue };
        if l.len() != 4
            || l.iter()
                .any(|c| ef[c.edge].len() != 2 || ef[c.edge][0] == ef[c.edge][1])
        {
            continue;
        }
        for k in 0..2 {
            let (ea, eb) = (l[k].edge, l[k + 2].edge);
            let (Some((oa, da)), Some((ob, db)), Some((na, nda)), Some((nb, _))) =
                (line(b, ea), line(b, eb), line(out, ea), line(out, eb))
            else {
                continue;
            };
            if da.cross(&db).norm() > tol::ANGULAR {
                continue;
            }
            let (w0, w1) = (across(oa, ob, da), across(na, nb, nda));
            // The width across the face after the move, as a share of the width before.
            let share = w0.dot(&w1) / w0.norm_squared();
            if share < -tol::ROUNDOFF && worst.is_none_or(|(x, _, _)| share < x) {
                worst = Some((share, fi, k));
            }
        }
    }
    worst.map(|(_, f, k)| (f, k))
}

/// `b` without the four-sided face `f` ([`crossed_face`]; its coedges k and k + 2 on its two
/// parallel edges), and `dist` without its distance: those two edges made one (the first)
/// between the faces beyond them, and the other two edges' ends merged, the edges gone from the
/// faces beyond them. The geometry is the old one: the offset recomputes it.
fn collapse_face(b: &Body, dist: &[f64], f: usize, k: usize) -> (Body, Vec<f64>) {
    let l = &b.faces[f].loops[0];
    let (ca, cs1, cb, cs2) = (&l[k], &l[k + 1], &l[(k + 2) % 4], &l[(k + 3) % 4]);
    let mut vmap: Vec<usize> = (0..b.verts.len()).collect();
    vmap[b.co_start(cb)] = b.co_end(ca);
    vmap[b.co_end(cb)] = b.co_start(ca);
    let mut nb = b.clone();
    for e in &mut nb.edges {
        e.v = [vmap[e.v[0]], vmap[e.v[1]]];
    }
    let (ea, eb, gone) = (ca.edge, cb.edge, [cs1.edge, cs2.edge]);
    for (fi, face) in nb.faces.iter_mut().enumerate() {
        if fi == f {
            continue;
        }
        let surf = face.surf.clone();
        for lp in &mut face.loops {
            lp.retain(|c| !gone.contains(&c.edge));
            for c in lp.iter_mut().filter(|c| c.edge == eb) {
                // The face beyond `cb` runs along it the way `ca` runs in `f`.
                let e = &b.edges[ea];
                *c = Coedge {
                    edge: ea,
                    fwd: ca.fwd,
                    pc: crate::native::pcurve::pcurve(&surf, &e.curve, e.t[0], e.t[1], None),
                };
            }
        }
    }
    nb.faces.remove(f);
    for sh in &mut nb.shells {
        sh.faces.retain(|&x| x != f);
        for x in &mut sh.faces {
            if *x > f {
                *x -= 1;
            }
        }
    }
    crate::native::unify::compact(&mut nb);
    let mut nd = dist.to_vec();
    nd.remove(f);
    (nb, nd)
}

/// A planar face's outward normal.
fn plane_normal(b: &Body, f: usize) -> Option<V3> {
    let face = &b.faces[f];
    let Surface::Plane { f: fr } = &face.surf else {
        return None;
    };
    Some(fr.x.cross(&fr.y).normalize() * if face.sense { 1.0 } else { -1.0 })
}

/// The prism's sharp offset as the union of what its moved profile encloses: each side on its
/// moved line, neighbours along one line joined by a step square to it at their corner, and
/// the loop (which may cross itself where the moves close a slot or make a side vanish) taken
/// as the region it winds round at least once; extruded between the moved caps.
#[allow(clippy::too_many_arguments)]
fn union_offset(
    b: &Body,
    dist: &[f64],
    bot: usize,
    top: usize,
    sides: &[(V2, f64, f64)],
    corners: &[P2],
    o: P3,
    fr: &Frame,
    n: V3,
) -> Result<Body> {
    let k = sides.len();
    let meet = |a: &(V2, f64, f64), c: &(V2, f64, f64)| -> P2 {
        let det = a.0.perp(&c.0);
        let (ca, cc) = (a.1 + a.2, c.1 + c.2);
        P2::new(
            (ca * c.0.y - cc * a.0.y) / det,
            (a.0.x * cc - c.0.x * ca) / det,
        )
    };
    let mut raw: Vec<P2> = Vec::new();
    for i in 0..k {
        let (a, c) = (sides[i], sides[(i + 1) % k]);
        if a.0.perp(&c.0).abs() > tol::ANGULAR_FINE {
            raw.push(meet(&a, &c));
        } else {
            raw.push(corners[i] + a.0 * a.2);
            raw.push(corners[i] + c.0 * c.2);
        }
    }
    let scale = 1.0 + raw.iter().map(|p| p.coords.norm()).fold(0.0, f64::max);
    let tol = tol::REL * scale;
    raw.dedup_by(|x, y| (*x - *y).norm() < tol);
    while raw.len() > 1 && (raw[0] - raw[raw.len() - 1]).norm() < tol {
        raw.pop();
    }
    let loops = positive_region(&raw, tol);
    if loops.is_empty() {
        return Err(KernelError::OperationFailed(
            "the offset profile vanishes".into(),
        ));
    }
    // Outer loops (counter-clockwise) with the holes (clockwise) inside them.
    let signed = |l: &[P2]| {
        (0..l.len())
            .map(|i| l[i].coords.perp(&l[(i + 1) % l.len()].coords))
            .sum::<f64>()
            * 0.5
    };
    let inside = |q: &P2, l: &[P2]| winding(l, q) != 0;
    let to_loop = |l: &[P2]| Loop {
        curves: (0..l.len())
            .map(|i| Curve2::Line {
                a: l[i],
                b: l[(i + 1) % l.len()],
                source: None,
            })
            .collect(),
    };
    let (outers, holes): (Vec<&Vec<P2>>, Vec<&Vec<P2>>) =
        loops.iter().partition(|l| signed(l) > 0.0);
    let regions: Vec<Region> = outers
        .iter()
        .map(|ol| Region {
            outer: to_loop(ol),
            holes: holes
                .iter()
                .filter(|h| inside(&h[0], ol))
                .map(|h| to_loop(h))
                .collect(),
            source: None,
        })
        .collect();
    let (z0, z1) = (
        -dist[bot],
        (b.verts[b.edges[b.faces[top].loops[0][0].edge].v[0]].p - o).dot(&n) + dist[top],
    );
    if z1 - z0 <= tol::ROUNDOFF {
        return Err(KernelError::OperationFailed(
            "the offset prism has no height".into(),
        ));
    }
    let plane = Plane {
        origin: o + n * z0,
        x_dir: nalgebra::Unit::new_normalize(fr.x),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let prof = Profile::new(plane, regions);
    let mut kk = NativeKernel::new();
    let r = crate::Kernel::extrude(&mut kk, &prof, Extent::Blind(z1 - z0))?;
    let [id] = r.bodies[..] else {
        return Err(KernelError::Unsupported(
            "an offset that splits the profile in pieces",
        ));
    };
    Ok((**kk.get(id)?).clone())
}

/// The winding number of the closed polygon `l` about `q`.
fn winding(l: &[P2], q: &P2) -> i32 {
    let mut w = 0;
    for i in 0..l.len() {
        let (a, c) = (l[i], l[(i + 1) % l.len()]);
        let side = (c - a).perp(&(q - a));
        if a.y <= q.y {
            if c.y > q.y && side > 0.0 {
                w += 1;
            }
        } else if c.y <= q.y && side < 0.0 {
            w -= 1;
        }
    }
    w
}

/// The boundary of the region a closed polygon winds round at least once, as loops (outer ones
/// counter-clockwise, holes clockwise; straight runs one side each): its edges cut where they
/// cross, the pieces with the region on their left and none on their right, chained.
fn positive_region(raw: &[P2], tol: f64) -> Vec<Vec<P2>> {
    let m = raw.len();
    let seg = |i: usize| (raw[i], raw[(i + 1) % m]);
    let mut pieces: Vec<(P2, P2)> = Vec::new();
    for i in 0..m {
        let (a, c) = seg(i);
        let d = c - a;
        let len2 = d.norm_squared();
        if len2 < tol * tol {
            continue;
        }
        let mut ts = vec![0.0, 1.0];
        for j in 0..m {
            if j == i {
                continue;
            }
            let (p, q) = seg(j);
            let e = q - p;
            let den = d.perp(&e);
            if den.abs() > 1e-15 * len2.sqrt() * e.norm().max(1e-300) {
                let t = (p - a).perp(&e) / den;
                let s = (p - a).perp(&d) / den;
                if t > 0.0
                    && t < 1.0
                    && (-tol::ROUNDOFF_FINE..=1.0 + tol::ROUNDOFF_FINE).contains(&s)
                {
                    ts.push(t);
                }
            } else {
                // Collinear overlaps: cut at the other's ends.
                for r in [p, q] {
                    if (r - a).perp(&d).abs() < tol * len2.sqrt() {
                        let t = (r - a).dot(&d) / len2;
                        if t > 0.0 && t < 1.0 {
                            ts.push(t);
                        }
                    }
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() * len2.sqrt() < tol);
        for w in ts.windows(2) {
            let (p, q) = (a + d * w[0], a + d * w[1]);
            if (q - p).norm() < tol {
                continue;
            }
            let mid = p + (q - p) * 0.5;
            let side = V2::new(-(q - p).y, (q - p).x).normalize() * (tol * 100.0);
            if winding(raw, &(mid - side)) <= 0 && winding(raw, &(mid + side)) >= 1 {
                pieces.push((p, q));
            }
        }
    }
    // Chained end to start.
    let mut used = vec![false; pieces.len()];
    let mut loops: Vec<Vec<P2>> = Vec::new();
    for s in 0..pieces.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let mut lp = vec![pieces[s].0];
        let mut end = pieces[s].1;
        while (end - lp[0]).norm() > tol * 10.0 {
            let Some(j) = (0..pieces.len()).filter(|&j| !used[j]).min_by(|&x, &y| {
                (pieces[x].0 - end)
                    .norm()
                    .total_cmp(&(pieces[y].0 - end).norm())
            }) else {
                break;
            };
            if (pieces[j].0 - end).norm() > tol * 10.0 {
                break;
            }
            used[j] = true;
            lp.push(pieces[j].0);
            end = pieces[j].1;
        }
        // Straight runs as one side.
        let mut simple: Vec<P2> = Vec::new();
        let n = lp.len();
        for i in 0..n {
            let (p, q, r) = (lp[(i + n - 1) % n], lp[i], lp[(i + 1) % n]);
            if (q - p).perp(&(r - q)).abs() > tol * ((q - p).norm() + (r - q).norm()) {
                simple.push(q);
            }
        }
        if simple.len() >= 3 {
            loops.push(simple);
        }
    }
    loops
}

/// The sharp offset of a prism (a polygon's extrusion: two parallel planar caps, flat sides
/// along the extrusion) when its faces' moves change its topology: the profile's sides moved by
/// their distances as a wavefront (OCCT's intersection offset and SMLib's offset executive trim
/// the offset faces against each other the same way): a side whose moved neighbours cross over
/// it vanishes and they meet instead, and of two sides left parallel and adjacent facing the
/// same way, the one the other moved past vanishes; then the trimmed profile extruded between
/// the moved caps. None when the body isn't such a prism or no side vanishes (the faces keep
/// their topology: [`replace_surfaces`] moves them).
fn prism_offset(b: &Body, dist: &[f64]) -> Option<Result<Body>> {
    // The caps: two planar faces with opposite normals; every other face planar along them.
    let normals: Vec<Option<V3>> = (0..b.faces.len()).map(|f| plane_normal(b, f)).collect();
    if normals.iter().any(|n| n.is_none()) || b.faces.len() < 5 || b.solids.len() != 1 {
        return None;
    }
    let normals: Vec<V3> = normals.into_iter().flatten().collect();
    let (bot, top) = (0..b.faces.len())
        .flat_map(|i| (0..b.faces.len()).map(move |j| (i, j)))
        .find(|&(i, j)| {
            i != j
                && normals[i].dot(&normals[j]) < -1.0 + tol::ANGULAR_FINE
                && (0..b.faces.len()).all(|k| {
                    k == i || k == j || normals[k].dot(&normals[i]).abs() < tol::ANGULAR_FINE
                })
        })?;
    if b.faces[bot].loops.len() != 1 || b.faces[top].loops.len() != 1 {
        return None;
    }
    let ef = b.edge_faces();
    let n = -normals[bot];
    let o = b.verts[b.edges[b.faces[bot].loops[0][0].edge].v[0]].p;
    let fr = Frame::from_z(o, n);
    let to2 = |p: &P3| P2::new((p - o).dot(&fr.x), (p - o).dot(&fr.y));
    // The bottom loop's sides in order, with their 2D outward normals and lines m·p = c.
    let mut sides: Vec<(V2, f64, f64)> = Vec::new();
    let mut pts: Vec<P2> = Vec::new();
    for c in &b.faces[bot].loops[0] {
        let e = &b.edges[c.edge];
        if !matches!(e.curve, Curve::Line { .. }) || ef[c.edge].len() != 2 {
            return None;
        }
        let side = ef[c.edge].iter().copied().find(|&f| f != bot)?;
        let m = V2::new(normals[side].dot(&fr.x), normals[side].dot(&fr.y));
        let (t0, _) = crate::native::topo::co_range(e, c.fwd);
        let p = to2(&e.point(t0));
        sides.push((m, m.dot(&p.coords), dist[side]));
        pts.push(p);
    }
    // Counter-clockwise about n. Each side's end corner (the start of the next).
    let area: f64 = (0..pts.len())
        .map(|i| pts[i].coords.perp(&pts[(i + 1) % pts.len()].coords))
        .sum();
    let k = sides.len();
    let mut corners: Vec<P2> = (0..k).map(|i| pts[(i + 1) % k]).collect();
    if area < 0.0 {
        sides.reverse();
        corners = (0..k).map(|j| pts[k - 1 - j]).collect();
    }
    // Neighbours along one line moved apart (coplanar faces offset differently): a step between
    // them, and the moved profile, which may then close over itself (a slot whose walls cross),
    // is the union of what it encloses.
    let steps = (0..k).any(|i| {
        let (a, c) = (sides[i], sides[(i + 1) % k]);
        a.0.perp(&c.0).abs() < tol::ANGULAR_FINE && (a.2 - c.2).abs() > tol::ROUNDOFF_FINE
    });
    if steps {
        return Some(union_offset(b, dist, bot, top, &sides, &corners, o, &fr, n));
    }
    // Each side's direction (along the loop, its outward normal on the right).
    let dir = |m: &V2| V2::new(-m.y, m.x);
    let meet = |a: &(V2, f64, f64), b: &(V2, f64, f64)| -> Option<P2> {
        let det = a.0.perp(&b.0);
        if det.abs() < tol::DEGENERATE {
            return None;
        }
        let (ca, cb) = (a.1 + a.2, b.1 + b.2);
        Some(P2::new(
            (ca * b.0.y - cb * a.0.y) / det,
            (a.0.x * cb - b.0.x * ca) / det,
        ))
    };
    let mut live: Vec<(V2, f64, f64)> = sides.clone();
    let mut removed = false;
    loop {
        let n_live = live.len();
        if n_live < 3 {
            return Some(Err(KernelError::TooLarge {
                op: "offset",
                why: "the offset profile vanishes".into(),
            }));
        }
        // Adjacent parallel sides: the one moved past the other vanishes.
        let mut drop: Option<usize> = None;
        for i in 0..n_live {
            let (a, c) = (live[i], live[(i + 1) % n_live]);
            if a.0.perp(&c.0).abs() < tol::ANGULAR_FINE {
                if a.0.dot(&c.0) < 0.0 {
                    return Some(union_offset(b, dist, bot, top, &sides, &corners, o, &fr, n));
                }
                drop = Some(if a.1 + a.2 < c.1 + c.2 {
                    i
                } else {
                    (i + 1) % n_live
                });
                break;
            }
        }
        // Else the side its moved neighbours cross over the most.
        if drop.is_none() {
            let mut worst = (0.0, None);
            for i in 0..n_live {
                let (p, q) = (
                    meet(&live[(i + n_live - 1) % n_live], &live[i])?,
                    meet(&live[i], &live[(i + 1) % n_live])?,
                );
                let along = (q - p).dot(&dir(&live[i].0));
                if along < worst.0 - tol::ROUNDOFF {
                    worst = (along, Some(i));
                }
            }
            drop = worst.1;
        }
        match drop {
            Some(i) => {
                live.remove(i);
                removed = true;
            }
            None => break,
        }
    }
    if !removed || k == live.len() {
        return None;
    }
    let n_live = live.len();
    let poly: Vec<P2> = (0..n_live)
        .filter_map(|i| meet(&live[(i + n_live - 1) % n_live], &live[i]))
        .collect();
    if poly.len() != n_live {
        return Some(Err(KernelError::InvalidGeometry {
            op: "offset",
            why: "the offset profile doesn't close".into(),
        }));
    }
    // Sides that shrank to nothing exactly (three or more meeting at one point): their
    // corners are one.
    let scale = 1.0 + poly.iter().map(|p| p.coords.norm()).fold(0.0, f64::max);
    let mut poly: Vec<P2> = poly;
    poly.dedup_by(|a, b| (*a - *b).norm() < tol::REL * scale);
    while poly.len() > 1 && (poly[0] - poly[poly.len() - 1]).norm() < tol::REL * scale {
        poly.pop();
    }
    let n_live = poly.len();
    if n_live < 3 {
        return Some(Err(KernelError::TooLarge {
            op: "offset",
            why: "the offset profile vanishes".into(),
        }));
    }
    let (z0, z1) = (
        -dist[bot],
        (b.verts[b.edges[b.faces[top].loops[0][0].edge].v[0]].p - o).dot(&n) + dist[top],
    );
    if z1 - z0 <= tol::ROUNDOFF {
        return Some(Err(KernelError::TooLarge {
            op: "offset",
            why: "the offset prism has no height".into(),
        }));
    }
    let plane = Plane {
        origin: o + n * z0,
        x_dir: nalgebra::Unit::new_normalize(fr.x),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let curves = (0..n_live)
        .map(|i| Curve2::Line {
            a: poly[i],
            b: poly[(i + 1) % n_live],
            source: None,
        })
        .collect();
    let prof = Profile::new(
        plane,
        vec![Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        }],
    );
    let mut kk = NativeKernel::new();
    Some(
        crate::Kernel::extrude(&mut kk, &prof, Extent::Blind(z1 - z0)).and_then(|r| {
            let [id] = r.bodies[..] else {
                return Err(KernelError::InvalidResult {
                    op: "offset",
                    why: "the offset prism isn't one body".into(),
                });
            };
            Ok((**kk.get(id)?).clone())
        }),
    )
}
