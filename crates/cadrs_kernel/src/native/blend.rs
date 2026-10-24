//! Fillets and chamfers as exact tools combined by booleans.
//!
//! For each edge, the cross-section of what the blend removes (a convex edge) or adds (a
//! concave one) is built in the plane square to the edge: the corner, the two contact points
//! on the faces and the section curve between them (a circular arc for a fillet, a straight
//! line for a chamfer, a conic or a quintic). The section is swept along a straight edge
//! between two planes (exact planes and a cylinder), or turned about the axis of a circular
//! edge between a plane and a coaxial surface of revolution (a torus), and extended past the
//! edge's ends, where the end faces trim it. Where three filleted edges between planes meet,
//! the corner is the ball rolling on all three faces: a corner tool (the block between the
//! faces and the ball's centre, less the ball) leaves the exact sphere octant.

use super::boolean::{BoolOut, boolean_bodies, boolean_bodies_flush};
use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::NurbsCurve;
#[allow(unused_imports)]
use super::profile;
use super::profile::{MergedFace, Piece, Src};
use super::surface::Surface;
use super::sweep::{self, Prism, Revolve};
use super::tol;
use super::topo::{Body, Face, co_range};
use crate::*;

/// What each face of a body continues: an input face, or a new face with an origin.
#[derive(Debug, Clone, PartialEq)]
pub enum FaceSrc {
    Input(usize),
    New(Origin),
}

/// A body with the origin of every face.
#[derive(Debug, Clone)]
pub struct Tracked {
    pub body: Body,
    pub src: Vec<Vec<FaceSrc>>,
}

impl Tracked {
    pub fn of(b: Body) -> Self {
        let n = b.faces.len();
        Self {
            body: b,
            src: (0..n).map(|i| vec![FaceSrc::Input(i)]).collect(),
        }
    }

    /// Combines with a tool whose faces carry `tool_src`.
    pub fn apply(&self, tool: &Body, tool_src: &[Vec<FaceSrc>], op: BoolOp) -> Result<Tracked> {
        let r: BoolOut = match boolean_bodies_flush(&self.body, tool, op) {
            Ok(r) => r,
            Err(e) => {
                super::kernel::dump("tracked-target", &self.body, &format!("{op:?}"));
                super::kernel::dump("tracked-tool", tool, &format!("{op:?}: {e}"));
                return Err(e);
            }
        };
        let src = r
            .from
            .iter()
            .map(|srcs| {
                let mut out: Vec<FaceSrc> = Vec::new();
                for &(o, f) in srcs {
                    let from = if o == 0 { &self.src[f] } else { &tool_src[f] };
                    for x in from {
                        if !out.contains(x) {
                            out.push(x.clone());
                        }
                    }
                }
                out
            })
            .collect();
        Ok(Tracked { body: r.body, src })
    }

    /// With no more solids than the input had (`n`): the largest kept. A blend never splits a
    /// solid: the pieces beyond are what its tools left of a corner (an island where two
    /// chamfers' cuts nearly meet, a fill run on past its corner).
    pub fn with_solids_of_input(self, n: usize) -> Result<Tracked> {
        let m = self.body.solids.len();
        if n == 0 || m <= n {
            return Ok(self);
        }
        let mut by_volume: Vec<(f64, Vec<usize>)> = (0..m)
            .map(|s| {
                let fs = self.body.solid_faces(s);
                (
                    super::mass::body_volume(&super::kernel::subset(&self.body, &fs, true)).abs(),
                    fs,
                )
            })
            .collect();
        by_volume.sort_by(|x, y| y.0.total_cmp(&x.0));
        let mut keep: Vec<usize> = by_volume
            .into_iter()
            .take(n)
            .flat_map(|(_, fs)| fs)
            .collect();
        keep.sort_unstable();
        keep.dedup();
        let mut body = super::kernel::subset(&self.body, &keep, false);
        super::boolean::assemble_shells(&mut body)?;
        let src = keep.iter().map(|&f| self.src[f].clone()).collect();
        Ok(Tracked { body, src })
    }

    pub fn into_result(mut self, k: &mut NativeKernel, input: BodyId) -> OpResult {
        // Edges split where tools met and left joining in a line or circle between the same
        // faces become one again (faces keep their indices).
        merge_edges(&mut self.body);
        let mut history = History::default();
        let n = k.get(input).map(|b| b.faces.len()).unwrap_or(0);
        let mut seen = vec![false; n];
        for (f, srcs) in self.src.iter().enumerate() {
            for s in srcs {
                match s {
                    FaceSrc::Input(i) => {
                        history.modified.push((
                            FaceId(f as u64),
                            InputFace {
                                body: input,
                                face: FaceId(*i as u64),
                            },
                        ));
                        if *i < n {
                            seen[*i] = true;
                        }
                    }
                    FaceSrc::New(o) => history.generated.push((FaceId(f as u64), *o)),
                }
            }
        }
        for (i, s) in seen.iter().enumerate() {
            if !s {
                history.deleted.push(InputFace {
                    body: input,
                    face: FaceId(i as u64),
                });
            }
        }
        let id = k.insert(self.body);
        OpResult {
            bodies: vec![id],
            history,
        }
    }
}

/// An edge to blend: its section for the faces' angle, and whether its faces are swapped.
pub type EdgeBlend = (EdgeId, Box<dyn Fn(f64) -> SectionKind>, bool);

/// The section curve of a blend between the contact points.
#[derive(Debug, Clone, Copy)]
pub enum SectionKind {
    /// A circular fillet of radius r (contact distances from the faces' angle).
    Circle { r: f64 },
    /// Straight, with the contact distances on the first and second face.
    Chamfer { d1: f64, d2: f64 },
    /// A conic with the circle's contact points for radius r and shape `rho`.
    Conic { r: f64, rho: f64 },
    /// A curvature-continuous quintic with the circle's contact points.
    Curvature { r: f64, m: f64 },
    /// Contact points of the circle for r1 (first face) and r2 (second), the circle's weight.
    Asymmetric { r1: f64, r2: f64 },
}

/// The geometry of an edge's blend in the plane square to it, in that plane's 2D coordinates:
/// the corner at the origin, the unit directions into the faces and whether the edge is
/// convex.
#[derive(Debug, Clone, Copy)]
pub struct Corner2 {
    pub u1: V2,
    pub u2: V2,
    pub convex: bool,
}

/// The loop (in 2D) of the region a blend removes (convex) or adds (concave), and the index of
/// the section piece within it.
pub fn section_loop(c: &Corner2, kind: SectionKind, src: Src) -> Result<(Vec<Piece>, P2, P2)> {
    let cos = c.u1.dot(&c.u2).clamp(-1.0, 1.0);
    let theta = cos.acos();
    if theta < tol::ANGULAR_POINT || theta > PI - tol::ANGULAR_POINT {
        return Err(KernelError::InvalidGeometry {
            op: "blend",
            why: "the faces at the edge are tangent: nothing to blend".into(),
        });
    }
    let bis = (c.u1 + c.u2).normalize();
    let tan_half = (theta / 2.0).tan();
    let (a1, a2) = match kind {
        SectionKind::Circle { r }
        | SectionKind::Conic { r, .. }
        | SectionKind::Curvature { r, .. } => (r / tan_half, r / tan_half),
        SectionKind::Chamfer { d1, d2 } => (d1, d2),
        SectionKind::Asymmetric { r1, r2 } => (r1 / tan_half, r2 / tan_half),
    };
    let p = P2::origin();
    let c1 = p + c.u1 * a1;
    let c2 = p + c.u2 * a2;
    let pt = |q: P2| P3::new(q.x, q.y, 0.0);
    let section: Curve = match kind {
        SectionKind::Circle { r } => {
            let center = p + bis * (r / (theta / 2.0).sin());
            let x = (c1 - center).normalize();
            let to2 = (c2 - center).normalize();
            // The short arc from c1 to c2.
            let y0 = V2::new(-x.y, x.x);
            let y = if y0.dot(&to2) >= 0.0 { y0 } else { -y0 };
            let sweep = to2.dot(&y).atan2(to2.dot(&x));
            let circle = Curve::Circle {
                c: pt(center),
                x: V3::new(x.x, x.y, 0.0),
                y: V3::new(y.x, y.y, 0.0),
                r,
            };
            return finish(circle, 0.0, sweep, c1, c2, c, src);
        }
        SectionKind::Chamfer { .. } => {
            let d = c2 - c1;
            let l = d.norm();
            return finish(
                Curve::Line {
                    o: pt(c1),
                    d: V3::new(d.x / l, d.y / l, 0.0),
                },
                0.0,
                l,
                c1,
                c2,
                c,
                src,
            );
        }
        SectionKind::Conic { rho, .. } => {
            if !(rho > 0.0 && rho < 1.0) {
                return Err(KernelError::InvalidParameter(
                    "The conic's rho must lie between 0 and 1".into(),
                ));
            }
            Curve::Nurbs(std::sync::Arc::new(NurbsCurve::bezier(
                &[pt(c1), pt(p), pt(c2)],
                &[1.0, rho / (1.0 - rho), 1.0],
            )))
        }
        SectionKind::Asymmetric { .. } => {
            let w = (theta / 2.0).sin();
            Curve::Nurbs(std::sync::Arc::new(NurbsCurve::bezier(
                &[pt(c1), pt(p), pt(c2)],
                &[1.0, w, 1.0],
            )))
        }
        SectionKind::Curvature { m, .. } => {
            if !(m > 0.0 && m <= 1.0) {
                return Err(KernelError::InvalidParameter(
                    "The curvature magnitude must lie in (0, 1]".into(),
                ));
            }
            let a = |q: P2, f: f64| q + (p - q) * f;
            let poles = [c1, a(c1, m / 2.0), a(c1, m), a(c2, m), a(c2, m / 2.0), c2].map(pt);
            Curve::Nurbs(std::sync::Arc::new(NurbsCurve::bezier(&poles, &[1.0; 6])))
        }
    };
    finish(section, 0.0, 1.0, c1, c2, c, src)
}

/// The section loop of a circular blend whose centre and contact points are known (solved on
/// curved faces): the short arc from `c1` to `c2` about `centre`.
fn arc_section_loop(
    c: &Corner2,
    centre: P2,
    c1: P2,
    c2: P2,
    r: f64,
    src: Src,
) -> Result<Vec<Piece>> {
    let x = (c1 - centre).normalize();
    let to2 = (c2 - centre).normalize();
    let y0 = V2::new(-x.y, x.x);
    let y = if y0.dot(&to2) >= 0.0 { y0 } else { -y0 };
    let sweep = to2.dot(&y).atan2(to2.dot(&x));
    let circle = Curve::Circle {
        c: P3::new(centre.x, centre.y, 0.0),
        x: V3::new(x.x, x.y, 0.0),
        y: V3::new(y.x, y.y, 0.0),
        r,
    };
    Ok(finish(circle, 0.0, sweep, c1, c2, c, src)?.0)
}

/// The centre of the ball of radius `r` rolling on both faces in the plane through `p` square
/// to `tan` (inside both for a convex edge, outside for a concave one), and its contact points:
/// Newton on the signed distances from the exact surfaces, started from the faces' tangent
/// lines' answer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ball_in_plane(
    f1: &Face,
    f2: &Face,
    p: &P3,
    tan: &V3,
    w1: &V3,
    w2: &V3,
    r: f64,
    convex: bool,
) -> Option<(P3, P3, P3)> {
    let s = if convex { -1.0 } else { 1.0 };
    let theta = w1.dot(w2).clamp(-1.0, 1.0).acos();
    if theta < tol::ANGULAR_POINT {
        return None;
    }
    let bis = (w1 + w2).normalize();
    let mut x = p + bis * (r / (theta / 2.0).sin());
    for _ in 0..60 {
        let (d1, g1, _) = out_dist(f1, &x);
        let (d2, g2, _) = out_dist(f2, &x);
        let m = nalgebra::Matrix3::from_rows(&[g1.transpose(), g2.transpose(), tan.transpose()]);
        let rhs = V3::new(s * r - d1, s * r - d2, -(x - p).dot(tan));
        let dx = m.try_inverse()? * rhs;
        x += dx;
        if dx.norm() < 1e-13 * (1.0 + x.coords.norm()) {
            break;
        }
    }
    let (d1, _, q1) = out_dist(f1, &x);
    let (d2, _, q2) = out_dist(f2, &x);
    let ok = (d1 - s * r).abs() < tol::REL * (1.0 + r)
        && (d2 - s * r).abs() < tol::REL * (1.0 + r)
        && (x - p).dot(tan).abs() < tol::REL * (1.0 + r);
    // The contacts on the faces' sides of the edge (not a far branch of a curved surface).
    let near = (q1 - p).dot(w1) > 0.0 && (q2 - p).dot(w2) > 0.0;
    (ok && near).then_some((x, q1, q2))
}

fn finish(
    section: Curve,
    t0: f64,
    t1: f64,
    c1: P2,
    c2: P2,
    c: &Corner2,
    src: Src,
) -> Result<(Vec<Piece>, P2, P2)> {
    let (t0, t1, rev) = if t1 >= t0 {
        (t0, t1, false)
    } else {
        (t1, t0, true)
    };
    let pt = |q: P2| P3::new(q.x, q.y, 0.0);
    let bis = (c.u1 + c.u2).normalize();
    let size = (c1.coords.norm()).max(c2.coords.norm());
    // The corner pushed out of the wedge, so no tool face lies on a body face.
    let pc = P2::origin() - bis * (size * 0.5 + 1e-3);
    let line = |a: P2, b: P2| {
        let d = b - a;
        let l = d.norm();
        Piece {
            c: Curve::Line {
                o: pt(a),
                d: V3::new(d.x / l, d.y / l, 0.0),
            },
            t0: 0.0,
            t1: l,
            rev: false,
            src: Src::Curve {
                region: u64::MAX,
                curve: u64::MAX,
            },
            more: vec![],
        }
    };
    let sec = Piece {
        c: section,
        t0,
        t1,
        rev,
        src,
        more: vec![],
    };
    let mut lp = vec![line(pc, c1), sec, line(c2, pc)];
    if super::profile::loop_area(&lp) < 0.0 {
        lp = super::profile::reverse_loop(&lp);
    }
    Ok((lp, c1, c2))
}

/// The faces on either side of an edge, with the coedges.
pub(crate) fn edge_sides(b: &Body, e: usize) -> Result<[(usize, bool); 2]> {
    let mut out = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        for l in &f.loops {
            for c in l {
                if c.edge == e {
                    out.push((fi, c.fwd));
                }
            }
        }
    }
    if out.len() != 2 || out[0].0 == out[1].0 {
        return Err(KernelError::InvalidParameter(
            "Only edges between two faces can be blended".into(),
        ));
    }
    Ok([out[0], out[1]])
}

/// The outward normal and the unit direction into the face, square to the edge, at the edge's
/// parameter t (the coedge's p-curve gives the face's uv there).
fn face_dirs(b: &Body, fi: usize, e: usize, fwd: bool, t: f64) -> (V3, V3) {
    let f = &b.faces[fi];
    let c = f
        .loops
        .iter()
        .flatten()
        .find(|c| c.edge == e && c.fwd == fwd)
        .unwrap();
    let uv = c.pc.point(t);
    let n = f.surf.normal(uv.x, uv.y) * if f.sense { 1.0 } else { -1.0 };
    let d = b.edges[e].curve.tangent(t).normalize() * if fwd { 1.0 } else { -1.0 };
    (n, n.cross(&d))
}

/// An edge's blend tool and whether it is removed (convex) or added (concave).
pub struct Tool {
    pub body: Body,
    pub src: Vec<Vec<FaceSrc>>,
    pub convex: bool,
    /// (corner point, faces' outward normals, ball centre) at each end, for corners.
    pub ends: [(P3, V3, V3); 2],
}

/// Builds the tool of one edge. `kind_at` gives the section for the edge's faces' angle
/// (normals φ apart) and which face is first.
pub fn edge_tool(
    b: &Body,
    input: BodyId,
    e: usize,
    kind: &dyn Fn(f64) -> SectionKind,
    swap: bool,
) -> Result<Tool> {
    edge_tool_range(b, input, e, kind, swap, (0.0, 1.0), (true, true))
}

