//! Equal-distance chamfers of a convex polyhedron, built at once: the solid is the intersection
//! of the half-spaces under its faces, under each chamfered edge's chamfer plane and (with flat
//! corners) under each cut corner's plane, made directly from those planes (each vertex where
//! three meet, each face the polygon of the vertices on its plane) where the local trimming
//! refuses chamfers meeting at a corner and the corners would each cost a boolean.

use super::blend::{FaceSrc, Tracked, vertex_index};
use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, BodyBuilder, Coedge};
use crate::*;
use std::collections::HashMap;

/// What a plane of the result came from.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tag {
    Face(usize),
    Edge(usize),
    Corner(usize),
}

/// A half-space n·x ≤ c (n a unit vector, pointing out of the solid).
#[derive(Debug, Clone, Copy)]
struct Half {
    n: V3,
    c: f64,
    tag: Tag,
}

/// Body `b`'s `edges` chamfered by `d` on both faces (measured along the faces square to the
/// edge), each corner whose edges are all chamfered cut flat when `flat` (as
/// [`super::blend::flat_corners`] cuts it). None unless `b` is a convex polyhedron (one closed
/// shell of plane faces, one loop each, straight edges, every vertex under every face's plane),
/// every corner where two or more chamfers meet either three edges' or cut flat, and every
/// face, chamfer and corner plane keeps a face of its own (no face consumed).
pub(crate) fn chamfer(
    k: &mut NativeKernel,
    body: BodyId,
    edges: &[usize],
    d: f64,
    flat: bool,
) -> Option<Result<OpResult>> {
    let b = (**k.get(body).ok()?).clone();
    if !(d > 0.0) || edges.is_empty() || b.solids.len() != 1 || b.shells.len() != 1 {
        return None;
    }
    let size = b.bbox().diagonal().max(1.0);
    let eps = 1e-9 * size;
    // The faces' half-spaces.
    let mut halves: Vec<Half> = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        let Surface::Plane { f: fr } = &f.surf else {
            return None;
        };
        if f.loops.len() != 1 {
            return None;
        }
        let n = fr.x.cross(&fr.y).normalize() * if f.sense { 1.0 } else { -1.0 };
        halves.push(Half {
            n,
            c: n.dot(&fr.o.coords),
            tag: Tag::Face(fi),
        });
    }
    if b.edges
        .iter()
        .any(|e| e.degenerate || !matches!(e.curve, Curve::Line { .. }))
    {
        return None;
    }
    // Convex: every vertex under every face's plane.
    if b.verts
        .iter()
        .any(|v| halves.iter().any(|h| h.n.dot(&v.p.coords) > h.c + eps))
    {
        return None;
    }
    let ef = b.edge_faces();
    let chosen: std::collections::HashSet<usize> = edges.iter().copied().collect();
    // Each chamfer's plane: through the lines `d` in from the edge on each of its faces.
    for &e in edges {
        let [f1, f2] = ef.get(e)?.as_slice() else {
            return None;
        };
        let (n1, n2) = (halves[*f1].n, halves[*f2].n);
        let ed = &b.edges[e];
        let (p0, p1) = (b.verts[ed.v[0]].p, b.verts[ed.v[1]].p);
        let dir = (p1 - p0).try_normalize(tol::DEGENERATE)?;
        // Into face 1, square to the edge.
        let w = n1.cross(&dir).normalize();
        let inside = face_middle(&b, *f1);
        let w = if (inside - p0).dot(&w) < 0.0 { -w } else { w };
        let m = (n1 + n2).try_normalize(tol::ANGULAR)?;
        let q = p0 + w * d;
        halves.push(Half {
            n: m,
            c: m.dot(&q.coords),
            tag: Tag::Edge(e),
        });
    }
    // The corners: flat where asked (as flat_corners places them), else a corner where two or
    // more chamfers meet must be three edges' (where the mitre is the planes' meeting point).
    let near = tol::REL_POINT * size;
    for v in 0..b.verts.len() {
        let at: Vec<usize> = (0..b.edges.len())
            .filter(|&e| b.edges[e].v.contains(&v))
            .collect();
        let cut = at.iter().filter(|e| chosen.contains(e)).count();
        let all = at.len() >= 3 && cut == at.len();
        if flat && all {
            let p = b.verts[v].p;
            let mut pts: Vec<P3> = Vec::new();
            for f in &b.faces {
                let mut dirs: Vec<V3> = Vec::new();
                for c in f.loops.iter().flatten() {
                    let e = &b.edges[c.edge];
                    if !e.v.contains(&v) {
                        continue;
                    }
                    let t = if e.v[0] == v { e.t[0] } else { e.t[1] };
                    let tan = e.curve.tangent(t);
                    let away = if e.v[0] == v { tan } else { -tan };
                    if let Some(u) = away.try_normalize(tol::TINY)
                        && !dirs.iter().any(|x| (x - u).norm() < 1e-9)
                    {
                        dirs.push(u);
                    }
                }
                if dirs.is_empty() {
                    continue;
                }
                if dirs.len() != 2 {
                    return None;
                }
                let theta = dirs[0].dot(&dirs[1]).clamp(-1.0, 1.0).acos();
                let bis = (dirs[0] + dirs[1]).try_normalize(tol::TINY)?;
                pts.push(p + bis * (d / (0.5 * theta).sin()));
            }
            if pts.len() < 3 {
                return None;
            }
            let c = P3::from(pts.iter().fold(V3::zeros(), |a, q| a + q.coords) / pts.len() as f64);
            let n = (pts[1] - pts[0]).cross(&(pts[2] - pts[0]));
            let n = n.try_normalize(tol::DEGENERATE)?;
            let n = if (p - c).dot(&n) < 0.0 { -n } else { n };
            let beyond = (p - c).dot(&n);
            // (Not one plane, or cutting nothing: flat_corners leaves it mitred, which a
            // corner of more than three edges can't be here.)
            if beyond <= near || pts.iter().any(|q| (q - c).dot(&n).abs() > near) {
                if at.len() != 3 {
                    return None;
                }
                continue;
            }
            halves.push(Half {
                n,
                c: n.dot(&c.coords),
                tag: Tag::Corner(v),
            });
        } else if cut >= 2 && at.len() != 3 {
            return None;
        }
    }
    let solid = polytope(&halves, eps)?;
    let (body_out, tags) = solid;
    // Every face and chamfer kept a face of its own.
    for (i, h) in halves.iter().enumerate() {
        if !matches!(h.tag, Tag::Corner(_)) && !tags.contains(&i) {
            return None;
        }
    }
    let src = tags
        .iter()
        .map(|&i| {
            vec![match halves[i].tag {
                Tag::Face(f) => FaceSrc::Input(f),
                Tag::Edge(e) => FaceSrc::New(Origin::FromEdge {
                    body,
                    edge: EdgeId(e as u64),
                }),
                Tag::Corner(v) => FaceSrc::New(Origin::FromVertex {
                    body,
                    vertex: VertexId(vertex_index(&b, v) as u64),
                }),
            }]
        })
        .collect();
    Some(Ok(Tracked {
        body: body_out,
        src,
    }
    .into_result(k, body)))
}

