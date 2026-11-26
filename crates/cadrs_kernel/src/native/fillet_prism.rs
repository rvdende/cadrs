//! A prism's top rim filleted whole, in closed form.
//!
//! A prism here is a planar top face bounded by one smooth loop of lines and arcs, a wall
//! square to it under each (a plane under a line, a cylinder about the arc's axis under an
//! arc), and a planar bottom: an extruded profile whose corners are rounded (a box with its
//! upright edges filleted). Every edge of the top loop rounded by one radius r is then the top
//! face's loop moved in by r, each wall cut down by r, and between them a cylinder of radius
//! r along each line and a torus about each arc's axis, the pieces meeting in the quarter
//! circles at the loop's vertices (the loop is smooth: no corner patches). The result is
//! checked (the fast check) and its volume held to the removed rim's by Pappus's theorem.

use std::f64::consts::{FRAC_PI_2, SQRT_2};

use super::blend::FaceSrc;
use super::curve::Curve;
use super::math::*;
use super::pcurve::pcurve;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, CheckLevel, Coedge, Edge, Face, TOL_MIN, Vertex};
use crate::{BodyId, EdgeId, Origin};

/// The outward unit normal of face `fi` of `b` at `p` (a point on it).
fn outward(b: &Body, fi: usize, p: &P3) -> V3 {
    let f = &b.faces[fi];
    let (u, v) = f.surf.project(p, None);
    f.surf.normal(u, v).normalize() * if f.sense { 1.0 } else { -1.0 }
}

/// One edge of the top loop: its coedge on the top face, the wall under it, and the curves the
/// round makes of it.
struct Side {
    edge: usize,
    /// Whether the top face's loop runs along the edge.
    fwd: bool,
    wall: usize,
    /// The edge moved in by r on the top face, and down by r on the wall.
    top: Curve,
    low: Curve,
    /// The round's surface and its sense.
    surf: Surface,
    sense: bool,
    /// Its start and end vertices along the loop.
    from: usize,
    to: usize,
    /// The volume the round takes off along it.
    removed: f64,
}

