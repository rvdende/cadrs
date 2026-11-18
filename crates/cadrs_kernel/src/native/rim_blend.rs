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
    fn lift(&self, x: &V2) -> P3 {
        self.c + self.m * x.x + self.z * x.y
    }
    /// [`Self::lift`] a radian round the axis from the rim's vertex: clear of the faces'
    /// seams, which run through the vertex.
    fn lift_aside(&self, x: &V2) -> P3 {
        let side = self.z.cross(&self.m);
        self.c + (self.m * 1f64.cos() + side * 1f64.sin()) * x.x + self.z * x.y
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

/// A rim read for its blend: its edge, its two faces (the one the edge runs forward in first)
/// each with where the edge is in its loops, their meridians at the rim's point `e2`, the way
/// each runs from it, and whether the rim is a ridge (1) or a groove (−1).
struct Read<'a> {
    b: &'a Body,
    e: usize,
    rim: Rim,
    uses: [(usize, usize, usize, bool); 2],
    lines: [Meridian; 2],
    e2: V2,
    runs: [V2; 2],
    side: f64,
    near: f64,
    polys: [Vec<Vec<P2>>; 2],
}

impl<'a> Read<'a> {
    /// The rim edge `e` of `b` is, where it is one (see the module).
    fn of(b: &'a Body, e: usize) -> Option<Self> {
        let ed = b.edges.get(e)?;
        let Curve::Circle { c, x, y, r } = &ed.curve else {
            return None;
        };
        if ed.degenerate
            || ed.v[0] != ed.v[1]
            || ((ed.t[1] - ed.t[0]).abs() - TAU).abs() > tol::ANGULAR
        {
            return None;
        }
        let start = ed.curve.point(ed.t[0]);
        let rim = Rim {
            c: *c,
            z: x.cross(y).try_normalize(tol::TINY)?,
            m: (start - c).try_normalize(tol::TINY)?,
        };
        let size = b.bbox().diagonal();
        let near = tol::REL_POINT * (1.0 + size);
        // The two faces: the one the edge runs forward in first.
        let mut uses: Vec<(usize, usize, usize, bool)> = Vec::new();
        for (fi, f) in b.faces.iter().enumerate() {
            for (li, l) in f.loops.iter().enumerate() {
                for (ci, co) in l.iter().enumerate() {
                    if co.edge == e {
                        uses.push((fi, li, ci, co.fwd));
                    }
                }
            }
        }
        let [a, o] = uses.as_slice() else { return None };
        if a.0 == o.0 || a.3 == o.3 {
            return None;
        }
        let (fwd, rev) = if a.3 { (*a, *o) } else { (*o, *a) };
        let faces = [fwd.0, rev.0];
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
        // Which way each face runs from the rim (in the half-plane), by a point of it a little
        // way along; so whether the rim is a ridge or a groove.
        let polys: Vec<Vec<Vec<P2>>> = faces
            .iter()
            .map(|&f| classify::face_polygons(b, &b.faces[f]))
            .collect();
        let within = |k: usize, p: &P3| {
            let s = &b.faces[faces[k]].surf;
            let (u, v) = s.project(p, None);
            (s.eval(u, v) - p).norm() <= near
                && classify::in_face(&polys[k], s, P2::new(u, v), tol::CONFUSION) == InFace::Inside
        };
        let step = 1e-3 * *r;
        let runs = |k: usize, n: V2| -> Option<V2> {
            let t = V2::new(-n.y, n.x);
            [t, -t].into_iter().find(|d| {
                lines[k]
                    .foot(&(e2 + d * step))
                    .is_some_and(|x| within(k, &rim.lift_aside(&x)))
            })
        };
        let (d0, d1) = (runs(0, n0)?, runs(1, n1)?);
        let ridge = d0.dot(&n1) < 0.0;
        if ridge != (d1.dot(&n0) < 0.0) {
            return None;
        }
        let mut lines = lines.into_iter();
        let mut polys = polys.into_iter();
        Some(Read {
            b,
            e,
            rim,
            uses: [fwd, rev],
            lines: [lines.next()?, lines.next()?],
            e2,
            runs: [d0, d1],
            side: if ridge { 1.0 } else { -1.0 },
            near,
            polys: [polys.next()?, polys.next()?],
        })
    }

    /// Whether the point `x` of face k's meridian is inside the face (asked a radian round
    /// the axis from the rim's vertex, clear of the faces' seams, which run through it).
    fn within(&self, k: usize, x: &V2) -> bool {
        let p = self.rim.lift_aside(x);
        let s = &self.b.faces[self.uses[k].0].surf;
        let (u, v) = s.project(&p, None);
        (s.eval(u, v) - p).norm() <= self.near
            && classify::in_face(&self.polys[k], s, P2::new(u, v), tol::CONFUSION) == InFace::Inside
    }

