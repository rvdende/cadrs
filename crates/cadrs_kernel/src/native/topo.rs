//! Boundary representation: vertices, edges, faces with loops of coedges, shells and solids.
//!
//! Conventions (as OCCT's):
//! - An edge runs along its curve for increasing t from `v[0]` (at `t[0]`) to `v[1]`; a closed
//!   edge (a whole circle) has `v[0] == v[1]`.
//! - A coedge uses an edge forward or reversed and carries the edge's p-curve on its face,
//!   parametrized by the edge's t.
//! - Loops run counter-clockwise about the face's outward normal (material on the left seen
//!   from outside); the first loop is the outer one.
//! - `sense`: the outward normal is the surface's natural normal Su × Sv when true.
//! - A seam (on a periodic surface) is one edge used twice by the same face, once each way, with
//!   two p-curves a period apart.
//!
//! Body kinds (Onshape's set, [`BodyClass`]): a body holds solids (closed shells), sheets (open
//! shells), wires (chains of free edges: edges no face uses, listed in [`Body::wires`]) and
//! acorn points (free vertices, listed in [`Body::points`]). Wires and points sit beside the
//! face lists, so code that walks `faces` and `edges` by index is unchanged: a wire body is a
//! body with edges and no faces, a point body one with vertices only.

#![warn(missing_docs)]

use super::curve::Curve;
use super::math::*;
use super::surface::Surface;
use super::tol;

/// The smallest tolerance an entity has (OCCT's `Precision::Confusion`): exact geometry
/// ([`super::tol::CONFUSION`]).
pub const TOL_MIN: f64 = super::tol::CONFUSION;

/// Whether a closed shell or solid (as a body of its own) encloses a negative volume (its faces
/// point in): by ray parity where every probe agrees ([`parity_orientation`], a few rays), else
/// by the sign of its volume (an integral over every face, which on a large B-spline model
/// costs far more).
pub(crate) fn points_in(shell: &Body) -> bool {
    match parity_orientation(shell) {
        Some(out) => !out,
        None => super::mass::body_volume(shell) < 0.0,
    }
}

/// Whether a closed shell (as a body of its own) points out by ray parity, without integrating:
/// at points just off a few of its faces, along their normals, the one behind the face
/// inside and the one in front outside. A face's normal here is the one its volume integral
/// takes ([`super::mass`]): the surface's, turned round where its outer loop runs clockwise in
/// (u, v). `Some(true)` when every probe says so, `Some(false)`
/// when every one says the opposite, else `None` (a thin wall or grazing rays: the volume
/// decides).
fn parity_orientation(shell: &Body) -> Option<bool> {
    parity_orientation_with(
        shell,
        shell
            .faces
            .iter()
            .map(|f| super::classify::face_polygons(shell, f))
            .collect(),
    )
}

/// [`parity_orientation`] with the faces' polygons made already.
fn parity_orientation_with(shell: &Body, polys: Vec<Vec<Vec<P2>>>) -> Option<bool> {
    let cls = super::classify::Classifier::new(shell, &polys);
    let size = shell.bbox().diagonal();
    let mut verdict: Option<bool> = None;
    let mut probes = 0;
    for (fi, f) in shell.faces.iter().enumerate() {
        if probes == 8 {
            break;
        }
        // A point well inside the face's domain: of a grid over its (u, v) box, the one
        // farthest from the loops (by the metric-scaled polygon).
        let ps = &polys[fi];
        let (mut lo, mut hi) = (
            P2::new(f64::INFINITY, f64::INFINITY),
            P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for q in ps.iter().flatten() {
            lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
            hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
        }
        if !(lo.x < hi.x && lo.y < hi.y) {
            continue;
        }
        let (mu, mv) = f.surf.metric();
        // The loops' segments in the metric-scaled (u, v), once for all the grid's points.
        let segs: Vec<(P2, V2, f64)> = ps
            .iter()
            .flat_map(|l| {
                (0..l.len()).map(move |k| {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    let (a, b) = (P2::new(a.x * mu, a.y * mv), P2::new(b.x * mu, b.y * mv));
                    let ab = b - a;
                    (a, ab, ab.norm_squared().max(1e-300))
                })
            })
            .collect();
        let n = 9;
        let mut best: Option<(f64, P2)> = None;
        for i in 1..n {
            for j in 1..n {
                let uv = P2::new(
                    lo.x + (hi.x - lo.x) * i as f64 / n as f64,
                    lo.y + (hi.y - lo.y) * j as f64 / n as f64,
                );
                // Its distance from the loops, given up as soon as it can't beat the best so
                // far (the same choice as the full minimum); whether it lies inside only asked
                // of a point that would be the best.
                let floor = best.map_or(f64::NEG_INFINITY, |b| b.0);
                let x = P2::new(uv.x * mu, uv.y * mv);
                let mut d = f64::INFINITY;
                for &(a, ab, n2) in &segs {
                    let s = ((x - a).dot(&ab) / n2).clamp(0.0, 1.0);
                    d = d.min((a + ab * s - x).norm());
                    if d <= floor {
                        break;
                    }
                }
                if best.is_none_or(|b| d > b.0) {
                    let inside = ps.first().is_some_and(|o| point_in_polygon(&uv, o))
                        && !ps.iter().skip(1).any(|h| point_in_polygon(&uv, h));
                    if inside {
                        best = Some((d, uv));
                    }
                }
            }
        }
        let Some((room, uv)) = best else { continue };
        let normal = f.surf.normal(uv.x, uv.y);
        if !(normal.norm() > 0.5) {
            continue;
        }
        let Some(outer) = ps.first() else { continue };
        let twice: f64 = (0..outer.len())
            .map(|k| {
                let (a, b) = (outer[k], outer[(k + 1) % outer.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum();
        if twice == 0.0 {
            continue;
        }
        let out = normal.normalize() * twice.signum();
        let p = f.surf.eval(uv.x, uv.y);
        let eps = (1e-4 * size).min(0.1 * room);
        if !(eps > tol::REL * size) {
            continue;
        }
        let (behind, front) = (
            cls.parity_inside(&(p - out * eps)),
            cls.parity_inside(&(p + out * eps)),
        );
        let this = match (behind, front) {
            (true, false) => true,
            (false, true) => false,
            _ => return None,
        };
        if verdict.is_some_and(|v| v != this) {
            return None;
        }
        verdict = Some(this);
        probes += 1;
    }
    verdict
}

/// How thorough [`Body::check_with`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckLevel {
    /// What operations check their results with ([`Body::check`]), fast enough to run after
    /// every step: references, coedges meeting end to start in 3D and in (u, v), p-curves on
    /// their edges at 7 points, edge ends on their vertices, loops not crossing themselves (8
    /// samples per coedge, confirmed by the full check's test), each edge of a closed shell
    /// used once each way by that shell's faces, the rules of each body kind, and solids
    /// enclosing a positive volume.
    #[default]
    Fast,
    /// Everything `Fast` checks, and (for tests, imports and debugging):
    /// - each p-curve's largest distance from its edge searched for (dense samples, then a
    ///   golden-section search about the worst), within the edge's tolerance;
    /// - each p-curve's ends on the face at its vertices, and a degenerate edge's p-curve on
    ///   its point, within the vertex's tolerance;
    /// - a face's loops, as polylines within a fraction of [`super::tol::LOOP_GAP`] of their
    ///   p-curves, not crossing themselves or each other in (u, v) by more than the edges'
    ///   tolerance (exact orientation predicates decide the crossings);
    /// - the first loop of a face its outer loop, counter-clockwise about the outward normal,
    ///   the others holes inside it running the other way, no hole inside another;
    /// - each shell edge-connected, and each face in at most one shell;
    /// - no two faces of a shell crossing each other away from their shared edges
    ///   ([`super::check_faces`]);
    /// - each solid's outer shell enclosing a positive volume and each void shell a negative
    ///   one, with no vertex outside the outer shell (touching it is allowed).
    Full,
}

/// A point where edges meet.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    /// Where it is (mm).
    pub p: P3,
    /// The ball about `p` the vertex stands for: ends of its edges and its position on faces
    /// lie within it ([`TOL_MIN`](super::tol::CONFUSION) for exact geometry; larger for read or healed shapes).
    pub tol: f64,
}

/// A bounded piece of a curve between two vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The 3D curve it lies on.
    pub curve: Curve,
    /// Its parameter range on `curve`, start then end (`t[0] < t[1]`).
    pub t: [f64; 2],
    /// Its start and end vertices (indices into [`Body::verts`]; equal for a closed edge).
    pub v: [usize; 2],
    /// A point edge (a cone's apex, a sphere's pole): only its p-curves mean anything.
    pub degenerate: bool,
    /// How far the edge's 3D curve, its p-curves on its faces and the faces may stray from one
    /// another along it ([`TOL_MIN`](super::tol::CONFUSION) for exact geometry).
    pub tol: f64,
}

impl Edge {
    /// The point of its curve at `t`.
    pub fn point(&self, t: f64) -> P3 {
        self.curve.point(t)
    }
    /// The middle of its parameter range.
    pub fn mid_t(&self) -> f64 {
        (self.t[0] + self.t[1]) * 0.5
    }
    /// A non-degenerate edge that ends where it starts (a whole circle).
    pub fn closed(&self) -> bool {
        self.v[0] == self.v[1] && !self.degenerate
    }
}

/// A face's use of an edge in one of its loops.
#[derive(Debug, Clone, PartialEq)]
pub struct Coedge {
    /// The edge (an index into [`Body::edges`]).
    pub edge: usize,
    /// Whether the loop runs along the edge (from `v[0]` to `v[1]`) or against it.
    pub fwd: bool,
    /// The p-curve (z = 0) in the face's (u, v), parametrized by the edge's t.
    pub pc: Curve,
}

/// A bounded piece of a surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// The surface it lies on.
    pub surf: Surface,
    /// Whether its outward normal is the surface's natural normal Su × Sv (else the opposite).
    pub sense: bool,
    /// Its boundary: the outer loop first, then the holes, each a cycle of coedges.
    pub loops: Vec<Vec<Coedge>>,
}

/// Faces joined along their edges.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shell {
    /// Its faces (indices into [`Body::faces`]).
    pub faces: Vec<usize>,
    /// Whether it encloses a volume (each of its edges used once each way).
    pub closed: bool,
}

/// A volume bounded by closed shells.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Solid {
    /// Outer shell first, then voids.
    pub shells: Vec<usize>,
}

/// A chain of free edges (used by no face), each with the direction the chain runs along it:
/// edge k ends where edge k + 1 starts. It is closed when the last edge ends where the first
/// starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wire {
    /// Its edges in order, each with whether the chain runs along it.
    pub edges: Vec<(usize, bool)>,
}

/// Declares [`Body`] with its entity lists `pub` under the `testkit` feature (the crate's tests
/// and tools build and edit bodies directly) and `pub(crate)` otherwise: a user of the crate
/// reads them through [`Body::verts`], [`Body::edges`], … and builds a body with a checked
/// [`BodyBuilder`].
macro_rules! body {
    ($vis:vis) => {
        /// A boundary representation: its entities in flat lists, referring to each other by
        /// index ([`Body::check_refs`] checks the indices, [`Body::check_with`] the rest). Read
        /// them with [`Body::verts`], [`Body::edges`], [`Body::faces`], [`Body::shells`],
        /// [`Body::solids`], [`Body::wires`] and [`Body::points`]; build one with
        /// [`BodyBuilder`], which checks it.
        #[derive(Debug, Clone, PartialEq, Default)]
        pub struct Body {
            /// The vertices.
            $vis verts: Vec<Vertex>,
            /// The edges (of faces, of wires, and degenerate ones at poles and apexes).
            $vis edges: Vec<Edge>,
            /// The faces.
            $vis faces: Vec<Face>,
            /// The shells: closed ones bound solids, open ones are sheets.
            $vis shells: Vec<Shell>,
            /// The solids.
            $vis solids: Vec<Solid>,
            /// Wire bodies' edge chains (Phase 1b). Edges of faces are never in a wire.
            $vis wires: Vec<Wire>,
            /// Acorn points: vertices no edge uses (a point body's points).
            $vis points: Vec<usize>,
        }
    };
}

#[cfg(feature = "testkit")]
body!(pub);
#[cfg(not(feature = "testkit"))]
body!(pub(crate));

impl Body {
    /// The vertices.
    pub fn verts(&self) -> &[Vertex] {
        &self.verts
    }
    /// The edges (of faces, of wires, and degenerate ones at poles and apexes).
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }
    /// The faces.
    pub fn faces(&self) -> &[Face] {
        &self.faces
    }
    /// The shells: closed ones bound solids, open ones are sheets.
    pub fn shells(&self) -> &[Shell] {
        &self.shells
    }
    /// The solids.
    pub fn solids(&self) -> &[Solid] {
        &self.solids
    }
    /// Wire bodies' edge chains. Edges of faces are never in a wire.
    pub fn wires(&self) -> &[Wire] {
        &self.wires
    }
    /// Acorn points: vertices no edge uses (a point body's points), as vertex indices.
    pub fn points(&self) -> &[usize] {
        &self.points
    }
}

