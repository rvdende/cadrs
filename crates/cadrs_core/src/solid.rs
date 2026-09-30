//! Extruded solids: the display mesh of a part ([`Solid`]), with the persistent name of every
//! face, edge and vertex (P3.2, [`cadrs_kernel::naming`]). Parts come from the kernel
//! (`crate::brep` converts its tessellation into a [`Solid`]); [`extrude`] is the M9 prism
//! mesh, kept as the fallback without the `occt` feature and as the reference the kernel's
//! names are tested against. The prism is the region's triangulated caps plus one side face per
//! sketch curve, named as the kernel names them (by region key and sketch curve).
//!
//! - **Faces** ([`FaceName`]): the end cap (the "top", at the extrusion's far end), the start
//!   cap (on the sketch plane) and a side face per boundary curve (outer boundary and holes).
//!   Lines give planar side faces; arcs and circles cylindrical ones. Planar faces carry a
//!   [`PlaneFrame`] (outward normal `u × v`) so they can be sketched on.
//! - **Edges** ([`EdgeName`]): each boundary curve at both caps, and the lateral edges where
//!   two curves meet. Cylindrical faces also record their rulings, so the app can draw their
//!   silhouettes.
//! - **Vertices** ([`VertexName`]): where three or more faces meet (kernel parts only).
//! - Triangles wind counter-clockwise seen from outside; normals point out. Each face has its
//!   own vertices (flat shading), except that a curved face shares normals along its curve.

use cadrs_kernel::naming::{self, Lost, Match};
use cadrs_sketch::region::Region;
pub use cadrs_sketch::{EdgeName, EdgeTag, FaceName, FaceOrigin, OpId, VertexName};
use cadrs_sketch::{CurveId, PlaneFrame, Vec2, Vec3};
use slotmap::Key;

/// A face of a [`Solid`].
#[derive(Debug, Clone, PartialEq)]
pub struct SolidFace {
    pub name: FaceName,
    /// The face's plane (outward normal `u × v`), if it is planar.
    pub plane: Option<PlaneFrame>,
    /// Its triangles: `indices[3 * first_triangle ..][.. 3 * triangle_count]`.
    pub first_triangle: usize,
    pub triangle_count: usize,
    /// Its boundary loops (closed polylines, first point not repeated), for hover outlines.
    pub loops: Vec<Vec<Vec3>>,
    /// Its exact area centroid, from the kernel (P3.8: a face's implicit mate connector).
    /// `None` for prism meshes.
    pub center: Option<Vec3>,
    /// The axis of a cylindrical, conical or other face of revolution (a point on it and its
    /// unit direction), from the kernel (P3.8: a mate connector or pattern axis on it).
    pub axis: Option<(Vec3, Vec3)>,
    /// Its exact area (mm²), from the kernel (P3.10, X7: the Mass properties Face tab). `None`
    /// for prism meshes.
    pub area: Option<f64>,
}

/// An edge of a [`Solid`]: a polyline (one or more kernel edges that continue each other
/// between the same two faces).
#[derive(Debug, Clone, PartialEq)]
pub struct SolidEdge {
    pub name: EdgeName,
    pub points: Vec<Vec3>,
    /// The exact circle it lies on, from the kernel (P3.4: Use projects the exact curve, and a
    /// circular edge can be a revolve axis). `None` for other curves and for prism meshes.
    pub circle: Option<EdgeCircle>,
    /// Which tangent-connected chain of the body it belongs to (P3.7, X12: Create selection
    /// → Tangent connected): edges with the same group meet end to end with parallel tangents
    /// (the kernel's exact tangents). `None` for prism meshes.
    pub tangent_group: Option<u32>,
}

/// A circle in space: its center, the unit normal of its plane and its radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeCircle {
    pub center: Vec3,
    pub normal: Vec3,
    pub radius: f64,
}

impl SolidEdge {
    /// The point halfway along it.
    pub fn midpoint(&self) -> Vec3 {
        let total: f64 = self.points.windows(2).map(|w| len(sub(w[1], w[0]))).sum();
        let mut left = total / 2.0;
        for w in self.points.windows(2) {
            let l = len(sub(w[1], w[0]));
            if l >= left && l > 0.0 {
                return add(w[0], scale(sub(w[1], w[0]), left / l));
            }
            left -= l;
        }
        self.points.first().copied().unwrap_or([0.0; 3])
    }

    /// The distance from `p` to the polyline.
    pub fn distance(&self, p: Vec3) -> f64 {
        match self.points.as_slice() {
            [] => f64::INFINITY,
            [q] => len(sub(p, *q)),
            pts => pts
                .windows(2)
                .map(|w| {
                    let d = sub(w[1], w[0]);
                    let t = (dot(sub(p, w[0]), d) / dot(d, d).max(1e-300)).clamp(0.0, 1.0);
                    len(sub(p, add(w[0], scale(d, t))))
                })
                .fold(f64::INFINITY, f64::min),
        }
    }
}

/// How two faces meet along an edge ([`Solid::dihedral`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dihedral {
    /// Material on the inside of the angle (a box's edge).
    Convex,
    /// Material round the outside (a pocket's floor meeting its wall).
    Concave,
    /// Tangent faces (a fillet running into a wall).
    Smooth,
}

/// A vertex of a [`Solid`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidVertex {
    pub name: VertexName,
    pub point: Vec3,
}

/// A line along a curved side face (from the start cap to the end cap) with the face's outward
/// normal there: where the normal turns from facing the viewer to facing away is a silhouette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ruling {
    pub start: Vec3,
    pub end: Vec3,
    pub normal: Vec3,
    /// The index of the curved face in [`Solid::faces`].
    pub face: usize,
    /// Which run of rulings it belongs to: a face swept by several separate pieces of a curve
    /// has one run per piece, and silhouettes are only looked for between neighbours in a run
    /// (not across the gap between two pieces).
    pub run: usize,
}