/// The tool over the fraction `range` of the edge, extended past the ends where `extend` says.
pub fn edge_tool_range(
    b: &Body,
    input: BodyId,
    e: usize,
    kind: &dyn Fn(f64) -> SectionKind,
    swap: bool,
    range: (f64, f64),
    extend: (bool, bool),
) -> Result<Tool> {
    let ed = &b.edges[e];
    if ed.degenerate {
        return Err(KernelError::InvalidParameter(
            "A degenerate edge can't be blended".into(),
        ));
    }
    let [mut s1, mut s2] = edge_sides(b, e)?;
    if swap {
        std::mem::swap(&mut s1, &mut s2);
    }
    let tm = ed.mid_t();
    let (n1, w1) = face_dirs(b, s1.0, e, s1.1, tm);
    let (n2, w2) = face_dirs(b, s2.0, e, s2.1, tm);
    let convex = w2.dot(&n1) < 0.0;
    let phi = n1.dot(&n2).clamp(-1.0, 1.0).acos();
    let section = kind(phi);
    let src = Src::Edge {
        body: input.0,
        edge: e as u64,
    };
    let (f1, f2) = (&b.faces[s1.0].surf, &b.faces[s2.0].surf);
    match (&ed.curve, f1, f2) {
        (Curve::Line { d, .. }, Surface::Plane { .. }, Surface::Plane { .. }) => {
            let t = d.normalize();
            // Section plane square to the edge at its start point.
            let p0 = ed.point(ed.t[0]);
            let x = w1;
            let y = t.cross(&x);
            let plane = Plane {
                origin: p0,
                x_dir: nalgebra::Unit::new_normalize(x),
                normal: nalgebra::Unit::new_normalize(t),
            };
            let to2 = |v: &V3| V2::new(v.dot(&x), v.dot(&y));
            let corner = Corner2 {
                u1: to2(&w1).normalize(),
                u2: to2(&w2).normalize(),
                convex,
            };
            let (lp, _, _) = section_loop(&corner, section, src)?;
            let len = (ed.t[1] - ed.t[0]) * d.norm();
            let size = lp
                .iter()
                .map(|p| p.start().coords.norm())
                .fold(0.0, f64::max);
            let ext = size * 2.0 + 1.0;
            let mf = MergedFace {
                outer: lp,
                holes: vec![],
                regions: vec![u64::MAX],
            };
            let kind_ = Prism { plane, dir: t };
            let s0 = range.0 * len - if extend.0 { ext } else { 0.0 };
            let s1 = range.1 * len + if extend.1 { ext } else { 0.0 };
            let built = sweep::sweep_full(&[mf], &[], &kind_, s0, s1, false, true)?;
            let src = tool_src(&built);
            let ends = corner_info(b, ed, &n1, &n2, &w1, &w2, section);
            Ok(Tool {
                body: built.body,
                src,
                convex,
                ends,
            })
        }
        (
            Curve::Circle {
                c, x: cx, y: cy, ..
            },
            _,
            _,
        ) => {
            // A rim: one face a plane square to the axis, the other a coaxial surface of
            // revolution; the section is the same all round, so the tool is a revolve.
            let axis_dir = cx.cross(cy).normalize();
            let coaxial = |s: &Surface| match s {
                Surface::Plane { f } => f.x.cross(&f.y).cross(&axis_dir).norm() < tol::ANGULAR,
                _ => s.axis().is_some_and(|(o, z)| {
                    z.cross(&axis_dir).norm() < tol::ANGULAR
                        && (o - c).cross(&axis_dir).norm() < 1e-7
                }),
            };
            if !coaxial(f1) || !coaxial(f2) {
                return edge_tool_general(b, input, e, kind, swap, range, extend);
            }
            let q = ed.point(ed.t[0]);
            let t = ed.curve.tangent(ed.t[0]).normalize();
            let (n1, w1) = face_dirs(b, s1.0, e, s1.1, ed.t[0]);
            let (n2, w2) = face_dirs(b, s2.0, e, s2.1, ed.t[0]);
            let _ = (n1, n2);
            // Meridian plane through q: x radial, normal along the tangent.
            let radial = (q - c).normalize();
            let plane = Plane {
                origin: q,
                x_dir: nalgebra::Unit::new_normalize(radial),
                normal: nalgebra::Unit::new_normalize(t),
            };
            let y = t.cross(&radial);
            let to2 = |v: &V3| V2::new(v.dot(&radial), v.dot(&y));
            let corner = Corner2 {
                u1: to2(&w1).normalize(),
                u2: to2(&w2).normalize(),
                convex,
            };
            // A face curved in the meridian plane (a sphere, a torus): the rolling ball's centre
            // solved on the exact surfaces, not on the faces' tangent lines at the edge.
            let curved = |s: &Surface| matches!(s, Surface::Sphere { .. } | Surface::Torus { .. });
            let lp = match section {
                SectionKind::Circle { r } if curved(f1) || curved(f2) => {
                    let (fa, fb) = (&b.faces[s1.0], &b.faces[s2.0]);
                    let (centre, k1, k2) = ball_in_plane(fa, fb, &q, &t, &w1, &w2, r, convex)
                        .ok_or_else(|| KernelError::TooLarge {
                            op: "blend",
                            why: "the blend is too large for the faces at the edge".into(),
                        })?;
                    let p2 = |x: &P3| P2::new((x - q).dot(&radial), (x - q).dot(&y));
                    arc_section_loop(&corner, p2(&centre), p2(&k1), p2(&k2), r, src)?
                }
                _ => section_loop(&corner, section, src)?.0,
            };
            let mf = MergedFace {
                outer: lp,
                holes: vec![],
                regions: vec![u64::MAX],
            };
            // Revolve frame: z the axis, x towards the section.
            let f = Frame {
                o: *c,
                x: radial,
                y: axis_dir.cross(&radial),
                z: axis_dir,
            };
            let kind_ = Revolve { plane, f };
            let closed = ed.closed();
            let span = (ed.t[1] - ed.t[0]).abs();
            let whole_range = range.0 <= 0.0 && range.1 >= 1.0;
            let (s0, s1, whole) = if closed && whole_range {
                (0.0, TAU, true)
            } else {
                // The angles the edge turns about the axis, from q.
                let sgn = if t.dot(&f.y) > 0.0 { 1.0 } else { -1.0 };
                let room = if closed {
                    0.0
                } else {
                    (TAU - span) / 2.0 - 1e-3
                };
                let ext = 0.2f64.min(room).max(0.0);
                let (a, c) = (
                    range.0 * span - if extend.0 { ext } else { 0.0 },
                    range.1 * span + if extend.1 { ext } else { 0.0 },
                );
                if sgn > 0.0 {
                    (a, c, false)
                } else {
                    (-c, -a, false)
                }
            };
            let built = sweep::sweep_full(&[mf], &[], &kind_, s0, s1, whole, true)?;
            let src = tool_src(&built);
            let z = (q, V3::zeros(), V3::zeros());
            Ok(Tool {
                body: built.body,
                src,
                convex,
                ends: [z, z],
            })
        }
        _ => edge_tool_general(b, input, e, kind, swap, range, extend),
    }
}

fn tool_src(built: &super::build::Built) -> Vec<Vec<FaceSrc>> {
    let mut src = vec![Vec::new(); built.body.faces.len()];
    for (f, o) in &built.origins {
        if let Origin::FromEdge { .. } = o {
            src[*f].push(FaceSrc::New(*o));
        }
    }
    src
}

fn corner_info(
    b: &Body,
    ed: &super::topo::Edge,
    _n1: &V3,
    _n2: &V3,
    _w1: &V3,
    _w2: &V3,
    _s: SectionKind,
) -> [(P3, V3, V3); 2] {
    let p0 = b.verts[ed.v[0]].p;
    let p1 = b.verts[ed.v[1]].p;
    [
        (p0, V3::zeros(), V3::zeros()),
        (p1, V3::zeros(), V3::zeros()),
    ]
}

/// The corner tool at a vertex where three filleted edges between planes meet with the same
/// radius: the block between the faces and the ball centre, less the ball.
pub fn corner_tool(
    b: &Body,
    v: usize,
    r: f64,
    vertex_origin: Origin,
) -> Result<Option<(Body, Vec<Vec<FaceSrc>>)>> {
    // The planar faces around the vertex.
    let p = b.verts[v].p;
    let mut normals: Vec<V3> = Vec::new();
    for f in &b.faces {
        let touches = f
            .loops
            .iter()
            .flatten()
            .any(|c| b.edges[c.edge].v.contains(&v));
        if !touches {
            continue;
        }
        let Surface::Plane { f: fr } = &f.surf else {
            return Ok(None);
        };
        let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
        if !normals.iter().any(|m| m.dot(&n) > 1.0 - tol::ANGULAR) {
            normals.push(n);
        }
    }
    if normals.len() != 3 {
        return Ok(None);
    }
    // Ball centre: n_i·(c − p) = −r.
    let m = nalgebra::Matrix3::from_rows(&[
        normals[0].transpose(),
        normals[1].transpose(),
        normals[2].transpose(),
    ]);
    let Some(inv) = m.try_inverse() else {
        return Ok(None);
    };
    let c = p + inv * V3::new(-r, -r, -r);
    // The block: between the planes through c square to the three edges (where their fillets
    // stop, tangent to the ball) and planes pushed out past p. (At a right-angled corner these
    // are the faces' planes moved in by r.)
    let mut dirs: Vec<V3> = Vec::new();
    for ed in b.edges.iter().filter(|x| !x.degenerate && x.v.contains(&v)) {
        if !matches!(ed.curve, Curve::Line { .. }) {
            return Ok(None);
        }
        // Along the edge towards the vertex (from its ends: a line may run either way).
        let other = if ed.v[0] == v { ed.v[1] } else { ed.v[0] };
        let towards = (p - b.verts[other].p).normalize();
        if !dirs.iter().any(|m| m.dot(&towards) > 1.0 - tol::ANGULAR) {
            dirs.push(towards);
        }
    }
    if dirs.len() != 3 {
        return Ok(None);
    }
    // Only a convex corner is bounded by where its fillets stop; elsewhere the faces' planes
    // moved in by r (the same at a right-angled corner).
    if !vertex_convex(b, v) {
        dirs = normals.clone();
    }
    let dm = nalgebra::Matrix3::from_rows(&[
        dirs[0].transpose(),
        dirs[1].transpose(),
        dirs[2].transpose(),
    ]);
    let Some(dinv) = dm.try_inverse() else {
        return Ok(None);
    };
    let push = r * 0.5 + 1e-3;
    let corner = |s: [bool; 3]| -> P3 {
        // s[i]: on the outer plane (d·(x − p) = push) or the inner one (d·(x − c) = 0).
        let rhs = V3::new(
            if s[0] {
                dirs[0].dot(&p.coords) + push
            } else {
                dirs[0].dot(&c.coords)
            },
            if s[1] {
                dirs[1].dot(&p.coords) + push
            } else {
                dirs[1].dot(&c.coords)
            },
            if s[2] {
                dirs[2].dot(&p.coords) + push
            } else {
                dirs[2].dot(&c.coords)
            },
        );
        P3::from(dinv * rhs)
    };
    let block = hexahedron(&[
        corner([false, false, false]),
        corner([true, false, false]),
        corner([true, true, false]),
        corner([false, true, false]),
        corner([false, false, true]),
        corner([true, false, true]),
        corner([true, true, true]),
        corner([false, true, true]),
    ])?;
    // The ball's axis along one face normal and its seam away from the corner, so the octant's
    // boundary arcs are its equator and two meridians (exact p-curves).
    let d = p - c;
    let z = normals[0];
    let side = d - z * d.dot(&z);
    // A corner that isn't square: the poles and seam turned away from it instead.
    let square = dirs.iter().all(|a| {
        dirs.iter()
            .all(|b2| (a - b2).norm() < tol::ANGULAR || a.dot(b2).abs() < tol::ANGULAR)
    });
    let ball = if square && side.norm() > tol::ROUNDOFF {
        ball_frame(c, r, z, -side.normalize())?
    } else {
        ball_towards(c, r, d)?
    };
    let r = boolean_bodies(&block, &ball, BoolOp::Subtract).inspect_err(|x| {
        if std::env::var("CADRS_BLEND_DEBUG").is_ok() {
            eprintln!(
                "corner tool at {p:?}: {x}; centre {c:?} r {r} dirs {dirs:?} square {square}"
            );
        }
    })?;
    let src = r
        .from
        .iter()
        .map(|s| {
            if s.iter().any(|(o, _)| *o == 1) {
                vec![FaceSrc::New(vertex_origin)]
            } else {
                vec![]
            }
        })
        .collect();
    Ok(Some((r.body, src)))
}

/// A solid ball; its seam and poles face away from the direction `away` (so a tool that keeps
/// the part of the sphere towards `away` has no seam or pole in it).
pub fn ball_body(c: P3, r: f64) -> Result<Body> {
    ball_towards(c, r, V3::z())
}

pub fn ball_towards(c: P3, r: f64, away: V3) -> Result<Body> {
    let d = away.normalize();
    ball_frame(c, r, any_perpendicular(&d), -d)
}

/// A ball with its axis along `z` and its seam (u = 0) towards `x` (square to z).
pub fn ball_frame(c: P3, r: f64, z: V3, x: V3) -> Result<Body> {
    let y = z.cross(&x);
    let plane = Plane {
        origin: c,
        x_dir: nalgebra::Unit::new_normalize(x),
        normal: nalgebra::Unit::new_normalize(-y),
    };
    let half = Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc {
                    center: nalgebra::Point2::new(0.0, 0.0),
                    radius: r,
                    start_angle: -PI / 2.0,
                    sweep: PI,
                    source: None,
                },
                Curve2::Line {
                    a: nalgebra::Point2::new(0.0, r),
                    b: nalgebra::Point2::new(0.0, -r),
                    source: None,
                },
            ],
        },
        holes: vec![],
        source: None,
    };
    let prof = Profile::new(plane, vec![half]);
    let regions = super::profile::regions(&prof)?;
    let mf = super::profile::merge_regions(&regions)?;
    let kind = Revolve {
        plane,
        f: Frame { o: c, x, y, z },
    };
    Ok(sweep::sweep_full(&mf, &[], &kind, 0.0, TAU, true, true)?.body)
}

/// A hexahedron with planar faces from its 8 corners: 0–3 the bottom quad, 4–7 the top one
/// above them in order.
pub fn hexahedron(c: &[P3; 8]) -> Result<Body> {
    let mut b = Body::default();
    for p in c {
        b.add_vertex(*p);
    }
    // Orient so the bottom is counter-clockwise seen from below (outward) when the top is above.
    let quads: [[usize; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let mut edges: std::collections::HashMap<(usize, usize), usize> = Default::default();
    let mut faces = Vec::new();
    for q in quads {
        let mut lp = Vec::new();
        let n = (c[q[1]] - c[q[0]]).cross(&(c[q[2]] - c[q[0]]));
        let f = Frame::from_xz(c[q[0]], c[q[1]] - c[q[0]], n);
        let surf = Surface::Plane { f };
        for i in 0..4 {
            let (a, bb) = (q[i], q[(i + 1) % 4]);
            let key = (a.min(bb), a.max(bb));
            let e = *edges.entry(key).or_insert_with(|| {
                let (line, len) = Curve::line(c[key.0], c[key.1]);
                b.add_edge(line, 0.0, len, key.0, key.1)
            });
            let fwd = a == key.0;
            let ed = &b.edges[e];
            let pc = super::pcurve::pcurve(&surf, &ed.curve, ed.t[0], ed.t[1], None);
            lp.push(super::topo::Coedge { edge: e, fwd, pc });
        }
        faces.push(super::topo::Face {
            surf,
            sense: true,
            loops: vec![lp],
        });
    }
    b.faces = faces;
    b.shells.push(super::topo::Shell {
        faces: (0..6).collect(),
        closed: true,
    });
    b.solids.push(super::topo::Solid { shells: vec![0] });
    // Outward: flip everything if the volume came out negative (the quads' signed volume,
    // each split in two triangles: they're planar).
    let volume: f64 = quads
        .iter()
        .map(|q| {
            c[q[0]].coords.dot(&c[q[1]].coords.cross(&c[q[2]].coords))
                + c[q[0]].coords.dot(&c[q[2]].coords.cross(&c[q[3]].coords))
        })
        .sum::<f64>()
        / 6.0;
    if volume < 0.0 {
        for f in &mut b.faces {
            Body::reverse_face(f);
        }
    }
    let _ = co_range;
    Ok(b)
}

/// Keeps the part of a tool on the side of the plane (through `o`, normal `n`) that `n`
/// points away from.
/// The face a planar face of `b` lies on, as (point, outward normal).
fn face_plane(b: &Body, fi: usize) -> Option<(P3, V3)> {
    let f = &b.faces[fi];
    let Surface::Plane { f: fr } = &f.surf else {
        return None;
    };
    Some((fr.o, fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 }))
}

/// The point on three planes (None when two are nearly parallel).
fn meet3(a: (P3, V3), b: (P3, V3), c: (P3, V3)) -> Option<P3> {
    let m = nalgebra::Matrix3::from_rows(&[a.1.transpose(), b.1.transpose(), c.1.transpose()]);
    let r = V3::new(
        a.1.dot(&a.0.coords),
        b.1.dot(&b.0.coords),
        c.1.dot(&c.0.coords),
    );
    let x = m.try_inverse()? * r;
    Some(P3::from(x))
}

/// The triangular corner face where the fill of concave edge `e` meets the cuts of `ca` and
/// `cb` at `v`: through the fill's face meeting each chamfer on that chamfer's other face (the
/// wall the fill leans on), and the chamfers' edges crossing on the face they share. As
/// (point, normal towards `v`: the side it cuts away).
fn corner_triangle(
    b: &Body,
    e: usize,
    ca: usize,
    cb: usize,
    v: usize,
    d1: f64,
    d2: f64,
) -> Option<(P3, V3)> {
    let shared = |x: usize, y: usize| -> Option<usize> {
        let (sx, sy) = (edge_sides(b, x).ok()?, edge_sides(b, y).ok()?);
        sx.iter()
            .map(|s| s.0)
            .find(|f| sy.iter().any(|t| t.0 == *f))
    };
    let fill = chamfer_plane(b, e, v, d1, d2)?;
    let (pa, pb) = (
        chamfer_plane(b, ca, v, d1, d2)?,
        chamfer_plane(b, cb, v, d1, d2)?,
    );
    let (wa, wb, top) = (
        face_plane(b, shared(e, ca)?)?,
        face_plane(b, shared(e, cb)?)?,
        face_plane(b, shared(ca, cb)?)?,
    );
    let p_a = meet3(pa, fill, wa)?;
    let p_b = meet3(pb, fill, wb)?;
    let q = meet3(pa, pb, top)?;
    let mut n = (p_b - p_a).cross(&(q - p_a));
    if n.norm() < tol::DEGENERATE {
        return None;
    }
    n = n.normalize();
    if n.dot(&(b.verts[v].p - p_a)) < 0.0 {
        n = -n;
    }
    Some((p_a, n))
}

/// The plane of a straight chamfer of edge `c` at its vertex `v`: through the points `d1` and
/// `d2` in from the edge on its two (flat) faces, with its normal pointing out of the material.
fn chamfer_plane(b: &Body, c: usize, v: usize, d1: f64, d2: f64) -> Option<(P3, V3)> {
    let ed = &b.edges[c];
    if !matches!(ed.curve, Curve::Line { .. }) {
        return None;
    }
    let [s1, s2] = edge_sides(b, c).ok()?;
    let p = b.verts[v].p;
    let t = ed.curve.tangent(ed.mid_t()).normalize();
    let side = |fi: usize, fwd: bool| -> Option<(V3, V3)> {
        let f = &b.faces[fi];
        let Surface::Plane { f: fr } = &f.surf else {
            return None;
        };
        let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
        // Into the face, square to the edge (material lies left of the coedge).
        let dir = t * if fwd { 1.0 } else { -1.0 };
        Some((n, n.cross(&dir).normalize()))
    };
    let ((n1, w1), (n2, w2)) = (side(s1.0, s1.1)?, side(s2.0, s2.1)?);
    let (q1, q2) = (p + w1 * d1, p + w2 * d2);
    let mut m = t.cross(&(q2 - q1)).normalize();
    if m.dot(&(n1 + n2)) < 0.0 {
        m = -m;
    }
    Some((q1, m))
}

/// Whether edge `e` is convex (the material's corner between its faces is under 180°), at
/// its middle.
pub(crate) fn edge_convex(b: &Body, e: usize) -> Option<bool> {
    let ed = &b.edges[e];
    if ed.degenerate {
        return None;
    }
    let [s1, s2] = edge_sides(b, e).ok()?;
    let p = ed.point(ed.mid_t());
    let normal = |fi: usize| {
        let f = &b.faces[fi];
        let (u, v) = f.surf.project(&p, None);
        f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 }
    };
    let (n1, n2) = (normal(s1.0), normal(s2.0));
    let dir = ed.curve.tangent(ed.mid_t()).normalize() * if s1.1 { 1.0 } else { -1.0 };
    // Into face 1, square to the edge (material lies left of a coedge about the normal).
    let w1 = n1.cross(&dir);
    let c = w1.dot(&n2);
    if c.abs() < tol::ANGULAR {
        None
    } else {
        Some(c < 0.0)
    }
}