/// [`super::fillet`]'s every edge of the top loop of prism `b` (`edges`, all of them) rounded
/// by `r`, built directly, with each face's origin; None for any other body, or a result that
/// fails its checks.
pub(crate) fn prism_rim(
    b: &Body,
    input: BodyId,
    edges: &[usize],
    r: f64,
) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
    if b.solids.len() != 1 || b.shells.len() != 1 || !(r > 0.0) {
        return None;
    }
    let size = b.bbox().diagonal();
    let near = 1e-9 * (1.0 + size);
    let mut chosen: Vec<usize> = edges.to_vec();
    chosen.sort_unstable();
    chosen.dedup();
    // The top face: one loop, made of exactly the edges.
    let top = (0..b.faces.len()).find(|&fi| {
        let f = &b.faces[fi];
        let mut es: Vec<usize> = f.loops.iter().flatten().map(|c| c.edge).collect();
        es.sort_unstable();
        f.loops.len() == 1 && es == chosen
    })?;
    let ft = &b.faces[top];
    let Surface::Plane { .. } = ft.surf else {
        return None;
    };
    let lp = &ft.loops[0];
    if b.faces.len() != lp.len() + 2 {
        return None;
    }
    let first = b.edges[lp[0].edge].point(b.edges[lp[0].edge].mid_t());
    let n = outward(b, top, &first);
    let start = |c: &Coedge| {
        let e = &b.edges[c.edge];
        if c.fwd { e.v[0] } else { e.v[1] }
    };
    // Each edge's wall, the curves and surface of its round.
    let k_c = (10.0 - 3.0 * std::f64::consts::PI) / (3.0 * (4.0 - std::f64::consts::PI));
    let area = r * r * (1.0 - std::f64::consts::FRAC_PI_4);
    let mut sides: Vec<Side> = Vec::with_capacity(lp.len());
    for (k, c) in lp.iter().enumerate() {
        let e = &b.edges[c.edge];
        if e.degenerate || e.v[0] == e.v[1] {
            return None;
        }
        let walls: Vec<usize> = (0..b.faces.len())
            .filter(|&fi| fi != top && b.faces[fi].loops.iter().flatten().any(|x| x.edge == c.edge))
            .collect();
        let [wall] = walls[..] else { return None };
        if b.faces[wall].loops.len() != 1 {
            return None;
        }
        let mid = e.point(e.mid_t());
        // Inward on the top face, square to the edge: against the wall's outward normal.
        let m = -outward(b, wall, &mid);
        if m.dot(&n).abs() > tol::ANGULAR {
            return None;
        }
        let (top_c, low_c, surf, removed) = match (&e.curve, &b.faces[wall].surf) {
            (Curve::Line { o, d }, Surface::Plane { .. }) => {
                let axis = d.normalize();
                let len = d.norm() * (e.t[1] - e.t[0]);
                let f = Frame::from_xz(o + m * r - n * r, (m - n) / SQRT_2, axis);
                (
                    Curve::Line {
                        o: o + m * r,
                        d: *d,
                    },
                    Curve::Line {
                        o: o - n * r,
                        d: *d,
                    },
                    Surface::Cylinder { f, r },
                    area * len,
                )
            }
            (Curve::Circle { c: o, x, y, r: big }, Surface::Cylinder { f: cf, r: cr }) => {
                // The wall about the circle's axis, the circle in the top face.
                let w = x.cross(y);
                let off = o - cf.o;
                if w.cross(&n).norm() > tol::ANGULAR
                    || cf.z.cross(&n).norm() > tol::ANGULAR
                    || (off - cf.z * off.dot(&cf.z)).norm() > near
                    || (big - cr).abs() > near
                {
                    return None;
                }
                // Inward toward the axis (a convex round) or away from it.
                let convex = m.dot(&(o - mid)) > 0.0;
                let r2 = if convex { big - r } else { big + r };
                if r2 <= 10.0 * tol::POINT {
                    return None;
                }
                let sweep = e.t[1] - e.t[0];
                let path = if convex { big - k_c * r } else { big + k_c * r };
                let away = (o - mid).normalize();
                let f = Frame::from_xz(o - n * r, away, n);
                (
                    Curve::Circle {
                        c: *o,
                        x: *x,
                        y: *y,
                        r: r2,
                    },
                    Curve::Circle {
                        c: o - n * r,
                        x: *x,
                        y: *y,
                        r: *big,
                    },
                    Surface::Torus {
                        f,
                        rmaj: r2,
                        rmin: r,
                    },
                    area * sweep * path,
                )
            }
            _ => return None,
        };
        // The round's sense: its normal out of the tube's centre at the quarter's middle.
        let q = mid + m * r - n * r;
        let p_out = q + (n - m) / SQRT_2 * r;
        let (u, v) = surf.project(&p_out, None);
        let sense = surf.normal(u, v).dot(&(n - m)) > 0.0;
        let next = &lp[(k + 1) % lp.len()];
        sides.push(Side {
            edge: c.edge,
            fwd: c.fwd,
            wall,
            top: top_c,
            low: low_c,
            surf,
            sense,
            from: start(c),
            to: start(next),
            removed,
        });
    }
    // The loop smooth at every vertex, and under each vertex one upright edge deeper than r.
    let tangent = |c: &Coedge, at_end: bool| {
        let e = &b.edges[c.edge];
        let t = if at_end == c.fwd { e.t[1] } else { e.t[0] };
        e.curve.tangent(t).normalize() * if c.fwd { 1.0 } else { -1.0 }
    };
    let mut upright: Vec<(usize, usize)> = Vec::with_capacity(lp.len());
    for (k, c) in lp.iter().enumerate() {
        let next = &lp[(k + 1) % lp.len()];
        if tangent(c, true).cross(&tangent(next, false)).norm() > tol::ANGULAR
            || tangent(c, true).dot(&tangent(next, false)) <= 0.0
        {
            return None;
        }
        let v = start(next);
        let ups: Vec<usize> = (0..b.edges.len())
            .filter(|&ei| !chosen.contains(&ei) && b.edges[ei].v.contains(&v))
            .collect();
        let [up] = ups[..] else { return None };
        let eu = &b.edges[up];
        let Curve::Line { d, .. } = eu.curve else {
            return None;
        };
        let w = if eu.v[0] == v { eu.v[1] } else { eu.v[0] };
        if d.normalize().cross(&n).norm() > tol::ANGULAR
            || (b.verts[w].p - b.verts[v].p).dot(&n) > -(r + 1e-6 * (1.0 + size))
        {
            return None;
        }
        upright.push((v, up));
    }
    // The bottom: the one face left, a plane facing away from the top.
    let bottom = (0..b.faces.len()).find(|&fi| fi != top && sides.iter().all(|s| s.wall != fi))?;
    let fb = &b.faces[bottom];
    if !matches!(fb.surf, Surface::Plane { .. }) {
        return None;
    }
    let pb = b.verts[b.edges[fb.loops.first()?.first()?.edge].v[0]].p;
    if outward(b, bottom, &pb).dot(&n) > -1.0 + tol::ANGULAR {
        return None;
    }

    let mut nb = b.clone();
    // Each loop vertex: the old one moved down the wall, a new one in on the top face, and the
    // quarter circle between them.
    let mut on_top = std::collections::HashMap::new();
    let mut quarter = std::collections::HashMap::new();
    for &(v, up) in &upright {
        let p = b.verts[v].p;
        let side = sides.iter().find(|s| s.from == v)?;
        let m = -outward(b, side.wall, &p);
        let (p_low, p_top) = (p - n * r, p + m * r);
        nb.verts[v].p = p_low;
        nb.verts.push(Vertex {
            p: p_top,
            tol: TOL_MIN,
        });
        let vt = nb.verts.len() - 1;
        on_top.insert(v, vt);
        // The upright edge now starts at the moved vertex.
        let eu = &mut nb.edges[up];
        let Curve::Line { o, d } = eu.curve else {
            return None;
        };
        let t = (p_low - o).dot(&d) / d.norm_squared();
        let slot = if eu.v[0] == v { 0 } else { 1 };
        eu.t[slot] = t;
        if !(eu.t[0] < eu.t[1]) {
            return None;
        }
        nb.edges.push(Edge {
            curve: Curve::Circle {
                c: p + m * r - n * r,
                x: n,
                y: -m,
                r,
            },
            t: [0.0, FRAC_PI_2],
            v: [vt, v],
            degenerate: false,
            tol: TOL_MIN,
        });
        quarter.insert(v, nb.edges.len() - 1);
    }
    let mut src: Vec<Vec<FaceSrc>> = (0..b.faces.len())
        .map(|i| vec![FaceSrc::Input(i)])
        .collect();
    let mut removed = 0.0;
    for s in &sides {
        let t = b.edges[s.edge].t;
        let [v0, v1] = b.edges[s.edge].v;
        // The edge itself moved in on the top face; a new one down the wall.
        nb.edges[s.edge] = Edge {
            curve: s.top.clone(),
            t,
            v: [on_top[&v0], on_top[&v1]],
            degenerate: false,
            tol: TOL_MIN,
        };
        nb.edges.push(Edge {
            curve: s.low.clone(),
            t,
            v: [v0, v1],
            degenerate: false,
            tol: TOL_MIN,
        });
        let low = nb.edges.len() - 1;
        let tsurf = nb.faces[top].surf.clone();
        for co in nb.faces[top].loops.iter_mut().flatten() {
            if co.edge == s.edge {
                co.pc = pcurve(&tsurf, &s.top, t[0], t[1], None);
            }
        }
        let wsurf = nb.faces[s.wall].surf.clone();
        for co in nb.faces[s.wall].loops.iter_mut().flatten() {
            if co.edge == s.edge {
                // (In the turn of the wall's parameters the old p-curve was in.)
                let was = co.pc.point(0.5 * (t[0] + t[1]));
                co.edge = low;
                co.pc = pcurve(&wsurf, &s.low, t[0], t[1], Some(P2::new(was.x, was.y)));
            }
        }
        // The round: back along the moved edge, down the start's quarter, along the lowered
        // edge, up the end's quarter. Its p-curves near the quarter's middle (clear of seams).
        let (qa, qb) = (quarter[&s.from], quarter[&s.to]);
        let mid = {
            let e = &b.edges[s.edge];
            let p = e.point(e.mid_t());
            let m = -outward(b, s.wall, &p);
            p + m * r - n * r + (n - m) / SQRT_2 * r
        };
        let (u, v) = s.surf.project(&mid, None);
        let hint = Some(P2::new(u, v));
        let co = |edge: usize, curve: &Curve, t: [f64; 2], fwd: bool| Coedge {
            edge,
            fwd,
            pc: pcurve(&s.surf, curve, t[0], t[1], hint),
        };
        let quarter_curve = |q: usize| nb.edges[q].curve.clone();
        let lp = vec![
            co(s.edge, &s.top, t, !s.fwd),
            co(qa, &quarter_curve(qa), [0.0, FRAC_PI_2], true),
            co(low, &s.low, t, s.fwd),
            co(qb, &quarter_curve(qb), [0.0, FRAC_PI_2], false),
        ];
        nb.faces.push(Face {
            surf: s.surf.clone(),
            sense: s.sense,
            loops: vec![lp],
        });
        let fi = nb.faces.len() - 1;
        nb.shells[0].faces.push(fi);
        src.push(vec![FaceSrc::New(Origin::FromEdge {
            body: input,
            edge: EdgeId(s.edge as u64),
        })]);
        removed += s.removed;
    }
    if !nb.check_with(CheckLevel::Fast).is_empty() {
        return None;
    }
    let (v0, v1) = (super::mass::body_volume(b), super::mass::body_volume(&nb));
    ((v0 - removed - v1).abs() <= 1e-9 * v0.abs().max(1.0)).then_some((nb, src))
}

