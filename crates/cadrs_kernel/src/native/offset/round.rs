//! Rounded offsets (the body grown by a ball: a Minkowski sum) and shells.
//!
//! The grown body is the body, each face swept out by the distance (a slab), a tube round each
//! convex edge and a ball at each vertex, united. Shells: inward, the body less its inward
//! sharp offset (faces moved in, removed faces moved out past the body); when that offset
//! can't be made (faces vanish, as round a counterbore), the walls are built from their
//! definition, the body's points within the thickness of its kept faces: the cavity is the body
//! less each kept face swept in, a tube round each concave edge and a ball at each concave
//! vertex. Outward, the grown body less the body, cut back to the removed faces' planes.

use crate::native::blend::{Tracked, ball_body, ball_frame};
use crate::native::boolean::boolean_bodies;
use crate::native::curve::Curve;
use crate::native::kernel::NativeKernel;
use crate::native::math::*;
use crate::native::surface::Surface;
use crate::native::tol;
use crate::native::topo::Body;
use crate::*;

/// A planar face swept along its outward normal by `d` (negative: inward).
fn face_slab(b: &Body, fi: usize, d: f64) -> Result<Body> {
    let f = &b.faces[fi];
    let Surface::Plane { f: fr } = &f.surf else {
        // A concave cylinder whose axis the offset reaches: the wedge out to its axis.
        if let Surface::Cylinder { f: fr, r } = &f.surf
            && d > 0.0
            && f.sense != fr.right_handed()
            && d >= r * (1.0 - tol::REL)
            && let Some(w) = cylinder_wedge(b, fi)?
        {
            return Ok(w);
        }
        return super::slab::face_slab(b, fi, d);
    };
    let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
    let plane = Plane {
        origin: fr.o,
        x_dir: nalgebra::Unit::new_normalize(fr.x),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let tmp = crate::native::kernel::subset(b, &[fi], false);
    let mut k = NativeKernel::new();
    let id = k.insert(tmp);
    let prof = Profile::new(plane, vec![]);
    let mut mf = crate::native::features::face_as_profile(
        &k,
        &prof,
        &FaceInput {
            body: id,
            face: FaceId(0),
            source: 0,
        },
    )?;
    // A closed spline edge would sweep a band round a closed, non-periodic surface (one seam
    // edge used twice), which the boolean doesn't take: in two halves, two plain sides.
    let halve = |l: &mut crate::native::profile::Loop2| {
        *l = l
            .drain(..)
            .flat_map(|p| {
                let closed = matches!(p.c, Curve::Nurbs(_))
                    && (p.c.point(p.t0) - p.c.point(p.t1)).norm()
                        < tol::REL * (1.0 + p.c.point(p.t0).coords.norm());
                if closed {
                    let m = 0.5 * (p.t0 + p.t1);
                    let (mut a, mut b) = (p.clone(), p);
                    // The halves in the loop's order (a reversed piece runs from t1 to t0).
                    if a.rev {
                        a.t0 = m;
                        b.t1 = m;
                    } else {
                        a.t1 = m;
                        b.t0 = m;
                    }
                    vec![a, b]
                } else {
                    vec![p]
                }
            })
            .collect();
    };
    halve(&mut mf.outer);
    mf.holes.iter_mut().for_each(halve);
    let dir = if d >= 0.0 { n } else { -n };
    let kind = crate::native::sweep::Prism { plane, dir };
    Ok(crate::native::sweep::sweep_full(&[mf], &[], &kind, 0.0, d.abs(), false, true)?.body)
}

/// A tube of radius r round an edge (a cylinder along a line, a torus section round an arc),
/// with flat ends.
pub(crate) fn edge_tube(b: &Body, e: usize, r: f64) -> Result<Body> {
    let ed = &b.edges[e];
    // A B-spline edge that is straight (a loft's rails to a point): the cylinder round the line
    // (a pipe swept along it touches the faces' slabs along seams the booleans can't settle).
    if let Curve::Nurbs(n) = &ed.curve
        && let Some(Curve::Line { .. }) = crate::native::recognize::curve(n)
    {
        let (p0, p1) = (ed.point(ed.t[0]), ed.point(ed.t[1]));
        if (p1 - p0).norm() > tol::POINT {
            let mut nb = b.clone();
            nb.edges[e].curve = Curve::Line { o: p0, d: p1 - p0 };
            nb.edges[e].t = [0.0, 1.0];
            return edge_tube(&nb, e, r);
        }
    }
    match &ed.curve {
        Curve::Line { d, .. } => {
            let p0 = ed.point(ed.t[0]);
            let t = d.normalize();
            // The seam (the circle's start) turned into the body, away from the lines along
            // which the tube touches its faces' slabs.
            let mid = ed.point(0.5 * (ed.t[0] + ed.t[1]));
            let ef = b.edge_faces();
            let inward = -ef[e]
                .iter()
                .map(|&f| outward_normal(b, f, &mid))
                .fold(V3::zeros(), |a, n| a + n.normalize());
            let fr = match (inward - t * inward.dot(&t)).try_normalize(tol::ANGULAR) {
                Some(x) => Frame::from_xz(p0, x, t),
                None => Frame::from_z(p0, t),
            };
            let plane = Plane {
                origin: p0,
                x_dir: nalgebra::Unit::new_normalize(fr.x),
                normal: nalgebra::Unit::new_normalize(t),
            };
            let prof = Profile::new(
                plane,
                vec![Region {
                    outer: Loop {
                        curves: vec![Curve2::Circle {
                            center: nalgebra::Point2::origin(),
                            radius: r,
                            source: None,
                        }],
                    },
                    holes: vec![],
                    source: None,
                }],
            );
            let regions = crate::native::profile::regions(&prof)?;
            let mf = crate::native::profile::merge_regions(&regions)?;
            let len = (ed.t[1] - ed.t[0]) * d.norm();
            let kind = crate::native::sweep::Prism { plane, dir: t };
            Ok(crate::native::sweep::sweep_full(&mf, &[], &kind, 0.0, len, false, true)?.body)
        }
        Curve::Circle { c, x, y, r: rc } => {
            let q = ed.point(ed.t[0]);
            let axis = x.cross(y).normalize();
            let radial = (q - c).normalize();
            let tangent = axis.cross(&radial);
            // The profile plane's x runs out from the axis, its y down it (−axis); the axis is
            // the line x = −rc.
            let plane = Plane {
                origin: q,
                x_dir: nalgebra::Unit::new_normalize(radial),
                normal: nalgebra::Unit::new_normalize(tangent),
            };
            let outer = if r < rc * (1.0 - tol::REL) {
                Loop {
                    curves: vec![Curve2::Circle {
                        center: nalgebra::Point2::origin(),
                        radius: r,
                        source: None,
                    }],
                }
            } else {
                // A tube wider than its arc's radius (a spindle torus): the balls along the
                // arc reach across the axis, but a point on the arc's side of the axis is
                // nearest the arc there, so the tube is the disc's part on that side revolved
                // (past the arc's ends the vertices' balls cover the rest).
                // (As wide as the arc's radius, the disc only touches the axis: its chord there
                // is kept 1e-4 of the radius long, a sliver 5e-9 of it off the axis.)
                let hmin = 1e-4 * r;
                let h = (r * r - rc * rc).max(0.0).sqrt();
                let (x, h) = if h < hmin {
                    (-(r * r - hmin * hmin).sqrt(), hmin)
                } else {
                    (-rc, h)
                };
                let (a, b) = (nalgebra::Point2::new(x, h), nalgebra::Point2::new(x, -h));
                let (ta, tb) = (h.atan2(x), (-h).atan2(x));
                Loop {
                    curves: vec![
                        Curve2::Arc {
                            center: nalgebra::Point2::origin(),
                            radius: r,
                            start_angle: tb,
                            sweep: ta - tb,
                            source: None,
                        },
                        Curve2::Line { a, b, source: None },
                    ],
                }
            };
            let prof = Profile::new(
                plane,
                vec![Region {
                    outer,
                    holes: vec![],
                    source: None,
                }],
            );
            let regions = crate::native::profile::regions(&prof)?;
            let mf = crate::native::profile::merge_regions(&regions)?;
            let f = Frame {
                o: *c,
                x: radial,
                y: axis.cross(&radial),
                z: axis,
            };
            let span = ed.t[1] - ed.t[0];
            let whole = span >= TAU - tol::ANGULAR;
            let kind = crate::native::sweep::Revolve { plane, f };
            Ok(crate::native::sweep::sweep_full(
                &mf,
                &[],
                &kind,
                0.0,
                if whole { TAU } else { span },
                whole,
                true,
            )?
            .body)
        }
        // Any other curve: a disc square to the edge at its start swept along it (a pipe).
        _ => {
            // Where the edge curves tighter than the radius the pipe folds over itself: not a
            // valid solid (and its booleans fail only after every retry).
            // (κ·r − 1 scaled by |c′|³, at 257 points and, between the neighbours of each that
            // tops both, at its peak: a curvature peak between samples is found too.)
            let n = 256;
            let over = |t: f64| {
                let (_, d1, d2) = ed.curve.d2(t);
                d1.cross(&d2).norm() * r - d1.norm().powi(3)
            };
            let at = |i: usize| ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / n as f64;
            let fs: Vec<f64> = (0..=n).map(|i| over(at(i))).collect();
            let tight = (0..=n).any(|i| {
                if fs[i] >= 0.0 {
                    return true;
                }
                if i == 0 || i == n || fs[i] < fs[i - 1] || fs[i] < fs[i + 1] {
                    return false;
                }
                const PHI: f64 = 0.618_033_988_749_894_9;
                let (mut lo, mut hi) = (at(i - 1), at(i + 1));
                let (mut x1, mut x2) = (hi - PHI * (hi - lo), lo + PHI * (hi - lo));
                let (mut f1, mut f2) = (over(x1), over(x2));
                for _ in 0..40 {
                    if f1 > f2 {
                        (hi, x2, f2) = (x2, x1, f1);
                        x1 = hi - PHI * (hi - lo);
                        f1 = over(x1);
                    } else {
                        (lo, x1, f1) = (x1, x2, f2);
                        x2 = lo + PHI * (hi - lo);
                        f2 = over(x2);
                    }
                }
                f1.max(f2) >= 0.0
            });
            let ef = b.edge_faces();
            if tight {
                // The whole tube would cross itself on the edge's inner side. Where the edge
                // lies in a plane, bends one way all along, and its faces' normals both point
                // to its outer side, the faces' wedge of the tube lies in the tube's outer
                // half, and that half never folds (the offsets of a convex curve on its outer
                // side don't cross): the half disc on that side swept along the edge (a
                // cylinder cut aslant, grown past its ellipse's radius of curvature).
                return outer_half_tube(b, e, r, &ef[e]).ok_or(KernelError::Unsupported(
                    "a tube round an edge curved tighter than its radius",
                ))?;
            }
            let p0 = ed.point(ed.t[0]);
            let t = ed
                .curve
                .tangent(ed.t[0])
                .try_normalize(tol::DEGENERATE)
                .ok_or(KernelError::Unsupported(
                    "a tube round an edge with no direction",
                ))?;
            // The seam (the circle's start) turned into the body, as the straight tube's: the
            // part of the tube left outside the faces' slabs then never runs across it (a
            // closed B-spline's parameters don't wrap, and a face can't lie over its seam).
            let inward = -ef[e]
                .iter()
                .map(|&f| outward_normal(b, f, &p0))
                .fold(V3::zeros(), |a, n| a + n.normalize());
            let fr = match (inward - t * inward.dot(&t)).try_normalize(tol::ANGULAR) {
                Some(x) => Frame::from_xz(p0, x, t),
                None => Frame::from_z(p0, t),
            };
            let plane = Plane {
                origin: p0,
                x_dir: nalgebra::Unit::new_normalize(fr.x),
                normal: nalgebra::Unit::new_normalize(t),
            };
            let prof = Profile::new(
                plane,
                vec![Region {
                    outer: Loop {
                        curves: vec![Curve2::Circle {
                            center: nalgebra::Point2::origin(),
                            radius: r,
                            source: None,
                        }],
                    },
                    holes: vec![],
                    source: None,
                }],
            );
            let mut k = NativeKernel::new();
            let body = k.insert(b.clone());
            let spec = SweepSpec {
                body: BodyKind::Solid,
                path: vec![PathCurve::Edge {
                    body,
                    edge: EdgeId(e as u64),
                }],
                control: SweepControl::None,
                corners: Default::default(),
                faces: vec![],
            };
            let out = k.sweep_with(&prof, &spec)?;
            let [id] = out.bodies[..] else {
                return Err(KernelError::InvalidResult {
                    op: "offset",
                    why: "a tube round an edge isn't one body".into(),
                });
            };
            Ok((**k.get(id)?).clone())
        }
    }
}

/// How an edge's two faces meet.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Dihedral {
    /// The material inside the wedge.
    Convex,
    Concave,
    /// Tangent faces, or a seam across which its face's normal turns no more than
    /// [`tol::ANGULAR`].
    Smooth,
    /// Not two faces (a boundary or non-manifold edge), or a seam across which the face kinks.
    Unknown,
}

fn dihedral(b: &Body, e: usize) -> Dihedral {
    let ed = &b.edges[e];
    let tm = ed.mid_t();
    let mut sides = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        for c in f.loops.iter().flatten() {
            if c.edge == e {
                let uv = c.pc.point(tm);
                let n = f.surf.normal(uv.x, uv.y) * if f.sense { 1.0 } else { -1.0 };
                let d = ed.curve.tangent(tm).normalize() * if c.fwd { 1.0 } else { -1.0 };
                sides.push((fi, n, n.cross(&d)));
            }
        }
    }
    if sides.len() != 2 {
        return Dihedral::Unknown;
    }
    let (n1, n2) = (sides[0].1, sides[1].1);
    if sides[0].0 == sides[1].0 {
        return if n1.cross(&n2).norm() < tol::ANGULAR && n1.dot(&n2) > 0.0 {
            Dihedral::Smooth
        } else {
            Dihedral::Unknown
        };
    }
    if n1.cross(&n2).norm() < tol::ANGULAR {
        return Dihedral::Smooth;
    }
    if sides[1].2.dot(&n1) < 0.0 {
        Dihedral::Convex
    } else {
        Dihedral::Concave
    }
}

/// Whether an edge is convex (its faces' material inside the wedge); None for tangent faces,
/// a seam or an edge not between two faces.
fn convex(b: &Body, e: usize) -> Option<bool> {
    match dihedral(b, e) {
        Dihedral::Convex => Some(true),
        Dihedral::Concave => Some(false),
        Dihedral::Smooth | Dihedral::Unknown => None,
    }
}

/// Whether the offset by a ball needs a ball at vertex `v` (research/algorithms/
/// offsets_sweeps_import.md card 5, Rossignac & Requicha 1986 §2.6, §4.1): a ball adds boundary
/// only where `v` is the nearest point of the body, which takes edges of the offset's kind
/// (`rounded`: convex ones outward, concave ones inward) meeting at a corner. No ball where
/// only smooth edges meet (a vertex between fillets), or where the only two of that kind run on
/// through it tangentially (a circle's seam vertex) and the rest are smooth. A vertex with a
/// point edge (an apex, a pole), an edge of unknown kind or one of the other kind keeps its
/// ball: by the rule it needs none either, but without it the slabs meeting there leave the
/// booleans tangent configurations they can't settle (SMLib's offset_bracket2 by 3.5).
fn needs_ball(b: &Body, kinds: &[Dihedral], v: usize, rounded: Dihedral) -> bool {
    let mut leaving: Vec<V3> = Vec::new();
    let mut others_smooth = true;
    for (e, ed) in b.edges.iter().enumerate() {
        for (end, &w) in ed.v.iter().enumerate() {
            if w != v {
                continue;
            }
            if ed.degenerate || kinds[e] == Dihedral::Unknown {
                return true;
            }
            if kinds[e] == rounded {
                let t = ed.curve.tangent(ed.t[end]).normalize();
                leaving.push(if end == 0 { t } else { -t });
            } else if kinds[e] != Dihedral::Smooth {
                others_smooth = false;
            }
        }
    }
    match leaving.as_slice() {
        [] => !others_smooth,
        [s, t] => !(others_smooth && s.dot(t) < -1.0 + tol::ANGULAR),
        _ => true,
    }
}

/// `acc` (op) `piece`; a failure says which piece (`what`) with `CADRS_OFFSET_DEBUG`, and
/// leaves both bodies in `CADRS_DUMP_DIR`.
fn combine(acc: &Body, piece: &Body, op: BoolOp, what: &dyn Fn() -> String) -> Result<Body> {
    let debug = crate::native::env_flag!("CADRS_OFFSET_DEBUG");
    let dump = || {
        if crate::native::env_flag!("CADRS_DUMP_DIR") {
            crate::native::kernel::dump("offset-target", acc, &format!("{op:?} {}", what()));
            crate::native::kernel::dump("offset-tool", piece, &format!("{op:?} {}", what()));
        }
    };
    // (The boolean returns only valid bodies, so a piece is never built on a broken result:
    // one that fails is tried again later.)
    Ok(boolean_bodies(acc, piece, op)
        .inspect_err(|e| {
            if debug {
                eprintln!("offset: {op:?} with {}: {e:?}", what());
            }
            dump();
        })?
        .body)
}