    /// The body with the rim cut off between `feet`, a point of each face's meridian: the
    /// faces cut back to the circles through them, the cone between. None where a foot isn't
    /// inside its face along from the rim, something else hangs on the rim's vertex, or the
    /// body made isn't sound.
    fn cut(&self, input: BodyId, feet: [V2; 2]) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
        let (b, e, rim, near, e2, side) =
            (self.b, self.e, &self.rim, self.near, self.e2, self.side);
        let ed = &b.edges[e];
        let Curve::Circle { x, y, .. } = &ed.curve else {
            return None;
        };
        let vert = ed.v[0];
        let [fwd, rev] = self.uses;
        let faces = [fwd.0, rev.0];
        if (feet[0] - e2).dot(&self.runs[0]) <= near
            || (feet[1] - e2).dot(&self.runs[1]) <= near
            || feet.iter().any(|x| x.x <= near)
            || (feet[0] - feet[1]).norm() <= near
            || !(self.within(0, &feet[0]) && self.within(1, &feet[1]))
        {
            return None;
        }
        let at = [rim.lift(&feet[0]), rim.lift(&feet[1])];
        let mut nb = b.clone();
        let (t0, t1) = (ed.t[0], ed.t[1]);
        let circle = |x2: &V2| Curve::Circle {
            c: rim.c + rim.z * x2.y,
            x: *x,
            y: *y,
            r: x2.x,
        };
        let nv = [nb.add_vertex(at[0]), nb.add_vertex(at[1])];
        let ne = [
            nb.add_edge(circle(&feet[0]), t0, t1, nv[0], nv[0]),
            nb.add_edge(circle(&feet[1]), t0, t1, nv[1], nv[1]),
        ];
        // Each face cut back to its circle: the rim's coedge on the new edge, and the face's seam
        // (the one edge of its own that ended at the rim's vertex) ending at the new vertex.
        let mut seams: Vec<usize> = Vec::new();
        for (k, &(fi, li, ci, dir)) in [fwd, rev].iter().enumerate() {
            let surf = b.faces[fi].surf.clone();
            let old = &b.faces[fi].loops[li][ci].pc;
            let mid = old.point(0.5 * (t0 + t1));
            let pc = super::pcurve::pcurve(
                &surf,
                &nb.edges[ne[k]].curve,
                t0,
                t1,
                Some(P2::new(mid.x, mid.y)),
            );
            // (The cut moves the rim's line in (u, v) along with its points: the hint is the old
            // line's middle, moved as the contact is from the rim's point.)
            nb.faces[fi].loops[li][ci] = Coedge {
                edge: ne[k],
                fwd: dir,
                pc,
            };
            for co in b.faces[fi].loops.iter().flatten() {
                let se = co.edge;
                if se == e || seams.contains(&se) || !b.edges[se].v.contains(&vert) {
                    continue;
                }
                let s = &b.edges[se];
                if s.degenerate || s.v[0] == s.v[1] {
                    return None;
                }
                let t = s.curve.closest(&at[k], s.t[0], s.t[1]);
                if (s.curve.point(t) - at[k]).norm() > near {
                    return None;
                }
                let end = usize::from(s.v[1] == vert);
                nb.edges[se].v[end] = nv[k];
                nb.edges[se].t[end] = t;
                if !(nb.edges[se].t[0] < nb.edges[se].t[1]) {
                    return None;
                }
                seams.push(se);
            }
        }
        // Nothing else may hang on the rim's vertex: an edge of a third face, or one shared.
        for (fi, f) in b.faces.iter().enumerate() {
            for co in f.loops.iter().flatten() {
                let touches = b.edges[co.edge].v.contains(&vert) && co.edge != e;
                if touches && (!seams.contains(&co.edge) || !faces.contains(&fi)) {
                    return None;
                }
            }
        }
        // (A seam is one face's own: used there and back in it, and nowhere else.)
        for &se in &seams {
            let users: Vec<usize> = b
                .faces
                .iter()
                .enumerate()
                .filter(|(_, f)| f.loops.iter().flatten().any(|co| co.edge == se))
                .map(|(fi, _)| fi)
                .collect();
            if users.len() != 1 {
                return None;
            }
        }