/// A plane face's outward unit normal.
fn plane_normal(b: &Body, fi: usize) -> Option<V3> {
    let f = &b.faces[fi];
    let Surface::Plane { f: fr } = &f.surf else {
        return None;
    };
    Some(fr.x.cross(&fr.y).normalize() * if f.sense { 1.0 } else { -1.0 })
}

/// [`super::fillet`]'s upright edges of a prism of planes (two caps, every other face a wall
/// square to them) rounded by `r`, built directly: at each such edge a cylinder of radius r
/// about the upright line `r / sin(φ/2)` in along the corner's bisector (φ the walls' angle),
/// touching each wall `r / tan(φ/2)` from the corner, and the caps' corners cut by its arcs.
/// Each corner takes `r²(cot(φ/2) − (π − φ)/2)` of the section, the volume checked against
/// that. None for any other body or edge (a concave corner, a round wider than its walls), or
/// a result that fails its checks.
pub(crate) fn prism_uprights(
    b: &Body,
    input: BodyId,
    edges: &[usize],
    r: f64,
) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
    if b.solids.len() != 1 || b.shells.len() != 1 || !(r > 0.0) || edges.is_empty() {
        return None;
    }
    if b.faces
        .iter()
        .any(|f| !matches!(f.surf, Surface::Plane { .. }) || f.loops.len() != 1)
    {
        return None;
    }
    let Curve::Line { d, .. } = b.edges[*edges.first()?].curve else {
        return None;
    };
    let axis = d.normalize();
    let normals: Vec<V3> = (0..b.faces.len())
        .map(|fi| plane_normal(b, fi))
        .collect::<Option<_>>()?;
    let caps: Vec<usize> = (0..b.faces.len())
        .filter(|&fi| normals[fi].dot(&axis).abs() > 1.0 - tol::ANGULAR)
        .collect();
    let [c0, c1] = caps[..] else { return None };
    let (top, bottom) = if normals[c0].dot(&axis) > 0.0 {
        (c0, c1)
    } else {
        (c1, c0)
    };
    if normals[bottom].dot(&axis) > 0.0
        || (0..b.faces.len())
            .any(|fi| fi != top && fi != bottom && normals[fi].dot(&axis).abs() > tol::ANGULAR)
    {
        return None;
    }
    let faces_of = |e: usize| -> Vec<usize> {
        (0..b.faces.len())
            .filter(|&fi| b.faces[fi].loops.iter().flatten().any(|c| c.edge == e))
            .collect()
    };
    // The coedge of a cap's loop that ends at vertex v, its index.
    let ending = |fi: usize, v: usize| {
        let lp = &b.faces[fi].loops[0];
        lp.iter().position(|c| {
            let e = &b.edges[c.edge];
            (if c.fwd { e.v[1] } else { e.v[0] }) == v
        })
    };
    let line_t = |c: &Curve, p: &P3| match c {
        Curve::Line { o, d } => Some((p - o).dot(d) / d.norm_squared()),
        _ => None,
    };
    let mut nb = b.clone();
    let mut src: Vec<Vec<FaceSrc>> = (0..b.faces.len())
        .map(|i| vec![FaceSrc::Input(i)])
        .collect();
    let mut removed = 0.0;
    let mut top_arcs: Vec<(usize, usize)> = Vec::new();
    let mut bottom_arcs: Vec<(usize, usize)> = Vec::new();
    for &e in edges {
        let ed = &b.edges[e];
        let Curve::Line { o, d } = ed.curve else {
            return None;
        };
        if ed.degenerate || d.normalize().cross(&axis).norm() > tol::ANGULAR {
            return None;
        }
        let [p0, p1] = [b.verts[ed.v[0]].p, b.verts[ed.v[1]].p];
        let (vt, vb) = if (p0 - p1).dot(&axis) > 0.0 {
            (ed.v[0], ed.v[1])
        } else {
            (ed.v[1], ed.v[0])
        };
        let (pt, pb) = (b.verts[vt].p, b.verts[vb].p);
        let h = (pt - pb).dot(&axis);
        // The walls in the top cap's order: w1's top edge comes in to the corner, w2's leaves.
        let it = ending(top, vt)?;
        let lt = &b.faces[top].loops[0];
        let (e1t, e2t) = (lt[it].edge, lt[(it + 1) % lt.len()].edge);
        let wall_of = |te: usize| faces_of(te).into_iter().find(|&fi| fi != top);
        let (w1, w2) = (wall_of(e1t)?, wall_of(e2t)?);
        let mut ws = faces_of(e);
        ws.sort_unstable();
        let mut want = vec![w1, w2];
        want.sort_unstable();
        if ws != want || w1 == w2 {
            return None;
        }
        // Their bottom edges: the bottom cap's loop runs the other way round.
        let ib = ending(bottom, vb)?;
        let lb = &b.faces[bottom].loops[0];
        let (e2b, e1b) = (lb[ib].edge, lb[(ib + 1) % lb.len()].edge);
        if wall_of_bottom(b, &faces_of, bottom, e1b) != Some(w1)
            || wall_of_bottom(b, &faces_of, bottom, e2b) != Some(w2)
        {
            return None;
        }
        // Along each wall away from the corner, and the angle between them.
        let away = |te: usize| {
            let te = &b.edges[te];
            let other = if te.v[0] == vt { te.v[1] } else { te.v[0] };
            (b.verts[other].p - pt).normalize()
        };
        let (t1, t2) = (away(e1t), away(e2t));
        let phi = t1.dot(&t2).clamp(-1.0, 1.0).acos();
        // Convex: along one wall away from the corner lies inside the other's half-space.
        if t1.dot(&normals[w2]) >= -tol::ANGULAR || phi < 1e-3 || phi > std::f64::consts::PI - 1e-3
        {
            return None;
        }
        let s = r / (phi / 2.0).tan();
        let bis = (t1 + t2).normalize();
        let ct = pt + bis * (r / (phi / 2.0).sin());
        let cb = ct - axis * h;
        let (a1, a2, b1, b2) = (pt + t1 * s, pt + t2 * s, pb + t1 * s, pb + t2 * s);
        // The corner's vertices moved to w1's side; new ones on w2's.
        nb.verts[vt].p = a1;
        nb.verts[vb].p = b1;
        nb.verts.push(Vertex {
            p: a2,
            tol: TOL_MIN,
        });
        let va2 = nb.verts.len() - 1;
        nb.verts.push(Vertex {
            p: b2,
            tol: TOL_MIN,
        });
        let vb2 = nb.verts.len() - 1;
        // The wall edges trimmed at the corner (w2's moved to its new vertices).
        for (we, from, to, newv) in [
            (e1t, vt, a1, vt),
            (e1b, vb, b1, vb),
            (e2t, vt, a2, va2),
            (e2b, vb, b2, vb2),
        ] {
            let edge = &mut nb.edges[we];
            let slot = if edge.v[0] == from {
                0
            } else if edge.v[1] == from {
                1
            } else {
                return None;
            };
            edge.t[slot] = line_t(&edge.curve, &to)?;
            edge.v[slot] = newv;
            if !(edge.t[1] - edge.t[0] > 1e-9 * (1.0 + size_of(b))) {
                return None;
            }
        }
        // The upright: w1's moved along w1, a new one along w2.
        let (s1, s2) = (t1 * s, t2 * s);
        nb.edges[e].curve = Curve::Line { o: o + s1, d };
        let map = |v: usize| if v == vt { va2 } else { vb2 };
        nb.edges.push(Edge {
            curve: Curve::Line { o: o + s2, d },
            t: ed.t,
            v: [map(ed.v[0]), map(ed.v[1])],
            degenerate: false,
            tol: TOL_MIN,
        });
        let e2 = nb.edges.len() - 1;
        for (w, edge) in [(w1, e), (w2, e2)] {
            let surf = nb.faces[w].surf.clone();
            let curve = nb.edges[edge].curve.clone();
            for co in nb.faces[w].loops.iter_mut().flatten() {
                if co.edge == e {
                    co.edge = edge;
                    co.pc = pcurve(&surf, &curve, ed.t[0], ed.t[1], None);
                }
            }
        }
        // The arcs, from w1's side to w2's.
        let alpha = std::f64::consts::PI - phi;
        let x = (a1 - ct) / r;
        let y = (((a2 - ct) / r - x * alpha.cos()) / alpha.sin()).normalize();
        let arc = |c: P3, v0: usize, v1: usize, nb: &mut Body| {
            nb.edges.push(Edge {
                curve: Curve::Circle { c, x, y, r },
                t: [0.0, alpha],
                v: [v0, v1],
                degenerate: false,
                tol: TOL_MIN,
            });
            nb.edges.len() - 1
        };
        let at = arc(ct, vt, va2, &mut nb);
        let ab = arc(cb, vb, vb2, &mut nb);
        top_arcs.push((e1t, at));
        bottom_arcs.push((e2b, ab));
        // The round.
        let f = Frame::from_xz(cb, -bis, axis);
        let surf = Surface::Cylinder { f, r };
        let mid = ct + (bis * -1.0) * r;
        let (u, v) = surf.project(&mid, None);
        let sense = surf.normal(u, v).dot(&(-bis)) > 0.0;
        let hint = Some(P2::new(u, v));
        let down = nb.edges[e].v[0] == vt;
        let mut lp: Vec<(usize, bool)> = vec![(at, false), (e, down), (ab, true), (e2, !down)];
        // (Against w1's use of its upright: else the loop the other way round.)
        let w1_fwd = nb.faces[w1]
            .loops
            .iter()
            .flatten()
            .find(|c| c.edge == e)?
            .fwd;
        if w1_fwd == down {
            lp.reverse();
            for c in lp.iter_mut() {
                c.1 = !c.1;
            }
        }
        let lp: Vec<Coedge> = lp
            .into_iter()
            .map(|(edge, fwd)| {
                let ee = &nb.edges[edge];
                Coedge {
                    edge,
                    fwd,
                    pc: pcurve(&surf, &ee.curve, ee.t[0], ee.t[1], hint),
                }
            })
            .collect();
        nb.faces.push(Face {
            surf,
            sense,
            loops: vec![lp],
        });
        let fi = nb.faces.len() - 1;
        nb.shells[0].faces.push(fi);
        src.push(vec![FaceSrc::New(Origin::FromEdge {
            body: input,
            edge: EdgeId(e as u64),
        })]);
        removed += (r * s - alpha / 2.0 * r * r) * h;
    }
    // The caps' loops take each arc after the wall edge it follows.
    for (cap, arcs, fwd) in [(top, &top_arcs, true), (bottom, &bottom_arcs, false)] {
        let surf = nb.faces[cap].surf.clone();
        for &(after, arc) in arcs {
            let lp = &mut nb.faces[cap].loops[0];
            let i = lp.iter().position(|c| c.edge == after)?;
            let ee = &nb.edges[arc];
            let pc = pcurve(&surf, &ee.curve, ee.t[0], ee.t[1], None);
            lp.insert(i + 1, Coedge { edge: arc, fwd, pc });
        }
    }
    if !nb.check_with(CheckLevel::Fast).is_empty() {
        return None;
    }
    let (v0, v1) = (super::mass::body_volume(b), super::mass::body_volume(&nb));
    ((v0 - removed - v1).abs() <= 1e-9 * v0.abs().max(1.0)).then_some((nb, src))
}

