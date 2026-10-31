//! Split, face split, face tools and point queries built on the boolean machinery.

use super::boolean::{BoolOut, boolean_bodies, imprint};
use super::curve::Curve;
use super::features::Tagged;
use super::kernel::{NativeKernel, subset};
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, Face, Shell, Solid};
use crate::*;

/// A plane as a large square sheet body (one face) through `o` with normal `n`, `size` across.
pub fn plane_sheet(o: P3, n: V3, size: f64) -> Body {
    let f = Frame::from_z(o, n);
    let mut b = Body::default();
    let c = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let vs: Vec<usize> = c
        .iter()
        .map(|(x, y)| b.add_vertex(f.o + f.x * (x * size) + f.y * (y * size)))
        .collect();
    let mut lp = Vec::new();
    for i in 0..4 {
        let (a, bb) = (b.verts[vs[i]].p, b.verts[vs[(i + 1) % 4]].p);
        let (line, len) = Curve::line(a, bb);
        let e = b.add_edge(line.clone(), 0.0, len, vs[i], vs[(i + 1) % 4]);
        let pc = super::pcurve::pcurve(&Surface::Plane { f }, &line, 0.0, len, None);
        lp.push(Coedge {
            edge: e,
            fwd: true,
            pc,
        });
    }
    b.faces.push(Face {
        surf: Surface::Plane { f },
        sense: true,
        loops: vec![lp],
    });
    b.shells.push(Shell {
        faces: vec![0],
        closed: false,
    });
    b
}

/// The plane of a split tool.
fn tool_plane(k: &NativeKernel, tool: &SplitTool) -> Result<(P3, V3)> {
    match tool {
        SplitTool::Plane(p) => Ok((p.origin, p.normal.into_inner())),
        SplitTool::Face { body, face } => {
            let b = k.get(*body)?;
            let f = b
                .faces
                .get(face.0 as usize)
                .ok_or_else(|| KernelError::InvalidParameter(format!("unknown {face:?}")))?;
            match &f.surf {
                Surface::Plane { f: fr } => Ok((fr.o, fr.x.cross(&fr.y))),
                _ => Err(KernelError::Unsupported("splitting by a curved face")),
            }
        }
        SplitTool::Body(_) => Err(KernelError::Unsupported("splitting by a surface body")),
        SplitTool::Curves(_) => Err(KernelError::Unsupported("splitting parts by curves")),
    }
}

