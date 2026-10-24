//! Surfacing (Onshape's Thicken, Fill, sewing) and Simplify.
//!
//! - **Thicken**: each face (of a surface body, a picked face, a sketch region's face) swept
//!   along its normal (and the other way), the slabs united. Planar faces are swept exactly; a
//!   cylinder's band is the ring between the radii.
//! - **Fill**: the boundary chained into a closed loop; a planar loop is filled with its plane,
//!   four (or three) curves with a Coons patch (bilinearly blended boundaries, sampled and
//!   interpolated by a B-spline surface).
//! - **Sew**: the faces of surface bodies glued along coincident edges into one closed shell.
//! - **Simplify**: faces on the same surface and edges on the same curve merged.

use std::sync::Arc;

use super::build::fix_senses;
use super::curve::Curve;
use super::features::Tagged;
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::{NurbsCurve, NurbsSurface};
use super::surface::Surface;
use super::sweep::path::{Seg, chain, path_segments};
use super::tol;
use super::topo::{Body, Coedge, Edge, Face, Shell};
use crate::*;

fn invalid(why: &str) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

/// One face as a body of its own (a sheet).
fn face_body(b: &Body, f: usize) -> Body {
    super::kernel::subset(b, &[f], false)
}

/// A face swept `d` along its outward normal (negative: the other way).
fn slab(face: &Body, d: f64) -> Result<Body> {
    let f = &face.faces[0];
    match &f.surf {
        Surface::Plane { f: fr } => {
            let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
            let plane = Plane {
                origin: fr.o,
                x_dir: nalgebra::Unit::new_normalize(fr.x),
                normal: nalgebra::Unit::new_normalize(n),
            };
            let mut k = NativeKernel::new();
            let id = k.insert(face.clone());
            let mf = super::features::face_as_profile(
                &k,
                &Profile::new(plane, vec![]),
                &FaceInput {
                    body: id,
                    face: FaceId(0),
                    source: 0,
                },
            )?;
            let dir = if d >= 0.0 { n } else { -n };
            let kind = super::sweep::Prism { plane, dir };
            Ok(super::sweep::sweep_full(&[mf], &[], &kind, 0.0, d.abs(), false, true)?.body)
        }
        _ => super::offset::slab::face_slab(face, 0, d),
    }
}

pub fn thicken(k: &mut NativeKernel, spec: &ThickenSpec) -> Result<OpResult> {
    if !(spec.along >= 0.0 && spec.against >= 0.0 && spec.along + spec.against > tol::POINT) {
        return Err(invalid("The thickness must be greater than zero"));
    }
    let mut sheets: Vec<Body> = Vec::new();
    for b in &spec.bodies {
        let body = k.get(*b)?;
        for f in 0..body.faces.len() {
            sheets.push(face_body(body, f));
        }
    }
    for fi in &spec.faces {
        let body = k.get(fi.body)?;
        if fi.face.0 as usize >= body.faces.len() {
            return Err(invalid("A face to thicken no longer exists"));
        }
        sheets.push(face_body(body, fi.face.0 as usize));
    }
    for p in &spec.profiles {
        let regions = super::profile::regions(p)?;
        let mf = super::profile::merge_regions(&regions)?;
        // The region's face: the start cap of a thin prism.
        let kind = super::sweep::Prism {
            plane: p.plane,
            dir: p.plane.normal.into_inner(),
        };
        let built = super::sweep::sweep_full(&mf, &[], &kind, 0.0, 1.0, false, true)?;
        for (f, o) in &built.origins {
            if let Origin::StartCap { .. } = o {
                let mut fb = face_body(&built.body, *f);
                // The cap faces down (outward of the prism); thicken along the plane's normal.
                Body::reverse_face(&mut fb.faces[0]);
                sheets.push(fb);
            }
        }
    }
    if sheets.is_empty() {
        return Err(invalid("Select surfaces or faces to thicken"));
    }
    // Faces that sew into one sheet with flat open ends: shelled as the solid they bound.
    if sheets.len() > 1
        && let Some(r) = thicken_sheet(k, &sheets, spec)
    {
        return Ok(r);
    }
    let mut acc: Option<Body> = None;
    for s in &sheets {
        for d in [spec.along, -spec.against] {
            if d.abs() < tol::ROUNDOFF {
                continue;
            }
            let sl = slab(s, d)?;
            acc = Some(match acc {
                None => sl,
                Some(a) => super::boolean::boolean_bodies(&a, &sl, BoolOp::Union)?.body,
            });
        }
    }
    let body = acc.ok_or_else(|| invalid("The thickness must be greater than zero"))?;
    let n = body.faces.len();
    let t = Tagged {
        body,
        tags: (0..n)
            .map(|i| {
                vec![Origin::ProfileCurve {
                    region: spec.source,
                    curve: i as u64,
                }]
            })
            .collect(),
    };
    Ok(t.into_result(k))
}