/// Whether the faces at edge `e` join smoothly all along it (their normals the same at every
/// sample): the edge marks no corner, and a blend there has nothing to take off or fill.
fn smooth_edge(b: &Body, e: usize) -> bool {
    smooth_within(b, e, tol::ANGULAR_POINT)
}

/// Whether the faces at edge `e` join smoothly all along it, their normals within `angle`.
pub(crate) fn smooth_within(b: &Body, e: usize, angle: f64) -> bool {
    let ed = &b.edges[e];
    let Ok([s1, s2]) = edge_sides(b, e) else {
        return false;
    };
    if ed.degenerate {
        return false;
    }
    (0..=8).all(|i| {
        let t = ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / 8.0;
        let p = ed.point(t);
        let normal = |fi: usize| {
            let f = &b.faces[fi];
            let (u, v) = f.surf.project(&p, None);
            f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 }
        };
        let (n1, n2) = (normal(s1.0), normal(s2.0));
        n1.dot(&n2) > 0.0 && n1.cross(&n2).norm() < angle.sin()
    })
}

/// The picked edges less those whose faces join smoothly (OCCT's blends pass over them too);
/// when every one does, there is nothing to blend.
fn sharp_edges(b: &Body, edges: &[EdgeId]) -> Result<Vec<EdgeId>> {
    let kept: Vec<EdgeId> = edges
        .iter()
        .copied()
        .filter(|e| (e.0 as usize) >= b.edges.len() || !smooth_edge(b, e.0 as usize))
        .collect();
    if kept.is_empty() && !edges.is_empty() {
        return Err(KernelError::InvalidGeometry {
            op: "blend",
            why: "the faces at the edge are tangent: nothing to blend".into(),
        });
    }
    Ok(kept)
}

/// The picked edges less degenerate ones (a pole's, an apex's): OCCT's blends pass over them
/// (`BRepFilletAPI` adds no contour for one), and there's nothing to round there.
fn real_edges(b: &Body, edges: &[EdgeId]) -> Result<Vec<EdgeId>> {
    let kept: Vec<EdgeId> = edges
        .iter()
        .copied()
        .filter(|e| b.edges.get(e.0 as usize).is_none_or(|ed| !ed.degenerate))
        .collect();
    if kept.is_empty() && !edges.is_empty() {
        return Err(KernelError::InvalidParameter(
            "A degenerate edge can't be blended".into(),
        ));
    }
    Ok(kept)
}

/// Whether the solid is convex round vertex `v`: every edge there convex.
fn vertex_convex(b: &Body, v: usize) -> bool {
    b.edges
        .iter()
        .enumerate()
        .filter(|(_, x)| !x.degenerate && x.v.contains(&v))
        .all(|(i, _)| edge_convex(b, i) == Some(true))
}

/// Whether the edge running `out` away from vertex `v` carries on smoothly there into another
/// edge (not `except`): a blend stopping at `v` then ends square, with no extension along it.
fn continues_smoothly(b: &Body, v: usize, out: V3, except: &[usize]) -> bool {
    b.edges.iter().enumerate().any(|(i, o)| {
        if except.contains(&i) || o.degenerate || !o.v.contains(&v) {
            return false;
        }
        let d = if o.v[0] == v {
            o.curve.tangent(o.t[0])
        } else {
            -o.curve.tangent(o.t[1])
        };
        d.norm() > 0.0 && d.normalize().dot(&out) > 1.0 - tol::ANGULAR_POINT
    })
}

/// Where to cut a blend tool's extension past the end of its edge (or chain) at vertex `v`,
/// heading `out` away from the edge, as (point, normal of the side cut away), given the faces
/// the blend runs between. A tool removing material (`convex`) is cut flush with a flat end face
/// it would otherwise dig into (one facing back along the edge: the floor a chamfered wall
/// stands on); a tool adding material is cut at an end face it would stick out of, else square
/// to the edge.
fn end_cut(b: &Body, v: usize, out: V3, sides: &[usize], convex: bool) -> Option<(P3, V3)> {
    let p = b.verts[v].p;
    let mut plane: Option<(P3, V3)> = None;
    for (fi, f) in b.faces.iter().enumerate() {
        if sides.contains(&fi)
            || !f
                .loops
                .iter()
                .flatten()
                .any(|c| b.edges[c.edge].v.contains(&v))
        {
            continue;
        }
        if let Surface::Plane { f: fr } = &f.surf {
            let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
            if convex && n.dot(&out) < -tol::ANGULAR_POINT {
                plane = Some((p, -n));
            } else if !convex && n.dot(&out) > tol::ANGULAR_POINT {
                plane = Some((p, n));
            }
        }
    }
    if convex {
        plane
    } else {
        Some(plane.unwrap_or((p, out)))
    }
}

fn trim_tool(tool: &Body, src: &[Vec<FaceSrc>], o: P3, n: V3) -> Result<(Body, Vec<Vec<FaceSrc>>)> {
    let bb = tool.bbox();
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
    let kind = Prism { plane, dir: n };
    let hs = sweep::sweep(&plane, &mf, &kind, 0.0, size, false)?.body;
    let t = Tracked {
        body: tool.clone(),
        src: src.to_vec(),
    };
    let r = t.apply(&hs, &vec![Vec::new(); hs.faces.len()], BoolOp::Subtract)?;
    Ok((r.body, r.src))
}

/// The section kind for a fillet spec on an edge whose faces' normals are `phi` apart.
fn fillet_section(spec: &FilletSpec, phi: f64) -> SectionKind {
    let r = match spec.size {
        FilletSize::Radius(r) => r,
        FilletSize::Width(w) => w / (2.0 * (phi / 2.0).sin()),
    };
    match spec.profile {
        FilletProfile::Circular => SectionKind::Circle { r },
        FilletProfile::Conic { rho } => SectionKind::Conic { r, rho },
        FilletProfile::Curvature { magnitude } => SectionKind::Curvature { r, m: magnitude },
        FilletProfile::Asymmetric { second, flip } => {
            if flip {
                SectionKind::Asymmetric { r1: second, r2: r }
            } else {
                SectionKind::Asymmetric { r1: r, r2: second }
            }
        }
    }
}

