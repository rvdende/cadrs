//! A chamfer of a rim: a whole circle between two faces turned about its axis, cut in closed
//! form.
//!
//! Where an edge is a full circle and the faces either side of it are surfaces of revolution
//! about the circle's axis (a plane square to it, a cylinder, a cone, a sphere or a torus about
//! it), everything about its blend is the same all the way round, so it is settled in the
//! half-plane through the axis: there each face is a straight line or a circle's arc, the ball
//! that touches both at its two offsets has its centre where those two, moved in by the
//! offsets, meet (two lines, a line and a circle, or two circles: a quadratic at most), and its
//! contacts are that centre's feet on them. The chamfer is the cone between the two contacts'
//! circles (a cylinder or a flat ring where it runs along the axis or square to it). The two
//! faces are cut back to those circles, their seams with them, and the cone is put between:
//! nothing is marched, intersected or trimmed. (The general local blend made the same solid
//! by following the ball round the edge.)

use super::blend::FaceSrc;
use super::classify::{self, InFace};
use super::curve::Curve;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, Face};
use crate::{BodyId, EdgeId, Origin};
use std::f64::consts::TAU;

/// None, saying why with `CADRS_BLEND_DEBUG`.
fn no<T>(why: &str) -> Option<T> {
    if crate::native::env_flag!("CADRS_BLEND_DEBUG") {
        eprintln!("rim chamfer: not made: {why}");
    }
    None
}

/// A face's cut by the half-plane through the axis, near the rim: a line through `p` or a
/// circle about `q`, with `n` its outward normal at the rim (in (radius, height)).
enum Meridian {
    Line { p: V2, n: V2 },
    Circle { q: V2, r: f64, out: f64 },
}

impl Meridian {
    /// This moved into the solid by `k` (out of it for less than nought).
    fn moved_in(&self, k: f64) -> Option<Meridian> {
        Some(match *self {
            Meridian::Line { p, n } => Meridian::Line { p: p - n * k, n },
            Meridian::Circle { q, r, out } => {
                let r = r - out * k;
                if r <= tol::POINT {
                    return None;
                }
                Meridian::Circle { q, r, out }
            }
        })
    }

    /// The point of this nearest `x`.
    fn foot(&self, x: &V2) -> Option<V2> {
        Some(match *self {
            Meridian::Line { p, n } => x - n * (x - p).dot(&n),
            Meridian::Circle { q, r, .. } => q + (x - q).try_normalize(tol::TINY)? * r,
        })
    }

    /// Where this and `o` meet, nearest `near`.
    fn meets(&self, o: &Meridian, near: &V2) -> Option<V2> {
        let nearest = |a: V2, b: V2| {
            if (a - near).norm() <= (b - near).norm() {
                a
            } else {
                b
            }
        };
        match (self, o) {
            (Meridian::Line { p, n }, Meridian::Line { p: p2, n: n2 }) => {
                let (x, y) = super::math::solve2_any(n.x, n.y, n2.x, n2.y, p.dot(n), p2.dot(n2))?;
                Some(V2::new(x, y))
            }
            (Meridian::Line { p, n }, Meridian::Circle { q, r, .. })
            | (Meridian::Circle { q, r, .. }, Meridian::Line { p, n }) => {
                // The line's foot from the centre, and half its chord.
                let h = (q - p).dot(n);
                let half = (r * r - h * h).max(0.0).sqrt();
                if h.abs() > r + tol::POINT {
                    return None;
                }
                let (foot, along) = (q - n * h, V2::new(-n.y, n.x));
                Some(nearest(foot + along * half, foot - along * half))
            }
            (Meridian::Circle { q, r, .. }, Meridian::Circle { q: q2, r: r2, .. }) => {
                let d = q2 - q;
                let l = d.norm();
                if l <= tol::POINT || l > r + r2 + tol::POINT || l < (r - r2).abs() - tol::POINT {
                    return None;
                }
                let a = (l * l + r * r - r2 * r2) / (2.0 * l);
                let half = (r * r - a * a).max(0.0).sqrt();
                let (u, w) = (d / l, V2::new(-d.y, d.x) / l);
                Some(nearest(q + u * a + w * half, q + u * a - w * half))
            }
        }
    }
}