/// A face's normal at an interior point (oriented by its sense), with that point.
fn face_normal(b: &Body, f: usize) -> Option<(P3, V3)> {
    let (_, c) = super::mass::face_area(b, f);
    let face = &b.faces[f];
    let (u, v) = face.surf.project(&c, None);
    let n = face.surf.normal(u, v) * if face.sense { 1.0 } else { -1.0 };
    Some((face.surf.eval(u, v), n))
}

/// Thickens faces that sew into one connected sheet whose free edges close in flat loops: the
/// sheet capped across those loops bounds a solid, which is shelled (inwards or outwards, as
/// the faces' normals point) without its caps. None when that doesn't apply.
fn thicken_sheet(k: &mut NativeKernel, sheets: &[Body], spec: &ThickenSpec) -> Option<OpResult> {
    let mut all = Body::default();
    let mut normals: Vec<(P3, V3)> = Vec::new();
    for s in sheets {
        all.append(s);
        normals.push(face_normal(s, 0)?);
    }
    let sewn = sew_faces(all, tol::POINT).ok()?;
    if sewn.faces.len() != sheets.len() {
        return None;
    }
    // One connected sheet.
    let ef = sewn.edge_faces();
    let mut comp = vec![usize::MAX; sewn.faces.len()];
    let mut stack = vec![0usize];
    comp[0] = 0;
    while let Some(f) = stack.pop() {
        for c in sewn.faces[f].loops.iter().flatten() {
            for &g in &ef[c.edge] {
                if comp[g] == usize::MAX {
                    comp[g] = 0;
                    stack.push(g);
                }
            }
        }
    }
    if comp.contains(&usize::MAX) {
        return None;
    }
    // Orienting may have flipped faces. Picked faces whose normals don't agree on a side of the
    // sheet (a box's walls facing out and its lid in) are each thickened along their own normal,
    // as OCCT did, by the slabs below.
    let agree: usize = (0..sewn.faces.len())
        .filter(|&f| face_normal(&sewn, f).is_some_and(|(_, n)| n.dot(&normals[f].1) > 0.0))
        .count();
    if agree != 0 && agree != sewn.faces.len() {
        return None;
    }
    let along_is_sewn = agree == sewn.faces.len();
    let n = sewn.faces.len();
    let id = k.insert(sewn);
    let tool = super::ops::face_tool(
        k,
        id,
        &(0..n as u64).map(FaceId).collect::<Vec<_>>(),
        spec.source,
    )
    .ok()?;
    let tid = tool.bodies[0];
    let caps: Vec<FaceId> = tool
        .history
        .generated
        .iter()
        .filter(|(_, o)| matches!(o, Origin::StartCap { .. }))
        .map(|(f, _)| *f)
        .collect();
    if caps.is_empty() {
        return None;
    }
    // Whether the sewn sheet's normals point out of the capped solid.
    let solid = k.get(tid).ok()?.clone();
    let sewn_b = k.get(id).ok()?.clone();
    let (p, nsheet) = face_normal(&sewn_b, 0)?;
    let fi = (0..solid.faces.len())
        .find(|&f| {
            !caps.contains(&FaceId(f as u64))
                && face_normal(&solid, f).is_some_and(|(q, _)| (q - p).norm() < tol::POINT)
        })
        .unwrap_or(0);
    let (_, nsolid) = face_normal(&solid, fi)?;
    let sheet_out = nsheet.dot(&nsolid) > 0.0;
    let along_out = sheet_out == along_is_sewn;
    let mut parts: Vec<BodyId> = Vec::new();
    for (d, out) in [(spec.along, along_out), (spec.against, !along_out)] {
        if d < tol::ROUNDOFF {
            continue;
        }
        let r = super::offset::round::shell(
            k,
            tid,
            &ShellSpec {
                remove: caps.clone(),
                thickness: d,
                outward: out,
                hollow: false,
                sharp: false,
            },
        )
        .ok()?;
        parts.push(r.bodies[0]);
    }
    let body = match parts.as_slice() {
        [a] => k.get(*a).ok()?.as_ref().clone(),
        [a, b] => {
            super::boolean::boolean_bodies(k.get(*a).ok()?, k.get(*b).ok()?, BoolOp::Union)
                .ok()?
                .body
        }
        _ => return None,
    };
    if body.solids.len() != 1 {
        return None;
    }
    let nf = body.faces.len();
    let t = Tagged {
        body,
        tags: (0..nf)
            .map(|i| {
                vec![Origin::ProfileCurve {
                    region: spec.source,
                    curve: i as u64,
                }]
            })
            .collect(),
    };
    Some(t.into_result(k))
}