/// Builds a [`Body`] entity by entity, each referring to the earlier ones by the index its
/// method returned, and checks it on [`BodyBuilder::build`]: a body the kernel's operations can
/// take, or the problems that keep it from being one.
///
/// # Examples
///
/// A wire body: a closed triangle of three line edges.
///
/// ```
/// use cadrs_kernel::native::{BodyBuilder, Curve, P3, V3};
///
/// let mut b = BodyBuilder::new();
/// let p = [P3::new(0.0, 0.0, 0.0), P3::new(10.0, 0.0, 0.0), P3::new(0.0, 10.0, 0.0)];
/// let v: Vec<usize> = p.iter().map(|&q| b.vertex(q)).collect();
/// let e: Vec<usize> = (0..3)
///     .map(|i| {
///         let (a, z) = (p[i], p[(i + 1) % 3]);
///         let len = (z - a).norm();
///         b.edge(Curve::Line { o: a, d: (z - a) / len }, 0.0, len, v[i], v[(i + 1) % 3])
///     })
///     .collect();
/// b.wire(e.iter().map(|&x| (x, true)).collect());
/// let body = b.build().unwrap();
/// assert_eq!((body.verts().len(), body.edges().len(), body.wires().len()), (3, 3, 1));
///
/// // An edge to a vertex that doesn't exist is refused.
/// let mut bad = BodyBuilder::new();
/// let v0 = bad.vertex(P3::origin());
/// bad.edge(Curve::Line { o: P3::origin(), d: V3::x() }, 0.0, 1.0, v0, 7);
/// assert!(bad.build().is_err());
/// ```
#[derive(Debug, Clone, Default)]
pub struct BodyBuilder {
    body: Body,
}

impl BodyBuilder {
    /// An empty body.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a vertex at `p` of tolerance [`TOL_MIN`]; returns its index.
    pub fn vertex(&mut self, p: P3) -> usize {
        self.body.add_vertex(p)
    }

    /// Adds a vertex at `p` standing for the ball of radius `tol` about it (at least
    /// [`TOL_MIN`]); returns its index.
    pub fn vertex_with_tol(&mut self, p: P3, tol: f64) -> usize {
        self.body.verts.push(Vertex {
            p,
            tol: tol.max(TOL_MIN),
        });
        self.body.verts.len() - 1
    }

    /// Adds the edge of `curve` from `t0` at vertex `v0` to `t1` at vertex `v1`; returns its
    /// index.
    pub fn edge(&mut self, curve: Curve, t0: f64, t1: f64, v0: usize, v1: usize) -> usize {
        self.body.add_edge(curve, t0, t1, v0, v1)
    }

    /// Adds a degenerate edge at `at` (a pole or apex) at vertex `v`, for the p-curve range
    /// `t0..t1`; returns its index.
    pub fn degenerate_edge(&mut self, at: P3, t0: f64, t1: f64, v: usize) -> usize {
        self.body.add_degenerate(at, t0, t1, v)
    }

    /// Adds the face of `surf` bounded by `loops` (the outer loop first, see [`Face`]), its
    /// outward normal the surface's natural one when `sense`; returns its index.
    pub fn face(&mut self, surf: Surface, sense: bool, loops: Vec<Vec<Coedge>>) -> usize {
        self.body.faces.push(Face { surf, sense, loops });
        self.body.faces.len() - 1
    }

    /// Adds a shell of `faces`, closed when it encloses a volume; returns its index.
    pub fn shell(&mut self, faces: Vec<usize>, closed: bool) -> usize {
        self.body.shells.push(Shell { faces, closed });
        self.body.shells.len() - 1
    }

    /// Adds a solid bounded by `shells` (the outer one first, then voids); returns its index.
    pub fn solid(&mut self, shells: Vec<usize>) -> usize {
        self.body.solids.push(Solid { shells });
        self.body.solids.len() - 1
    }

    /// Adds a wire: a chain of free edges, each with whether the chain runs along it; returns
    /// its index.
    pub fn wire(&mut self, edges: Vec<(usize, bool)>) -> usize {
        self.body.wires.push(Wire { edges });
        self.body.wires.len() - 1
    }

    /// Makes vertex `v` an acorn point (a vertex no edge uses).
    pub fn point(&mut self, v: usize) {
        self.body.points.push(v);
    }

    /// The body, checked with [`Body::check`] ([`CheckLevel::Fast`]).
    ///
    /// # Errors
    ///
    /// [`crate::KernelError::InvalidInput`] naming the first problem found (an index out of
    /// range, a loop that doesn't close, an open shell marked closed, …) and how many there are.
    pub fn build(self) -> crate::Result<Body> {
        self.build_with(CheckLevel::Fast)
    }

    /// The body, checked at `level`.
    ///
    /// # Errors
    ///
    /// As [`BodyBuilder::build`].
    pub fn build_with(self, level: CheckLevel) -> crate::Result<Body> {
        let errs = self.body.check_with(level);
        match errs.first() {
            None => Ok(self.body),
            Some(e) => Err(crate::KernelError::InvalidInput(format!(
                "the body isn't valid: {e} ({} problems)",
                errs.len()
            ))),
        }
    }
}

pub use crate::BodyClass;

/// A coedge's start and end parameters in its direction of travel.
pub fn co_range(e: &Edge, fwd: bool) -> (f64, f64) {
    if fwd {
        (e.t[0], e.t[1])
    } else {
        (e.t[1], e.t[0])
    }
}

impl Body {
    /// Adds a vertex of tolerance [`TOL_MIN`](super::tol::CONFUSION); returns its index.
    pub fn add_vertex(&mut self, p: P3) -> usize {
        self.verts.push(Vertex { p, tol: TOL_MIN });
        self.verts.len() - 1
    }

    /// Adds the edge of `curve` from `t0` at vertex `v0` to `t1` at `v1`, of tolerance
    /// [`TOL_MIN`](super::tol::CONFUSION); returns its index.
    pub fn add_edge(&mut self, curve: Curve, t0: f64, t1: f64, v0: usize, v1: usize) -> usize {
        self.edges.push(Edge {
            curve,
            t: [t0, t1],
            v: [v0, v1],
            degenerate: false,
            tol: TOL_MIN,
        });
        self.edges.len() - 1
    }

    /// Adds a degenerate edge at `at` (a pole or apex) at vertex `v` for the p-curve range
    /// `t0..t1`; returns its index.
    pub fn add_degenerate(&mut self, at: P3, t0: f64, t1: f64, v: usize) -> usize {
        self.edges.push(Edge {
            curve: Curve::Line {
                o: at,
                d: V3::zeros(),
            },
            t: [t0, t1],
            v: [v, v],
            degenerate: true,
            tol: TOL_MIN,
        });
        self.edges.len() - 1
    }

    /// The start vertex of a coedge.
    pub fn co_start(&self, c: &Coedge) -> usize {
        let e = &self.edges[c.edge];
        if c.fwd { e.v[0] } else { e.v[1] }
    }

    /// The end vertex of a coedge.
    pub fn co_end(&self, c: &Coedge) -> usize {
        let e = &self.edges[c.edge];
        if c.fwd { e.v[1] } else { e.v[0] }
    }

    /// For each edge, the faces using it (a seam lists its face twice).
    pub fn edge_faces(&self) -> Vec<Vec<usize>> {
        let mut out = vec![Vec::new(); self.edges.len()];
        for (fi, f) in self.faces.iter().enumerate() {
            for l in &f.loops {
                for c in l {
                    out[c.edge].push(fi);
                }
            }
        }
        out
    }

    /// Faces of each solid (and the free faces of sheets as a last group).
    pub fn solid_faces(&self, s: usize) -> Vec<usize> {
        self.solids[s]
            .shells
            .iter()
            .flat_map(|&sh| self.shells[sh].faces.iter().copied())
            .collect()
    }

    /// Appends another body (indices shifted). Returns the offsets (verts, edges, faces).
    pub fn append(&mut self, o: &Body) -> (usize, usize, usize) {
        let (vo, eo, fo, so) = (
            self.verts.len(),
            self.edges.len(),
            self.faces.len(),
            self.shells.len(),
        );
        self.verts.extend(o.verts.iter().cloned());
        for e in &o.edges {
            let mut e = e.clone();
            e.v = [e.v[0] + vo, e.v[1] + vo];
            self.edges.push(e);
        }
        for f in &o.faces {
            let mut f = f.clone();
            for l in &mut f.loops {
                for c in l {
                    c.edge += eo;
                }
            }
            self.faces.push(f);
        }
        for s in &o.shells {
            self.shells.push(Shell {
                faces: s.faces.iter().map(|x| x + fo).collect(),
                closed: s.closed,
            });
        }
        for s in &o.solids {
            self.solids.push(Solid {
                shells: s.shells.iter().map(|x| x + so).collect(),
            });
        }
        for w in &o.wires {
            self.wires.push(Wire {
                edges: w.edges.iter().map(|&(e, f)| (e + eo, f)).collect(),
            });
        }
        self.points.extend(o.points.iter().map(|v| v + vo));
        (vo, eo, fo)
    }

    /// The body moved by an affine map (a reflection flips every face's sense and loops).
    pub fn transformed(&self, m: &Affine) -> Body {
        let mut b = self.clone();
        for v in &mut b.verts {
            v.p = m.point(&v.p);
        }
        for e in &mut b.edges {
            e.curve = e.curve.transformed(m);
        }
        let mirror = m.det() < 0.0;
        let k = m.scale();
        for f in &mut b.faces {
            // Lengths in the surface's parameters scale with the body.
            if (k - 1.0).abs() > 1e-15 {
                let uv = match f.surf {
                    Surface::Plane { .. } => Some(V3::new(k, k, 1.0)),
                    Surface::Cylinder { .. } | Surface::Cone { .. } => Some(V3::new(1.0, k, 1.0)),
                    _ => None,
                };
                if let Some(d) = uv {
                    let a = Affine {
                        m: nalgebra::Matrix3::from_diagonal(&d),
                        t: V3::zeros(),
                    };
                    for l in &mut f.loops {
                        for c in l.iter_mut() {
                            c.pc = c.pc.transformed(&a);
                        }
                    }
                }
            }
            f.surf = f.surf.transformed(m);
            if mirror {
                f.sense = !f.sense;
                for l in &mut f.loops {
                    l.reverse();
                    for c in l.iter_mut() {
                        c.fwd = !c.fwd;
                    }
                }
            }
        }
        b
    }

    /// A box sure to hold the body: its vertices, its edges' exact boxes and its faces' boxes
    /// (curved faces bulge past their edges).
    pub fn bbox(&self) -> Bbox {
        let mut bb = Bbox::empty();
        for v in &self.verts {
            bb.add(&v.p);
        }
        for e in &self.edges {
            if !e.degenerate {
                bb.union(&super::boolean::curve_bbox(&e.curve, e.t[0], e.t[1]));
            }
        }
        for fi in 0..self.faces.len() {
            if !matches!(self.faces[fi].surf, Surface::Plane { .. }) {
                bb.union(&self.face_bbox(fi));
            }
        }
        bb
    }

    /// A box sure to hold face `fi`: its edges' exact boxes and its surface's box over the
    /// rectangle holding its p-curves.
    pub fn face_bbox(&self, fi: usize) -> Bbox {
        let f = &self.faces[fi];
        let mut bb = Bbox::empty();
        let mut uv = Bbox::empty();
        for c in f.loops.iter().flatten() {
            let e = &self.edges[c.edge];
            if !e.degenerate {
                bb.union(&super::boolean::curve_bbox(&e.curve, e.t[0], e.t[1]));
            }
            uv.union(&super::boolean::curve_bbox(&c.pc, e.t[0], e.t[1]));
        }
        if !matches!(f.surf, Surface::Plane { .. }) && !uv.is_empty() {
            bb.union(
                &f.surf
                    .patch_bbox((uv.min.x, uv.max.x), (uv.min.y, uv.max.y)),
            );
        }
        bb
    }