/// The body with its adjacent faces on one surface merged (a face imprinted with a region of
/// its own plane): the offset doesn't depend on how a surface is split, but the pieces' slabs
/// would meet along bands on one surface, which the boolean can't settle. Unchanged when the
/// merge doesn't give a valid solid.
fn merged(b: &Body) -> Body {
    let mut k = NativeKernel::new();
    let id = k.insert(b.clone());
    match crate::Kernel::simplify(&mut k, id).ok().and_then(|r| {
        r.bodies
            .first()
            .and_then(|x| k.get(*x).ok())
            .map(|x| (**x).clone())
    }) {
        Some(m) if m.check().is_empty() && m.faces.len() < b.faces.len() => m,
        _ => b.clone(),
    }
}

/// The body grown by a ball of radius d (all faces moved out by d, edges and corners rounded).
pub fn offset_rounded(b: &Body, dist: &[f64]) -> Result<Body> {
    let d = dist[0];
    if dist.iter().any(|x| (x - d).abs() > tol::ROUNDOFF_FINE) || d <= 0.0 {
        return Err(KernelError::Unsupported(
            "rounded offsets with different distances per face",
        ));
    }
    // A solid of revolution: its section grown, revolved.
    if let Some(g) = crate::native::sweep_solid::grown_revolved(b, d) {
        return Ok(g);
    }
    // Every edge between faces that meet tangentially (a body offset already, its rounds
    // joining its faces smoothly): each face moved along its normal meets its neighbours'
    // moves exactly, so the sharp offset is the rounded one, without the slabs, tubes and balls.
    // (No torus grown past its hole: its offset crosses itself, the body inside out.)
    let crosses_itself = b.faces.iter().any(|f| {
        matches!(f.surf, Surface::Torus { rmaj, rmin, .. } if rmin + d >= rmaj * (1.0 - tol::REL))
    });
    if !b.edges.is_empty()
        && !crosses_itself
        && (0..b.edges.len())
            .all(|e| b.edges[e].degenerate || matches!(dihedral(b, e), Dihedral::Smooth))
        && let Ok(g) = super::sharp::offset_sharp(b, dist)
        && g.check_with(crate::native::topo::CheckLevel::Full)
            .is_empty()
    {
        return Ok(g);
    }
    grown_except(&merged(b), d, &[], &[])
}

/// A body of planes, cylinders and tori (each a torus still offset) whose edges are all convex
/// or smooth (besides point edges) grown by a ball of radius `d`: its sharp offset, each face moved `d` out, with every edge filleted by `d`. The ball
/// that rounds an edge of the sharp offset touches both its faces, so its centre lies `d`
/// inside both, on the edge they were moved from: the round is the tube about that edge, as the
/// ball's sum makes it, and at a corner the ball about its vertex; faces meeting tangentially
/// meet tangentially moved, needing nothing. None where the sharp offset
/// changes the body's faces or edges (a hole the distance closes: a face lost), or the fillet
/// fails or isn't sound.
fn grown_filleted(b: &Body, dist: &[f64], d: f64) -> Option<Body> {
    use crate::native::topo::CheckLevel;
    let kinds_ok = |b: &Body| {
        (0..b.edges.len()).all(|e| {
            b.edges[e].degenerate
                || match dihedral(b, e) {
                    // (Smooth: a seam, or faces meeting tangentially, whose moves meet
                    // tangentially again. Concave: the moved faces meet in the grown body's
                    // edge, nothing to round, where no convex edge shares its ends.)
                    Dihedral::Convex | Dihedral::Smooth | Dihedral::Concave => true,
                    _ => false,
                }
        })
    };
    // Planes, cylinders, and tori whose move keeps them tori: a round's tube (material
    // outside it) narrowed short of nothing, a ring's (material inside) widened short of its
    // axis.
    let simple = b.faces.iter().all(|f| match &f.surf {
        Surface::Plane { .. } | Surface::Cylinder { .. } | Surface::Cone { .. } => true,
        Surface::Torus { f: tf, rmaj, rmin } => {
            let p = tf.o + tf.x * (rmaj + rmin);
            let (u, v) = f.surf.project(&p, None);
            let out = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
            let ring = out.dot(&tf.x) > 0.0;
            if ring {
                rmin + d < rmaj * (1.0 - tol::REL)
            } else {
                rmin - d > tol::POINT * (1.0 + rmin)
            }
        }
        _ => false,
    });
    // (A concave edge's ends no convex edge's: the round of a convex edge running into a
    // concave one isn't the moved faces' meeting.)
    let apart = |b: &Body| {
        let kinds: Vec<Dihedral> = (0..b.edges.len()).map(|e| dihedral(b, e)).collect();
        let mut convex_at = vec![false; b.verts.len()];
        for (e, k) in kinds.iter().enumerate() {
            if matches!(k, Dihedral::Convex) && !b.edges[e].degenerate {
                b.edges[e].v.iter().for_each(|&v| convex_at[v] = true);
            }
        }
        (0..b.edges.len()).all(|e| {
            !matches!(kinds[e], Dihedral::Concave) || b.edges[e].v.iter().all(|&v| !convex_at[v])
        })
    };
    // (A cone's apex is a corner its edges' rounds never reach: the ball about it is the
    // grown body's there.)
    let apex = b.edges.iter().any(|e| e.degenerate);
    if apex || !simple || b.solids.len() != 1 || b.edges.is_empty() || !kinds_ok(b) || !apart(b) {
        return None;
    }
    let sharp = super::sharp::offset_sharp(b, dist).ok()?;
    if sharp.faces.len() != b.faces.len()
        || sharp.edges.len() != b.edges.len()
        || !kinds_ok(&sharp)
        || !sharp.check_with(CheckLevel::Fast).is_empty()
    {
        return None;
    }
    let edges: Vec<EdgeId> = (0..sharp.edges.len())
        .filter(|&e| !sharp.edges[e].degenerate && matches!(dihedral(&sharp, e), Dihedral::Convex))
        .map(|e| EdgeId(e as u64))
        .collect();
    let mut k = NativeKernel::new();
    let id = k.insert(sharp);
    let r = crate::native::blend::fillet(&mut k, id, &edges, &FilletSpec::radius(d)).ok()?;
    let out = k.get(r.first().ok()?).ok()?.as_ref().clone();
    (out.solids.len() == 1 && out.check_with(CheckLevel::Fast).is_empty()).then_some(out)
}

/// A convex polyhedron grown by a ball of radius `d`, built face by face: each face moved out
/// by `d`, a cylinder of radius `d` along each edge between its faces' moved copies, and a ball
/// patch at each corner bounded by its edges' end arcs. The same solid the slabs', tubes' and
/// balls' union makes, without the dozens of booleans. None unless every face is a plane with
/// one loop and every edge a straight convex edge between two faces (then the result is checked).
pub(crate) fn grown_convex(b: &Body, d: f64) -> Option<Body> {
    use crate::native::pcurve::pcurve;
    use crate::native::topo::{BodyBuilder, Coedge};
    use std::collections::HashMap;
    if b.solids.len() != 1 || b.shells.len() != 1 || !b.shells[0].closed {
        return None;
    }
    let ef = b.edge_faces();
    // Each face's outward normal; every face a plane with one loop.
    let mut normal = Vec::with_capacity(b.faces.len());
    for f in &b.faces {
        let Surface::Plane { f: fr } = &f.surf else {
            return None;
        };
        if f.loops.len() != 1 {
            return None;
        }
        normal.push(fr.x.cross(&fr.y).normalize() * if f.sense { 1.0 } else { -1.0 });
    }
    // Each edge straight, convex, and run forward by exactly one of its two faces: (f1, f2).
    let mut sides = vec![(usize::MAX, usize::MAX); b.edges.len()];
    for (fi, f) in b.faces.iter().enumerate() {
        for co in &f.loops[0] {
            let s = &mut sides[co.edge];
            let slot = if co.fwd { &mut s.0 } else { &mut s.1 };
            if *slot != usize::MAX {
                return None;
            }
            *slot = fi;
        }
    }
    for (e, ed) in b.edges.iter().enumerate() {
        if ed.degenerate
            || !matches!(ed.curve, Curve::Line { .. })
            || ef[e].len() != 2
            || sides[e].0 == usize::MAX
            || sides[e].1 == usize::MAX
            || convex(b, e) != Some(true)
        {
            return None;
        }
    }
    let mut out = BodyBuilder::new();
    let mut vert: HashMap<(usize, usize), usize> = HashMap::new();
    let mut at = |out: &mut BodyBuilder, f: usize, v: usize| -> usize {
        *vert
            .entry((f, v))
            .or_insert_with(|| out.vertex(b.verts[v].p + normal[f] * d))
    };
    // The moved faces' edges: (face, edge) -> new edge, in the old edge's direction.
    let mut line: HashMap<(usize, usize), usize> = HashMap::new();
    for e in 0..b.edges.len() {
        let [v0, v1] = b.edges[e].v;
        for f in [sides[e].0, sides[e].1] {
            let (a, z) = (at(&mut out, f, v0), at(&mut out, f, v1));
            let (pa, pz) = (b.verts[v0].p + normal[f] * d, b.verts[v1].p + normal[f] * d);
            let c = Curve::Line { o: pa, d: pz - pa };
            line.insert((f, e), out.edge(c, 0.0, 1.0, a, z));
        }
    }
    // Each edge's end arcs, about its vertices from f1's side to f2's: (edge, vertex) -> new edge.
    let mut arc: HashMap<(usize, usize), (usize, Curve, f64)> = HashMap::new();
    for e in 0..b.edges.len() {
        let (f1, f2) = sides[e];
        let (n1, n2) = (normal[f1], normal[f2]);
        let y = (n2 - n1 * n1.dot(&n2)).try_normalize(tol::ANGULAR)?;
        let th = n1.dot(&n2).clamp(-1.0, 1.0).acos();
        for w in b.edges[e].v {
            let c = Curve::Circle {
                c: b.verts[w].p,
                x: n1,
                y,
                r: d,
            };
            let (a, z) = (at(&mut out, f1, w), at(&mut out, f2, w));
            let id = out.edge(c.clone(), 0.0, th, a, z);
            arc.insert((e, w), (id, c, th));
        }
    }
    let co = |surf: &Surface, near: P2, id: usize, c: &Curve, t0: f64, t1: f64, fwd: bool| Coedge {
        edge: id,
        fwd,
        pc: pcurve(surf, c, t0, t1, Some(near)),
    };
    let line_curve = |f: usize, e: usize| {
        let [v0, v1] = b.edges[e].v;
        let pa = b.verts[v0].p + normal[f] * d;
        let pz = b.verts[v1].p + normal[f] * d;
        Curve::Line { o: pa, d: pz - pa }
    };
    let sensed = |surf: &Surface, p: P3, out_dir: V3| {
        let (u, v) = surf.project(&p, None);
        (P2::new(u, v), surf.normal(u, v).dot(&out_dir) > 0.0)
    };
    let mut faces = Vec::new();
    // The moved faces.
    for (fi, f) in b.faces.iter().enumerate() {
        let Surface::Plane { f: fr } = &f.surf else {
            return None;
        };
        let surf = Surface::Plane {
            f: Frame {
                o: fr.o + normal[fi] * d,
                ..*fr
            },
        };
        let p = b.verts[b.edges[f.loops[0][0].edge].v[0]].p + normal[fi] * d;
        let (near, _) = sensed(&surf, p, normal[fi]);
        let lp = f.loops[0]
            .iter()
            .map(|c| {
                co(
                    &surf,
                    near,
                    line[&(fi, c.edge)],
                    &line_curve(fi, c.edge),
                    0.0,
                    1.0,
                    c.fwd,
                )
            })
            .collect();
        faces.push(out.face(surf, f.sense, vec![lp]));
    }
    // The edges' cylinders.
    for e in 0..b.edges.len() {
        let (f1, f2) = sides[e];
        let [v0, v1] = b.edges[e].v;
        let (p0, p1) = (b.verts[v0].p, b.verts[v1].p);
        let mid_dir = (normal[f1] + normal[f2]).normalize();
        let surf = Surface::Cylinder {
            f: Frame::from_xz(p0, -mid_dir, p1 - p0),
            r: d,
        };
        let (near, sense) = sensed(&surf, nalgebra::center(&p0, &p1) + mid_dir * d, mid_dir);
        let (a1, c1, t1) = &arc[&(e, v1)];
        let (a0, c0, t0) = &arc[&(e, v0)];
        let lp = vec![
            co(
                &surf,
                near,
                line[&(f2, e)],
                &line_curve(f2, e),
                0.0,
                1.0,
                true,
            ),
            co(&surf, near, *a1, c1, 0.0, *t1, false),
            co(
                &surf,
                near,
                line[&(f1, e)],
                &line_curve(f1, e),
                0.0,
                1.0,
                false,
            ),
            co(&surf, near, *a0, c0, 0.0, *t0, true),
        ];
        faces.push(out.face(surf, sense, vec![lp]));
    }
    // The corners' balls: each bounded by its edges' arcs, each run the other way from its
    // cylinder (which runs an edge's end arc from f2's side to f1's, its start arc from f1's to
    // f2's).
    let mut around: Vec<Vec<(usize, bool)>> = vec![Vec::new(); b.verts.len()];
    for (e, ed) in b.edges.iter().enumerate() {
        let [v0, v1] = ed.v;
        around[v0].push((e, false));
        around[v1].push((e, true));
    }
    for (w, arcs) in around.iter().enumerate() {
        if arcs.is_empty() {
            continue;
        }
        let m = arcs
            .iter()
            .flat_map(|&(e, _)| [normal[sides[e].0], normal[sides[e].1]])
            .fold(V3::zeros(), |a, n| a + n)
            .try_normalize(tol::ANGULAR)?;
        let pw = b.verts[w].p;
        // Chain the arcs: each from its start vertex to its end vertex in the loop's direction.
        let ends = |&(e, fwd): &(usize, bool)| {
            let (f1, f2) = sides[e];
            if fwd {
                ((f1, w), (f2, w))
            } else {
                ((f2, w), (f1, w))
            }
        };
        let mut order = vec![arcs[0]];
        while order.len() < arcs.len() {
            let (_, end) = ends(order.last()?);
            let next = arcs
                .iter()
                .find(|a| ends(a).0 == end && !order.contains(a))?;
            order.push(*next);
        }
        if ends(order.last()?).1 != ends(&order[0]).0 {
            return None;
        }
        // The ball's pole at the corner's point on one of its faces: the arcs through it are
        // meridians, and those square to it the equator, both with exact p-curves (any other
        // arc's is fitted, which costs more than the rest of the body): the face that makes
        // the most of them.
        let starts: Vec<usize> = order.iter().map(|a| ends(a).0.0).collect();
        let k = starts.len();
        let exact = |p: usize| {
            let np = normal[p];
            (0..k)
                .filter(|&i| {
                    let (fa, fb) = (starts[i], starts[(i + 1) % k]);
                    fa == p
                        || fb == p
                        || (normal[fa].dot(&np).abs() < tol::ANGULAR_FINE
                            && normal[fb].dot(&np).abs() < tol::ANGULAR_FINE)
                })
                .count()
        };
        let pole = *starts.iter().max_by_key(|&&p| exact(p))?;
        let surf = Surface::Sphere {
            f: Frame::from_xz(pw, m, normal[pole]),
            r: d,
        };
        let (near, sense) = sensed(&surf, pw + m * d, m);
        let mut lp: Vec<Coedge> = order
            .iter()
            .map(|&(e, fwd)| {
                let (id, c, th) = &arc[&(e, w)];
                co(&surf, near, *id, c, 0.0, *th, fwd)
            })
            .collect();
        // At the pole, the loop runs along it in u from the arc coming in to the arc going out.
        let i = (0..k).find(|&i| starts[(i + 1) % k] == pole)?;
        // (Each arc's edge runs over 0..θ.)
        let end_uv = |j: usize, at_end: bool| {
            let (e, fwd) = order[j];
            lp[j]
                .pc
                .point(if fwd == at_end { arc[&(e, w)].2 } else { 0.0 })
        };
        let (a, z) = (end_uv(i, true), end_uv((i + 1) % k, false));
        let pole_v = at(&mut out, pole, w);
        let de = out.degenerate_edge(pw + normal[pole] * d, 0.0, 1.0, pole_v);
        lp.insert(
            i + 1,
            Coedge {
                edge: de,
                fwd: true,
                pc: Curve::Line {
                    o: P3::new(a.x, a.y, 0.0),
                    d: V3::new(z.x - a.x, z.y - a.y, 0.0),
                },
            },
        );
        faces.push(out.face(surf, sense, vec![lp]));
    }
    let shell = out.shell(faces, true);
    out.solid(vec![shell]);
    let body = out.build().ok()?;
    body.check().is_empty().then_some(body)
}