/// A rim's blend ([`rim_chamfer`]), where its edge and faces allow.
struct Rim {
    /// A point of the axis, its direction and the radial direction the rim's vertex lies in.
    c: P3,
    z: V3,
    m: V3,
}

impl Rim {
    fn flat(&self, p: &P3) -> V2 {
        let d = p - self.c;
        V2::new(d.dot(&self.m), d.dot(&self.z))
    }
    /// [`Self::lift`] the angle `a` round the axis from the rim's first vertex.
    fn lift_at(&self, x: &V2, a: f64) -> P3 {
        let side = self.z.cross(&self.m);
        self.c + (self.m * a.cos() + side * a.sin()) * x.x + self.z * x.y
    }
    fn flat_dir(&self, v: &V3) -> V2 {
        V2::new(v.dot(&self.m), v.dot(&self.z))
    }

    /// A face's meridian at the rim's point `e`, where its surface is one of revolution about
    /// the rim's axis.
    fn meridian(&self, f: &Face, e: &P3) -> Option<Meridian> {
        let near = tol::REL_POINT * (1.0 + (e - self.c).norm());
        let along = |v: &V3| v.cross(&self.z).norm() < tol::ANGULAR;
        let on_axis = |p: &P3| {
            let d = p - self.c;
            (d - self.z * d.dot(&self.z)).norm() <= near
        };
        let (u, v) = f.surf.project(e, None);
        let n3 = f.surf.normal(u, v).try_normalize(tol::TINY)? * if f.sense { 1.0 } else { -1.0 };
        let n = self.flat_dir(&n3).try_normalize(0.5)?;
        let x = self.flat(e);
        Some(match &f.surf {
            Surface::Plane { f } if along(&f.z) => Meridian::Line { p: x, n },
            Surface::Cylinder { f, .. } | Surface::Cone { f, .. }
                if along(&f.z) && on_axis(&f.o) =>
            {
                Meridian::Line { p: x, n }
            }
            Surface::Sphere { f, r } if on_axis(&f.o) => {
                let q = self.flat(&f.o);
                Meridian::Circle {
                    q,
                    r: *r,
                    out: (x - q).dot(&n).signum(),
                }
            }
            Surface::Torus { f, rmaj, rmin } if along(&f.z) && on_axis(&f.o) => {
                let q = V2::new(*rmaj, self.flat(&f.o).y);
                Meridian::Circle {
                    q,
                    r: *rmin,
                    out: (x - q).dot(&n).signum(),
                }
            }
            _ => return None,
        })
    }
}

/// One arc of a rim: its edge, the angle round the axis its start is at (from the rim's first
/// vertex), how much its circle's parameter is behind that angle, and where it is in the two
/// faces' loops (loop, coedge).
struct Piece {
    edge: usize,
    from: f64,
    behind: f64,
    place: [(usize, usize); 2],
}

/// A rim read for its blend: its arcs in order round the axis, its two faces (the one its
/// edges run forward in first), their meridians at the rim's point `e2`, the way each runs
/// from it, and whether the rim is a ridge (1) or a groove (−1).
struct Read<'a> {
    b: &'a Body,
    rim: Rim,
    pieces: Vec<Piece>,
    faces: [usize; 2],
    lines: [Meridian; 2],
    e2: V2,
    runs: [V2; 2],
    side: f64,
    near: f64,
    polys: [Vec<Vec<P2>>; 2],
    /// An angle round the axis clear of every vertex of the rim (the faces' seams run through
    /// those), where points are asked of the faces.
    aside: f64,
}