/// A box on one side of a plane, covering `bb`.
fn half_space_box(o: P3, n: V3, bb: &Bbox) -> Result<Body> {
    let size = bb.diagonal() * 2.0 + (bb.center() - o).norm() * 2.0 + 10.0;
    let f = Frame::from_z(o, n);
    let plane = Plane {
        origin: o,
        x_dir: nalgebra::Unit::new_normalize(f.x),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let p2 = |x: f64, y: f64| nalgebra::Point2::new(x, y);
    let l = |a, b| Curve2::Line { a, b, source: None };
    let sq = Region {
        outer: Loop {
            curves: vec![
                l(p2(-size, -size), p2(size, -size)),
                l(p2(size, -size), p2(size, size)),
                l(p2(size, size), p2(-size, size)),
                l(p2(-size, size), p2(-size, -size)),
            ],
        },
        holes: vec![],
        source: None,
    };
    let prof = Profile::new(plane, vec![sq]);
    let regions = super::profile::regions(&prof)?;
    let mf = super::profile::merge_regions(&regions)?;
    let kind = super::sweep::Prism { plane, dir: n };
    Ok(super::sweep::sweep(&plane, &mf, &kind, 0.0, size, false)?.body)
}

pub fn split(
    k: &mut NativeKernel,
    body: BodyId,
    tool: &SplitTool,
    source: u64,
) -> Result<OpResult> {
    let (o, n) = tool_plane(k, tool)?;
    let a = (**k.get(body)?).clone();
    let bb = a.bbox();
    let hs = half_space_box(o, n, &bb)?;
    // The two sides side by side (each a boolean of the part with the half space).
    let (inn, out) = rayon::join(
        || boolean_bodies(&a, &hs, BoolOp::Intersect),
        || boolean_bodies(&a, &hs, BoolOp::Subtract),
    );
    let (Ok(inn), Ok(out)) = (inn, out) else {
        return Err(KernelError::InvalidParameter(
            "The split tool doesn't cross the part".into(),
        ));
    };
    // A face isn't extended: the cut must lie within it.
    if let SplitTool::Face { body: tb, face } = tool {
        let t = k.get(*tb)?;
        let tf = &t.faces[face.0 as usize];
        let polys = super::classify::face_polygons(t, tf);
        for r in [&inn, &out] {
            for (i, srcs) in r.from.iter().enumerate() {
                if !srcs.iter().any(|&(owner, _)| owner == 1) {
                    continue;
                }
                for c in r.body.faces[i].loops.iter().flatten() {
                    let e = &r.body.edges[c.edge];
                    for q in [e.point(e.t[0]), e.point(e.mid_t())] {
                        let (u, v) = tf.surf.project(&q, None);
                        if super::classify::in_face(&polys, &tf.surf, P2::new(u, v), tol::POINT)
                            == super::classify::InFace::Outside
                        {
                            return Err(KernelError::InvalidParameter(
                                "The split face doesn't reach across the part".into(),
                            ));
                        }
                    }
                }
            }
        }
    }
    let mut result = Body::default();
    let mut history = History::default();
    for r in [&inn, &out] {
        let (_, _, fo) = result.append(&r.body);
        for (i, srcs) in r.from.iter().enumerate() {
            for &(owner, f) in srcs {
                if owner == 0 {
                    history.modified.push((
                        FaceId((fo + i) as u64),
                        InputFace {
                            body,
                            face: FaceId(f as u64),
                        },
                    ));
                } else {
                    history
                        .generated
                        .push((FaceId((fo + i) as u64), Origin::StartCap { region: source }));
                }
            }
        }
    }
    let id = k.insert(result);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

pub fn split_faces(
    k: &mut NativeKernel,
    body: BodyId,
    faces: &[FaceId],
    tool: &SplitTool,
) -> Result<OpResult> {
    if let SplitTool::Curves(curves) = tool {
        return super::curve_split::split_faces_by_curves(k, body, faces, curves);
    }
    let (o, n) = tool_plane(k, tool)?;
    let a = (**k.get(body)?).clone();
    let bb = a.bbox();
    let sheet = plane_sheet(
        o,
        n,
        bb.diagonal() * 2.0 + (bb.center() - o).norm() * 2.0 + 10.0,
    );
    let list: Vec<usize> = faces.iter().map(|f| f.0 as usize).collect();
    let r: BoolOut = imprint(&a, &sheet, Some(list.clone()))?;
    if r.body.faces.len() == a.faces.len() {
        return Err(KernelError::InvalidParameter(
            "The split tool doesn't cross the faces".into(),
        ));
    }
    let mut history = History::default();
    for (i, srcs) in r.from.iter().enumerate() {
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
    let id = k.insert(r.body);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// The solid bounded by the picked faces, capped flat across their free edges.
pub fn face_tool(
    k: &mut NativeKernel,
    body: BodyId,
    faces: &[FaceId],
    source: u64,
) -> Result<OpResult> {
    let a = k.get(body)?.clone();
    let list: Vec<usize> = faces.iter().map(|f| f.0 as usize).collect();
    let mut b = subset(&a, &list, false);
    // Free coedges: edges used once.
    let ef = b.edge_faces();
    let mut free: Vec<(usize, bool)> = Vec::new();
    for f in &b.faces {
        for l in &f.loops {
            for c in l {
                if ef[c.edge].len() == 1 && !b.edges[c.edge].degenerate {
                    free.push((c.edge, c.fwd));
                }
            }
        }
    }
    // Chain the free coedges reversed (the caps run the other way) into loops.
    let mut used = vec![false; free.len()];
    let mut caps: Vec<Vec<(usize, bool)>> = Vec::new();
    for s in 0..free.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let rev = |(e, f): (usize, bool)| (e, !f);
        let mut lp = vec![rev(free[s])];
        let start = {
            let (e, f) = lp[0];
            if f { b.edges[e].v[0] } else { b.edges[e].v[1] }
        };
        while let Some(&(e, f)) = lp.last() {
            let end = if f { b.edges[e].v[1] } else { b.edges[e].v[0] };
            if end == start {
                break;
            }
            let nx = (0..free.len()).find(|&k| {
                if used[k] {
                    return false;
                }
                let (e2, f2) = rev(free[k]);
                let st = if f2 {
                    b.edges[e2].v[0]
                } else {
                    b.edges[e2].v[1]
                };
                st == end
            });
            let Some(nx) = nx else {
                return Err(KernelError::InvalidParameter(
                    "The faces' open edges don't form closed loops".into(),
                ));
            };
            used[nx] = true;
            lp.push(rev(free[nx]));
        }
        caps.push(lp);
    }
    // A planar cap for each loop.
    let mut cap_faces = Vec::new();
    for lp in caps {
        // The plane through the loop's points.
        let mut pts = Vec::new();
        for &(e, _) in &lp {
            let ed = &b.edges[e];
            for i in 0..8 {
                pts.push(ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / 8.0));
            }
        }
        let c = pts.iter().fold(V3::zeros(), |s, p| s + p.coords) / pts.len() as f64;
        let mut n = V3::zeros();
        for i in 0..pts.len() {
            let (p, q) = (pts[i].coords - c, pts[(i + 1) % pts.len()].coords - c);
            n += p.cross(&q);
        }
        if n.norm() < tol::DEGENERATE {
            return Err(KernelError::InvalidParameter(
                "An open loop of the faces isn't planar".into(),
            ));
        }
        let n = n.normalize();
        if pts
            .iter()
            .any(|p| (p.coords - c).dot(&n).abs() > tol::POINT)
        {
            return Err(KernelError::InvalidParameter(
                "An open loop of the faces isn't planar".into(),
            ));
        }
        let fr = Frame::from_z(P3::from(c), n);
        let surf = Surface::Plane { f: fr };
        let coedges: Vec<Coedge> = lp
            .iter()
            .map(|&(e, fwd)| {
                let ed = &b.edges[e];
                Coedge {
                    edge: e,
                    fwd,
                    pc: super::pcurve::pcurve(&surf, &ed.curve, ed.t[0], ed.t[1], None),
                }
            })
            .collect();
        let mut face = Face {
            surf,
            sense: true,
            loops: vec![coedges],
        };
        let area = super::build::loop_uv_area(&b, &face.loops[0]);
        face.sense = area > 0.0;
        cap_faces.push(face);
    }
    let n_picked = b.faces.len();
    b.faces.extend(cap_faces);
    b.shells = vec![Shell {
        faces: (0..b.faces.len()).collect(),
        closed: true,
    }];
    b.solids = vec![Solid { shells: vec![0] }];
    let mut v = super::mass::body_volume(&b);
    if v < 0.0 {
        for f in &mut b.faces {
            Body::reverse_face(f);
        }
        v = -v;
    }
    if v < tol::ROUNDOFF {
        return Err(KernelError::InvalidParameter(
            "The faces enclose no volume".into(),
        ));
    }
    let mut t = Tagged {
        tags: vec![Vec::new(); b.faces.len()],
        body: b,
    };
    for f in n_picked..t.body.faces.len() {
        t.tags[f].push(Origin::StartCap { region: source });
    }
    let mut r = t.into_result(k);
    for (i, f) in list.iter().enumerate() {
        r.history.modified.push((
            FaceId(i as u64),
            InputFace {
                body,
                face: FaceId(*f as u64),
            },
        ));
    }
    Ok(r)
}

/// The exact outward normals of a face at the points nearest the given ones.
pub fn face_normals_at(
    k: &NativeKernel,
    body: BodyId,
    face: FaceId,
    points: &[P3],
) -> Result<Vec<V3>> {
    let b = k.get(body)?;
    let f = b
        .faces
        .get(face.0 as usize)
        .ok_or_else(|| KernelError::InvalidParameter(format!("unknown {face:?}")))?;
    let polys = super::classify::face_polygons(b, f);
    let center = super::boolean::interior_of_polys(&polys).unwrap_or(P2::origin());
    Ok(points
        .iter()
        .map(|p| {
            // From the face's interior, and (a polar patch can lead Newton astray) from the
            // nearest point of a coarse grid when that lands closer.
            let mut uv = f.surf.project(p, Some((center.x, center.y)));
            if (f.surf.eval(uv.0, uv.1) - p).norm() > tol::ROUNDOFF {
                let w = f.surf.project(p, None);
                if (f.surf.eval(w.0, w.1) - p).norm() < (f.surf.eval(uv.0, uv.1) - p).norm() {
                    uv = w;
                }
            }
            let (u, v) = uv;
            f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 }
        })
        .collect())
}