/// A convex body of planes and cylinders, every edge a whole circle or ellipse between two of
/// them (a cylinder cut aslant, a puck), grown by a ball of radius `d`, built face by face:
/// each face moved out by `d`, and round each edge the tube of radius `d` about it, between
/// the curves along which the tube touches the two moved faces. Those curves are known: the
/// edge moved `d` along a plane's normal is the edge again, and moved `d` along a cylinder's
/// it is a conic too (each point's part square to the axis scaled). The solid the slabs' and
/// tubes' union makes, without intersecting a tube with the faces it only touches. None for
/// any other body, or a result that doesn't pass the full check.
fn grown_round(b: &Body, d: f64) -> Option<Body> {
    use crate::native::topo::CheckLevel;
    use crate::native::xbuild::{XBuilder, XFace};
    use std::f64::consts::TAU;
    if b.solids.len() != 1 || b.shells.len() != 1 || !b.shells[0].closed || d <= 0.0 {
        return None;
    }
    if b.faces
        .iter()
        .any(|f| !matches!(f.surf, Surface::Plane { .. } | Surface::Cylinder { .. }))
    {
        return None;
    }
    // Each edge's uses: (face, forward).
    let mut uses: Vec<Vec<(usize, bool)>> = vec![Vec::new(); b.edges.len()];
    for (fi, f) in b.faces.iter().enumerate() {
        for c in f.loops.iter().flatten() {
            uses[c.edge].push((fi, c.fwd));
        }
    }
    let seam = |e: usize| matches!(uses[e][..], [(f, _), (g, _)] if f == g);
    // A conic as its centre and two conjugate radii: the point at t is c + A cos t + B sin t.
    let radii = |c: &Curve| match c {
        Curve::Circle { c, x, y, r } => Some((*c, x * *r, y * *r)),
        Curve::Ellipse { c, x, y, a, b } => Some((*c, x * *a, y * *b)),
        _ => None,
    };
    // The conic of a centre and two conjugate radii, run the same way round.
    let conic = |c: P3, a: V3, b: V3| -> Option<Curve> {
        let (aa, bb, ab) = (a.norm_squared(), b.norm_squared(), a.dot(&b));
        if (aa - bb).abs() <= tol::REL_FINE * (aa + bb) && ab.abs() <= tol::REL_FINE * (aa + bb) {
            let r = aa.sqrt();
            return (r > tol::POINT).then(|| Curve::Circle {
                c,
                x: a / r,
                y: b / bb.sqrt(),
                r,
            });
        }
        // (The radii turned in the parameter until they are square to each other: the axes.)
        let phi = 0.5 * (2.0 * ab).atan2(aa - bb);
        let (major, minor) = (a * phi.cos() + b * phi.sin(), b * phi.cos() - a * phi.sin());
        let (la, lb) = (major.norm(), minor.norm());
        (la > tol::POINT && lb > tol::POINT).then(|| Curve::Ellipse {
            c,
            x: major / la,
            y: minor / lb,
            a: la,
            b: lb,
        })
    };
    // Edge `e` moved `d` along the outward normal of face `fi`, where that is a conic: its
    // centre and conjugate radii, the point at t the edge's point at t moved.
    let moved = |e: usize, fi: usize| -> Option<(P3, V3, V3)> {
        let (c, a, bv) = radii(&b.edges[e].curve)?;
        let f = &b.faces[fi];
        match &f.surf {
            Surface::Plane { f: fr } => {
                let n = fr.x.cross(&fr.y).normalize() * if f.sense { d } else { -d };
                Some((c + n, a, bv))
            }
            // (A cylinder pointing away from its axis: one pointing in is a hole's, and a
            // body with a hole isn't convex.)
            Surface::Cylinder { f: fr, r } if f.sense => {
                let radial = |w: V3| w - fr.z * w.dot(&fr.z);
                let k = d / r;
                Some((
                    c + radial(c - fr.o) * k,
                    a + radial(a) * k,
                    bv + radial(bv) * k,
                ))
            }
            _ => None,
        }
    };
    let mut out = XBuilder::new();
    // The moved edges: (edge, face) -> the new edge.
    let mut ring: std::collections::HashMap<(usize, usize), usize> = Default::default();
    // Each edge's round: its surface and its loops.
    let mut rounds: Vec<XFace> = Vec::new();
    for (e, ed) in b.edges.iter().enumerate() {
        if ed.degenerate {
            return None;
        }
        if seam(e) {
            continue;
        }
        // (The face running the edge backwards first: the round runs that side's curve
        // forwards.)
        let (back, fore) = match uses[e][..] {
            [(f0, false), (f1, true)] => (f0, f1),
            [(f0, true), (f1, false)] => (f1, f0),
            _ => return None,
        };
        let whole = ed.v[0] == ed.v[1] && ((ed.t[1] - ed.t[0]).abs() - TAU).abs() < tol::ANGULAR;
        if !whole || convex(b, e) != Some(true) {
            return None;
        }
        let sides = [moved(e, back)?, moved(e, fore)?];
        let at = |k: usize, t: f64| sides[k].0 + sides[k].1 * t.cos() + sides[k].2 * t.sin();
        // The tube round the edge: the face of it both moved curves lie on.
        let tube = edge_tube(b, e, d).ok()?;
        let near = 1e-6 * (1.0 + d);
        let on = |s: &Surface| {
            (0..2).all(|k| {
                (0..8).all(|i| {
                    let p = at(k, TAU * (i as f64 + 0.37) / 8.0);
                    let (u, v) = s.project(&p, None);
                    (s.eval(u, v) - p).norm() < near
                })
            })
        };
        let surf = tube
            .faces
            .iter()
            .map(|f| &f.surf)
            .find(|s| !matches!(s, Surface::Plane { .. }) && on(s))?
            .clone();
        // A spline tube closes where its path starts, a seam the builder doesn't make: the
        // round's own, the tube's section there from one moved curve to the other, the moved
        // curves starting at its ends. (A torus's is made by the builder.)
        let splined = !surf.u_periodic() && !surf.v_periodic();
        let ts = if splined {
            let ((u0, u1), (v0, v1)) = surf.param_box();
            let (um, vm) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
            let closes = |p: P3, q: P3| (p - q).norm() < near;
            let p = if closes(surf.eval(u0, vm), surf.eval(u1, vm)) {
                surf.eval(u0, vm)
            } else if closes(surf.eval(um, v0), surf.eval(um, v1)) {
                surf.eval(um, v0)
            } else {
                return None;
            };
            ed.curve.closest(&p, ed.t[0], ed.t[1])
        } else {
            ed.t[0]
        };
        let mut new = [0usize; 2];
        let mut ends = [0usize; 2];
        for (k, f) in [back, fore].into_iter().enumerate() {
            let v = out.vertex(at(k, ts));
            let c = conic(sides[k].0, sides[k].1, sides[k].2)?;
            new[k] = out.edge(c, v, v)?;
            ends[k] = v;
            ring.insert((e, f), new[k]);
        }
        let loops = if splined {
            let o = ed.point(ts);
            let x = (at(0, ts) - o).try_normalize(tol::TINY)?;
            let toward = at(1, ts) - o;
            let y = (toward - x * toward.dot(&x)).try_normalize(tol::ANGULAR)?;
            let arc = out.edge(Curve::Circle { c: o, x, y, r: d }, ends[0], ends[1])?;
            vec![vec![
                (new[0], true),
                (arc, true),
                (new[1], false),
                (arc, false),
            ]]
        } else {
            vec![vec![(new[0], true)], vec![(new[1], false)]]
        };
        if crate::native::env_flag!("CADRS_OFFSET_DEBUG") {
            eprintln!(
                "grown round: edge {e}: tube {:?} box {:?}, seam at {ts}",
                surf.kind(),
                surf.param_box(),
            );
        }
        rounds.push(XFace {
            surf,
            sense: true,
            loops,
        });
    }
    let mut faces = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        // Each whole edge of the face a loop of its own (the seam between them is made anew).
        let loops: Vec<Vec<(usize, bool)>> = f
            .loops
            .iter()
            .flatten()
            .filter(|c| !seam(c.edge))
            .map(|c| Some(vec![(*ring.get(&(c.edge, fi))?, c.fwd)]))
            .collect::<Option<_>>()?;
        if loops.is_empty() {
            return None;
        }
        faces.push(out.face(XFace {
            surf: super::sharp::offset_surface(&f.surf, f.sense, d)?,
            sense: f.sense,
            loops,
        }));
    }
    for round in rounds {
        faces.push(out.face(round));
    }
    let shell = out.shell(faces, true);
    out.solid(vec![shell]);
    let (mut g, problems) = out.finish();
    // (A spline tube is a fit: the curves it is trimmed along lie off it by the fit's error,
    // which the edges record, as a union's would: up to a part in 10⁵ of the size.)
    g.fit_tolerances_searched(1e-5 * g.bbox().diagonal());
    let checked = g.check_with(CheckLevel::Full);
    if crate::native::env_flag!("CADRS_OFFSET_DEBUG") {
        eprintln!(
            "grown round: {} faces, problems {problems:?}, check {checked:?}",
            g.faces.len()
        );
    }
    let sound = problems.is_empty()
        && g.solids.len() == 1
        && checked.is_empty()
        && crate::native::mass::body_volume(&g) > crate::native::mass::body_volume(b);
    sound.then_some(g)
}

/// A face's outward normal at a point on it.
fn outward_normal(b: &Body, fi: usize, p: &P3) -> V3 {
    let f = &b.faces[fi];
    let (u, v) = f.surf.project(p, None);
    f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 }
}

/// The body grown by a ball of radius d, but for the parts beyond the planes `cut` (a point
/// and an outward normal: an outward shell's removed faces, beyond which the caller cuts the
/// grown body away): the slab of a face on a cut plane, the tube round an edge of one whose
/// other face doesn't point below the plane, and the ball at a vertex of one whose faces don't
/// (the points within d of those lie beyond the plane or within d of a face's middle, in its
/// slab). Rounds there are the costliest pieces to unite (a tube round a spline touches its
/// faces' slabs along their offsets) and are cut away whole. Nor the slabs of the faces
/// `skip` (an outward shell's curved removed faces, whose slabs the caller cuts away). `b` is
/// [`merged`] already.
fn grown_except(b: &Body, d: f64, cut: &[(P3, V3)], skip: &[usize]) -> Result<Body> {
    // A convex polyhedron: grown whole, directly (what lies beyond a cut plane, which the pieces
    // here leave out, the callers cut away after).
    if skip.is_empty()
        && let Some(g) = grown_convex(b, d)
    {
        return Ok(g);
    }
    // A convex body of planes and cylinders whose edges are whole circles and ellipses:
    // likewise, each face moved and a round along each edge.
    if skip.is_empty()
        && cut.is_empty()
        && !crate::native::env_flag!("CADRS_NO_GROWN_ROUND")
        && let Some(g) = grown_round(b, d)
    {
        return Ok(g);
    }
    // A convex polyhedron drilled through by round holes: the block grown whole, a dimple at
    // each end of a hole the distance closes, a narrowed hole with rounded mouths otherwise.
    if skip.is_empty()
        && cut.is_empty()
        && let Some(g) = grown_closed_holes(b, d)
    {
        return Ok(g);
    }
    // Every edge convex or smooth, of planes, cylinders and tori (a box drilled through, a
    // boss with a root round): the sharp offset with every edge rounded by d (a few milliseconds, where the union
    // of the slabs, tubes and balls takes dozens of booleans).
    if skip.is_empty()
        && cut.is_empty()
        && let Some(g) = grown_filleted(b, &vec![d; b.faces.len()], d)
    {
        return Ok(g);
    }
    // A polyhedron with concave edges: cut into convex parts, each grown whole (growing is
    // the union of its parts' growths), a boolean or two in place of dozens.
    if skip.is_empty()
        && cut.is_empty()
        && let Some(g) = grown_by_convex_parts(b, d, 0)
    {
        return Ok(g);
    }
    let size = 1.0 + b.bbox().diagonal();
    let ef = b.edge_faces();
    // The cut plane each face lies on.
    let on_cut: Vec<Option<V3>> = (0..b.faces.len())
        .map(|fi| {
            let Surface::Plane { f: fr } = &b.faces[fi].surf else {
                return None;
            };
            let n = fr.x.cross(&fr.y).normalize() * if b.faces[fi].sense { 1.0 } else { -1.0 };
            cut.iter()
                .find(|(o, m)| {
                    n.dot(m) > 1.0 - tol::ANGULAR && (fr.o - o).dot(m).abs() < tol::REL * size
                })
                .map(|(_, m)| *m)
        })
        .collect();
    // A face that doesn't point below the cut plane `m` at `p`.
    let above = |fi: usize, m: &V3, p: &P3| outward_normal(b, fi, p).dot(m) >= -tol::ANGULAR;
    let mut pieces: Vec<(Body, String)> = Vec::new();
    for fi in 0..b.faces.len() {
        if on_cut[fi].is_none() && !skip.contains(&fi) {
            pieces.push((
                face_slab(b, fi, d)?,
                format!("the slab of face {fi} ({:?})", b.faces[fi].surf.kind()),
            ));
        }
    }
    for e in 0..b.edges.len() {
        let ed = &b.edges[e];
        if ed.degenerate || convex(b, e) != Some(true) {
            continue;
        }
        let beyond = ef[e].iter().any(|&f| {
            on_cut[f].is_some_and(|m| {
                ef[e].iter().all(|&g| {
                    (0..=8).all(|i| {
                        above(
                            g,
                            &m,
                            &ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / 8.0),
                        )
                    })
                })
            })
        });
        if !beyond {
            pieces.push((edge_tube(b, e, d)?, format!("the tube round edge {e}")));
        }
    }
    // Each vertex's faces.
    let mut vf: Vec<Vec<usize>> = vec![Vec::new(); b.verts.len()];
    for (e, fs) in ef.iter().enumerate() {
        for &v in &b.edges[e].v {
            for &f in fs {
                if !vf[v].contains(&f) {
                    vf[v].push(f);
                }
            }
        }
    }
    let kinds: Vec<Dihedral> = (0..b.edges.len()).map(|e| dihedral(b, e)).collect();
    let mut done: Vec<P3> = Vec::new();
    for e in &b.edges {
        for v in e.v {
            let p = b.verts[v].p;
            if done.iter().any(|q| (q - p).norm() < tol::ROUNDOFF) {
                continue;
            }
            done.push(p);
            if !needs_ball(b, &kinds, v, Dihedral::Convex) {
                continue;
            }
            let beyond = vf[v]
                .iter()
                .any(|&f| on_cut[f].is_some_and(|m| vf[v].iter().all(|&g| above(g, &m, &p))));
            if !beyond {
                pieces.push((ball_body(p, d)?, format!("the ball at {p:?}")));
            }
        }
    }
    combine_all(b.clone(), pieces, BoolOp::Union)
}

/// A polyhedron (every face a plane) grown by a ball of radius `d` as the union of its convex
/// parts grown ([`grown_convex`]): a ball's sum distributes over a union, so the body cut along
/// the plane of a face beside a concave edge grows as its two sides grown and united. None
/// where a face isn't a plane, a side falls apart or isn't one solid, or the parts go deeper
/// than a few cuts (the slabs', tubes' and balls' union then).
fn grown_by_convex_parts(b: &Body, d: f64, depth: usize) -> Option<Body> {
    if let Some(g) = grown_convex(b, d) {
        return Some(g);
    }
    if depth >= 4
        || b.solids.len() != 1
        || !b
            .faces
            .iter()
            .all(|f| matches!(f.surf, Surface::Plane { .. }))
    {
        return None;
    }
    let ef = b.edge_faces();
    let e = (0..b.edges.len()).find(|&e| matches!(dihedral(b, e), Dihedral::Concave))?;
    let f = *ef[e].first()?;
    let Surface::Plane { f: fr } = &b.faces[f].surf else {
        return None;
    };
    let n = fr.x.cross(&fr.y).normalize() * if b.faces[f].sense { 1.0 } else { -1.0 };
    let size = 4.0 * (1.0 + b.bbox().diagonal());
    let mut grown: Vec<Body> = Vec::new();
    for side in [n, -n] {
        let h = half_space(fr.o, side, size).ok()?;
        let part = boolean_bodies(b, &h, BoolOp::Intersect).ok()?.body;
        if part.solids.len() != 1 {
            return None;
        }
        grown.push(grown_by_convex_parts(&part, d, depth + 1)?);
    }
    let [x, y] = <[Body; 2]>::try_from(grown).ok()?;
    let out = boolean_bodies(&x, &y, BoolOp::Union).ok()?.body;
    (out.solids.len() == 1
        && out
            .check_with(crate::native::topo::CheckLevel::Full)
            .is_empty())
    .then_some(out)
}

/// `acc` combined (`op`) with each piece in turn. A piece whose boolean fails is tried again
/// after the others: a union or difference of pieces doesn't depend on their order, but the
/// touching configurations a boolean has to settle do (a slab's side against a neighbour's
/// slab, not yet merged into a face). Fails when a round makes no progress, or after three
/// rounds (each failed boolean costs its retries).
fn combine_all(acc: Body, todo: Vec<(Body, String)>, op: BoolOp) -> Result<Body> {
    // (Batched first; where that ends in failure, piece by piece from the start, as before.)
    // (Only for many: a few pieces gain little, and a batch that fails costs a second pass.)
    if op == BoolOp::Union
        && todo.len() >= 8
        && !crate::native::env_flag!("CADRS_NO_PIECE_BATCHES")
        && let Ok(r) = combine_batched(acc.clone(), todo.clone(), op)
    {
        return Ok(r);
    }
    combine_in_turn(acc, todo, op)
}

/// A batch of an offset's pieces that keep apart: the body they make together, their boxes,
/// and the pieces themselves (to go one at a time if the batch fails).
type Batch = (Body, Vec<Bbox>, Vec<(Body, String)>);