/// The best plane through points, when they lie in one (within 1e-6 of the size).
fn best_plane(pts: &[P3]) -> Option<(P3, V3)> {
    let c = P3::from(pts.iter().fold(V3::zeros(), |a, p| a + p.coords) / pts.len() as f64);
    let mut n = V3::zeros();
    for i in 0..pts.len() {
        n += (pts[i] - c).cross(&(pts[(i + 1) % pts.len()] - c));
    }
    if n.norm() < tol::DEGENERATE {
        return None;
    }
    let n = n.normalize();
    let size = pts.iter().map(|p| (p - c).norm()).fold(0.0, f64::max);
    pts.iter()
        .all(|p| (p - c).dot(&n).abs() < tol::REL_POINT * (1.0 + size))
        .then_some((c, n))
}

pub fn fill(k: &mut NativeKernel, spec: &FillSpec) -> Result<OpResult> {
    let pcs: Vec<PathCurve> = spec
        .curves
        .iter()
        .map(|c| match c {
            FillCurve::Edge { body, edge } => PathCurve::Edge {
                body: *body,
                edge: *edge,
            },
            FillCurve::Sketch { plane, curve } => PathCurve::Sketch {
                plane: *plane,
                curve: curve.clone(),
            },
        })
        .collect();
    if pcs.is_empty() {
        return Err(invalid("Select edges or curves to fill"));
    }
    let segs = path_segments(k, &pcs)?;
    let (path, closed) =
        chain(segs).map_err(|_| invalid("The boundary must be one chain of connected curves"))?;
    if !closed {
        return Err(invalid("The boundary is not closed"));
    }
    let samples: Vec<Vec<P3>> = path
        .iter()
        .map(|s| (0..=32).map(|i| point_at(s, i as f64 / 32.0)).collect())
        .collect();
    let pts: Vec<P3> = samples
        .iter()
        .flat_map(|v| v[..v.len() - 1].iter().copied())
        .collect();
    let body = match best_plane(&pts) {
        Some((c, n)) => planar_face(&path, c, n)?,
        None => coons(&path)?,
    };
    let n = body.faces.len();
    let t = Tagged {
        body,
        tags: (0..n)
            .map(|_| {
                vec![Origin::StartCap {
                    region: spec.source,
                }]
            })
            .collect(),
    };
    Ok(t.into_result(k))
}

fn point_at(s: &Seg, f: f64) -> P3 {
    let t = if s.rev {
        s.t1 + (s.t0 - s.t1) * f
    } else {
        s.t0 + (s.t1 - s.t0) * f
    };
    s.c.point(t)
}

/// A planar sheet bounded by the chained segments.
fn planar_face(path: &[Seg], c: P3, n: V3) -> Result<Body> {
    let fr = Frame::from_z(c, n);
    let surf = Surface::Plane { f: fr };
    let mut b = Body::default();
    let vs: Vec<usize> = path.iter().map(|s| b.add_vertex(s.start())).collect();
    let k = path.len();
    let mut lp = Vec::new();
    for (i, s) in path.iter().enumerate() {
        let (v0, v1) = (vs[i], vs[(i + 1) % k]);
        let (a, bb) = if s.rev { (v1, v0) } else { (v0, v1) };
        let e = b.add_edge(s.c.clone(), s.t0, s.t1, a, bb);
        let pc = super::pcurve::pcurve(&surf, &s.c, s.t0, s.t1, None);
        lp.push(Coedge {
            edge: e,
            fwd: !s.rev,
            pc,
        });
    }
    b.faces.push(Face {
        surf,
        sense: true,
        loops: vec![lp],
    });
    fix_senses(&mut b, 0..1);
    // Outward along the plane's normal by convention: flip if the loop ran the other way.
    if !b.faces[0].sense {
        Body::reverse_face(&mut b.faces[0]);
    }
    b.shells.push(Shell {
        faces: vec![0],
        closed: false,
    });
    Ok(b)
}