impl<'a> Read<'a> {
    /// The rim the edges `es` of `b` make, where they make one (see the module): arcs of one
    /// circle, end to end the whole way round, between the same two faces, each running
    /// forward in the same one.
    fn of(b: &'a Body, es: &[usize]) -> Option<Self> {
        let first = b.edges.get(*es.first()?)?;
        let Curve::Circle { c, x, y, r } = &first.curve else {
            return no("the edge is no circle");
        };
        let start = first.curve.point(first.t[0]);
        let rim = Rim {
            c: *c,
            z: x.cross(y).try_normalize(tol::TINY)?,
            m: (start - c).try_normalize(tol::TINY)?,
        };
        let size = b.bbox().diagonal();
        let near = tol::REL_POINT * (1.0 + size);
        // Each edge's two uses, forward and back.
        let mut uses: Vec<[(usize, usize, usize); 2]> = Vec::with_capacity(es.len());
        for &e in es {
            let (mut fwd, mut rev) = (None, None);
            for (fi, f) in b.faces.iter().enumerate() {
                for (li, l) in f.loops.iter().enumerate() {
                    for (ci, co) in l.iter().enumerate() {
                        if co.edge == e {
                            let slot = if co.fwd { &mut fwd } else { &mut rev };
                            if slot.replace((fi, li, ci)).is_some() {
                                return no("an edge used twice one way");
                            }
                        }
                    }
                }
            }
            uses.push([fwd?, rev?]);
        }
        let faces = [uses[0][0].0, uses[0][1].0];
        if faces[0] == faces[1]
            || uses
                .iter()
                .any(|u| u[0].0 != faces[0] || u[1].0 != faces[1])
        {
            return no("not one pair of faces all round");
        }
        // The arcs in order round the axis from the first's start.
        let mut pieces: Vec<Piece> = Vec::with_capacity(es.len());
        let (mut at, mut vertex) = (0.0, first.v[0]);
        for _ in 0..es.len() {
            let k = es.iter().position(|&e| {
                b.edges[e].v[0] == vertex && pieces.iter().all(|p: &Piece| p.edge != e)
            })?;
            let ed = &b.edges[es[k]];
            let Curve::Circle {
                c: c2,
                x: x2,
                y: y2,
                r: r2,
            } = &ed.curve
            else {
                return no("an edge is no circle");
            };
            let span = ed.t[1] - ed.t[0];
            if ed.degenerate
                || (c2 - c).norm() > near
                || (r2 - r).abs() > near
                || x2.cross(y2).dot(&rim.z) < 1.0 - tol::ANGULAR
                || !(span > 0.0)
            {
                return no("an edge of another circle");
            }
            // (Its parameter's angle round the axis: where its own x axis points.)
            let behind = at - ed.t[0];
            if (rim.lift_at(&V2::new(*r, 0.0), ed.t[1] + behind) - ed.curve.point(ed.t[1])).norm()
                > near
            {
                return no("an arc out of turn");
            }
            pieces.push(Piece {
                edge: es[k],
                from: at,
                behind,
                place: [(uses[k][0].1, uses[k][0].2), (uses[k][1].1, uses[k][1].2)],
            });
            at += span;
            vertex = ed.v[1];
        }
        if (at - TAU).abs() > tol::ANGULAR || vertex != first.v[0] {
            return no("not the whole way round");
        }
        // (Midway along the longest arc: as far from the vertices as may be.)
        let aside = pieces
            .iter()
            .map(|p| {
                let ed = &b.edges[p.edge];
                (ed.t[1] - ed.t[0], p.from + 0.5 * (ed.t[1] - ed.t[0]))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))?
            .1;
        let e2 = rim.flat(&start);
        let lines: Vec<Meridian> = faces
            .iter()
            .map(|&f| rim.meridian(&b.faces[f], &start))
            .collect::<Option<_>>()?;
        let normal = |m: &Meridian| match *m {
            Meridian::Line { n, .. } => n,
            Meridian::Circle { q, out, .. } => (e2 - q).normalize() * out,
        };
        let (n0, n1) = (normal(&lines[0]), normal(&lines[1]));
        let polys: Vec<Vec<Vec<P2>>> = faces
            .iter()
            .map(|&f| classify::face_polygons(b, &b.faces[f]))
            .collect();
        let mut lines = lines.into_iter();
        let mut polys = polys.into_iter();
        let mut read = Read {
            b,
            rim,
            pieces,
            faces,
            lines: [lines.next()?, lines.next()?],
            e2,
            runs: [V2::zeros(); 2],
            side: 1.0,
            near,
            polys: [polys.next()?, polys.next()?],
            aside,
        };
        // Which way each face runs from the rim (in the half-plane), by a point of it a little
        // way along; so whether the rim is a ridge or a groove.
        let step = 1e-3 * *r;
        let runs = |k: usize, n: V2| -> Option<V2> {
            let t = V2::new(-n.y, n.x);
            [t, -t].into_iter().find(|d| {
                read.lines[k]
                    .foot(&(e2 + d * step))
                    .is_some_and(|x| read.within(k, &x))
            })
        };
        let (d0, d1) = (runs(0, n0)?, runs(1, n1)?);
        let ridge = d0.dot(&n1) < 0.0;
        if ridge != (d1.dot(&n0) < 0.0) {
            return no("neither a ridge nor a groove");
        }
        read.runs = [d0, d1];
        read.side = if ridge { 1.0 } else { -1.0 };
        Some(read)
    }