    /// `faces` split into the groups joined through shared edges (each group in the order
    /// `faces` lists them; groups ordered by their first face).
    pub fn face_components(&self, faces: &[usize]) -> Vec<Vec<usize>> {
        let mut parent: Vec<usize> = (0..faces.len()).collect();
        fn root(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let mut by_edge: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for (k, &fi) in faces.iter().enumerate() {
            for c in self.faces[fi].loops.iter().flatten() {
                let first = *by_edge.entry(c.edge).or_insert(k);
                let (a, b) = (root(&mut parent, first), root(&mut parent, k));
                parent[a.max(b)] = a.min(b);
            }
        }
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut slot: Vec<Option<usize>> = vec![None; faces.len()];
        for (k, &f) in faces.iter().enumerate() {
            let r = root(&mut parent, k);
            let g = *slot[r].get_or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[g].push(f);
        }
        groups
    }

    /// Whether the body has a solid.
    pub fn is_solid(&self) -> bool {
        !self.solids.is_empty()
    }

    /// Reverses a face's orientation (the outward normal flips): sense and loops.
    pub fn reverse_face(f: &mut Face) {
        f.sense = !f.sense;
        for l in &mut f.loops {
            l.reverse();
            for c in l.iter_mut() {
                c.fwd = !c.fwd;
            }
        }
    }

    /// Checks that every index the body holds refers to an entity it has (coedges to edges,
    /// edges to vertices, shells to faces, solids to shells) and that its geometry is well
    /// formed (finite points, valid B-splines): what every other method relies on without
    /// checking. Bodies from outside (files, the body format) are checked with it before use.
    /// Returns the problems (empty when sound).
    pub fn check_refs(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let finite = |p: &P3| p.iter().all(|x| x.is_finite());
        for (vi, v) in self.verts.iter().enumerate() {
            if !finite(&v.p) || !v.tol.is_finite() {
                errs.push(format!("vertex {vi} isn't finite"));
            }
        }
        for (ei, e) in self.edges.iter().enumerate() {
            if let Some(&v) = e.v.iter().find(|&&v| v >= self.verts.len()) {
                errs.push(format!("edge {ei}: vertex {v} doesn't exist"));
            }
            if !e.t.iter().all(|t| t.is_finite()) || !e.tol.is_finite() {
                errs.push(format!("edge {ei}: its range isn't finite"));
            }
            if let Some(why) = curve_issue(&e.curve) {
                errs.push(format!("edge {ei}: {why}"));
            }
        }
        for (fi, f) in self.faces.iter().enumerate() {
            if let Some(why) = surface_issue(&f.surf) {
                errs.push(format!("face {fi}: {why}"));
            }
            for (li, l) in f.loops.iter().enumerate() {
                if l.is_empty() {
                    errs.push(format!("face {fi} loop {li} is empty"));
                }
                for c in l {
                    if c.edge >= self.edges.len() {
                        errs.push(format!(
                            "face {fi} loop {li}: edge {} doesn't exist",
                            c.edge
                        ));
                    }
                    if let Some(why) = curve_issue(&c.pc) {
                        errs.push(format!("face {fi} loop {li}: p-curve: {why}"));
                    }
                }
            }
        }
        for (si, s) in self.shells.iter().enumerate() {
            if let Some(f) = s.faces.iter().find(|&&f| f >= self.faces.len()) {
                errs.push(format!("shell {si}: face {f} doesn't exist"));
            }
        }
        for (si, s) in self.solids.iter().enumerate() {
            if let Some(sh) = s.shells.iter().find(|&&sh| sh >= self.shells.len()) {
                errs.push(format!("solid {si}: shell {sh} doesn't exist"));
            }
        }
        for (wi, w) in self.wires.iter().enumerate() {
            if w.edges.is_empty() {
                errs.push(format!("wire {wi} is empty"));
            }
            if let Some((e, _)) = w.edges.iter().find(|&&(e, _)| e >= self.edges.len()) {
                errs.push(format!("wire {wi}: edge {e} doesn't exist"));
            }
        }
        if let Some(v) = self.points.iter().find(|&&v| v >= self.verts.len()) {
            errs.push(format!("point vertex {v} doesn't exist"));
        }
        errs
    }

    /// Checks the structural invariants at [`CheckLevel::Fast`], the level operations run on
    /// their results; returns a list of problems (empty when valid). A body whose indices or
    /// geometry are unsound ([`Body::check_refs`]) gets only those problems.
    pub fn check(&self) -> Vec<String> {
        self.check_with(CheckLevel::Fast)
    }

    /// Checks the body at `level` ([`CheckLevel`] says what each level covers); returns the
    /// problems found (empty when valid). [`CheckLevel::Full`] reports everything
    /// [`CheckLevel::Fast`] does and more, never less.
    pub fn check_with(&self, level: CheckLevel) -> Vec<String> {
        self.check_parts(level, true, None)
    }

    /// [`Body::check`] without its last test, the solids' volumes: for a caller that has them
    /// already (a boolean, which computes them to drop empty solids).
    pub fn check_structure(&self) -> Vec<String> {
        self.check_parts(CheckLevel::Fast, false, None)
    }

    /// [`CheckLevel::Full`] for a body made from a valid one by changing the faces `changed`
    /// marks (a local operation's result): everything [`CheckLevel::Fast`] checks over the
    /// whole body, and the full check's per-face tests (p-curves searched against their edges,
    /// p-curve ends on the faces, the faces' domains, faces crossing) on the changed faces and
    /// on the pairs with a changed face; the faces left as they were passed them before.
    pub fn check_changed(&self, changed: &[bool]) -> Vec<String> {
        self.check_parts(CheckLevel::Full, true, Some(changed))
    }

    fn check_parts(&self, level: CheckLevel, volumes: bool, only: Option<&[bool]>) -> Vec<String> {
        let full = level == CheckLevel::Full;
        // The faces the full check's per-face tests cover.
        let fully = |fi: usize| full && only.is_none_or(|c| c.get(fi).copied().unwrap_or(true));
        let refs = self.check_refs();
        if !refs.is_empty() {
            return refs;
        }
        let mut errs = Vec::new();
        // The faces' polygons, as the crossings' test makes them: the orientation's again.
        let mut polys: Vec<Option<Vec<Vec<P2>>>> = vec![None; self.faces.len()];
        for (fi, f) in self.faces.iter().enumerate() {
            for (li, l) in f.loops.iter().enumerate() {
                for k in 0..l.len() {
                    let (a, b) = (&l[k], &l[(k + 1) % l.len()]);
                    if self.co_end(a) != self.co_start(b) {
                        errs.push(format!(
                            "face {fi} loop {li}: coedge {k} doesn't meet the next"
                        ));
                    }
                    // p-curve ends meet in uv (modulo periods).
                    let ea = &self.edges[a.edge];
                    let eb = &self.edges[b.edge];
                    let (_, ta1) = co_range(ea, a.fwd);
                    let (tb0, _) = co_range(eb, b.fwd);
                    let pa = a.pc.point(ta1);
                    let pb = b.pc.point(tb0);
                    let du = (pa.x - pb.x).abs();
                    let dv = (pa.y - pb.y).abs();
                    let (mu, mv) = f.surf.metric();
                    // Within the shared vertex's tolerance (LOOP_GAP for exact geometry).
                    let gap_tol = self
                        .verts
                        .get(self.co_end(a))
                        .map_or(0.0, |v| v.tol)
                        .max(super::tol::LOOP_GAP);
                    if du * mu > gap_tol || dv * mv > gap_tol {
                        errs.push(format!(
                            "face {fi} loop {li}: p-curve gap after coedge {k}: ({:.3e}, {:.3e})",
                            pa.x - pb.x,
                            pa.y - pb.y
                        ));
                    }
                    // p-curve on the edge in 3D, within the edge's tolerance (a read model's
                    // p-curves stray by it): 7 points, or the largest gap searched for (Full).
                    if !ea.degenerate {
                        let allowed = ea.tol.max(super::tol::POINT);
                        let gap = if fully(fi) {
                            pcurve_gap(ea, &a.pc, &f.surf)
                        } else {
                            [0.1, 0.25, 0.4, 0.5, 0.6, 0.75, 0.9]
                                .into_iter()
                                .map(|s| {
                                    let t = ea.t[0] + (ea.t[1] - ea.t[0]) * s;
                                    let uv = a.pc.point(t);
                                    (f.surf.eval(uv.x, uv.y) - ea.point(t)).norm()
                                })
                                .find(|&d| d > allowed)
                                .unwrap_or(0.0)
                        };
                        if gap > allowed {
                            errs.push(format!("face {fi} loop {li} coedge {k} (edge {}): p-curve off the edge by {gap:.3e}", a.edge));
                        }
                    }
                }
            }
        }
        // Each edge ends at its vertices (within their tolerances).
        for (ei, e) in self.edges.iter().enumerate() {
            if e.degenerate {
                continue;
            }
            for k in 0..2 {
                let Some(v) = self.verts.get(e.v[k]) else {
                    errs.push(format!("edge {ei}: vertex {} doesn't exist", e.v[k]));
                    continue;
                };
                let d = (e.point(e.t[k]) - v.p).norm();
                if d > v.tol.max(e.tol).max(super::tol::POINT) {
                    errs.push(format!("edge {ei}: end {k} is {d:.3e} from its vertex"));
                }
            }
        }
        // Loops don't cross themselves in (u, v).
        for (fi, f) in self.faces.iter().enumerate() {
            for (li, l) in f.loops.iter().enumerate() {
                let mut poly: Vec<P2> = Vec::new();
                for c in l {
                    let e = &self.edges[c.edge];
                    let (t0, t1) = co_range(e, c.fwd);
                    for i in 0..8 {
                        let q = c.pc.point(t0 + (t1 - t0) * i as f64 / 8.0);
                        poly.push(P2::new(q.x, q.y));
                    }
                }
                // Eight samples a coedge can cut across a tightly curved p-curve: a crossing
                // they show stands only if the loop's polyline within LOOP_GAP of its p-curves
                // crosses itself too (the full check's test).
                if let Some((i, j)) = self_crossing(&poly)
                    && self
                        .check_face_domain(fi)
                        .iter()
                        .any(|e| e.starts_with(&format!("face {fi} loop {li} crosses itself")))
                {
                    errs.push(format!(
                        "face {fi} loop {li} crosses itself in (u, v) (segments {i} and {j})"
                    ));
                }
            }
        }
        // Each edge of a closed shell is used twice, once each way.
        let mut uses = vec![(0i32, 0i32); self.edges.len()];
        for f in &self.faces {
            for l in &f.loops {
                for c in l {
                    if c.fwd {
                        uses[c.edge].0 += 1;
                    } else {
                        uses[c.edge].1 += 1;
                    }
                }
            }
        }
        // Counted per shell: closed shells of one body may share an edge (two solids touching
        // along it, as a non-manifold model has them), each using it once each way.
        for s in &self.shells {
            if !s.closed {
                continue;
            }
            let mut shell_uses: std::collections::BTreeMap<usize, (i32, i32)> =
                std::collections::BTreeMap::new();
            for c in s
                .faces
                .iter()
                .flat_map(|&fi| self.faces[fi].loops.iter().flatten())
            {
                let u = shell_uses.entry(c.edge).or_default();
                if c.fwd {
                    u.0 += 1;
                } else {
                    u.1 += 1;
                }
            }
            for (&ei, &u) in &shell_uses {
                if u != (1, 1) && !self.edges[ei].degenerate {
                    errs.push(format!("edge {ei} used {u:?} times in a closed shell"));
                }
            }
        }
        errs.extend(self.check_kinds(&uses));
        if full {
            errs.extend(self.check_ends_on_faces(&fully));
            errs.extend(self.check_shells());
            // (Each face's on its own, in parallel; their problems in face order.)
            use rayon::prelude::*;
            let one = |fi: usize| {
                if fully(fi) {
                    self.check_face_domain(fi)
                } else {
                    Vec::new()
                }
            };
            let domain: Vec<Vec<String>> = if super::fillet::parallel(self.faces.len()) {
                (0..self.faces.len()).into_par_iter().map(one).collect()
            } else {
                (0..self.faces.len()).map(one).collect()
            };
            errs.extend(domain.into_iter().flatten());
            if errs.is_empty() {
                let marked = |f: usize| only.is_none_or(|c| c.get(f).copied().unwrap_or(true));
                errs.extend(super::check_faces::face_crossings_in(
                    self,
                    &|i, j| marked(i) || marked(j),
                    &mut polys,
                ));
            }
        }
        // Solids enclose a positive volume (their faces point out).
        if volumes && errs.is_empty() {
            for si in 0..self.solids.len() {
                let faces = self.solid_faces(si);
                let sub = super::kernel::subset(self, &faces, true);
                // (The subset's faces are the solid's, with their edges as they are: their
                // polygons the same.)
                let sp: Vec<Vec<Vec<P2>>> = faces
                    .iter()
                    .zip(&sub.faces)
                    .map(|(&fi, f)| {
                        polys
                            .get_mut(fi)
                            .and_then(Option::take)
                            .unwrap_or_else(|| super::classify::face_polygons(&sub, f))
                    })
                    .collect();
                if check_compare() {
                    for (k, f) in sub.faces.iter().enumerate() {
                        assert_eq!(
                            sp[k],
                            super::classify::face_polygons(&sub, f),
                            "solid {si}: face {k}'s polygons differ from the subset's"
                        );
                    }
                }
                // Ray parity settles the orientation where it can; else the exact volume.
                if parity_orientation_with(&sub, sp) == Some(true) {
                    continue;
                }
                let v = super::mass::body_volume(&sub);
                if !(v > 0.0) {
                    errs.push(format!(
                        "solid {si} encloses a volume of {v:.3e} (its faces point in)"
                    ));
                }
            }
            if full && errs.is_empty() {
                errs.extend(self.check_voids());
            }
        }
        errs
    }
}

impl Body {
    /// What the body is: one kind of piece (solids, sheets, wires or points), several
    /// (`Mixed`, a compound), or nothing. Free edges left in no wire don't count (they are
    /// construction leftovers, as before Phase 1b).
    pub fn class(&self) -> BodyClass {
        let in_solid: std::collections::HashSet<usize> = self
            .solids
            .iter()
            .flat_map(|s| s.shells.iter().copied())
            .collect();
        let in_shell: std::collections::HashSet<usize> = self
            .shells
            .iter()
            .flat_map(|s| s.faces.iter().copied())
            .collect();
        let solid = !self.solids.is_empty();
        // Faces of a shell of no solid, or of no shell at all.
        let sheet = (0..self.shells.len())
            .any(|s| !in_solid.contains(&s) && !self.shells[s].faces.is_empty())
            || (0..self.faces.len()).any(|f| !in_shell.contains(&f));
        let wire = !self.wires.is_empty();
        let point = !self.points.is_empty();
        match (solid, sheet, wire, point) {
            (false, false, false, false) => BodyClass::Empty,
            (true, false, false, false) => BodyClass::Solid,
            (false, true, false, false) => BodyClass::Sheet,
            (false, false, true, false) => BodyClass::Wire,
            (false, false, false, true) => BodyClass::Point,
            _ => BodyClass::Mixed,
        }
    }

