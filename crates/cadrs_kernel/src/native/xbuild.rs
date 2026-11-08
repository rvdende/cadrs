//! Rebuilding a [`Body`] from a neutral boundary description, as STEP and IGES files hold it:
//! points, 3D edge curves between vertices, and faces as surfaces bounded by loops of oriented
//! edges, without p-curves (files may carry some, but we compute our own). Files from other
//! systems are often loose, so the rebuild repairs what it reasonably can.
//!
//! - **Surfaces.** A surface of revolution or extrusion that is really a plane, cylinder, cone,
//!   sphere or torus becomes one ([`analytic`]). A face on a cone's far nappe (beyond the apex)
//!   or on the inner part of a spindle torus is re-expressed so our projections find it.
//! - **Edges.** An edge's parameter range comes from its vertices projected on its curve; it
//!   runs from its first vertex to its second for increasing t (a curve running the other way is
//!   turned round). A closed edge (one vertex) on a circle or ellipse spans a whole turn.
//! - **Loops** that don't chain end to start (wrong orientation flags, edges out of order) are
//!   repaired first.
//! - **p-curves.** Exact ones from [`pcurve::pcurve`] (checked: the surface must meet the edge),
//!   else a piecewise cubic fit of projected points within the modelling tolerance (relative to
//!   the model's size). Each is shifted by whole periods so it starts where the previous one
//!   ended (as `unify` aligns them), so the loop is continuous in (u, v) on periodic surfaces;
//!   on closed spline surfaces (not periodic in evaluation) only within the parameter range, so a
//!   seam's two uses lie on its two boundaries. Fitted p-curves end exactly at their vertices.
//! - **Singular points.** Files leave out the point edges at a sphere's pole or a cone's apex,
//!   which our loops need: where a loop's p-curves jump at a point where the surface collapses,
//!   a degenerate edge is inserted. Its jump in u is free by whole periods: the loop must close,
//!   and when that leaves a choice (a whole sphere's seam used both ways) the loop is made to run
//!   the way the file's face sense says.
//! - **Missing seams.** A face on a periodic surface whose loops wrap around it with no seam (a
//!   cylinder band bounded by two circles, a spherical cap bounded by one, a whole sphere or
//!   torus with no bounds) gets a seam edge along an iso line, splitting an edge where needed.
//! - **Senses.** The outer loop is the one enclosing the largest uv area; it goes first, and its
//!   orientation in uv sets the face's sense ([`super::build::fix_senses`]'s rule: loops run
//!   counter-clockwise about the outward normal).
//! - **Shells** are made consistently oriented ([`orient_shells`]): a face whose loops run the
//!   wrong way about it is turned round, and solids are turned to enclose a positive volume.

use std::sync::Arc;

use super::curve::Curve;
use super::math::*;
use super::nurbs::{NurbsCurve, NurbsSurface};
use super::pcurve;
use super::surface::Surface;
use super::tol;
use super::topo::*;

/// A face as a file describes it: a surface, the sense the file gives (used only to settle
/// ambiguous loops) and loops of (edge, along the edge).
#[derive(Debug, Clone)]
pub struct XFace {
    pub surf: Surface,
    pub sense: bool,
    pub loops: Vec<Vec<(usize, bool)>>,
}

/// Collects vertices, edges, faces, shells and solids, then builds the body ([`Self::finish`]).
#[derive(Debug, Default)]
pub struct XBuilder {
    /// Vertices and edges as they are added (faces, shells and solids come at the end).
    pub body: Body,
    pub faces: Vec<XFace>,
    shells: Vec<(Vec<usize>, bool)>,
    solids: Vec<Vec<usize>>,
    /// Problems met (faces dropped, gaps left), for diagnostics.
    pub warnings: Vec<String>,
    /// Whether the file says nothing of the faces' orientation (IGES trimmed surfaces): solids
    /// are then always turned to enclose a positive volume.
    pub unoriented: bool,
    /// Whether the loops' edge orientations are right (OCCT files): loops are then only
    /// reordered, never turned round edge by edge.
    pub trust_orientation: bool,
    /// p-curves by (face, edge), before any shift by periods (faces are traced more than once).
    pc_cache: std::cell::RefCell<std::collections::HashMap<(usize, usize), Curve>>,
}

/// A boundary piece of a face given without topology: a curve from `a` to `b` along it.
#[derive(Debug, Clone)]
pub struct LooseSeg {
    pub c: Curve,
    pub a: P3,
    pub b: P3,
}

/// The boundary of a spline surface's whole parameter range, counter-clockwise in (u, v).
pub fn natural_boundary(surf: &Surface) -> Result<Vec<LooseSeg>, String> {
    let Surface::Nurbs(n) = surf else {
        return Err("an untrimmed surface that is not a spline".into());
    };
    let ((u0, u1), (v0, v1)) = n.range();
    let (nu, nv) = (n.cps.len(), n.cps[0].len());
    let row = |j: usize| NurbsCurve {
        degree: n.du,
        knots: n.ku.clone(),
        cps: (0..nu).map(|i| n.cps[i][j]).collect(),
    };
    let col = |i: usize| NurbsCurve {
        degree: n.dv,
        knots: n.kv.clone(),
        cps: n.cps[i].clone(),
    };
    let mut out = Vec::new();
    for (c, (pa, pb)) in [
        (row(0), ((u0, v0), (u1, v0))),
        (col(nu - 1), ((u1, v0), (u1, v1))),
        (row(nv - 1), ((u1, v1), (u0, v1))),
        (col(0), ((u0, v1), (u0, v0))),
    ] {
        let (a, b) = (surf.eval(pa.0, pa.1), surf.eval(pb.0, pb.1));
        out.push(LooseSeg {
            c: Curve::Nurbs(Arc::new(c)),
            a,
            b,
        });
    }
    Ok(out)
}

/// Faces given without topology (IGES trimmed and bounded surfaces, STEP curve-bounded
/// surfaces), sewn by their boundary curves: vertices welded within `tol`, segments matched to
/// the edges other faces already have, each connected group of faces a shell (a solid when
/// closed). `unoriented`: the file says nothing of the faces' sides (they are then made
/// consistent and outward). The vertices and edges take the gaps left (up to ten weld
/// distances) as their tolerances. Returns the body and the problems met.
pub fn sew_loose(
    faces: Vec<(Surface, Vec<Vec<LooseSeg>>)>,
    tol: f64,
    unoriented: bool,
) -> (Body, Vec<String>) {
    let mut xb = XBuilder::new();
    let mut weld: Vec<P3> = Vec::new();
    let mut cells: std::collections::HashMap<[i64; 3], Vec<usize>> =
        std::collections::HashMap::new();
    let mut vertex = |xb: &mut XBuilder, p: P3| -> usize {
        let k = [p.x, p.y, p.z].map(|c| (c / tol).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(v) = cells.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                        for &i in v {
                            if (weld[i] - p).norm() <= tol {
                                return i;
                            }
                        }
                    }
                }
            }
        }
        weld.push(p);
        let i = xb.vertex(p);
        cells.entry(k).or_default().push(i);
        i
    };
    let mut by_pair: std::collections::HashMap<(usize, usize), Vec<usize>> =
        std::collections::HashMap::new();
    let mut fids = Vec::new();
    for (surf, seg_loops) in faces {
        let mut loops = Vec::new();
        for segs in seg_loops {
            let mut l = Vec::new();
            for sg in segs {
                let (va, vb) = (vertex(&mut xb, sg.a), vertex(&mut xb, sg.b));
                let Some(ne) = xb.edge(sg.c, va, vb) else {
                    continue;
                };
                // The same edge as one of another face's?
                let key = (va.min(vb), va.max(vb));
                let found = by_pair.get(&key).and_then(|cands| {
                    let new = &xb.body.edges[ne];
                    let mid = new.point(new.mid_t());
                    cands.iter().copied().find(|&c| {
                        let old = &xb.body.edges[c];
                        (old.point(old.curve.closest(&mid, old.t[0], old.t[1])) - mid).norm()
                            < tol * LOOSE_SHARE
                    })
                });
                match found {
                    Some(c) => {
                        let fwd = if va != vb {
                            xb.body.edges[c].v[0] == va
                        } else {
                            let (new, old) = (&xb.body.edges[ne], &xb.body.edges[c]);
                            new.curve
                                .tangent(new.t[0])
                                .dot(&old.curve.tangent(old.t[0]))
                                > 0.0
                        };
                        xb.body.edges.pop();
                        l.push((c, fwd));
                    }
                    None => {
                        by_pair.entry(key).or_default().push(ne);
                        l.push((ne, true));
                    }
                }
            }
            if !l.is_empty() {
                loops.push(l);
            }
        }
        if !loops.is_empty() {
            fids.push(xb.face(XFace {
                surf,
                sense: true,
                loops,
            }));
        }
    }
    // Shells: faces connected through shared edges; closed when every edge is used twice.
    let mut parent: Vec<usize> = (0..fids.len()).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        p[x] = r;
        r
    }
    let mut users: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for (k, &fi) in fids.iter().enumerate() {
        for &(e, _) in xb.faces[fi].loops.iter().flatten() {
            users.entry(e).or_default().push(k);
        }
    }
    for us in users.values() {
        for w in us.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            parent[a.max(b)] = a.min(b);
        }
    }
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    for k in 0..fids.len() {
        let g = find(&mut parent, k);
        match groups.iter_mut().find(|x| x.0 == g) {
            Some(x) => x.1.push(k),
            None => groups.push((g, vec![k])),
        }
    }
    for (_, ks) in groups {
        let closed = ks.iter().all(|&k| {
            xb.faces[fids[k]]
                .loops
                .iter()
                .flatten()
                .all(|&(e, _)| users[&e].len() == 2)
        });
        let sh = xb.shell(ks.iter().map(|&k| fids[k]).collect(), closed);
        if closed {
            xb.solid(vec![sh]);
        }
    }
    xb.unoriented = unoriented;
    let (mut body, w) = xb.finish();
    // The faces' curves meet within the weld distance, not exactly: the vertices and edges
    // take their gaps as their tolerances.
    body.fit_tolerances(tol * LOOSE_SHARE);
    (body, w)
}

/// How close (in weld distances) a loose face's boundary segment must run to another face's
/// edge between the same vertices to be that edge.
const LOOSE_SHARE: f64 = 10.0;

/// The distance under which two points of a file are the same point (files are written to about
/// 1e-7 of their size; exporters' tolerances are often 1e-6 to 1e-5 mm).
const JOIN: f64 = 1e-5;

/// The period of periodic surface coordinates.
const P: f64 = TAU;

/// The parameter of a point on a curve (`near`: the range to prefer for NURBS).
fn param_of(c: &Curve, p: &P3) -> f64 {
    match c {
        Curve::Line { o, d } => (p - o).dot(d) / d.norm_squared(),
        Curve::Circle { c, x, y, .. } => {
            let v = p - c;
            v.dot(y).atan2(v.dot(x))
        }
        Curve::Ellipse { c, x, y, a, b } => {
            let v = p - c;
            (v.dot(y) / b).atan2(v.dot(x) / a)
        }
        Curve::Nurbs(n) => {
            let (a, b) = n.range();
            c.closest(p, a, b)
        }
    }
}

/// The parameter range of an edge from `p0` to `p1` on `c` (`closed`: one vertex, a whole
/// curve). None when the edge would run against the curve (a non-periodic curve) or is empty.
pub fn edge_range(c: &Curve, p0: &P3, p1: &P3, closed: bool) -> Option<(f64, f64)> {
    match c {
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            let t0 = param_of(c, p0);
            if closed {
                return Some((t0, t0 + TAU));
            }
            let mut t1 = param_of(c, p1);
            while t1 <= t0 + tol::ROUNDOFF_FINE {
                t1 += TAU;
            }
            while t1 > t0 + TAU {
                t1 -= TAU;
            }
            Some((t0, t1))
        }
        Curve::Nurbs(n) if closed => Some(n.range()),
        Curve::Nurbs(n) => {
            let (a, b) = n.range();
            let (mut t0, mut t1) = (param_of(c, p0), param_of(c, p1));
            // A closed spline's closing point is both ends of its range: an edge from there starts
            // at the range's start and one to there ends at its end. (Taking the nearest end
            // instead, an edge that ends there ran backwards, and the caller turned the curve
            // round: the edge then went the other, long way round.)
            let (pa, pb) = (n.point(a), n.point(b));
            if (pa - pb).norm() < JOIN {
                if (p0 - pa).norm() < JOIN {
                    t0 = a;
                }
                if (p1 - pb).norm() < JOIN {
                    t1 = b;
                }
            }
            if t1 > t0 + tol::ROUNDOFF_FINE {
                Some((t0, t1))
            } else {
                None
            }
        }
        _ => {
            let (t0, t1) = (param_of(c, p0), param_of(c, p1));
            if t1 > t0 + tol::ROUNDOFF_FINE {
                Some((t0, t1))
            } else {
                None
            }
        }
    }
}