/// [`combine_all`] with the pieces that keep apart united in batches.
fn combine_batched(mut acc: Body, mut todo: Vec<(Body, String)>, op: BoolOp) -> Result<Body> {
    // A union of many pieces: those whose boxes keep apart taken together, as one body of
    // several solids, so each union pays its fixed costs (the body's import, its check) once
    // for the batch, not once a piece. A batch the union fails on goes piece by piece below.
    {
        let pad = 1e-6 * (1.0 + acc.bbox().diagonal());
        let mut batches: Vec<Batch> = Vec::new();
        for (piece, what) in todo {
            // (Grown by a hundredth of its size too: a box sampled off curved faces may fall a
            // hair short of them, and two pieces that touch must not share a batch.)
            let bb0 = piece.bbox();
            let bb = bb0.grow(pad + 0.01 * bb0.diagonal());
            match batches
                .iter_mut()
                .find(|b| !b.1.iter().any(|o| o.overlaps(&bb)))
            {
                Some(b) => {
                    b.0.append(&piece);
                    b.1.push(bb);
                    b.2.push((piece, what));
                }
                None => batches.push((piece.clone(), vec![bb], vec![(piece, what)])),
            }
        }
        let mut left = Vec::new();
        for (body, _, pieces) in batches {
            if pieces.len() == 1 {
                left.extend(pieces);
                continue;
            }
            match combine(&acc, &body, op, &|| {
                format!("{} pieces together", pieces.len())
            }) {
                Ok(r) => acc = r,
                Err(_) => left.extend(pieces),
            }
        }
        todo = left;
        if todo.is_empty() {
            return Ok(acc);
        }
    }
    combine_in_turn(acc, todo, op)
}

/// The pieces united (or subtracted) one at a time, those that fail tried again after the
/// rest, up to three rounds.
fn combine_in_turn(mut acc: Body, mut todo: Vec<(Body, String)>, op: BoolOp) -> Result<Body> {
    for round in 0..3 {
        let n = todo.len();
        let mut left = Vec::new();
        let mut first: Option<KernelError> = None;
        for (piece, what) in todo {
            match combine(&acc, &piece, op, &|| what.clone()) {
                Ok(r) => {
                    acc = r;
                    if crate::native::env_flag!("CADRS_SHELL_DEBUG") {
                        eprintln!(
                            "after {what}: {:.6} solids {}",
                            crate::native::mass::body_volume(&acc),
                            acc.solids.len()
                        );
                    }
                }
                Err(e) => {
                    first.get_or_insert(e);
                    left.push((piece, what));
                }
            }
        }
        match first {
            None => return Ok(acc),
            Some(e) if left.len() == n || round == 2 => return Err(e),
            Some(_) => todo = left,
        }
    }
    Err(KernelError::InvalidResult {
        op: "offset",
        why: "the offset's pieces don't combine".into(),
    })
}

pub fn shell(k: &mut NativeKernel, body: BodyId, spec: &ShellSpec) -> Result<OpResult> {
    let b = std::sync::Arc::clone(k.get(body)?);
    let t = spec.thickness;
    if !(t > 0.0) {
        return Err(KernelError::InvalidParameter(
            "The shell's thickness must be positive".into(),
        ));
    }
    let remove: Vec<usize> = if spec.hollow {
        vec![]
    } else {
        spec.remove.iter().map(|f| f.0 as usize).collect()
    };
    if remove.is_empty() && !spec.hollow {
        return Err(KernelError::InvalidParameter(
            "Pick the faces to remove, or make the shell hollow".into(),
        ));
    }
    if remove.iter().any(|&f| f >= b.faces.len()) {
        return Err(KernelError::InvalidParameter("unknown face".into()));
    }
    let size = b.bbox().diagonal();
    // Walls closer than twice the thickness: the faces moved in cross over each other, so no
    // sharp inset exists. Rounded, the cavity from its definition leaves the body solid where
    // they meet (OCCT's thick solids do the same); with no cavity left, or sharp, an error.
    let crossing = !spec.outward && walls_cross(&b, &remove, t);
    let too_thick = || {
        KernelError::InvalidParameter(
            "The shell's walls would intersect themselves; reduce the thickness".into(),
        )
    };
    let cur = Tracked::of((*b).clone());
    let v0 = crate::native::mass::body_volume(&b);
    // Walls whose surfaces the whole body lies within (a box's sides, a bore, a ball's sphere):
    // the cavity is where the moved surfaces' insides meet in the body, as far as the openings.
    let supported = if spec.outward || spec.hollow {
        None
    } else {
        cavity_of_supports(&b, &remove, t)
    };
    if crate::native::env_flag!("CADRS_SHELL_DEBUG") {
        eprintln!(
            "shell: cavity of supporting walls: {:?}",
            supported
                .as_ref()
                .map(|r| r.as_ref().map(|c| c.as_ref().map(|c| c.faces.len())))
        );
    }
    let result = if let Some(Ok(cavity)) = supported {
        match cavity {
            // Every point of the body within t of a wall: nothing is hollowed. Walls that just
            // meet (a hair thinner would leave a cavity) leave the body itself; a shell thicker
            // than that is as thick as the part, and refused, as Onshape and OCCT do.
            None => {
                let thinner = t - tol::POINT.max(1e-9 * size);
                let meeting =
                    || matches!(cavity_of_supports(&b, &remove, thinner), Some(Ok(Some(_))));
                match spec.nothing_hollowed {
                    NothingHollowed::Unchanged => cur,
                    NothingHollowed::RefuseUnlessMeeting if meeting() => cur,
                    _ => {
                        return Err(KernelError::InvalidParameter(
                            "The shell is as thick as the part, nothing is left to hollow; reduce the thickness".into(),
                        ));
                    }
                }
            }
            Some(cavity) => cur.apply(
                &cavity,
                &vec![Vec::new(); cavity.faces.len()],
                BoolOp::Subtract,
            )?,
        }
    } else if crossing && spec.sharp {
        return Err(too_thick());
    } else if !spec.outward {
        // Faces the thickness takes past their own radius (a round of radius 2 under a wall 3
        // thick) are no part of the cavity's boundary: the cavity is that of the body without
        // them, the faces round them run on to meet ([`super::without_vanishing`]).
        let inward: Vec<f64> = (0..b.faces.len())
            .map(|f| if remove.contains(&f) { 0.0 } else { -t })
            .collect();
        let (b, remove) = match super::without_vanishing(&b, &inward) {
            Some((b2, from)) => {
                let remove: Vec<usize> = (0..b2.faces.len())
                    .filter(|&f| from[f].iter().any(|g| remove.contains(g)))
                    .collect();
                (b2, remove)
            }
            None => ((*b).clone(), remove.clone()),
        };
        // The cavity: kept faces moved in by t, removed faces moved out past the body, or,
        // failing that (a spline wall's offset doesn't reach that far), where they are (the
        // cavity opens flush with them).
        let dist: Vec<f64> = (0..b.faces.len())
            .map(|f| {
                if remove.contains(&f) {
                    size * 0.1 + crate::native::tol::small(size)
                } else {
                    -t
                }
            })
            .collect();
        let flush: Vec<f64> = dist
            .iter()
            .enumerate()
            .map(|(f, d)| if remove.contains(&f) { 0.0 } else { *d })
            .collect();
        // Concave edges between kept faces round the cavity (the walls are t thick
        // everywhere): build it from its definition then. Each way of making the cavity is
        // tried in turn until the body less it comes out (a cavity flush with a removed face
        // shares that face with the body, which the boolean may not settle).
        // (An edge beside an opening has no wall on that side to round: the face moved in runs
        // on to the opening's surface, as the sharp cavity has it. An L opened at its step.)
        let ef = b.edge_faces();
        let concave = (0..b.edges.len()).any(|e| {
            !b.edges[e].degenerate
                && convex(&b, e) == Some(false)
                && ef[e].iter().all(|f| !remove.contains(f))
        });
        let sharp = |x: &Vec<f64>| {
            super::sharp::offset_sharp(&b, x).and_then(|c| {
                if crate::native::mass::body_volume(&c) > 0.0 {
                    Ok(c)
                } else {
                    Err(KernelError::TooLarge {
                        op: "shell",
                        why: "the inset is empty".into(),
                    })
                }
            })
        };
        let regions = || cavity_by_regions(&b, &remove, t);
        // (The cavity flush with the openings, made once: for its volume below and as the
        // second way.)
        let flush_made: std::cell::OnceCell<Result<Body>> = std::cell::OnceCell::new();
        let flush_cavity = || flush_made.get_or_init(|| sharp(&flush)).clone();
        let ways: Vec<Box<dyn Fn() -> Result<Body>>> = if (concave || crossing) && !spec.sharp {
            // A prism opened at a cap, or hollowed: its outline's inset, rounded about the
            // reflex corners, extruded from the kept caps' insets to past the removed ones.
            match super::prism::curved_prism_inset(&b, &dist, true, false) {
                Some(Ok(c)) => vec![Box::new(move || Ok(c.clone())), Box::new(regions)],
                _ => vec![Box::new(regions)],
            }
        } else {
            vec![
                Box::new(|| sharp(&dist)),
                Box::new(flush_cavity),
                Box::new(regions),
            ]
        };
        // The cavity with the removed faces moved out past the body reaches past them by the
        // walls' insets: where a kept face rises from the opening beyond its plane (an L opened
        // at its low step, issue #237), the corner of the moved face and that face's inset lies
        // inside its wall, and the cavity eats it. The cavity flush with the openings doesn't,
        // and lies wholly in the body: the moved one may take out no more than it does.
        let moved = !((concave || crossing) && !spec.sharp);
        // (Only where some face reaches past a removed face's plane, by its box: else no wall
        // rises beyond an opening for the cavity past it to cut into.)
        let size_tol = tol::POINT.max(tol::REL * size);
        let past_openings = remove.iter().any(|&f| {
            let Surface::Plane { f: fr } = &b.faces[f].surf else {
                return true;
            };
            let n = fr.x.cross(&fr.y).normalize() * if b.faces[f].sense { 1.0 } else { -1.0 };
            (0..b.faces.len()).any(|g| {
                let bb = b.face_bbox(g);
                let far = P3::new(
                    if n.x > 0.0 { bb.max.x } else { bb.min.x },
                    if n.y > 0.0 { bb.max.y } else { bb.min.y },
                    if n.z > 0.0 { bb.max.z } else { bb.min.z },
                );
                (far - fr.o).dot(&n) > size_tol
            })
        });
        let flush_volume = if moved && past_openings {
            flush_cavity()
                .ok()
                .map(|c| crate::native::mass::body_volume(&c))
        } else {
            None
        };
        let mut first: Option<KernelError> = None;
        let mut done = None;
        for (i, way) in ways.iter().enumerate() {
            let out = way().and_then(|cavity| {
                let mut out = cur.apply(
                    &cavity,
                    &vec![Vec::new(); cavity.faces.len()],
                    BoolOp::Subtract,
                )?;
                if spec.hollow {
                    // A closed void: the body with the cavity as an inner shell.
                    if out.body.solids.len() != 1 {
                        return Err(KernelError::InvalidResult {
                            op: "shell",
                            why: "the hollow shell isn't one solid".into(),
                        });
                    }
                    out.body.solids[0].shells.sort();
                }
                let v1 = crate::native::mass::body_volume(&out.body);
                if v1 <= tol::ROUNDOFF || v1 > v0 + tol::ROUNDOFF {
                    return Err(KernelError::InvalidResult {
                        op: "shell",
                        why: "the shell came out wrong".into(),
                    });
                }
                if moved
                    && i == 0
                    && let Some(fv) = flush_volume
                    && v0 - v1 > fv + 1e-7 * v0.max(1.0)
                {
                    return Err(KernelError::InvalidResult {
                        op: "shell",
                        why: "the cavity past the opening cut into a wall".into(),
                    });
                }
                // Walls that meet everywhere leave no cavity.
                if crossing && v1 >= v0 * (1.0 - tol::REL) {
                    return Err(too_thick());
                }
                Ok(out)
            });
            match out {
                Ok(o) => {
                    done = Some(o);
                    break;
                }
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        match (done, first) {
            (Some(o), _) => o,
            (None, Some(_)) if crossing => return Err(too_thick()),
            (None, Some(e)) => return Err(e),
            (None, None) => {
                return Err(KernelError::TooLarge {
                    op: "shell",
                    why: "no cavity".into(),
                });
            }
        }
    } else {
        // Outward: the body grown by a ball, less the body, cut back at the removed faces.
        let rounded = || -> Result<Tracked> {
            let mb = merged(&b);
            let remove: Vec<usize> = remove
                .iter()
                .map(|&f| face_in(&b, f, &mb))
                .collect::<Option<_>>()
                .ok_or(KernelError::Unsupported(
                    "a removed face merged with its neighbour in an outward shell",
                ))?;
            // A curved face is cut away with its slab, which ends square to its edges: where
            // they are smooth, as the neighbours' walls do (an opening across a fillet).
            let curved: Vec<usize> = remove
                .iter()
                .copied()
                .filter(|&f| !matches!(mb.faces[f].surf, Surface::Plane { .. }))
                .collect();
            let sharp_edge = curved.iter().any(|&f| {
                mb.faces[f].loops.iter().flatten().any(|c| {
                    !mb.edges[c.edge].degenerate && dihedral(&mb, c.edge) != Dihedral::Smooth
                })
            });
            if sharp_edge {
                return Err(KernelError::Unsupported(
                    "removing a curved face with a sharp edge from an outward shell",
                ));
            }
            // (Not the rounds beyond the removed faces, which are cut away.)
            let cut: Vec<(P3, V3)> = remove
                .iter()
                .filter_map(|&f| match &mb.faces[f].surf {
                    Surface::Plane { f: fr } => Some((
                        fr.o,
                        fr.x.cross(&fr.y).normalize() * if mb.faces[f].sense { 1.0 } else { -1.0 },
                    )),
                    _ => None,
                })
                .collect();
            let grown = grown_except(&mb, t, &cut, &curved)?;
            let mut walls = Tracked {
                src: vec![Vec::new(); grown.faces.len()],
                body: grown,
            };
            for &f in &remove {
                let face = &mb.faces[f];
                let Surface::Plane { f: fr } = &face.surf else {
                    // Its slab, past the walls (whatever else reaches over it).
                    let slab = face_slab(&mb, f, 2.0 * t)?;
                    walls = walls.apply(
                        &slab,
                        &vec![Vec::new(); slab.faces.len()],
                        BoolOp::Subtract,
                    )?;
                    continue;
                };
                let n = fr.x.cross(&fr.y) * if face.sense { 1.0 } else { -1.0 };
                let mut hs = half_space(fr.o, n, size * 4.0 + t * 4.0 + 10.0)?;
                // The walls over faces tangent to the removed one round over its plane as
                // everywhere else (a filleted rim): only the opening goes straight through.
                let keep = tangent_slabs_beyond(&mb, t, fr.o, n.normalize(), &remove)?;
                if !keep.is_empty() {
                    hs = combine_all(hs, keep, BoolOp::Subtract)?;
                }
                // Nor the walls of the body's part beyond the plane (a leg of an L rising past
                // its opened step, issue #237): they are that part grown by t.
                // (None when the body's faces are flat and its corners all lie on or behind the
                // plane: it lies within their hull, a convex block opened at a face.)
                let m = n.normalize();
                let behind = mb
                    .faces
                    .iter()
                    .all(|g| matches!(g.surf, Surface::Plane { .. }))
                    && mb
                        .verts
                        .iter()
                        .all(|v| (v.p - fr.o).dot(&m) <= tol::POINT.max(tol::REL * size));
                let beyond = if behind {
                    None
                } else {
                    match combine(&mb, &hs, BoolOp::Intersect, &|| {
                        format!("the body beyond removed face {f}")
                    }) {
                        Ok(x) => Some(x),
                        Err(KernelError::EmptyResult(_)) => None,
                        Err(e) => return Err(e),
                    }
                };
                if let Some(beyond) = beyond
                    && crate::native::mass::body_volume(&beyond) > tol::ROUNDOFF
                {
                    let walls_beyond = offset_rounded(&beyond, &vec![t; beyond.faces.len()])?;
                    hs = combine(&hs, &walls_beyond, BoolOp::Subtract, &|| {
                        format!("the walls beyond removed face {f}")
                    })?;
                }
                walls = walls.apply(&hs, &vec![Vec::new(); hs.faces.len()], BoolOp::Subtract)?;
            }
            // walls − body: the body's faces bound the inside.
            let r = boolean_bodies(&walls.body, &b, BoolOp::Subtract)?;
            let src = r
                .from
                .iter()
                .map(|s| {
                    s.iter()
                        .filter(|(o, _)| *o == 1)
                        .map(|(_, f)| crate::native::blend::FaceSrc::Input(*f))
                        .collect()
                })
                .collect();
            Ok(Tracked { body: r.body, src })
        };
        // With sharp corners: the kept faces moved out by t (the removed ones staying where
        // they are, so the walls end flush with them), less the body. They differ from rounded
        // ones only at a convex edge between kept faces; where the sharp walls can't be made
        // (an offset face that doesn't reach its neighbour, a boolean that fails), rounded.
        let ef = b.edge_faces();
        let corners = (0..b.edges.len()).any(|e| {
            !b.edges[e].degenerate
                && convex(&b, e) == Some(true)
                && ef[e].iter().all(|f| !remove.contains(f))
        });
        let sharp = || -> Result<Tracked> {
            let dist: Vec<f64> = (0..b.faces.len())
                .map(|f| if remove.contains(&f) { 0.0 } else { t })
                .collect();
            let grown = super::sharp::offset_sharp(&b, &dist)?;
            let r = boolean_bodies(&grown, &b, BoolOp::Subtract)?;
            let src = r
                .from
                .iter()
                .map(|s| {
                    s.iter()
                        .filter(|(o, _)| *o == 1)
                        .map(|(_, f)| crate::native::blend::FaceSrc::Input(*f))
                        .collect()
                })
                .collect();
            Ok(Tracked { body: r.body, src })
        };
        if spec.sharp && corners {
            match sharp() {
                Ok(r) => r,
                Err(e) => {
                    if crate::native::env_flag!("CADRS_SHELL_DEBUG") {
                        eprintln!("shell: sharp outward walls failed ({e}); rounded");
                    }
                    rounded()?
                }
            }
        } else {
            // (With no convex edge between kept faces the rounded walls are the sharp ones:
            // where the rounded can't be made, a curved opening with a sharp rim, those.)
            match rounded() {
                Ok(r) => r,
                Err(e) if !corners => {
                    if crate::native::env_flag!("CADRS_SHELL_DEBUG") {
                        eprintln!("shell: rounded outward walls failed ({e}); sharp");
                    }
                    sharp().map_err(|_| e)?
                }
                Err(e) => return Err(e),
            }
        }
    };
    // The walls' faces the booleans couldn't trace (the cavity's, the grown body's): the kept
    // faces' offsets (modified) and the rounds (generated), from where they lie.
    let mut result = result;
    let dist: Vec<f64> = (0..b.faces.len())
        .map(|f| {
            if remove.contains(&f) {
                f64::NAN
            } else if spec.outward {
                t
            } else {
                -t
            }
        })
        .collect();
    let found = offset_sources(&b, body, &result.body, &dist, !spec.sharp);
    for (s, f) in result.src.iter_mut().zip(found) {
        if s.is_empty() {
            *s = f;
        }
    }
    let v1 = crate::native::mass::body_volume(&result.body);
    if crate::native::env_flag!("CADRS_SHELL_DEBUG") {
        eprintln!(
            "shell: body {v0} result {v1} solids {} faces {}",
            result.body.solids.len(),
            result.body.faces.len()
        );
    }
    if v1 <= tol::ROUNDOFF || (!spec.outward && v1 > v0 + tol::ROUNDOFF) {
        return Err(KernelError::InvalidResult {
            op: "shell",
            why: "the shell came out wrong".into(),
        });
    }
    Ok(result.into_result(k, body))
}

/// What each face of `out` came from, when `out` is bounded by offsets of `b`'s faces by `dist`
/// (a face not offset at `f64::NAN`) and their rounds: the faces whose offset surface it lies on
/// ([`FaceSrc::Input`], the face moved: modified), else the edge or vertex its round is about
/// ([`FaceSrc::New`], generated: it lies the distance from that edge or vertex). Names survive
/// an offset however its pieces were united. Empty for a face that is neither.
pub fn offset_sources(
    b: &Body,
    input: BodyId,
    out: &Body,
    dist: &[f64],
    rounded: bool,
) -> Vec<Vec<crate::native::blend::FaceSrc>> {
    use crate::native::blend::FaceSrc;
    let size = 1.0 + b.bbox().diagonal();
    let moved: Vec<Option<Surface>> = b
        .faces
        .iter()
        .zip(dist)
        .map(|(f, d)| {
            if d.is_nan() {
                None
            } else if *d == 0.0 {
                Some(f.surf.clone())
            } else {
                super::sharp::offset_surface(&f.surf, f.sense, *d)
            }
        })
        .collect();
    let reach = dist
        .iter()
        .filter(|d| !d.is_nan())
        .fold(0.0f64, |m, d| m.max(d.abs()));
    // A sharp offset's step between neighbours moved apart (or a face that closes over a slot
    // their moves filled): from the input edges it holds, on its plane and within it.
    let steps = |f: &crate::native::topo::Face| -> Vec<FaceSrc> {
        if !matches!(f.surf, Surface::Plane { .. }) {
            return Vec::new();
        }
        let tol = 1e-7 * size;
        let polys = crate::native::classify::face_polygons(out, f);
        (0..b.edges.len())
            .filter(|&e| !b.edges[e].degenerate)
            .filter(|&e| {
                let ed = &b.edges[e];
                let m = ed.point(ed.mid_t());
                if f.surf.distance(&m) > tol {
                    return false;
                }
                let (u, v) = f.surf.project(&m, None);
                !matches!(
                    crate::native::classify::in_face(&polys, &f.surf, P2::new(u, v), tol),
                    crate::native::classify::InFace::Outside
                )
            })
            .map(|e| {
                FaceSrc::New(Origin::FromEdge {
                    body: input,
                    edge: EdgeId(e as u64),
                })
            })
            .collect()
    };
    out.faces
        .iter()
        .map(|f| {
            // A point of the face: the middle of its outer loop in (u, v), on its surface.
            let mut sum = P2::origin();
            let mut n = 0.0;
            for c in f.loops.first().into_iter().flatten() {
                let e = &out.edges[c.edge];
                for i in 0..8 {
                    let q =
                        c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * (i as f64 + 0.5) / 8.0);
                    sum += q.coords.xy();
                    n += 1.0;
                }
            }
            if n == 0.0 {
                return Vec::new();
            }
            let p = f.surf.eval(sum.x / n, sum.y / n);
            // (Offsets of splines are fits.)
            let near = |s: &Surface| {
                s.distance(&p)
                    < if matches!(s, Surface::Nurbs(_)) {
                        1e-5
                    } else {
                        1e-7
                    } * size
            };
            let faces: Vec<FaceSrc> = (0..b.faces.len())
                .filter(|&fi| moved[fi].as_ref().is_some_and(near))
                .map(FaceSrc::Input)
                .collect();
            if !faces.is_empty() || reach == 0.0 {
                return faces;
            }
            if !rounded {
                return steps(f);
            }
            // A round: the edge it lies the distance from (nearest within the edge, not at an
            // end: a corner's ball is that far from its edges' ends too), else the vertex.
            let tol = tol::REL_POINT * size;
            let off = |d: f64| (d - reach).abs();
            let edge = (0..b.edges.len())
                .filter(|&e| !b.edges[e].degenerate)
                .filter_map(|e| {
                    let ed = &b.edges[e];
                    let t = ed.curve.closest(&p, ed.t[0], ed.t[1]);
                    let inside = (t - ed.t[0]).abs().min((ed.t[1] - t).abs())
                        > tol::REL * (ed.t[1] - ed.t[0]).abs();
                    inside.then(|| (e, off((ed.curve.point(t) - p).norm())))
                })
                .filter(|x| x.1 < tol)
                .min_by(|a, c| a.1.total_cmp(&c.1));
            if let Some((e, _)) = edge {
                return vec![FaceSrc::New(Origin::FromEdge {
                    body: input,
                    edge: EdgeId(e as u64),
                })];
            }
            if let Some(v) = (0..b.verts.len()).find(|&v| off((b.verts[v].p - p).norm()) < tol) {
                return vec![FaceSrc::New(Origin::FromVertex {
                    body: input,
                    vertex: VertexId(v as u64),
                })];
            }
            steps(f)
        })
        .collect()
}

/// The face of `mb` (`b` [`merged`]) that face `fi` of `b` is, or lies in: on its surface,
/// its vertices within the face's box.
fn face_in(b: &Body, fi: usize, mb: &Body) -> Option<usize> {
    if mb.faces.len() == b.faces.len() {
        return Some(fi);
    }
    let f = &b.faces[fi];
    let pts: Vec<P3> = f
        .loops
        .iter()
        .flatten()
        .flat_map(|c| b.edges[c.edge].v)
        .map(|v| b.verts[v].p)
        .collect();
    let size = 1.0 + b.bbox().diagonal();
    (0..mb.faces.len()).find(|&g| {
        let h = &mb.faces[g];
        if h.surf.kind() != f.surf.kind() || h.sense != f.sense {
            return false;
        }
        let bb = crate::native::kernel::subset(mb, &[g], false)
            .bbox()
            .grow(tol::REL * size);
        pts.iter().all(|p| {
            let (u, v) = h.surf.project(p, None);
            (h.surf.eval(u, v) - p).norm() < tol::REL * size && bb.contains(p)
        })
    })
}

/// The slabs (by `d`) of the faces that reach beyond the plane through `o` along `m` (a removed
/// face's, of an outward shell) and are tangent to the faces on it: those joined to them
/// through smooth edges, but for any with a sharp edge on them. Beyond the plane an outward
/// shell's walls stop flush at a sharp edge of the opening (the removed face's plane extended)
/// but round over a tangent one (the face's offset carried on): the cut there keeps these,
/// but for the faces `removed` (other openings: the chain runs on through them). `b` is
/// [`merged`] already.
fn tangent_slabs_beyond(
    b: &Body,
    d: f64,
    o: P3,
    m: V3,
    removed: &[usize],
) -> Result<Vec<(Body, String)>> {
    let size = 1.0 + b.bbox().diagonal();
    let on_plane: Vec<bool> = (0..b.faces.len())
        .map(|fi| {
            let Surface::Plane { f: fr } = &b.faces[fi].surf else {
                return false;
            };
            let n = fr.x.cross(&fr.y).normalize() * if b.faces[fi].sense { 1.0 } else { -1.0 };
            n.dot(&m) > 1.0 - tol::ANGULAR && (fr.o - o).dot(&m).abs() < tol::REL * size
        })
        .collect();
    let ef = b.edge_faces();
    let kinds: Vec<Dihedral> = (0..b.edges.len()).map(|e| dihedral(b, e)).collect();
    // Faces with a sharp edge on the plane's faces stop flush there.
    let mut sharp_to_plane = vec![false; b.faces.len()];
    for (e, fs) in ef.iter().enumerate() {
        if kinds[e] != Dihedral::Smooth && fs.iter().any(|&f| on_plane[f]) {
            for &f in fs {
                sharp_to_plane[f] = true;
            }
        }
    }
    let mut tangent = vec![false; b.faces.len()];
    let mut todo: Vec<usize> = (0..b.faces.len()).filter(|&f| on_plane[f]).collect();
    while let Some(f) = todo.pop() {
        for (e, fs) in ef.iter().enumerate() {
            if kinds[e] != Dihedral::Smooth || !fs.contains(&f) {
                continue;
            }
            for &g in fs {
                if !on_plane[g] && !tangent[g] {
                    tangent[g] = true;
                    todo.push(g);
                }
            }
        }
    }
    let mut out = Vec::new();
    for fi in 0..b.faces.len() {
        if !tangent[fi] || sharp_to_plane[fi] || removed.contains(&fi) {
            continue;
        }
        let bb = crate::native::kernel::subset(b, &[fi], false)
            .bbox()
            .grow(d);
        let reach = (0..8)
            .map(|i| {
                let c = P3::new(
                    if i & 1 == 0 { bb.min.x } else { bb.max.x },
                    if i & 2 == 0 { bb.min.y } else { bb.max.y },
                    if i & 4 == 0 { bb.min.z } else { bb.max.z },
                );
                (c - o).dot(&m)
            })
            .fold(f64::NEG_INFINITY, f64::max);
        if reach > tol::REL * size {
            out.push((
                face_slab(b, fi, d)?,
                format!("the slab of face {fi}, tangent to the opening"),
            ));
        }
    }
    Ok(out)
}

/// A large box beyond the plane through `o` along the normal `n`.
fn half_space(o: P3, n: V3, size: f64) -> Result<Body> {
    let f = Frame::from_z(o, n);
    let c = |x: f64, y: f64, z: f64| f.o + f.x * x + f.y * y + f.z * z;
    crate::native::blend::hexahedron(&[
        c(-size, -size, 0.0),
        c(size, -size, 0.0),
        c(size, size, 0.0),
        c(-size, size, 0.0),
        c(-size, -size, size),
        c(size, -size, size),
        c(size, size, size),
        c(-size, size, size),
    ])
}

/// The outer half of the tube of radius `r` round edge `e` (a planar curve bending one way,
/// its `faces`' outward normals on its outer side all along): the half disc on the side away
/// from the centre of curvature, its straight side through the edge and square to its plane,
/// swept along it. `None` where the edge isn't such a curve (`Some(Err)` where the sweep fails).
fn outer_half_tube(b: &Body, e: usize, r: f64, faces: &[usize]) -> Option<Result<Body>> {
    let ed = &b.edges[e];
    let n = 64;
    let at = |i: usize| ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / n as f64;
    // The plane's normal (the binormal), one all along; the outer side by it.
    let binormal = |t: f64| {
        let (_, d1, d2) = ed.curve.d2(t);
        d1.cross(&d2).try_normalize(tol::DEGENERATE)
    };
    let up = binormal(at(0))?;
    for i in 0..=n {
        let t = at(i);
        if binormal(t)?.dot(&up) < 1.0 - tol::ANGULAR_POINT.sqrt() {
            return None;
        }
        let (p, d1, _) = ed.curve.d2(t);
        // (Out of the curve's bend: the tangent turned a quarter about the binormal, away
        // from the centre of curvature.)
        let out = d1.cross(&up).try_normalize(tol::DEGENERATE)?;
        if faces
            .iter()
            .any(|&f| outward_normal(b, f, &p).normalize().dot(&out) < -tol::ANGULAR_POINT.sqrt())
        {
            return None;
        }
    }
    let (p0, d1, _) = ed.curve.d2(ed.t[0]);
    let t = d1.try_normalize(tol::DEGENERATE)?;
    let out = t.cross(&up);
    let plane = Plane {
        origin: p0,
        x_dir: nalgebra::Unit::new_normalize(out),
        normal: nalgebra::Unit::new_normalize(t),
    };
    let half = Loop {
        curves: vec![
            Curve2::Arc {
                center: nalgebra::Point2::origin(),
                radius: r,
                start_angle: -std::f64::consts::FRAC_PI_2,
                sweep: PI,
                source: None,
            },
            Curve2::Line {
                a: nalgebra::Point2::new(0.0, r),
                b: nalgebra::Point2::new(0.0, -r),
                source: None,
            },
        ],
    };
    let prof = Profile::new(
        plane,
        vec![Region {
            outer: half,
            holes: vec![],
            source: None,
        }],
    );
    let mut k = NativeKernel::new();
    let body = k.insert(b.clone());
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path: vec![PathCurve::Edge {
            body,
            edge: EdgeId(e as u64),
        }],
        control: SweepControl::None,
        corners: Default::default(),
        faces: vec![],
    };
    Some(
        k.sweep_with(&prof, &spec)
            .and_then(|out| match out.bodies[..] {
                [id] => Ok((**k.get(id)?).clone()),
                _ => Err(KernelError::InvalidResult {
                    op: "offset",
                    why: "a tube round an edge isn't one body".into(),
                }),
            }),
    )
}