    /// Whether wire `w` ends where it starts.
    pub fn wire_closed(&self, w: &Wire) -> bool {
        let (Some(&(e0, f0)), Some(&(e1, f1))) = (w.edges.first(), w.edges.last()) else {
            return false;
        };
        let start = self.edges[e0].v[if f0 { 0 } else { 1 }];
        let end = self.edges[e1].v[if f1 { 1 } else { 0 }];
        start == end
    }

    /// Adds an acorn point.
    pub fn add_point(&mut self, p: P3) -> usize {
        let v = self.add_vertex(p);
        self.points.push(v);
        v
    }

    /// The edges no face uses and no degenerate one (wire edges and construction leftovers).
    pub fn free_edges(&self) -> Vec<usize> {
        let mut used = vec![false; self.edges.len()];
        for c in self.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
            used[c.edge] = true;
        }
        (0..self.edges.len())
            .filter(|&e| !used[e] && !self.edges[e].degenerate)
            .collect()
    }

    /// Gathers the free edges in no wire into wires: each chain of edges meeting at vertices
    /// that only two of them share (a vertex where three or more meet ends the chains there).
    /// Chains start at an end (a vertex of one edge) where there is one, so open chains come
    /// out whole; what is left is closed loops.
    pub fn chain_wires(&mut self) {
        let mut taken = vec![false; self.edges.len()];
        for w in &self.wires {
            for &(e, _) in &w.edges {
                taken[e] = true;
            }
        }
        let free: Vec<usize> = self
            .free_edges()
            .into_iter()
            .filter(|&e| !taken[e])
            .collect();
        let mut at: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
        for &e in &free {
            at.entry(self.edges[e].v[0]).or_default().push(e);
            if !self.edges[e].closed() {
                at.entry(self.edges[e].v[1]).or_default().push(e);
            }
        }
        let degree = |v: usize| at.get(&v).map_or(0, |l| l.len());
        // Ends of chains first, then anything left (closed loops).
        let mut starts: Vec<(usize, bool)> = Vec::new();
        for &e in &free {
            let ed = &self.edges[e];
            if degree(ed.v[0]) != 2 {
                starts.push((e, true));
            }
            if !ed.closed() && degree(ed.v[1]) != 2 {
                starts.push((e, false));
            }
        }
        starts.extend(free.iter().map(|&e| (e, true)));
        for (e0, fwd0) in starts {
            if taken[e0] {
                continue;
            }
            taken[e0] = true;
            let mut w = Wire {
                edges: vec![(e0, fwd0)],
            };
            let first = self.edges[e0].v[if fwd0 { 0 } else { 1 }];
            loop {
                let &(e, f) = w.edges.last().unwrap_or(&(e0, fwd0));
                let end = self.edges[e].v[if f { 1 } else { 0 }];
                if end == first || degree(end) != 2 {
                    break;
                }
                let Some(&next) = at[&end].iter().find(|&&n| !taken[n]) else {
                    break;
                };
                taken[next] = true;
                w.edges.push((next, self.edges[next].v[0] == end));
            }
            self.wires.push(w);
        }
    }

    /// The rules of each kind of piece ([`Body::check`]): `uses` counts each edge's forward and
    /// reversed uses by faces.
    /// - solids: their shells are closed (and, as checked before, each edge of a closed shell is
    ///   used once each way and the faces point out);
    /// - sheets: manifold: an edge of open shells only has at most two uses (boundary edges
    ///   have one). Not required oriented: read and 2D-boolean sheets may use an edge twice the
    ///   same way;
    /// - wires: chains of free, non-degenerate edges, each edge starting where the one before
    ///   ends, each in one wire once; a body with wires has every free edge in one;
    /// - points: vertices no edge uses, each listed once.
    fn check_kinds(&self, uses: &[(i32, i32)]) -> Vec<String> {
        let mut errs = Vec::new();
        for (si, s) in self.solids.iter().enumerate() {
            for &sh in &s.shells {
                if !self.shells[sh].closed {
                    errs.push(format!("solid {si}: shell {sh} is open"));
                }
            }
        }
        let mut in_closed = vec![false; self.edges.len()];
        for s in self.shells.iter().filter(|s| s.closed) {
            for &fi in &s.faces {
                for c in self.faces[fi].loops.iter().flatten() {
                    in_closed[c.edge] = true;
                }
            }
        }
        for (ei, &(f, r)) in uses.iter().enumerate() {
            if in_closed[ei] || self.edges[ei].degenerate {
                continue;
            }
            if f + r > 2 {
                errs.push(format!(
                    "edge {ei} is used by {} faces (a sheet must be manifold)",
                    f + r
                ));
            }
        }
        let mut in_wire = vec![0usize; self.edges.len()];
        for (wi, w) in self.wires.iter().enumerate() {
            for (k, &(e, fwd)) in w.edges.iter().enumerate() {
                in_wire[e] += 1;
                if self.edges[e].degenerate {
                    errs.push(format!("wire {wi}: edge {e} is degenerate"));
                }
                if uses[e] != (0, 0) {
                    errs.push(format!("wire {wi}: edge {e} is an edge of a face"));
                }
                if let Some(&(n, nf)) = w.edges.get(k + 1) {
                    let end = self.edges[e].v[if fwd { 1 } else { 0 }];
                    let start = self.edges[n].v[if nf { 0 } else { 1 }];
                    if end != start {
                        errs.push(format!("wire {wi}: edge {k} doesn't meet the next"));
                    }
                }
            }
        }
        for (e, &n) in in_wire.iter().enumerate() {
            if n > 1 {
                errs.push(format!("edge {e} is in wires {n} times"));
            }
        }
        if !self.wires.is_empty()
            && let Some(e) = self.free_edges().into_iter().find(|&e| in_wire[e] == 0)
        {
            errs.push(format!("edge {e} is free but in no wire"));
        }
        let mut on_edge = vec![false; self.verts.len()];
        for e in &self.edges {
            on_edge[e.v[0]] = true;
            on_edge[e.v[1]] = true;
        }
        let mut seen = std::collections::HashSet::new();
        for &v in &self.points {
            if on_edge[v] {
                errs.push(format!("point vertex {v} is the end of an edge"));
            }
            if !seen.insert(v) {
                errs.push(format!("point vertex {v} is listed twice"));
            }
        }
        errs
    }

    /// The first of the body's points, curves and surfaces that reaches outside the size box
    /// ±`max` (vertices; centres, origins and radii of curves and surfaces; B-spline control
    /// points; p-curves, in parameters, are left alone): a file's model the kernel can't hold
    /// to its tolerances ([`super::tol::SIZE`]).
    pub fn size_issue(&self, max: f64) -> Option<String> {
        let out = |x: f64| !(x.abs() <= max);
        let pt = |p: &P3| p.iter().any(|x| out(*x));
        let curve = |c: &Curve| match c {
            Curve::Line { o, .. } => pt(o),
            Curve::Circle { c, r, .. } => pt(c) || out(*r),
            Curve::Ellipse { c, a, b, .. } => pt(c) || out(*a) || out(*b),
            Curve::Nurbs(n) => n.cps.iter().any(|h| pt(&super::nurbs::dehom(h))),
        };
        if let Some(i) = self.verts.iter().position(|v| pt(&v.p)) {
            return Some(format!(
                "vertex {i} lies outside the size box (±{max:e} mm)"
            ));
        }
        if let Some(i) = self.edges.iter().position(|e| curve(&e.curve)) {
            return Some(format!(
                "edge {i}'s curve reaches outside the size box (±{max:e} mm)"
            ));
        }
        let surf = |s: &Surface| match s {
            Surface::Plane { f } => pt(&f.o),
            Surface::Cylinder { f, r } | Surface::Sphere { f, r } | Surface::Cone { f, r, .. } => {
                pt(&f.o) || out(*r)
            }
            Surface::Torus { f, rmaj, rmin } => pt(&f.o) || out(*rmaj) || out(*rmin),
            Surface::Revolution { f, m } => pt(&f.o) || curve(m),
            Surface::Extrusion { c, .. } => curve(c),
            Surface::Nurbs(n) => n.cps.iter().flatten().any(|h| pt(&super::nurbs::dehom(h))),
        };
        let i = self.faces.iter().position(|f| surf(&f.surf))?;
        Some(format!(
            "face {i}'s surface reaches outside the size box (±{max:e} mm)"
        ))
    }
}

impl Body {
    /// [`Body::fit_tolerances`] measured as [`CheckLevel::Full`] measures: each p-curve's largest
    /// gap from its edge searched for (not taken at 7 points), and each p-curve's ends on the
    /// face against their vertices. For readers, whose models' tolerances must cover the gaps
    /// their curves actually leave (the p-curves they fit, the files' own); gaps beyond `cap`
    /// are left for the check to report.
    pub fn fit_tolerances_searched(&mut self, cap: f64) {
        self.fit_tolerances(cap);
        let mut etol = vec![0.0f64; self.edges.len()];
        let mut vtol = vec![0.0f64; self.verts.len()];
        for f in &self.faces {
            for c in f.loops.iter().flatten() {
                let e = &self.edges[c.edge];
                if !e.degenerate {
                    etol[c.edge] = etol[c.edge].max(pcurve_gap(e, &c.pc, &f.surf));
                }
                for k in 0..2 {
                    let uv = c.pc.point(e.t[k]);
                    let d = (f.surf.eval(uv.x, uv.y) - self.verts[e.v[k]].p).norm();
                    if d.is_finite() {
                        vtol[e.v[k]] = vtol[e.v[k]].max(d);
                    }
                }
            }
        }
        for (e, d) in self.edges.iter_mut().zip(etol) {
            if d <= cap && d * 1.01 > e.tol {
                e.tol = (d * 1.01).max(TOL_MIN);
            }
        }
        for (v, d) in self.verts.iter_mut().zip(vtol) {
            if d <= cap && d * 1.01 > v.tol {
                v.tol = (d * 1.01).max(TOL_MIN);
            }
        }
    }
}

/// Why a curve can't be evaluated (non-finite numbers, a malformed B-spline), if it can't.
fn curve_issue(c: &Curve) -> Option<String> {
    let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
    let ok = match c {
        Curve::Line { o, d } => finite(o.coords.as_slice()) && finite(d.as_slice()),
        Curve::Circle { c, x, y, r } => {
            finite(c.coords.as_slice())
                && finite(x.as_slice())
                && finite(y.as_slice())
                && r.is_finite()
        }
        Curve::Ellipse { c, x, y, a, b } => {
            finite(c.coords.as_slice())
                && finite(x.as_slice())
                && finite(y.as_slice())
                && a.is_finite()
                && b.is_finite()
        }
        Curve::Nurbs(n) => return n.validate().err().map(|e| e.message()),
    };
    (!ok).then(|| "a curve isn't finite".to_string())
}

/// Why a surface can't be evaluated, if it can't.
fn surface_issue(s: &Surface) -> Option<String> {
    let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
    let frame = |f: &Frame| {
        finite(f.o.coords.as_slice())
            && finite(f.x.as_slice())
            && finite(f.y.as_slice())
            && finite(f.z.as_slice())
    };
    let ok = match s {
        Surface::Plane { f } => frame(f),
        Surface::Cylinder { f, r } | Surface::Sphere { f, r } => frame(f) && r.is_finite(),
        Surface::Cone { f, r, a } => frame(f) && r.is_finite() && a.is_finite(),
        Surface::Torus { f, rmaj, rmin } => frame(f) && rmaj.is_finite() && rmin.is_finite(),
        Surface::Revolution { f, m } => {
            return if frame(f) {
                curve_issue(m)
            } else {
                Some("a surface isn't finite".into())
            };
        }
        Surface::Extrusion { c, d } => {
            return if finite(d.as_slice()) {
                curve_issue(c)
            } else {
                Some("a surface isn't finite".into())
            };
        }
        Surface::Nurbs(n) => return n.validate().err().map(|e| e.message()),
    };
    (!ok).then(|| "a surface isn't finite".to_string())
}

impl Body {
    /// Raises vertex and edge tolerances to the gaps actually there (an edge's ends from its
    /// vertices, its p-curves from its curve), up to `cap`: an operation that merged within a
    /// tolerance records it, as OCCT does. Gaps beyond `cap` are left for [`Body::check`].
    pub fn fit_tolerances(&mut self, cap: f64) {
        for ei in 0..self.edges.len() {
            let e = &self.edges[ei];
            if e.degenerate {
                continue;
            }
            for k in 0..2 {
                let d = (e.point(e.t[k]) - self.verts[e.v[k]].p).norm();
                if d <= cap && d * 1.01 > self.verts[e.v[k]].tol {
                    self.verts[e.v[k]].tol = (d * 1.01).max(TOL_MIN);
                }
            }
        }
        let mut etol = vec![0.0f64; self.edges.len()];
        for f in &self.faces {
            for c in f.loops.iter().flatten() {
                let e = &self.edges[c.edge];
                if e.degenerate {
                    continue;
                }
                for s in [0.1, 0.25, 0.4, 0.5, 0.6, 0.75, 0.9] {
                    let t = e.t[0] + (e.t[1] - e.t[0]) * s;
                    let uv = c.pc.point(t);
                    etol[c.edge] = etol[c.edge].max((f.surf.eval(uv.x, uv.y) - e.point(t)).norm());
                }
            }
        }
        for (e, d) in self.edges.iter_mut().zip(etol) {
            if d <= cap && d * 1.01 > e.tol {
                e.tol = (d * 1.01).max(TOL_MIN);
            }
        }
    }
}

