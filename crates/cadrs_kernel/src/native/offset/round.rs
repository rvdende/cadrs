//! Rounded offsets (the body grown by a ball: a Minkowski sum) and shells.
//!
//! The grown body is the body, each face swept out by the distance (a slab), a tube round each
//! convex edge and a ball at each vertex, united. Shells: inward, the body less its inward
//! sharp offset (faces moved in, removed faces moved out past the body); when that offset
//! can't be made (faces vanish, as round a counterbore), the walls are built from their
//! definition, the body's points within the thickness of its kept faces: the cavity is the body
//! less each kept face swept in, a tube round each concave edge and a ball at each concave
//! vertex. Outward, the grown body less the body, cut back to the removed faces' planes.

use crate::native::blend::{Tracked, ball_body};
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
fn edge_tube(b: &Body, e: usize, r: f64) -> Result<Body> {
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
            let n = 256;
            let tight = (0..=n).any(|i| {
                let (_, d1, d2) = ed
                    .curve
                    .d2(ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / n as f64);
                d1.cross(&d2).norm() * r >= d1.norm().powi(3)
            });
            if tight {
                return Err(KernelError::Unsupported(
                    "a tube round an edge curved tighter than its radius",
                ));
            }
            let p0 = ed.point(ed.t[0]);
            let t = ed
                .curve
                .tangent(ed.t[0])
                .try_normalize(tol::DEGENERATE)
                .ok_or(KernelError::Unsupported(
                    "a tube round an edge with no direction",
                ))?;
            let fr = Frame::from_z(p0, t);
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
    let debug = std::env::var_os("CADRS_OFFSET_DEBUG").is_some();
    let dump = || {
        if std::env::var_os("CADRS_DUMP_DIR").is_some() {
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
    grown_except(&merged(b), d, &[], &[])
}

/// A convex polyhedron grown by a ball of radius `d`, built face by face: each face moved out
/// by `d`, a cylinder of radius `d` along each edge between its faces' moved copies, and a ball
/// patch at each corner bounded by its edges' end arcs. The same solid the slabs', tubes' and
/// balls' union makes, without the dozens of booleans. None unless every face is a plane with
/// one loop and every edge a straight convex edge between two faces (then the result is checked).
fn grown_convex(b: &Body, d: f64) -> Option<Body> {
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

/// `acc` combined (`op`) with each piece in turn. A piece whose boolean fails is tried again
/// after the others: a union or difference of pieces doesn't depend on their order, but the
/// touching configurations a boolean has to settle do (a slab's side against a neighbour's
/// slab, not yet merged into a face). Fails when a round makes no progress, or after three
/// rounds (each failed boolean costs its retries).
fn combine_all(mut acc: Body, mut todo: Vec<(Body, String)>, op: BoolOp) -> Result<Body> {
    for round in 0..3 {
        let n = todo.len();
        let mut left = Vec::new();
        let mut first: Option<KernelError> = None;
        for (piece, what) in todo {
            match combine(&acc, &piece, op, &|| what.clone()) {
                Ok(r) => {
                    acc = r;
                    if std::env::var("CADRS_SHELL_DEBUG").is_ok() {
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
    let b = (**k.get(body)?).clone();
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
    let cur = Tracked::of(b.clone());
    let v0 = crate::native::mass::body_volume(&b);
    // Walls whose surfaces the whole body lies within (a box's sides, a bore, a ball's sphere):
    // the cavity is where the moved surfaces' insides meet in the body, as far as the openings.
    let supported = if spec.outward || spec.hollow {
        None
    } else {
        cavity_of_supports(&b, &remove, t)
    };
    if std::env::var("CADRS_SHELL_DEBUG").is_ok() {
        eprintln!(
            "shell: cavity of supporting walls: {:?}",
            supported
                .as_ref()
                .map(|r| r.as_ref().map(|c| c.as_ref().map(|c| c.faces.len())))
        );
    }
    let result = if let Some(Ok(cavity)) = supported {
        match cavity {
            // Every point of the body within t of a wall: nothing is hollowed.
            None => cur,
            Some(cavity) => cur.apply(
                &cavity,
                &vec![Vec::new(); cavity.faces.len()],
                BoolOp::Subtract,
            )?,
        }
    } else if crossing && spec.sharp {
        return Err(too_thick());
    } else if !spec.outward {
        // The cavity: kept faces moved in by t, removed faces moved out past the body, or,
        // failing that (a spline wall's offset doesn't reach that far), where they are (the
        // cavity opens flush with them).
        let dist: Vec<f64> = (0..b.faces.len())
            .map(|f| {
                if remove.contains(&f) {
                    size * 0.1 + 1.0
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
        let concave =
            (0..b.edges.len()).any(|e| !b.edges[e].degenerate && convex(&b, e) == Some(false));
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
                Box::new(|| sharp(&flush)),
                Box::new(regions),
            ]
        };
        // The cavity with the removed faces moved out past the body reaches past them by the
        // walls' insets: where a kept face rises from the opening beyond its plane (an L opened
        // at its low step, issue #237), the corner of the moved face and that face's inset lies
        // inside its wall, and the cavity eats it. The cavity flush with the openings doesn't,
        // and lies wholly in the body: the moved one may take out no more than it does.
        let moved = !((concave || crossing) && !spec.sharp);
        let flush_volume = if moved {
            sharp(&flush)
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
                    if std::env::var("CADRS_SHELL_DEBUG").is_ok() {
                        eprintln!("shell: sharp outward walls failed ({e}); rounded");
                    }
                    rounded()?
                }
            }
        } else {
            rounded()?
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
    if std::env::var("CADRS_SHELL_DEBUG").is_ok() {
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
                let sub = crate::native::kernel::subset(b, &[i], false);
                Some((i, fr.o, n, sub.bbox()))
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
            // Overlap of the faces' boxes across the gap direction.
            let pa = ba.grow(gap);
            if gap < 2.0 * t + tol::ROUNDOFF && pa.overlaps(bb) {
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
    /// Inside (true) or outside the sphere about c of radius r.
    Sphere(P3, f64, bool),
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
            Support::Sphere(c, r, inside) => {
                let rho = (p - c).norm();
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
    let (meshes, _) = crate::native::tess::mesh_body(b, 1e-3 * size, 0.2);
    let pts: Vec<P3> = meshes
        .iter()
        .flat_map(|m| m.positions.iter().copied())
        .chain(b.verts.iter().map(|v| v.p))
        .collect();
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
            Surface::Sphere { f: fr, r } => Support::Sphere(fr.o, *r, (q - fr.o).dot(&n) > 0.0),
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
                Support::Sphere(c, r, true) => {
                    if r - t <= close {
                        return Ok(None);
                    }
                    (ball_body(c, r - t)?, BoolOp::Intersect)
                }
                Support::Sphere(c, r, false) => (ball_body(c, r + t)?, BoolOp::Subtract),
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
    let at_still = |e: usize| !round_still && ef[e].iter().any(|f| still.contains(f));
    let still_verts: Vec<usize> = (0..b.edges.len())
        .filter(|&e| at_still(e))
        .flat_map(|e| b.edges[e].v)
        .collect();
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