/// The analytic surface a surface of revolution or extrusion really is (exporters write tori as
/// revolved circles, cylinders as extruded circles, ...): a revolved line is a cylinder, cone or
/// plane, a revolved circle in a plane through the axis a torus or sphere; an extruded line is a
/// plane and a circle extruded along its normal a cylinder. None when it is none of these.
pub fn analytic(s: &Surface) -> Option<Surface> {
    const EPS: f64 = 1e-9;
    match s {
        // A B-spline that is exactly a plane, cylinder, cone or sphere.
        Surface::Nurbs(n) if std::env::var_os("CADRS_NO_RECOG_S").is_none() => {
            super::recognize::surface(n)
        }
        Surface::Revolution { f, m } => {
            let z = f.z.normalize();
            let foot = |p: &P3| f.o + z * (p - f.o).dot(&z);
            match m {
                Curve::Line { o, d } => {
                    let d = d.normalize();
                    let w = o - foot(o);
                    let r = w.norm();
                    if r < EPS {
                        return None;
                    }
                    let x = w / r;
                    if d.dot(&z.cross(&x)).abs() > EPS {
                        return None; // skew: a hyperboloid
                    }
                    let (dr, dz) = (d.dot(&x), d.dot(&z));
                    let fr = Frame::from_xz(foot(o), x, z);
                    Some(if dr.abs() < EPS {
                        Surface::Cylinder { f: fr, r }
                    } else if dz.abs() < EPS {
                        Surface::Plane { f: fr }
                    } else {
                        Surface::Cone {
                            f: fr,
                            r,
                            a: dr.atan2(dz),
                        }
                    })
                }
                Curve::Circle { c, x, y, r } => {
                    let n = x.cross(y);
                    if n.dot(&z).abs() > EPS
                        || (c - f.o).dot(&n).abs() > EPS * (1.0 + (c - f.o).norm())
                    {
                        return None;
                    }
                    let w = c - foot(c);
                    let rr = w.norm();
                    if rr < EPS * (1.0 + r) {
                        Some(Surface::Sphere {
                            f: Frame::from_xz(foot(c), f.x, z),
                            r: *r,
                        })
                    } else {
                        Some(Surface::Torus {
                            f: Frame::from_xz(foot(c), w / rr, z),
                            rmaj: rr,
                            rmin: *r,
                        })
                    }
                }
                _ => None,
            }
        }
        Surface::Extrusion { c, d } => match c {
            Curve::Line { o, d: ld } => {
                let x = ld.normalize();
                let y = d - x * d.dot(&x);
                if y.norm() < EPS {
                    return None;
                }
                Some(Surface::Plane {
                    f: Frame::from_xz(*o, x, x.cross(&y.normalize())),
                })
            }
            Curve::Circle { c, x, y, r } => {
                let n = x.cross(y).normalize();
                if d.normalize().cross(&n).norm() > EPS {
                    return None;
                }
                Some(Surface::Cylinder {
                    f: Frame::from_xz(*c, *x, n),
                    r: *r,
                })
            }
            _ => None,
        },
        _ => None,
    }
}

/// The same surface re-expressed so that a face through `p` lies where our projections look,
/// or None when it already does:
/// - a face on a cone's far nappe (beyond the apex, where `r + v sin a < 0`) gets the cone
///   opening the other way;
/// - a face on the inner (lemon) part of a spindle torus (R < r, where `R + r cos v < 0`) gets a
///   surface of revolution of the mirrored meridian circle.
fn near_branch(s: &Surface, p: &P3) -> Option<Surface> {
    match s {
        Surface::Cone { f, r, a } => {
            let l = f.local(p);
            let (sa, ca) = a.sin_cos();
            let radius = r + l.z * sa / ca;
            if radius >= -JOIN {
                return None;
            }
            Some(Surface::Cone {
                f: Frame::from_xz(f.o + f.z * l.z, f.x, -f.z),
                r: -radius,
                a: *a,
            })
        }
        Surface::Torus { f, rmaj, rmin } if rmaj < rmin => {
            let l = f.local(p);
            let rho = l.x.hypot(l.y);
            let apple = ((rho - rmaj).hypot(l.z) - rmin).abs();
            let lemon = ((rho + rmaj).hypot(l.z) - rmin).abs();
            if lemon >= apple {
                return None;
            }
            let m = Curve::Circle {
                c: f.o - f.x * *rmaj,
                x: f.x,
                y: f.z,
                r: *rmin,
            };
            Some(Surface::Revolution {
                f: Frame::from_xz(f.o, f.x, f.z),
                m,
            })
        }
        _ => None,
    }
}

/// Distance (on the surface) from a p-curve's point at `t` to the projection of the curve's.
fn pc_error(s: &Surface, per: &Periods, c: &Curve, pc: &Curve, t: f64, off: f64) -> f64 {
    let q = pc.point(t);
    let (u, v) = project(s, per, &c.point(t), Some((q.x, q.y)), off);
    let (qu, qv) = per.in_domain(q.x, q.y);
    let (u, v) = per.in_domain(u, v);
    (s.eval(qu, qv) - s.eval(u, v)).norm()
}

/// [`Surface::project`] near `hint` (across a closed spline surface's seam too, so successive
/// points stay continuous), but from a fresh start when that lands farther from the point
/// (Newton stuck where a spline surface collapses to a point, its Jacobian singular).
fn project(s: &Surface, per: &Periods, p: &P3, hint: Option<(f64, f64)>, off: f64) -> (f64, f64) {
    let near = hint.map(|(u, v)| per.in_domain(u, v));
    let (mut u, mut v) = s.project(p, near);
    if let Some(h) = near {
        if matches!(
            s,
            Surface::Nurbs(_) | Surface::Extrusion { .. } | Surface::Revolution { .. }
        ) {
            // Files' edges lie up to their tolerance (1e-4 mm and more) off their faces: only
            // retry when clearly off.
            let d = (s.eval(u, v) - p).norm();
            if d >= off {
                // From the nearest point of the surface's cached grid (a B-spline's), else from
                // the surface's own fresh search.
                let seeded = per.seed(s, p).map(|g| s.project(p, Some(g)));
                let (u2, v2) = match seeded {
                    Some(uv) if (s.eval(uv.0, uv.1) - p).norm() < off => uv,
                    _ => s.project(p, None),
                };
                if (s.eval(u2, v2) - p).norm() < d - tol::ROUNDOFF {
                    (u, v) = (u2, v2);
                }
            }
        }
        let _ = h;
    }
    if let (Some((hu, hv)), Some(_)) = (hint, per.domain) {
        if let Some(pu) = per.pu {
            u = unwrap_near(u, hu, pu);
        }
        if let Some(pv) = per.pv {
            v = unwrap_near(v, hv, pv);
        }
    }
    (u, v)
}

/// [`pcurve::pcurve`], checked against the projection of the curve (files' edges can be
/// degree-7 splines with C1 joints that its single fit doesn't follow); when it is off by more
/// than the modelling tolerance, a fit of our own, piece by piece of the curve.
pub fn pcurve_checked(s: &Surface, c: &Curve, t0: f64, t1: f64) -> Curve {
    pcurve_checked_tol(s, c, t0, t1, 0.0)
}

/// [`pcurve_checked`] for an edge of tolerance `etol`: a point of the curve as far off the face
/// as that is no reason to search the surface afresh.
pub fn pcurve_checked_tol(s: &Surface, c: &Curve, t0: f64, t1: f64, etol: f64) -> Curve {
    snap_tiny(pcurve_unsnapped(s, c, t0, t1, etol))
}

/// Zeroes coefficients below 1e-13 of the curve's size (rounding residue of files' 1e-16
/// direction components): the triangulator rejects coordinates under ~1.8e-43, which sums of
/// such residues (a seam's p-curve along u = 0) reach.
fn snap_tiny(c: Curve) -> Curve {
    let size = match &c {
        Curve::Line { o, d } => o.coords.amax().max(d.amax()),
        Curve::Circle { c, r, .. } => c.coords.amax().max(*r),
        Curve::Ellipse { c, a, b, .. } => c.coords.amax().max(a.max(*b)),
        Curve::Nurbs(n) => n
            .cps
            .iter()
            .map(|h| (h.x / h.w).abs().max((h.y / h.w).abs()))
            .fold(0.0, f64::max),
    };
    let tiny = 1e-13 * size.max(1.0);
    let z = |x: f64| if x.abs() < tiny { 0.0 } else { x };
    let p = |q: &P3| P3::new(z(q.x), z(q.y), z(q.z));
    let v = |q: &V3| V3::new(z(q.x), z(q.y), z(q.z));
    match c {
        Curve::Line { o, d } => Curve::Line { o: p(&o), d: v(&d) },
        Curve::Circle { c, x, y, r } => Curve::Circle {
            c: p(&c),
            x: v(&x),
            y: v(&y),
            r,
        },
        Curve::Ellipse { c, x, y, a, b } => Curve::Ellipse {
            c: p(&c),
            x: v(&x),
            y: v(&y),
            a,
            b,
        },
        Curve::Nurbs(n) => {
            let mut n = (*n).clone();
            for h in &mut n.cps {
                h.x = z(h.x);
                h.y = z(h.y);
                h.z = z(h.z);
            }
            Curve::Nurbs(Arc::new(n))
        }
    }
}

fn pcurve_unsnapped(s: &Surface, c: &Curve, t0: f64, t1: f64, etol: f64) -> Curve {
    pcurve_unsnapped_on(s, &periods(s), c, t0, t1, etol)
}

/// [`pcurve_unsnapped`] with the surface's [`Periods`] (its seed grid made once for all the
/// edges of a face).
fn pcurve_unsnapped_on(
    s: &Surface,
    per: &Periods,
    c: &Curve,
    t0: f64,
    t1: f64,
    etol: f64,
) -> Curve {
    let pieces = c.pieces(t0, t1);
    // Tolerances relative to the model's size (files come in metres-long models too).
    let size = {
        let mut bb = Bbox::empty();
        for &t in &pieces {
            bb.add(&c.point(t));
        }
        bb.add(&c.point((t0 + t1) / 2.0));
        bb.diagonal()
            .max(bb.max.coords.amax())
            .max(-bb.min.coords.min())
    };
    let tol = Tol {
        fit: TOL.max(1e-11 * size),
        off: 5e-4f64.max(1e-5 * size).max(2.0 * etol),
    };
    // An iso line of a spline surface (its seam, an offset's): the straight p-curve, exact where
    // a fit's ends at a pole would land anywhere along it.
    if matches!(s, Surface::Nurbs(_))
        && let Some(pc) = pcurve::iso_line(s, c, t0, t1)
    {
        return pc;
    }
    // A spline on a curved surface, or anything on a spline surface, has no exact p-curve: fit
    // it directly.
    if ((matches!(c, Curve::Nurbs(_)) && !matches!(s, Surface::Plane { .. }))
        || matches!(s, Surface::Nurbs(_)))
        && let Some(pc) = fit_pieces(s, per, c, &pieces, &tol)
    {
        return pc;
    }
    let mut pc = pcurve::pcurve(s, c, t0, t1, None);
    // A meridian arc past a sphere's pole is the same arc at u + π with v mirrored in the pole.
    if let (Surface::Sphere { .. }, Curve::Line { o, d }) = (s, &pc)
        && d.x == 0.0
    {
        // v is periodic along a whole meridian: bring the arc's middle into (−π, π] first.
        let vm = pc.point((t0 + t1) / 2.0).y;
        let k = ((vm - wrap(vm, -PI, TAU)) / TAU).round() * TAU;
        let (o, vm) = (P3::new(o.x, o.y - k, 0.0), vm - k);
        pc = if vm.abs() > PI / 2.0 + tol::ANGULAR {
            let pole = if vm > 0.0 { PI } else { -PI };
            Curve::Line {
                o: P3::new(o.x + PI, pole - o.y, 0.0),
                d: V3::new(0.0, -d.y, 0.0),
            }
        } else {
            Curve::Line { o, d: *d }
        };
    }
    if matches!(s, Surface::Plane { .. }) {
        return pc;
    }
    let samples = |_: &Curve| {
        pieces
            .windows(2)
            .flat_map(|w| [0.0, 0.25, 0.5, 0.75, 1.0].map(|f| w[0] + (w[1] - w[0]) * f))
            .collect::<Vec<f64>>()
    };
    if !matches!(pc, Curve::Nurbs(_)) {
        // An exact p-curve must put the surface on the curve.
        let on = |pc: &Curve| {
            samples(pc).iter().all(|&t| {
                let q = pc.point(t);
                (s.eval(q.x, q.y) - c.point(t)).norm() < tol::POINT
            })
        };
        if on(&pc) {
            return pc;
        }
        if let Some(m) = torus_meridian(s, c)
            && on(&m)
        {
            return m;
        }
    }
    if samples(&pc)
        .iter()
        .all(|&t| pc_error(s, per, c, &pc, t, tol.off) < tol.fit)
    {
        return pc;
    }
    fit_pieces(s, per, c, &pieces, &tol).unwrap_or(pc)
}