/// The middle of face `fi`'s vertices (each once).
fn face_middle(b: &Body, fi: usize) -> P3 {
    let mut vs: Vec<usize> = b.faces[fi]
        .loops
        .iter()
        .flatten()
        .flat_map(|c| b.edges[c.edge].v)
        .collect();
    vs.sort_unstable();
    vs.dedup();
    let s = vs.iter().fold(V3::zeros(), |a, &v| a + b.verts[v].p.coords);
    P3::from(s / vs.len().max(1) as f64)
}

/// The convex polytope the half-spaces bound, and for each of its faces the half-space it lies
/// on: its vertices where three planes meet inside all the others (merged within `eps`), each
/// face the polygon of the vertices on its plane, counter-clockwise about its normal. None
/// when it is empty, unbounded or not a valid solid.
fn polytope(halves: &[Half], eps: f64) -> Option<(Body, Vec<usize>)> {
    let m = halves.len();
    let mut verts: Vec<P3> = Vec::new();
    for i in 0..m {
        for j in i + 1..m {
            for l in j + 1..m {
                let a = nalgebra::Matrix3::from_rows(&[
                    halves[i].n.transpose(),
                    halves[j].n.transpose(),
                    halves[l].n.transpose(),
                ]);
                if a.determinant().abs() < 1e-12 {
                    continue;
                }
                let Some(x) = a
                    .try_inverse()
                    .map(|inv| inv * V3::new(halves[i].c, halves[j].c, halves[l].c))
                else {
                    continue;
                };
                if halves.iter().any(|h| h.n.dot(&x) > h.c + eps) {
                    continue;
                }
                let p = P3::from(x);
                if !verts.iter().any(|q| (q - p).norm() <= 10.0 * eps) {
                    verts.push(p);
                }
            }
        }
    }
    if verts.len() < 4 {
        return None;
    }
    let mut out = BodyBuilder::new();
    let vid: Vec<usize> = verts.iter().map(|&p| out.vertex(p)).collect();
    let mut edge_of: HashMap<(usize, usize), usize> = HashMap::new();
    let mut faces = Vec::new();
    let mut tags = Vec::new();
    for (hi, h) in halves.iter().enumerate() {
        let on: Vec<usize> = (0..verts.len())
            .filter(|&v| (h.n.dot(&verts[v].coords) - h.c).abs() <= 10.0 * eps)
            .collect();
        if on.len() < 3 {
            continue;
        }
        let mid =
            P3::from(on.iter().fold(V3::zeros(), |a, &v| a + verts[v].coords) / on.len() as f64);
        let fr = Frame::from_z(mid, h.n);
        let mut ring: Vec<(f64, usize)> = on
            .iter()
            .map(|&v| {
                let w = verts[v] - mid;
                (w.dot(&fr.y).atan2(w.dot(&fr.x)), v)
            })
            .collect();
        ring.sort_by(|a, b| a.0.total_cmp(&b.0));
        let surf = Surface::Plane { f: fr };
        let mut lp = Vec::with_capacity(ring.len());
        for k in 0..ring.len() {
            let (a, z) = (ring[k].1, ring[(k + 1) % ring.len()].1);
            let key = (a.min(z), a.max(z));
            let e = *edge_of.entry(key).or_insert_with(|| {
                let (pa, pz) = (verts[key.0], verts[key.1]);
                out.edge(
                    Curve::Line { o: pa, d: pz - pa },
                    0.0,
                    1.0,
                    vid[key.0],
                    vid[key.1],
                )
            });
            let (pa, pz) = (verts[key.0], verts[key.1]);
            let line = Curve::Line { o: pa, d: pz - pa };
            lp.push(Coedge {
                edge: e,
                fwd: a == key.0,
                pc: super::pcurve::pcurve(&surf, &line, 0.0, 1.0, None),
            });
        }
        faces.push(out.face(surf, true, vec![lp]));
        tags.push(hi);
    }
    let shell = out.shell(faces, true);
    out.solid(vec![shell]);
    let body = out.build().ok()?;
    (body.check().is_empty() && super::mass::body_volume(&body) > 0.0).then_some((body, tags))
}