/// Blends the edges (each with its section) and the corners where three circular fillets of
/// one radius meet between planes.
pub fn blend(
    k: &mut NativeKernel,
    body: BodyId,
    edges: &[EdgeBlend],
    corners: bool,
) -> Result<OpResult> {
    // For measuring what local trimming covers: no tool at all.
    if std::env::var_os("CADRS_LOCAL_FILLET_ONLY").is_some() {
        return Err(KernelError::Unsupported(
            "the blend tool (CADRS_LOCAL_FILLET_ONLY is set)",
        ));
    }
    let b = (**k.get(body)?).clone();
    let picked: Vec<usize> = edges.iter().map(|(e, _, _)| e.0 as usize).collect();
    for &e in &picked {
        if e >= b.edges.len() {
            return Err(KernelError::InvalidParameter(format!("unknown edge {e}")));
        }
    }
    if std::env::var_os("CADRS_BLEND_DEBUG").is_some() {
        let ef = b.edge_faces();
        for &e in &picked {
            let ed = &b.edges[e];
            let kinds: Vec<String> = ef[e]
                .iter()
                .map(|&f| format!("{f}:{:?}", b.faces[f].surf.kind()))
                .collect();
            eprintln!(
                "blend edge {e}: {} convex {:?} faces {kinds:?}",
                format!("{:?}", ed.curve)
                    .chars()
                    .take(40)
                    .collect::<String>(),
                edge_convex(&b, e)
            );
        }
    }
    check_overlaps(&b, edges)?;
    // Picked edges at each vertex.
    let mut at_vertex: std::collections::HashMap<usize, Vec<usize>> = Default::default();
    for &e in &picked {
        let ed = &b.edges[e];
        for v in ed.v {
            at_vertex.entry(v).or_default().push(e);
        }
    }
    let mut cur = Tracked::of(b.clone());
    let mut radius_at: std::collections::HashMap<usize, f64> = Default::default();
    // Corner faces where a fill meets two cuts (by vertex): the plane, its normal towards the
    // material it cuts away. The cuts there stay above it.
    let mut corner_planes: std::collections::HashMap<usize, (P3, V3)> = Default::default();
    let mut corner_vertices: Vec<usize> = Vec::new();
    // Tangent chains of picked edges (two picked edges meeting tangentially at a vertex, and
    // nothing else picked there): one tool each.
    let tangent_out = |e: usize, v: usize| -> V3 {
        let ed = &b.edges[e];
        if ed.v[0] == v {
            -ed.curve.tangent(ed.t[0]).normalize()
        } else {
            ed.curve.tangent(ed.t[1]).normalize()
        }
    };
    let continues = |e: usize, v: usize| -> Option<usize> {
        let at = at_vertex.get(&v)?;
        let others: Vec<usize> = at.iter().copied().filter(|&o| o != e).collect();
        if others.len() != 1 {
            return None;
        }
        let o = others[0];
        // Tangent: this edge's direction out of v equals the other's direction into it.
        (tangent_out(e, v).dot(&-tangent_out(o, v)) > 1.0 - tol::ANGULAR_POINT).then_some(o)
    };
    let mut in_chain = vec![false; b.edges.len()];
    let mut chain_tools: Vec<(Vec<(usize, bool)>, usize)> = Vec::new(); // (chain, index of the spec)
    for (k, (e, _, _)) in edges.iter().enumerate() {
        let e = e.0 as usize;
        if in_chain[e] || b.edges[e].closed() {
            continue;
        }
        // Walk back to the chain's start, then forwards.
        let mut start = (e, true);
        let mut seen = vec![e];
        loop {
            let (ce, fwd) = start;
            let v = if fwd {
                b.edges[ce].v[0]
            } else {
                b.edges[ce].v[1]
            };
            let Some(o) = continues(ce, v) else { break };
            if seen.contains(&o) {
                break;
            }
            seen.push(o);
            let ofwd = b.edges[o].v[1] == v;
            start = (o, ofwd);
        }
        let mut chain = vec![start];
        loop {
            let (ce, fwd) = *chain.last().unwrap();
            let v = if fwd {
                b.edges[ce].v[1]
            } else {
                b.edges[ce].v[0]
            };
            let Some(o) = continues(ce, v) else { break };
            if chain.iter().any(|c| c.0 == o) {
                break;
            }
            chain.push((o, b.edges[o].v[0] == v));
        }
        // Only chains with an edge the exact tools can't take (the exact tools are better
        // wherever they apply).
        let exact = |e: usize| -> bool {
            let ed = &b.edges[e];
            let Ok([s1, s2]) = edge_sides(&b, e) else {
                return false;
            };
            let (f1, f2) = (&b.faces[s1.0].surf, &b.faces[s2.0].surf);
            match &ed.curve {
                Curve::Line { .. } => {
                    matches!(f1, Surface::Plane { .. }) && matches!(f2, Surface::Plane { .. })
                }
                Curve::Circle { c, x, y, .. } => {
                    let axis = x.cross(y).normalize();
                    let coax = |s: &Surface| match s {
                        Surface::Plane { f } => f.x.cross(&f.y).cross(&axis).norm() < tol::ANGULAR,
                        _ => s.axis().is_some_and(|(o, z)| {
                            z.cross(&axis).norm() < tol::ANGULAR
                                && (o - c).cross(&axis).norm() < 1e-7
                        }),
                    };
                    coax(f1) && coax(f2)
                }
                _ => false,
            }
        };
        if chain.len() > 1 && chain.iter().any(|c| !exact(c.0)) {
            for c in &chain {
                in_chain[c.0] = true;
            }
            chain_tools.push((chain, k));
        }
    }
    for (chain, k) in &chain_tools {
        let (_, kind, swap) = &edges[*k];
        for &(e, _) in chain {
            check_fits(&b, e, kind.as_ref())?;
        }
        // Exact strips on planes and cylinders where it applies, else the lofted tool.
        // The chain's open ends, each with the direction away from the chain.
        let (first, last) = (chain[0], chain[chain.len() - 1]);
        let start_v = if first.1 {
            b.edges[first.0].v[0]
        } else {
            b.edges[first.0].v[1]
        };
        let end_v = if last.1 {
            b.edges[last.0].v[1]
        } else {
            b.edges[last.0].v[0]
        };
        let away = |ce: (usize, bool), at_start: bool| -> V3 {
            let ed = &b.edges[ce.0];
            let t = if ce.1 == at_start { ed.t[0] } else { ed.t[1] };
            let along = ed.curve.tangent(t).normalize() * if ce.1 { 1.0 } else { -1.0 };
            if at_start { -along } else { along }
        };
        let in_this: Vec<usize> = chain.iter().map(|c| c.0).collect();
        let open = start_v != end_v;
        // Ends running on smoothly into edges left as they are stop square, unextended; a
        // closed chain (smooth all round) has no ends: its tool closes on itself.
        let extend = (
            open && !continues_smoothly(&b, start_v, away(first, true), &in_this),
            open && !continues_smoothly(&b, end_v, away(last, false), &in_this),
        );
        let tool = chain_tool(&b, body, chain, kind.as_ref(), *swap, (0.0, 1.0), extend)?;
        // A tool that didn't close into a solid has no inside to take away (its boolean only
        // spends minutes finding so).
        if !tool.body.is_solid() {
            return Err(KernelError::InvalidResult {
                op: "blend",
                why: "the blend's tool isn't a solid".into(),
            });
        }
        let (mut tb, mut ts) = (tool.body, tool.src);
        // Cut the extensions at the end faces as for one edge.
        if open {
            for (ce, v, at_start, ext) in [
                (first, start_v, true, extend.0),
                (last, end_v, false, extend.1),
            ] {
                if !ext {
                    continue;
                }
                let out = away(ce, at_start);
                let sides: Vec<usize> = chain
                    .iter()
                    .flat_map(|c| {
                        edge_sides(&b, c.0)
                            .map(|s| [s[0].0, s[1].0])
                            .unwrap_or([usize::MAX; 2])
                    })
                    .collect();
                if let Some((o, n)) = end_cut(&b, v, out, &sides, tool.convex) {
                    let (nb, ns) = trim_tool(&tb, &ts, o, n)?;
                    tb = nb;
                    ts = ns;
                }
            }
        }
        let op = if tool.convex {
            BoolOp::Subtract
        } else {
            BoolOp::Union
        };
        cur = cur.apply(&tb, &ts, op)?;
    }
    // Fills (concave edges' blends, adding material) before cuts: the result is the solid with
    // the fills added, less the cuts (a chamfer along a top edge cuts the top of a fill in the
    // inside corner below it).
    let mut order: Vec<&EdgeBlend> = edges.iter().collect();
    order.sort_by_key(|(e, _, _)| edge_convex(&b, e.0 as usize) == Some(true));
    for (e, kind, swap) in order {
        if in_chain[e.0 as usize] {
            continue;
        }
        let e = e.0 as usize;
        let ed = b.edges[e].clone();
        // How each end meets the other picked edges there.
        let tangent_at = |end: usize| -> V3 {
            let tv = ed.curve.tangent(ed.t[end]).normalize();
            if end == 0 { -tv } else { tv } // away from the edge at this end
        };
        let other_dir = |o: usize, v: usize| -> V3 {
            let oe = &b.edges[o];
            if oe.v[0] == v {
                oe.curve.tangent(oe.t[0]).normalize()
            } else {
                -oe.curve.tangent(oe.t[1]).normalize()
            }
        };
        let mut extend = (true, true);
        // A closed edge smooth at its vertex (a closed spline rim) has no ends: its tool closes
        // on itself.
        if ed.closed() {
            let (t0, t1) = (ed.curve.tangent(ed.t[0]), ed.curve.tangent(ed.t[1]));
            if t0.normalize().dot(&t1.normalize()) > 1.0 - tol::ANGULAR {
                extend = (false, false);
            }
        }
        let mut plans: Vec<(usize, usize, Vec<usize>)> = Vec::new(); // (end, vertex, others)
        for (end, v) in ed.v.iter().enumerate() {
            if ed.closed() {
                continue;
            }
            // The other blended edges here of the same kind (a cut meets cuts, a fill fills): a
            // cut and a fill don't mitre, each ends as at a free end.
            let mine = edge_convex(&b, e);
            let others: Vec<usize> = at_vertex
                .get(v)
                .map(|x| {
                    x.iter()
                        .copied()
                        .filter(|&o| o != e && edge_convex(&b, o) == mine)
                        .collect()
                })
                .unwrap_or_default();
            // A tangent-continuing neighbour: no extension; the tools meet square to the edges.
            if others.len() == 1
                && tangent_at(end).dot(&other_dir(others[0], *v)) > 1.0 - tol::ANGULAR_FINE
            {
                if end == 0 {
                    extend.0 = false;
                } else {
                    extend.1 = false;
                }
                continue;
            }
            plans.push((end, *v, others));
        }
        // An end where the edge runs on smoothly into an edge left as it is: the blend stops
        // square there.
        for (end, v) in ed.v.iter().enumerate() {
            // (Blended neighbours are mitred instead.)
            let mut except = at_vertex.get(v).cloned().unwrap_or_default();
            except.push(e);
            if !ed.closed() && continues_smoothly(&b, *v, tangent_at(end), &except) {
                if end == 0 {
                    extend.0 = false;
                } else {
                    extend.1 = false;
                }
                plans.retain(|x| x.0 != end);
            }
        }
        let tool = edge_tool_range(&b, body, e, kind.as_ref(), *swap, (0.0, 1.0), extend)?;
        // The section's contact points must lie on the faces (the blend fits).
        check_fits(&b, e, kind.as_ref())?;
        let (mut tb, mut ts) = (tool.body, tool.src);
        for (end, v, others) in plans {
            let p = b.verts[v].p;
            let out = tangent_at(end);
            match others.len() {
                0 => {}
                1 => {
                    // Miter: the plane through p bisecting the two edges; its normal points to
                    // the other edge's side, which is cut away.
                    let od = other_dir(others[0], v);
                    let n = out + od;
                    if n.norm() < tol::ROUNDOFF {
                        continue;
                    }
                    // Chamfers where the solid is convex round the vertex: each tool, run on
                    // past it, already cuts exactly what its chamfer plane does there (the solid lies
                    // inside its faces' planes): no trim. Elsewhere each keeps its side of the
                    // plane bisecting the corner (an inside corner: the cuts meet in a valley).
                    if matches!(kind(PI / 2.0), SectionKind::Chamfer { .. }) && vertex_convex(&b, v)
                    {
                        continue;
                    }
                    let (nb, ns) = trim_tool(&tb, &ts, p, n.normalize())?;
                    tb = nb;
                    ts = ns;
                }
                2 if corners && matches!(ed.curve, Curve::Line { .. }) => {
                    // Three fillets: stop square to the edge at the ball centre.
                    if let SectionKind::Circle { r } = kind(PI / 2.0) {
                        radius_at.insert(v, r);
                        if !corner_vertices.contains(&v) {
                            corner_vertices.push(v);
                        }
                        let c = ball_centre(&b, v, r).unwrap_or(p);
                        let along = (c - p).dot(&out);
                        let (nb, ns) = trim_tool(&tb, &ts, p + out * along, out)?;
                        tb = nb;
                        ts = ns;
                    }
                }
                _ => {}
            }
        }
        // A fill ending where chamfers cut. One cut: its plane runs on over the fill's end (the
        // chamfer face carries straight on across the corner). Two cuts (the top edges either
        // side of a filled inside corner): as Onshape, a triangular corner face closes the
        // corner, through the points where the fill's face meets each chamfer on its wall and
        // where the two chamfers' edges cross on the face they share. The fill stops under it,
        // and the cuts don't go below it there (see `corner_planes`).
        if !tool.convex
            && let SectionKind::Chamfer { d1, d2 } = kind(PI / 2.0)
        {
            for v in ed.v.iter() {
                let cuts: Vec<usize> = at_vertex
                    .get(v)
                    .map(|x| {
                        x.iter()
                            .copied()
                            .filter(|&o| o != e && edge_convex(&b, o) == Some(true))
                            .collect()
                    })
                    .unwrap_or_default();
                match cuts.len() {
                    1 => {
                        if let Some((o, n)) = chamfer_plane(&b, cuts[0], *v, d1, d2) {
                            let (nb, ns) = trim_tool(&tb, &ts, o, n)?;
                            tb = nb;
                            ts = ns;
                        }
                    }
                    2 => {
                        if let Some((o, n)) = corner_triangle(&b, e, cuts[0], cuts[1], *v, d1, d2) {
                            let (nb, ns) = trim_tool(&tb, &ts, o, n)?;
                            tb = nb;
                            ts = ns;
                            corner_planes.insert(*v, (o, n));
                        }
                    }
                    _ => {}
                }
            }
        }
        if tool.convex {
            for v in ed.v.iter() {
                if let Some(&(o, n)) = corner_planes.get(v) {
                    let (nb, ns) = trim_tool(&tb, &ts, o, -n)?;
                    tb = nb;
                    ts = ns;
                }
            }
        }
        // Its extensions end at the end faces: flush where a tool removing material would dig
        // into one, and where a tool adding material would stick out of one.
        let [s1, s2] = edge_sides(&b, e)?;
        for (end, v) in ed.v.iter().enumerate() {
            if ed.closed() || (tool.convex && ((end == 0 && !extend.0) || (end == 1 && !extend.1)))
            {
                continue;
            }
            // Ends meeting other blended edges were mitred above; only free ends here. (Fills
            // too: two fills meeting round a convex corner of the wall between them run on past
            // it to their mitre, as Onshape's do, the corner filled up to the lower of their two
            // faces; cut flush at the wall's plane, the corner was left empty.)
            let mine = edge_convex(&b, e);
            let others = at_vertex.get(v).map_or(0, |x| {
                x.iter()
                    .filter(|&&o| o != e && edge_convex(&b, o) == mine)
                    .count()
            });
            if others > 0 {
                continue;
            }
            // A cut ending where a fill runs (a chamfered top edge over a filled inside corner):
            // its corner, pushed out past its faces for a clean boolean, would bite the fill.
            let fill_here = at_vertex.get(v).is_some_and(|x| {
                x.iter()
                    .any(|&o| o != e && edge_convex(&b, o) == Some(!tool.convex))
            });
            if tool.convex && fill_here {
                for (fi, _) in [s1, s2] {
                    if let Surface::Plane { f: fr } = &b.faces[fi].surf {
                        let nf = fr.x.cross(&fr.y) * if b.faces[fi].sense { 1.0 } else { -1.0 };
                        let (nb, ns) = trim_tool(&tb, &ts, b.verts[*v].p, nf)?;
                        tb = nb;
                        ts = ns;
                    }
                }
            }
            let tv = ed.curve.tangent(ed.t[end]).normalize();
            let out = if end == 0 { -tv } else { tv };
            if let Some((o, n)) = end_cut(&b, *v, out, &[s1.0, s2.0], tool.convex) {
                let (nb, ns) = trim_tool(&tb, &ts, o, n)?;
                tb = nb;
                ts = ns;
            }
        }
        let op = if tool.convex {
            BoolOp::Subtract
        } else {
            BoolOp::Union
        };
        cur = cur.apply(&tb, &ts, op)?;
    }
    for v in corner_vertices {
        let r = radius_at[&v];
        if let Some((tool, src)) = corner_tool(
            &b,
            v,
            r,
            Origin::FromVertex {
                body,
                vertex: VertexId(vertex_index(&b, v) as u64),
            },
        )? {
            cur = cur.apply(&tool, &src, BoolOp::Subtract)?;
        }
    }
    let cur = cur.with_solids_of_input(b.solids.len())?;
    // A solid must stay a solid (an open result means a tool went wrong).
    if !b.solids.is_empty() && cur.body.solids.is_empty() {
        return Err(KernelError::InvalidResult {
            op: "blend",
            why: "the blend left an open body".into(),
        });
    }
    Ok(cur.into_result(k, body))
}

/// The index of a vertex among the vertices the kernel reports (those with edges).
pub(crate) fn vertex_index(b: &Body, v: usize) -> usize {
    let mut used = vec![false; b.verts.len()];
    for e in &b.edges {
        used[e.v[0]] = true;
        used[e.v[1]] = true;
    }
    used[..v].iter().filter(|x| **x).count()
}

fn ball_centre(b: &Body, v: usize, r: f64) -> Option<P3> {
    let p = b.verts[v].p;
    let mut normals: Vec<V3> = Vec::new();
    for f in &b.faces {
        if !f
            .loops
            .iter()
            .flatten()
            .any(|c| b.edges[c.edge].v.contains(&v))
        {
            continue;
        }
        let Surface::Plane { f: fr } = &f.surf else {
            return None;
        };
        let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
        if !normals.iter().any(|m| m.dot(&n) > 1.0 - tol::ANGULAR) {
            normals.push(n);
        }
    }
    if normals.len() != 3 {
        return None;
    }
    let m = nalgebra::Matrix3::from_rows(&[
        normals[0].transpose(),
        normals[1].transpose(),
        normals[2].transpose(),
    ]);
    Some(p + m.try_inverse()? * V3::new(-r, -r, -r))
}

/// Refuses a blend whose contact lines would leave the faces.
fn check_fits(b: &Body, e: usize, kind: &dyn Fn(f64) -> SectionKind) -> Result<()> {
    let ed = &b.edges[e];
    let [s1, s2] = edge_sides(b, e)?;
    let tm = ed.mid_t();
    let (n1, w1) = face_dirs(b, s1.0, e, s1.1, tm);
    let (n2, w2) = face_dirs(b, s2.0, e, s2.1, tm);
    let phi = n1.dot(&n2).clamp(-1.0, 1.0).acos();
    let theta = w1.dot(&w2).clamp(-1.0, 1.0).acos();
    let (a1, a2) = match kind(phi) {
        SectionKind::Circle { r }
        | SectionKind::Conic { r, .. }
        | SectionKind::Curvature { r, .. } => (r / (theta / 2.0).tan(), r / (theta / 2.0).tan()),
        SectionKind::Chamfer { d1, d2 } => (d1, d2),
        SectionKind::Asymmetric { r1, r2 } => (r1 / (theta / 2.0).tan(), r2 / (theta / 2.0).tan()),
    };
    // The contact lines must reach the faces somewhere along the edge (a fillet running into a
    // hole beside the edge is the boolean's business, not a misfit).
    let mut fits = [false, false];
    for frac in [0.1, 0.3, 0.5, 0.7, 0.9] {
        let tm = ed.t[0] + (ed.t[1] - ed.t[0]) * frac;
        let p = ed.point(tm);
        for (side, (s, w, a)) in [(s1, w1, a1), (s2, w2, a2)].into_iter().enumerate() {
            if !(a > 0.0) || !a.is_finite() {
                return Err(KernelError::InvalidParameter(
                    "The blend's size must be positive".into(),
                ));
            }
            let q = p + w * a;
            let f = &b.faces[s.0];
            let polys = super::classify::face_polygons(b, f);
            let (u, v) = f.surf.project(&q, None);
            if (f.surf.eval(u, v) - q).norm() > tol::REL_POINT * (1.0 + a) {
                // Curved across the edge: the contact point isn't on the face's surface; leave the
                // check to the boolean.
                fits[side] = true;
                continue;
            }
            let inside = super::classify::periodic_candidates(&f.surf, u, v, &polys)
                .into_iter()
                .any(|uv| {
                    super::classify::in_face(&polys, &f.surf, uv, 1e-7)
                        != super::classify::InFace::Outside
                });
            if inside {
                fits[side] = true;
            }
        }
    }
    if !(fits[0] && fits[1]) {
        return Err(KernelError::TooLarge {
            op: "blend",
            why: "the blend is too large for the faces at the edge".into(),
        });
    }
    Ok(())
}

/// Blends of two edges that share a flat face but no vertex must not overlap on it: the strips
/// they take of it (from each edge into the face, as wide as its contact offset) may not meet.
/// (Edges nearer than the two offsets can blend away from each other: the corners either side
/// of a narrow hole.)
fn check_overlaps(b: &Body, edges: &[EdgeBlend]) -> Result<()> {
    // Whether a point of the face lies inside the strip edge `e` takes of face `fi` (coedge
    // direction `fwd`), `w` wide: beside the edge, within the strip's width, short of its far
    // side by a hair (strips that only touch don't overlap).
    let in_strip = |e: usize, fi: usize, fwd: bool, w: f64, p: &P3| {
        let ed = &b.edges[e];
        let t = ed.curve.closest(p, ed.t[0], ed.t[1]);
        if t <= ed.t[0].min(ed.t[1]) || t >= ed.t[0].max(ed.t[1]) {
            return false;
        }
        let (n, into) = face_dirs(b, fi, e, fwd, t);
        let v = p - ed.point(t);
        let across = v.dot(&into);
        v.dot(&n).abs() < 1e-6 * (1.0 + w) && across > 1e-7 && across < w - 1e-7
    };
    // Points of the strip: across it at a few widths, along it at a few places.
    let strip_points = |e: usize, fi: usize, fwd: bool, w: f64| -> Vec<P3> {
        let ed = &b.edges[e];
        let mut pts = Vec::new();
        for k in 1..16 {
            let t = ed.t[0] + (ed.t[1] - ed.t[0]) * k as f64 / 16.0;
            let (_, into) = face_dirs(b, fi, e, fwd, t);
            for s in [0.05, 0.25, 0.5, 0.75, 0.95] {
                pts.push(ed.point(t) + into * (w * s));
            }
        }
        pts
    };
    // Per edge: (planar face, contact offset on it, coedge direction) for both sides.
    type Side = (usize, f64, bool);
    let mut info: Vec<(usize, [Side; 2])> = Vec::new();
    for (e, kind, _) in edges {
        let e = e.0 as usize;
        let Ok([s1, s2]) = edge_sides(b, e) else {
            continue;
        };
        let tm = b.edges[e].mid_t();
        let (n1, w1) = face_dirs(b, s1.0, e, s1.1, tm);
        let (n2, w2) = face_dirs(b, s2.0, e, s2.1, tm);
        let phi = n1.dot(&n2).clamp(-1.0, 1.0).acos();
        let theta = w1.dot(&w2).clamp(-1.0, 1.0).acos();
        let (a1, a2) = match kind(phi) {
            SectionKind::Circle { r }
            | SectionKind::Conic { r, .. }
            | SectionKind::Curvature { r, .. } => {
                (r / (theta / 2.0).tan(), r / (theta / 2.0).tan())
            }
            SectionKind::Chamfer { d1, d2 } => (d1, d2),
            SectionKind::Asymmetric { r1, r2 } => {
                (r1 / (theta / 2.0).tan(), r2 / (theta / 2.0).tan())
            }
        };
        info.push((e, [(s1.0, a1, s1.1), (s2.0, a2, s2.1)]));
    }
    for i in 0..info.len() {
        for j in i + 1..info.len() {
            let (ei, si) = info[i];
            let (ej, sj) = info[j];
            let (a, c) = (&b.edges[ei], &b.edges[ej]);
            if a.v.iter().any(|v| c.v.contains(v)) {
                continue;
            }
            for &(fi, ai, fwd_i) in &si {
                for &(fj, aj, fwd_j) in &sj {
                    if fi != fj
                        || !matches!(b.faces[fi].surf, Surface::Plane { .. })
                        || !(ai + aj).is_finite()
                    {
                        continue;
                    }
                    let mut d = f64::INFINITY;
                    for k in 0..=16 {
                        let p = a.point(a.t[0] + (a.t[1] - a.t[0]) * k as f64 / 16.0);
                        let t = c.curve.closest(&p, c.t[0], c.t[1]);
                        d = d.min((c.point(t) - p).norm());
                    }
                    if d < ai + aj - 1e-7
                        && (strip_points(ei, fi, fwd_i, ai)
                            .iter()
                            .any(|p| in_strip(ej, fj, fwd_j, aj, p))
                            || strip_points(ej, fj, fwd_j, aj)
                                .iter()
                                .any(|p| in_strip(ei, fi, fwd_i, ai, p)))
                    {
                        return Err(KernelError::TooLarge { op: "blend", why: "the blends of two edges overlap: the size is too large for the face between them".into() });
                    }
                }
            }
        }
    }
    Ok(())
}