/// A Coons patch through four (or three) boundary curves.
fn coons(path: &[Seg]) -> Result<Body> {
    let m = path.len();
    if m != 4 && m != 3 {
        return Err(invalid(
            "Fill takes a planar boundary or four (or three) curves",
        ));
    }
    let side = |i: usize, s: f64| -> P3 { point_at(&path[i], s) };
    // c0(u) bottom, c1(v) right, c2(u) top reversed, c3(v) left reversed; with three curves the
    // fourth collapses to the first curve's start.
    let c0 = |u: f64| side(0, u);
    let c1 = |v: f64| side(1, v);
    let c2 = |u: f64| side(2, 1.0 - u);
    let c3 = |v: f64| {
        if m == 4 {
            side(3, 1.0 - v)
        } else {
            side(0, 0.0)
        }
    };
    let (p00, p10, p11, p01) = (c0(0.0), c0(1.0), c2(1.0), c2(0.0));
    let s = |u: f64, v: f64| -> P3 {
        let a = c0(u).coords * (1.0 - v) + c2(u).coords * v;
        let b = c3(v).coords * (1.0 - u) + c1(v).coords * u;
        let c = p00.coords * ((1.0 - u) * (1.0 - v))
            + p10.coords * (u * (1.0 - v))
            + p11.coords * (u * v)
            + p01.coords * ((1.0 - u) * v);
        P3::from(a + b - c)
    };
    let n = 24;
    let params: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
    let rows: Vec<NurbsCurve> = params
        .iter()
        .map(|&v| {
            NurbsCurve::interpolate(
                &params.iter().map(|&u| s(u, v)).collect::<Vec<_>>(),
                &params,
                3,
                None,
                None,
            )
        })
        .collect();
    // Columns through the rows' poles (all rows share their knots).
    let ncp = rows[0].cps.len();
    let mut cols: Vec<Vec<super::nurbs::H>> = Vec::new();
    let mut kv = Vec::new();
    for i in 0..ncp {
        let col: Vec<P3> = rows
            .iter()
            .map(|r| super::nurbs::dehom(&r.cps[i]))
            .collect();
        let c = NurbsCurve::interpolate(&col, &params, 3, None, None);
        kv = c.knots.clone();
        cols.push(c.cps);
    }
    let surf = NurbsSurface {
        du: 3,
        dv: 3,
        ku: rows[0].knots.clone(),
        kv,
        cps: cols,
    };
    // Edges: the surface's boundary iso-curves (exact on the patch).
    let iso_u = |at_v1: bool| NurbsCurve {
        degree: 3,
        knots: surf.ku.clone(),
        cps: surf
            .cps
            .iter()
            .map(|c| if at_v1 { c[c.len() - 1] } else { c[0] })
            .collect(),
    };
    let iso_v = |at_u1: bool| NurbsCurve {
        degree: 3,
        knots: surf.kv.clone(),
        cps: if at_u1 {
            surf.cps[surf.cps.len() - 1].clone()
        } else {
            surf.cps[0].clone()
        },
    };
    let mut b = Body::default();
    let v00 = b.add_vertex(p00);
    let v10 = b.add_vertex(p10);
    let v11 = b.add_vertex(p11);
    let v01 = if m == 4 { b.add_vertex(p01) } else { v00 };
    let line2 = |o: P2, d: V2| Curve::Line {
        o: P3::new(o.x, o.y, 0.0),
        d: V3::new(d.x, d.y, 0.0),
    };
    let e0 = b.add_edge(Curve::Nurbs(Arc::new(iso_u(false))), 0.0, 1.0, v00, v10);
    let e1 = b.add_edge(Curve::Nurbs(Arc::new(iso_v(true))), 0.0, 1.0, v10, v11);
    let e2 = b.add_edge(Curve::Nurbs(Arc::new(iso_u(true))), 0.0, 1.0, v01, v11);
    let mut lp = vec![
        Coedge {
            edge: e0,
            fwd: true,
            pc: line2(P2::new(0.0, 0.0), V2::new(1.0, 0.0)),
        },
        Coedge {
            edge: e1,
            fwd: true,
            pc: line2(P2::new(1.0, 0.0), V2::new(0.0, 1.0)),
        },
        Coedge {
            edge: e2,
            fwd: false,
            pc: line2(P2::new(0.0, 1.0), V2::new(1.0, 0.0)),
        },
    ];
    if m == 4 {
        let e3 = b.add_edge(Curve::Nurbs(Arc::new(iso_v(false))), 0.0, 1.0, v00, v01);
        lp.push(Coedge {
            edge: e3,
            fwd: false,
            pc: line2(P2::new(0.0, 0.0), V2::new(0.0, 1.0)),
        });
    } else {
        let e3 = b.add_degenerate(p00, 0.0, 1.0, v00);
        lp.push(Coedge {
            edge: e3,
            fwd: false,
            pc: line2(P2::new(0.0, 0.0), V2::new(0.0, 1.0)),
        });
    }
    b.faces.push(Face {
        surf: Surface::Nurbs(Arc::new(surf)),
        sense: true,
        loops: vec![lp],
    });
    b.shells.push(Shell {
        faces: vec![0],
        closed: false,
    });
    Ok(b)
}