/// The p-curve of a torus's meridian (a tube circle), in the half plane of the circle's
/// centre: u constant, v = v0 ± t.
fn torus_meridian(s: &Surface, c: &Curve) -> Option<Curve> {
    let (Surface::Torus { f, rmaj, .. }, Curve::Circle { c: cc, x, y, .. }) = (s, c) else {
        return None;
    };
    let l = f.local(cc);
    if l.z.abs() > 1e-7
        || (l.x.hypot(l.y) - rmaj).abs() > 1e-7
        || x.cross(y).dot(&f.z).abs() > tol::ANGULAR
    {
        return None;
    }
    let radial = (f.x * l.x + f.y * l.y).normalize();
    let u = l.y.atan2(l.x);
    let (xr, xz, yr, yz) = (x.dot(&radial), x.dot(&f.z), y.dot(&radial), y.dot(&f.z));
    let sig = (xr * yz - xz * yr).signum();
    Some(Curve::Line {
        o: P3::new(u, xz.atan2(xr), 0.0),
        d: V3::new(0.0, sig, 0.0),
    })
}

/// Fitting tolerances: how close a p-curve must follow the projection, and the distance beyond
/// which a projection is taken to have failed.
struct Tol {
    fit: f64,
    off: f64,
}

/// A piecewise cubic p-curve: per piece of the curve, a cubic through a dozen projected
/// points, halved until its midpoints are within the tolerance; joined into one spline (and on
/// a closed spline surface moved by its period into its parameter range).
fn fit_pieces(s: &Surface, per: &Periods, c: &Curve, pieces: &[f64], tol: &Tol) -> Option<Curve> {
    let mut hint: Option<(f64, f64)> = None;
    let mut segs: Vec<NurbsCurve> = Vec::new();
    for w in pieces.windows(2) {
        if w[1] - w[0] > 1e-14 {
            fit_span(s, per, tol, c, w[0], w[1], &mut hint, 0, &mut segs);
        }
    }
    let out = join_spans(segs)?;
    let pc = Curve::Nurbs(Arc::new(out));
    if let Some(((u0, u1), (v0, v1))) = per.domain {
        let (a, b) = (pieces[0], *pieces.last().unwrap());
        let pts: Vec<P3> = (0..=16)
            .map(|i| pc.point(a + (b - a) * i as f64 / 16.0))
            .collect();
        let shift = |lo: f64, hi: f64, period: Option<f64>, get: &dyn Fn(&P3) -> f64| -> f64 {
            let Some(p) = period else { return 0.0 };
            let eps = tol::REL * (hi - lo).abs().max(1.0);
            let (mn, mx) = pts
                .iter()
                .map(get)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                    (a.min(x), b.max(x))
                });
            if mx > hi + eps {
                -((mx - hi - eps) / p).ceil() * p
            } else if mn < lo - eps {
                ((lo - eps - mn) / p).ceil() * p
            } else {
                0.0
            }
        };
        let du = shift(u0, u1, per.pu, &|q| q.x);
        let dv = shift(v0, v1, per.pv, &|q| q.y);
        if du != 0.0 || dv != 0.0 {
            return Some(pc.transformed(&Affine::translation(V3::new(du, dv, 0.0))));
        }
    }
    Some(pc)
}

/// Fitted spans joined end to end into one spline: a knot of full multiplicity at each joint,
/// the spans first raised to one degree (a span's interpolation falls back to degree one where
/// cubics fail). None for no spans.
fn join_spans(segs: Vec<NurbsCurve>) -> Option<NurbsCurve> {
    let mut it = segs.into_iter();
    let mut out = it.next()?;
    for mut fit in it {
        while fit.degree < out.degree {
            fit = fit.elevated();
        }
        while out.degree < fit.degree {
            out = out.elevated();
        }
        // Join at the shared end: a knot of full multiplicity there.
        let p = out.degree;
        out.knots.pop();
        out.knots.extend_from_slice(&fit.knots[p + 1..]);
        out.cps.extend_from_slice(&fit.cps[1..]);
    }
    Some(out)
}

#[allow(clippy::too_many_arguments)]
fn fit_span(
    s: &Surface,
    per: &Periods,
    tol: &Tol,
    c: &Curve,
    a: f64,
    b: f64,
    hint: &mut Option<(f64, f64)>,
    depth: usize,
    out: &mut Vec<NurbsCurve>,
) {
    const N: usize = 12;
    let mut h = *hint;
    let ts: Vec<f64> = (0..=N).map(|i| a + (b - a) * i as f64 / N as f64).collect();
    let pts: Vec<P3> = ts
        .iter()
        .map(|&t| {
            let (u, v) = project(s, per, &c.point(t), h, tol.off);
            h = Some((u, v));
            P3::new(u, v, 0.0)
        })
        .collect();
    let pts = settle_pole_samples(s, pts);
    let f = NurbsCurve::interpolate(&pts, &ts, 3, None, None);
    let fc = Curve::Nurbs(Arc::new(f));
    let err = (0..N)
        .map(|i| pc_error(s, per, c, &fc, (ts[i] + ts[i + 1]) / 2.0, tol.off))
        .fold(0.0, f64::max);
    if err < tol.fit || depth >= 12 {
        *hint = h;
        let Curve::Nurbs(f) = fc else { unreachable!() };
        out.push(Arc::try_unwrap(f).unwrap_or_else(|a| (*a).clone()));
        return;
    }
    let m = (a + b) / 2.0;
    fit_span(s, per, tol, c, a, m, hint, depth + 1, out);
    fit_span(s, per, tol, c, m, b, hint, depth + 1, out);
}

/// On an iso line through a pole (a meridian of a spline surface of revolution, from pole to
/// pole: every sample off the poles at one u, or one v), the samples at the poles (where the
/// surface collapses: every u, or every v, the same point) take that coordinate too: a
/// projection there lands anywhere along the pole, and a fit through it would swing the p-curve
/// off the curve between. Other curves' samples are left as they are.
fn settle_pole_samples(s: &Surface, mut pts: Vec<P3>) -> Vec<P3> {
    // (Collapsed: one derivative vanishing against the other, a ten-millionth of it.)
    let collapsed = |q: &P3| -> (bool, bool) {
        let (_, su, sv) = s.d1(q.x, q.y);
        let (a, b) = (su.norm(), sv.norm());
        (a <= 1e-7 * b, b <= 1e-7 * a)
    };
    let flags: Vec<(bool, bool)> = pts.iter().map(collapsed).collect();
    for dir in 0..2 {
        let at = |f: &(bool, bool)| if dir == 0 { f.0 } else { f.1 };
        let Some(first) = flags.iter().position(|f| !at(f)) else {
            continue;
        };
        let last = flags.iter().rposition(|f| !at(f)).unwrap_or(first);
        let c = |q: &P3| if dir == 0 { q.x } else { q.y };
        let iso = pts
            .iter()
            .zip(&flags)
            .filter(|(_, f)| !at(f))
            .all(|(q, _)| (c(q) - c(&pts[first])).abs() <= tol::REL * (1.0 + c(&pts[first]).abs()));
        if !iso {
            continue;
        }
        for i in 0..pts.len() {
            if !at(&flags[i]) {
                continue;
            }
            // From the nearer end's sample off the pole (inside a run, the one before).
            let from = if i < first {
                first
            } else if i > last {
                last
            } else {
                (0..i).rev().find(|&j| !at(&flags[j])).unwrap_or(first)
            };
            if dir == 0 {
                pts[i].x = pts[from].x;
            } else {
                pts[i].y = pts[from].y;
            }
        }
    }
    pts
}

/// How a surface closes on itself: periods in u and v, and for spline surfaces (closed where
/// their opposite boundaries meet, but not periodic in evaluation) the parameter range p-curves
/// must stay in.
struct Periods {
    pu: Option<f64>,
    pv: Option<f64>,
    domain: Option<((f64, f64), (f64, f64))>,
    /// A B-spline surface's points on a 25 × 25 grid of its range, made the first time a
    /// projection needs a fresh start (one edge's fit projects many points).
    grid: std::cell::OnceCell<Vec<(f64, f64, P3)>>,
}

impl Periods {
    /// The (u, v) of the grid point nearest `p` on a B-spline surface (its grid made once).
    fn seed(&self, s: &Surface, p: &P3) -> Option<(f64, f64)> {
        let Surface::Nurbs(n) = s else { return None };
        let grid = self.grid.get_or_init(|| {
            let ((u0, u1), (v0, v1)) = n.range();
            const G: usize = 24;
            let at = |k: usize, lo: f64, hi: f64| lo + (hi - lo) * k as f64 / G as f64;
            (0..=G)
                .flat_map(|i| (0..=G).map(move |j| (i, j)))
                .map(|(i, j)| {
                    (
                        at(i, u0, u1),
                        at(j, v0, v1),
                        n.point(at(i, u0, u1), at(j, v0, v1)),
                    )
                })
                .collect()
        });
        grid.iter()
            .min_by(|a, b| {
                (a.2 - p)
                    .norm_squared()
                    .total_cmp(&(b.2 - p).norm_squared())
            })
            .map(|g| (g.0, g.1))
    }

    /// (u, v) moved by periods into a closed spline surface's parameter range (where it
    /// evaluates), unchanged elsewhere.
    fn in_domain(&self, u: f64, v: f64) -> (f64, f64) {
        let Some(((u0, u1), (v0, v1))) = self.domain else {
            return (u, v);
        };
        let f = |x: f64, lo: f64, hi: f64, p: Option<f64>| -> f64 {
            match p {
                Some(p) if x < lo - tol::REL * p || x > hi + tol::REL * p => wrap(x, lo, p),
                _ => x,
            }
        };
        (f(u, u0, u1, self.pu), f(v, v0, v1, self.pv))
    }
}

/// A B-spline surface closed in u (`dir` 0) or v (1) restarted at `at`: every row of poles along
/// that direction restarted there (`NurbsCurve::restarted`), the surface's range there then
/// [at, at + period]. None when a row's poles at the seam don't match (weights scaled
/// differently: no common restart).
fn restarted(n: &NurbsSurface, dir: usize, at: f64) -> Option<NurbsSurface> {
    let (nu, nv) = (n.cps.len(), n.cps.first()?.len());
    let (deg, knots, rows, len) = if dir == 0 {
        (n.du, &n.ku, nv, nu)
    } else {
        (n.dv, &n.kv, nu, nv)
    };
    let pole = |r: usize, i: usize| if dir == 0 { n.cps[i][r] } else { n.cps[r][i] };
    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        let c = NurbsCurve {
            degree: deg,
            knots: knots.clone(),
            cps: (0..len).map(|i| pole(r, i)).collect(),
        };
        // The seam's poles must be the same homogeneous point (the restart joins them as one).
        let (first, last) = (c.cps.first()?, c.cps.last()?);
        if (first - last).norm() > tol::REL * (1.0 + first.norm()) {
            return None;
        }
        out.push(c.restarted(at).ok()?);
    }
    let k = out[0].knots.clone();
    if out.iter().any(|c| c.knots != k) {
        return None;
    }
    let m = out[0].cps.len();
    let (ku, kv, cps) = if dir == 0 {
        (
            k,
            n.kv.clone(),
            (0..m)
                .map(|i| out.iter().map(|c| c.cps[i]).collect())
                .collect(),
        )
    } else {
        (n.ku.clone(), k, out.iter().map(|c| c.cps.clone()).collect())
    };
    NurbsSurface::new(n.du, n.dv, ku, kv, cps).ok()
}

fn periods(s: &Surface) -> Periods {
    match s {
        Surface::Nurbs(n) => {
            let ((u0, u1), (v0, v1)) = n.range();
            let closed_u = (0..=4).all(|k| {
                let v = v0 + (v1 - v0) * k as f64 / 4.0;
                (n.point(u0, v) - n.point(u1, v)).norm() < JOIN
            });
            let closed_v = (0..=4).all(|k| {
                let u = u0 + (u1 - u0) * k as f64 / 4.0;
                (n.point(u, v0) - n.point(u, v1)).norm() < JOIN
            });
            Periods {
                pu: closed_u.then_some(u1 - u0),
                pv: closed_v.then_some(v1 - v0),
                domain: Some(((u0, u1), (v0, v1))),
                grid: Default::default(),
            }
        }
        Surface::Extrusion {
            c: Curve::Nurbs(n), ..
        } => {
            let (a, b) = n.range();
            let closed = (n.point(a) - n.point(b)).norm() < JOIN;
            Periods {
                pu: closed.then_some(b - a),
                pv: None,
                domain: Some(((a, b), (f64::NEG_INFINITY, f64::INFINITY))),
                grid: Default::default(),
            }
        }
        _ => Periods {
            pu: s.u_periodic().then_some(P),
            pv: s.v_periodic().then_some(P),
            domain: None,
            grid: Default::default(),
        },
    }
}