/// Walls facing each other across the body closer than twice the thickness (kept, planar,
/// anti-parallel, overlapping).
fn walls_cross(b: &Body, remove: &[usize], t: f64) -> bool {
    let planes: Vec<(usize, P3, V3, Bbox)> = b
        .faces
        .iter()
        .enumerate()
        .filter(|(i, _)| !remove.contains(i))
        .filter_map(|(i, f)| match &f.surf {
            Surface::Plane { f: fr } => {
                let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
                Some((i, fr.o, n, b.face_bbox(i)))
            }
            _ => None,
        })
        .collect();
    for (i, oa, na, ba) in &planes {
        for (j, ob, nb, bb) in &planes {
            if i >= j || na.dot(nb) > -1.0 + tol::ANGULAR {
                continue;
            }
            let gap = (ob - oa).dot(na).abs();
            // Overlap of the faces' boxes across the gap: the second's carried along the
            // normal onto the first's plane (faces apart sideways stay apart).
            let across = na * (oa - ob).dot(na);
            let carried = Bbox {
                min: bb.min + across,
                max: bb.max + across,
            };
            if gap < 2.0 * t + tol::ROUNDOFF && ba.grow(tol::POINT).overlaps(&carried) {
                return true;
            }
        }
    }
    false
}

/// How a wall's surface bounds the body: the body lies on one side of the whole surface (inside a
/// sphere or a cylinder, outside one, behind a plane).
enum Support {
    /// The half-space behind the plane through `o` with outward normal `n`.
    Plane(P3, V3),
    /// Inside (true) or outside the cylinder about the axis (o, z) of radius r.
    Cylinder(P3, V3, f64, bool),
    /// Inside (true) or outside the sphere of radius r about the frame's origin (the frame the
    /// face's own: a ball made for it keeps its poles and seam).
    Sphere(Frame, f64, bool),
    /// Inside (true) or outside the cone about the axis (o, z): of radius `r` at o, its radius
    /// growing by `tan` a unit of height along z, `cos` the cosine of its half-angle (a moved
    /// cone's radius at a height differs by the distance over that).
    Cone {
        o: P3,
        z: V3,
        r: f64,
        tan: f64,
        cos: f64,
        inside: bool,
    },
}

impl Support {
    /// How far a point is outside the region (negative inside).
    fn outside(&self, p: &P3) -> f64 {
        match self {
            Support::Plane(o, n) => (p - o).dot(n),
            Support::Cylinder(o, z, r, inside) => {
                let w = p - o;
                let rho = (w - z * w.dot(z)).norm();
                if *inside { rho - r } else { r - rho }
            }
            Support::Sphere(f, r, inside) => {
                let rho = (p - f.o).norm();
                if *inside { rho - r } else { r - rho }
            }
            // (Past the apex the cone's radius is less than nothing: no point there is inside
            // it, every one outside.)
            Support::Cone {
                o,
                z,
                r,
                tan,
                cos,
                inside,
            } => {
                let w = p - o;
                let h = w.dot(z);
                let rho = (w - z * h).norm();
                let off = (rho - (r + h * tan)) * cos;
                if *inside { off } else { -off }
            }
        }
    }
}