/// The kernel's fillet: every edge with its tangent chain (only the edges without
/// propagation).
/// Joins edges split for nothing ([`super::unify::merge_edges`]) and sizes the joined edges'
/// tolerances to their p-curves' true gaps (a joined edge's p-curves are the pieces' joined,
/// off it by as much as the pieces were off theirs).
fn merge_edges(b: &mut Body) {
    super::unify::merge_edges(b);
    b.fit_tolerances_searched(1e-4 * (1.0 + b.bbox().diagonal()));
}

/// The first defect of a local blend's result `out` of `input`: by the full check
/// ([`CheckLevel::Full`](super::topo::CheckLevel::Full)), or by the fast one where the input
/// itself doesn't pass the full one (its own defects carry over into the result).
fn local_problem(input: &Body, out: &Body) -> Option<String> {
    use super::topo::CheckLevel;
    // The full check on the faces the blend changed (the others are the input's, as they were),
    // the fast one everywhere; the input checked only when the result fails (its check computes
    // its volumes too). (None again for a result the local blend passed by the full check on
    // all its faces.)
    if super::fillet::fully_checked(out) {
        return None;
    }
    let full = out
        .check_changed(&changed_faces(input, out))
        .into_iter()
        .next()?;
    if input.check_with(CheckLevel::Full).is_empty() {
        return Some(full);
    }
    out.check_with(CheckLevel::Fast).into_iter().next()
}

/// Which faces of `out` aren't faces of `input` as they were: same surface, sense, loops,
/// vertices, edge ends and p-curves (bit for bit: a local operation copies the faces it leaves).
fn changed_faces(input: &Body, out: &Body) -> Vec<bool> {
    use std::hash::{Hash, Hasher};
    let print = |b: &Body, fi: usize| -> u64 {
        let f = &b.faces[fi];
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut put = |p: &P3| p.coords.iter().for_each(|x| x.to_bits().hash(&mut h));
        put(&f.surf.eval(0.31, 0.67));
        put(&f.surf.eval(-0.43, 1.29));
        let mut h2 = std::collections::hash_map::DefaultHasher::new();
        f.sense.hash(&mut h2);
        for l in &f.loops {
            l.len().hash(&mut h2);
            for c in l {
                let e = &b.edges[c.edge];
                let (t0, t1) = super::topo::co_range(e, c.fwd);
                let tm = 0.5 * (t0 + t1);
                for p in [b.verts[b.co_start(c)].p, e.point(tm)] {
                    p.coords.iter().for_each(|x| x.to_bits().hash(&mut h2));
                }
                for t in [t0, tm] {
                    let q = c.pc.point(t);
                    [q.x, q.y].iter().for_each(|x| x.to_bits().hash(&mut h2));
                }
                e.degenerate.hash(&mut h2);
            }
        }
        h.finish() ^ h2.finish().rotate_left(17)
    };
    let before: std::collections::HashSet<u64> =
        (0..input.faces.len()).map(|fi| print(input, fi)).collect();
    (0..out.faces.len())
        .map(|fi| !before.contains(&print(out, fi)))
        .collect()
}

/// Flat faces two of `edges` bound on opposite sides, parallel lines exactly two radii `r`
/// apart (the fillets on them meet across the face and consume it whole: a full round), when
/// two such faces meet: the width of one. A single full round builds as any fillet; rounds
/// meeting square at a corner between the same walls by [`full_round_corners`], others not.
fn consumed_face(b: &Body, edges: &[EdgeId], r: f64) -> Option<f64> {
    let consumed: Vec<(usize, f64)> = (0..b.faces.len())
        .filter_map(|f| consumed_width(b, f, edges, r).map(|w| (f, w)))
        .collect();
    let edges_of =
        |f: usize| -> Vec<usize> { b.faces[f].loops.iter().flatten().map(|c| c.edge).collect() };
    for (i, &(f, w)) in consumed.iter().enumerate() {
        let mine = edges_of(f);
        if consumed[i + 1..]
            .iter()
            .any(|&(g, _)| edges_of(g).iter().any(|e| mine.contains(e)))
        {
            return Some(w);
        }
    }
    None
}

/// The width of face `f` when it is flat and two of `edges` bound it on opposite sides exactly
/// two radii `r` apart.
fn consumed_width(b: &Body, f: usize, edges: &[EdgeId], r: f64) -> Option<f64> {
    let close = tol::POINT.max(tol::REL_POINT * r);
    let line = |e: usize| -> Option<(P3, V3)> {
        let ed = &b.edges[e];
        let Curve::Line { .. } = ed.curve else {
            return None;
        };
        let (p, q) = (ed.point(ed.t[0]), ed.point(ed.t[1]));
        let d = q - p;
        (d.norm() > tol::POINT).then(|| (p, d / d.norm()))
    };
    let face = &b.faces[f];
    if !matches!(face.surf, Surface::Plane { .. }) {
        return None;
    }
    let on: Vec<(P3, V3)> = face
        .loops
        .iter()
        .flatten()
        .filter(|c| edges.contains(&EdgeId(c.edge as u64)))
        .filter_map(|c| line(c.edge))
        .collect();
    for (i, (p, d)) in on.iter().enumerate() {
        for (q, d2) in &on[i + 1..] {
            if d.cross(d2).norm() > tol::ANGULAR_POINT {
                continue;
            }
            let w = q - p;
            let width = (w - d * w.dot(d)).norm();
            if (width - 2.0 * r).abs() <= close {
                return Some(width);
            }
        }
    }
    None
}

/// A flat face a full round consumes whole: two picked straight edges along it, parallel and
/// exactly two radii apart, between flat walls square to it that face away from each other,
/// and two straight ends square to them closing it.
struct RoundFace {
    /// The face.
    face: usize,
    /// The picked edges along it.
    sides: [usize; 2],
    /// The walls: the faces across `sides`.
    walls: [usize; 2],
    /// Its outward normal.
    n: V3,
    /// The unit direction of the round's axis.
    d: V3,
    /// A point on the axis: halfway between the walls, a radius under the face.
    c: P3,
    /// Where its two ends lie along `d` from `c`, the lower first.
    span: [f64; 2],
    /// The edges ending it there.
    ends: [usize; 2],
}

/// Whether point `p` (on face `f`'s surface) lies on the face.
fn on_face(b: &Body, f: usize, p: &P3) -> bool {
    let face = &b.faces[f];
    let polys = super::classify::face_polygons(b, face);
    let (u, v) = face.surf.project(p, None);
    super::classify::periodic_candidates(&face.surf, u, v, &polys)
        .into_iter()
        .any(|uv| {
            super::classify::in_face(&polys, &face.surf, uv, 1e-7)
                != super::classify::InFace::Outside
        })
}

/// Face `f` as a [`RoundFace`] of the `picked` edges' fillets of radius `r`, when it is one
/// (and its walls reach a radius under it all along, so the round cuts nothing beyond them).
fn round_face(b: &Body, f: usize, picked: &[usize], r: f64) -> Option<RoundFace> {
    let close = tol::POINT.max(tol::REL_POINT * r);
    let (_, n) = face_plane(b, f)?;
    let [lp] = &b.faces[f].loops[..] else {
        return None;
    };
    if lp.len() != 4 {
        return None;
    }
    let line = |e: usize| -> Option<(P3, V3)> {
        let ed = &b.edges[e];
        let Curve::Line { .. } = ed.curve else {
            return None;
        };
        let (p, q) = (ed.point(ed.t[0]), ed.point(ed.t[1]));
        let d = q - p;
        (d.norm() > tol::POINT).then(|| (p, d / d.norm()))
    };
    // The picked pair of opposite edges two radii apart (one pair only: a square face of
    // side 2r with all four edges picked has no direction).
    let pairs: Vec<(usize, usize, P3, V3, P3)> = [(0, 2), (1, 3)]
        .into_iter()
        .filter_map(|(i, j)| {
            let (a, c) = (lp[i].edge, lp[j].edge);
            if !picked.contains(&a) || !picked.contains(&c) {
                return None;
            }
            let ((p, d), (q, d2)) = (line(a)?, line(c)?);
            let w = q - p;
            let across = w - d * w.dot(&d);
            (d.cross(&d2).norm() <= tol::ANGULAR_POINT && (across.norm() - 2.0 * r).abs() <= close)
                .then_some((i, j, p, d, p + across * 0.5))
        })
        .collect();
    let [(i, j, _, d, mid)] = pairs[..] else {
        return None;
    };
    let sides = [lp[i].edge, lp[j].edge];
    let ends = [lp[(i + 1) % 4].edge, lp[(j + 1) % 4].edge];
    // The walls: flat, facing away from each other, square to the face and the axis.
    let wall = |e: usize| -> Option<(usize, V3)> {
        let [s1, s2] = edge_sides(b, e).ok()?;
        let g = if s1.0 == f { s2.0 } else { s1.0 };
        let (_, m) = face_plane(b, g)?;
        (m.dot(&n).abs() < tol::ANGULAR && m.dot(&d).abs() < tol::ANGULAR).then_some((g, m))
    };
    let ((w0, m0), (w1, m1)) = (wall(sides[0])?, wall(sides[1])?);
    let c = mid - n * r;
    let (p0, p1) = (
        b.edges[sides[0]].point(b.edges[sides[0]].t[0]),
        b.edges[sides[1]].point(b.edges[sides[1]].t[0]),
    );
    if w0 == w1
        || m0.dot(&m1) > -1.0 + tol::ANGULAR
        || m0.dot(&(p0 - c)) <= 0.0
        || m1.dot(&(p1 - c)) <= 0.0
    {
        return None;
    }
    // The ends square to the axis: both vertices of each the same distance along it.
    let mut at = [0.0; 2];
    for (k, &e) in ends.iter().enumerate() {
        let ed = &b.edges[e];
        let (a, z) = (
            (b.verts[ed.v[0]].p - c).dot(&d),
            (b.verts[ed.v[1]].p - c).dot(&d),
        );
        if (a - z).abs() > close || line(e).is_none() {
            return None;
        }
        at[k] = a;
    }
    let (span, ends) = if at[0] <= at[1] {
        ([at[0], at[1]], ends)
    } else {
        ([at[1], at[0]], [ends[1], ends[0]])
    };
    if span[1] - span[0] <= close {
        return None;
    }
    // The walls reach a radius under the face all along it (the round's tangent lines lie on
    // them).
    for (w, m) in [(w0, m0), (w1, m1)] {
        for k in 0..=4 {
            let s = span[0] + (span[1] - span[0]) * (0.01 + 0.98 * k as f64 / 4.0);
            for h in [0.0, 0.5 * r] {
                if !on_face(b, w, &(c + d * s + m * r + n * h)) {
                    return None;
                }
            }
        }
    }
    Some(RoundFace {
        face: f,
        sides,
        walls: [w0, w1],
        n,
        d,
        c,
        span,
        ends,
    })
}

/// The section of a full round's tool in its plane (x across between the walls, y up out of
/// the face, the axis at the origin): the strip between the walls from the axis to half a
/// radius above the face, less the half disc of the round (curve source 1).
fn round_section(plane: Plane, r: f64) -> Result<Vec<MergedFace>> {
    let top = r * 1.5 + 1e-3;
    let p2 = |a: f64, c: f64| nalgebra::Point2::new(a, c);
    let l = |a, c| Curve2::Line {
        a,
        b: c,
        source: None,
    };
    let region = Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc {
                    center: p2(0.0, 0.0),
                    radius: r,
                    start_angle: 0.0,
                    sweep: PI,
                    source: Some(1),
                },
                l(p2(-r, 0.0), p2(-r, top)),
                l(p2(-r, top), p2(r, top)),
                l(p2(r, top), p2(r, 0.0)),
            ],
        },
        holes: vec![],
        source: None,
    };
    super::profile::merge_regions(&super::profile::regions(&Profile::new(
        plane,
        vec![region],
    ))?)
}

/// The faces' sources of a full round's tool: the round's surface (swept by curve 1) `src`,
/// the rest none (they lie on the walls, past the face, or inside where tools meet).
fn round_src(built: &super::build::Built, src: &FaceSrc) -> Vec<Vec<FaceSrc>> {
    let mut out = vec![Vec::new(); built.body.faces.len()];
    for (f, o) in &built.origins {
        if let Origin::ProfileCurve { curve: 1, .. } = o {
            out[*f].push(src.clone());
        }
    }
    out
}

/// The tool of full round `rf` between `range` along its axis: its section swept straight.
fn round_prism(
    rf: &RoundFace,
    r: f64,
    range: [f64; 2],
    src: FaceSrc,
) -> Result<(Body, Vec<Vec<FaceSrc>>)> {
    // x = n × d across, so y = d × x is the face's normal.
    let x = rf.n.cross(&rf.d);
    let plane = Plane {
        origin: rf.c,
        x_dir: nalgebra::Unit::new_normalize(x),
        normal: nalgebra::Unit::new_normalize(rf.d),
    };
    let mf = round_section(plane, r)?;
    let built = sweep::sweep_full(
        &mf,
        &[],
        &Prism { plane, dir: rf.d },
        range[0],
        range[1],
        false,
        true,
    )?;
    let src = round_src(&built, &src);
    Ok((built.body, src))
}

/// The tool of the corner where a full round (face normal `n`, running `out` into the corner)
/// meets another turning down square to it, about the ball centre `centre`: the round's
/// section turned a quarter about the line through the centre across the walls, so the ball's
/// quarter sphere joins the two half cylinders.
fn round_corner(
    centre: P3,
    n: V3,
    out: V3,
    r: f64,
    src: FaceSrc,
) -> Result<(Body, Vec<Vec<FaceSrc>>)> {
    // The section plane holds the axis (z = n × out) and the start direction n: x = n
    // (radial, the section's y above), plane y = z (the section's x across).
    let z = n.cross(&out);
    let plane = Plane {
        origin: centre,
        x_dir: nalgebra::Unit::new_normalize(n),
        normal: nalgebra::Unit::new_normalize(-out),
    };
    // The same section as the straight tools', in (radial, across) coordinates: the strip
    // from the axis out to `top`, less the half disc (from (0, −r) round through (r, 0)).
    let top = r * 1.5 + 1e-3;
    let p2 = |a: f64, c: f64| nalgebra::Point2::new(a, c);
    let l = |a, c| Curve2::Line {
        a,
        b: c,
        source: None,
    };
    let region = Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc {
                    center: p2(0.0, 0.0),
                    radius: r,
                    start_angle: -PI / 2.0,
                    sweep: PI,
                    source: Some(1),
                },
                l(p2(0.0, r), p2(top, r)),
                l(p2(top, r), p2(top, -r)),
                l(p2(top, -r), p2(0.0, -r)),
            ],
        },
        holes: vec![],
        source: None,
    };
    let mf = super::profile::merge_regions(&super::profile::regions(&Profile::new(
        plane,
        vec![region],
    ))?)?;
    let kind = Revolve {
        plane,
        f: Frame {
            o: centre,
            x: n,
            y: out,
            z,
        },
    };
    let built = sweep::sweep_full(&mf, &[], &kind, 0.0, PI / 2.0, false, true)?;
    let src = round_src(&built, &src);
    Ok((built.body, src))
}