    /// Whether the point `x` of face k's meridian is inside the face (asked clear of the
    /// faces' seams).
    fn within(&self, k: usize, x: &V2) -> bool {
        let p = self.rim.lift_at(x, self.aside);
        let s = &self.b.faces[self.faces[k]].surf;
        let (u, v) = s.project(&p, None);
        (s.eval(u, v) - p).norm() <= self.near
            && classify::in_face(&self.polys[k], s, P2::new(u, v), tol::CONFUSION) == InFace::Inside
    }

    /// The body with the rim cut off between `feet`, a point of each face's meridian: the
    /// faces cut back to the circles through them, the cone between. None where a foot isn't
    /// inside its face along from the rim, something else hangs on a vertex of the rim, or
    /// the body made isn't sound.
    fn cut(&self, input: BodyId, feet: [V2; 2]) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
        let (b, rim, near, e2, side, faces) =
            (self.b, &self.rim, self.near, self.e2, self.side, self.faces);
        if (feet[0] - e2).dot(&self.runs[0]) <= near
            || (feet[1] - e2).dot(&self.runs[1]) <= near
            || feet.iter().any(|x| x.x <= near)
            || (feet[0] - feet[1]).norm() <= near
            || !(self.within(0, &feet[0]) && self.within(1, &feet[1]))
        {
            return no("a contact off its face");
        }
        let n = self.pieces.len();
        let mut nb = b.clone();
        // A vertex on each face's new circle for each of the rim's, and an arc for each arc.
        let nv: [Vec<usize>; 2] = [0, 1].map(|k| {
            self.pieces
                .iter()
                .map(|p| nb.add_vertex(rim.lift_at(&feet[k], p.from)))
                .collect()
        });
        let ne: [Vec<usize>; 2] = [0, 1].map(|k| {
            (0..n)
                .map(|j| {
                    let ed = &b.edges[self.pieces[j].edge];
                    let Curve::Circle { x, y, .. } = &ed.curve else {
                        unreachable!("a rim's arcs are circles")
                    };
                    let arc = Curve::Circle {
                        c: rim.c + rim.z * feet[k].y,
                        x: *x,
                        y: *y,
                        r: feet[k].x,
                    };
                    nb.add_edge(arc, ed.t[0], ed.t[1], nv[k][j], nv[k][(j + 1) % n])
                })
                .collect()
        });
        // Each face cut back to its circle: the rim's coedges on the new arcs, and the face's
        // seams (the edges of its own that ended at the rim's vertices) ending at the new ones.
        let mut seams: Vec<usize> = Vec::new();
        let rim_edge = |e: usize| self.pieces.iter().any(|p| p.edge == e);
        for k in 0..2 {
            let fi = faces[k];
            let surf = b.faces[fi].surf.clone();
            for (j, p) in self.pieces.iter().enumerate() {
                let (li, ci) = p.place[k];
                let ed = &b.edges[p.edge];
                let mid = b.faces[fi].loops[li][ci]
                    .pc
                    .point(0.5 * (ed.t[0] + ed.t[1]));
                // (The cut moves the rim's line in (u, v) with its points; near where it was.)
                let pc = super::pcurve::pcurve(
                    &surf,
                    &nb.edges[ne[k][j]].curve,
                    ed.t[0],
                    ed.t[1],
                    Some(P2::new(mid.x, mid.y)),
                );
                nb.faces[fi].loops[li][ci] = Coedge {
                    edge: ne[k][j],
                    fwd: k == 0,
                    pc,
                };
            }
            for co in b.faces[fi].loops.iter().flatten() {
                let se = co.edge;
                if rim_edge(se) || seams.contains(&se) {
                    continue;
                }
                let s = &b.edges[se];
                for (j, p) in self.pieces.iter().enumerate() {
                    let vert = b.edges[p.edge].v[0];
                    if !s.v.contains(&vert) {
                        continue;
                    }
                    if s.degenerate || s.v[0] == s.v[1] {
                        return no("a closed edge on the rim's vertex");
                    }
                    let at = nb.verts[nv[k][j]].p;
                    let t = s.curve.closest(&at, s.t[0], s.t[1]);
                    if (s.curve.point(t) - at).norm() > near {
                        return no("a seam off the contact");
                    }
                    let end = usize::from(s.v[1] == vert);
                    nb.edges[se].v[end] = nv[k][j];
                    nb.edges[se].t[end] = t;
                    if !(nb.edges[se].t[0] < nb.edges[se].t[1]) {
                        return no("a seam shorter than the cut");
                    }
                    seams.push(se);
                }
            }
        }
        // Nothing else may hang on the rim's vertices: an edge of a third face, or one shared.
        let verts: Vec<usize> = self.pieces.iter().map(|p| b.edges[p.edge].v[0]).collect();
        for (fi, f) in b.faces.iter().enumerate() {
            for co in f.loops.iter().flatten() {
                let touches =
                    !rim_edge(co.edge) && b.edges[co.edge].v.iter().any(|v| verts.contains(v));
                if touches && (!seams.contains(&co.edge) || !faces.contains(&fi)) {
                    return no("another edge on the rim's vertex");
                }
            }
        }
        // (A seam is one face's own: used there and back in it, and nowhere else.)
        for &se in &seams {
            let users = b
                .faces
                .iter()
                .filter(|f| f.loops.iter().flatten().any(|co| co.edge == se))
                .count();
            if users != 1 {
                return no("a seam shared");
            }
        }