/// The checks [`CheckLevel::Full`] adds to [`CheckLevel::Fast`]. Each runs on a body whose
/// references are sound ([`Body::check_refs`]).
impl Body {
    /// Each p-curve's ends on the face at the edge's vertices, and a degenerate edge's whole
    /// p-curve on its point, within the vertex's tolerance (or the edge's, if larger).
    fn check_ends_on_faces(&self, fully: &dyn Fn(usize) -> bool) -> Vec<String> {
        let mut errs = Vec::new();
        for (fi, f) in self.faces.iter().enumerate().filter(|(fi, _)| fully(*fi)) {
            for (li, l) in f.loops.iter().enumerate() {
                for (k, c) in l.iter().enumerate() {
                    let e = &self.edges[c.edge];
                    let on = |t: f64, v: usize| {
                        let uv = c.pc.point(t);
                        (f.surf.eval(uv.x, uv.y) - self.verts[v].p).norm()
                    };
                    let allowed = |v: usize| self.verts[v].tol.max(e.tol).max(super::tol::POINT);
                    for end in 0..2 {
                        let d = on(e.t[end], e.v[end]);
                        if !(d <= allowed(e.v[end])) {
                            errs.push(format!("face {fi} loop {li} coedge {k} (edge {}): p-curve end {end} lies {d:.3e} from vertex {}", c.edge, e.v[end]));
                        }
                    }
                    if e.degenerate {
                        let worst = (1..8)
                            .map(|i| on(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0, e.v[0]))
                            .fold(0.0, f64::max);
                        if !(worst <= allowed(e.v[0])) {
                            errs.push(format!("face {fi} loop {li} coedge {k} (degenerate edge {}): p-curve strays {worst:.3e} from its point", c.edge));
                        }
                    }
                }
            }
        }
        errs
    }

    /// Shells: each face in one shell at most, and each shell edge-connected (that each edge of
    /// a closed shell is used once each way by its faces, the fast check checks).
    fn check_shells(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let mut owner: Vec<Option<usize>> = vec![None; self.faces.len()];
        for (si, s) in self.shells.iter().enumerate() {
            if s.faces.is_empty() {
                errs.push(format!("shell {si} has no faces"));
                continue;
            }
            for &f in &s.faces {
                match owner[f] {
                    Some(o) if o == si => {
                        errs.push(format!("face {f} is listed twice in shell {si}"))
                    }
                    Some(o) => errs.push(format!("face {f} is in shells {o} and {si}")),
                    None => owner[f] = Some(si),
                }
            }
            let pieces = self.face_components(&s.faces).len();
            if pieces > 1 {
                errs.push(format!(
                    "shell {si} isn't connected: its faces make {pieces} pieces that share no edge"
                ));
            }
        }
        errs
    }

    /// Face `fi`'s loops in (u, v): none crosses itself or another, the first is the outer one
    /// (counter-clockwise about the outward normal), the others holes inside it running the
    /// other way, and no hole lies inside another.
    pub(crate) fn check_face_domain(&self, fi: usize) -> Vec<String> {
        let f = &self.faces[fi];
        let errs = Vec::new();
        // The loops as polylines in (u, v) scaled to lengths on the surface, each point with its
        // edge's tolerance plus how far its segment strays from the p-curve.
        let mut ubox = (f64::INFINITY, f64::NEG_INFINITY);
        let mut vbox = (f64::INFINITY, f64::NEG_INFINITY);
        for c in f.loops.iter().flatten() {
            let e = &self.edges[c.edge];
            for s in [0.0, 0.5, 1.0] {
                let q = c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * s);
                ubox = (ubox.0.min(q.x), ubox.1.max(q.x));
                vbox = (vbox.0.min(q.y), vbox.1.max(q.y));
            }
        }
        if !(ubox.0 <= ubox.1 && vbox.0 <= vbox.1) {
            return errs;
        }
        let (su, sv) = uv_scale(&f.surf, ubox, vbox);
        let flat = 0.5 * super::tol::LOOP_GAP;
        let rings: Vec<Ring> = f
            .loops
            .iter()
            .map(|l| {
                let mut r = Ring::default();
                for c in l {
                    let e = &self.edges[c.edge];
                    let (t0, t1) = co_range(e, c.fwd);
                    let tol = e.tol.max(super::tol::LOOP_GAP);
                    flatten(&c.pc, t0, t1, (su, sv), flat, tol, &mut r);
                }
                r
            })
            .collect();
        let errs = self.domain_errors(fi, &rings, (su, sv));
        if check_compare() {
            let old: Vec<Ring> = f
                .loops
                .iter()
                .map(|l| {
                    let mut r = Ring::default();
                    for c in l {
                        let e = &self.edges[c.edge];
                        let (t0, t1) = co_range(e, c.fwd);
                        flatten_ref(
                            &c.pc,
                            t0,
                            t1,
                            (su, sv),
                            flat,
                            e.tol.max(super::tol::LOOP_GAP),
                            &mut r,
                        );
                    }
                    r
                })
                .collect();
            // (The verdicts, not the points they name, which may differ in their last digits.)
            let gist = |x: &[String]| -> Vec<String> {
                x.iter()
                    .map(|m| m.split(" near ").next().unwrap_or(m).to_string())
                    .collect()
            };
            assert_eq!(
                gist(&errs),
                gist(&self.domain_errors(fi, &old, (su, sv))),
                "face {fi}: the domain check differs from the reference flattening's"
            );
        }
        errs
    }

    /// The domain check's problems with face `fi`'s loops flattened into `rings` (in the scaled
    /// (u, v) `scale`).
    fn domain_errors(&self, fi: usize, rings: &[Ring], (su, sv): (f64, f64)) -> Vec<String> {
        let f = &self.faces[fi];
        let mut errs = Vec::new();
        let crossings = loop_crossings(rings);
        for &(a, b, p) in &crossings {
            let (u, v) = (p.x / su, p.y / sv);
            if a == b {
                errs.push(format!(
                    "face {fi} loop {a} crosses itself in (u, v) near ({u:.6}, {v:.6})"
                ));
            } else {
                errs.push(format!(
                    "face {fi}: loops {a} and {b} cross in (u, v) near ({u:.6}, {v:.6})"
                ));
            }
        }
        if !crossings.is_empty() {
            return errs;
        }
        // Orientation and nesting (only meaningful for loops that don't cross).
        let sign = if f.sense { 1.0 } else { -1.0 };
        let areas: Vec<f64> = rings.iter().map(|r| r.area()).collect();
        let significant = |i: usize| areas[i].abs() > rings[i].perimeter() * rings[i].tol_max();
        if significant(0) && areas[0] * sign < 0.0 {
            errs.push(format!("face {fi}: its outer loop runs clockwise about the outward normal (area {:.3e} in (u, v))", areas[0]));
        }
        // Whole periods a hole may be shifted by to lie in the outer loop's range.
        let shifts = period_shifts(&f.surf, (su, sv));
        for i in 1..rings.len() {
            if significant(i) && areas[i] * sign > 0.0 {
                errs.push(format!(
                    "face {fi} loop {i}: a hole running the same way as the outer loop"
                ));
            }
            if rings[i].pts.len() >= 2
                && rings[0].pts.len() >= 3
                && !shifts
                    .iter()
                    .any(|d| rings[0].holds(&rings[i], *d) != Some(false))
            {
                errs.push(format!("face {fi} loop {i}: a hole outside the outer loop"));
            }
            for j in 1..rings.len() {
                if i != j
                    && rings[j].pts.len() >= 3
                    && rings[j].bbox_holds(&rings[i])
                    && rings[j].holds(&rings[i], V2::zeros()) == Some(true)
                {
                    errs.push(format!("face {fi}: hole {i} lies inside hole {j}"));
                }
            }
        }
        errs
    }

    /// Each solid of several shells: the outer shell encloses a positive volume, each void a
    /// negative one (its faces point into the void), and no vertex of a void lies outside the
    /// outer shell (on it is allowed).
    fn check_voids(&self) -> Vec<String> {
        let mut errs = Vec::new();
        for (si, s) in self.solids.iter().enumerate() {
            let [outer, voids @ ..] = s.shells.as_slice() else {
                continue;
            };
            if voids.is_empty() {
                continue;
            }
            let shell = |sh: usize| super::kernel::subset(self, &self.shells[sh].faces, true);
            let out = shell(*outer);
            let v = if parity_orientation(&out) == Some(true) {
                1.0
            } else {
                super::mass::body_volume(&out)
            };
            if !(v > 0.0) {
                errs.push(format!(
                    "solid {si}: its outer shell {outer} encloses a volume of {v:.3e}"
                ));
                continue;
            }
            let polys: Vec<Vec<Vec<P2>>> = out
                .faces
                .iter()
                .map(|f| super::classify::face_polygons(&out, f))
                .collect();
            for &sh in voids {
                let void = shell(sh);
                let v = if parity_orientation(&void) == Some(false) {
                    -1.0
                } else {
                    super::mass::body_volume(&void)
                };
                if !(v < 0.0) {
                    errs.push(format!("solid {si}: void shell {sh} encloses a volume of {v:.3e} (its faces must point into the void)"));
                }
                // A void may touch its outer shell (its vertices on it); none may lie outside.
                let outside = void.verts.iter().any(|v| {
                    super::classify::classify_point(
                        &out,
                        &polys,
                        &v.p,
                        v.tol.max(super::tol::POINT),
                    ) == crate::PointClass::Outside
                });
                if outside {
                    errs.push(format!(
                        "solid {si}: void shell {sh} lies outside the outer shell {outer}"
                    ));
                }
            }
        }
        errs
    }
}

/// The largest distance between edge `e`'s curve and its p-curve `pc` on `surf` (infinite if
/// either can't be evaluated): 33 samples, then a golden-section search about the worst.
pub(super) fn pcurve_gap(e: &Edge, pc: &Curve, surf: &Surface) -> f64 {
    const N: usize = 32;
    let gap = |t: f64| {
        let uv = pc.point(t);
        let d = (surf.eval(uv.x, uv.y) - e.point(t)).norm();
        if d.is_nan() { f64::INFINITY } else { d }
    };
    // A line on a plane along a straight p-curve: the gap is a length affine in t, largest at
    // an end.
    if matches!(
        (&e.curve, pc, surf),
        (
            Curve::Line { .. },
            Curve::Line { .. },
            Surface::Plane { .. }
        )
    ) {
        return gap(e.t[0]).max(gap(e.t[1]));
    }
    let at = |i: usize| e.t[0] + (e.t[1] - e.t[0]) * i as f64 / N as f64;
    let (mut worst, mut wi) = (0.0, 0);
    for i in 0..=N {
        let d = gap(at(i));
        if d > worst {
            (worst, wi) = (d, i);
        }
    }
    if worst.is_infinite() {
        return worst;
    }
    // The gap is smooth along the edge: its largest value lies within a sample of the worst.
    let (mut a, mut b) = (at(wi.saturating_sub(1)), at((wi + 1).min(N)));
    let r = 0.5 * (5f64.sqrt() - 1.0);
    let (mut x1, mut x2) = (b - r * (b - a), a + r * (b - a));
    let (mut g1, mut g2) = (gap(x1), gap(x2));
    for _ in 0..40 {
        if g1 > g2 {
            (b, x2, g2) = (x2, x1, g1);
            x1 = b - r * (b - a);
            g1 = gap(x1);
        } else {
            (a, x1, g1) = (x1, x2, g2);
            x2 = a + r * (b - a);
            g2 = gap(x2);
        }
    }
    worst.max(g1).max(g2)
}

/// The lengths on `surf` of a unit step in u and in v over the box `u` × `v`: the surface's own
/// metric for planes and quadrics, else the median of central differences on a 5 × 5 grid.
fn uv_scale(surf: &Surface, u: (f64, f64), v: (f64, f64)) -> (f64, f64) {
    if matches!(
        surf,
        Surface::Plane { .. }
            | Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
    ) {
        return surf.metric();
    }
    let (hu, hv) = (
        ((u.1 - u.0) * 1e-4).max(tol::ROUNDOFF),
        ((v.1 - v.0) * 1e-4).max(tol::ROUNDOFF),
    );
    let (mut du, mut dv) = (Vec::with_capacity(25), Vec::with_capacity(25));
    for i in 0..5 {
        for j in 0..5 {
            let x = u.0 + (u.1 - u.0) * (i as f64 + 0.5) / 5.0;
            let y = v.0 + (v.1 - v.0) * (j as f64 + 0.5) / 5.0;
            du.push((surf.eval(x + hu, y) - surf.eval(x - hu, y)).norm() / (2.0 * hu));
            dv.push((surf.eval(x, y + hv) - surf.eval(x, y - hv)).norm() / (2.0 * hv));
        }
    }
    let median = |mut d: Vec<f64>| {
        d.sort_by(f64::total_cmp);
        let m = d[d.len() / 2];
        if m.is_finite() && m > 0.0 { m } else { 1.0 }
    };
    (median(du), median(dv))
}