fn size_of(b: &Body) -> f64 {
    b.bbox().diagonal()
}

/// The wall (the face besides the bottom cap) of bottom edge `e`.
fn wall_of_bottom(
    _b: &Body,
    faces_of: &dyn Fn(usize) -> Vec<usize>,
    bottom: usize,
    e: usize,
) -> Option<usize> {
    faces_of(e).into_iter().find(|&fi| fi != bottom)
}

/// [`super::fillet`]'s whole circles `edges` of `b`, each between a plane square to its axis
/// and a cylinder about it (a rim: a disc's edge, a hole's mouth, a boss's foot), rounded by
/// `r`, built directly: each face cut back by r from the rim (the plane's circle moved in or
/// out, the cylinder's along its axis), and between them a quarter of the torus about the
/// axis whose tube is the ball rolled round the rim. Convex rims lose and concave ones gain
/// `r²(1 − π/4)` of section along the circle its centroid follows (Pappus), which the result's
/// volume is held to. None for any other edge, or a result that fails its checks.
pub(crate) fn rims(
    b: &Body,
    input: BodyId,
    edges: &[usize],
    r: f64,
) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
    use std::f64::consts::{FRAC_PI_4, PI, TAU};
    if b.solids.len() != 1 || b.shells.len() != 1 || !(r > 0.0) || edges.is_empty() {
        return None;
    }
    let k_c = (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let area = r * r * (1.0 - FRAC_PI_4);
    let mut nb = b.clone();
    let mut src: Vec<Vec<FaceSrc>> = (0..b.faces.len())
        .map(|i| vec![FaceSrc::Input(i)])
        .collect();
    let mut change = 0.0;
    for &e in edges {
        let ed = &b.edges[e];
        let Curve::Circle {
            c: oc,
            x: cx,
            y: cy,
            r: big,
        } = ed.curve
        else {
            return None;
        };
        if !ed.closed() || (ed.t[1] - ed.t[0] - TAU).abs() > 1e-9 {
            return None;
        }
        let axis = cx.cross(&cy).normalize();
        // Its two faces: a plane square to the axis and a cylinder about it (one may have
        // another rim: a cylinder's two ends, edited one after the other).
        let fs: Vec<(usize, bool)> = (0..b.faces.len())
            .flat_map(|fi| {
                b.faces[fi]
                    .loops
                    .iter()
                    .flatten()
                    .filter(move |c| c.edge == e)
                    .map(move |c| (fi, c.fwd))
            })
            .collect();
        let [(fa, sa), (fb, sb)] = fs[..] else {
            return None;
        };
        if fa == fb {
            return None;
        }
        let kind_ok = |fi: usize| match &b.faces[fi].surf {
            Surface::Plane { f } => f.x.cross(&f.y).normalize().cross(&axis).norm() < tol::ANGULAR,
            Surface::Cylinder { f, r: cr } => {
                let off = oc - f.o;
                f.z.cross(&axis).norm() < tol::ANGULAR
                    && (off - f.z * off.dot(&f.z)).norm() < 1e-9 * (1.0 + big)
                    && (cr - big).abs() < 1e-9 * (1.0 + big)
            }
            _ => false,
        };
        let is_plane = |fi: usize| matches!(b.faces[fi].surf, Surface::Plane { .. });
        if !kind_ok(fa) || !kind_ok(fb) || is_plane(fa) == is_plane(fb) {
            return None;
        }
        // At the rim's vertex: each face's outward normal, and the way into it from the rim.
        let t0 = ed.t[0];
        let p = ed.point(t0);
        let tangent = ed.curve.tangent(t0).normalize();
        let n_of = |fi: usize| outward(b, fi, &p);
        let (na, nb_) = (n_of(fa), n_of(fb));
        let into = |n: V3, fwd: bool| n.cross(&(tangent * if fwd { 1.0 } else { -1.0 }));
        let (ma, mb) = (into(na, sa), into(nb_, sb));
        // Convex: into each face runs against the other's outward normal.
        let convex = ma.dot(&nb_) < 0.0;
        if ma.dot(&nb_).abs() < 1.0 - 1e-6 || mb.dot(&na).abs() < 1.0 - 1e-6 {
            return None;
        }
        // The tube's centre, the two new circles' points, at the rim's vertex.
        let centre = p + (ma + mb) * r;
        let (pa, pb) = (p + ma * r, p + mb * r);
        let radial = |q: P3| {
            let w = q - oc;
            let h = w.dot(&axis);
            ((w - axis * h).norm(), h)
        };
        let (rc, hc) = radial(centre);
        if !(rc > r * (1.0 + 1e-9)) {
            return None;
        }
        let circle_at = |q: P3| {
            let (rq, hq) = radial(q);
            Curve::Circle {
                c: oc + axis * hq,
                x: cx,
                y: cy,
                r: rq,
            }
        };
        let (ca, cb) = (circle_at(pa), circle_at(pb));
        if let (Curve::Circle { r: ra, .. }, Curve::Circle { r: rb, .. }) = (&ca, &cb)
            && (*ra <= 10.0 * tol::POINT || *rb <= 10.0 * tol::POINT)
        {
            return None;
        }
        // Each face's other edges clear of its moved circle on the side the face lies: on the
        // plane past the new circle (the round would run off it: a boss near a plate's
        // corner), on the cylinder farther along it than r.
        let clear = |fi: usize, m: V3, q: P3| {
            let (rq, hq) = radial(q);
            b.faces[fi]
                .loops
                .iter()
                .flatten()
                .filter(|c| c.edge != e)
                .all(|c| {
                    let oe = &b.edges[c.edge];
                    (0..=8).all(|i| {
                        let x = oe.point(oe.t[0] + (oe.t[1] - oe.t[0]) * i as f64 / 8.0);
                        let (rx, hx) = radial(x);
                        // (A point of the rim itself: a seam's end.)
                        let (r0, h0) = radial(p);
                        if (rx - r0).abs() + (hx - h0).abs() <= 1e-9 * (1.0 + big) {
                            return true;
                        }
                        if is_plane(fi) {
                            // Into the plane face toward or away from the axis.
                            let inward = m.dot(&(oc - p)) > 0.0;
                            if inward {
                                rx < rq - tol::POINT
                            } else {
                                rx > rq + tol::POINT
                            }
                        } else {
                            let up = m.dot(&axis) > 0.0;
                            if up {
                                hx > hq + tol::POINT
                            } else {
                                hx < hq - tol::POINT
                            }
                        }
                    })
                })
        };
        if !clear(fa, ma, pa) || !clear(fb, mb, pb) {
            return None;
        }
        // The torus: u from the rim's vertex round the axis, v the tube's angle (its x the
        // radial direction there, z the axis).
        let ex = (p - oc - axis * (p - oc).dot(&axis)).normalize();
        let tf = Frame {
            o: oc + axis * hc,
            x: ex,
            y: axis.cross(&ex),
            z: axis,
        };
        let surf = Surface::Torus {
            f: tf,
            rmaj: rc,
            rmin: r,
        };
        let angle = |q: P3| {
            let w = q - centre;
            w.dot(&axis).atan2(w.dot(&ex))
        };
        let (va, mut vb) = (angle(pa), angle(pb));
        // (The quarter between them, the short way.)
        if vb - va > PI {
            vb -= TAU;
        } else if va - vb > PI {
            vb += TAU;
        }
        // The rim's vertex: moved to b's side; a new one on a's.
        let v = ed.v[0];
        nb.verts[v].p = pb;
        nb.verts.push(Vertex {
            p: pa,
            tol: TOL_MIN,
        });
        let vnew = nb.verts.len() - 1;
        // Seams of the faces ending at the vertex: a cylinder's line, cut back along it.
        for (fi, vert) in [(fa, vnew), (fb, v)] {
            if is_plane(fi) {
                continue;
            }
            let to = nb.verts[vert].p;
            let seams: Vec<usize> = b.faces[fi]
                .loops
                .iter()
                .flatten()
                .map(|c| c.edge)
                .filter(|&x| x != e && b.edges[x].v.contains(&v))
                .collect();
            for s in seams {
                let se = &mut nb.edges[s];
                let Curve::Line { o, d } = se.curve else {
                    return None;
                };
                let slot = if se.v[0] == v { 0 } else { 1 };
                se.t[slot] = (to - o).dot(&d) / d.norm_squared();
                se.v[slot] = vert;
                if !(se.t[0] < se.t[1]) {
                    return None;
                }
            }
        }
        // The rim on a, and a new one on b.
        let t = ed.t;
        nb.edges[e] = Edge {
            curve: ca.clone(),
            t,
            v: [vnew, vnew],
            degenerate: false,
            tol: TOL_MIN,
        };
        nb.edges.push(Edge {
            curve: cb.clone(),
            t,
            v: [v, v],
            degenerate: false,
            tol: TOL_MIN,
        });
        let eb = nb.edges.len() - 1;
        for (fi, edge, curve) in [(fa, e, &ca), (fb, eb, &cb)] {
            let s = nb.faces[fi].surf.clone();
            for co in nb.faces[fi].loops.iter_mut().flatten() {
                if co.edge == e {
                    let was = co.pc.point(0.5 * (t[0] + t[1]));
                    co.edge = edge;
                    co.pc = pcurve(&s, curve, t[0], t[1], Some(P2::new(was.x, was.y)));
                }
            }
        }
        // The meridian at the vertex, from the lower tube angle to the higher.
        let (lo, hi, vlo, vhi) = if va < vb {
            (va, vb, vnew, v)
        } else {
            (vb, va, v, vnew)
        };
        nb.edges.push(Edge {
            curve: Curve::Circle {
                c: centre,
                x: ex,
                y: axis,
                r,
            },
            t: [lo, hi],
            v: [vlo, vhi],
            degenerate: false,
            tol: TOL_MIN,
        });
        let seam = nb.edges.len() - 1;
        let (elo, ehi) = if va < vb { (e, eb) } else { (eb, e) };
        let line = |ox: f64, oy: f64, dx: f64, dy: f64| Curve::Line {
            o: P3::new(ox, oy, 0.0),
            d: V3::new(dx, dy, 0.0),
        };
        // Counter-clockwise in (u, v) (u = t − t0): along the lower circle, up the seam at
        // u = 2π, back along the higher, down the seam at u = 0.
        let mut lp = vec![
            Coedge {
                edge: elo,
                fwd: true,
                pc: line(-t0, lo, 1.0, 0.0),
            },
            Coedge {
                edge: seam,
                fwd: true,
                pc: line(TAU, 0.0, 0.0, 1.0),
            },
            Coedge {
                edge: ehi,
                fwd: false,
                pc: line(-t0, hi, 1.0, 0.0),
            },
            Coedge {
                edge: seam,
                fwd: false,
                pc: line(0.0, 0.0, 0.0, 1.0),
            },
        ];
        // Out of the tube where the material is round its centre (a convex rim).
        let sense = convex;
        if !sense {
            lp.reverse();
            for c in lp.iter_mut() {
                c.fwd = !c.fwd;
            }
        }
        // Each rim run against its face's use of it.
        let fwd_of = |edge: usize| lp.iter().find(|c| c.edge == edge).map(|c| c.fwd);
        if fwd_of(e)? == sa || fwd_of(eb)? == sb {
            return None;
        }
        nb.faces.push(Face {
            surf,
            sense,
            loops: vec![lp],
        });
        let fi = nb.faces.len() - 1;
        nb.shells[0].faces.push(fi);
        src.push(vec![FaceSrc::New(Origin::FromEdge {
            body: input,
            edge: EdgeId(e as u64),
        })]);
        // The corner's centroid k_c·r in from the rim along both faces (out of the material
        // for a concave rim, where the round adds).
        let (rho, _) = radial(p + (ma + mb) * (k_c * r));
        change += if convex { -1.0 } else { 1.0 } * area * TAU * rho;
    }
    if !nb.check_with(CheckLevel::Fast).is_empty() {
        return None;
    }
    let (v0, v1) = (super::mass::body_volume(b), super::mass::body_volume(&nb));
    ((v0 + change - v1).abs() <= 1e-9 * v0.abs().max(1.0)).then_some((nb, src))
}