/// A triangulated solid with face and edge identity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Solid {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<u32>,
    pub faces: Vec<SolidFace>,
    pub edges: Vec<SolidEdge>,
    pub vertices: Vec<SolidVertex>,
    pub rulings: Vec<Ruling>,
    /// Doubly curved faces (a revolve's torus or sphere) as grids, for their silhouettes.
    pub grids: Vec<SurfaceGrid>,
    /// The Part Studio's explicit mate connectors this part owns (P3B.7, A22.2): they travel
    /// with the part into every assembly instance of it (the rebuild attaches them).
    pub connectors: Vec<SolidConnector>,
    /// Faces with an appearance the rebuild gives them (P3H.6: a Transform's copies of context
    /// parts keep their Part Studio's colours, and a composite part its members'). A part's own
    /// and its faces' appearances still win (`crate::appearance::face_appearance`).
    pub looks: Vec<(FaceName, crate::appearance::Appearance)>,
    /// Other names of merged faces ([`cadrs_kernel::naming::BodyNames::aliases`]): the kernel
    /// merges neighbouring faces on one surface into one face, named after the smallest of its
    /// pieces' names; references to the other names find it through these.
    pub face_aliases: Vec<FaceAlias>,
    /// Bounding boxes for picking, built on the first pick ([`Solid::pick_index`]).
    pub pick_cache: PickCache,
}

/// An axis-aligned box: its lowest and highest corners.
pub type Bounds = (Vec3, Vec3);

/// A [`Solid`]'s boxes for picking: its own, and each face's and edge's (by index). A pick
/// tests a box before the triangles or segments in it, so a ray through a big part costs about
/// the faces it passes near, not all of its triangles.
#[derive(Debug, Clone, PartialEq)]
pub struct PickIndex {
    pub bounds: Option<Bounds>,
    pub faces: Vec<Option<Bounds>>,
    pub edges: Vec<Option<Bounds>>,
    /// The solid's counts when this was built, to catch geometry changed afterwards.
    shape: [usize; 4],
}

/// The [`PickIndex`] of a [`Solid`], built once. A copy starts empty (it may be moved or
/// otherwise changed, as [`crate::assembly::transform_solid`] does); the solid's geometry is not
/// changed in place once it has been picked (only its looks and connectors are), and an index
/// whose counts no longer match is rebuilt for each pick instead.
#[derive(Default)]
pub struct PickCache(std::sync::OnceLock<PickIndex>);

impl Clone for PickCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for PickCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for PickCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PickCache")
    }
}

fn bounds_of(points: impl IntoIterator<Item = Vec3>) -> Option<Bounds> {
    let mut it = points.into_iter();
    let first = it.next()?;
    let (mut lo, mut hi) = (first, first);
    for p in it {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    // A hair larger, so a ray grazing a flat face's box (zero thick) still reaches its
    // triangles.
    let pad = 1e-6 * (0..3).map(|i| hi[i] - lo[i]).fold(1.0, f64::max);
    Some((lo.map(|v| v - pad), hi.map(|v| v + pad)))
}

/// True if the line `origin + t·dir` (any `t`, as [`Solid::pick`] takes it) passes through the
/// box.
pub fn line_hits_box(origin: Vec3, dir: Vec3, (lo, hi): &Bounds) -> bool {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for i in 0..3 {
        if dir[i].abs() < 1e-15 {
            if origin[i] < lo[i] || origin[i] > hi[i] {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo[i] - origin[i]) / dir[i], (hi[i] - origin[i]) / dir[i]);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// Another name of a face of a [`Solid`] (a face merged into it, see [`Solid::face_aliases`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceAlias {
    /// The merged-away name.
    pub name: FaceName,
    /// The name of the face it is part of now.
    pub face: FaceName,
    /// The sketch frame the face has under this name (planar faces): the frame a face with
    /// this name had before it was merged, so a sketch on it stays where it was.
    pub plane: Option<PlaneFrame>,
}

/// An explicit mate connector carried by a part ([`Solid::connectors`]): the Mate connector
/// feature that made it and its frame (the part's coordinates; X `u`, Y `v`, Z `u × v`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidConnector {
    pub feature: crate::ids::FeatureId,
    pub frame: PlaneFrame,
}

/// A doubly curved face sampled as a grid (P3.4): rows of points on the surface (a revolve's
/// meridians, one row per angle) with the outward normal at each. A silhouette runs where the
/// normal turns from facing the viewer to facing away, found cell by cell ([`Self::silhouette`]),
/// since it may cross the rows in any direction (a torus seen from above runs round it).
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceGrid {
    /// The index of the face in [`Solid::faces`].
    pub face: usize,
    /// Each row's points and normals (every row as long as the first).
    pub rows: Vec<Vec<(Vec3, Vec3)>>,
}

impl SurfaceGrid {
    /// The silhouette seen along `back` (the unit direction to the viewer) as short segments,
    /// with the normal at each end.
    pub fn silhouette(&self, back: Vec3) -> Vec<[(Vec3, Vec3); 2]> {
        let f = |p: &(Vec3, Vec3)| dot(p.1, back);
        let lerp = |a: &(Vec3, Vec3), b: &(Vec3, Vec3)| -> (Vec3, Vec3) {
            let (fa, fb) = (f(a), f(b));
            let t = if (fa - fb).abs() < 1e-300 { 0.5 } else { fa / (fa - fb) };
            let mix = |x: Vec3, y: Vec3| add(x, scale(sub(y, x), t));
            (mix(a.0, b.0), normalize(mix(a.1, b.1)))
        };
        let mut out = Vec::new();
        for w in self.rows.windows(2) {
            let (r0, r1) = (&w[0], &w[1]);
            let n = r0.len().min(r1.len());
            for j in 0..n.saturating_sub(1) {
                // The cell's corners, round it.
                let c = [&r0[j], &r0[j + 1], &r1[j + 1], &r1[j]];
                let mut hits = Vec::new();
                for k in 0..4 {
                    let (a, b) = (c[k], c[(k + 1) % 4]);
                    if (f(a) >= 0.0) != (f(b) >= 0.0) {
                        hits.push(lerp(a, b));
                    }
                }
                if hits.len() == 2 {
                    out.push([hits[0], hits[1]]);
                } else if hits.len() == 4 {
                    out.push([hits[0], hits[1]]);
                    out.push([hits[2], hits[3]]);
                }
            }
        }
        out
    }
}

/// The point of triangle `abc` nearest `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add(a, scale(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, scale(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return add(b, scale(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let denom = 1.0 / (va + vb + vc);
    add(a, add(scale(ab, vb * denom), scale(ac, vc * denom)))
}