/// The shifts by whole periods (scaled as the rings are) worth trying to bring a hole into the
/// outer loop's range of a periodic surface: none first.
fn period_shifts(surf: &Surface, (su, sv): (f64, f64)) -> Vec<V2> {
    let periodic = |p: bool| {
        if p && !matches!(surf, Surface::Extrusion { .. }) {
            vec![0.0, -TAU, TAU]
        } else {
            vec![0.0]
        }
    };
    let (us, vs) = (periodic(surf.u_periodic()), periodic(surf.v_periodic()));
    us.iter()
        .flat_map(|&a| vs.iter().map(move |&b| V2::new(a * su, b * sv)))
        .collect()
}

/// A loop as a closed polyline in scaled (u, v): `pts[i]` starts segment i (to `pts[i + 1]`,
/// the last back to the first), which may stray `dev[i]` from the p-curves and whose edge has
/// tolerance `tol[i]`.
#[derive(Default)]
struct Ring {
    pts: Vec<P2>,
    tol: Vec<f64>,
}

impl Ring {
    fn seg(&self, i: usize) -> (P2, P2) {
        (self.pts[i], self.pts[(i + 1) % self.pts.len()])
    }

    /// Twice-halved shoelace: the signed area (counter-clockwise positive).
    fn area(&self) -> f64 {
        (0..self.pts.len())
            .map(|i| {
                let (a, b) = self.seg(i);
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>()
            * 0.5
    }

    fn perimeter(&self) -> f64 {
        (0..self.pts.len())
            .map(|i| {
                let (a, b) = self.seg(i);
                (b - a).norm()
            })
            .sum()
    }

    fn tol_max(&self) -> f64 {
        self.tol.iter().copied().fold(0.0, f64::max)
    }

    fn bbox(&self) -> (P2, P2) {
        let mut lo = P2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in &self.pts {
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        (lo, hi)
    }

    /// Whether `o`'s box lies within this ring's box.
    fn bbox_holds(&self, o: &Ring) -> bool {
        let ((a0, a1), (b0, b1)) = (self.bbox(), o.bbox());
        a0.x <= b0.x && a0.y <= b0.y && b1.x <= a1.x && b1.y <= a1.y
    }

    /// Whether ring `o`, moved by `d`, lies inside this one, judged on up to 16 of its points
    /// clear of this ring's boundary: `None` when every point tried lies on the boundary.
    fn holds(&self, o: &Ring, d: V2) -> Option<bool> {
        let step = o.pts.len().div_ceil(16).max(1);
        let tol = self.tol_max().max(o.tol_max());
        let mut verdict = None;
        for p in o.pts.iter().step_by(step).map(|p| p + d) {
            let near = (0..self.pts.len()).any(|i| {
                let (a, b) = self.seg(i);
                seg_dist2(&p, &a, &b) <= tol
            });
            if near {
                continue;
            }
            if !point_in_polygon(&p, &self.pts) {
                return Some(false);
            }
            verdict = Some(true);
        }
        verdict
    }
}

/// Appends coedge p-curve `pc` from `t0` towards `t1` to `r` (its end is the next coedge's
/// start): scaled by `scale`, split until each segment lies within `flat` of the curve (lines
/// whole; at most 4 · 2¹⁰ segments, a segment left coarser carrying how far it strays).
fn flatten(pc: &Curve, t0: f64, t1: f64, scale: (f64, f64), flat: f64, tol: f64, r: &mut Ring) {
    let at = |t: f64| {
        let q = pc.point(t);
        P2::new(q.x * scale.0, q.y * scale.1)
    };
    if matches!(pc, Curve::Line { .. }) {
        r.pts.push(at(t0));
        r.tol.push(tol);
        return;
    }
    // Each span's three samples (at a quarter, the middle and three quarters) are its halves'
    // middles and its split point, so a split evaluates the curve four times, not eight.
    #[allow(clippy::too_many_arguments, reason = "a recursion's state")]
    fn split(
        at: &dyn Fn(f64) -> P2,
        ta: f64,
        tb: f64,
        pa: P2,
        pb: P2,
        s: [P2; 3],
        depth: u32,
        flat: f64,
        tol: f64,
        r: &mut Ring,
    ) {
        let stray = s.iter().map(|q| seg_dist2(q, &pa, &pb)).fold(0.0, f64::max);
        if stray > flat && depth < 10 {
            let tm = 0.5 * (ta + tb);
            let pm = s[1];
            let quarters =
                |a: f64, b: f64, mid: P2| [at(a + (b - a) * 0.25), mid, at(a + (b - a) * 0.75)];
            split(
                at,
                ta,
                tm,
                pa,
                pm,
                quarters(ta, tm, s[0]),
                depth + 1,
                flat,
                tol,
                r,
            );
            split(
                at,
                tm,
                tb,
                pm,
                pb,
                quarters(tm, tb, s[2]),
                depth + 1,
                flat,
                tol,
                r,
            );
        } else {
            r.pts.push(pa);
            r.tol.push(tol + stray.max(flat));
        }
    }
    const N0: usize = 4;
    for i in 0..N0 {
        let (ta, tb) = (
            t0 + (t1 - t0) * i as f64 / N0 as f64,
            t0 + (t1 - t0) * (i + 1) as f64 / N0 as f64,
        );
        let s = [
            at(ta + (tb - ta) * 0.25),
            at(0.5 * (ta + tb)),
            at(ta + (tb - ta) * 0.75),
        ];
        split(&at, ta, tb, at(ta), at(tb), s, 0, flat, tol, r);
    }
}

/// Whether `CADRS_CHECK_COMPARE` is set: the full check also runs its reference versions
/// and panics where they disagree (for the gate, to show a faster check is as strict).
pub(crate) fn check_compare() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CADRS_CHECK_COMPARE").is_some())
}

/// [`flatten`] as it was (each span sampled afresh), for `CADRS_CHECK_COMPARE`: the domain
/// check run both ways must give the same verdicts.
fn flatten_ref(pc: &Curve, t0: f64, t1: f64, scale: (f64, f64), flat: f64, tol: f64, r: &mut Ring) {
    let at = |t: f64| {
        let q = pc.point(t);
        P2::new(q.x * scale.0, q.y * scale.1)
    };
    if matches!(pc, Curve::Line { .. }) {
        r.pts.push(at(t0));
        r.tol.push(tol);
        return;
    }
    #[allow(clippy::too_many_arguments, reason = "a recursion's state")]
    fn split(
        at: &dyn Fn(f64) -> P2,
        ta: f64,
        tb: f64,
        pa: P2,
        pb: P2,
        depth: u32,
        flat: f64,
        tol: f64,
        r: &mut Ring,
    ) {
        let stray = [0.25, 0.5, 0.75]
            .into_iter()
            .map(|s| seg_dist2(&at(ta + (tb - ta) * s), &pa, &pb))
            .fold(0.0, f64::max);
        if stray > flat && depth < 10 {
            let tm = 0.5 * (ta + tb);
            let pm = at(tm);
            split(at, ta, tm, pa, pm, depth + 1, flat, tol, r);
            split(at, tm, tb, pm, pb, depth + 1, flat, tol, r);
        } else {
            r.pts.push(pa);
            r.tol.push(tol + stray.max(flat));
        }
    }
    const N0: usize = 4;
    for i in 0..N0 {
        let (ta, tb) = (
            t0 + (t1 - t0) * i as f64 / N0 as f64,
            t0 + (t1 - t0) * (i + 1) as f64 / N0 as f64,
        );
        split(&at, ta, tb, at(ta), at(tb), 0, flat, tol, r);
    }
}

/// The pairs of loops (a loop with itself for a self-crossing) whose polylines cross properly
/// by more than their tolerance, each pair once with a point near the crossing. Neighbouring
/// segments of a loop are never compared. Segments are binned in a grid of about √n × √n cells
/// over their box, and only segments sharing a cell are compared (a dense p-curve's segments
/// would make a sweep along one axis quadratic).
fn loop_crossings(rings: &[Ring]) -> Vec<(usize, usize, P2)> {
    struct Seg {
        a: P2,
        b: P2,
        lp: usize,
        i: usize,
        tol: f64,
        lo: P2,
        hi: P2,
    }
    let mut segs: Vec<Seg> = Vec::new();
    for (lp, r) in rings.iter().enumerate() {
        if r.pts.len() < 2 {
            continue;
        }
        for i in 0..r.pts.len() {
            let (a, b) = r.seg(i);
            segs.push(Seg {
                a,
                b,
                lp,
                i,
                tol: r.tol[i],
                lo: P2::new(a.x.min(b.x), a.y.min(b.y)),
                hi: P2::new(a.x.max(b.x), a.y.max(b.y)),
            });
        }
    }
    let mut found: Vec<(usize, usize, P2)> = Vec::new();
    if segs.len() < 2 {
        return found;
    }
    let (mut lo, mut hi) = (segs[0].lo, segs[0].hi);
    for s in &segs {
        lo = P2::new(lo.x.min(s.lo.x), lo.y.min(s.lo.y));
        hi = P2::new(hi.x.max(s.hi.x), hi.y.max(s.hi.y));
    }
    let g = ((segs.len() as f64).sqrt().ceil() as usize).clamp(1, 512);
    let (w, h) = ((hi.x - lo.x).max(tol::TINY), (hi.y - lo.y).max(tol::TINY));
    let cell = |x: f64, y: f64| {
        (((x - lo.x) / w * g as f64) as usize).min(g - 1)
            + g * (((y - lo.y) / h * g as f64) as usize).min(g - 1)
    };
    let mut bins: Vec<Vec<usize>> = vec![Vec::new(); g * g];
    for (k, s) in segs.iter().enumerate() {
        let (c0, c1) = (cell(s.lo.x, s.lo.y), cell(s.hi.x, s.hi.y));
        for cy in c0 / g..=c1 / g {
            for cx in c0 % g..=c1 % g {
                bins[cx + g * cy].push(k);
            }
        }
    }
    for bin in &bins {
        for (n, &j) in bin.iter().enumerate() {
            let sj = &segs[j];
            for &k in &bin[..n] {
                let sk = &segs[k];
                let (pair_a, pair_b) = (sk.lp.min(sj.lp), sk.lp.max(sj.lp));
                if sk.hi.x < sj.lo.x
                    || sj.hi.x < sk.lo.x
                    || sk.hi.y < sj.lo.y
                    || sj.hi.y < sk.lo.y
                    || found.iter().any(|&(a, b, _)| (a, b) == (pair_a, pair_b))
                {
                    continue;
                }
                if sk.lp == sj.lp {
                    let len = rings[sk.lp].pts.len();
                    if sk.i.abs_diff(sj.i) <= 1 || sk.i.abs_diff(sj.i) == len - 1 {
                        continue;
                    }
                }
                if crosses(&sk.a, &sk.b, &sj.a, &sj.b, sk.tol.max(sj.tol)) {
                    found.push((pair_a, pair_b, sj.a));
                }
            }
        }
    }
    found
}

/// Whether segments a–b and c–d cross properly, each pair of ends lying more than `tol` to
/// either side of the other segment's line. The sides come from [`orient2d`]'s exact sign.
fn crosses(a: &P2, b: &P2, c: &P2, d: &P2, tol: f64) -> bool {
    let (o1, o2, o3, o4) = (
        orient2d(a, b, c),
        orient2d(a, b, d),
        orient2d(c, d, a),
        orient2d(c, d, b),
    );
    if o1 * o2 >= 0 || o3 * o4 >= 0 {
        return false;
    }
    let side = |p: &P2, q: &P2, r: &P2| {
        ((q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x)).abs()
            / (q - p).norm().max(tol::TINY)
    };
    side(a, b, c) > tol && side(a, b, d) > tol && side(c, d, a) > tol && side(c, d, b) > tol
}

/// The exact sign of the orientation of `a`, `b`, `c` (+1 counter-clockwise, −1 clockwise, 0
/// collinear): Shewchuk's filtered `orient2d`, falling back to the determinant summed exactly
/// as an expansion of error-free products and sums when the filter can't decide.
fn orient2d(a: &P2, b: &P2, c: &P2) -> i32 {
    let l = (a.x - c.x) * (b.y - c.y);
    let r = (a.y - c.y) * (b.x - c.x);
    let det = l - r;
    // Shewchuk's ccwerrboundA = (3 + 16ε)ε with ε = 2⁻⁵³.
    let bound = (3.0 + 16.0 * f64::EPSILON / 2.0) * (f64::EPSILON / 2.0) * (l.abs() + r.abs());
    if det.abs() > bound {
        return if det > 0.0 { 1 } else { -1 };
    }
    // (ax − cx)(by − cy) − (ay − cy)(bx − cx) expanded into six products, each split exactly
    // into a product and its rounding error, summed into a non-overlapping expansion whose
    // largest (last non-zero) component carries the sign.
    let terms = [
        (a.x, b.y),
        (-a.x, c.y),
        (-c.x, b.y),
        (-a.y, b.x),
        (a.y, c.x),
        (b.x, c.y),
    ];
    let mut e: Vec<f64> = Vec::with_capacity(24);
    for (x, y) in terms {
        let p = x * y;
        let err = x.mul_add(y, -p);
        for v in [err, p] {
            // Grow the expansion by v (Shewchuk's GROW-EXPANSION).
            let mut q = v;
            for h in e.iter_mut() {
                let s = q + *h;
                let bv = s - q;
                let low = (q - (s - bv)) + (*h - bv);
                *h = low;
                q = s;
            }
            e.push(q);
        }
    }
    match e.iter().rev().find(|&&x| x != 0.0) {
        Some(&x) if x > 0.0 => 1,
        Some(_) => -1,
        None => 0,
    }
}

/// Two non-neighbouring segments of a closed polygon that cross (proper crossings only:
/// touching at a point is allowed, as a loop through a pole or along a seam does).
fn self_crossing(p: &[P2]) -> Option<(usize, usize)> {
    let n = p.len();
    if n < 4 {
        return None;
    }
    let mut size: f64 = 0.0;
    for q in p {
        size = size.max(q.x.abs()).max(q.y.abs());
    }
    let eps = tol::REL * (1.0 + size);
    let cross = |a: &P2, b: &P2, c: &P2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        for j in i + 2..n {
            if (j + 1) % n == i {
                continue;
            }
            let (c, d) = (p[j], p[(j + 1) % n]);
            if a.x.max(b.x) < c.x.min(d.x) - eps
                || c.x.max(d.x) < a.x.min(b.x) - eps
                || a.y.max(b.y) < c.y.min(d.y) - eps
                || c.y.max(d.y) < a.y.min(b.y) - eps
            {
                continue;
            }
            let (d1, d2, d3, d4) = (
                cross(&a, &b, &c),
                cross(&a, &b, &d),
                cross(&c, &d, &a),
                cross(&c, &d, &b),
            );
            let l1 = (b - a).norm() * eps.max(tol::TINY);
            let l2 = (d - c).norm() * eps.max(tol::TINY);
            if ((d1 > l1 && d2 < -l1) || (d1 < -l1 && d2 > l1))
                && ((d3 > l2 && d4 < -l2) || (d3 < -l2 && d4 > l2))
            {
                return Some((i, j));
            }
        }
    }
    None
}

/// The exact orientation predicate of the full check.
#[cfg(test)]
mod predicate_tests {
    use super::{P2, crosses, orient2d};