/// Fillets making full rounds that meet at corners (two flat faces each consumed by the
/// fillets of its two long edges, joined at a picked edge square to both, between the same
/// walls): as the rolling ball, each face becomes a half cylinder and each corner the ball's
/// quarter sphere. The rounds and corners are cut by exact tools (the round's section swept
/// along its axis, stopped at the corner's ball centre; turned a quarter about the ball at the
/// corner), then the other picked edges are filleted on the result as usual (an edge where a
/// round ran into a wall is now the half circle there). `None` when no rounds meet so (or not
/// in a way these tools build).
fn full_round_corners(
    k: &mut NativeKernel,
    body: BodyId,
    all: &[EdgeId],
    spec: &FilletSpec,
    r: f64,
) -> Result<Option<OpResult>> {
    let b = (**k.get(body)?).clone();
    let picked: Vec<usize> = all
        .iter()
        .map(|e| e.0 as usize)
        .filter(|&e| e < b.edges.len())
        .collect();
    let rounds: Vec<RoundFace> = (0..b.faces.len())
        .filter_map(|f| round_face(&b, f, &picked, r))
        .collect();
    let close = tol::POINT.max(tol::REL_POINT * r);
    // Each round's tool's range along its axis, and the corners: (round, its end, the edge
    // there, the ball centre).
    let mut ranges: Vec<[f64; 2]> = Vec::new();
    let mut corners: Vec<(usize, usize, usize, P3)> = Vec::new();
    for (i, rf) in rounds.iter().enumerate() {
        let mut range = rf.span;
        for end in 0..2 {
            let e = rf.ends[end];
            let sign = if end == 0 { -1.0 } else { 1.0 };
            let out = rf.d * sign;
            let Ok([s1, s2]) = edge_sides(&b, e) else {
                return Ok(None);
            };
            let g = if s1.0 == rf.face { s2.0 } else { s1.0 };
            match rounds.iter().find(|o| o.face == g) {
                // Another round across a picked end edge: a corner, the other turning down
                // square to this one between the same walls; both stop at the ball centre.
                Some(o) if picked.contains(&e) => {
                    let same_walls = o.walls.iter().all(|w| rf.walls.contains(w));
                    if !same_walls
                        || o.n.dot(&out) < 1.0 - tol::ANGULAR
                        || rf.n.dot(&o.d).abs() < 1.0 - tol::ANGULAR
                    {
                        return Ok(None);
                    }
                    range[end] = rf.span[end] - sign * r;
                    let centre = rf.c + rf.d * range[end];
                    let off = centre - o.c;
                    if (off - o.d * off.dot(&o.d)).norm() > close {
                        return Ok(None);
                    }
                    if !corners.iter().any(|x| x.2 == e) {
                        corners.push((i, end, e, centre));
                    }
                }
                // Rounds crossing at an edge left sharp: not built.
                Some(_) => return Ok(None),
                // A free end at a flat face square to the axis: one the round runs out of
                // (convex) is passed by half a radius; at a wall it runs into (concave) the
                // tool stops flush.
                None => {
                    let Some((_, m)) = face_plane(&b, g) else {
                        return Ok(None);
                    };
                    if m.dot(&out).abs() < 1.0 - tol::ANGULAR {
                        return Ok(None);
                    }
                    if m.dot(&out) > 0.0 {
                        range[end] += sign * 0.5 * r;
                    }
                }
            }
        }
        ranges.push(range);
    }
    if corners.is_empty() {
        return Ok(None);
    }
    // The rounds' and corners' tools (each corner's tool meets its rounds' flush on their
    // ends), subtracted in as few booleans as keep the tools of one boolean apart: each tool
    // joins the first batch none of whose tools' boxes it comes near.
    let mut tools: Vec<(Body, Vec<Vec<FaceSrc>>)> = Vec::new();
    for (rf, range) in rounds.iter().zip(&ranges) {
        tools.push(round_prism(rf, r, *range, FaceSrc::Input(rf.face))?);
    }
    for &(i, end, e, centre) in &corners {
        let rf = &rounds[i];
        let out = if end == 0 { -rf.d } else { rf.d };
        tools.push(round_corner(
            centre,
            rf.n,
            out,
            r,
            FaceSrc::New(Origin::FromEdge {
                body,
                edge: EdgeId(e as u64),
            }),
        )?);
    }
    let gap = r * 0.1 + close;
    let mut batches: Vec<(Vec<super::math::Bbox>, Body, Vec<Vec<FaceSrc>>)> = Vec::new();
    for (tool, src) in tools {
        let bb = tool.bbox().grow(gap);
        match batches
            .iter_mut()
            .find(|(boxes, _, _)| boxes.iter().all(|x| !x.overlaps(&bb)))
        {
            Some((boxes, body, s)) => {
                boxes.push(bb);
                body.append(&tool);
                s.extend(src);
            }
            None => batches.push((vec![bb], tool, src)),
        }
    }
    let mut cur = Tracked::of(b.clone());
    for (_, tool, src) in &batches {
        cur = cur.apply(tool, src, BoolOp::Subtract)?;
    }
    if cur.body.solids.len() != b.solids.len() {
        return Err(KernelError::InvalidResult {
            op: "fillet",
            why: "the full rounds split the solid".into(),
        });
    }
    let mid = cur.into_result(k, body);
    let mid_id = mid.bodies[0];
    // The other picked edges, on the result: the edges between the faces continuing theirs,
    // within a radius of where they were.
    let consumed: Vec<usize> = rounds
        .iter()
        .flat_map(|rf| rf.sides)
        .chain(corners.iter().map(|x| x.2))
        .collect();
    let rest: Vec<usize> = picked
        .iter()
        .copied()
        .filter(|e| !consumed.contains(e))
        .collect();
    if rest.is_empty() {
        return Ok(Some(mid));
    }
    let out = (**k.get(mid_id)?).clone();
    let ef = out.edge_faces();
    let continues = |g: usize, f: usize| {
        mid.history
            .modified
            .iter()
            .any(|(x, i)| x.0 as usize == g && i.face.0 as usize == f)
    };
    // (new edge, the picked edge it continues)
    let mut back: Vec<(usize, usize)> = Vec::new();
    for &e in &rest {
        let ed = &b.edges[e];
        let Ok([s1, s2]) = edge_sides(&b, e) else {
            continue;
        };
        let (f1, f2) = (s1.0, s2.0);
        let found: Vec<usize> = (0..out.edges.len())
            .filter(|&x| {
                let [g1, g2] = ef[x][..] else { return false };
                let between = (continues(g1, f1) && continues(g2, f2))
                    || (continues(g1, f2) && continues(g2, f1));
                let oe = &out.edges[x];
                let p = oe.point(oe.mid_t());
                between
                    && !oe.degenerate
                    && (ed.point(ed.curve.closest(&p, ed.t[0], ed.t[1])) - p).norm() <= r + close
            })
            .collect();
        if found.is_empty() {
            k.release(mid_id);
            return Err(KernelError::InvalidResult {
                op: "fillet",
                why: format!("edge {e} is gone after the full rounds"),
            });
        }
        back.extend(found.into_iter().map(|x| (x, e)));
    }
    let mapped: Vec<EdgeId> = back.iter().map(|&(x, _)| EdgeId(x as u64)).collect();
    let res = fillet(
        k,
        mid_id,
        &mapped,
        &FilletSpec {
            propagate: false,
            ..*spec
        },
    );
    k.release(mid_id);
    let res = res?;
    // The history back to the input: through the full rounds' faces, and the fillets' faces
    // from the picked edges they continue.
    let mut history = History::default();
    for (f, i) in &res.history.modified {
        for (g, x) in &mid.history.modified {
            if *g == i.face {
                history.modified.push((*f, *x));
            }
        }
        for (g, o) in &mid.history.generated {
            if *g == i.face {
                history.generated.push((*f, *o));
            }
        }
    }
    let from_edge = |x: usize| Origin::FromEdge {
        body,
        edge: EdgeId(
            back.iter()
                .find(|(m, _)| *m == x)
                .map_or(rest[0], |(_, e)| *e) as u64,
        ),
    };
    for (f, o) in &res.history.generated {
        let o = match o {
            Origin::FromEdge { edge, .. } => from_edge(edge.0 as usize),
            // A corner patch: from the edge whose fillet ends there.
            Origin::FromVertex { vertex, .. } => {
                let at = back.iter().find(|(m, _)| {
                    out.edges[*m]
                        .v
                        .iter()
                        .any(|&v| vertex_index(&out, v) == vertex.0 as usize)
                });
                from_edge(at.map_or(back[0].0, |(m, _)| *m))
            }
            o => *o,
        };
        history.generated.push((*f, o));
    }
    for i in 0..b.faces.len() {
        if !history.modified.iter().any(|(_, x)| x.face.0 as usize == i) {
            history.deleted.push(InputFace {
                body,
                face: FaceId(i as u64),
            });
        }
    }
    Ok(Some(OpResult {
        bodies: res.bodies,
        history,
    }))
}

pub fn fillet(
    k: &mut NativeKernel,
    body: BodyId,
    edges: &[EdgeId],
    spec: &FilletSpec,
) -> Result<OpResult> {
    let (FilletSize::Radius(r) | FilletSize::Width(r)) = spec.size;
    if !(r > 0.0) {
        return Err(KernelError::InvalidParameter(
            "The fillet's size must be positive".into(),
        ));
    }
    let edges = &real_edges(k.get(body)?, edges)?[..];
    let mut all: Vec<EdgeId> = Vec::new();
    for &e in edges {
        let chain = if spec.propagate {
            k.tangent_chain(body, e)?
        } else {
            vec![e]
        };
        for c in chain {
            if !all.contains(&c) {
                all.push(c);
            }
        }
    }
    // Section profiles other than circular: straight edges between flat faces only.
    if !matches!(spec.profile, FilletProfile::Circular) {
        let b = k.get(body)?;
        for e in &all {
            let ed = &b.edges[e.0 as usize];
            let [s1, s2] = edge_sides(b, e.0 as usize)?;
            let flat = matches!(b.faces[s1.0].surf, Surface::Plane { .. })
                && matches!(b.faces[s2.0].surf, Surface::Plane { .. });
            if !matches!(ed.curve, Curve::Line { .. }) || !flat {
                return Err(KernelError::InvalidParameter("Conic, curvature and asymmetric fillets work on straight edges between flat faces".into()));
            }
        }
    }
    // A circular section of constant radius: by local trimming (no boolean) where that applies.
    // A width: the radius whose contacts lie that far apart, on each edge whose faces meet at
    // one angle (normals φ apart: r = w / (2 sin(φ/2))).
    let b = (**k.get(body)?).clone();
    // Full rounds meeting at corners: by their own exact tools, the other edges after.
    if let (FilletSize::Radius(r), FilletProfile::Circular) = (spec.size, spec.profile)
        && consumed_face(&b, &all, r).is_some()
    {
        match full_round_corners(k, body, &all, spec, r) {
            Ok(Some(res)) => {
                if !spec.allow_overflow {
                    check_overflow(k, body, &all, &res)?;
                }
                return Ok(res);
            }
            Ok(None) => {}
            Err(e) => {
                if std::env::var_os("CADRS_BLEND_DEBUG").is_some() {
                    eprintln!("full rounds meeting at corners: {e}");
                }
            }
        }
    }
    // (Local trimming passes over edges whose faces join smoothly.)
    let sharp = sharp_edges(&b, &all)?;
    let laws: Option<Vec<super::fillet::RadiusLaw>> = sharp
        .iter()
        .map(|e| {
            let e = e.0 as usize;
            let r = match spec.size {
                FilletSize::Radius(r) => r,
                // (Where the faces' angle varies along the edge, a law of radii.)
                FilletSize::Width(w) => match super::fillet::chamfer_angle(&b, e) {
                    Some(th) => w / (2.0 * ((PI - th) / 2.0).sin()),
                    None => return Some((e, super::fillet::width_law(&b, e, w)?)),
                },
            };
            Some((e, vec![(0.0, r), (1.0, r)]))
        })
        .collect();
    if let Some(laws) = laws.filter(|_| {
        matches!(spec.profile, FilletProfile::Circular)
            && std::env::var_os("CADRS_NO_LOCAL_FILLET").is_none()
    }) {
        // The result's edges merge where the trimming split them for nothing (as every blend's
        // do): it must stay valid.
        let local =
            super::fillet::fillet_edges_laws(&b, body, &laws, false).and_then(|(out, src)| {
                // (The local result's edges are merged and its tolerances fitted already.)
                match local_problem(&b, &out) {
                    Some(e) => Err(KernelError::InvalidResult {
                        op: "fillet",
                        why: format!("the local fillet isn't valid: {e}"),
                    }),
                    None => Ok((out, src)),
                }
            });
        match local {
            Ok((out, src)) => {
                let res = Tracked { body: out, src }.into_result(k, body);
                if !spec.allow_overflow {
                    check_overflow(k, body, &all, &res)?;
                }
                return Ok(res);
            }
            Err(e) => {
                if std::env::var_os("CADRS_LOCAL_FILLET_ONLY").is_some() {
                    return Err(e);
                }
                if std::env::var_os("CADRS_BLEND_DEBUG").is_some() {
                    eprintln!("local fillet: {e}; falling back to the blend tool");
                }
            }
        }
    }
    let spec = *spec;
    let list: Vec<EdgeBlend> = all
        .iter()
        .map(|&e| {
            let f: Box<dyn Fn(f64) -> SectionKind> =
                Box::new(move |phi| fillet_section(&spec, phi));
            (e, f, false)
        })
        .collect();
    let corners = matches!(spec.profile, FilletProfile::Circular)
        && matches!(spec.size, FilletSize::Radius(_));
    let res = match blend(k, body, &list, corners) {
        Ok(res) => res,
        // Neither method builds full rounds that meet at a corner: say so (OCCT's fillet does;
        // a slightly smaller radius leaves a strip of each face here).
        Err(e) => {
            let FilletSize::Radius(radius) = spec.size else {
                return Err(e);
            };
            return Err(consumed_face(&b, &all, radius).map_or(e, |width| KernelError::TooLarge {
                op: "fillet",
                why: format!(
                    "the R{radius} fillets make full rounds of two faces that meet (one {width:.3} mm wide, its \
                     opposite edges' fillets meeting across it): full rounds meeting at a corner aren't \
                     supported (a radius a little smaller works)"
                ),
            }));
        }
    };
    if !spec.allow_overflow {
        check_overflow(k, body, &all, &res)?;
    }
    Ok(res)
}

/// With Allow edge overflow off: a fillet face may only touch faces continuing the faces at its
/// edge or around the edge's end vertices.
fn check_overflow(k: &NativeKernel, body: BodyId, edges: &[EdgeId], r: &OpResult) -> Result<()> {
    let b = k.get(body)?;
    let out = k.get(r.bodies[0])?;
    let ef = out.edge_faces();
    for (f, o) in &r.history.generated {
        let Origin::FromEdge { edge, .. } = o else {
            continue;
        };
        let e = edge.0 as usize;
        // Allowed input faces: those at the edge and at its end vertices.
        let ed = &b.edges[e];
        let mut allowed: Vec<usize> = Vec::new();
        for (fi, face) in b.faces.iter().enumerate() {
            if face.loops.iter().flatten().any(|c| {
                c.edge == e
                    || b.edges[c.edge].v.contains(&ed.v[0])
                    || b.edges[c.edge].v.contains(&ed.v[1])
            }) {
                allowed.push(fi);
            }
        }
        let fi = f.0 as usize;
        for l in &out.faces[fi].loops {
            for c in l {
                for &g in &ef[c.edge] {
                    if g == fi {
                        continue;
                    }
                    let from: Vec<usize> = r
                        .history
                        .modified
                        .iter()
                        .filter(|(x, _)| x.0 as usize == g)
                        .map(|(_, i)| i.face.0 as usize)
                        .collect();
                    let fresh = r.history.generated.iter().any(|(x, _)| x.0 as usize == g);
                    if !fresh && !from.is_empty() && !from.iter().any(|x| allowed.contains(x)) {
                        return Err(KernelError::TooLarge {
                            op: "fillet",
                            why: "The fillet runs onto another face; turn on Allow edge overflow to let it".into(),
                        });
                    }
                }
            }
        }
        let _ = edges;
    }
    Ok(())
}

/// The kernel's chamfer.
pub fn chamfer(
    k: &mut NativeKernel,
    body: BodyId,
    edges: &[EdgeId],
    opts: &ChamferOpts,
) -> Result<OpResult> {
    let spec = opts.spec;
    let b = (**k.get(body)?).clone();
    let edges = &real_edges(&b, edges)?[..];
    // (Local trimming passes over edges whose faces join smoothly.)
    let sharp = sharp_edges(&b, edges)?;
    // Equal distances: by local trimming, their contacts that far along the faces
    // ([`super::fillet::chamfer_equal_edges`]), cut straight across.
    if let (ChamferSpec::EqualDistance(d), ChamferMeasure::Offset) = (spec, opts.measurement)
        && d > 0.0
        && std::env::var_os("CADRS_NO_LOCAL_FILLET").is_none()
    {
        let sharp: Vec<usize> = sharp.iter().map(|e| e.0 as usize).collect();
        if let Some(local) = super::fillet::chamfer_equal_edges(&b, body, &sharp, d) {
            let local = local.and_then(|(out, src)| {
                // (The local result's edges are merged and its tolerances fitted already.)
                match local_problem(&b, &out) {
                    Some(e) => Err(KernelError::InvalidResult {
                        op: "chamfer",
                        why: format!("the local chamfer isn't valid: {e}"),
                    }),
                    None => Ok((out, src)),
                }
            });
            match local {
                Ok((out, src)) => return Ok(Tracked { body: out, src }.into_result(k, body)),
                Err(e) => {
                    if std::env::var_os("CADRS_LOCAL_FILLET_ONLY").is_some() {
                        return Err(e);
                    }
                    if std::env::var_os("CADRS_BLEND_DEBUG").is_some() {
                        eprintln!("local chamfer: {e}; falling back to the blend tool");
                    }
                }
            }
        }
    }
    // The other specs and measures: by local trimming too, each edge with its distances along
    // its two faces (tangent ones converted once the edge's rule is chosen, as below). Two
    // distances that differ only along straight edges between flat faces; equal ones wherever
    // the faces meet at one angle (as the equal-distance chamfer above).
    if !matches!(
        (spec, opts.measurement),
        (ChamferSpec::EqualDistance(_), ChamferMeasure::Offset)
    ) && std::env::var_os("CADRS_NO_LOCAL_FILLET").is_none()
    {
        let dists: Option<Vec<(usize, [f64; 2])>> = sharp
            .iter()
            .map(|&e| {
                let ei = e.0 as usize;
                let ed = b.edges.get(ei)?;
                let sides = edge_sides(&b, ei).ok()?;
                let flip = opts.flip != opts.flipped.contains(&e);
                let (s0, s1) = if flip { (1, 0) } else { (0, 1) };
                let (d1, d2) = match spec {
                    ChamferSpec::EqualDistance(d) => (d, d),
                    ChamferSpec::TwoDistances(a, c) => (a, c),
                    ChamferSpec::DistanceAngle { distance, angle } => {
                        // As the blend tool's section: d₂ = d sin(a) / sin(π − a − θ).
                        let theta = super::fillet::chamfer_angle(&b, ei)?;
                        (
                            distance,
                            distance * angle.sin() / (PI - angle - theta).sin(),
                        )
                    }
                };
                let flat = sides
                    .iter()
                    .all(|s| matches!(b.faces[s.0].surf, Surface::Plane { .. }))
                    && matches!(ed.curve, Curve::Line { .. });
                let mut d = [0.0; 2];
                d[s0] = d1;
                d[s1] = d2;
                // (Unequal on a curved face: where its section doesn't change along the edge.)
                (d1 > 0.0
                    && d2 > 0.0
                    && (flat
                        || d1 == d2
                        || super::fillet::chamfer_offsets_along(&b, ei, d).is_some()))
                .then_some((ei, d))
            })
            .collect();
        if let Some(dists) = dists {
            let tangent = opts.measurement == ChamferMeasure::Tangent;
            let local = super::fillet::chamfer_edges_measured(&b, body, &dists, tangent).and_then(
                |(out, src)| {
                    // (The local result's edges are merged and its tolerances fitted already.)
                    match local_problem(&b, &out) {
                        Some(e) => Err(KernelError::InvalidResult {
                            op: "chamfer",
                            why: format!("the local chamfer isn't valid: {e}"),
                        }),
                        None => Ok((out, src)),
                    }
                },
            );
            match local {
                Ok((out, src)) => return Ok(Tracked { body: out, src }.into_result(k, body)),
                Err(e) => {
                    if std::env::var_os("CADRS_LOCAL_FILLET_ONLY").is_some() {
                        return Err(e);
                    }
                    if std::env::var_os("CADRS_BLEND_DEBUG").is_some() {
                        eprintln!("local chamfer: {e}; falling back to the blend tool");
                    }
                }
            }
        }
    }
    chamfer_tool(k, body, edges, opts)
}