        // The chamfer's face between the two circles, from the lower to the higher along the
        // axis (or from the inner to the outer where it is flat), its normal out of the solid:
        // towards the rim's point, which a ridge's chamfer cuts off and a groove's covers.
        let out2 = {
            let d = feet[1] - feet[0];
            let n = V2::new(-d.y, d.x).normalize();
            n * (e2 - 0.5 * (feet[0] + feet[1])).dot(&n).signum() * side
        };
        // Its (u, v) from the rim's first vertex round the axis: an arc's parameter is that
        // much behind u.
        let frame = |h: f64| Frame {
            o: rim.c + rim.z * h,
            x: rim.m,
            y: rim.z.cross(&rim.m),
            z: rim.z,
        };
        let line_pc = |o: (f64, f64), d: (f64, f64)| Curve::Line {
            o: P3::new(o.0, o.1, 0.0),
            d: V3::new(d.0, d.1, 0.0),
        };
        // The new coedges run against the faces' own: back on the circle the first face was
        // cut to, forward on the other's. A loop along a circle takes its arcs in that order.
        let round = |k: usize, pc: &dyn Fn(usize) -> Curve| -> Vec<Coedge> {
            let fwd = k == 1;
            let order: Vec<usize> = if fwd {
                (0..n).collect()
            } else {
                (0..n).rev().collect()
            };
            order
                .into_iter()
                .map(|j| Coedge {
                    edge: ne[k][j],
                    fwd,
                    pc: pc(j),
                })
                .collect()
        };
        let level = (feet[0].y - feet[1].y).abs() <= near;
        let face = if level {
            // A flat ring: its outer circle first.
            let (outer, inner) = if feet[0].x > feet[1].x {
                (0, 1)
            } else {
                (1, 0)
            };
            let sense = out2.y > 0.0;
            if (outer == 1) != sense {
                return no("a ring the wrong way round");
            }
            let ring = |k: usize| {
                round(k, &|j| {
                    let a = self.pieces[j].behind;
                    Curve::Circle {
                        c: P3::origin(),
                        x: V3::new(a.cos(), a.sin(), 0.0),
                        y: V3::new(-a.sin(), a.cos(), 0.0),
                        r: feet[k].x,
                    }
                })
            };
            Face {
                surf: Surface::Plane {
                    f: frame(feet[0].y),
                },
                sense,
                loops: vec![ring(outer), ring(inner)],
            }
        } else {
            let (lo, hi) = if feet[0].y < feet[1].y {
                (0, 1)
            } else {
                (1, 0)
            };
            let d = feet[hi] - feet[lo];
            let length = d.norm();
            let surf = if d.x.abs() <= near {
                Surface::Cylinder {
                    f: frame(feet[lo].y),
                    r: feet[lo].x,
                }
            } else {
                Surface::Cone {
                    f: frame(feet[lo].y),
                    r: feet[lo].x,
                    a: d.x.atan2(d.y),
                }
            };
            // The surface's own normal points away from the axis: the face's sense by that.
            let sense = out2.x > 0.0;
            if (lo == 1) != sense {
                return no("a cone the wrong way round");
            }
            let (seam_curve, _) = Curve::line(nb.verts[nv[lo][0]].p, nb.verts[nv[hi][0]].p);
            let seam = nb.add_edge(seam_curve, 0.0, length, nv[lo][0], nv[hi][0]);
            let up = |u: f64, fwd: bool| Coedge {
                edge: seam,
                fwd,
                pc: line_pc((u, 0.0), (0.0, 1.0)),
            };
            let along =
                |k: usize, v: f64| round(k, &|j| line_pc((self.pieces[j].behind, v), (1.0, 0.0)));
            // Along the lower circle, up the seam, back along the upper, down the seam: the
            // seam at u = 2π after a circle run forward, at nought after one run back.
            let mut l = along(lo, 0.0);
            l.push(up(if sense { TAU } else { 0.0 }, true));
            l.extend(along(hi, length));
            l.push(up(if sense { 0.0 } else { TAU }, false));
            Face {
                surf,
                sense,
                loops: vec![l],
            }
        };
        nb.faces.push(face);
        let new = nb.faces.len() - 1;
        for sh in &mut nb.shells {
            if sh.faces.contains(&faces[0]) {
                sh.faces.push(new);
            }
        }
        // (Its faces point as the input's did, the cone's the way it was told to above: the
        // check of how it is joined, without the solids' volumes for their sign.)
        let problems = nb.check_structure();
        if !problems.is_empty() {
            return no(&format!("not sound: {problems:?}"));
        }
        let mut src: Vec<Vec<FaceSrc>> = (0..b.faces.len())
            .map(|i| vec![FaceSrc::Input(i)])
            .collect();
        src.push(vec![FaceSrc::New(Origin::FromEdge {
            body: input,
            edge: EdgeId(self.pieces[0].edge as u64),
        })]);
        Some((nb, src))
    }
}