    /// The exact sign of the orientation determinant of integer points (as i128).
    fn exact(a: (i64, i64), b: (i64, i64), c: (i64, i64)) -> i32 {
        let d = (a.0 as i128 - c.0 as i128) * (b.1 as i128 - c.1 as i128)
            - (a.1 as i128 - c.1 as i128) * (b.0 as i128 - c.0 as i128);
        d.signum() as i32
    }

    #[test]
    fn orient2d_is_exact_near_collinear() {
        // Points of up to 2⁵² (exact doubles, whose products aren't) far apart and nearly on
        // one line, a few units off it: the plain determinant rounds; the predicate must not.
        let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut wrong_naive = 0;
        for _ in 0..20000 {
            let big = 1u64 << 50;
            let a = ((next() % big) as i64, (next() % big) as i64);
            let (dx, dy) = ((next() % 1000) as i64 + 1, (next() % 1000) as i64 + 1);
            let k = (next() % (1 << 39)) as i64 + 1;
            let b = (a.0 + dx * k, a.1 + dy * k);
            let c = (
                a.0 + dx * 2 * k + (next() % 7) as i64 - 3,
                a.1 + dy * 2 * k + (next() % 7) as i64 - 3,
            );
            let f = |q: (i64, i64)| P2::new(q.0 as f64, q.1 as f64);
            let want = exact(a, b, c);
            assert_eq!(orient2d(&f(a), &f(b), &f(c)), want, "{a:?} {b:?} {c:?}");
            let naive =
                (f(a).x - f(c).x) * (f(b).y - f(c).y) - (f(a).y - f(c).y) * (f(b).x - f(c).x);
            if (naive > 0.0) as i32 - ((naive < 0.0) as i32) != want {
                wrong_naive += 1;
            }
        }
        assert!(wrong_naive > 0, "the cases never needed the exact fallback");
        assert_eq!(
            orient2d(&P2::new(0.0, 0.0), &P2::new(1.0, 1.0), &P2::new(2.0, 2.0)),
            0
        );
    }

    #[test]
    fn crossings_at_tolerance() {
        let p = P2::new;
        // An X crosses; a touch at an end, or a crossing shallower than the tolerance, doesn't.
        assert!(crosses(
            &p(0.0, 0.0),
            &p(2.0, 2.0),
            &p(0.0, 2.0),
            &p(2.0, 0.0),
            1e-6
        ));
        assert!(!crosses(
            &p(0.0, 0.0),
            &p(2.0, 0.0),
            &p(1.0, 0.0),
            &p(1.0, 1.0),
            1e-6
        ));
        assert!(!crosses(
            &p(0.0, 0.0),
            &p(2.0, 0.0),
            &p(1.0, -1e-7),
            &p(1.0, 1.0),
            1e-6
        ));
        assert!(crosses(
            &p(0.0, 0.0),
            &p(2.0, 0.0),
            &p(1.0, -1e-5),
            &p(1.0, 1.0),
            1e-6
        ));
    }
}

/// Body kinds (Phase 1b): each kind's `check`, mass properties, tessellation, the body format
/// and STEP/IGES round trips, sweeps one kind higher, the cylindrical box and the revolve about
/// an axis off the sketch plane.
#[cfg(test)]
mod kind_tests {
    use std::f64::consts::{PI, TAU};

    use nalgebra::{Point2, Point3, Unit, Vector3};

    use super::Body;
    use crate::exchange::ExchangeFormat;
    use crate::native::kernel::NativeKernel;
    use crate::*;

    fn near(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1.0)
    }

    fn line(a: [f64; 2], b: [f64; 2]) -> PathCurve {
        PathCurve::Sketch {
            plane: Plane::top(),
            curve: Curve2::Line {
                a: Point2::new(a[0], a[1]),
                b: Point2::new(b[0], b[1]),
                source: None,
            },
        }
    }

    /// A closed 10 × 10 square wire in the top plane.
    fn square(k: &mut NativeKernel) -> BodyId {
        let c = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let curves: Vec<PathCurve> = (0..4).map(|i| line(c[i], c[(i + 1) % 4])).collect();
        k.create_wire(&curves).unwrap().bodies[0]
    }

    fn body(k: &NativeKernel, id: BodyId) -> &Body {
        k.get(id).unwrap()
    }