/// Glues the faces of the bodies along coincident edges into one closed solid.
pub fn sew(k: &mut NativeKernel, bodies: &[BodyId], tol: f64) -> Result<OpResult> {
    let mut b = Body::default();
    for id in bodies {
        b.append(k.get(*id)?);
    }
    let mut b = sew_faces(b, tol)?;
    b.shells.clear();
    b.solids.clear();
    super::boolean::assemble_shells(&mut b)?;
    if b.solids.is_empty() {
        return Err(invalid("The surfaces don't close"));
    }
    let mut v = super::mass::body_volume(&b);
    if v < 0.0 {
        for f in &mut b.faces {
            Body::reverse_face(f);
        }
        v = -v;
    }
    if v < tol::ROUNDOFF {
        return Err(invalid("The surfaces don't enclose a volume"));
    }
    let n = b.faces.len();
    let t = Tagged {
        body: b,
        tags: (0..n)
            .map(|i| {
                vec![Origin::ProfileCurve {
                    region: u64::MAX,
                    curve: i as u64,
                }]
            })
            .collect(),
    };
    Ok(t.into_result(k))
}

/// Faces sewn into one sheet: coincident vertices and edges merged, the faces oriented
/// consistently (the sheet may stay open), one shell per edge-connected piece and no solids;
/// p-curves off their edges refitted and the tolerances fitted to the gaps left.
pub fn sew_faces(mut b: Body, tol: f64) -> Result<Body> {
    let tol = tol.max(1e-7);
    // Merge coincident vertices.
    let nv = b.verts.len();
    let mut vmap: Vec<usize> = (0..nv).collect();
    let point_box = |p: &P3, r: f64| {
        let mut bb = Bbox::empty();
        bb.add(p);
        bb.grow(r)
    };
    let vtree = super::bvh::Bvh::new(b.verts.iter().map(|v| point_box(&v.p, 0.0)).collect());
    for i in 0..nv {
        if vmap[i] != i {
            continue;
        }
        let mut near = vtree.query(&point_box(&b.verts[i].p, tol));
        near.sort_unstable();
        for j in near.into_iter().filter(|&j| j > i) {
            if vmap[j] == j && (b.verts[i].p - b.verts[j].p).norm() < tol {
                vmap[j] = i;
            }
        }
    }
    for e in &mut b.edges {
        e.v = [vmap[e.v[0]], vmap[e.v[1]]];
    }
    // T-junctions: a free edge with another free edge's vertex lying on it (one face's long
    // edge against two of its neighbour's) is split there, at that vertex.
    for _ in 0..10_000 {
        let ef = b.edge_faces();
        let free: Vec<usize> = (0..b.edges.len())
            .filter(|&e| ef[e].len() == 1 && !b.edges[e].degenerate)
            .collect();
        let mut ends: Vec<usize> = free.iter().flat_map(|&e| b.edges[e].v).collect();
        ends.sort_unstable();
        ends.dedup();
        let mut hit = None;
        let etree = super::bvh::Bvh::new(
            ends.iter()
                .map(|&w| point_box(&b.verts[w].p, 0.0))
                .collect(),
        );
        'find: for &e in &free {
            let ed = &b.edges[e];
            let (lo, hi) = (ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
            let mut near = etree.query(&super::boolean::curve_bbox(&ed.curve, lo, hi).grow(tol));
            near.sort_unstable();
            for w in near.into_iter().map(|k| ends[k]) {
                if ed.v.contains(&w) {
                    continue;
                }
                let p = b.verts[w].p;
                let t = ed.curve.closest(&p, lo, hi);
                let q = ed.curve.point(t);
                if (q - p).norm() < tol
                    && (q - ed.point(ed.t[0])).norm() > tol
                    && (q - ed.point(ed.t[1])).norm() > tol
                {
                    hit = Some((e, t, w));
                    break 'find;
                }
            }
        }
        let Some((e, t, w)) = hit else { break };
        split_edge_at(&mut b, e, t, w);
    }
    // Merge coincident edges (same ends and middle), re-parametrizing the p-curves.
    // Pairs only (an edge bounds two faces at most), closest first: an edge between two
    // vertices that several edges join takes the one that runs along it.
    let ne = b.edges.len();
    let mut emap: Vec<(usize, bool)> = (0..ne).map(|i| (i, true)).collect();
    let mut uses = vec![0usize; ne];
    for c in b.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        uses[c.edge] += 1;
    }
    let mut cands: Vec<(f64, usize, usize, bool)> = Vec::new();
    for i in 0..ne {
        if b.edges[i].degenerate || uses[i] >= 2 {
            continue;
        }
        for j in i + 1..ne {
            if b.edges[j].degenerate || uses[i] + uses[j] > 2 {
                continue;
            }
            let (a, c) = (&b.edges[i], &b.edges[j]);
            let same = a.v == c.v || a.v == [c.v[1], c.v[0]];
            if !same {
                continue;
            }
            // The farthest of a few points of one from the other.
            let mut dmax: f64 = 0.0;
            for f in [0.25, 0.5, 0.75] {
                let m = c.point(c.t[0] + (c.t[1] - c.t[0]) * f);
                let t = a.curve.closest(&m, a.t[0], a.t[1]);
                dmax = dmax.max((a.curve.point(t) - m).norm());
            }
            if dmax < tol {
                let m = c.point(c.mid_t());
                let t = a.curve.closest(&m, a.t[0], a.t[1]);
                let fwd = if a.v[0] != a.v[1] {
                    a.v == c.v
                } else {
                    a.curve.tangent(t).dot(&c.curve.tangent(c.mid_t())) > 0.0
                };
                cands.push((dmax, i, j, fwd));
            }
        }
    }
    cands.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut taken = vec![false; ne];
    for (_, i, j, fwd) in cands {
        if taken[i] || taken[j] {
            continue;
        }
        taken[i] = true;
        taken[j] = true;
        emap[j] = (i, fwd);
    }
    for fi in 0..b.faces.len() {
        let surf = b.faces[fi].surf.clone();
        for li in 0..b.faces[fi].loops.len() {
            for ci in 0..b.faces[fi].loops[li].len() {
                let c = b.faces[fi].loops[li][ci].clone();
                let (x, same) = emap[c.edge];
                if x == c.edge {
                    continue;
                }
                let ed = b.edges[x].clone();
                let fwd = if same { c.fwd } else { !c.fwd };
                let old = b.edges[c.edge].clone();
                let (ot0, _) = super::topo::co_range(&old, c.fwd);
                let sp = c.pc.point(ot0);
                let mut pc = super::pcurve::pcurve(&surf, &ed.curve, ed.t[0], ed.t[1], None);
                let (nt0, _) = super::topo::co_range(&ed, fwd);
                let q = pc.point(nt0);
                let du = if surf.u_periodic() {
                    ((sp.x - q.x) / TAU).round() * TAU
                } else {
                    0.0
                };
                let dv = if surf.v_periodic() {
                    ((sp.y - q.y) / TAU).round() * TAU
                } else {
                    0.0
                };
                if du != 0.0 || dv != 0.0 {
                    pc = pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)));
                }
                b.faces[fi].loops[li][ci] = Coedge { edge: x, fwd, pc };
            }
        }
    }
    super::unify::compact(&mut b);
    // Orient the faces consistently: across every shared edge the two uses run opposite ways.
    orient(&mut b)?;
    // One shell per piece of faces joined through shared edges (the input's shells no longer
    // say what is joined, and a read model may list a face twice), closed when it uses each
    // edge once each way. No solids: a caller that wants them assembles them.
    let all: Vec<usize> = (0..b.faces.len()).collect();
    let pieces = b.face_components(&all);
    b.shells = pieces
        .into_iter()
        .map(|faces| {
            let mut uses: std::collections::HashMap<usize, (u32, u32)> =
                std::collections::HashMap::new();
            for c in faces
                .iter()
                .flat_map(|&f| b.faces[f].loops.iter().flatten())
            {
                let u = uses.entry(c.edge).or_default();
                if c.fwd {
                    u.0 += 1;
                } else {
                    u.1 += 1;
                }
            }
            let closed = uses
                .iter()
                .all(|(&e, &u)| u == (1, 1) || b.edges[e].degenerate);
            Shell { faces, closed }
        })
        .collect();
    b.solids.clear();
    // Sewn within `tol`: the merged vertices and edges carry the gaps they close. Then OCCT's
    // SameParameter step (`BRepBuilderAPI_Sewing::SameParameterShape`, on by default): a
    // p-curve that strays from its edge further than that is projected again where that
    // strays less, and the tolerances grow (within `tol`) to the gaps left, searched for along
    // each edge rather than sampled.
    b.fit_tolerances(tol);
    super::heal::refit_pcurves(&mut b, 0.0);
    b.fit_tolerances_searched(tol);
    Ok(b)
}