/// Points all over the faces of `b`, no chord between neighbours sagging more than `sag` off
/// its face: each face's loops' points (as its polygons have them) and a grid in its (u, v)
/// inside them, as fine as its surface's bend asks. None where a face is neither a plane, a
/// cylinder, a cone, a sphere nor a torus (a mesh has such a face's points). No triangles are
/// made: a body's mesh to that sag took most of a small shell's time.
fn face_samples(b: &Body, sag: f64) -> Option<Vec<P3>> {
    use crate::native::classify;
    let mut pts: Vec<P3> = b.verts.iter().map(|v| v.p).collect();
    // The step of an angle whose chord sags `sag` on a circle of radius r.
    let turn = |r: f64| (8.0 * sag / r.abs().max(sag)).sqrt().clamp(0.02, 0.5);
    for f in &b.faces {
        // (u, v) steps: an angle's on the round directions, a few across a straight one.
        let (du, dv) = match &f.surf {
            Surface::Plane { .. } => (None, None),
            Surface::Cylinder { r, .. } => (Some(turn(*r)), None),
            Surface::Cone { r, .. } => (Some(turn(r.abs().max(sag))), None),
            Surface::Sphere { r, .. } => (Some(turn(*r)), Some(turn(*r))),
            Surface::Torus { rmaj, rmin, .. } => (Some(turn(rmaj + rmin)), Some(turn(*rmin))),
            _ => return None,
        };
        let kept = classify::face_polygons_kept(b, f);
        let (polys, boxes) = (&kept.0, &kept.1);
        let (mut lo, mut hi) = (
            P2::new(f64::INFINITY, f64::INFINITY),
            P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for q in polys.iter().flatten() {
            lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
            hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
            pts.push(f.surf.eval(q.x, q.y));
        }
        if !(lo.x < hi.x && lo.y < hi.y) {
            continue;
        }
        let steps = |span: f64, step: Option<f64>| match step {
            Some(s) => ((span / s).ceil() as usize).clamp(4, 512),
            None => 8,
        };
        let (nu, nv) = (steps(hi.x - lo.x, du), steps(hi.y - lo.y, dv));
        for i in 1..nu {
            for j in 1..nv {
                let uv = P2::new(
                    lo.x + (hi.x - lo.x) * i as f64 / nu as f64,
                    lo.y + (hi.y - lo.y) * j as f64 / nv as f64,
                );
                if classify::in_face_boxed(polys, boxes, &f.surf, uv, 0.0)
                    != classify::InFace::Outside
                {
                    pts.push(f.surf.eval(uv.x, uv.y));
                }
            }
        }
    }
    Some(pts)
}

/// The cavity of an inward shell `t` thick open at the `remove` faces, when the surface of every
/// wall (each face kept) bounds the whole body (the body on one side of the whole plane,
/// cylinder, sphere or cone): the walls moved in by t are then those surfaces moved in, extended or
/// trimmed where they meet and run on to the openings, and the cavity is the body's points inside
/// all of them. None when some wall's surface doesn't bound the body this way (an L-shaped
/// block's inner walls cut across its other leg); Ok(None) when the cavity is empty, every point
/// within t of a wall (walls moved in until they meet).
fn cavity_of_supports(b: &Body, remove: &[usize], t: f64) -> Option<Result<Option<Body>>> {
    let bb = b.bbox();
    let size = bb.diagonal();
    let close = tol::POINT.max(tol::REL * size);
    // A body that is itself where half-spaces meet (a box, a wedge): every face's plane bounds
    // it, by its corners, so the cavity is its polygons cut by each wall's plane moved in,
    // exactly and with no boolean, and nothing of it is sampled to find that out.
    if let Some(mut cell) = crate::native::polytope::Polytope::of_body(b, close)
        && remove.len() < b.faces.len()
    {
        let (planes, whole) = (cell.planes(), cell.volume());
        let within = (0..b.faces.len())
            .filter(|f| !remove.contains(f))
            .all(|f| cell.clip(planes[f].0 - planes[f].1 * t, planes[f].1, close));
        if !within || cell.volume() <= tol::REL * whole {
            return Some(Ok(None));
        }
        if let Some(cavity) = cell.body(close) {
            return Some(Ok(Some(cavity)));
        }
    }
    // Points of the body's faces, a chord's sag of a thousandth of its size apart at most:
    // sampled straight off its faces where they are planes, cylinders, cones, spheres and
    // tori (their loops' points and a grid inside them), else its mesh's.
    let pts: Vec<P3> = match face_samples(b, 1e-3 * size) {
        Some(pts) => pts,
        None => {
            let (meshes, _) = crate::native::tess::mesh_body(b, 1e-3 * size, 0.2);
            meshes
                .iter()
                .flat_map(|m| m.positions.iter().copied())
                .chain(b.verts.iter().map(|v| v.p))
                .collect()
        }
    };
    // A concave edge between two walls rounds a rounded shell's cavity: not this one's.
    let ef = b.edge_faces();
    if (0..b.edges.len()).any(|e| {
        !b.edges[e].degenerate
            && convex(b, e) == Some(false)
            && ef[e].iter().all(|f| !remove.contains(f))
    }) {
        return None;
    }
    let mut walls = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        if remove.contains(&fi) {
            continue;
        }
        // The face's outward normal at a point of it.
        let (_, at) = crate::native::mass::face_area(b, fi);
        let (u, v) = f.surf.project(&at, None);
        let (q, n) = (
            f.surf.eval(u, v),
            f.surf.normal(u, v).normalize() * if f.sense { 1.0 } else { -1.0 },
        );
        let w = match &f.surf {
            Surface::Plane { .. } => Support::Plane(q, n),
            Surface::Cylinder { f: fr, r } => {
                let z = fr.z.normalize();
                let radial = (q - fr.o) - z * (q - fr.o).dot(&z);
                Support::Cylinder(fr.o, z, *r, radial.dot(&n) > 0.0)
            }
            Surface::Sphere { f: fr, r } => Support::Sphere(*fr, *r, (q - fr.o).dot(&n) > 0.0),
            Surface::Cone { f: fr, r, a } if a.tan().abs() > tol::ANGULAR && a.cos() > 0.0 => {
                let z = fr.z.normalize();
                let radial = (q - fr.o) - z * (q - fr.o).dot(&z);
                Support::Cone {
                    o: fr.o,
                    z,
                    r: *r,
                    tan: a.tan(),
                    cos: a.cos(),
                    inside: radial.dot(&n) > 0.0,
                }
            }
            _ => return None,
        };
        if pts.iter().any(|p| w.outside(p) > close) {
            return None;
        }
        walls.push(w);
    }
    if walls.is_empty() {
        return None;
    }
    let reach = size * 2.0 + t * 2.0 + 10.0;
    let mut grown = bb;
    grown.add(&(bb.min - V3::repeat(reach)));
    grown.add(&(bb.max + V3::repeat(reach)));
    let big = |o: P3, z: V3, r: f64| -> Result<Body> {
        let fr = Frame::from_z(o - z * reach, z);
        let plane = Plane {
            origin: fr.o,
            x_dir: nalgebra::Unit::new_normalize(fr.x),
            normal: nalgebra::Unit::new_normalize(z),
        };
        let disk = Region {
            outer: Loop {
                curves: vec![Curve2::Circle {
                    center: nalgebra::Point2::new(0.0, 0.0),
                    radius: r,
                    source: None,
                }],
            },
            holes: vec![],
            source: None,
        };
        let regions = crate::native::profile::regions(&Profile::new(plane, vec![disk]))?;
        let mf = crate::native::profile::merge_regions(&regions)?;
        let kind = crate::native::sweep::Prism { plane, dir: z };
        Ok(crate::native::sweep::sweep(&plane, &mf, &kind, 0.0, 2.0 * reach, false)?.body)
    };
    // The solid inside the cone about (o, z) of radius `r` at o, growing by `tan` a unit of
    // height: its part over the body's heights (and a margin), from its apex on where that
    // lies among them. None where it has none there.
    let cone = |o: P3, z: V3, r: f64, tan: f64| -> Result<Option<Body>> {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &pts {
            let h = (p - o).dot(&z);
            (lo, hi) = (lo.min(h), hi.max(h));
        }
        let (mut lo, mut hi) = (lo - reach, hi + reach);
        let apex = -r / tan;
        if tan > 0.0 {
            lo = lo.max(apex);
        } else {
            hi = hi.min(apex);
        }
        if hi - lo <= close {
            return Ok(None);
        }
        let at = |h: f64| (r + h * tan).max(0.0);
        let mut k = NativeKernel::new();
        let made = k.primitive(&Primitive::Cone {
            base: o + z * lo,
            axis: nalgebra::Unit::new_normalize(z),
            base_radius: at(lo),
            top_radius: at(hi),
            height: hi - lo,
        })?;
        Ok(Some((**k.get(made.first()?)?).clone()))
    };
    // Flat walls only: the cavity is the body within the one solid behind every wall's plane
    // moved in, where each plane in turn cost a boolean of its own.
    if walls.iter().all(|w| matches!(w, Support::Plane(..))) {
        let mut cell = crate::native::polytope::Polytope::of_box(&grown);
        let within = walls.iter().all(|w| match *w {
            Support::Plane(o, n) => cell.clip(o - n * t, n, close),
            _ => unreachable!(),
        });
        if !within {
            return Some(Ok(None));
        }
        // (A boolean with the one solid that fails leaves the walls to be taken one at a time,
        // below, as before.)
        if let Some(cell) = cell.body(close) {
            match boolean_bodies(b, &cell, BoolOp::Intersect) {
                Ok(r) if !r.body.faces.is_empty() => {
                    let v = crate::native::mass::body_volume(&r.body);
                    return Some(Ok(
                        (v > tol::REL * crate::native::mass::body_volume(b)).then_some(r.body)
                    ));
                }
                Ok(_) | Err(KernelError::EmptyResult(_)) => return Some(Ok(None)),
                Err(_) => {}
            }
        }
    }
    Some((|| {
        let mut cavity = b.clone();
        // The walls moved in: the regions the cavity is within, then those it is outside.
        let mut keep: Vec<(Body, BoolOp)> = Vec::new();
        for w in &walls {
            keep.push(match *w {
                // (A cone moved by t square to itself: at each height its radius t over the
                // cosine of its half-angle less, or more.)
                Support::Cone {
                    o,
                    z,
                    r,
                    tan,
                    cos,
                    inside: true,
                } => match cone(o, z, r - t / cos, tan)? {
                    Some(c) => (c, BoolOp::Intersect),
                    None => return Ok(None),
                },
                Support::Cone {
                    o,
                    z,
                    r,
                    tan,
                    cos,
                    inside: false,
                } => match cone(o, z, r + t / cos, tan)? {
                    Some(c) => (c, BoolOp::Subtract),
                    None => continue,
                },
                Support::Plane(o, n) => (
                    crate::native::ops::half_space_box(o - n * t, -n, &grown)?,
                    BoolOp::Intersect,
                ),
                Support::Cylinder(o, z, r, true) => {
                    if r - t <= close {
                        return Ok(None);
                    }
                    (big(o, z, r - t)?, BoolOp::Intersect)
                }
                Support::Cylinder(o, z, r, false) => (big(o, z, r + t)?, BoolOp::Subtract),
                // (About the face's own axis: a circle square to it, where an opening's plane
                // meets the ball, is then one of its parallels, with an exact p-curve.)
                Support::Sphere(f, r, true) => {
                    if r - t <= close {
                        return Ok(None);
                    }
                    (ball_frame(f.o, r - t, f.z, f.x)?, BoolOp::Intersect)
                }
                Support::Sphere(f, r, false) => {
                    (ball_frame(f.o, r + t, f.z, f.x)?, BoolOp::Subtract)
                }
            });
        }
        keep.sort_by_key(|(_, op)| *op == BoolOp::Subtract);
        for (tool, op) in &keep {
            cavity = match boolean_bodies(&cavity, tool, *op) {
                Ok(r) if !r.body.faces.is_empty() => r.body,
                Ok(_) | Err(KernelError::EmptyResult(_)) => return Ok(None),
                Err(e) => return Err(e),
            };
        }
        let v = crate::native::mass::body_volume(&cavity);
        if v <= tol::REL * crate::native::mass::body_volume(b) {
            return Ok(None);
        }
        Ok(Some(cavity))
    })())
}

/// The body shrunk by a ball of radius `d` (every face moved in by d, the concave edges and
/// corners rounded: the points farther than d inside the body), the faces at distance 0 left
/// where they are (SMLib's inset with its convex edges, the body's concave ones, rounded:
/// `SmOffsetExecutive` with `SetExtendConvexEdges(FALSE)`). A body with no concave edge
/// shrinks to its sharp inset.
pub fn inset_rounded(b: &Body, dist: &[f64]) -> Result<Body> {
    let t = -dist.iter().copied().fold(0.0, f64::min);
    if dist
        .iter()
        .any(|x| *x != 0.0 && (x + t).abs() > tol::ROUNDOFF_FINE)
        || !(t > 0.0)
    {
        return Err(KernelError::Unsupported(
            "rounded insets with different distances per face",
        ));
    }
    let concave = (0..b.edges.len()).any(|e| !b.edges[e].degenerate && convex(b, e) == Some(false));
    if !concave && let Ok(c) = super::sharp::offset_sharp(b, dist) {
        return Ok(c);
    }
    // A prism with a curved side: its outline's offset (rounded about reflex corners) extruded.
    if let Some(Ok(c)) = super::prism::curved_prism_inset(b, dist, true, false) {
        return Ok(c);
    }
    let still: Vec<usize> = (0..b.faces.len()).filter(|&f| dist[f] == 0.0).collect();
    inner_core(b, &still, t, false)
}

/// A tube round each concave edge and a ball at each of their vertices (not round the `still`
/// faces' edges and vertices unless `round_still`).
fn rounds(b: &Body, still: &[usize], t: f64, round_still: bool) -> Result<Vec<(Body, String)>> {
    let ef = b.edge_faces();
    let still = crate::native::topo::Mask::of(b.faces.len(), still);
    let at_still = |e: usize| !round_still && ef[e].iter().any(|f| still.contains(f));
    let still_verts: Vec<usize> = (0..b.edges.len())
        .filter(|&e| at_still(e))
        .flat_map(|e| b.edges[e].v)
        .collect();
    let still_verts = crate::native::topo::Mask::of(b.verts.len(), &still_verts);
    let kinds: Vec<Dihedral> = (0..b.edges.len()).map(|e| dihedral(b, e)).collect();
    let mut pieces: Vec<(Body, String)> = Vec::new();
    let mut balls: Vec<P3> = Vec::new();
    for e in 0..b.edges.len() {
        if b.edges[e].degenerate || kinds[e] != Dihedral::Concave || at_still(e) {
            continue;
        }
        pieces.push((edge_tube(b, e, t)?, format!("the tube round edge {e}")));
        // A closed edge (a whole circle) has no corner: its seam vertex needs no ball.
        if b.edges[e].v[0] == b.edges[e].v[1] {
            continue;
        }
        for v in b.edges[e].v {
            let p = b.verts[v].p;
            if !still_verts.contains(&v)
                && needs_ball(b, &kinds, v, Dihedral::Concave)
                && !balls.iter().any(|q| (q - p).norm() < tol::ROUNDOFF)
            {
                balls.push(p);
            }
        }
    }
    for p in balls {
        pieces.push((ball_body(p, t)?, format!("the ball at {p:?}")));
    }
    Ok(pieces)
}

/// The cavity built from its definition: the body less each kept face swept inward by t, a
/// tube round each concave edge between kept faces and a ball at each vertex of a concave edge.
pub fn cavity_by_regions(b: &Body, remove: &[usize], t: f64) -> Result<Body> {
    inner_core(b, remove, t, true)
}

/// The body less each face but `still` swept inward by t, a tube round each concave edge and a
/// ball at each vertex of one; with `round_still` false, not round the edges and vertices of
/// the `still` faces (which stay where they are).
fn inner_core(b: &Body, remove: &[usize], t: f64, round_still: bool) -> Result<Body> {
    let mut pieces: Vec<(Body, String)> = Vec::new();
    for fi in 0..b.faces.len() {
        if remove.contains(&fi) {
            continue;
        }
        let slab = match &b.faces[fi].surf {
            Surface::Plane { .. } => face_slab(b, fi, -t)?,
            _ => face_band(b, fi, t)?,
        };
        pieces.push((
            slab,
            format!(
                "the inward slab of face {fi} ({:?})",
                b.faces[fi].surf.kind()
            ),
        ));
    }
    pieces.extend(rounds(b, remove, t, round_still)?);
    combine_all(b.clone(), pieces, BoolOp::Subtract)
}

/// The region within t inside a curved face (a cylinder's band). A convex round (a cylinder
/// or sphere bulging out) at least t thick inside collapses onto its axis or centre: the cavity
/// is sharp there, its walls the planes the round is tangent to moved in by t, so the round's
/// part is the block between those planes and their offsets ([`collapsed_round`]).
fn face_band(b: &Body, fi: usize, t: f64) -> Result<Body> {
    let f = &b.faces[fi];
    let convex = f
        .surf
        .frame()
        .is_some_and(|fr| f.sense == fr.right_handed());
    match &f.surf {
        Surface::Cylinder { r, .. } | Surface::Sphere { r, .. }
            if convex && t >= r * (1.0 - tol::REL) =>
        {
            return collapsed_round(b, fi, t)?.ok_or(KernelError::Unsupported(
                "a shell thicker than a round's radius",
            ));
        }
        _ => {}
    }
    if let Surface::Cylinder { f: fr, r } = &f.surf {
        // The ring between r and r ∓ t behind the face.
        let outward = f.sense == fr.right_handed();
        let (r0, r1) = if outward { (r - t, *r) } else { (*r, r + t) };
        if r0 <= 0.0 {
            return Err(KernelError::Unsupported(
                "a shell thicker than a cylinder's radius",
            ));
        }
        if let Some(band) = cylinder_band(b, fi, r0, r1)? {
            return Ok(band);
        }
    }
    super::slab::face_slab(b, fi, -t)
}