// Small vector helpers (world coordinates are `[f64; 3]`).
pub(crate) fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn scale(a: Vec3, k: f64) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub(crate) fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn len(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}
pub(crate) fn normalize(a: Vec3) -> Vec3 {
    let l = dot(a, a).sqrt();
    if l < 1e-15 { a } else { scale(a, 1.0 / l) }
}

impl Solid {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The face with this name (or with this name among its other names, a merged face's).
    pub fn face(&self, name: &FaceName) -> Option<&SolidFace> {
        let name = self.canonical_face(name);
        self.faces.iter().find(|f| f.name == name)
    }

    /// The edge with this name (its faces by any of their names).
    pub fn edge(&self, name: &EdgeName) -> Option<&SolidEdge> {
        let name = self.canonical_edge(name);
        self.edges.iter().find(|e| e.name == name)
    }

    /// The vertex with this name (its faces by any of their names).
    pub fn vertex(&self, name: &VertexName) -> Option<&SolidVertex> {
        let name = self.canonical_vertex(name);
        self.vertices.iter().find(|v| v.name == name)
    }

    /// The name a face is known by now: the face a merged-away name was merged into
    /// ([`Solid::face_aliases`]), else the name itself.
    pub fn canonical_face(&self, name: &FaceName) -> FaceName {
        if self.face_aliases.is_empty() || self.faces.iter().any(|f| f.name == *name) {
            return *name;
        }
        self.face_aliases.iter().find(|a| a.name == *name).map_or(*name, |a| a.face)
    }

    /// An edge name with its faces by the names they are known by now.
    pub fn canonical_edge(&self, name: &EdgeName) -> EdgeName {
        if self.face_aliases.is_empty() {
            return *name;
        }
        EdgeName::new(self.canonical_face(&name.faces[0]), self.canonical_face(&name.faces[1]), name.index)
    }

    /// A vertex name with its faces by the names they are known by now.
    pub fn canonical_vertex(&self, name: &VertexName) -> VertexName {
        if self.face_aliases.is_empty() {
            return *name;
        }
        let mut faces = name.faces.map(|f| self.canonical_face(&f));
        faces.sort();
        VertexName { faces, index: name.index }
    }

    /// The sketch frame of face `i` when it is referred to as `name`: the frame its merged-away
    /// piece of that name had ([`FaceAlias::plane`]), else the face's own.
    pub fn face_plane_as(&self, i: usize, name: &FaceName) -> Option<PlaneFrame> {
        let face = self.faces.get(i)?;
        if face.name != *name
            && let Some(a) = self.face_aliases.iter().find(|a| a.name == *name && a.face == face.name)
            && a.plane.is_some()
        {
            return a.plane;
        }
        face.plane
    }

    /// The face a stored name refers to now (see [`naming::resolve`]): by name, else (a face
    /// since split) the piece containing `seed`, else, as the geometric fallback, the planar
    /// face in `plane` (facing the same way) that contains `seed`, a point that was on the face.
    /// `Err(Lost)` if there is none.
    pub fn resolve_face(
        &self,
        name: &FaceName,
        plane: Option<&PlaneFrame>,
        seed: Option<Vec3>,
    ) -> Result<(usize, Match), Lost> {
        let names: Vec<FaceName> = self.faces.iter().map(|f| f.name).collect();
        let name = &self.canonical_face(name);
        let distance = |i: usize| -> Option<f64> {
            let seed = seed?;
            if let (Some(h), Some(p)) = (plane, self.faces[i].plane) {
                let (a, b) = (normalize(h.normal()), normalize(p.normal()));
                if dot(a, b) < 1.0 - 1e-9 || h.distance(p.origin).abs() / len(h.normal()).max(1e-300) > 1e-6 {
                    return None;
                }
            }
            self.face_contains(i, seed).then_some(0.0)
        };
        naming::resolve(name, &names, distance, 1e-6)
    }

    /// True if the point lies on one of the face's triangles (within 1e-6 mm).
    pub fn face_contains(&self, face: usize, p: Vec3) -> bool {
        let f = &self.faces[face];
        (f.first_triangle..f.first_triangle + f.triangle_count).any(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| self.positions[self.indices[3 * t + k] as usize]);
            let n = cross(sub(b, a), sub(c, a));
            let nn = dot(n, n);
            if nn < 1e-24 {
                return false;
            }
            // Off the triangle's plane?
            if dot(sub(p, a), n).abs() / nn.sqrt() > 1e-6 {
                return false;
            }
            // Barycentric: inside all three edges.
            [(a, b), (b, c), (c, a)]
                .iter()
                .all(|(u, v)| dot(cross(sub(*v, *u), sub(p, *u)), n) >= -1e-9 * nn)
        })
    }

    /// The face's outward normal at its largest triangle (out of the material).
    pub fn face_normal(&self, face: usize) -> Option<Vec3> {
        let f = self.faces.get(face)?;
        (f.first_triangle..f.first_triangle + f.triangle_count)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| self.positions[self.indices[3 * t + k] as usize]);
                cross(sub(b, a), sub(c, a))
            })
            .max_by(|x, y| dot(*x, *x).total_cmp(&dot(*y, *y)))
            .map(normalize)
    }

    /// A point on the face (the middle of its largest triangle).
    pub fn face_point(&self, face: usize) -> Option<Vec3> {
        let f = self.faces.get(face)?;
        (f.first_triangle..f.first_triangle + f.triangle_count)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| self.positions[self.indices[3 * t + k] as usize]);
                let n = cross(sub(b, a), sub(c, a));
                (dot(n, n), scale(add(add(a, b), c), 1.0 / 3.0))
            })
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, p)| p)
    }

    /// The edge a stored name refers to now: by name, else (re-indexed) the one `distance`
    /// finds nearest where the reference was, else one `distance` puts within 1e-6 mm of it.
    /// `distance` returns `None` where it can't tell.
    pub fn resolve_edge(
        &self,
        name: &EdgeName,
        distance: impl Fn(&SolidEdge) -> Option<f64>,
    ) -> Result<(usize, Match), Lost> {
        let names: Vec<EdgeName> = self.edges.iter().map(|e| e.name).collect();
        naming::resolve(&self.canonical_edge(name), &names, |i| distance(&self.edges[i]), 1e-6)
    }

    /// How the two faces along an edge meet (P3.8, for Create selection → Pocket), from the mesh
    /// next to the edge's middle: each face's triangle nearest it gives the face's outward normal
    /// there and the way into the face. Faces whose normals are within 8° are `Smooth`;
    /// otherwise the edge is `Concave` when the way into one face runs along the other's outward
    /// normal (a pocket's floor meeting its wall), else `Convex`.
    pub fn dihedral(&self, e: &SolidEdge) -> Option<Dihedral> {
        let n = e.points.len();
        if n < 2 {
            return None;
        }
        let k = (n - 1) / 2;
        let (a, b) = (e.points[k], e.points[k + 1]);
        let m = scale(add(a, b), 0.5);
        let t = normalize(sub(b, a));
        let near = |name: &FaceName| -> Option<(Vec3, Vec3)> {
            let f = self.faces.iter().find(|f| f.name == *name)?;
            // The triangle the edge's middle is on (nearest by distance to the triangle, not
            // its centre: a face round a pocket has triangles across the pocket too).
            (f.first_triangle..f.first_triangle + f.triangle_count)
                .map(|tri| {
                    let [p, q, r] = [0, 1, 2].map(|j| self.positions[self.indices[3 * tri + j] as usize]);
                    let c = scale(add(add(p, q), r), 1.0 / 3.0);
                    let d = sub(closest_on_triangle(m, p, q, r), m);
                    (dot(d, d), normalize(cross(sub(q, p), sub(r, p))), c)
                })
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, nrm, c)| (nrm, c))
        };
        let (na, ca) = near(&e.name.faces[0])?;
        let (nb, _) = near(&e.name.faces[1])?;
        if dot(na, nb) > 8f64.to_radians().cos() {
            return Some(Dihedral::Smooth);
        }
        let into_a = sub(ca, m);
        let into_a = normalize(sub(into_a, scale(t, dot(into_a, t))));
        let s = dot(into_a, nb);
        Some(if s.abs() < 0.15 {
            Dihedral::Smooth
        } else if s > 0.0 {
            Dihedral::Concave
        } else {
            Dihedral::Convex
        })
    }

    /// The faces of the pocket (or boss's recess) `seed` belongs to (P3.8, Create selection →
    /// Faces → Pocket, PS27.12): `seed` and every face reached from it across concave or smooth
    /// edges; convex edges (the pocket's rim) stop it.
    pub fn pocket_faces(&self, seed: usize) -> Vec<usize> {
        let Some(first) = self.faces.get(seed).map(|f| f.name) else { return Vec::new() };
        let mut names = vec![first];
        let mut todo = vec![first];
        while let Some(cur) = todo.pop() {
            for e in self.edges.iter().filter(|e| e.name.touches(&cur)) {
                let other = if e.name.faces[0] == cur { e.name.faces[1] } else { e.name.faces[0] };
                if other == cur || names.contains(&other) {
                    continue;
                }
                if matches!(self.dihedral(e), Some(Dihedral::Concave | Dihedral::Smooth)) {
                    names.push(other);
                    todo.push(other);
                }
            }
        }
        names.iter().filter_map(|n| self.faces.iter().position(|f| f.name == *n)).collect()
    }

    /// The edges of a face.
    pub fn face_edges(&self, face: &FaceName) -> Vec<EdgeName> {
        let face = &self.canonical_face(face);
        let mut out: Vec<EdgeName> = Vec::new();
        for e in self.edges.iter().filter(|e| e.name.touches(face)) {
            if !out.contains(&e.name) {
                out.push(e.name);
            }
        }
        out
    }

    /// The index of the face a triangle belongs to.
    pub fn face_of_triangle(&self, tri: usize) -> Option<usize> {
        self.faces
            .iter()
            .position(|f| tri >= f.first_triangle && tri < f.first_triangle + f.triangle_count)
    }

    /// The nearest face the ray `origin + t·dir` hits (with `t`), if any.
    pub fn pick(&self, origin: Vec3, dir: Vec3) -> Option<(usize, f64)> {
        self.pick_where(origin, dir, |_| true)
    }

    /// The solid's [`PickIndex`], built on first use.
    pub fn pick_index(&self) -> std::borrow::Cow<'_, PickIndex> {
        let shape = [self.positions.len(), self.indices.len(), self.faces.len(), self.edges.len()];
        let index = self.pick_cache.0.get_or_init(|| self.build_pick_index());
        if index.shape == shape {
            std::borrow::Cow::Borrowed(index)
        } else {
            std::borrow::Cow::Owned(self.build_pick_index())
        }
    }

    fn build_pick_index(&self) -> PickIndex {
        let face = |f: &SolidFace| {
            let idx = self.indices.get(3 * f.first_triangle..3 * (f.first_triangle + f.triangle_count)).unwrap_or(&[]);
            bounds_of(idx.iter().filter_map(|&i| self.positions.get(i as usize).copied()))
        };
        PickIndex {
            bounds: bounds_of(self.positions.iter().copied()),
            faces: self.faces.iter().map(face).collect(),
            edges: self.edges.iter().map(|e| bounds_of(e.points.iter().copied())).collect(),
            shape: [self.positions.len(), self.indices.len(), self.faces.len(), self.edges.len()],
        }
    }

    /// [`Solid::pick`] among the faces `keep` accepts (the others let the ray through).
    pub fn pick_where(&self, origin: Vec3, dir: Vec3, keep: impl Fn(&SolidFace) -> bool) -> Option<(usize, f64)> {
        let index = self.pick_index();
        if !index.bounds.as_ref().is_some_and(|b| line_hits_box(origin, dir, b)) {
            return None;
        }
        let mut best: Option<(usize, f64)> = None;
        for (fi, f) in self.faces.iter().enumerate() {
            if !index.faces[fi].as_ref().is_some_and(|b| line_hits_box(origin, dir, b)) || !keep(f) {
                continue;
            }
            for tri in f.first_triangle..f.first_triangle + f.triangle_count {
                let [a, b, c] = [0, 1, 2].map(|k| self.positions[self.indices[3 * tri + k] as usize]);
                let Some(t) = ray_triangle(origin, dir, a, b, c) else {
                    continue;
                };
                if best.is_none_or(|(_, bt)| t < bt) {
                    best = Some((fi, t));
                }
            }
        }
        best
    }

    /// The corners of the axis-aligned box around the solid (for zoom to fit).
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let first = *self.positions.first()?;
        let (mut lo, mut hi) = (first, first);
        for p in &self.positions {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        Some((lo, hi))
    }

    /// The enclosed volume (mm³), from the divergence theorem (for tests and sanity checks).
    pub fn volume(&self) -> f64 {
        (0..self.triangle_count())
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| self.positions[self.indices[3 * t + k] as usize]);
                dot(a, cross(b, c)) / 6.0
            })
            .sum()
    }
}