/// The u = const iso line of a periodic surface, parametrized by v.
fn iso_u(s: &Surface, u: f64) -> Option<Curve> {
    match s {
        Surface::Revolution { f, m } => {
            let r = Frame {
                o: f.o,
                x: f.x * u.cos() + f.y * u.sin(),
                y: f.y * u.cos() - f.x * u.sin(),
                z: f.z,
            };
            // The meridian turned by u about the axis (in the frame's sense of rotation).
            let to = nalgebra::Matrix3::from_columns(&[r.x, r.y, r.z]);
            let from = nalgebra::Matrix3::from_columns(&[f.x, f.y, f.z]).transpose();
            let m3 = to * from;
            Some(m.transformed(&Affine {
                m: m3,
                t: f.o.coords - m3 * f.o.coords,
            }))
        }
        Surface::Extrusion { c, d } => Some(Curve::Line {
            o: c.point(u),
            d: *d,
        }),
        _ => s.iso_u(u),
    }
}

/// One loop traced in uv: per coedge its (aligned) p-curve, and per joint (joint j is where
/// coedge j starts; joint 0 closes the loop) the gap from the previous end.
struct Trace {
    pcs: Vec<Curve>,
    /// Start and end uv of each coedge.
    ends: Vec<(P2, P2)>,
    /// Per joint: the gap (start of j − end of j − 1, joint 0 reduced by whole periods), whether
    /// the surface collapses there along u (a pole or apex), and the joint's vertex.
    gaps: Vec<V2>,
    free: Vec<bool>,
    /// The same for v: the surface collapses along v there (a cone's apex as a row of a
    /// B-spline closed in v).
    free_v: Vec<bool>,
    /// Net turns about u and v (with every free gap at its smallest).
    wu: i64,
    wv: i64,
    /// The period in u the shifts and free jumps are counted in (2π, or a closed B-spline's
    /// range).
    pu: f64,
    pv: f64,
    /// Per coedge, the whole periods (in u) it is shifted by before any free jump: the jumps of
    /// a closed spline's p-curves kept in its range.
    base: Vec<i64>,
    /// The same in v.
    base_v: Vec<i64>,
}