        // The chamfer's face between the two circles, from the lower to the higher along the axis
        // (or from the inner to the outer where it is flat), its normal out of the solid.
        let out2 = {
            let d = feet[1] - feet[0];
            let n = V2::new(-d.y, d.x).normalize();
            // (Towards the rim's point, which a ridge's chamfer cuts off and a groove's covers.)
            n * (e2 - 0.5 * (feet[0] + feet[1])).dot(&n).signum() * side
        };
        let frame = |h: f64| Frame {
            o: rim.c + rim.z * h,
            x: *x,
            y: *y,
            z: rim.z,
        };
        let line_pc = |o: (f64, f64), d: (f64, f64)| Curve::Line {
            o: P3::new(o.0, o.1, 0.0),
            d: V3::new(d.0, d.1, 0.0),
        };
        // The new coedges run against the faces' own: forward on the circle the reversed face
        // was cut to, back on the other.
        let dirs = [!fwd.3, !rev.3];
        let level = (feet[0].y - feet[1].y).abs() <= near;
        let face = if level {
            // A flat ring: its outer circle first.
            let (outer, inner) = if feet[0].x > feet[1].x {
                (0, 1)
            } else {
                (1, 0)
            };
            let sense = out2.y > 0.0;
            if dirs[outer] != sense || dirs[inner] == sense {
                return None;
            }
            let ring = |k: usize| {
                vec![Coedge {
                    edge: ne[k],
                    fwd: dirs[k],
                    pc: Curve::Circle {
                        c: P3::origin(),
                        x: V3::x(),
                        y: V3::y(),
                        r: feet[k].x,
                    },
                }]
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
            // (Its seam through the two new vertices: u from the rim's vertex round the axis,
            // so a circle's parameter t is u + t0.)
            let turned = Frame {
                o: rim.c + rim.z * feet[lo].y,
                x: rim.m,
                y: rim.z.cross(&rim.m),
                z: rim.z,
            };
            let surf = if d.x.abs() <= near {
                Surface::Cylinder {
                    f: turned,
                    r: feet[lo].x,
                }
            } else {
                Surface::Cone {
                    f: turned,
                    r: feet[lo].x,
                    a: d.x.atan2(d.y),
                }
            };
            // The surface's own normal points away from the axis: the face's sense by that.
            let sense = out2.x > 0.0;
            if dirs[lo] != sense || dirs[hi] == sense {
                return None;
            }
            let (seam_curve, _) = Curve::line(at[lo], at[hi]);
            let seam = nb.add_edge(seam_curve, 0.0, length, nv[lo], nv[hi]);
            let bottom = Coedge {
                edge: ne[lo],
                fwd: dirs[lo],
                pc: line_pc((-t0, 0.0), (1.0, 0.0)),
            };
            let top = Coedge {
                edge: ne[hi],
                fwd: dirs[hi],
                pc: line_pc((-t0, length), (1.0, 0.0)),
            };
            let up = |u: f64, fwd: bool| Coedge {
                edge: seam,
                fwd,
                pc: line_pc((u, 0.0), (0.0, 1.0)),
            };
            let loops = if sense {
                vec![bottom, up(TAU, true), top, up(0.0, false)]
            } else {
                vec![bottom, up(0.0, true), top, up(TAU, false)]
            };
            Face {
                surf,
                sense,
                loops: vec![loops],
            }
        };
        nb.faces.push(face);
        let new = nb.faces.len() - 1;
        for sh in &mut nb.shells {
            if sh.faces.contains(&faces[0]) {
                sh.faces.push(new);
            }
        }
        if !nb.check().is_empty() {
            if crate::native::env_flag!("CADRS_BLEND_DEBUG") {
                eprintln!("rim chamfer: not sound: {:?}", nb.check());
            }
            return None;
        }
        let mut src: Vec<Vec<FaceSrc>> = (0..b.faces.len())
            .map(|i| vec![FaceSrc::Input(i)])
            .collect();
        src.push(vec![FaceSrc::New(Origin::FromEdge {
            body: input,
            edge: EdgeId(e as u64),
        })]);
        Some((nb, src))
    }
}

/// Edge `e` of `b` chamfered between the contacts of the ball whose offsets are `law` from the
/// face the edge runs forward in and `law · ratio` from the other, where the edge is a rim
/// (see the module): the body, and each face's source. None where it isn't one, the contacts
/// leave their faces, or the body made isn't sound (the general blend then has it).
pub(crate) fn rim_chamfer(
    b: &Body,
    input: BodyId,
    e: usize,
    law: f64,
    ratio: f64,
) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
    if !(law > 0.0 && ratio > 0.0) {
        return None;
    }
    let read = Read::of(b, e)?;
    // The ball's centre, where the two meridians moved in by its offsets meet, and its feet.
    let centre = read.lines[0]
        .moved_in(read.side * law)?
        .meets(&read.lines[1].moved_in(read.side * law * ratio)?, &read.e2)?;
    let feet = [read.lines[0].foot(&centre)?, read.lines[1].foot(&centre)?];
    read.cut(input, feet)
}

/// Edge `e` of `b` chamfered along the line through the points `told` along its two faces'
/// tangents from it (each a face and how far along its tangent at the rim, in the half-plane
/// through the axis), where the edge is a rim: the chamfer's contacts are where that line
/// meets the faces themselves (the tangents' own points where a face is straight there). None
/// as [`rim_chamfer`].
pub(crate) fn rim_chamfer_through(
    b: &Body,
    input: BodyId,
    e: usize,
    told: [(usize, f64); 2],
) -> Option<(Body, Vec<Vec<FaceSrc>>)> {
    let read = Read::of(b, e)?;
    let dist = |k: usize| {
        told.iter()
            .find(|(f, _)| *f == read.uses[k].0)
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