/// The edges `es` of `b` chamfered between the contacts of the ball whose offsets are `law`
/// from the face they run forward in and `law · ratio` from the other, where they are a rim
/// (see the module): the body, and each face's source. None where they aren't one, the
/// contacts leave their faces, or the body made isn't sound (the general blend then has it).
pub(crate) fn rim_chamfer(
    b: &Body,
    input: BodyId,
    es: &[usize],
    law: f64,
    ratio: f64,
) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
    if !(law > 0.0 && ratio > 0.0) {
        return None;
    }
    let read = Read::of(b, es)?;
    // The ball's centre, where the two meridians moved in by its offsets meet, and its feet.
    let centre = read.lines[0]
        .moved_in(read.side * law)?
        .meets(&read.lines[1].moved_in(read.side * law * ratio)?, &read.e2)?;
    let feet = [read.lines[0].foot(&centre)?, read.lines[1].foot(&centre)?];
    read.cut(input, feet)
}

/// The edges `es` of `b` chamfered along the line through the points `told` along their two
/// faces' tangents from them (each a face and how far along its tangent at the rim, in the
/// half-plane through the axis), where they are a rim: the chamfer's contacts are where that
/// line meets the faces themselves (the tangents' own points where a face is straight there).
/// None as [`rim_chamfer`].
pub(crate) fn rim_chamfer_through(
    b: &Body,
    input: BodyId,
    es: &[usize],
    told: [(usize, f64); 2],
) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
    let read = Read::of(b, es)?;
    let dist = |k: usize| {
        told.iter()
            .find(|(f, _)| *f == read.faces[k])
            .map(|x| x.1)
            .filter(|d| *d > 0.0)
    };
    let on = [
        read.e2 + read.runs[0] * dist(0)?,
        read.e2 + read.runs[1] * dist(1)?,
    ];
    let d = on[1] - on[0];
    let line = Meridian::Line {
        p: on[0],
        n: V2::new(-d.y, d.x).try_normalize(tol::TINY)?,
    };
    let feet = [
        read.lines[0].meets(&line, &on[0])?,
        read.lines[1].meets(&line, &on[1])?,
    ];
    read.cut(input, feet)
}