/// Möller–Trumbore: the ray parameter where it hits the triangle (either side).
/// Vector helpers for the rest of the crate.
pub(crate) fn sub3(a: Vec3, b: Vec3) -> Vec3 {
    sub(a, b)
}
pub(crate) fn dot3(a: Vec3, b: Vec3) -> f64 {
    dot(a, b)
}
pub(crate) fn cross3(a: Vec3, b: Vec3) -> Vec3 {
    cross(a, b)
}
pub(crate) fn dist3(a: Vec3, b: Vec3) -> f64 {
    len(sub(a, b))
}

fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, a);
    let u = dot(s, p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    Some(dot(e2, q) * inv)
}

/// One boundary ring of a region: its points and the curve of each segment.
struct Ring<'a> {
    pts: &'a [Vec2],
    curves: &'a [CurveId],
}

/// Extrudes closed regions of a sketch on `frame` by `depth` mm along the plane's normal
/// (`flip`: the other way). `regions` pairs each region with the index it is known by in the
/// feature (it goes into the face tags). Adjacent regions extruded together share their
/// common curve; its side faces and edges are left out (the regions merge into one body).
pub fn extrude(op: OpId, frame: &PlaneFrame, regions: &[(u64, Region)], depth: f64, flip: bool) -> Solid {
    let n = normalize(frame.normal());
    let dir = if flip { scale(n, -1.0) } else { n };
    let offset = scale(dir, depth);
    let w = |p: Vec2| frame.to_world(p);
    let mut out = Solid::default();
    // Curves that bound two of the regions (their side is inside the merged body).
    let mut uses: std::collections::HashMap<CurveId, usize> = std::collections::HashMap::new();
    for (_, r) in regions {
        let mut seen: Vec<CurveId> = r.outer_curves.clone();
        for h in &r.hole_curves {
            seen.extend(h);
        }
        seen.sort();
        seen.dedup();
        for c in seen {
            *uses.entry(c).or_default() += 1;
        }
    }
    let shared = |c: &CurveId| uses.get(c).copied().unwrap_or(0) > 1;

    for (index, r) in regions {
        let index = *index;
        if r.outer.len() < 3 {
            continue;
        }
        // Caps.
        let (verts, tris) = r.triangulate();
        for end in [true, false] {
            let name = cap_name(op, index, end);
            let outward = if end { dir } else { scale(dir, -1.0) };
            let shift = if end { offset } else { [0.0; 3] };
            let base = out.positions.len() as u32;
            for p in &verts {
                out.positions.push(add(w(*p), shift));
                out.normals.push(outward);
            }
            let first_triangle = out.triangle_count();
            for t in tris.chunks(3) {
                let [i, j, k] = [t[0], t[1], t[2]];
                let (a, b, c) = (verts[i as usize], verts[j as usize], verts[k as usize]);
                let area2 = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
                // Counter-clockwise in the sketch means facing +n.
                let faces_n = area2 > 0.0;
                let want_n = dot(outward, n) > 0.0;
                if faces_n == want_n {
                    out.indices.extend([base + i, base + j, base + k]);
                } else {
                    out.indices.extend([base + i, base + k, base + j]);
                }
            }
            let triangle_count = out.triangle_count() - first_triangle;
            // The cap's frame: the sketch frame moved along, turned to face outward.
            let origin = add(frame.origin, shift);
            let plane = if dot(outward, n) > 0.0 {
                PlaneFrame { origin, u: frame.u, v: frame.v }
            } else {
                PlaneFrame { origin, u: frame.u, v: scale(frame.v, -1.0) }
            };
            let mut loops = vec![r.outer.iter().map(|p| add(w(*p), shift)).collect::<Vec<_>>()];
            for h in &r.holes {
                loops.push(h.iter().map(|p| add(w(*p), shift)).collect());
            }
            out.faces.push(SolidFace {
                name,
                plane: Some(plane),
                first_triangle,
                triangle_count,
                loops,
                center: None,
                axis: None,
                area: None,
            });
        }

        // Side faces and edges, ring by ring.
        let mut rings = vec![Ring {
            pts: &r.outer,
            curves: &r.outer_curves,
        }];
        for (h, c) in r.holes.iter().zip(&r.hole_curves) {
            rings.push(Ring { pts: h, curves: c });
        }
        for ring in rings {
            side_faces(&mut out, op, index, &ring, frame, n, dir, offset, &shared);
        }
    }
    out
}