impl XBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertex(&mut self, p: P3) -> usize {
        self.body.add_vertex(p)
    }

    /// An edge from `v0` to `v1` along `curve` (turned round when it runs from `v1` to `v0`).
    /// None for an empty edge.
    pub fn edge(&mut self, curve: Curve, v0: usize, v1: usize) -> Option<usize> {
        // A B-spline that is exactly a line or circle.
        let curve = match &curve {
            Curve::Nurbs(n) if std::env::var_os("CADRS_NO_RECOG_C").is_none() => {
                super::recognize::curve(n).unwrap_or(curve)
            }
            _ => curve,
        };
        let (p0, p1) = (self.body.verts[v0].p, self.body.verts[v1].p);
        let closed = v0 == v1;
        let (curve, (t0, t1)) = match edge_range(&curve, &p0, &p1, closed) {
            Some(r) => (curve, r),
            None => {
                if (p0 - p1).norm() < JOIN
                    && !curve.is_periodic()
                    && !matches!(curve, Curve::Nurbs(_))
                {
                    return None;
                }
                let (rc, _, _) = curve.reversed(0.0, 1.0);
                match edge_range(&rc, &p0, &p1, closed) {
                    Some(r) => (rc, r),
                    // A closed spline the edge runs across its start: restarted at the edge's
                    // first vertex (either way round).
                    None => [curve, rc].into_iter().find_map(|c| {
                        let Curve::Nurbs(n) = &c else { return None };
                        let (a, b) = n.range();
                        if (n.point(a) - n.point(b)).norm() >= JOIN {
                            return None;
                        }
                        let rs = Curve::Nurbs(Arc::new(n.restarted(param_of(&c, &p0)).ok()?));
                        edge_range(&rs, &p0, &p1, closed).map(|r| (rs, r))
                    })?,
                }
            }
        };
        // A corrupt file's curve (a 1e308 put into it) whose points aren't numbers: no edge, as
        // every projection onto the faces would trip on it.
        let finite = |t: f64| t.is_finite() && curve.point(t).coords.iter().all(|x| x.is_finite());
        if !(finite(t0) && finite(t1) && finite((t0 + t1) / 2.0)) {
            return None;
        }
        // Empty? (A sampled length: exact lengths of long splines cost.)
        let len: f64 = (0..8)
            .map(|i| {
                (curve.point(t0 + (t1 - t0) * (i + 1) as f64 / 8.0)
                    - curve.point(t0 + (t1 - t0) * i as f64 / 8.0))
                .norm()
            })
            .sum();
        if len < tol::ROUNDOFF {
            return None;
        }
        // Rounding residue (atan2(1e-49, 1)) would reach the triangulator, which rejects it.
        let small = tol::REL_FINE * (1.0 + (t1 - t0).abs());
        let z = |t: f64| if t.abs() < small { 0.0 } else { t };
        let (t0, t1) = (z(t0), z(t1));
        Some(self.body.add_edge(curve, t0, t1, v0, v1))
    }

    pub fn face(&mut self, f: XFace) -> usize {
        self.faces.push(f);
        self.faces.len() - 1
    }

    pub fn shell(&mut self, faces: Vec<usize>, closed: bool) -> usize {
        self.shells.push((faces, closed));
        self.shells.len() - 1
    }

    pub fn solid(&mut self, shells: Vec<usize>) {
        self.solids.push(shells);
    }

    fn co_start(&self, e: usize, fwd: bool) -> usize {
        let ed = &self.body.edges[e];
        if fwd { ed.v[0] } else { ed.v[1] }
    }

    /// The p-curve of edge `e` on `s`, shifted by periods so its start (along `fwd`) is nearest
    /// `at` (on a closed spline surface only within its parameter range: a seam's two uses lie
    /// on its two boundaries).
    fn pc_from(&self, fi: usize, s: &Surface, e: usize, fwd: bool, at: Option<P2>) -> Curve {
        let ed = &self.body.edges[e];
        let per = periods(s);
        let key = (fi, e);
        let cached = self.pc_cache.borrow().get(&key).cloned();
        let pc = match cached {
            Some(pc) => pc,
            None => {
                let pc = self.pc_raw(s, e);
                self.pc_cache.borrow_mut().insert(key, pc.clone());
                pc
            }
        };
        let Some(at) = at else { return pc };
        let (t0, _) = co_range(ed, fwd);
        let p = pc.point(t0);
        let mut du = per.pu.map_or(0.0, |pu| ((at.x - p.x) / pu).round() * pu);
        let mut dv = per.pv.map_or(0.0, |pv| ((at.y - p.y) / pv).round() * pv);
        if let Some(((u0, u1), (v0, v1))) = per.domain {
            let inside = |x: f64, lo: f64, hi: f64| {
                x >= lo - tol::REL * (hi - lo).abs().max(1.0)
                    && x <= hi + tol::REL * (hi - lo).abs().max(1.0)
            };
            let ts = [ed.t[0], ed.mid_t(), ed.t[1]];
            if !ts.iter().all(|&t| inside(pc.point(t).x + du, u0, u1)) {
                du = 0.0;
            }
            if !ts.iter().all(|&t| inside(pc.point(t).y + dv, v0, v1)) {
                dv = 0.0;
            }
        }
        if du == 0.0 && dv == 0.0 {
            pc
        } else {
            pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)))
        }
    }

    /// The p-curves of every face's edges (into the cache the traces read), the faces in
    /// parallel (research/algorithms/offsets_sweeps_import.md card 4): each a function of the
    /// face's surface and the edge alone, so the same whatever the threads.
    fn fit_pcurves(&mut self, dropped: &[bool]) {
        use rayon::prelude::*;
        let edges = &self.body.edges;
        super::par::start();
        let fits: Vec<Vec<((usize, usize), Curve)>> = self
            .faces
            .par_iter()
            .enumerate()
            .filter(|(fi, _)| !dropped[*fi])
            .map(|(fi, f)| {
                let per = periods(&f.surf);
                let mut seen = std::collections::HashSet::new();
                f.loops
                    .iter()
                    .flatten()
                    .filter(|(e, _)| seen.insert(*e))
                    .map(|&(e, _)| {
                        let ed = &edges[e];
                        (
                            (fi, e),
                            snap_tiny(pcurve_unsnapped_on(
                                &f.surf, &per, &ed.curve, ed.t[0], ed.t[1], ed.tol,
                            )),
                        )
                    })
                    .collect()
            })
            .collect();
        self.pc_cache.get_mut().extend(fits.into_iter().flatten());
    }

    /// The p-curve of edge `e` on `s`.
    fn pc_raw(&self, s: &Surface, e: usize) -> Curve {
        let ed = &self.body.edges[e];
        pcurve_checked_tol(s, &ed.curve, ed.t[0], ed.t[1], ed.tol)
    }

    /// Whether moving along u at `uv` leaves the surface point in place (a pole or an apex):
    /// round a periodic surface, or across a closed B-spline's whole range.
    fn collapsed_u(s: &Surface, per: &Periods, uv: P2) -> bool {
        let us: Vec<f64> = match per.domain {
            Some(((u0, u1), _)) => [0.0, 0.25, 0.5, 0.75, 1.0]
                .iter()
                .map(|f| u0 + (u1 - u0) * f)
                .collect(),
            None => [PI / 2.0, PI, -PI / 2.0]
                .iter()
                .map(|du| uv.x + du)
                .collect(),
        };
        let p = s.eval(uv.x, uv.y);
        us.iter().all(|&u| (s.eval(u, uv.y) - p).norm() < JOIN)
    }

    /// Whether moving along v at `uv` leaves the surface point in place.
    fn collapsed_v(s: &Surface, per: &Periods, uv: P2) -> bool {
        let vs: Vec<f64> = match per.domain {
            Some((_, (v0, v1))) => [0.0, 0.25, 0.5, 0.75, 1.0]
                .iter()
                .map(|f| v0 + (v1 - v0) * f)
                .collect(),
            None => [PI / 2.0, PI, -PI / 2.0]
                .iter()
                .map(|dv| uv.y + dv)
                .collect(),
        };
        let p = s.eval(uv.x, uv.y);
        vs.iter().all(|&v| (s.eval(uv.x, v) - p).norm() < JOIN)
    }

    fn trace(&self, fi: usize, s: &Surface, l: &[(usize, bool)]) -> Trace {
        let mut pcs = Vec::with_capacity(l.len());
        let mut ends: Vec<(P2, P2)> = Vec::with_capacity(l.len());
        let mut at: Option<P2> = None;
        let per = periods(s);
        // Joints where the loop runs out along a seam to a pole and straight back along it: it
        // goes round the pole there, so the way back is the seam's other side, a period over.
        let mut round_pole = vec![false; l.len()];
        for (k, &(e, fwd)) in l.iter().enumerate() {
            let back = k > 0 && l[k - 1] == (e, !fwd);
            let from = match (at, back) {
                (Some(p), true) => {
                    // A period over, within a closed spline surface's range.
                    let over = |x: f64, p: f64, range: Option<(f64, f64)>| match range {
                        Some((_, hi)) if x + p > hi + tol::REL * p => x - p,
                        _ => x + p,
                    };
                    // (From where the way out started: along the pole its p-curve may drift.)
                    let dom = per.domain;
                    let start: P2 = ends[k - 1].0;
                    if let Some(pv) = per.pv.filter(|_| Self::collapsed_v(s, &per, p)) {
                        round_pole[k] = true;
                        Some(P2::new(p.x, over(start.y, pv, dom.map(|d| d.1))))
                    } else if let Some(pu) = per.pu.filter(|_| Self::collapsed_u(s, &per, p)) {
                        round_pole[k] = true;
                        Some(P2::new(over(start.x, pu, dom.map(|d| d.0)), p.y))
                    } else {
                        Some(p)
                    }
                }
                (a, _) => a,
            };
            let pc = self.pc_from(fi, s, e, fwd, from);
            let ed = &self.body.edges[e];
            let (a, b) = co_range(ed, fwd);
            let (pa, pb) = (pc.point(a), pc.point(b));
            ends.push((P2::new(pa.x, pa.y), P2::new(pb.x, pb.y)));
            at = Some(P2::new(pb.x, pb.y));
            pcs.push(pc);
        }
        let n = l.len();
        let mut gaps = Vec::with_capacity(n);
        let mut free = Vec::with_capacity(n);
        let mut free_v = Vec::with_capacity(n);
        let (mut wu, mut wv) = (0i64, 0i64);
        // On a closed spline surface the p-curves stay in its range (`pc_from`), so a loop
        // crossing the seam inside a coedge's run (a whole circle starting at the far side) jumps
        // by a whole period from one coedge to the next: the coedges after it are a period over
        // (`base`), and the jump is no gap.
        let mut base = vec![0i64; n];
        let pu_dom = per.pu.filter(|_| per.domain.is_some());
        if let Some(pu) = pu_dom {
            for j in 1..n {
                base[j] = base[j - 1];
                let g = ends[j].0.x - ends[j - 1].1.x + (base[j] - base[j - 1]) as f64 * pu;
                let k = (g / pu).round();
                // (Not the jump round a pole: a jump in the loop, which a pole edge spans.)
                if k != 0.0 && (g - k * pu).abs() <= 1e-7 * pu && !round_pole[j] {
                    base[j] -= k as i64;
                }
            }
        }
        let mut base_v = vec![0i64; n];
        if let Some(pv) = per.pv.filter(|_| per.domain.is_some()) {
            for j in 1..n {
                base_v[j] = base_v[j - 1];
                let g = ends[j].0.y - ends[j - 1].1.y;
                let k = (g / pv).round();
                if k != 0.0 && (g - k * pv).abs() <= 1e-7 * pv && !round_pole[j] {
                    base_v[j] -= k as i64;
                }
            }
        }
        let off = |k: usize| {
            V2::new(
                base[k] as f64 * per.pu.unwrap_or(0.0),
                base_v[k] as f64 * per.pv.unwrap_or(0.0),
            )
        };
        for j in 0..n {
            let pk = (j + n - 1) % n;
            let prev = ends[pk].1 + off(pk);
            let mut g = ends[j].0 + off(j) - prev;
            if j == 0 {
                if let Some(pu) = per.pu {
                    let k = (g.x / pu).round();
                    g.x -= k * pu;
                    wu -= k as i64;
                }
                if let Some(pv) = per.pv {
                    let k = (g.y / pv).round();
                    g.y -= k * pv;
                    wv -= k as i64;
                }
            }
            gaps.push(g);
            free.push(per.pu.is_some() && Self::collapsed_u(s, &per, prev));
            free_v.push(per.pv.is_some() && Self::collapsed_v(s, &per, prev));
        }
        Trace {
            pcs,
            ends,
            gaps,
            free,
            free_v,
            wu,
            wv,
            pu: per.pu.unwrap_or(P),
            pv: per.pv.unwrap_or(P),
            base,
            base_v,
        }
    }

    /// Signed uv area (scaled by the surface metric) of a traced loop with per-coedge period
    /// shifts `n` (in u).
    fn trace_area(
        s: &Surface,
        body: &Body,
        l: &[(usize, bool)],
        t: &Trace,
        n: &[i64],
        nv: &[i64],
    ) -> f64 {
        let (mu, mv) = s.metric();
        let mut pts = Vec::new();
        for (k, &(e, fwd)) in l.iter().enumerate() {
            let ed = &body.edges[e];
            let (a, b) = co_range(ed, fwd);
            let m = match t.pcs[k] {
                Curve::Line { .. } => 1,
                _ => 32,
            };
            let du = (n[k] + t.base[k]) as f64 * t.pu;
            let dv = (nv[k] + t.base_v[k]) as f64 * t.pv;
            // Both ends: a jump at a pole joins one coedge's end to the next one's start.
            for i in 0..=m {
                let q = t.pcs[k].point(a + (b - a) * i as f64 / m as f64);
                pts.push(P2::new((q.x + du) * mu, (q.y + dv) * mv));
            }
        }
        polygon_area(&pts)
    }

    /// Settles the free (pole) jumps of a traced loop: per coedge its shift in whole periods of
    /// u. None when the loop wraps around the surface with nothing to absorb it (a seam is
    /// missing).
    fn resolve(
        &self,
        s: &Surface,
        l: &[(usize, bool)],
        t: &Trace,
        hint: bool,
    ) -> Option<(Vec<i64>, Vec<i64>)> {
        let n = l.len();
        let zero = vec![0; n];
        if t.wv == 0 {
            let gaps: Vec<f64> = t.gaps.iter().map(|g| g.x).collect();
            return Self::resolve_along(hint, &t.free, &gaps, t.wu, t.pu, |sh| {
                Self::trace_area(s, &self.body, l, t, sh, &zero)
            })
            .map(|u| (u, zero.clone()));
        }
        // Turning in v with poles across v (a B-spline cone closed in v, its apex a row).
        if t.wu == 0 && t.free_v.iter().any(|&f| f) {
            let gaps: Vec<f64> = t.gaps.iter().map(|g| g.y).collect();
            return Self::resolve_along(hint, &t.free_v, &gaps, t.wv, t.pv, |sh| {
                Self::trace_area(s, &self.body, l, t, &zero, sh)
            })
            .map(|v| (zero.clone(), v));
        }
        None
    }

    /// [`Self::resolve`] along one direction: `free` joints, their `gaps` along it, the turns
    /// `w` and the period `p`; `area` of the loop with per-coedge shifts.
    fn resolve_along(
        hint: bool,
        free: &[bool],
        gaps: &[f64],
        w: i64,
        p: f64,
        area: impl Fn(&[i64]) -> f64,
    ) -> Option<Vec<i64>> {
        let n = free.len();
        let free: Vec<usize> = (0..n).filter(|&j| free[j]).collect();
        // Shifts from per-joint extra jumps m_j: coedge k is shifted by the sum of m_j, j in 1..=k.
        let shifts = |m: &[i64]| -> Vec<i64> {
            let mut acc = 0;
            (0..n)
                .map(|k| {
                    if k > 0
                        && let Some(i) = free.iter().position(|&j| j == k)
                    {
                        acc += m[i];
                    }
                    acc
                })
                .collect()
        };
        if free.is_empty() {
            return if w == 0 { Some(vec![0; n]) } else { None };
        }
        // The jumps must add up to −w (so the loop closes) and each stay within a period.
        let mut best: Option<(bool, f64, Vec<i64>)> = None;
        let nf = free.len().min(3);
        let range: Vec<i64> = vec![-2, -1, 0, 1, 2];
        let mut combos: Vec<Vec<i64>> = vec![vec![]];
        for _ in 0..nf {
            combos = combos
                .into_iter()
                .flat_map(|c| range.iter().map(move |&x| [c.clone(), vec![x]].concat()))
                .collect();
        }
        for mut m in combos {
            m.resize(free.len(), 0);
            if m.iter().sum::<i64>() != -w {
                continue;
            }
            let ok = free
                .iter()
                .zip(&m)
                .all(|(&j, &mj)| (gaps[j] + mj as f64 * p).abs() <= p * (1.0 + tol::REL));
            if !ok {
                continue;
            }
            let sh = shifts(&m);
            let a = area(&sh);
            if a.abs() < tol::DEGENERATE {
                continue;
            }
            let matches = (a > 0.0) == hint;
            let better = match &best {
                None => true,
                Some((bm, ba, _)) => {
                    (matches && !bm)
                        || (matches == *bm
                            && if matches {
                                a.abs() < *ba
                            } else {
                                a.abs() > *ba
                            })
                }
            };
            if better {
                best = Some((matches, a.abs(), sh));
            }
        }
        best.map(|b| b.2)
    }

    /// Whether two vertices are the same point.
    fn same_vertex(&self, a: usize, b: usize) -> bool {
        a == b || (self.body.verts[a].p - self.body.verts[b].p).norm() < JOIN
    }

    fn chains(&self, l: &[(usize, bool)]) -> bool {
        let n = l.len();
        (0..n).all(|k| {
            let (e, f) = l[k];
            let (e2, f2) = l[(k + 1) % n];
            let end = self.co_start(e, !f);
            self.same_vertex(end, self.co_start(e2, f2))
        })
    }

    /// Makes a loop chain end to start, as some files' don't: coedges that only chain the other
    /// way round are turned (wrong orientation flags), and failing that the loop is re-chained
    /// from its first coedge by shared vertices (edges listed out of order). Returns whether the
    /// loop chains.
    fn repair_loop(&self, l: &mut Vec<(usize, bool)>) -> bool {
        if l.len() < 2 || self.chains(l) {
            return true;
        }
        let flips: &[bool] = if self.trust_orientation {
            &[]
        } else {
            &[false, true]
        };
        for &turn in flips {
            let first = l[0].1 ^ turn;
            let mut m = l.clone();
            m[0].1 = first;
            for k in 1..m.len() {
                let prev_end = self.co_start(m[k - 1].0, !m[k - 1].1);
                if !self.same_vertex(prev_end, self.co_start(m[k].0, m[k].1))
                    && self.same_vertex(prev_end, self.co_start(m[k].0, !m[k].1))
                {
                    m[k].1 = !m[k].1;
                }
            }
            if self.chains(&m) {
                *l = m;
                return true;
            }
        }
        // Re-chain greedily.
        let mut rest: Vec<(usize, bool)> = l[1..].to_vec();
        let mut m = vec![l[0]];
        while !rest.is_empty() {
            let (e, f) = *m.last().unwrap();
            let end = self.co_start(e, !f);
            // The edges that go on from here: as given before turned round, and another edge
            // before going straight back along the same one (a seam's two uses are apart, with
            // the closed edges at its ends between them); of those, one that closes on itself
            // first (taken later, the chain would leave it behind).
            let Some((_, _, i, flip)) = rest
                .iter()
                .enumerate()
                .filter_map(|(i, &(e2, f2))| {
                    let back = (e2 == e) as u8 * 2;
                    let open = !self.same_vertex(self.co_start(e2, f2), self.co_start(e2, !f2));
                    if self.same_vertex(end, self.co_start(e2, f2)) {
                        Some((back, open, i, false))
                    } else if !self.trust_orientation
                        && self.same_vertex(end, self.co_start(e2, !f2))
                    {
                        Some((back + 1, open, i, true))
                    } else {
                        None
                    }
                })
                .min_by_key(|&(score, open, i, _)| (score, open, i))
            else {
                return false;
            };
            let (e2, f2) = rest.remove(i);
            m.push((e2, f2 ^ flip));
        }
        if self.chains(&m) {
            *l = m;
            true
        } else {
            false
        }
    }

    /// Splits edge `e` at `t` in every face description using it; returns the new vertex.
    fn split_edge(&mut self, e: usize, t: f64) -> usize {
        // (Only the split edge's p-curves change: the others' edges stay as they are.)
        self.pc_cache.borrow_mut().retain(|k, _| k.1 != e);
        let p = self.body.edges[e].point(t);
        let v = self.body.add_vertex(p);
        let old = self.body.edges[e].clone();
        self.body.edges[e] = Edge {
            t: [old.t[0], t],
            v: [old.v[0], v],
            ..old.clone()
        };
        self.body.edges.push(Edge {
            t: [t, old.t[1]],
            v: [v, old.v[1]],
            ..old
        });
        let e2 = self.body.edges.len() - 1;
        for f in &mut self.faces {
            for l in &mut f.loops {
                let mut nl = Vec::with_capacity(l.len() + 1);
                for &(x, fwd) in l.iter() {
                    if x == e {
                        if fwd {
                            nl.push((e, true));
                            nl.push((e2, true));
                        } else {
                            nl.push((e2, false));
                            nl.push((e, false));
                        }
                    } else {
                        nl.push((x, fwd));
                    }
                }
                *l = nl;
            }
        }
        v
    }

    /// A vertex at `p`: an existing one within the join distance, else a new one.
    fn vertex_at(&mut self, p: P3) -> usize {
        if let Some(i) = self.body.verts.iter().position(|v| (v.p - p).norm() < JOIN) {
            return i;
        }
        self.body.add_vertex(p)
    }

    /// Where a loop reaches u ≡ `u` (mod 2π): a vertex (the index of the coedge starting there)
    /// or a point inside a coedge (its index and edge parameter).
    fn find_u(&self, l: &[(usize, bool)], t: &Trace, u: f64) -> Option<(usize, Option<f64>)> {
        let off = |x: f64| wrap(x - u, -PI, P);
        for (k, &(e, fwd)) in l.iter().enumerate() {
            if off(t.ends[k].0.x).abs() < tol::ROUNDOFF {
                return Some((k, None));
            }
            let ed = &self.body.edges[e];
            let (a, b) = co_range(ed, fwd);
            let m = 64;
            let f = |s: f64| off(t.pcs[k].point(a + (b - a) * s).x);
            let mut prev = f(0.0);
            for i in 1..=m {
                let s1 = i as f64 / m as f64;
                let cur = f(s1);
                if prev.abs() < PI / 2.0 && cur.abs() < PI / 2.0 && prev * cur <= 0.0 {
                    if cur.abs() < tol::ROUNDOFF_FINE && i == m {
                        break; // the next coedge's start
                    }
                    let (mut lo, mut hi) = ((i - 1) as f64 / m as f64, s1);
                    for _ in 0..80 {
                        let mid = (lo + hi) / 2.0;
                        if (f(mid) > 0.0) == (f(lo) > 0.0) {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    return Some((k, Some(a + (b - a) * (lo + hi) / 2.0)));
                }
                prev = cur;
            }
        }
        None
    }

    /// Adds an edge along the iso line `c` (parametrized by v) between vertices at v = va and
    /// v = vb; returns the coedge from a to b.
    fn iso_edge(&mut self, c: Curve, a: usize, va: f64, b: usize, vb: f64) -> (usize, bool) {
        if va <= vb {
            (self.body.add_edge(c, va, vb, a, b), true)
        } else {
            (self.body.add_edge(c, vb, va, b, a), false)
        }
    }

    /// Gives a face whose loops wrap around its surface a seam. Returns false when it can't.
    /// A face on a B-spline surface closed in u or v (a revolved profile as a spline) whose
    /// loops, traced across the surface's seam, run past its range (a loop closing round its
    /// poles, its meridians on both sides of the seam): its p-curves moved by whole periods to
    /// start in the range and, where they still run past its end, the surface restarted at the
    /// loops' lowest coordinate (`NurbsCurve::restarted` along every row or column of poles: the
    /// same surface, its seam moved out of the face), so the p-curves are on the surface as they
    /// stand. Else the surface as it is.
    fn fit_closed_range(body: &Body, s: &Surface, loops: &mut [(f64, Vec<Coedge>)]) -> Surface {
        let Surface::Nurbs(n0) = s else {
            return s.clone();
        };
        let per = periods(s);
        let mut n: NurbsSurface = (**n0).clone();
        let mut changed = false;
        for (dir, p) in [(0usize, per.pu), (1, per.pv)] {
            let Some(pd) = p else { continue };
            let ((u0, u1), (v0, v1)) = n.range();
            let (a0, a1) = if dir == 0 { (u0, u1) } else { (v0, v1) };
            let coord = |q: P3| if dir == 0 { q.x } else { q.y };
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for c in loops.iter().flat_map(|l| l.1.iter()) {
                let e = &body.edges[c.edge];
                for i in 0..=8 {
                    let x = coord(c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0));
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
            let eps = tol::REL * pd;
            if !(lo.is_finite() && hi.is_finite())
                || (lo >= a0 - eps && hi <= a1 + eps)
                || hi - lo > pd + eps
            {
                continue;
            }
            // Whole periods first: the lowest coordinate into [a0, a0 + period).
            let k = ((lo - a0 + eps) / pd).floor();
            if k != 0.0 {
                let d = -k * pd;
                let m = Affine::translation(if dir == 0 {
                    V3::new(d, 0.0, 0.0)
                } else {
                    V3::new(0.0, d, 0.0)
                });
                for c in loops.iter_mut().flat_map(|l| l.1.iter_mut()) {
                    c.pc = c.pc.transformed(&m);
                }
                changed = true;
            }
            let (lo, hi) = (lo - k * pd, hi - k * pd);
            if hi > a1 + eps
                && let Some(r) = restarted(&n, dir, lo)
            {
                n = r;
                changed = true;
            }
        }
        if changed {
            Surface::Nurbs(Arc::new(n))
        } else {
            s.clone()
        }
    }

    fn add_seam(&mut self, fi: usize, wrapping: &[(usize, i64)]) -> bool {
        let s = self.faces[fi].surf.clone();
        let hint = self.faces[fi].sense;
        let traces: Vec<Trace> = wrapping
            .iter()
            .map(|&(li, _)| self.trace(fi, &s, &self.faces[fi].loops[li]))
            .collect();
        if traces.iter().any(|t| t.wv != 0) {
            return false;
        }
        let la = wrapping[0].0;
        let a_loop = self.faces[fi].loops[la].clone();
        let ta = &traces[0];
        let qa = ta.ends[0].0;
        let va = self.co_start(a_loop[0].0, a_loop[0].1);
        let Some(seam) = iso_u(&s, qa.x) else {
            return false;
        };
        match wrapping.len() {
            2 => {
                let lb = wrapping[1].0;
                if wrapping[0].1 != -wrapping[1].1 {
                    return false;
                }
                let tb = &traces[1];
                let Some((k, inner)) = self.find_u(&self.faces[fi].loops[lb].clone(), tb, qa.x)
                else {
                    return false;
                };
                let (kb, vbv) = match inner {
                    None => (
                        k,
                        self.co_start(self.faces[fi].loops[lb][k].0, self.faces[fi].loops[lb][k].1),
                    ),
                    Some(t) => {
                        let (e, fwd) = self.faces[fi].loops[lb][k];
                        let nv = self.split_edge(e, t);
                        // The piece starting at the new vertex follows in the loop.
                        let _ = fwd;
                        (k + 1, nv)
                    }
                };
                let b_loop = self.faces[fi].loops[lb].clone();
                let pb = self.body.verts[vbv].p;
                let (_, vb_param) = s.project(&pb, Some((qa.x, qa.y)));
                let va_param = qa.y;
                let fwd_seam = self.iso_edge(seam, va, va_param, vbv, vb_param);
                let mut nl: Vec<(usize, bool)> = a_loop.clone();
                nl.push(fwd_seam);
                let nb = b_loop.len();
                for i in 0..nb {
                    nl.push(b_loop[(kb + i) % nb]);
                }
                nl.push((fwd_seam.0, !fwd_seam.1));
                let f = &mut self.faces[fi];
                let (lo, hi) = if la < lb { (la, lb) } else { (lb, la) };
                f.loops.remove(hi);
                f.loops.remove(lo);
                f.loops.insert(0, nl);
                true
            }
            1 => {
                // The face reaches a pole or apex on the side its material lies (left of the
                // loop's direction in uv when the sense is natural).
                let side = (wrapping[0].1.signum() as f64) * if hint { 1.0 } else { -1.0 };
                let pole_v = match &s {
                    Surface::Sphere { .. } => Some(PI / 2.0 * side),
                    Surface::Cone { r, a, .. } => {
                        let v = -r / a.sin();
                        if (v - qa.y) * side > 0.0 {
                            Some(v)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let Some(pv) = pole_v else { return false };
                let pole = self.vertex_at(s.eval(qa.x, pv));
                let fwd_seam = self.iso_edge(seam, va, qa.y, pole, pv);
                let mut nl = a_loop.clone();
                nl.push(fwd_seam);
                nl.push((fwd_seam.0, !fwd_seam.1));
                self.faces[fi].loops[la] = nl;
                true
            }
            _ => false,
        }
    }

    /// Makes a seam of two edges of one loop that are the same curve run both ways (some files
    /// bound a torus or a cylinder band by four closed edges, the two copies of each seam as
    /// edges of their own): the second becomes a use of the first, and its vertices the
    /// first's. Only edges no other face uses.
    fn merge_twin_edges(&mut self, fi: usize) {
        let mut uses = vec![0usize; self.body.edges.len()];
        for f in &self.faces {
            for &(e, _) in f.loops.iter().flatten() {
                uses[e] += 1;
            }
        }
        for li in 0..self.faces[fi].loops.len() {
            let n = self.faces[fi].loops[li].len();
            for i in 0..n {
                for j in i + 1..n {
                    let ((ei, fwi), (ej, fwj)) =
                        (self.faces[fi].loops[li][i], self.faces[fi].loops[li][j]);
                    if ei == ej || uses[ei] != 1 || uses[ej] != 1 {
                        continue;
                    }
                    let (a, b) = (&self.body.edges[ei], &self.body.edges[ej]);
                    if a.degenerate || b.degenerate {
                        continue;
                    }
                    let tol = 10.0 * JOIN.max(a.tol).max(b.tol);
                    // Ends at the same places (cheap, first).
                    let at = |v: usize| self.body.verts[v].p;
                    let ends_meet = |x: [usize; 2], y: [usize; 2]| {
                        let d = |u: usize, w: usize| (at(u) - at(w)).norm() <= tol;
                        (d(x[0], y[0]) && d(x[1], y[1])) || (d(x[0], y[1]) && d(x[1], y[0]))
                    };
                    if !ends_meet(a.v, b.v) {
                        continue;
                    }
                    // Every point of one on the other, and run the other way round the loop.
                    let mut opposite = None;
                    let mut same = true;
                    for k in 1..8 {
                        let t = a.t[0] + (a.t[1] - a.t[0]) * k as f64 / 8.0;
                        let p = a.point(t);
                        let tb = b.curve.closest(&p, b.t[0].min(b.t[1]), b.t[0].max(b.t[1]));
                        if (b.point(tb) - p).norm() > tol {
                            same = false;
                            break;
                        }
                        let da = a.curve.tangent(t) * if fwi { 1.0 } else { -1.0 };
                        let db = b.curve.tangent(tb) * if fwj { 1.0 } else { -1.0 };
                        opposite.get_or_insert(da.dot(&db) < 0.0);
                    }
                    let same = same && {
                        let q = b.point(b.mid_t());
                        let ta = a.curve.closest(&q, a.t[0].min(a.t[1]), a.t[0].max(a.t[1]));
                        (a.point(ta) - q).norm() <= tol
                    };
                    // Closed edges at one vertex chain either way: their flags in such files
                    // don't always run them opposite, which a seam's two sides must.
                    let closed = a.v[0] == a.v[1] && b.v[0] == b.v[1];
                    if !same || (opposite != Some(true) && !closed) {
                        continue;
                    }
                    // ej's vertices become ei's at the same places.
                    let (av, bv) = (a.v, b.v);
                    let mut map: Vec<(usize, usize)> = Vec::new();
                    for x in bv {
                        let p = self.body.verts[x].p;
                        if let Some(&y) = av.iter().min_by(|&&u, &&w| {
                            (self.body.verts[u].p - p)
                                .norm()
                                .total_cmp(&(self.body.verts[w].p - p).norm())
                        }) {
                            if (self.body.verts[y].p - p).norm() > tol {
                                map.clear();
                                break;
                            }
                            map.push((x, y));
                        }
                    }
                    if map.len() != 2 {
                        continue;
                    }
                    for e in &mut self.body.edges {
                        for v in &mut e.v {
                            if let Some(&(_, y)) = map.iter().find(|m| m.0 == *v) {
                                *v = y;
                            }
                        }
                    }
                    self.faces[fi].loops[li][j] = (ei, !fwi);
                    uses[ei] += 1;
                    uses[ej] = 0;
                    self.pc_cache.borrow_mut().clear();
                }
            }
        }
    }

    /// Bounds a whole sphere or torus given with no loops by its seams.
    fn close_whole(&mut self, fi: usize) -> bool {
        let s = self.faces[fi].surf.clone();
        let hint = self.faces[fi].sense;
        let l = match &s {
            Surface::Sphere { .. } => {
                let (sp, np) = (
                    self.vertex_at(s.eval(0.0, -PI / 2.0)),
                    self.vertex_at(s.eval(0.0, PI / 2.0)),
                );
                let m = s.iso_u(0.0).unwrap();
                let e = self.body.add_edge(m, -PI / 2.0, PI / 2.0, sp, np);
                vec![(e, true), (e, false)]
            }
            Surface::Torus { .. } => {
                let v = self.vertex_at(s.eval(0.0, 0.0));
                let eu = self.body.add_edge(s.iso_v(0.0).unwrap(), 0.0, TAU, v, v);
                let ev = self.body.add_edge(s.iso_u(0.0).unwrap(), 0.0, TAU, v, v);
                // Counter-clockwise in uv: along u, up v, back along u, down v.
                let l = vec![(eu, true), (ev, true), (eu, false), (ev, false)];
                if hint {
                    l
                } else {
                    l.into_iter().rev().map(|(e, f)| (e, !f)).collect()
                }
            }
            _ => return false,
        };
        self.faces[fi].loops = vec![l];
        true
    }

    /// Builds the body: seams where missing, p-curves, pole edges, outer loops and senses.
    pub fn finish(self) -> (Body, Vec<String>) {
        let (body, w, _) = self.finish_mapped();
        (body, w)
    }

    /// [`finish`](Self::finish), with the body face each builder face became (None when it was
    /// dropped), by builder face id.
    pub fn finish_mapped(mut self) -> (Body, Vec<String>, Vec<Option<usize>>) {
        let mut prof = super::boolean::Prof::new();
        let nf = self.faces.len();
        let mut dropped = vec![false; nf];
        for fi in 0..nf {
            self.faces[fi].loops.retain(|l| !l.is_empty());
            for li in 0..self.faces[fi].loops.len() {
                let mut l = std::mem::take(&mut self.faces[fi].loops[li]);
                if !self.repair_loop(&mut l) {
                    self.warnings
                        .push(format!("face {fi}: loop {li} doesn't chain"));
                }
                self.faces[fi].loops[li] = l;
            }
            // A point of the face: the middle of an edge (vertices may sit on a cone's apex).
            let p = match self.faces[fi].loops.first().and_then(|l| l.first()) {
                Some(&(e, _)) => {
                    let ed = &self.body.edges[e];
                    ed.point(ed.mid_t())
                }
                None => {
                    let ((u0, u1), (v0, v1)) = self.faces[fi].surf.param_box();
                    self.faces[fi]
                        .surf
                        .eval((u0 + u1) / 2.0 + 0.1, (v0 + v1) / 2.0 + 0.1)
                }
            };
            for step in 0..2 {
                let s = &self.faces[fi].surf;
                let Some(new) = (if step == 0 {
                    analytic(s)
                } else {
                    near_branch(s, &p)
                }) else {
                    continue;
                };
                // The file's sense is about the old surface's normal.
                let (u0, v0) = s.project(&p, None);
                let (u1, v1) = new.project(&p, None);
                if s.normal(u0, v0).dot(&new.normal(u1, v1)) < 0.0 {
                    self.faces[fi].sense = !self.faces[fi].sense;
                }
                self.faces[fi].surf = new;
            }
            if self.faces[fi].surf.u_periodic()
                || self.faces[fi].surf.v_periodic()
                || matches!(self.faces[fi].surf, Surface::Nurbs(_))
            {
                self.merge_twin_edges(fi);
            }
            if self.faces[fi].loops.is_empty() && !self.close_whole(fi) {
                self.warnings.push(format!("face {fi}: no bounds"));
                dropped[fi] = true;
            }
        }
        prof.mark("loops and surfaces");
        self.fit_pcurves(&dropped);
        prof.mark("p-curves");
        // The file's senses against the loops of faces whose loops alone say which way round
        // they go (no jumps at poles, no turns round the surface): when most disagree, the file
        // writes its senses the other way about, and the faces whose loops can close either way
        // round (round a sphere's poles) take the file's sense turned round too.
        let (mut agree, mut disagree) = (0usize, 0usize);
        for fi in 0..nf {
            if dropped[fi] || self.faces[fi].loops.is_empty() {
                continue;
            }
            let s = self.faces[fi].surf.clone();
            let mut areas = Vec::new();
            let mut plain = true;
            for l in &self.faces[fi].loops {
                let t = self.trace(fi, &s, l);
                if t.wu != 0 || t.wv != 0 || t.free.iter().chain(&t.free_v).any(|&f| f) {
                    plain = false;
                    break;
                }
                let z = vec![0; l.len()];
                areas.push(Self::trace_area(&s, &self.body, l, &t, &z, &z));
            }
            let Some(outer) = areas
                .iter()
                .copied()
                .max_by(|a, b| a.abs().total_cmp(&b.abs()))
                .filter(|_| plain)
            else {
                continue;
            };
            if outer.abs() > 0.0 {
                if (outer > 0.0) == self.faces[fi].sense {
                    agree += 1
                } else {
                    disagree += 1
                }
            }
        }
        if disagree > agree {
            for f in &mut self.faces {
                f.sense = !f.sense;
            }
        }
        prof.mark("senses");
        // A whole sphere or torus bounded only by slits (a seam and a cut, each edge used both
        // ways: loops enclosing no area, as SMLib writes some spheres) is bounded by its seams
        // instead, as SMLib reads it; the slits' edges are dropped.
        for fi in 0..nf {
            if dropped[fi]
                || self.faces[fi].loops.is_empty()
                || !matches!(
                    self.faces[fi].surf,
                    Surface::Sphere { .. } | Surface::Torus { .. }
                )
            {
                continue;
            }
            let s = self.faces[fi].surf.clone();
            let slits = self.faces[fi].loops.iter().all(|l| {
                let mut uses: std::collections::HashMap<usize, (usize, usize)> =
                    std::collections::HashMap::new();
                for &(e, f) in l {
                    let u = uses.entry(e).or_default();
                    if f { u.0 += 1 } else { u.1 += 1 }
                }
                let t = self.trace(fi, &s, l);
                // As the loop closes (round the poles); one that doesn't close yet gets a seam
                // the usual way.
                let Some((su, sv)) = self.resolve(&s, l, &t, self.faces[fi].sense) else {
                    return false;
                };
                let area = Self::trace_area(&s, &self.body, l, &t, &su, &sv);
                let (mu, mv) = s.metric();
                area.abs() < tol::REL * mu * mv
                    && uses
                        .iter()
                        .all(|(&e, &(f, r))| self.body.edges[e].degenerate || f == r)
            });
            if slits {
                self.faces[fi].loops.clear();
                self.close_whole(fi);
            }
        }
        prof.mark("slits");
        // Seams (they may split edges of other faces, so trace again after each).
        for _round in 0..4 {
            let mut changed = false;
            for fi in 0..nf {
                if dropped[fi] {
                    continue;
                }
                let s = self.faces[fi].surf.clone();
                if !s.u_periodic() && !s.v_periodic() {
                    continue;
                }
                let mut wrapping = Vec::new();
                for (li, l) in self.faces[fi].loops.iter().enumerate() {
                    let t = self.trace(fi, &s, l);
                    if self.resolve(&s, l, &t, self.faces[fi].sense).is_none() {
                        wrapping.push((li, if t.wu != 0 { t.wu } else { t.wv }));
                    }
                }
                if wrapping.is_empty() {
                    continue;
                }
                if self.add_seam(fi, &wrapping) {
                    changed = true;
                } else {
                    self.warnings.push(format!(
                        "face {fi}: loops wrap around the surface and no seam could be added"
                    ));
                    dropped[fi] = true;
                }
            }
            if !changed {
                break;
            }
        }
        prof.mark("seams");
        // Faces.
        let mut new_index = vec![usize::MAX; nf];
        let faces = std::mem::take(&mut self.faces);
        for (fi, xf) in faces.iter().enumerate() {
            if dropped[fi] {
                continue;
            }
            let s = &xf.surf;
            let (mu, mv) = s.metric();
            let mut loops: Vec<(f64, Vec<Coedge>)> = Vec::new();
            for l in &xf.loops {
                let t = self.trace(fi, s, l);
                let Some((shift, vshift)) = self.resolve(s, l, &t, xf.sense) else {
                    self.warnings
                        .push(format!("face {fi}: a loop doesn't close"));
                    continue;
                };
                let n = l.len();
                // A spline surface's loop brought into its range in u.
                let mut shift: Vec<i64> = shift.iter().zip(&t.base).map(|(a, b)| a + b).collect();
                let vshift: Vec<i64> = vshift.iter().zip(&t.base_v).map(|(a, b)| a + b).collect();
                if let Some(((u0, u1), _)) = periods(s).domain.filter(|_| periods(s).pu.is_some()) {
                    let lo = (0..n)
                        .map(|k| t.ends[k].0.x.min(t.ends[k].1.x) + shift[k] as f64 * t.pu)
                        .fold(f64::INFINITY, f64::min);
                    let c = ((u0 - lo) / t.pu - 1e-7).ceil();
                    if c != 0.0 && lo + c * t.pu <= u1 {
                        shift.iter_mut().for_each(|x| *x += c as i64);
                    }
                }
                let mut out: Vec<Coedge> = Vec::with_capacity(n + 2);
                for k in 0..n {
                    let (du, dv) = (shift[k] as f64 * t.pu, vshift[k] as f64 * t.pv);
                    let pc = if du == 0.0 && dv == 0.0 {
                        t.pcs[k].clone()
                    } else {
                        t.pcs[k].transformed(&Affine::translation(V3::new(du, dv, 0.0)))
                    };
                    // A degenerate edge across a jump at a pole or apex.
                    let pk = (k + n - 1) % n;
                    let prev =
                        t.ends[pk].1 + V2::new(shift[pk] as f64 * t.pu, vshift[pk] as f64 * t.pv);
                    let start = t.ends[k].0 + V2::new(du, dv);
                    let d = start - prev;
                    let len = d.norm();
                    let collapsed = || {
                        let p0 = s.eval(prev.x, prev.y);
                        let mid = prev + d * 0.5;
                        (s.eval(start.x, start.y) - p0).norm() < JOIN
                            && (s.eval(mid.x, mid.y) - p0).norm() < JOIN
                    };
                    if len > tol::ROUNDOFF && (t.free[k] || t.free_v[k] || collapsed()) {
                        let vtx = self.co_start(l[k].0, l[k].1);
                        let p = self.body.verts[vtx].p;
                        let de = self.body.add_degenerate(p, 0.0, len, vtx);
                        out.push(Coedge {
                            edge: de,
                            fwd: true,
                            pc: Curve::Line {
                                o: P3::new(prev.x, prev.y, 0.0),
                                d: V3::new(d.x / len, d.y / len, 0.0),
                            },
                        });
                    }
                    out.push(Coedge {
                        edge: l[k].0,
                        fwd: l[k].1,
                        pc,
                    });
                }
                // The closing joint's degenerate edge belongs at the loop's end.
                if out
                    .first()
                    .is_some_and(|c| self.body.edges[c.edge].degenerate)
                {
                    let c = out.remove(0);
                    out.push(c);
                }
                close_gaps(&self.body, s, &mut out);
                let area = {
                    let mut pts = Vec::new();
                    for c in &out {
                        let e = &self.body.edges[c.edge];
                        let (a, b) = co_range(e, c.fwd);
                        let m = if matches!(c.pc, Curve::Line { .. }) {
                            1
                        } else {
                            32
                        };
                        for i in 0..m {
                            let q = c.pc.point(a + (b - a) * i as f64 / m as f64);
                            pts.push(P2::new(q.x * mu, q.y * mv));
                        }
                    }
                    polygon_area(&pts)
                };
                loops.push((area, out));
            }
            if loops.is_empty() {
                continue;
            }
            let surf = Self::fit_closed_range(&self.body, s, &mut loops);
            let outer = (0..loops.len())
                .max_by(|&a, &b| loops[a].0.abs().total_cmp(&loops[b].0.abs()))
                .unwrap();
            let o = loops.remove(outer);
            let sense = o.0 > 0.0;
            if sense != xf.sense {
                self.warnings.push(format!(
                    "face {fi}: the file's sense disagrees with its loops"
                ));
            }
            let mut ls = vec![o.1];
            ls.extend(loops.into_iter().map(|l| l.1));
            self.body.faces.push(Face {
                surf,
                sense,
                loops: ls,
            });
            new_index[fi] = self.body.faces.len() - 1;
        }
        prof.mark("faces");
        let shells = std::mem::take(&mut self.shells);
        let mut shell_index = Vec::new();
        for (fs, closed) in &shells {
            let faces: Vec<usize> = fs
                .iter()
                .filter_map(|&f| new_index.get(f).copied().filter(|&i| i != usize::MAX))
                .collect();
            let complete = faces.len() == fs.len();
            if faces.is_empty() {
                shell_index.push(None);
                continue;
            }
            self.body.shells.push(Shell {
                faces,
                closed: *closed && complete,
            });
            shell_index.push(Some(self.body.shells.len() - 1));
        }
        for sh in std::mem::take(&mut self.solids) {
            let ss: Vec<usize> = sh
                .iter()
                .filter_map(|&s| shell_index.get(s).copied().flatten())
                .collect();
            if ss.is_empty() || !self.body.shells[ss[0]].closed {
                continue;
            }
            self.body.solids.push(Solid { shells: ss });
        }
        let flipped = orient_shells(&mut self.body, self.unoriented);
        if flipped > 0 {
            self.warnings.push(format!(
                "{flipped} faces turned round to orient their shells consistently"
            ));
        }
        drop_unused(&mut self.body);
        prof.mark("shells");
        prof.report(&format!(
            "XBuilder::finish ({nf} faces, {} p-curves)",
            self.pc_cache.borrow().len()
        ));
        let map = new_index
            .iter()
            .map(|&i| (i != usize::MAX).then_some(i))
            .collect();
        (self.body, self.warnings, map)
    }
}

/// Closes the small gaps files' tolerances leave between consecutive p-curves of a loop (a
/// vertex its tolerance off one of its curves): the end of a fitted (spline) p-curve is moved
/// onto its neighbour's, the exact one where there is one.
fn close_gaps(b: &Body, s: &Surface, l: &mut [Coedge]) {
    let n = l.len();
    let clamped = |c: &NurbsCurve| {
        let p = c.degree;
        let k = &c.knots;
        k[..=p].iter().all(|x| *x == k[0])
            && k[k.len() - 1 - p..].iter().all(|x| *x == k[k.len() - 1])
    };
    // Moves the start (or end) of coedge k's spline p-curve to `to`.
    let moved = |c: &Coedge, start: bool, to: P2| -> Option<Curve> {
        let Curve::Nurbs(nc) = &c.pc else { return None };
        // Its control points end where the edge does only over the edge's own range (an edge on
        // part of a conic's arc has the whole arc's p-curve): cut it to that range first.
        let [t0, t1] = b.edges[c.edge].t;
        let mut nc = nc.restricted(t0, t1);
        let (a, z) = nc.range();
        let near = |x: f64, y: f64| (x - y).abs() <= 1e-12 * (1.0 + y.abs());
        if !clamped(&nc) || !near(a, t0) || !near(z, t1) {
            return None;
        }
        // The coedge's start is the curve's start when it runs along the edge.
        let i = if start == c.fwd { 0 } else { nc.cps.len() - 1 };
        let w = nc.cps[i].w;
        nc.cps[i] = super::nurbs::hom(&P3::new(to.x, to.y, 0.0), w);
        Some(Curve::Nurbs(Arc::new(nc)))
    };
    let end_uv = |c: &Coedge, end: bool| -> P2 {
        let (t0, t1) = co_range(&b.edges[c.edge], c.fwd);
        let q = c.pc.point(if end { t1 } else { t0 });
        P2::new(q.x, q.y)
    };
    for k in 0..n {
        let j = (k + 1) % n;
        if b.edges[l[k].edge].degenerate || b.edges[l[j].edge].degenerate {
            continue;
        }
        let (a, z) = (end_uv(&l[k], true), end_uv(&l[j], false));
        if a == z {
            continue;
        }
        // Only gaps of the files' tolerances (poles have their degenerate edges by now).
        let d3 = (s.eval(a.x, a.y) - s.eval(z.x, z.y)).norm();
        if d3 > 1e-2 {
            continue;
        }
        if let Some(pc) = moved(&l[j], true, a) {
            l[j].pc = pc;
        } else if let Some(pc) = moved(&l[k], false, z) {
            l[k].pc = pc;
        }
    }
}

/// Makes each shell consistently oriented, as files don't always are (a face whose loops run
/// the wrong way about it uses its edges the same way as its neighbours): orientation is
/// propagated across edges shared by two faces, flipping the fewer faces; then (when that turned
/// any, or `always`) a solid's outer shell is turned to enclose a positive volume and its voids a
/// negative one. Returns the number of faces turned round.
pub fn orient_shells(b: &mut Body, always: bool) -> usize {
    let mut flipped_total = 0;
    let void: std::collections::HashSet<usize> = b
        .solids
        .iter()
        .flat_map(|s| s.shells.iter().skip(1).copied())
        .collect();
    let in_solid: std::collections::HashSet<usize> = b
        .solids
        .iter()
        .flat_map(|s| s.shells.iter().copied())
        .collect();
    for si in 0..b.shells.len() {
        let faces = b.shells[si].faces.clone();
        // Edge → uses (face position in the shell, along the edge), seams within a face aside.
        let mut uses: std::collections::HashMap<usize, Vec<(usize, bool)>> =
            std::collections::HashMap::new();
        for (k, &fi) in faces.iter().enumerate() {
            let mut own: std::collections::HashMap<usize, Vec<bool>> =
                std::collections::HashMap::new();
            for c in b.faces[fi].loops.iter().flatten() {
                if !b.edges[c.edge].degenerate {
                    own.entry(c.edge).or_default().push(c.fwd);
                }
            }
            for (e, dirs) in own {
                if dirs.len() == 1 {
                    uses.entry(e).or_default().push((k, dirs[0]));
                }
            }
        }
        let mut nbrs: Vec<Vec<(usize, bool)>> = vec![Vec::new(); faces.len()];
        for us in uses.values() {
            if let [(a, fa), (c, fc)] = us.as_slice() {
                // Consistent when the two use the edge opposite ways.
                let same_way = fa == fc;
                nbrs[*a].push((*c, same_way));
                nbrs[*c].push((*a, same_way));
            }
        }
        let mut flip: Vec<Option<bool>> = vec![None; faces.len()];
        for seed in 0..faces.len() {
            if flip[seed].is_some() {
                continue;
            }
            // One connected group: flip the smaller side.
            let mut group = vec![seed];
            flip[seed] = Some(false);
            let mut i = 0;
            while i < group.len() {
                let f = group[i];
                i += 1;
                for &(g, same_way) in &nbrs[f] {
                    if flip[g].is_none() {
                        flip[g] = Some(flip[f].unwrap() ^ same_way);
                        group.push(g);
                    }
                }
            }
            let n_flip = group.iter().filter(|&&f| flip[f] == Some(true)).count();
            if 2 * n_flip > group.len() {
                for &f in &group {
                    flip[f] = flip[f].map(|x| !x);
                }
            }
        }
        let mut flipped = 0;
        for (k, &fi) in faces.iter().enumerate() {
            if flip[k] == Some(true) {
                Body::reverse_face(&mut b.faces[fi]);
                flipped += 1;
            }
        }
        flipped_total += flipped;
        // When faces were turned, outward (or, for voids, inward) by the enclosed volume.
        if (flipped > 0 || always) && b.shells[si].closed && in_solid.contains(&si) {
            // The faces' boxes (a torus's vertices alone can be one point).
            let mut bb = Bbox::empty();
            for &fi in &faces {
                bb.union(&b.face_bbox(fi));
            }
            // The sign of the enclosed volume from a coarse tessellation (exact integration of
            // spline faces would cost far more).
            let shift = bb.center().coords;
            let defl = (bb.diagonal() * 1e-2).max(tol::POINT);
            let es = super::tess::edge_samples(b, defl, 0.5);
            let vol: f64 = faces
                .iter()
                .map(|&fi| {
                    let m = super::tess::mesh_face(b, &b.faces[fi], &es, defl, 0.5);
                    m.indices
                        .iter()
                        .map(|t| {
                            let [p, q, r] = t.map(|i| m.positions[i as usize].coords - shift);
                            p.dot(&q.cross(&r)) / 6.0
                        })
                        .sum::<f64>()
                })
                .sum();
            if (vol < 0.0) != void.contains(&si) {
                for &fi in &faces {
                    Body::reverse_face(&mut b.faces[fi]);
                }
                flipped_total += faces.len();
            }
        }
    }
    flipped_total
}

/// Removes vertices and edges no face uses (renumbering the rest).
pub fn drop_unused(b: &mut Body) {
    let mut used_e = vec![false; b.edges.len()];
    for f in &b.faces {
        for l in &f.loops {
            for c in l {
                used_e[c.edge] = true;
            }
        }
    }
    let mut emap = vec![usize::MAX; b.edges.len()];
    let mut edges = Vec::new();
    for (i, e) in b.edges.iter().enumerate() {
        if used_e[i] {
            emap[i] = edges.len();
            edges.push(e.clone());
        }
    }
    let mut used_v = vec![false; b.verts.len()];
    for e in &edges {
        used_v[e.v[0]] = true;
        used_v[e.v[1]] = true;
    }
    let mut vmap = vec![usize::MAX; b.verts.len()];
    let mut verts = Vec::new();
    for (i, v) in b.verts.iter().enumerate() {
        if used_v[i] {
            vmap[i] = verts.len();
            verts.push(v.clone());
        }
    }
    for e in &mut edges {
        e.v = [vmap[e.v[0]], vmap[e.v[1]]];
    }
    for f in &mut b.faces {
        for l in &mut f.loops {
            for c in l {
                c.edge = emap[c.edge];
            }
        }
    }
    b.edges = edges;
    b.verts = verts;
}

/// Removes vertices no edge uses and that are no acorn point (renumbering the rest).
pub fn drop_unused_verts(b: &mut Body) {
    let mut used = vec![false; b.verts.len()];
    for e in &b.edges {
        used[e.v[0]] = true;
        used[e.v[1]] = true;
    }
    for &p in &b.points {
        used[p] = true;
    }
    let mut map = vec![usize::MAX; b.verts.len()];
    let mut verts = Vec::new();
    for (i, v) in b.verts.iter().enumerate() {
        if used[i] {
            map[i] = verts.len();
            verts.push(v.clone());
        }
    }
    for e in &mut b.edges {
        e.v = [map[e.v[0]], map[e.v[1]]];
    }
    for p in &mut b.points {
        *p = map[*p];
    }
    b.verts = verts;
}

/// A NURBS curve's knots as distinct values with multiplicities.
pub fn knot_mults(k: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut vals: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &x in k {
        match vals.last() {
            Some(&l) if (x - l).abs() <= 1e-14 * (1.0 + l.abs()) => *mults.last_mut().unwrap() += 1,
            _ => {
                vals.push(x);
                mults.push(1);
            }
        }
    }
    (vals, mults)
}

/// Knots from distinct values and multiplicities.
pub fn expand_knots(vals: &[f64], mults: &[usize]) -> Vec<f64> {
    vals.iter()
        .zip(mults)
        .flat_map(|(v, m)| std::iter::repeat_n(*v, *m))
        .collect()
}

/// Wraps a NURBS curve in our curve type (for readers).
pub fn nurbs_curve(n: super::nurbs::NurbsCurve) -> Curve {
    Curve::Nurbs(Arc::new(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meridian_arcs_stay_in_the_sphere_domain() {
        let s = Surface::Sphere {
            f: Frame::world(),
            r: 10.0,
        };
        for (x, y) in [
            (V3::x(), V3::z()),
            (V3::x(), -V3::z()),
            (-V3::y(), V3::z()),
            (V3::z(), V3::x()),
        ] {
            let c = Curve::Circle {
                c: P3::origin(),
                x,
                y,
                r: 10.0,
            };
            // Arcs ending at a pole or between poles (none crosses one).
            for (t0, t1) in [
                (PI, 1.5 * PI),
                (0.0, 0.5 * PI),
                (0.2, 1.4),
                (1.7, 3.0),
                (3.3, 4.5),
                (4.8, 6.1),
                (-1.4, -0.2),
            ] {
                let pc = pcurve_checked(&s, &c, t0, t1);
                for k in 0..=8 {
                    let t = t0 + (t1 - t0) * k as f64 / 8.0;
                    let q = pc.point(t);
                    assert!(
                        q.y.abs() <= PI / 2.0 + 1e-9,
                        "{x:?} {y:?} {t0}..{t1}: v {}",
                        q.y
                    );
                    assert!((s.eval(q.x, q.y) - c.point(t)).norm() < 1e-9);
                }
            }
        }
    }

    /// Loose faces whose boundary curves meet only within the weld distance (as IGES trimmed
    /// surfaces do): two triangles sharing an edge, one's copy of the shared corner 4e-4 off.
    /// The sewn sheet's vertices take the gap as their tolerance, so it checks clean.
    #[test]
    fn loose_faces_sew_within_the_weld_distance() {
        let tri = |p: [P3; 3]| {
            let n = (p[1] - p[0]).cross(&(p[2] - p[0])).normalize();
            let f = Frame::from_xz(p[0], (p[1] - p[0]).normalize(), n);
            let segs = (0..3).map(|k| {
                let (a, b) = (p[k], p[(k + 1) % 3]);
                LooseSeg {
                    c: Curve::line(a, b).0,
                    a,
                    b,
                }
            });
            (Surface::Plane { f }, vec![segs.collect()])
        };
        let off = P3::new(1.0004, 0.0, 0.0);
        let faces = vec![
            tri([P3::origin(), P3::new(1.0, 0.0, 0.0), P3::new(0.0, 1.0, 0.0)]),
            tri([off, P3::new(1.0, 1.0, 0.0), P3::new(0.0, 1.0, 0.0)]),
        ];
        let (b, _) = sew_loose(faces, 1e-3, true);
        assert_eq!(b.faces.len(), 2);
        assert_eq!(b.verts.len(), 4);
        assert!(b.check().is_empty(), "{:?}", b.check());
    }

    /// Spans of different degrees (a cubic and a linear fallback) join into a valid spline
    /// through both (r0901_ec's p-curve had 50 control points and 52 knots).
    #[test]
    fn spans_of_mixed_degree_join() {
        let pts: Vec<P3> = (0..5)
            .map(|i| P3::new(i as f64, (i as f64).sin(), 0.0))
            .collect();
        let ts: Vec<f64> = (0..5).map(|i| i as f64).collect();
        let cubic = NurbsCurve::interpolate(&pts, &ts, 3, None, None);
        let line = NurbsCurve {
            degree: 1,
            knots: vec![4.0, 4.0, 6.0, 6.0],
            cps: vec![
                super::super::nurbs::hom(&pts[4], 1.0),
                super::super::nurbs::hom(&P3::new(6.0, 0.0, 0.0), 1.0),
            ],
        };
        let j = join_spans(vec![cubic.clone(), line]).unwrap();
        assert!(j.validate().is_ok());
        assert!((j.point(2.0) - cubic.point(2.0)).norm() < 1e-9);
        assert!((j.point(5.0) - P3::new(5.0, (4f64.sin()) / 2.0, 0.0)).norm() < 1e-9);
    }

    /// An edge along a closed spline ending at the spline's start (its vertex projects to the
    /// start's parameter, before the edge's first): read along the spline from its first
    /// vertex, not dropped (as1_motor's, r0901_ec's and trj*-ec's loops lost such edges).
    #[test]
    fn edges_ending_at_a_closed_splines_start() {
        let pts = [
            P3::origin(),
            P3::new(1.0, 0.0, 0.0),
            P3::new(1.0, 1.0, 0.0),
            P3::new(0.0, 1.0, 0.0),
            P3::origin(),
        ];
        let knots = vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0];
        let sq = NurbsCurve {
            degree: 1,
            knots,
            cps: pts
                .iter()
                .map(|p| super::super::nurbs::hom(p, 1.0))
                .collect(),
        };
        let mut xb = XBuilder::new();
        let (a, b) = (xb.vertex(P3::new(0.0, 1.0, 0.0)), xb.vertex(P3::origin()));
        let e = xb.edge(Curve::Nurbs(Arc::new(sq)), a, b).expect("the edge");
        let ed = &xb.body.edges[e];
        assert!((ed.point(ed.t[0]) - P3::new(0.0, 1.0, 0.0)).norm() < 1e-9);
        assert!((ed.point(ed.t[1]) - P3::origin()).norm() < 1e-9);
        // The last side, not the other three.
        assert!((ed.point(0.5 * (ed.t[0] + ed.t[1])) - P3::new(0.0, 0.5, 0.0)).norm() < 1e-9);
    }
}