/// The chamfer by a tool and boolean alone (the sections solved on the exact faces, their
/// contacts the distances along the faces), as [`chamfer`] makes it where local trimming
/// doesn't.
pub fn chamfer_tool(
    k: &mut NativeKernel,
    body: BodyId,
    edges: &[EdgeId],
    opts: &ChamferOpts,
) -> Result<OpResult> {
    let spec = opts.spec;
    let b = (**k.get(body)?).clone();
    let edges = &real_edges(&b, edges)?[..];
    let mut list: Vec<EdgeBlend> = Vec::new();
    for &e in edges {
        let flip = opts.flip != opts.flipped.contains(&e);
        let ei = e.0 as usize;
        // Tangent measurement: a face curving across the edge (a cylinder along a straight edge,
        // a sphere) of radius R takes the arc R·atan(d/R) round it on that side (its chord
        // 2R sin(atan(d/R)/2)).
        let chord = |side: usize, d: f64| -> f64 {
            if opts.measurement != ChamferMeasure::Tangent {
                return d;
            }
            let Ok(sides) = edge_sides(&b, ei) else {
                return d;
            };
            let f = &b.faces[sides[side].0];
            let ed = &b.edges[ei];
            let across = match (&f.surf, &ed.curve) {
                (Surface::Cylinder { f: fr, r }, Curve::Line { d: dl, .. })
                    if dl.normalize().cross(&fr.z).norm() < tol::ANGULAR =>
                {
                    Some(*r)
                }
                (Surface::Sphere { r, .. }, _) => Some(*r),
                _ => None,
            };
            // (The section's distance is along the face: the arc to the chord's end.)
            match across {
                Some(r) => r * (d / r).atan(),
                None => d,
            }
        };
        let (s0, s1) = if flip { (1usize, 0usize) } else { (0, 1) };
        let (d1, d2) = match spec {
            ChamferSpec::EqualDistance(d) => (chord(s0, d), chord(s1, d)),
            ChamferSpec::TwoDistances(a, c) => (chord(s0, a), chord(s1, c)),
            ChamferSpec::DistanceAngle { distance, angle } => (distance, f64::NAN + angle),
        };
        let kind: Box<dyn Fn(f64) -> SectionKind> = match spec {
            ChamferSpec::DistanceAngle { distance, angle } => Box::new(move |phi| {
                // The second distance from the angle at the first face: in the section triangle
                // with the faces' interior angle θ = π − φ, d2 = d sin(a) / sin(π − a − θ).
                let theta = PI - phi;
                let d2 = distance * angle.sin() / (PI - angle - theta).sin();
                SectionKind::Chamfer { d1: distance, d2 }
            }),
            _ => Box::new(move |_| SectionKind::Chamfer { d1, d2 }),
        };
        list.push((e, kind, flip));
    }
    blend(k, body, &list, false)
}

/// Onshape's Full round: the middle face replaced by a half cylinder tangent to two parallel
/// flat side faces (r = half their distance).
pub fn full_round(
    k: &mut NativeKernel,
    body: BodyId,
    side1: FaceId,
    center: FaceId,
    side2: FaceId,
) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    let plane_of = |f: FaceId| -> Result<(P3, V3)> {
        let face = b
            .faces
            .get(f.0 as usize)
            .ok_or_else(|| KernelError::InvalidParameter(format!("unknown {f:?}")))?;
        match &face.surf {
            Surface::Plane { f: fr } => Ok((
                fr.o,
                fr.x.cross(&fr.y) * if face.sense { 1.0 } else { -1.0 },
            )),
            _ => Err(KernelError::InvalidParameter(
                "A full round needs flat side and middle faces".into(),
            )),
        }
    };
    let (o1, n1) = plane_of(side1)?;
    let (o2, n2) = plane_of(side2)?;
    let (om, nm) = plane_of(center)?;
    if n1.dot(&n2) > -1.0 + tol::ANGULAR {
        return Err(KernelError::InvalidParameter(
            "A full round needs two parallel side faces facing away from each other".into(),
        ));
    }
    if n1.dot(&nm).abs() > tol::ANGULAR {
        return Err(KernelError::InvalidParameter(
            "The middle face must be square to the parallel side faces".into(),
        ));
    }
    let width = (o2 - o1).dot(&n1).abs();
    let r = width / 2.0;
    // Each side face must reach r below the middle face, else the round would cut through
    // what lies under it: its boundary's deepest point, from points along its edges.
    let depth = |f: FaceId| -> f64 {
        b.faces[f.0 as usize]
            .loops
            .iter()
            .flatten()
            .flat_map(|c| {
                let e = &b.edges[c.edge];
                (0..=8).map(move |i| e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0))
            })
            .map(|p| (om - p).dot(&nm))
            .fold(f64::NEG_INFINITY, f64::max)
    };
    if depth(side1).min(depth(side2)) < r - tol::REL * (1.0 + r) {
        return Err(KernelError::InvalidParameter(
            "The side faces are too shallow for the full round".into(),
        ));
    }
    // The round's axis: in the middle face's plane moved r into the material, midway between
    // the sides, along the sides' and middle face's common direction.
    let along = n1.cross(&nm).normalize();
    let mid_side = o1 + (o2 - o1).dot(&n1) * n1 * 0.5 + n1 * 0.0;
    let _ = mid_side;
    // A point on the axis: on the middle plane, halfway between the side planes, then r down.
    let base = om + n1 * ((o1 - om).dot(&n1) + (o2 - om).dot(&n1)) * 0.5 - nm * r;
    // Section plane square to the axis.
    let x = n1;
    let y = nm;
    let plane = Plane {
        origin: base,
        x_dir: nalgebra::Unit::new_normalize(x),
        normal: nalgebra::Unit::new_normalize(x.cross(&y)),
    };
    let ydir = plane.y_dir().into_inner();
    let up = if ydir.dot(&nm) > 0.0 { 1.0 } else { -1.0 };
    // Rectangle above the axis less the half disc: corners (±r, 0), (±r, r + δ) in (x, y·up).
    let delta = r * 0.5 + 1e-3;
    let p2 = |a: f64, c: f64| nalgebra::Point2::new(a, c * up);
    let l = |a, c| Curve2::Line {
        a,
        b: c,
        source: None,
    };
    let (start, sweep) = if up > 0.0 { (0.0, PI) } else { (0.0, -PI) };
    let region = Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc {
                    center: p2(0.0, 0.0),
                    radius: r,
                    start_angle: start,
                    sweep,
                    source: Some(1),
                },
                l(p2(-r, 0.0), p2(-r, r + delta)),
                l(p2(-r, r + delta), p2(r, r + delta)),
                l(p2(r, r + delta), p2(r, 0.0)),
            ],
        },
        holes: vec![],
        source: None,
    };
    let prof = Profile::new(plane, vec![region]);
    let regions = super::profile::regions(&prof)?;
    let mf = super::profile::merge_regions(&regions)?;
    // Long enough to cover the body along the axis.
    let (lo, hi) = super::kernel::extent_along(&b, &base, &along);
    let dir = plane.normal.into_inner();
    let s = if dir.dot(&along) > 0.0 { 1.0 } else { -1.0 };
    let (lo, hi) = if s > 0.0 { (lo, hi) } else { (-hi, -lo) };
    let kind = Prism { plane, dir };
    let built = sweep::sweep_full(&mf, &[], &kind, lo - 1.0, hi + 1.0, false, true)?;
    let mut src = vec![Vec::new(); built.body.faces.len()];
    for (f, o) in &built.origins {
        if let Origin::ProfileCurve { curve: 1, .. } = o {
            src[*f].push(FaceSrc::Input(center.0 as usize));
        }
    }
    let cur = Tracked::of(b.clone());
    let out = cur.apply(&built.body, &src, BoolOp::Subtract)?;
    Ok(out.into_result(k, body))
}

/// A partial fillet (PS14.6): the whole fillet's tool between the planes square to the edge at
/// the fractions `from` and `to`; at an end of the edge with no tangent neighbour the tool runs
/// past the vertex, as the whole fillet does.
pub fn fillet_partial(
    k: &mut NativeKernel,
    body: BodyId,
    edge: EdgeId,
    spec: &FilletSpec,
    from: f64,
    to: f64,
) -> Result<OpResult> {
    if !(0.0..=1.0).contains(&from) || !(0.0..=1.0).contains(&to) || to <= from + tol::ROUNDOFF {
        return Err(KernelError::InvalidParameter(
            "A partial fillet needs 0 ≤ from < to ≤ 1".into(),
        ));
    }
    let b = (**k.get(body)?).clone();
    let e = edge.0 as usize;
    if e >= b.edges.len() {
        return Err(KernelError::InvalidParameter(format!("unknown {edge:?}")));
    }
    let chain = k.tangent_chain(body, edge)?;
    let ed = b.edges[e].clone();
    // An end at a vertex with a tangent neighbour stops square to the edge there.
    let tangent_at = |v: usize| {
        chain
            .iter()
            .any(|c| c.0 as usize != e && b.edges[c.0 as usize].v.contains(&v))
    };
    let extend = (
        from <= 0.0 && !tangent_at(ed.v[0]) && !ed.closed(),
        to >= 1.0 && !tangent_at(ed.v[1]) && !ed.closed(),
    );
    let spec = *spec;
    let kind = move |phi: f64| fillet_section(&spec, phi);
    check_fits(&b, e, &kind)?;
    let tool = edge_tool_range(&b, body, e, &kind, false, (from, to), extend)?;
    // The tool's caps (square to the edge at the bounds) become the fillet's end faces.
    let mut src = tool.src.clone();
    let mut caps = Vec::new();
    for (fi, f) in tool.body.faces.iter().enumerate() {
        if src[fi].is_empty()
            && let Surface::Plane { f: fr } = &f.surf
        {
            let n = fr.x.cross(&fr.y);
            let t = ed.curve.tangent(ed.mid_t()).normalize();
            if n.cross(&t).norm() < tol::ANGULAR_POINT || matches!(ed.curve, Curve::Circle { .. }) {
                caps.push(fi);
            }
        }
    }
    // Which cap is at the start: nearer the edge's start point.
    let p0 = ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * from);
    let mut caps_d: Vec<(usize, f64)> = caps
        .iter()
        .map(|&fi| (fi, (super::mass::face_area(&tool.body, fi).1 - p0).norm()))
        .collect();
    caps_d.sort_by(|a, c| a.1.total_cmp(&c.1));
    if let Some(&(f, _)) = caps_d.first()
        && !extend.0
    {
        src[f].push(FaceSrc::New(Origin::StartCap { region: 0 }));
    }
    if caps_d.len() > 1 {
        let f = caps_d.last().unwrap().0;
        if !extend.1 {
            src[f].push(FaceSrc::New(Origin::EndCap { region: 0 }));
        }
    }
    let cur = Tracked::of(b);
    let op = if tool.convex {
        BoolOp::Subtract
    } else {
        BoolOp::Union
    };
    let out = cur.apply(&tool.body, &src, op)?;
    Ok(out.into_result(k, body))
}

/// Fillets of a radius law each by local trimming alone (no tool and boolean): the edges'
/// strips and their corners, edges of different radii meeting in patched corners.
pub fn fillet_laws_local(
    k: &mut NativeKernel,
    body: BodyId,
    laws: &[FilletLaw],
) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    if !laws
        .iter()
        .all(|l| (l.edge.0 as usize) < b.edges.len() && !l.radii.is_empty())
    {
        return Err(KernelError::InvalidParameter(
            "unknown edge or empty law".into(),
        ));
    }
    if std::env::var_os("CADRS_NO_LOCAL_FILLET").is_some() {
        return Err(KernelError::Unsupported(
            "local fillets (CADRS_NO_LOCAL_FILLET is set)",
        ));
    }
    let edges: Vec<super::fillet::RadiusLaw> = laws
        .iter()
        .map(|l| {
            let mut pts = l.radii.clone();
            pts.sort_by(|a, c| a.0.total_cmp(&c.0));
            (l.edge.0 as usize, pts)
        })
        .collect();
    let (out, src) = super::fillet::fillet_edges_laws(&b, body, &edges, false)?;
    // (The local result's edges are merged and its tolerances fitted already.)
    if let Some(e) = local_problem(&b, &out) {
        return Err(KernelError::InvalidResult {
            op: "fillet",
            why: format!("the local fillet isn't valid: {e}"),
        });
    }
    Ok(Tracked { body: out, src }.into_result(k, body))
}

/// Chamfers of equal distance `d` (offsets along the faces) on `edges` by local trimming alone
/// (no tool and boolean), as [`chamfer`] makes them first.
pub fn chamfer_local(
    k: &mut NativeKernel,
    body: BodyId,
    edges: &[EdgeId],
    d: f64,
) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    let edges = &real_edges(&b, edges)?[..];
    let sharp = sharp_edges(&b, edges)?;
    let sharp: Vec<usize> = sharp.iter().map(|e| e.0 as usize).collect();
    let (out, src) = super::fillet::chamfer_equal_edges(&b, body, &sharp, d).ok_or(
        KernelError::Unsupported("a chamfer whose faces' angle can't be followed along its edge"),
    )??;
    if let Some(e) = local_problem(&b, &out) {
        return Err(KernelError::InvalidResult {
            op: "chamfer",
            why: format!("the local chamfer isn't valid: {e}"),
        });
    }
    Ok(Tracked { body: out, src }.into_result(k, body))
}