/// Flips faces so each shared edge is used once each way (breadth first from face 0).
pub(crate) fn orient(b: &mut Body) -> Result<()> {
    let nf = b.faces.len();
    let ef = b.edge_faces();
    let mut done = vec![false; nf];
    for start in 0..nf {
        if done[start] {
            continue;
        }
        done[start] = true;
        let mut queue = vec![start];
        while let Some(f) = queue.pop() {
            let uses: Vec<(usize, bool)> = b.faces[f]
                .loops
                .iter()
                .flatten()
                .map(|c| (c.edge, c.fwd))
                .collect();
            for (e, fwd) in uses {
                for &g in &ef[e] {
                    if g == f || done[g] {
                        continue;
                    }
                    let gfwd = b.faces[g]
                        .loops
                        .iter()
                        .flatten()
                        .find(|c| c.edge == e)
                        .map(|c| c.fwd)
                        .unwrap_or(!fwd);
                    if gfwd == fwd {
                        Body::reverse_face(&mut b.faces[g]);
                    }
                    done[g] = true;
                    queue.push(g);
                }
            }
        }
    }
    Ok(())
}

/// Simplify: faces on the same surface and edges on one curve merged.
pub fn simplify(k: &mut NativeKernel, body: BodyId) -> Result<OpResult> {
    let mut b = (**k.get(body)?).clone();
    let n = b.faces.len();
    let from: Vec<Vec<(usize, usize)>> = (0..n).map(|i| vec![(0, i)]).collect();
    let from = super::unify::unify_faces(&mut b, from, &|_, _| true);
    super::unify::merge_edges(&mut b);
    super::boolean::assemble_shells(&mut b)?;
    if b.solids.len() != 1 {
        return Err(KernelError::InvalidResult {
            op: "simplify",
            why: "the simplified body is not a valid solid".into(),
        });
    }
    let mut history = History::default();
    for (i, srcs) in from.iter().enumerate() {
        for &(_, f) in srcs {
            history.modified.push((
                FaceId(i as u64),
                InputFace {
                    body,
                    face: FaceId(f as u64),
                },
            ));
        }
    }
    let id = k.insert(b);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// Delete face (defeaturing): faces whose whole boundary runs along inner loops of the faces
/// left (a hole's wall, a boss's sides and top, a pocket) go, and with them those loops: the
/// surrounding faces close over the gap.
pub fn delete_faces(k: &mut NativeKernel, body: BodyId, remove: &[FaceId]) -> Result<OpResult> {
    if remove.is_empty() {
        return Err(invalid("Select the faces to delete"));
    }
    let b = (**k.get(body)?).clone();
    let gone: Vec<usize> = remove.iter().map(|f| f.0 as usize).collect();
    if gone.iter().any(|&f| f >= b.faces.len()) {
        return Err(KernelError::InvalidParameter("unknown face".into()));
    }
    let ef = b.edge_faces();
    // Edges between a deleted face and a kept one.
    let boundary: Vec<usize> = (0..b.edges.len())
        .filter(|&e| {
            ef[e].iter().any(|f| gone.contains(f)) && ef[e].iter().any(|f| !gone.contains(f))
        })
        .collect();
    let mut out = b.clone();
    for fi in 0..out.faces.len() {
        if gone.contains(&fi) {
            continue;
        }
        let loops = std::mem::take(&mut out.faces[fi].loops);
        let mut kept = Vec::new();
        for (li, l) in loops.into_iter().enumerate() {
            let touches = l.iter().any(|c| boundary.contains(&c.edge));
            if !touches {
                kept.push(l);
                continue;
            }
            let all = l.iter().all(|c| boundary.contains(&c.edge));
            if li == 0 || !all {
                // Not whole inner loops (a teardrop's sides, a fillet, a chamfer): the faces
                // round them extended over the gap.
                return super::defeature::heal(k, body, &gone);
            }
        }
        out.faces[fi].loops = kept;
    }
    let keep: Vec<usize> = (0..out.faces.len()).filter(|f| !gone.contains(f)).collect();
    out.faces = keep.iter().map(|&f| out.faces[f].clone()).collect();
    super::unify::compact(&mut out);
    out.shells.clear();
    out.solids.clear();
    super::boolean::assemble_shells(&mut out)?;
    if out.solids.len() != 1 {
        return Err(KernelError::InvalidResult {
            op: "delete face",
            why: "the faces can't be deleted: the healed body is not a valid solid".into(),
        });
    }
    let mut history = History::default();
    for (i, f) in keep.iter().enumerate() {
        history.modified.push((
            FaceId(i as u64),
            InputFace {
                body,
                face: FaceId(*f as u64),
            },
        ));
    }
    for f in &gone {
        history.deleted.push(InputFace {
            body,
            face: FaceId(*f as u64),
        });
    }
    let id = k.insert(out);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// Delete face's Cap and Leave open (Heal is [`delete_faces`]): the faces removed; left open,
/// the rest is a sheet; capped, each opening (a loop of the sheet's free edges) gets a face
/// ([`fill`]: a plane on a flat opening, else a fill) and the faces are sewn into a solid.
pub fn delete_faces_with(
    k: &mut NativeKernel,
    body: BodyId,
    remove: &[FaceId],
    void: VoidFill,
) -> Result<OpResult> {
    if void == VoidFill::Heal {
        return delete_faces(k, body, remove);
    }
    if remove.is_empty() {
        return Err(invalid("Select the faces to delete"));
    }
    let b = (**k.get(body)?).clone();
    let gone: Vec<usize> = remove.iter().map(|f| f.0 as usize).collect();
    if gone.iter().any(|&f| f >= b.faces.len()) {
        return Err(KernelError::InvalidParameter("unknown face".into()));
    }
    let keep: Vec<usize> = (0..b.faces.len()).filter(|f| !gone.contains(f)).collect();
    if keep.is_empty() {
        return Err(invalid("Deleting every face leaves nothing"));
    }
    let mut sheet = super::kernel::subset(&b, &keep, false);
    sheet.shells.clear();
    super::boolean::assemble_shells(&mut sheet)?;
    let mut history = History::default();
    for (i, &f) in keep.iter().enumerate() {
        history.modified.push((
            FaceId(i as u64),
            InputFace {
                body,
                face: FaceId(f as u64),
            },
        ));
    }
    for &f in &gone {
        history.deleted.push(InputFace {
            body,
            face: FaceId(f as u64),
        });
    }
    if void == VoidFill::Open {
        let id = k.insert(sheet);
        return Ok(OpResult {
            bodies: vec![id],
            history,
        });
    }
    // The openings: the sheet's free edges chained into loops at their vertices.
    let ef = sheet.edge_faces();
    let mut free: Vec<usize> = (0..sheet.edges.len())
        .filter(|&e| ef[e].len() == 1)
        .collect();
    let mut loops: Vec<Vec<usize>> = Vec::new();
    while let Some(first) = free.pop() {
        let mut lp = vec![first];
        let mut grew = true;
        while grew {
            grew = false;
            let ends: Vec<usize> = lp.iter().flat_map(|&e| sheet.edges[e].v).collect();
            if let Some(i) = free
                .iter()
                .position(|&e| sheet.edges[e].v.iter().any(|v| ends.contains(v)))
            {
                lp.push(free.remove(i));
                grew = true;
            }
        }
        loops.push(lp);
    }
    if loops.is_empty() {
        return Err(invalid("The faces left have no opening to cap"));
    }
    let sid = k.insert(sheet.clone());
    let mut caps = Vec::new();
    for lp in &loops {
        let spec = FillSpec {
            curves: lp
                .iter()
                .map(|&e| FillCurve::Edge {
                    body: sid,
                    edge: EdgeId(e as u64),
                })
                .collect(),
            source: 0,
        };
        match fill(k, &spec) {
            Ok(r) => caps.extend(r.bodies),
            Err(e) => {
                k.release(sid);
                for c in caps {
                    k.release(c);
                }
                return Err(e);
            }
        }
    }
    k.release(sid);
    let mut all = sheet;
    for c in caps {
        all.append(k.get(c)?);
        k.release(c);
    }
    let mut all = sew_faces(all, tol::POINT)?;
    all.shells.clear();
    all.solids.clear();
    super::boolean::assemble_shells(&mut all)?;
    if all.solids.len() != 1 {
        return Err(invalid("The caps don't close the part"));
    }
    if super::mass::body_volume(&all) < 0.0 {
        for f in &mut all.faces {
            Body::reverse_face(f);
        }
    }
    for i in keep.len()..all.faces.len() {
        history.generated.push((
            FaceId(i as u64),
            Origin::ProfileCurve {
                region: u64::MAX,
                curve: i as u64,
            },
        ));
    }
    let id = k.insert(all);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// Splits edge `e` at parameter `t` at the existing vertex `w`, in every face using it.
fn split_edge_at(b: &mut Body, e: usize, t: f64, w: usize) {
    let old = b.edges[e].clone();
    b.edges[e] = Edge {
        t: [old.t[0], t],
        v: [old.v[0], w],
        ..old.clone()
    };
    b.edges.push(Edge {
        t: [t, old.t[1]],
        v: [w, old.v[1]],
        ..old
    });
    let e2 = b.edges.len() - 1;
    for f in &mut b.faces {
        for l in &mut f.loops {
            let mut nl = Vec::with_capacity(l.len() + 1);
            for c in l.drain(..) {
                if c.edge == e {
                    let second = Coedge {
                        edge: e2,
                        fwd: c.fwd,
                        pc: c.pc.clone(),
                    };
                    if c.fwd {
                        nl.push(c);
                        nl.push(second);
                    } else {
                        nl.push(second);
                        nl.push(c);
                    }
                } else {
                    nl.push(c);
                }
            }
            *l = nl;
        }
    }
}