/// A convex round's part of a shell's walls when the shell is at least as thick as the round's
/// radius: the block between the planes the round is tangent to along its edges and those
/// planes moved in by t (a box edge's round: the t × t square at the edge, along it; a box
/// corner's ball: the t cube at the corner). Holds the round and what lies between it and its
/// neighbours' moved planes. Only for a cylinder over a rectangle of angle (under a half turn)
/// and height, and a sphere's octant (bounded by three great circles square to each other);
/// None for another face.
fn collapsed_round(b: &Body, fi: usize, t: f64) -> Result<Option<Body>> {
    let f = &b.faces[fi];
    // Planes' normals e_i (at distance r from the centre point o): the block's corners are
    // o + Σ a_i·(dual of e_i) with a_i ∈ {r − t, r}.
    let corner = |e: &[V3], a: &[f64]| -> Option<V3> {
        match e {
            [e0, e1] => {
                // α e0 + β e1 with α + β g = a0, α g + β = a1.
                let g = e0.dot(e1);
                let det = 1.0 - g * g;
                (det > tol::ROUNDOFF)
                    .then(|| e0 * ((a[0] - g * a[1]) / det) + e1 * ((a[1] - g * a[0]) / det))
            }
            [e0, e1, e2] => Some(e0 * a[0] + e1 * a[1] + e2 * a[2]),
            _ => None,
        }
    };
    let cycle = |lo: f64, hi: f64| [[lo, lo], [hi, lo], [hi, hi], [lo, hi]];
    match &f.surf {
        Surface::Cylinder { f: fr, r } => {
            let Some((lo, hi, u0, u1)) = cylinder_domain(b, fi) else {
                return Ok(None);
            };
            if u1 - u0 >= PI - tol::ANGULAR {
                return Ok(None);
            }
            let radial = |u: f64| fr.x * u.cos() + fr.y * u.sin();
            let e = [radial(u0), radial(u1)];
            let mut c = [P3::origin(); 8];
            for (k, a) in cycle(r - t, *r).iter().enumerate() {
                let Some(q) = corner(&e, a) else {
                    return Ok(None);
                };
                c[k] = fr.o + fr.z * lo + q;
                c[k + 4] = fr.o + fr.z * hi + q;
            }
            Ok(Some(crate::native::blend::hexahedron(&c)?))
        }
        Surface::Sphere { f: fr, r } => {
            // The planes of its edges, which must hold the centre.
            let o = fr.o;
            let mut normals: Vec<V3> = Vec::new();
            let mut mid = V3::zeros();
            for co in f.loops.iter().flatten() {
                let e = &b.edges[co.edge];
                if e.degenerate {
                    continue;
                }
                let Curve::Circle { c: cc, x, y, r: rc } = &e.curve else {
                    return Ok(None);
                };
                if (cc - o).norm() > 1e-7 * (1.0 + r) || (rc - r).abs() > 1e-7 * (1.0 + r) {
                    return Ok(None);
                }
                let n = x.cross(y).normalize();
                if !normals.iter().any(|m| m.cross(&n).norm() < tol::ANGULAR) {
                    normals.push(n);
                }
                mid += (e.point(e.mid_t()) - o).normalize();
            }
            let [n1, n2, n3] = normals[..] else {
                return Ok(None);
            };
            if n1.dot(&n2).abs() > tol::ANGULAR
                || n2.dot(&n3).abs() > tol::ANGULAR
                || n3.dot(&n1).abs() > tol::ANGULAR
            {
                return Ok(None);
            }
            // The octant's axes: each plane's normal towards the face.
            let side = |n: V3| if n.dot(&mid) >= 0.0 { n } else { -n };
            let e = [side(n1), side(n2), side(n3)];
            let mut c = [P3::origin(); 8];
            for (k, a) in cycle(r - t, *r).iter().enumerate() {
                for (z, off) in [(r - t, 0), (*r, 4)] {
                    let Some(q) = corner(&e, &[a[0], a[1], z]) else {
                        return Ok(None);
                    };
                    c[k + off] = o + q;
                }
            }
            Ok(Some(crate::native::blend::hexahedron(&c)?))
        }
        _ => Ok(None),
    }
}

/// The points of a concave cylindrical face's slab nearer the face than its axis: the wedge
/// between the face and the axis. A concave face's normals meet at its axis, so no point past
/// the axis is nearer the face than the rest of the body: when the offset reaches the axis
/// this is the face's part of the grown body (its offset surface would have no radius).
fn cylinder_wedge(b: &Body, fi: usize) -> Result<Option<Body>> {
    let Surface::Cylinder { r, .. } = &b.faces[fi].surf else {
        return Ok(None);
    };
    cylinder_band(b, fi, 0.0, *r)
}

/// The solid between radii r0 and r1 (r0 = 0: out from the axis) over a cylindrical face's
/// angles and heights: an annular sector (a ring for a whole turn; a pie slice or a disc from
/// the axis) swept along the axis. None unless the face spans whole axial lines between two
/// circles square to the axis (its domain a rectangle in angle and height).
fn cylinder_band(b: &Body, fi: usize, r0: f64, r1: f64) -> Result<Option<Body>> {
    let Surface::Cylinder { f: fr, .. } = &b.faces[fi].surf else {
        return Ok(None);
    };
    let Some((lo, hi, u0, u1)) = cylinder_domain(b, fi) else {
        return Ok(None);
    };
    let plane = Plane {
        origin: fr.o + fr.z * lo,
        x_dir: nalgebra::Unit::new_normalize(fr.x),
        normal: nalgebra::Unit::new_normalize(fr.z),
    };
    let circ = |rr: f64| Loop {
        curves: vec![Curve2::Circle {
            center: nalgebra::Point2::origin(),
            radius: rr,
            source: None,
        }],
    };
    // The angles in the plane (whose y is z × x: the frame's y for a right-handed frame, else
    // the other way).
    let whole = !(u1 - u0 < TAU - tol::ANGULAR);
    let (u0, u1) = if fr.right_handed() {
        (u0, u1)
    } else {
        (-u1, -u0)
    };
    let o = nalgebra::Point2::origin();
    let at = |rr: f64, u: f64| nalgebra::Point2::new(rr * u.cos(), rr * u.sin());
    let region = match (whole, r0 > 0.0) {
        (true, true) => Region {
            outer: circ(r1),
            holes: vec![circ(r0)],
            source: None,
        },
        (true, false) => Region {
            outer: circ(r1),
            holes: vec![],
            source: None,
        },
        (false, true) => Region {
            outer: Loop {
                curves: vec![
                    Curve2::Arc {
                        center: o,
                        radius: r1,
                        start_angle: u0,
                        sweep: u1 - u0,
                        source: None,
                    },
                    Curve2::Line {
                        a: at(r1, u1),
                        b: at(r0, u1),
                        source: None,
                    },
                    Curve2::Arc {
                        center: o,
                        radius: r0,
                        start_angle: u1,
                        sweep: u0 - u1,
                        source: None,
                    },
                    Curve2::Line {
                        a: at(r0, u0),
                        b: at(r1, u0),
                        source: None,
                    },
                ],
            },
            holes: vec![],
            source: None,
        },
        (false, false) => Region {
            outer: Loop {
                curves: vec![
                    Curve2::Arc {
                        center: o,
                        radius: r1,
                        start_angle: u0,
                        sweep: u1 - u0,
                        source: None,
                    },
                    Curve2::Line {
                        a: at(r1, u1),
                        b: o,
                        source: None,
                    },
                    Curve2::Line {
                        a: o,
                        b: at(r1, u0),
                        source: None,
                    },
                ],
            },
            holes: vec![],
            source: None,
        },
    };
    let prof = Profile::new(plane, vec![region]);
    let regions = crate::native::profile::regions(&prof)?;
    let mf = crate::native::profile::merge_regions(&regions)?;
    let kind = crate::native::sweep::Prism { plane, dir: fr.z };
    Ok(Some(
        crate::native::sweep::sweep_full(&mf, &[], &kind, 0.0, hi - lo, false, true)?.body,
    ))
}

/// A cylindrical face's heights along its axis and angles (its p-curves' u), when it spans
/// whole axial lines between two circles square to the axis (its domain a rectangle in angle
/// and height): (lo, hi, u0, u1).
fn cylinder_domain(b: &Body, fi: usize) -> Option<(f64, f64, f64, f64)> {
    let f = &b.faces[fi];
    let Surface::Cylinder { f: fr, r } = &f.surf else {
        return None;
    };
    let size = 1.0 + r + fr.o.coords.norm();
    let rect = f.loops.iter().flatten().all(|c| {
        let e = &b.edges[c.edge];
        e.degenerate
            || match &e.curve {
                Curve::Line { d, .. } => d.normalize().cross(&fr.z).norm() < tol::ANGULAR,
                Curve::Circle { c: cc, x, y, r: rc } => {
                    let w = cc - fr.o;
                    x.cross(y).normalize().cross(&fr.z).norm() < tol::ANGULAR
                        && (w - fr.z * w.dot(&fr.z)).norm() < 1e-7 * size
                        && (rc - r).abs() < 1e-7 * size
                }
                _ => false,
            }
    });
    if !rect {
        return None;
    }
    let sub = crate::native::kernel::subset(b, &[fi], false);
    let (lo, hi) = crate::native::kernel::extent_along(&sub, &fr.o, &fr.z);
    let (mut u0, mut u1) = (f64::INFINITY, f64::NEG_INFINITY);
    for c in f.loops.iter().flatten() {
        let e = &b.edges[c.edge];
        for i in 0..=8 {
            let q = c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0);
            u0 = u0.min(q.x);
            u1 = u1.max(q.x);
        }
    }
    Some((lo, hi, u0, u1))
}

/// [`grown_closed_holes`]'s hole `h` that a distance `d` short of its radius narrows, cut into
/// `g` (the block grown whole): each moved cap a circle of the hole's radius, from it a quarter
/// of the ring torus about the rim (the ball rolled round it) down to the hole's wall moved in
/// by d, which runs between the mouths. Returns the volume taken from `g`:
/// π(r − d)²H through the block and, over each cap, the hole's radius's disc d high less the
/// quarter of the tube (Pappus: πd²/4 round the circle of radius r − 4d/3π).
fn narrowed_hole(g: &mut Body, keep: &[usize], h: &Hole, d: f64) -> Option<f64> {
    use crate::native::topo::{Coedge, Edge, Face, TOL_MIN, Vertex};
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let r = h.r;
    let [(cap0, _, o0, n0), (cap1, _, o1, n1)] = h.ends[..] else {
        return None;
    };
    // The ends: the one the axis leaves as "top".
    let axis = n0;
    if n1.dot(&axis) > -1.0 + tol::ANGULAR {
        return None;
    }
    let len = (o0 - o1).dot(&axis);
    if !(len > 0.0) {
        return None;
    }
    let x_ref = {
        let a = if axis.x.abs() < 0.9 { V3::x() } else { V3::y() };
        (a - axis * a.dot(&axis)).normalize()
    };
    let line = |ox: f64, oy: f64, dx: f64, dy: f64| Curve::Line {
        o: P3::new(ox, oy, 0.0),
        d: V3::new(dx, dy, 0.0),
    };
    let push_v = |g: &mut Body, p: P3| {
        g.verts.push(Vertex { p, tol: TOL_MIN });
        g.verts.len() - 1
    };
    let push_e = |g: &mut Body, curve: Curve, t: [f64; 2], v: [usize; 2]| {
        g.edges.push(Edge {
            curve,
            t,
            v,
            degenerate: false,
            tol: TOL_MIN,
        });
        g.edges.len() - 1
    };
    let inner_r = r - d;
    // Each mouth: the circle on the moved cap, the inner circle, the ring's quarter.
    let mut inner: Vec<(usize, usize)> = Vec::new();
    for (cap, o, n) in [(cap0, o0, n0), (cap1, o1, n1)] {
        let gcap = keep.iter().position(|&k| k == cap)?;
        let tf = Frame::from_xz(o, x_ref, n);
        let (x, y) = (tf.x, tf.y);
        let surf = Surface::Torus {
            f: tf,
            rmaj: r,
            rmin: d,
        };
        let cv = push_v(g, o + x * r + n * d);
        let iv = push_v(g, o + x * inner_r);
        let capc = Curve::Circle {
            c: o + n * d,
            x,
            y,
            r,
        };
        let ec = push_e(g, capc.clone(), [0.0, TAU], [cv, cv]);
        let ei = push_e(
            g,
            Curve::Circle {
                c: o,
                x,
                y,
                r: inner_r,
            },
            [0.0, TAU],
            [iv, iv],
        );
        let em = push_e(
            g,
            Curve::Circle {
                c: o + x * r,
                x,
                y: n,
                r: d,
            },
            [FRAC_PI_2, PI],
            [cv, iv],
        );
        let mut lp = vec![
            Coedge {
                edge: ec,
                fwd: true,
                pc: line(0.0, FRAC_PI_2, 1.0, 0.0),
            },
            Coedge {
                edge: em,
                fwd: true,
                pc: line(TAU, 0.0, 0.0, 1.0),
            },
            Coedge {
                edge: ei,
                fwd: false,
                pc: line(0.0, PI, 1.0, 0.0),
            },
            Coedge {
                edge: em,
                fwd: false,
                pc: line(0.0, 0.0, 0.0, 1.0),
            },
        ];
        // Out of the tube (the material is within d of the rim).
        let (um, vm) = (PI, 0.75 * PI);
        let pm = surf.eval(um, vm);
        let core = o + (x * um.cos() + y * um.sin()) * r;
        let sense = surf.normal(um, vm).dot(&(pm - core)) > 0.0;
        if !sense {
            lp.reverse();
            for c in lp.iter_mut() {
                c.fwd = !c.fwd;
            }
        }
        let capc_fwd = lp.iter().find(|c| c.edge == ec)?.fwd;
        let csurf = g.faces[gcap].surf.clone();
        let pc = crate::native::pcurve::pcurve(&csurf, &capc, 0.0, TAU, None);
        g.faces[gcap].loops.push(vec![Coedge {
            edge: ec,
            fwd: !capc_fwd,
            pc,
        }]);
        g.faces.push(Face {
            surf,
            sense,
            loops: vec![lp],
        });
        let fi = g.faces.len() - 1;
        g.shells[0].faces.push(fi);
        inner.push((ei, iv));
    }
    // The wall: a cylinder of radius r − d from the second mouth to the first, its seam
    // along x_ref.
    let (eit, ivt) = inner[0];
    let (eib, ivb) = inner[1];
    let cf = Frame::from_xz(o1, x_ref, axis);
    let wall = Surface::Cylinder { f: cf, r: inner_r };
    let seam = push_e(
        g,
        Curve::Line {
            o: o1 + x_ref * inner_r,
            d: axis,
        },
        [0.0, len],
        [ivb, ivt],
    );
    // Each inner circle's parameter runs with the wall's u where its y is the wall's.
    let along = |yc: V3| yc.dot(&cf.y) > 0.0;
    // Counter-clockwise in (u, v): along the bottom circle, up the seam at u = 2π, back along
    // the top circle, down the seam at u = 0; each circle's coedge by which way its t runs.
    let (bot_with, top_with) = (along(g_circle_y(g, eib)?), along(g_circle_y(g, eit)?));
    let mut lp = vec![
        Coedge {
            edge: eib,
            fwd: bot_with,
            pc: if bot_with {
                line(0.0, 0.0, 1.0, 0.0)
            } else {
                line(TAU, 0.0, -1.0, 0.0)
            },
        },
        Coedge {
            edge: seam,
            fwd: true,
            pc: line(TAU, 0.0, 0.0, 1.0),
        },
        Coedge {
            edge: eit,
            fwd: !top_with,
            pc: if top_with {
                line(0.0, len, 1.0, 0.0)
            } else {
                line(TAU, len, -1.0, 0.0)
            },
        },
        Coedge {
            edge: seam,
            fwd: false,
            pc: line(0.0, 0.0, 0.0, 1.0),
        },
    ];
    // The wall's outward normal toward the axis (the hole is empty).
    let sense = false;
    lp.reverse();
    for c in lp.iter_mut() {
        c.fwd = !c.fwd;
    }
    // Each inner circle run against the ring's use of it.
    for (ei, _) in &inner {
        let ring = g
            .faces
            .iter()
            .rev()
            .take(2)
            .flat_map(|f| f.loops.iter().flatten())
            .find(|c| c.edge == *ei)?
            .fwd;
        if lp.iter().find(|c| c.edge == *ei)?.fwd == ring {
            return None;
        }
    }
    g.faces.push(Face {
        surf: wall,
        sense,
        loops: vec![lp],
    });
    let fi = g.faces.len() - 1;
    g.shells[0].faces.push(fi);
    let quarter = PI * d * d / 4.0 * TAU * (r - 4.0 * d / (3.0 * PI));
    Some(PI * inner_r * inner_r * len + 2.0 * (PI * r * r * d - quarter))
}

/// The y axis of circle edge `e` of `g`.
fn g_circle_y(g: &Body, e: usize) -> Option<V3> {
    match g.edges[e].curve {
        Curve::Circle { y, .. } => Some(y),
        _ => None,
    }
}