    fn valid(k: &NativeKernel, id: BodyId) {
        let issues = body(k, id).check();
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// Round trips through STEP, IGES and the body format; returns the bodies read.
    fn round_trips(k: &mut NativeKernel, id: BodyId) -> Vec<BodyId> {
        let bytes = k.write_body(id).unwrap();
        let back = k.read_body(&bytes).unwrap();
        assert_eq!(
            body(k, back),
            body(k, id),
            "the body format changed the body"
        );
        let mut out = vec![back];
        for format in [ExchangeFormat::Step, ExchangeFormat::Iges] {
            let file = k
                .export_model(format, "kinds", &[(id, "part".into())], &[])
                .unwrap();
            let m = k
                .import_model(format, &file)
                .unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert_eq!(m.parts.len(), 1, "{format:?}");
            out.push(m.parts[0].body);
        }
        out
    }

    #[test]
    fn point_bodies() {
        let mut k = NativeKernel::new();
        let id = k
            .create_points(&[Point3::new(1.0, 2.0, 3.0), Point3::new(3.0, 4.0, 5.0)])
            .unwrap()
            .bodies[0];
        assert_eq!(k.body_class(id).unwrap(), BodyClass::Point);
        valid(&k, id);
        let m = k.mass_properties(id).unwrap();
        assert_eq!((m.volume, m.surface_area, m.length), (0.0, 0.0, 0.0));
        assert!((m.center_of_mass - Point3::new(2.0, 3.0, 4.0)).norm() < 1e-12);
        let t = k.tessellate(id, Tessellation::default()).unwrap();
        assert_eq!(t.points.len(), 2);
        assert!(t.indices.is_empty() && t.edges.is_empty());
        assert_eq!(k.vertices(id).unwrap().len(), 2);
        assert!(k.faces(id).unwrap().is_empty() && k.edges(id).unwrap().is_empty());
        for b in round_trips(&mut k, id) {
            assert_eq!(k.body_class(b).unwrap(), BodyClass::Point);
            valid(&k, b);
            let pts: Vec<Point3<f64>> = body(&k, b)
                .points
                .iter()
                .map(|&v| body(&k, b).verts[v].p)
                .collect();
            assert_eq!(pts.len(), 2);
            assert!((pts[1] - Point3::new(3.0, 4.0, 5.0)).norm() < 1e-9);
        }
        assert!(matches!(
            k.create_points(&[]),
            Err(KernelError::InvalidParameter(_))
        ));
        assert!(matches!(
            k.create_points(&[Point3::new(f64::NAN, 0.0, 0.0)]),
            Err(KernelError::InvalidInput(_))
        ));
    }

    #[test]
    fn wire_bodies() {
        let mut k = NativeKernel::new();
        let id = square(&mut k);
        assert_eq!(k.body_class(id).unwrap(), BodyClass::Wire);
        valid(&k, id);
        let b = body(&k, id);
        assert_eq!(b.wires.len(), 1);
        assert!(b.wire_closed(&b.wires[0]));
        let m = k.mass_properties(id).unwrap();
        assert!(near(m.length, 40.0, 1e-12));
        assert!((m.center_of_mass - Point3::new(5.0, 5.0, 0.0)).norm() < 1e-9);
        let t = k.tessellate(id, Tessellation::default()).unwrap();
        assert_eq!(t.edges.len(), 4);
        assert!(t.indices.is_empty());
        let es = k.edges(id).unwrap();
        assert_eq!(es.len(), 4);
        assert!(es.iter().all(|e| e.faces == [None, None]));
        assert_eq!(k.vertices(id).unwrap().len(), 4);
        // An open wire with an arc: a quarter circle of radius 5, then a line of 3.
        let arc = PathCurve::Sketch {
            plane: Plane::top(),
            curve: Curve2::Arc {
                center: Point2::origin(),
                radius: 5.0,
                start_angle: 0.0,
                sweep: PI / 2.0,
                source: None,
            },
        };
        let open = k
            .create_wire(&[line([0.0, 5.0], [-3.0, 5.0]), arc])
            .unwrap()
            .bodies[0];
        valid(&k, open);
        let ob = body(&k, open);
        assert!(!ob.wire_closed(&ob.wires[0]));
        let want = 5.0 * PI / 2.0 + 3.0;
        assert!(near(k.mass_properties(open).unwrap().length, want, 1e-9));
        for b in round_trips(&mut k, id) {
            assert_eq!(k.body_class(b).unwrap(), BodyClass::Wire, "{b:?}");
            valid(&k, b);
            let bb = body(&k, b);
            assert!(bb.wires.len() == 1 && bb.wire_closed(&bb.wires[0]));
            assert!(near(k.mass_properties(b).unwrap().length, 40.0, 1e-9));
        }
        for b in round_trips(&mut k, open) {
            valid(&k, b);
            assert!(
                near(k.mass_properties(b).unwrap().length, want, 1e-9),
                "{}",
                k.mass_properties(b).unwrap().length
            );
        }
        // Curves that don't join are refused.
        assert!(
            k.create_wire(&[line([0.0, 0.0], [1.0, 0.0]), line([5.0, 5.0], [6.0, 5.0])])
                .is_err()
        );
    }

    #[test]
    fn check_finds_broken_kinds() {
        let mut k = NativeKernel::new();
        let id = square(&mut k);
        let good = body(&k, id).clone();
        // A wire whose edges don't meet.
        let mut b = good.clone();
        b.wires[0].edges.swap(1, 2);
        assert!(
            b.check().iter().any(|i| i.contains("doesn't meet")),
            "{:?}",
            b.check()
        );
        // A free edge left out of the wires.
        let mut b = good.clone();
        b.wires[0].edges.pop();
        assert!(
            b.check().iter().any(|i| i.contains("in no wire")),
            "{:?}",
            b.check()
        );
        // An edge in two wires; a wire edge out of range.
        let mut b = good.clone();
        b.wires.push(super::Wire {
            edges: vec![b.wires[0].edges[0]],
        });
        assert!(b.check().iter().any(|i| i.contains("in wires 2 times")));
        let mut b = good.clone();
        b.wires[0].edges.push((99, true));
        assert!(b.check_refs().iter().any(|i| i.contains("edge 99")));
        // A point on an edge, and listed twice.
        let mut b = good.clone();
        b.points = vec![0, 0];
        let issues = b.check();
        assert!(
            issues.iter().any(|i| i.contains("end of an edge"))
                && issues.iter().any(|i| i.contains("twice")),
            "{issues:?}"
        );
        // A sheet whose edge three faces use (non-manifold).
        let sheet = k
            .sweep_body(id, &BodySweep::Translate(Vector3::new(0.0, 0.0, 5.0)))
            .unwrap()
            .bodies[0];
        let mut b = body(&k, sheet).clone();
        valid(&k, sheet);
        b.faces.push(b.faces[0].clone());
        b.shells[0].faces.push(b.faces.len() - 1);
        assert!(
            b.check().iter().any(|i| i.contains("manifold")),
            "{:?}",
            b.check()
        );
        // A solid whose shell isn't closed.
        let mut c = k
            .extrude(
                &Profile::new(
                    Plane::top(),
                    vec![Region {
                        outer: Loop {
                            curves: vec![Curve2::Circle {
                                center: Point2::origin(),
                                radius: 1.0,
                                source: None,
                            }],
                        },
                        holes: vec![],
                        source: None,
                    }],
                ),
                Extent::Blind(1.0),
            )
            .map(|r| body(&k, r.bodies[0]).clone())
            .unwrap();
        c.shells[0].closed = false;
        assert!(c.check().iter().any(|i| i.contains("is open")));
    }

    #[test]
    fn sweeps_one_kind_higher() {
        let mut k = NativeKernel::new();
        // Points into a wire, straight and turned (part and whole).
        let p = k
            .create_points(&[Point3::new(5.0, 0.0, 0.0)])
            .unwrap()
            .bodies[0];
        let w = k
            .sweep_body(p, &BodySweep::Translate(Vector3::new(0.0, 3.0, 4.0)))
            .unwrap()
            .bodies[0];
        assert_eq!(k.body_class(w).unwrap(), BodyClass::Wire);
        assert!(near(k.mass_properties(w).unwrap().length, 5.0, 1e-12));
        let z = Axis {
            origin: Point3::origin(),
            dir: Vector3::z_axis(),
        };
        let arc = k
            .sweep_body(
                p,
                &BodySweep::Rotate {
                    axis: z,
                    angle: PI / 2.0,
                },
            )
            .unwrap()
            .bodies[0];
        valid(&k, arc);
        assert!(near(
            k.mass_properties(arc).unwrap().length,
            2.5 * PI,
            1e-12
        ));
        let ring = k
            .sweep_body(
                p,
                &BodySweep::Rotate {
                    axis: z,
                    angle: -TAU,
                },
            )
            .unwrap()
            .bodies[0];
        valid(&k, ring);
        assert!(body(&k, ring).edges[0].closed());
        assert!(near(
            k.mass_properties(ring).unwrap().length,
            10.0 * PI,
            1e-12
        ));
        // A point on the axis sweeps nothing.
        let o = k
            .create_points(&[Point3::new(0.0, 0.0, 2.0)])
            .unwrap()
            .bodies[0];
        assert!(
            k.sweep_body(
                o,
                &BodySweep::Rotate {
                    axis: z,
                    angle: 1.0
                }
            )
            .is_err()
        );
        // A wire into a sheet: the square's walls.
        let sq = square(&mut k);
        let walls = k
            .sweep_body(sq, &BodySweep::Translate(Vector3::new(0.0, 0.0, 5.0)))
            .unwrap();
        let wb = walls.bodies[0];
        valid(&k, wb);
        assert_eq!(k.body_class(wb).unwrap(), BodyClass::Sheet);
        assert!(near(
            k.mass_properties(wb).unwrap().surface_area,
            200.0,
            1e-12
        ));
        assert_eq!(walls.history.generated.len(), 4);
        assert!(
            walls
                .history
                .generated
                .iter()
                .all(|(_, o)| matches!(o, Origin::FromEdge { body, .. } if *body == sq))
        );
        // A segment into a flat sheet, then that into a box.
        let seg = k
            .create_wire(&[line([0.0, 0.0], [10.0, 0.0])])
            .unwrap()
            .bodies[0];
        let flat = k
            .sweep_body(seg, &BodySweep::Translate(Vector3::new(0.0, 10.0, 0.0)))
            .unwrap()
            .bodies[0];
        valid(&k, flat);
        assert!(near(
            k.mass_properties(flat).unwrap().surface_area,
            100.0,
            1e-12
        ));
        let boxed = k
            .sweep_body(flat, &BodySweep::Translate(Vector3::new(0.0, 0.0, -5.0)))
            .unwrap()
            .bodies[0];
        valid(&k, boxed);
        assert_eq!(k.body_class(boxed).unwrap(), BodyClass::Solid);
        let m = k.mass_properties(boxed).unwrap();
        assert!(
            near(m.volume, 500.0, 1e-12) && near(m.surface_area, 400.0, 1e-12),
            "{m:?}"
        );
        // A sheet swept along itself makes no solid.
        assert!(matches!(
            k.sweep_body(flat, &BodySweep::Translate(Vector3::new(1.0, 0.0, 0.0))),
            Err(KernelError::InvalidParameter(_))
        ));
        // Turned: a segment parallel to the axis into a cylinder, its rectangle into a tube.
        let up = k
            .create_wire(&[PathCurve::Bezier3 {
                poles: vec![Point3::new(5.0, 0.0, 0.0), Point3::new(5.0, 0.0, 4.0)],
                weights: vec![],
            }])
            .unwrap()
            .bodies[0];
        let cyl = k
            .sweep_body(
                up,
                &BodySweep::Rotate {
                    axis: z,
                    angle: TAU,
                },
            )
            .unwrap()
            .bodies[0];
        valid(&k, cyl);
        assert!(near(
            k.mass_properties(cyl).unwrap().surface_area,
            40.0 * PI,
            1e-9
        ));
        let rect = k
            .sweep_body(up, &BodySweep::Translate(Vector3::new(2.0, 0.0, 0.0)))
            .unwrap()
            .bodies[0];
        let tube = k
            .sweep_body(
                rect,
                &BodySweep::Rotate {
                    axis: z,
                    angle: TAU,
                },
            )
            .unwrap()
            .bodies[0];
        valid(&k, tube);
        assert!(near(
            k.mass_properties(tube).unwrap().volume,
            PI * 24.0 * 4.0,
            1e-9
        ));
        let quarter = k
            .sweep_body(
                rect,
                &BodySweep::Rotate {
                    axis: z,
                    angle: PI / 2.0,
                },
            )
            .unwrap()
            .bodies[0];
        valid(&k, quarter);
        assert!(near(
            k.mass_properties(quarter).unwrap().volume,
            PI * 24.0,
            1e-9
        ));
        // A segment skew to the axis turned: a hyperboloid; area 2π ∫ ρ √(ρ'² + h'²) dt.
        let skew = k
            .create_wire(&[PathCurve::Bezier3 {
                poles: vec![Point3::new(5.0, -1.0, 0.0), Point3::new(5.0, 1.0, 3.0)],
                weights: vec![],
            }])
            .unwrap()
            .bodies[0];
        let hyp = k
            .sweep_body(
                skew,
                &BodySweep::Rotate {
                    axis: z,
                    angle: TAU,
                },
            )
            .unwrap()
            .bodies[0];
        valid(&k, hyp);
        let n = 20000;
        let want: f64 = (0..n)
            .map(|i| {
                let t = (i as f64 + 0.5) / n as f64;
                let (y, dy) = (-1.0 + 2.0 * t, 2.0);
                let rho = (25.0 + y * y).sqrt();
                TAU * rho * ((y * dy / rho).powi(2) + 9.0).sqrt() / n as f64
            })
            .sum();
        assert!(
            near(k.mass_properties(hyp).unwrap().surface_area, want, 1e-6),
            "{} vs {want}",
            k.mass_properties(hyp).unwrap().surface_area
        );
        for b in round_trips(&mut k, hyp) {
            assert!(near(k.mass_properties(b).unwrap().surface_area, want, 1e-6));
        }
    }

    #[test]
    fn cylindrical_box() {
        let mut k = NativeKernel::new();
        let spec = |len: f64, ri: f64, ro: f64, a0: f64, a1: f64| CylindricalBox {
            frame: Plane::top(),
            length: len,
            inner_radius: ri,
            outer_radius: ro,
            start_angle: a0.to_radians(),
            end_angle: a1.to_radians(),
        };
        let id = k
            .cylindrical_box(&spec(10.0, 2.0, 5.0, 30.0, 120.0))
            .unwrap()
            .bodies[0];
        valid(&k, id);
        let m = k.mass_properties(id).unwrap();
        let th = PI / 2.0;
        assert!(near(m.volume, th / 2.0 * 21.0 * 10.0, 1e-9));
        assert!(near(
            m.surface_area,
            th * 21.0 + th * 7.0 * 10.0 + 2.0 * 3.0 * 10.0,
            1e-9
        ));
        // It starts at 30°: its centroid lies at 75°.
        let c = m.center_of_mass;
        assert!((c.y.atan2(c.x) - 75f64.to_radians()).abs() < 1e-9);
        let tube = k
            .cylindrical_box(&spec(1.0, 1.0, 2.0, 0.0, 360.0))
            .unwrap()
            .bodies[0];
        valid(&k, tube);
        assert!(near(
            k.mass_properties(tube).unwrap().volume,
            3.0 * PI,
            1e-9
        ));
        for bad in [
            spec(0.0, 2.0, 5.0, 0.0, 90.0),
            spec(-1.0, 2.0, 5.0, 0.0, 90.0),
            spec(10.0, 0.0, 5.0, 0.0, 90.0),
            spec(10.0, 5.0, 2.0, 0.0, 90.0),
            spec(10.0, 5.0, 5.0, 0.0, 90.0),
            spec(10.0, 2.0, 5.0, 400.0, 450.0),
            spec(10.0, 2.0, 5.0, -10.0, 50.0),
            spec(10.0, 2.0, 5.0, 90.0, 10.0),
            spec(10.0, 2.0, 5.0, 0.0, 400.0),
            spec(f64::NAN, 2.0, 5.0, 0.0, 90.0),
            spec(10.0, 2.0, f64::INFINITY, 0.0, 90.0),
        ] {
            assert!(
                matches!(
                    k.cylindrical_box(&bad),
                    Err(KernelError::InvalidParameter(_))
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn revolve_about_an_axis_off_the_plane() {
        let mut k = NativeKernel::new();
        // The square [1, 2] × [0, 1] of the top plane about the y-parallel axis one below it:
        // swept volume θ ∫ x dA = 1.5 θ (each point leaves the plane at speed x).
        let sq = Region {
            outer: Loop {
                curves: [[1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0]]
                    .iter()
                    .zip([[2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 0.0]])
                    .map(|(a, b)| Curve2::Line {
                        a: Point2::new(a[0], a[1]),
                        b: Point2::new(b[0], b[1]),
                        source: None,
                    })
                    .collect(),
            },
            holes: vec![],
            source: None,
        };
        let p = Profile::new(Plane::top(), vec![sq]);
        let axis = Axis {
            origin: Point3::new(0.0, 0.0, -1.0),
            dir: Vector3::y_axis(),
        };
        for (angle, vol) in [(PI / 2.0, 0.75 * PI), (TAU, 3.0 * PI)] {
            let id = k.revolve(&p, axis, angle).unwrap().bodies[0];
            valid(&k, id);
            assert!(
                near(k.mass_properties(id).unwrap().volume, vol, 1e-9),
                "{angle}: {}",
                k.mass_properties(id).unwrap().volume
            );
        }
        // A skew axis (SMLib sweep 31's kind): valid, and volume θ ∫ (z × r)·n dA.
        let skew = Axis {
            origin: Point3::new(4.0, 2.0, -3.0),
            dir: Unit::new_normalize(Vector3::new(-2.0, 5.0, 1.0)),
        };
        let id = k.revolve(&p, skew, 1.0).unwrap().bodies[0];
        valid(&k, id);
        let (z, o) = (skew.dir.into_inner(), skew.origin);
        let speed = |x: f64, y: f64| z.cross(&(Point3::new(x, y, 0.0) - o)).z.abs();
        let n = 200;
        let want: f64 = (0..n * n)
            .map(|i| {
                speed(
                    1.0 + ((i % n) as f64 + 0.5) / n as f64,
                    ((i / n) as f64 + 0.5) / n as f64,
                ) / (n * n) as f64
            })
            .sum();
        assert!(
            near(k.mass_properties(id).unwrap().volume, want, 1e-6),
            "{} vs {want}",
            k.mass_properties(id).unwrap().volume
        );
        // An axis square to the plane, or one the profile straddles, is refused.
        let normal = Axis {
            origin: Point3::new(0.0, 0.0, -1.0),
            dir: Vector3::z_axis(),
        };
        assert!(k.revolve(&p, normal, 1.0).is_err());
        let across = Axis {
            origin: Point3::new(1.5, 0.0, -1.0),
            dir: Vector3::y_axis(),
        };
        assert!(k.revolve(&p, across, 1.0).is_err());
    }
}

#[cfg(test)]
mod tests {
    use crate::native::draw::Shape;
    use crate::native::kernel::NativeKernel;

    /// The volume test settles a solid's orientation by ray parity (a box), and reports an
    /// inside-out solid with its exact volume as before.
    #[test]
    fn an_inside_out_solid_is_reported_with_its_volume() {
        let mut k = NativeKernel::new();
        let Shape::Body(id) = k.box_(10.0, 20.0, 30.0).unwrap() else {
            unreachable!()
        };
        let mut b = (**k.get(id).unwrap()).clone();
        assert_eq!(b.check(), Vec::<String>::new());
        for f in &mut b.faces {
            f.sense = !f.sense;
            for l in &mut f.loops {
                l.reverse();
                for c in l.iter_mut() {
                    c.fwd = !c.fwd;
                }
            }
        }
        assert_eq!(
            b.check(),
            vec!["solid 0 encloses a volume of -6.000e3 (its faces point in)".to_string()]
        );
    }
}