/// The name of a cap of region `region` (its key) of the extrude `op`.
pub fn cap_name(op: OpId, region: u64, end: bool) -> FaceName {
    FaceName::new(op, FaceOrigin::Cap { region, end })
}

/// The name of the side face of sketch curve `curve` of region `region`.
pub fn side_name(op: OpId, region: u64, curve: CurveId) -> FaceName {
    FaceName::new(
        op,
        FaceOrigin::Side {
            region,
            curve: curve.data().as_ffi(),
        },
    )
}

/// The side faces of one boundary ring: consecutive segments on the same curve form one face.
#[allow(clippy::too_many_arguments)]
fn side_faces(
    out: &mut Solid,
    op: OpId,
    region: u64,
    ring: &Ring,
    frame: &PlaneFrame,
    n: Vec3,
    dir: Vec3,
    offset: Vec3,
    shared: &dyn Fn(&CurveId) -> bool,
) {
    let len = ring.pts.len();
    if len < 2 || ring.curves.len() != len {
        return;
    }
    let w = |i: usize| frame.to_world(ring.pts[i % len]);
    // Split the ring into runs of one curve, starting where the curve changes (a ring made
    // of one curve, a circle, is one run).
    let start = (0..len)
        .find(|&i| ring.curves[i] != ring.curves[(i + len - 1) % len])
        .unwrap_or(0);
    let mut runs: Vec<(CurveId, Vec<usize>)> = Vec::new();
    for k in 0..len {
        let i = (start + k) % len;
        match runs.last_mut() {
            Some((c, segs)) if *c == ring.curves[i] => segs.push(i),
            _ => runs.push((ring.curves[i], vec![i])),
        }
    }
    let closed = runs.len() == 1;
    let outward_of = |i: usize| normalize(cross(sub(w(i + 1), w(i)), n));
    for (run_index, (curve, segs)) in runs.iter().enumerate() {
        let curve = *curve;
        // Lateral edge where the previous curve ends and this one begins.
        if !closed {
            let prev = runs[(run_index + runs.len() - 1) % runs.len()].0;
            if !shared(&prev) || !shared(&curve) {
                let p = w(segs[0]);
                out.edges.push(SolidEdge {
                    name: EdgeName::new(
                        side_name(op, region, prev),
                        side_name(op, region, curve),
                        0,
                    ),
                    points: vec![p, add(p, offset)],
                    circle: None,
                    tangent_group: None,
                });
            }
        }
        if shared(&curve) {
            continue;
        }
        // The run's points along the curve (the closing point repeated for a closed ring).
        let mut idx: Vec<usize> = segs.clone();
        idx.push(segs[segs.len() - 1] + 1);
        let pts: Vec<Vec3> = idx.iter().map(|&i| w(i)).collect();
        let seg_normals: Vec<Vec3> = segs.iter().map(|&i| outward_of(i)).collect();
        let straight = segs.len() == 1;
        // Vertex normals: flat on a line, averaged along a curve.
        let vertex_normal = |k: usize| -> Vec3 {
            if straight {
                return seg_normals[0];
            }
            let before = if k > 0 {
                Some(seg_normals[k - 1])
            } else if closed {
                seg_normals.last().copied()
            } else {
                None
            };
            let after = if k < seg_normals.len() {
                Some(seg_normals[k])
            } else if closed {
                seg_normals.first().copied()
            } else {
                None
            };
            match (before, after) {
                (Some(a), Some(b)) => normalize(add(a, b)),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => n,
            }
        };
        let face_index = out.faces.len();
        let base = out.positions.len() as u32;
        for (k, p) in pts.iter().enumerate() {
            let nv = vertex_normal(k);
            out.positions.push(*p);
            out.normals.push(nv);
            out.positions.push(add(*p, offset));
            out.normals.push(nv);
            if !straight {
                out.rulings.push(Ruling {
                    start: *p,
                    end: add(*p, offset),
                    normal: nv,
                    face: face_index,
                    run: face_index,
                });
            }
        }
        let first_triangle = out.triangle_count();
        // Along +dir the quad (b0, b1, t1, t0) faces outward when dir is the plane normal;
        // the other way round it must be reversed.
        let forward = dot(dir, n) > 0.0;
        for k in 0..segs.len() {
            let (b0, t0, b1, t1) = (
                base + 2 * k as u32,
                base + 2 * k as u32 + 1,
                base + 2 * k as u32 + 2,
                base + 2 * k as u32 + 3,
            );
            if forward {
                out.indices.extend([b0, b1, t1, b0, t1, t0]);
            } else {
                out.indices.extend([b0, t1, b1, b0, t0, t1]);
            }
        }
        let triangle_count = out.triangle_count() - first_triangle;
        let plane = straight.then(|| {
            let t = normalize(sub(pts[1], pts[0]));
            // u × v must be the outward normal t × n.
            let (u, v) = if forward { (t, dir) } else { (scale(t, -1.0), dir) };
            let outward = cross(u, v);
            // The projection of the world origin onto the face.
            let origin = scale(outward, dot(pts[0], outward));
            PlaneFrame { origin, u, v }
        });
        let mut face_loop: Vec<Vec3> = pts.clone();
        face_loop.extend(pts.iter().rev().map(|p| add(*p, offset)));
        out.faces.push(SolidFace {
            name: side_name(op, region, curve),
            plane,
            first_triangle,
            triangle_count,
            loops: vec![face_loop],
            center: None,
            axis: None,
            area: None,
        });
        for end in [false, true] {
            let shift = if end { offset } else { [0.0; 3] };
            out.edges.push(SolidEdge {
                name: EdgeName::new(side_name(op, region, curve), cap_name(op, region, end), 0),
                points: pts.iter().map(|p| add(*p, shift)).collect(),
                circle: None,
                tangent_group: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_sketch::region::regions;
    use cadrs_sketch::{PlaneRef, Sketch, SketchOp};

    fn rect(s: &mut Sketch, x: f64, y: f64, w: f64, h: f64) {
        SketchOp::AddPolyline {
            points: vec![
                Vec2::new(x, y),
                Vec2::new(x + w, y),
                Vec2::new(x + w, y + h),
                Vec2::new(x, y + h),
            ],
            closed: true,
            construction: false,
            label: "Add rectangle",
        }
        .apply(s)
        .unwrap();
    }

    fn circle(s: &mut Sketch, x: f64, y: f64, r: f64) {
        SketchOp::AddCircle {
            center: Vec2::new(x, y),
            radius: r,
            construction: false,
        }
        .apply(s)
        .unwrap();
    }

    const OP: OpId = uuid::Uuid::from_u128(1);

    fn solid_of(s: &Sketch, plane: PlaneRef, depth: f64, flip: bool) -> Solid {
        let rs: Vec<(u64, Region)> = regions(s)
            .into_iter()
            .enumerate()
            .map(|(i, r)| (i as u64, r))
            .collect();
        extrude(OP, &plane.frame(), &rs, depth, flip)
    }

    fn face(t: cadrs_sketch::FaceTag) -> FaceName {
        t.to_name(OP)
    }

    use cadrs_sketch::FaceTag;

    fn is_side(f: &SolidFace) -> bool {
        matches!(f.name.origin, FaceOrigin::Side { .. })
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    /// The 50 × 30 × 25 box of the reference walkthrough.
    #[test]
    fn rectangle_prism_counts_and_volume() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 50.0, 30.0);
        let solid = solid_of(&s, PlaneRef::Top, 25.0, false);
        // 2 caps (2 triangles each) + 4 sides (2 each).
        assert_eq!(solid.faces.len(), 6);
        assert_eq!(solid.triangle_count(), 12);
        // Caps: 4 vertices each; sides: 4 each (flat shading, no sharing).
        assert_eq!(solid.positions.len(), 24);
        // 4 edges at each cap + 4 lateral edges.
        assert_eq!(solid.edges.len(), 12);
        assert!((solid.volume() - 50.0 * 30.0 * 25.0).abs() < 1e-6, "{}", solid.volume());
        let (lo, hi) = solid.bounds().unwrap();
        assert!(close(lo, [0.0, 0.0, 0.0]) && close(hi, [50.0, 30.0, 25.0]));
        assert!(solid.rulings.is_empty());
    }

    #[test]
    fn faces_keep_their_identity_and_frames() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 50.0, 30.0);
        let solid = solid_of(&s, PlaneRef::Top, 25.0, false);
        let top = solid.face(&face(FaceTag::End { region: 0 })).unwrap();
        let f = top.plane.unwrap();
        assert!(close(f.normal(), [0.0, 0.0, 1.0]));
        assert!(close(f.origin, [0.0, 0.0, 25.0]));
        // Sketch coordinates on the top face line up with the Top plane's.
        assert!(close(f.to_world(Vec2::new(10.0, 5.0)), [10.0, 5.0, 25.0]));
        let bottom = solid.face(&face(FaceTag::Start { region: 0 })).unwrap().plane.unwrap();
        assert!(close(bottom.normal(), [0.0, 0.0, -1.0]));
        // Four planar side faces, one per line, facing out.
        let sides: Vec<&SolidFace> = solid
            .faces
            .iter()
            .filter(|f| is_side(f))
            .collect();
        assert_eq!(sides.len(), 4);
        let mut normals: Vec<Vec3> = sides.iter().map(|f| f.plane.unwrap().normal()).collect();
        normals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let want = [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
        for (a, b) in normals.iter().zip(want) {
            assert!(close(*a, b), "{normals:?}");
        }
        // Each side's curve is a different line of the sketch.
        let mut curves: Vec<u64> = sides
            .iter()
            .map(|f| match f.name.origin {
                FaceOrigin::Side { curve, .. } => curve,
                _ => unreachable!(),
            })
            .collect();
        curves.sort();
        curves.dedup();
        assert_eq!(curves.len(), 4);
        // The right side (x = 50) lies at x = 50 and its frame spans the face.
        let right = sides
            .iter()
            .find(|f| close(f.plane.unwrap().normal(), [1.0, 0.0, 0.0]))
            .unwrap()
            .plane
            .unwrap();
        assert!(right.distance([50.0, 7.0, 3.0]).abs() < 1e-9);
        assert!(close(right.v, [0.0, 0.0, 1.0]));
    }

    #[test]
    fn normals_point_outward_and_triangles_wind_outward() {
        for flip in [false, true] {
            for plane in PlaneRef::ALL {
                let mut s = Sketch::new();
                rect(&mut s, 5.0, -3.0, 20.0, 10.0);
                circle(&mut s, 12.0, 2.0, 2.0);
                // Only the rectangle's region (with the circle as its hole).
                let r = regions(&s).into_iter().find(|r| r.curves.len() == 4).unwrap();
                let solid = extrude(OP, &plane.frame(), &[(0, r)], 8.0, flip);
                for face in &solid.faces {
                    for t in face.first_triangle..face.first_triangle + face.triangle_count {
                        let [a, b, c] =
                            [0, 1, 2].map(|k| solid.positions[solid.indices[3 * t + k] as usize]);
                        let wind = cross(sub(b, a), sub(c, a));
                        let na = solid.normals[solid.indices[3 * t] as usize];
                        assert!(dot(wind, na) > 0.0, "{:?} winds against its normal", face.name);
                        if let Some(p) = face.plane {
                            assert!(dot(p.normal(), na) > 0.99, "{:?}", face.name);
                        }
                    }
                }
                // Volume: the box minus the hole (tessellated circle, so approximately).
                let want = 20.0 * 10.0 * 8.0 - std::f64::consts::PI * 4.0 * 8.0;
                assert!(
                    (solid.volume().abs() - want).abs() / want < 0.01,
                    "{plane:?} {flip}: {}",
                    solid.volume()
                );
                // Outward winding everywhere gives a positive volume.
                assert!(solid.volume() > 0.0, "{plane:?} flip {flip}");
            }
        }
    }

    #[test]
    fn holes_become_inner_side_faces() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 40.0, 40.0);
        circle(&mut s, 20.0, 20.0, 5.0);
        let rs = regions(&s);
        let square = rs.iter().position(|r| r.curves.len() == 4).unwrap();
        let solid = extrude(
            OP,
            &PlaneRef::Top.frame(),
            &[(0, rs[square].clone())],
            10.0,
            false,
        );
        // 2 caps + 4 outer sides + 1 cylindrical hole face.
        assert_eq!(solid.faces.len(), 7);
        let hole = solid
            .faces
            .iter()
            .find(|f| is_side(f) && f.plane.is_none())
            .unwrap();
        // The cap has two loops: the outline and the hole.
        assert_eq!(solid.face(&face(FaceTag::End { region: 0 })).unwrap().loops.len(), 2);
        // The hole's normals point toward its axis (out of the material).
        let tri = hole.first_triangle;
        let p = solid.positions[solid.indices[3 * tri] as usize];
        let nrm = solid.normals[solid.indices[3 * tri] as usize];
        let to_axis = normalize(sub([20.0, 20.0, p[2]], p));
        assert!(dot(nrm, to_axis) > 0.9);
        // A full circle has no lateral edges, only its two cap circles, and rulings for
        // silhouettes.
        assert_eq!(solid.edges.len(), 4 * 3 + 2);
        assert!(!solid.rulings.is_empty());
        assert!((solid.volume() - (1600.0 - std::f64::consts::PI * 25.0) * 10.0).abs() < 5.0);
    }

    #[test]
    fn arcs_are_tessellated_into_one_curved_face() {
        // A D shape: a line and a half circle.
        let mut s = Sketch::new();
        SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }
        .apply(&mut s)
        .unwrap();
        SketchOp::AddArc {
            center: Vec2::ZERO,
            start: Vec2::new(0.0, -5.0),
            end: Vec2::new(0.0, 5.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let solid = solid_of(&s, PlaneRef::Front, 4.0, false);
        // Caps, one flat side, one curved side.
        assert_eq!(solid.faces.len(), 4);
        let curved = solid.faces.iter().filter(|f| f.plane.is_none()).count();
        assert_eq!(curved, 1);
        // Two lateral edges where the line and the arc meet.
        let lateral = solid
            .edges
            .iter()
            .filter(|e| e.name.faces.iter().all(|f| matches!(f.origin, FaceOrigin::Side { .. })))
            .count();
        assert_eq!(lateral, 2);
        let want = std::f64::consts::PI * 25.0 / 2.0 * 4.0;
        assert!((solid.volume() - want).abs() / want < 0.01);
    }

    #[test]
    fn flipping_extrudes_the_other_way() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 10.0);
        let solid = solid_of(&s, PlaneRef::Top, 5.0, true);
        let (lo, hi) = solid.bounds().unwrap();
        assert!((lo[2] + 5.0).abs() < 1e-9 && hi[2].abs() < 1e-9);
        let end = solid.face(&face(FaceTag::End { region: 0 })).unwrap().plane.unwrap();
        assert!(close(end.normal(), [0.0, 0.0, -1.0]));
        assert!(close(end.origin, [0.0, 0.0, -5.0]));
    }

    #[test]
    fn picking_finds_the_nearest_face() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 50.0, 30.0);
        let solid = solid_of(&s, PlaneRef::Top, 25.0, false);
        // Straight down onto the top face.
        let (f, t) = solid.pick([25.0, 15.0, 100.0], [0.0, 0.0, -1.0]).unwrap();
        assert_eq!(solid.faces[f].name, face(FaceTag::End { region: 0 }));
        assert!((t - 75.0).abs() < 1e-9);
        // From the front (-Y), the front side face.
        let (f, _) = solid.pick([25.0, -100.0, 10.0], [0.0, 1.0, 0.0]).unwrap();
        assert!(close(solid.faces[f].plane.unwrap().normal(), [0.0, -1.0, 0.0]));
        // Missing it.
        assert!(solid.pick([80.0, 15.0, 100.0], [0.0, 0.0, -1.0]).is_none());
    }

    #[test]
    fn picking_through_the_boxes_finds_what_every_triangle_would() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 50.0, 30.0);
        circle(&mut s, 15.0, 15.0, 6.0);
        circle(&mut s, 36.0, 12.0, 4.0);
        let solid = solid_of(&s, PlaneRef::Top, 25.0, false);
        let brute = |o: Vec3, d: Vec3| {
            let mut best: Option<(usize, f64)> = None;
            for (fi, f) in solid.faces.iter().enumerate() {
                for tri in f.first_triangle..f.first_triangle + f.triangle_count {
                    let [a, b, c] = [0, 1, 2].map(|k| solid.positions[solid.indices[3 * tri + k] as usize]);
                    if let Some(t) = ray_triangle(o, d, a, b, c)
                        && best.is_none_or(|(_, bt)| t < bt)
                    {
                        best = Some((fi, t));
                    }
                }
            }
            best
        };
        // Rays in many directions through a grid of points around the part (some miss it,
        // some pass behind their origin: the pick takes the whole line).
        let dirs = [[0.0, 0.0, -1.0], [0.3, 0.8, -0.5], [-0.6, 0.2, 0.7], [1.0, 0.0, 0.0], [0.577, -0.577, 0.577]];
        let mut hits = 0;
        for d in dirs {
            for i in -2..14 {
                for j in -2..10 {
                    let o = [i as f64 * 4.3, j as f64 * 3.9, 12.5];
                    let (a, b) = (solid.pick(o, d), brute(o, d));
                    assert_eq!(a.map(|x| x.0), b.map(|x| x.0), "ray from {o:?} along {d:?}");
                    hits += a.is_some() as usize;
                }
            }
        }
        assert!(hits > 100, "most rays hit the part ({hits})");
        // A moved copy gets its own boxes.
        let mut moved = solid.clone();
        moved.positions.iter_mut().for_each(|p| p[0] += 100.0);
        assert!(moved.pick([125.0, 15.0, 100.0], [0.0, 0.0, -1.0]).is_some());
        assert!(line_hits_box([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], &([-1.0; 3], [1.0; 3])));
        assert!(!line_hits_box([2.0, 0.0, 5.0], [0.0, 0.0, 1.0], &([-1.0; 3], [1.0; 3])));
    }

    #[test]
    fn touching_regions_merge() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 10.0);
        rect(&mut s, 10.0, 0.0, 10.0, 10.0);
        let rs = regions(&s);
        assert_eq!(rs.len(), 2);
        let solid = solid_of(&s, PlaneRef::Top, 5.0, false);
        // The shared wall is not a face.
        let sides = solid
            .faces
            .iter()
            .filter(|f| is_side(f))
            .count();
        assert_eq!(sides, 6);
        assert!((solid.volume() - 1000.0).abs() < 1e-6);
    }
}