/// A hole of [`grown_closed_holes`]: its wall, and at each end the cap it opens in, that cap's
/// loop round it, the rim's centre and the cap's outward normal.
struct Hole {
    wall: usize,
    r: f64,
    ends: Vec<(usize, usize, P3, V3)>,
}

/// A convex polyhedron drilled straight through by round holes, each between two planar caps
/// square to its axis, grown by a ball of radius `d` at least each hole's radius: the hole
/// fills, and where it opened in a cap the grown body has a dimple, the points within `d` of
/// the cap's points but farther than `d` from the rim: under the moved cap a spindle torus
/// about the axis (its tube the ball rolled round the rim, radius `d` about the rim circle),
/// meeting the cap tangentially in a circle of the hole's radius and closing in a point on the
/// axis. So the block filled ([`grown_convex`]) with that circle cut in each moved cap and the
/// torus closing it, checked, its volume held to the block's grown less the dimples'
/// `πdr² − 2π(r·I₀ − I₁)` (I₀ = ∫₀ʳ√(d² − s²) ds, I₁ = ∫₀ʳ s√(d² − s²) ds). None for any other
/// body.
fn grown_closed_holes(b: &Body, d: f64) -> Option<Body> {
    use crate::native::topo::{CheckLevel, Coedge, Edge, Face, TOL_MIN, Vertex};
    use std::f64::consts::{FRAC_PI_2, TAU};
    if b.solids.len() != 1 || b.shells.len() != 1 {
        return None;
    }
    let has = |fi: usize, e: usize| b.faces[fi].loops.iter().flatten().any(|c| c.edge == e);
    let mut holes: Vec<Hole> = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        let Surface::Cylinder { f: cf, r } = &f.surf else {
            if !matches!(f.surf, Surface::Plane { .. }) {
                return None;
            }
            continue;
        };
        // A hole: the wall's outward normal toward the axis; the distance closes it.
        let p = cf.o + cf.x * *r;
        let (u, v) = f.surf.project(&p, None);
        let out = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
        if out.dot(&cf.x) >= 0.0 || (d - r).abs() <= r * 1e-9 || f.loops.len() != 1 {
            return None;
        }
        let mut ends = Vec::new();
        for c in &f.loops[0] {
            let e = &b.edges[c.edge];
            let Curve::Circle { c: o, r: cr, .. } = e.curve else {
                continue;
            };
            if !e.closed() || (cr - r).abs() > tol::REL * (1.0 + r) {
                return None;
            }
            let cap = (0..b.faces.len()).find(|&g| g != fi && has(g, c.edge))?;
            let Surface::Plane { f: pf } = &b.faces[cap].surf else {
                return None;
            };
            let n = pf.x.cross(&pf.y).normalize() * if b.faces[cap].sense { 1.0 } else { -1.0 };
            let li = b.faces[cap]
                .loops
                .iter()
                .position(|l| l.len() == 1 && l[0].edge == c.edge)?;
            if li == 0 || n.cross(&cf.z).norm() > tol::ANGULAR {
                return None;
            }
            ends.push((cap, li, o, n));
        }
        if ends.len() != 2 {
            return None;
        }
        holes.push(Hole {
            wall: fi,
            r: *r,
            ends,
        });
    }
    if holes.is_empty() {
        return None;
    }
    // Each hole inside its caps' outlines and clear of the others. (A point over a hole's
    // mouth is nearest that hole's rim among the cap's points (the triangle inequality, the
    // holes apart), and a convex block has nothing over its cap: so each dimple is its rim's
    // alone, whatever lies near.)
    for (i, h) in holes.iter().enumerate() {
        for &(cap, _, o, _) in &h.ends {
            for c in &b.faces[cap].loops[0] {
                let e = &b.edges[c.edge];
                let Curve::Line { o: lo, d: ld } = e.curve else {
                    return None;
                };
                // The segment's distance from the rim's centre.
                let t = ((o - lo).dot(&ld) / ld.norm_squared()).clamp(e.t[0], e.t[1]);
                if (lo + ld * t - o).norm() <= h.r + tol::POINT {
                    return None;
                }
            }
        }
        for g in &holes[i + 1..] {
            let (a, c) = (h.ends[0].2, g.ends[0].2);
            let axis = h.ends[0].3;
            let off = (c - a) - axis * (c - a).dot(&axis);
            if off.norm() <= h.r + g.r + tol::POINT {
                return None;
            }
        }
    }
    // The block: the holes' walls left out, their rims' loops off the caps.
    let mut filled_src = b.clone();
    let mut drop: Vec<(usize, usize)> = holes
        .iter()
        .flat_map(|h| h.ends.iter().map(|e| (e.0, e.1)))
        .collect();
    drop.sort_unstable_by(|x, y| y.cmp(x));
    for (cap, li) in drop {
        filled_src.faces[cap].loops.remove(li);
    }
    let keep: Vec<usize> = (0..b.faces.len())
        .filter(|fi| holes.iter().all(|h| h.wall != *fi))
        .collect();
    let filled = crate::native::kernel::subset(&filled_src, &keep, true);
    let mut g = grown_convex(&filled, d)?;
    let whole = crate::native::mass::body_volume(&g);
    let mut dimples = 0.0;
    for h in &holes {
        let r = h.r;
        // A hole the distance only narrows: its wall moved in, a ring's quarter at each mouth.
        if d < r {
            let removed = narrowed_hole(&mut g, &keep, h, d)?;
            dimples += removed;
            continue;
        }
        let va = (-r / d).clamp(-1.0, 1.0).acos();
        for &(cap, _, o, n) in &h.ends {
            let gcap = keep.iter().position(|&k| k == cap)?;
            let tf = Frame::from_z(o, n);
            let surf = Surface::Torus {
                f: tf,
                rmaj: r,
                rmin: d,
            };
            let (x, y, z) = (tf.x, tf.y, tf.z);
            // The circle on the moved cap, the meridian at u = 0 down to the apex.
            let cv = g.verts.len();
            g.verts.push(Vertex {
                p: o + x * r + z * d,
                tol: TOL_MIN,
            });
            let av = g.verts.len();
            g.verts.push(Vertex {
                p: o + z * (d * va.sin()),
                tol: TOL_MIN,
            });
            let circle = Curve::Circle {
                c: o + z * d,
                x,
                y,
                r,
            };
            let meridian = Curve::Circle {
                c: o + x * r,
                x,
                y: z,
                r: d,
            };
            let push =
                |g: &mut Body, curve: Curve, t: [f64; 2], v: [usize; 2], degenerate: bool| {
                    g.edges.push(Edge {
                        curve,
                        t,
                        v,
                        degenerate,
                        tol: TOL_MIN,
                    });
                    g.edges.len() - 1
                };
            let ec = push(&mut g, circle.clone(), [0.0, TAU], [cv, cv], false);
            let em = push(&mut g, meridian, [FRAC_PI_2, va], [cv, av], false);
            let apex = Curve::Line {
                o: o + z * (d * va.sin()),
                d: V3::zeros(),
            };
            let ea = push(&mut g, apex, [0.0, TAU], [av, av], true);
            let line = |ox: f64, oy: f64, dx: f64, dy: f64| Curve::Line {
                o: P3::new(ox, oy, 0.0),
                d: V3::new(dx, dy, 0.0),
            };
            // Counter-clockwise in (u, v): down the seam at u = 0, along the circle, up the
            // seam at u = 2π, back along the apex.
            let mut lp = vec![
                Coedge {
                    edge: em,
                    fwd: false,
                    pc: line(0.0, 0.0, 0.0, 1.0),
                },
                Coedge {
                    edge: ec,
                    fwd: true,
                    pc: line(0.0, FRAC_PI_2, 1.0, 0.0),
                },
                Coedge {
                    edge: em,
                    fwd: true,
                    pc: line(TAU, 0.0, 0.0, 1.0),
                },
                Coedge {
                    edge: ea,
                    fwd: false,
                    pc: line(0.0, va, 1.0, 0.0),
                },
            ];
            // Its outward normal out of the tube (the material is within d of the rim).
            let (um, vm) = (std::f64::consts::PI, 0.5 * (FRAC_PI_2 + va));
            let pm = surf.eval(um, vm);
            let core = o + (x * um.cos() + y * um.sin()) * r;
            let sense = surf.normal(um, vm).dot(&(pm - core)) > 0.0;
            if !sense {
                lp.reverse();
                for c in lp.iter_mut() {
                    c.fwd = !c.fwd;
                }
            }
            let circle_fwd = lp.iter().find(|c| c.edge == ec)?.fwd;
            let csurf = g.faces[gcap].surf.clone();
            let pc = crate::native::pcurve::pcurve(&csurf, &circle, 0.0, TAU, None);
            g.faces[gcap].loops.push(vec![Coedge {
                edge: ec,
                fwd: !circle_fwd,
                pc,
            }]);
            g.faces.push(Face {
                surf,
                sense,
                loops: vec![lp],
            });
            let fi = g.faces.len() - 1;
            g.shells[0].faces.push(fi);
            let i0 = 0.5 * r * (d * d - r * r).sqrt() + 0.5 * d * d * (r / d).asin();
            let i1 = (d.powi(3) - (d * d - r * r).powf(1.5)) / 3.0;
            dimples += std::f64::consts::PI * d * r * r - TAU * (r * i0 - i1);
        }
    }
    if !g.check_with(CheckLevel::Fast).is_empty() {
        return None;
    }
    let got = crate::native::mass::body_volume(&g);
    ((whole - dimples - got).abs() <= 1e-9 * whole.abs().max(1.0)).then_some(g)
}

#[cfg(test)]
mod tests {
    use crate::native::kernel::NativeKernel;
    use crate::*;
    use nalgebra::Point3;
    use std::f64::consts::PI;

    /// A cube of side a grown by r, built directly (a convex polyhedron): its volume
    /// a³ + 6a²r + 3πar² + 4πr³/3 and area 6a² + 6πar + 4πr², and its 26 faces (6 moved, 12
    /// cylinder strips, 8 ball corners).
    #[test]
    fn a_cube_grown_is_built_directly_and_measures_right() {
        let (a, r) = (10.0, 1.0);
        let mut k = NativeKernel::new();
        let id = k
            .primitive(&Primitive::Box {
                min: Point3::new(0.0, 0.0, 0.0),
                max: Point3::new(a, a, a),
            })
            .unwrap()
            .bodies[0];
        let b = (**k.get(id).unwrap()).clone();
        let g = super::grown_convex(&b, r).expect("a cube is a convex polyhedron");
        assert_eq!(g.faces.len(), 26);
        let gid = k.insert(g);
        let m = k.mass_properties(gid).unwrap();
        let volume =
            a * a * a + 6.0 * a * a * r + 3.0 * PI * a * r * r + 4.0 * PI * r * r * r / 3.0;
        let area = 6.0 * a * a + 6.0 * PI * a * r + 4.0 * PI * r * r;
        assert!(
            (m.volume - volume).abs() < 1e-9 * volume,
            "{} {volume}",
            m.volume
        );
        assert!(
            (m.surface_area - area).abs() < 1e-9 * area,
            "{} {area}",
            m.surface_area
        );
    }

    /// A cylinder cut aslant, grown: built directly (three moved faces, a torus round the base
    /// circle and a spline round the ellipse), its volume Steiner's V + S d + M d² + 4πd³/3 for
    /// a convex body (M the mean curvature's integral: the side's area over its diameter, and
    /// along each edge half the angle between its faces' normals).
    #[test]
    fn a_cylinder_cut_aslant_grown_is_built_directly_and_measures_right() {
        let (r, h, alpha) = (3.0f64, 10.0f64, 45f64.to_radians());
        let mut k = NativeKernel::new();
        let whole = k
            .primitive(&Primitive::Cylinder {
                base: Point3::new(0.0, 0.0, -h),
                axis: nalgebra::Unit::new_normalize(nalgebra::Vector3::z()),
                radius: r,
                height: 3.0 * h,
            })
            .unwrap()
            .bodies[0];
        let t = alpha.tan();
        let trace = Loop::polygon(&[
            [-20.0, -20.0 * t],
            [20.0, 20.0 * t],
            [20.0, -40.0],
            [-20.0, -40.0],
        ]);
        let profile = Profile::of(Plane::xz(0.0), vec![trace]);
        let below = k
            .extrude_with(
                &profile,
                &ExtrudeSpec::blind(profile.plane.normal, 40.0).both_ways(),
            )
            .unwrap()
            .bodies[0];
        let cut = k
            .boolean(BoolOp::Intersect, whole, &[below])
            .unwrap()
            .bodies[0];
        let b = (**k.get(cut).unwrap()).clone();
        // The ellipse's part of M: half the angle between the side's normal and the cut's,
        // along its length.
        let n = 20000;
        let along: f64 = (0..n)
            .map(|i| {
                let u = 2.0 * PI * (i as f64 + 0.5) / n as f64;
                let angle = (-alpha.sin() * u.cos()).acos();
                let speed = r * (u.sin().powi(2) / alpha.cos().powi(2) + u.cos().powi(2)).sqrt();
                0.5 * angle * speed * 2.0 * PI / n as f64
            })
            .sum();
        let v0 = PI * r * r * h;
        let s0 = 2.0 * PI * r * h + PI * r * r + PI * r * r / alpha.cos();
        let m0 = PI * h + PI * PI * r / 2.0 + along;
        for d in [0.5, 1.0, 4.0] {
            let g = super::grown_round(&b, d).expect("a convex body of planes and a cylinder");
            assert_eq!(g.faces.len(), 5);
            let gid = k.insert(g);
            let m = k.mass_properties(gid).unwrap();
            let volume = v0 + s0 * d + m0 * d * d + 4.0 * PI * d * d * d / 3.0;
            assert!(
                (m.volume - volume).abs() < 1e-6 * volume,
                "grown by {d}: {} {volume}",
                m.volume
            );
        }
    }

    /// A cylinder and a cone grown by revolving their grown sections agree with the general
    /// route (the slabs', tubes' and balls' union): volume and area to 1e-9, the same faces.
    #[test]
    fn solids_of_revolution_grown_agree_with_the_general_route() {
        let z = nalgebra::Unit::new_normalize(nalgebra::Vector3::z());
        let shapes = [
            Primitive::Cylinder {
                base: Point3::new(1.0, 2.0, 0.0),
                axis: z,
                radius: 5.0,
                height: 10.0,
            },
            Primitive::Cone {
                base: Point3::new(0.0, 0.0, -3.0),
                axis: z,
                base_radius: 6.0,
                top_radius: 0.0,
                height: 9.0,
            },
            Primitive::Cone {
                base: Point3::new(0.0, 0.0, 0.0),
                axis: z,
                base_radius: 6.0,
                top_radius: 2.0,
                height: 5.0,
            },
        ];
        for shape in shapes {
            let mut k = NativeKernel::new();
            let id = k.primitive(&shape).unwrap().bodies[0];
            let b = (**k.get(id).unwrap()).clone();
            let fast = crate::native::sweep_solid::grown_revolved(&b, 1.5).expect("revolved");
            let general = super::grown_except(&super::merged(&b), 1.5, &[], &[]).unwrap();
            let (f, g) = (k.insert(fast), k.insert(general));
            let (mf, mg) = (k.mass_properties(f).unwrap(), k.mass_properties(g).unwrap());
            assert!(
                (mf.volume - mg.volume).abs() < 1e-9 * mg.volume,
                "{shape:?}: {} {}",
                mf.volume,
                mg.volume
            );
            assert!((mf.surface_area - mg.surface_area).abs() < 1e-9 * mg.surface_area);
            assert_eq!(k.faces(f).unwrap().len(), k.faces(g).unwrap().len());
        }
    }

    /// An L-shaped prism (arms 20 × 5 and 5 × 15, 8 tall) shrunk by 1 and shelled 1 thick open
    /// at its top, by its outline's inset: the inset outline is the arms' 18 × 3 and 3 × 10
    /// rectangles, joined past their corner by the square of side 1 less the quarter disc about
    /// the reflex corner, area 85 − π/4; the inset 6 tall, the cavity 7.
    #[test]
    fn an_l_prism_shrunk_and_shelled_measures_right() {
        use crate::native::surface::Surface;
        let outline = Loop::polygon(&[
            [0.0, 0.0],
            [20.0, 0.0],
            [20.0, 5.0],
            [5.0, 5.0],
            [5.0, 15.0],
            [0.0, 15.0],
        ]);
        let inset_area = 85.0 - PI / 4.0;
        let mut k = NativeKernel::new();
        let id = k
            .extrude(
                &Profile::of(Plane::xy(0.0), vec![outline]),
                Extent::Blind(8.0),
            )
            .unwrap()
            .bodies[0];
        let b = (**k.get(id).unwrap()).clone();
        let shrunk = super::inset_rounded(&b, &vec![-1.0; b.faces.len()]).unwrap();
        let s = k.insert(shrunk);
        let v = k.mass_properties(s).unwrap().volume;
        assert!((v - 6.0 * inset_area).abs() < 1e-9 * v, "{v}");
        let top = (0..b.faces.len())
            .find(|&f| match &b.faces[f].surf {
                Surface::Plane { f: fr } => (fr.o.z - 8.0).abs() < 1e-9,
                _ => false,
            })
            .unwrap();
        let spec = ShellSpec::inward(vec![FaceId(top as u64)], 1.0);
        let shelled = k.shell_with(id, &spec).unwrap().bodies[0];
        let v = k.mass_properties(shelled).unwrap().volume;
        let want = 150.0 * 8.0 - 7.0 * inset_area;
        assert!((v - want).abs() < 1e-9 * want, "{v} {want}");
    }
}