#[cfg(test)]
mod tests {
    use crate::native::kernel::NativeKernel;
    use crate::*;
    use nalgebra::Point3;

    /// The body's volume, area and face count.
    fn measures(k: &NativeKernel, id: BodyId) -> (f64, f64, usize) {
        let m = k.mass_properties(id).unwrap();
        (m.volume, m.surface_area, k.faces(id).unwrap().len())
    }

    /// The same chamfer by the polytope and by the general route (local trimming, the blend
    /// tool, flat corners), which agree to 1e-9.
    fn agrees(make: &dyn Fn(&mut NativeKernel) -> BodyId, d: f64, corners: ChamferCorners) {
        let opts = ChamferOpts {
            corners,
            ..ChamferOpts::new(ChamferSpec::EqualDistance(d))
        };
        let mut k = NativeKernel::new();
        let a = make(&mut k);
        let edges: Vec<EdgeId> = k.edges(a).unwrap().iter().map(|e| e.id).collect();
        let fast = k.chamfer_with(a, &edges, &opts).unwrap().bodies[0];
        let ids: Vec<usize> = edges.iter().map(|e| e.0 as usize).collect();
        let flat = corners == ChamferCorners::Flat;
        assert!(
            super::chamfer(&mut k, a, &ids, d, flat).is_some(),
            "not taken"
        );
        let general = super::super::blend::chamfer(&mut k, a, &edges, &opts).unwrap();
        let general = if flat {
            super::super::blend::flat_corners(&mut k, a, &edges, &opts, general).unwrap()
        } else {
            general
        };
        let (f, g) = (measures(&k, fast), measures(&k, general.bodies[0]));
        assert!((f.0 - g.0).abs() < 1e-9 * g.0, "volumes {f:?} {g:?}");
        assert!((f.1 - g.1).abs() < 1e-9 * g.1, "areas {f:?} {g:?}");
        assert_eq!(f.2, g.2, "face counts");
    }

    fn cube(k: &mut NativeKernel) -> BodyId {
        k.primitive(&Primitive::Box {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(10.0, 10.0, 10.0),
        })
        .unwrap()
        .bodies[0]
    }

    #[test]
    fn a_cubes_chamfers_agree_with_the_general_route() {
        agrees(&cube, 2.0, ChamferCorners::Flat);
        agrees(&cube, 2.0, ChamferCorners::Miter);
    }

    #[test]
    fn an_octahedrons_flat_chamfers_agree_with_the_general_route() {
        // Two pyramids on the square |x| + |y| ≤ 10, tips at z = ±10.
        let octahedron = |k: &mut NativeKernel| {
            let pyramid = |k: &mut NativeKernel, tip: f64| {
                let square = Profile::of(
                    Plane::xy(0.0),
                    vec![Loop::polygon(&[
                        [10.0, 0.0],
                        [0.0, 10.0],
                        [-10.0, 0.0],
                        [0.0, -10.0],
                    ])],
                );
                let spec = LoftSpec {
                    body: BodyKind::Solid,
                    sections: vec![
                        LoftSection::Profile(square),
                        LoftSection::Point(Point3::new(0.0, 0.0, tip)),
                    ],
                    start: LoftEnd::default(),
                    end: LoftEnd::default(),
                    source: 1,
                };
                k.loft_with(&spec).unwrap().bodies[0]
            };
            let (up, down) = (pyramid(k, 10.0), pyramid(k, -10.0));
            k.boolean(BoolOp::Union, up, &[down]).unwrap().bodies[0]
        };
        agrees(&octahedron, 6f64.sqrt(), ChamferCorners::Flat);
    }
}