/// A variable fillet: circular sections whose radius follows a law along each edge, lofted
/// through sections at the law's points (and past the edge's ends).
pub fn fillet_variable(
    k: &mut NativeKernel,
    body: BodyId,
    laws: &[FilletLaw],
    _allow_overflow: bool,
) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    let real = real_edges(&b, &laws.iter().map(|l| l.edge).collect::<Vec<_>>())?;
    let laws: Vec<FilletLaw> = laws
        .iter()
        .filter(|l| real.contains(&l.edge))
        .cloned()
        .collect();
    let laws = &laws[..];
    // By local trimming first (any edge and faces), then the tool for straight edges between
    // flat faces.
    match fillet_laws_local(k, body, laws) {
        Ok(r) => return Ok(r),
        Err(e) => {
            if std::env::var_os("CADRS_LOCAL_FILLET_ONLY").is_some() {
                return Err(e);
            }
            if std::env::var_os("CADRS_BLEND_DEBUG").is_some() {
                eprintln!("local variable fillet: {e}; falling back to the blend tool");
            }
        }
    }
    let mut cur = Tracked::of(b.clone());
    for law in laws {
        let e = law.edge.0 as usize;
        if e >= b.edges.len() || law.radii.is_empty() {
            return Err(KernelError::InvalidParameter(
                "unknown edge or empty law".into(),
            ));
        }
        if law.radii.iter().any(|(_, r)| !(*r > 0.0)) {
            return Err(KernelError::InvalidParameter(
                "A fillet radius must be positive".into(),
            ));
        }
        let ed = b.edges[e].clone();
        let Curve::Line { d, .. } = &ed.curve else {
            return Err(KernelError::Unsupported("variable fillets of curved edges"));
        };
        let [s1, s2] = edge_sides(&b, e)?;
        if !matches!(b.faces[s1.0].surf, Surface::Plane { .. })
            || !matches!(b.faces[s2.0].surf, Surface::Plane { .. })
        {
            return Err(KernelError::Unsupported(
                "variable fillets between curved faces",
            ));
        }
        let mut pts: Vec<(f64, f64)> = law.radii.clone();
        pts.sort_by(|a, c| a.0.total_cmp(&c.0));
        if pts.len() == 1 {
            pts = vec![(0.0, pts[0].1), (1.0, pts[0].1)];
        }
        if pts[0].0 > 0.0 {
            pts.insert(0, (0.0, pts[0].1));
        }
        if pts.last().unwrap().0 < 1.0 {
            let r = pts.last().unwrap().1;
            pts.push((1.0, r));
        }
        let t = d.normalize();
        let len = (ed.t[1] - ed.t[0]) * d.norm();
        let p_start = ed.point(ed.t[0]);
        let (n1, w1) = face_dirs(&b, s1.0, e, s1.1, ed.mid_t());
        let (n2, w2) = face_dirs(&b, s2.0, e, s2.1, ed.mid_t());
        let convex = w2.dot(&n1) < 0.0;
        let _ = n2;
        let x = w1;
        let y = t.cross(&x);
        let to2 = |v: &V3| V2::new(v.dot(&x), v.dot(&y));
        let corner = Corner2 {
            u1: to2(&w1).normalize(),
            u2: to2(&w2).normalize(),
            convex,
        };
        let rmax = pts.iter().map(|p| p.1).fold(0.0, f64::max);
        let ext = rmax * 4.0 + 1.0;
        // Sections: the law's points, and one past each end with the end radius.
        let mut stations: Vec<(f64, f64)> = vec![(-ext, pts[0].1)];
        stations.extend(pts.iter().map(|(f, r)| (f * len, *r)));
        stations.push((len + ext, pts.last().unwrap().1));
        let mut sections = Vec::new();
        for (s, r) in &stations {
            let o = p_start + t * *s;
            let plane = Plane {
                origin: o,
                x_dir: nalgebra::Unit::new_normalize(x),
                normal: nalgebra::Unit::new_normalize(t),
            };
            let (lp, _, _) = section_loop(
                &corner,
                SectionKind::Circle { r: *r },
                Src::Edge {
                    body: body.0,
                    edge: e as u64,
                },
            )?;
            sections.push((lp, plane));
        }
        let mut tk = NativeKernel::new();
        let built = super::loft_build::loft_pieces(&tk, &sections, 0)?;
        let lr = super::features::Tagged::from_built(built).into_result(&mut tk);
        let mut tool = (**tk.get(lr.bodies[0])?).clone();
        // The sides swept by the sections' straight pieces lie on the edge's faces: as planes,
        // the boolean sees them coincide.
        super::unify::planarize(&mut tool);
        // The face swept by the arcs is the fillet face.
        let mut src = vec![Vec::new(); tool.faces.len()];
        for (f, o) in &lr.history.generated {
            if let Origin::ProfileCurve { curve, .. } = o {
                // The arc is the second curve of each section loop.
                if *curve == unsourced_curve(0, 1)
                    || matches!(tool.faces[f.0 as usize].surf, Surface::Nurbs(_))
                        && is_arc_face(&tool, f.0 as usize)
                {
                    src[f.0 as usize].push(FaceSrc::New(Origin::FromEdge {
                        body,
                        edge: EdgeId(e as u64),
                    }));
                }
            }
        }
        cur = cur.apply(
            &tool,
            &src,
            if convex {
                BoolOp::Subtract
            } else {
                BoolOp::Union
            },
        )?;
    }
    if !b.solids.is_empty() && cur.body.solids.is_empty() {
        return Err(KernelError::InvalidResult {
            op: "fillet",
            why: "the fillet left an open body".into(),
        });
    }
    Ok(cur.into_result(k, body))
}

/// A loft face that isn't flat (the one the arcs swept).
fn is_arc_face(b: &Body, f: usize) -> bool {
    let face = &b.faces[f];
    let (u0, v0) = (0.5, 0.5);
    let d = face.surf.d2(u0, v0);
    d[3].norm() > tol::ROUNDOFF
}

/// The outward signed distance of a point from a face's surface.
pub(crate) fn out_dist(f: &Face, p: &P3) -> (f64, V3, P3) {
    let (u, v) = f.surf.project(p, None);
    let q = f.surf.eval(u, v);
    let n = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
    ((p - q).dot(&n), n, q)
}

/// A point on the face's surface `dist` along the face from `p`, in the plane through `p`
/// square to `t`, starting along `w` (marching along the section curve).
pub(crate) fn along_face(f: &Face, p: &P3, t: &V3, w: &V3, dist: f64) -> P3 {
    let steps = 64;
    let h = dist / steps as f64;
    let mut q = *p;
    let mut dir = *w;
    for _ in 0..steps {
        let mut x = q + dir * h;
        // Back onto the surface and into the plane.
        for _ in 0..4 {
            let (u, v) = f.surf.project(&x, None);
            x = f.surf.eval(u, v);
            x -= t * (x - p).dot(t);
        }
        let step = x - q;
        if step.norm() > 1e-15 {
            dir = step.normalize();
        }
        q = x;
    }
    q
}

/// A blend tool for any edge between two faces: at samples along the edge (and past its ends)
/// the section is solved on the exact surfaces (the rolling ball's centre at r inside both faces
/// in the plane square to the edge, or the chamfer's contact points `d` along each face), and
/// the line–curve–line sections are lofted.
pub fn edge_tool_general(
    b: &Body,
    input: BodyId,
    e: usize,
    kind: &dyn Fn(f64) -> SectionKind,
    swap: bool,
    range: (f64, f64),
    extend: (bool, bool),
) -> Result<Tool> {
    chain_tool(b, input, &[(e, true)], kind, swap, range, extend)
}

/// One blend tool along a chain of edges (each run forwards or backwards), so a tangent chain's
/// blend is one smooth tool with no joints. On each edge the first face is the one whose loop
/// runs along the chain (swapped when `swap`); `range` and `extend` apply to the chain's ends.
pub fn chain_tool(
    b: &Body,
    input: BodyId,
    chain: &[(usize, bool)],
    kind: &dyn Fn(f64) -> SectionKind,
    swap: bool,
    range: (f64, f64),
    extend: (bool, bool),
) -> Result<Tool> {
    super::blend_chain::chain_tool(b, input, chain, kind, swap, range, extend)
}

/// The previous lofted chain tool (the contacts on the chain edges' own faces).
#[allow(dead_code)]
fn chain_tool_lofted(
    b: &Body,
    input: BodyId,
    chain: &[(usize, bool)],
    kind: &dyn Fn(f64) -> SectionKind,
    swap: bool,
    range: (f64, f64),
    extend: (bool, bool),
) -> Result<Tool> {
    // Faces of each edge: (first, second) with their coedge directions.
    let mut sides: Vec<[(usize, bool); 2]> = Vec::new();
    for &(e, fwd) in chain {
        let [a, c] = edge_sides(b, e)?;
        let (mut s1, mut s2) = if a.1 == fwd { (a, c) } else { (c, a) };
        if swap {
            std::mem::swap(&mut s1, &mut s2);
        }
        sides.push([s1, s2]);
    }
    let e0 = chain[0].0;
    let ed0 = &b.edges[e0];
    let tm = ed0.mid_t();
    let (n1m, _) = face_dirs(b, sides[0][0].0, e0, sides[0][0].1, tm);
    let (_, w2m) = face_dirs(b, sides[0][1].0, e0, sides[0][1].1, tm);
    let convex = w2m.dot(&n1m) < 0.0;
    let lens: Vec<f64> = chain
        .iter()
        .map(|&(e, _)| b.edges[e].curve.length(b.edges[e].t[0], b.edges[e].t[1]))
        .collect();
    let len: f64 = lens.iter().sum();
    // Stations: (edge index in the chain, fraction along it in the chain's direction).
    let size_guess0 = match kind(PI / 2.0) {
        SectionKind::Circle { r }
        | SectionKind::Conic { r, .. }
        | SectionKind::Curvature { r, .. } => r,
        SectionKind::Chamfer { d1, d2 } => d1.max(d2),
        SectionKind::Asymmetric { r1, r2 } => r1.max(r2),
    };
    let ext_len = size_guess0 * 2.0 + 0.5;
    let mut stations: Vec<(usize, f64)> = Vec::new();
    let (ra, rb) = (range.0 * len, range.1 * len);
    let total_lo = ra - if extend.0 { ext_len } else { 0.0 };
    let total_hi = rb + if extend.1 { ext_len } else { 0.0 };
    let n_total = (((total_hi - total_lo) / 1.5).ceil() as usize).clamp(8, 160);
    let mut acc = 0.0;
    let starts: Vec<f64> = lens
        .iter()
        .map(|l| {
            let s0 = acc;
            acc += l;
            s0
        })
        .collect();
    for i in 0..=n_total {
        let s = total_lo + (total_hi - total_lo) * i as f64 / n_total as f64;
        // Which edge: clamp into the chain (extensions run on the end edges' curves).
        let mut k = starts.iter().rposition(|&st| st <= s).unwrap_or(0);
        if k >= chain.len() {
            k = chain.len() - 1;
        }
        let f = (s - starts[k]) / lens[k].max(tol::TINY);
        stations.push((k, f));
    }
    // Also a station exactly at each joint.
    for (k, &sj) in starts.iter().enumerate().take(chain.len()).skip(1) {
        if sj > total_lo && sj < total_hi {
            stations.push((k, 0.0));
        }
    }
    stations.sort_by(|a, c| {
        (starts[a.0] + a.1 * lens[a.0]).total_cmp(&(starts[c.0] + c.1 * lens[c.0]))
    });
    stations.dedup_by(|a, c| {
        ((starts[a.0] + a.1 * lens[a.0]) - (starts[c.0] + c.1 * lens[c.0])).abs() < tol::POINT
    });
    let mut sections = Vec::new();
    for &(k, fr) in &stations {
        let (e, fwd) = chain[k];
        let ed = &b.edges[e];
        let [s1, s2] = sides[k];
        let (f1, f2) = (&b.faces[s1.0], &b.faces[s2.0]);
        {
            let fr = if fwd { fr } else { 1.0 - fr };
            let t = ed.t[0] + (ed.t[1] - ed.t[0]) * fr;
            let p = ed.curve.point(t);
            let tan = ed.curve.tangent(t).normalize() * if fwd { 1.0 } else { -1.0 };
            // The faces' outward normals and directions into them square to the edge at p (the
            // surfaces continue past the faces' boundaries for the extensions).
            let dirs = |f: &Face, co_fwd: bool| -> (V3, V3) {
                let (u, v) = f.surf.project(&p, None);
                let nrm = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
                let d = tan * if co_fwd == fwd { 1.0 } else { -1.0 };
                (nrm, nrm.cross(&d))
            };
            let (n1, w1) = dirs(f1, s1.1);
            let (n2, w2) = dirs(f2, s2.1);
            let phi = n1.dot(&n2).clamp(-1.0, 1.0).acos();
            let theta = w1.dot(&w2).clamp(-1.0, 1.0).acos();
            let section = kind(phi);
            let (c1, c2, curve): (P3, P3, Curve2Section) = match section {
            SectionKind::Circle { r } => {
                // Ball centre: r inside both faces (outside for a concave edge), in the plane.
                let s = if convex { -1.0 } else { 1.0 };
                let bis = (w1 + w2).normalize();
                let mut x = p + bis * (r / (theta / 2.0).sin());
                for _ in 0..50 {
                    let (d1, g1, _) = out_dist(f1, &x);
                    let (d2, g2, _) = out_dist(f2, &x);
                    // Solve d1 = s r, d2 = s r, (x − p)·tan = 0.
                    let m = nalgebra::Matrix3::from_rows(&[g1.transpose(), g2.transpose(), tan.transpose()]);
                    let rhs = V3::new(s * r - d1, s * r - d2, -(x - p).dot(&tan));
                    let Some(inv) = m.try_inverse() else { break };
                    let dx = inv * rhs;
                    x += dx;
                    if dx.norm() < 1e-13 {
                        break;
                    }
                }
                let (_, _, q1) = out_dist(f1, &x);
                let (_, _, q2) = out_dist(f2, &x);
                (q1, q2, Curve2Section::Arc(x, r))
            }
            SectionKind::Chamfer { d1, d2 } => (along_face(f1, &p, &tan, &w1, d1), along_face(f2, &p, &tan, &w2, d2), Curve2Section::Line),
            _ => return Err(KernelError::InvalidParameter("Conic, curvature and asymmetric fillets work on straight edges between flat faces".into())),
        };
            // The section's plane: through the contacts and the corner.
            let nrm = (c1 - p).cross(&(c2 - p));
            if nrm.norm() < tol::DEGENERATE {
                return Err(KernelError::InvalidGeometry {
                    op: "blend",
                    why: "the blend's section degenerates along the edge".into(),
                });
            }
            let nrm = nrm.normalize();
            let xd = (c1 - p).normalize();
            let plane = Plane {
                origin: p,
                x_dir: nalgebra::Unit::new_normalize(xd),
                normal: nalgebra::Unit::new_normalize(nrm),
            };
            let yd = plane.y_dir().into_inner();
            let to2 = |q: &P3| nalgebra::Point2::new((q - p).dot(&xd), (q - p).dot(&yd));
            let (q1, q2) = (to2(&c1), to2(&c2));
            let bis = (q1.coords.normalize() + q2.coords.normalize()).normalize();
            let size = q1.coords.norm().max(q2.coords.norm());
            let pc = nalgebra::Point2::origin() - bis * (size * 0.5 + 1e-3);
            let (mid, q1, q2): (Curve2, nalgebra::Point2<f64>, nalgebra::Point2<f64>) = match curve
            {
                Curve2Section::Line => (
                    Curve2::Line {
                        a: q1,
                        b: q2,
                        source: Some(1),
                    },
                    q1,
                    q2,
                ),
                Curve2Section::Arc(center, _r) => {
                    let cc = to2(&center);
                    let r = ((q1 - cc).norm() + (q2 - cc).norm()) * 0.5;
                    let a1 = (q1.y - cc.y).atan2(q1.x - cc.x);
                    let a2 = (q2.y - cc.y).atan2(q2.x - cc.x);
                    // The short way round.
                    let mut sw = a2 - a1;
                    while sw > PI {
                        sw -= TAU;
                    }
                    while sw < -PI {
                        sw += TAU;
                    }
                    let arc = Curve2::Arc {
                        center: cc,
                        radius: r,
                        start_angle: a1,
                        sweep: sw,
                        source: Some(1),
                    };
                    // The lines run to the arc's own ends.
                    let (s, e) = (arc.start(), arc.end());
                    (arc, s, e)
                }
            };
            let curves = [
                Curve2::Line {
                    a: pc,
                    b: q1,
                    source: Some(0),
                },
                mid,
                Curve2::Line {
                    a: q2,
                    b: pc,
                    source: Some(2),
                },
            ];
            // The same three pieces in the same order at every station (lofted as they are).
            let pieces: Vec<Piece> = curves
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    super::profile::piece_of(
                        c,
                        Src::Curve {
                            region: 0,
                            curve: i as u64,
                        },
                    )
                })
                .collect::<Result<_>>()?;
            sections.push((pieces, plane));
        }
    }
    let e = e0;
    let mut tk = NativeKernel::new();
    let built = super::loft_build::loft_pieces(&tk, &sections, 0)?;
    let lr = super::features::Tagged::from_built(built).into_result(&mut tk);
    let body = (**tk.get(lr.bodies[0])?).clone();
    let mut src = vec![Vec::new(); body.faces.len()];
    for (f, o) in &lr.history.generated {
        if let Origin::ProfileCurve { curve, .. } = o
            && *curve == unsourced_curve(0, 1)
        {
            src[f.0 as usize].push(FaceSrc::New(Origin::FromEdge {
                body: input,
                edge: EdgeId(e as u64),
            }));
        }
    }
    let z = (P3::origin(), V3::zeros(), V3::zeros());
    Ok(Tool {
        body,
        src,
        convex,
        ends: [z, z],
    })
}

#[derive(Debug, Clone, Copy)]
enum Curve2Section {
    Line,
    Arc(P3, f64),
}

#[cfg(test)]
mod tests {
    use crate::native::draw::Shape;
    use crate::native::kernel::NativeKernel;
    use crate::*;

    /// A full round whose side face doesn't reach the round's radius below the middle face is
    /// refused before any boolean (it would cut through what lies under the side).
    #[test]
    fn a_full_round_needs_deep_enough_sides() {
        let mut k = NativeKernel::new();
        let bx = k.box_(10.0, 20.0, 30.0).unwrap();
        let step = k.box_at(-1.0, -1.0, -1.0, 3.0, 22.0, 28.0).unwrap();
        let Shape::Body(b) = k.bcut(&bx, &step).unwrap() else {
            unreachable!()
        };
        let faces = k.faces(b).unwrap();
        let find = |n: [f64; 3], at: f64| -> FaceId {
            let n = nalgebra::Vector3::new(n[0], n[1], n[2]);
            faces
                .iter()
                .find(|f| {
                    f.plane.is_some_and(|pl| {
                        pl.normal.dot(&n) > 0.999999 && (pl.origin.coords.dot(&n) - at).abs() < 1e-9
                    })
                })
                .unwrap()
                .id
        };
        let (left, top, right) = (
            find([-1.0, 0.0, 0.0], 0.0),
            find([0.0, 0.0, 1.0], 30.0),
            find([1.0, 0.0, 0.0], 10.0),
        );
        let err = k.full_round(b, left, top, right).unwrap_err();
        assert!(err.to_string().contains("too shallow"), "{err}");
    }
}
