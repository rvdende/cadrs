//! The solid-modeling layer of cadrs.
//!
//! The rest of cadrs talks to B-rep kernels only through the [`Kernel`] trait and the types in
//! [`types`], so backends (OpenCascade, monstertruck, our own code) can be swapped or combined.
//! See `crates/cadrs_kernel/README.md`.

pub mod backend;
pub mod exchange;
pub mod loft;
pub mod naming;
pub mod projection;
pub mod thin;
pub mod types;

pub use naming::{BodyNames, EdgeName, FaceName, FaceOrigin, OpId, VertexName};
pub use projection::{
    ProjClass, ProjCurve, ProjEdge, ProjSource, ProjVisibility, ProjectOptions, Projection, ViewFrame,
};
pub use types::*;

use std::fmt;

use nalgebra::{Point3, Vector3};

#[derive(Debug, Clone, PartialEq)]
pub enum KernelError {
    /// The profile is empty, open or self-intersecting.
    InvalidProfile(String),
    /// The backend could not perform the operation (e.g. a fillet radius too large).
    OperationFailed(String),
    /// A parameter the kernel can't build with (e.g. a depth below its modelling tolerance);
    /// the message says why in words for the user.
    InvalidParameter(String),
    /// This backend does not implement the operation.
    Unsupported(&'static str),
    UnknownBody(BodyId),
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProfile(why) => write!(f, "invalid profile: {why}"),
            Self::OperationFailed(why) => write!(f, "operation failed: {why}"),
            Self::InvalidParameter(why) => write!(f, "{why}"),
            Self::Unsupported(op) => write!(f, "{op} is not supported by this kernel"),
            Self::UnknownBody(id) => write!(f, "unknown body {id:?}"),
        }
    }
}

impl std::error::Error for KernelError {}

pub type Result<T> = std::result::Result<T, KernelError>;

/// The bodies an operation produced, plus what it did to its inputs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpResult {
    pub bodies: Vec<BodyId>,
    pub history: History,
}

/// A kernel session: it owns bodies and performs modeling operations on them.
///
/// Operations never mutate their inputs; they return new bodies, so a feature list can be
/// rebuilt from any point.
pub trait Kernel {
    /// Short backend name, for diagnostics.
    fn name(&self) -> &'static str;

    // Creating bodies.
    fn extrude(&mut self, profile: &Profile, extent: Extent) -> Result<OpResult>;

    /// The full extrude of Onshape's dialog (P3.3): Solid, Surface or Thin; Blind, Up to
    /// next/face/part/vertex (with offset) or Through all; Symmetric, a second end, a starting
    /// offset and any direction out of the plane; planar faces as input.
    ///
    /// The default handles what [`Kernel::extrude`] can (a solid, blind or symmetric, along the
    /// plane normal, from the plane, regions only) and refuses the rest.
    fn extrude_with(&mut self, profile: &Profile, spec: &ExtrudeSpec) -> Result<OpResult> {
        let along = spec.direction.dot(&profile.plane.normal);
        let normal = (along.abs() - 1.0).abs() < 1e-12;
        let simple = spec.body == BodyKind::Solid
            && spec.start_offset == 0.0
            && spec.faces.is_empty()
            && normal;
        match (simple, spec.end, spec.symmetric, spec.second) {
            (true, ExtrudeEnd::Blind(d), false, None) => self.extrude(profile, Extent::Blind(d * along.signum())),
            (true, ExtrudeEnd::Blind(d), true, None) => self.extrude(profile, Extent::Symmetric(d)),
            (true, ExtrudeEnd::Blind(f), false, Some(ExtrudeEnd::Blind(b))) if along > 0.0 => {
                self.extrude(profile, Extent::TwoSided { forward: f, backward: b })
            }
            _ => Err(KernelError::Unsupported("this extrude")),
        }
    }
    fn revolve(&mut self, profile: &Profile, axis: Axis, angle: f64) -> Result<OpResult>;

    /// The full revolve of Onshape's dialog (P3.4, PS7): Solid, Surface or Thin; Full, an angle,
    /// Symmetric, Up to next/face/part/vertex (with an offset angle) and a second end.
    ///
    /// The default handles what [`Kernel::revolve`] can (a solid of regions, a whole turn or one
    /// angle) and refuses the rest.
    fn revolve_with(&mut self, profile: &Profile, spec: &RevolveSpec) -> Result<OpResult> {
        let simple = spec.body == BodyKind::Solid && profile.chains.is_empty() && spec.faces.is_empty();
        match (simple, spec.full, spec.end, spec.symmetric, spec.second) {
            (true, true, ..) => self.revolve(profile, spec.axis, std::f64::consts::TAU),
            (true, false, RevolveEnd::Angle(a), false, None) => self.revolve(profile, spec.axis, a),
            _ => Err(KernelError::Unsupported("this revolve")),
        }
    }
    /// A sweep of a profile along a path (P3.7, PS19): Solid, Surface or Thin, with a profile
    /// control. Faces are named as an extrude's: the side of each profile curve, the start cap
    /// (on the profile) and the end cap.
    fn sweep_with(&mut self, _profile: &Profile, _spec: &SweepSpec) -> Result<OpResult> {
        Err(KernelError::Unsupported("sweep"))
    }

    /// A loft through sections (P3.7, PS20), with start and end conditions. Faces: the side of
    /// each curve of the first profile section (`ProfileCurve { region: spec.source, .. }`), and
    /// the start and end caps (`StartCap`/`EndCap { region: spec.source }`).
    fn loft_with(&mut self, _spec: &LoftSpec) -> Result<OpResult> {
        Err(KernelError::Unsupported("loft"))
    }

    /// Splits a body with a tool (P3.7, PS18.5) into one body holding every piece (a boolean
    /// feature then makes each solid a part with [`Kernel::split_solids`]). The body's faces are
    /// `modified`; the faces the tool cut are generated as `StartCap { region: source }`.
    fn split(&mut self, _body: BodyId, _tool: &SplitTool, _source: u64) -> Result<OpResult> {
        Err(KernelError::Unsupported("split"))
    }

    /// Splits the faces `faces` of a body where a tool crosses them (P3.8, a Face split): still
    /// one body, with more faces. Each split face is `modified` into its pieces; no face is
    /// generated. An error when the tool crosses none of them.
    fn split_faces(&mut self, _body: BodyId, _faces: &[FaceId], _tool: &SplitTool) -> Result<OpResult> {
        Err(KernelError::Unsupported("face split"))
    }

    // Combining and editing bodies.
    fn boolean(&mut self, op: BoolOp, target: BodyId, tools: &[BodyId]) -> Result<OpResult>;
    fn transform(&mut self, body: BodyId, transform: &Transform) -> Result<OpResult>;

    /// A copy of the body moved by a rigid motion or reflected (P3.8: patterns and mirrors).
    /// Every face of the copy is `modified` from the same face of `body`. The default handles
    /// rigid motions through [`Kernel::transform`] and refuses reflections.
    fn transform_motion(&mut self, body: BodyId, motion: &Motion) -> Result<OpResult> {
        match motion.to_isometry() {
            Some(t) => self.transform(body, &t),
            None => Err(KernelError::Unsupported("reflections")),
        }
    }

    /// A copy of the body scaled uniformly by `factor` (greater than zero) about `center`
    /// (Onshape's Transform, Scale uniformly). Every face of the copy is `modified` from the same
    /// face of `body`.
    fn scale(&mut self, _body: BodyId, _center: Point3<f64>, _factor: f64) -> Result<OpResult> {
        Err(KernelError::Unsupported("scaling"))
    }

    /// The solid bounded by `faces` of `body` and a flat cap across each loop of their free
    /// edges (P3.8): the pocket (or boss) the faces bound, which a face pattern or a face mirror
    /// copies. Each face of the tool is `modified` from the face it continues; the caps are
    /// generated as `StartCap { region: source }`. Faces whose opening isn't flat are an error.
    fn face_tool(&mut self, _body: BodyId, _faces: &[FaceId], _source: u64) -> Result<OpResult> {
        Err(KernelError::Unsupported("face tools"))
    }

    /// Where a point is relative to a solid body (P3.8: whether a face tool is a pocket or a
    /// boss), within `tol`.
    fn classify(&self, _body: BodyId, _point: Point3<f64>, _tol: f64) -> Result<PointClass> {
        Err(KernelError::Unsupported("point classification"))
    }
    fn fillet(&mut self, _body: BodyId, _edges: &[EdgeId], _radius: f64) -> Result<OpResult> {
        Err(KernelError::Unsupported("fillet"))
    }
    fn chamfer(&mut self, _body: BodyId, _edges: &[EdgeId], _spec: ChamferSpec) -> Result<OpResult> {
        Err(KernelError::Unsupported("chamfer"))
    }
    fn shell(&mut self, _body: BodyId, _remove: &[FaceId], _thickness: f64) -> Result<OpResult> {
        Err(KernelError::Unsupported("shell"))
    }

    /// The fillet of Onshape's Edge tab (P3.6, PS14.2–14.6): a radius or a constant width, and
    /// whether it may run over onto neighbouring faces. The edges' tangent chains are filleted
    /// too (OpenCascade continues a fillet along tangent edges). The default handles a radius
    /// with overflow allowed.
    fn fillet_with(&mut self, body: BodyId, edges: &[EdgeId], spec: &FilletSpec) -> Result<OpResult> {
        match (spec.size, spec.allow_overflow) {
            (FilletSize::Radius(r), true) => self.fillet(body, edges, r),
            _ => Err(KernelError::Unsupported("this fillet")),
        }
    }

    /// Onshape's Full round fillet (P3.6, PS14.1): the `center` face is replaced by a round
    /// tangent to the two `side` faces. Built for a flat rectangular centre face between two
    /// parallel flat side faces at least half their distance apart deep (radius = half the
    /// distance); anything else is an `InvalidParameter` error saying so.
    fn full_round(&mut self, _body: BodyId, _side1: FaceId, _center: FaceId, _side2: FaceId) -> Result<OpResult> {
        Err(KernelError::Unsupported("full round"))
    }

    /// The chamfer of Onshape's dialog (P3.6, PS14.7–14.8): its type, Offset or Tangent
    /// measurement, and which face takes the first distance (flipped for all edges, or per
    /// edge). The default handles Offset without flips.
    fn chamfer_with(&mut self, body: BodyId, edges: &[EdgeId], opts: &ChamferOpts) -> Result<OpResult> {
        if opts.measurement == ChamferMeasure::Offset && !opts.flip && opts.flipped.is_empty() {
            self.chamfer(body, edges, opts.spec)
        } else {
            Err(KernelError::Unsupported("this chamfer"))
        }
    }

    /// The shell of Onshape's dialog (P3.6, PS16): faces removed, inward or outward, or a
    /// closed hollow body. A shell whose walls would intersect themselves is an
    /// `OperationFailed` error, never a wrong body. The default handles an inward shell.
    fn shell_with(&mut self, body: BodyId, spec: &ShellSpec) -> Result<OpResult> {
        if spec.outward || spec.hollow {
            return Err(KernelError::Unsupported("this shell"));
        }
        self.shell(body, &spec.remove, spec.thickness)
    }

    /// A draft (P3.10, PS4.9): the faces turned by the angle about where they meet the neutral
    /// plane, for the pull direction; the other faces follow. Faces kept or drafted are
    /// `modified`. Only planes, cylinders and cones can be drafted.
    fn draft(&mut self, _body: BodyId, _spec: &DraftSpec) -> Result<OpResult> {
        Err(KernelError::Unsupported("draft"))
    }

    /// The solid with its faces offset (P3.10, the Boolean feature's Subtract offset): every
    /// face moved outward by `distance`, or by its own distance. Faces are `modified`.
    fn offset(&mut self, _body: BodyId, _spec: &OffsetSpec) -> Result<OpResult> {
        Err(KernelError::Unsupported("offset"))
    }

    /// A fillet whose radius varies along each edge (P3.10, PS14.6 Variable fillet). Every edge
    /// tangent to a listed edge must be listed too (OCCT continues a fillet along them). With
    /// `allow_overflow` off it is refused where it runs onto another face, as `fillet_with`.
    fn fillet_variable(&mut self, _body: BodyId, _laws: &[FilletLaw], _allow_overflow: bool) -> Result<OpResult> {
        Err(KernelError::Unsupported("variable fillet"))
    }

    /// A partial fillet (P3.11, PS14.6): `spec`'s fillet on `edge` only between the fractions
    /// `from < to` of its length (in the direction the edge runs, 0 at its start). The fillet
    /// stops at a flat end face square to the edge at each bound, as Onshape's does; at a bound
    /// of 0 or 1 it ends as the whole edge's fillet would. The fillet's face is generated
    /// `FromEdge`, the end faces `StartCap`/`EndCap` with region 0.
    fn fillet_partial(&mut self, _body: BodyId, _edge: EdgeId, _spec: &FilletSpec, _from: f64, _to: f64) -> Result<OpResult> {
        Err(KernelError::Unsupported("partial fillet"))
    }

    /// Onshape's *Smooth fillet corners* (Final, PS14.6): the fillet of `edges` as
    /// [`Kernel::fillet_with`] (a constant-radius circular one), then at every vertex of `body`
    /// where three or more filleted edges meet, the corner set back: everything of the fillet
    /// within `ρ = √(setback² + r²)` of the vertex (the fillets' ends and the default corner
    /// patch; the contact lines on the faces end `setback` from the vertex) is replaced by one
    /// N-sided patch meeting every face around it tangentially (G1). The patch is generated
    /// `FromVertex`. A corner OCCT can't fill is an `OperationFailed` error saying so.
    fn fillet_smooth(&mut self, _body: BodyId, _edges: &[EdgeId], _spec: &FilletSpec, _setback: f64) -> Result<OpResult> {
        Err(KernelError::Unsupported("smooth fillet corners"))
    }

    /// The outward unit normals of `face` at the points of it nearest `points` (exact, from
    /// its surface; Final: to check that faces meet tangentially).
    fn face_normals_at(&self, _body: BodyId, _face: FaceId, _points: &[Point3<f64>]) -> Result<Vec<Vector3<f64>>> {
        Err(KernelError::Unsupported("face normals at points"))
    }

    /// Splits a body into one body per solid (a boolean can leave several pieces; Onshape makes
    /// each a part). Each result's history has every face `modified` from the same face of
    /// `body`. A body with one solid, or a surface body, comes back as one copy.
    fn split_solids(&mut self, _body: BodyId) -> Result<Vec<OpResult>> {
        Err(KernelError::Unsupported("split into solids"))
    }

    /// Gathers copies of `bodies` into one body without merging them (a compound: P3H.6's
    /// Composite part). Its faces are the inputs' faces in order (the first body's, then the
    /// next's); the history has each face `modified` from its input face. Volume and area are
    /// the sums.
    fn compound(&mut self, _bodies: &[BodyId]) -> Result<OpResult> {
        Err(KernelError::Unsupported("compound"))
    }

    /// Reads a STEP file (the Import feature): one body per solid in the file, in the reader's
    /// order (lengths converted to mm). A file without solids gives its shells or faces as one
    /// surface body.
    fn import_step(&mut self, _bytes: &[u8]) -> Result<Vec<BodyId>> {
        Err(KernelError::Unsupported("STEP import"))
    }

    /// A solid bounded by a closed triangle mesh (an STL file's triangles, mm): the triangles
    /// become planar faces, sewn within `tol` into a closed shell and made solid.
    fn mesh_solid(&mut self, _triangles: &[[Point3<f64>; 3]], _tol: f64) -> Result<BodyId> {
        Err(KernelError::Unsupported("mesh to solid"))
    }

    /// Forgets a body (for example when a rebuild cache evicts a feature's output). Unknown ids
    /// are ignored.
    /// Thickens surfaces and faces into a solid (Onshape's Thicken).
    fn thicken_surfaces(&mut self, _spec: &ThickenSpec) -> Result<OpResult> {
        Err(KernelError::Unsupported("thicken"))
    }

    /// A surface filling a closed chain of curves (Onshape's Fill).
    fn fill(&mut self, _spec: &FillSpec) -> Result<OpResult> {
        Err(KernelError::Unsupported("fill"))
    }

    /// Sews surface bodies that meet along their edges into one solid, if they close it.
    fn sew_solid(&mut self, _bodies: &[BodyId], _tol: f64) -> Result<OpResult> {
        Err(KernelError::Unsupported("sew"))
    }

    fn release(&mut self, _body: BodyId) {}

    /// A body as bytes (the backend's own exact format), which [`Self::read_body`] gives back
    /// with the same faces, edges and vertices in the same order: names indexed by them stay
    /// valid (cadrs caches built bodies, and they may travel between peers).
    fn write_body(&self, _body: BodyId) -> Result<Vec<u8>> {
        Err(KernelError::Unsupported("write body"))
    }

    /// Reads a body written by [`Self::write_body`] into this session.
    fn read_body(&mut self, _bytes: &[u8]) -> Result<BodyId> {
        Err(KernelError::Unsupported("read body"))
    }

    // Queries.
    fn faces(&self, body: BodyId) -> Result<Vec<FaceInfo>>;
    fn edges(&self, body: BodyId) -> Result<Vec<EdgeInfo>>;
    fn mass_properties(&self, body: BodyId) -> Result<MassProperties>;
    fn tessellate(&self, body: BodyId, quality: Tessellation) -> Result<TriMesh>;

    /// The number of solids in a body (0 for a surface body).
    fn solid_count(&self, _body: BodyId) -> Result<usize> {
        Ok(1)
    }

    /// Whether two solid bodies touch or overlap: their union is one solid. Only asks the
    /// question (the automatic merge scope of a new body), so a backend can skip the work a
    /// real union does to make a clean result.
    fn joins(&mut self, a: BodyId, b: BodyId) -> Result<bool> {
        let r = self.boolean(BoolOp::Union, a, &[b])?;
        let joined = self.solid_count(r.bodies[0]).unwrap_or(2) == 1;
        self.release(r.bodies[0]);
        Ok(joined)
    }

    /// A tight axis-aligned bounding box of the body's exact geometry.
    fn bounding_box(&self, _body: BodyId) -> Result<Aabb> {
        Err(KernelError::Unsupported("bounding box"))
    }

    /// Where the line through `origin` along the unit `dir` crosses the body's faces, sorted by
    /// `t` (hits behind the origin have negative `t`). Up to next uses it to find what lies
    /// ahead of a profile.
    fn ray_hits(&self, _body: BodyId, _origin: Point3<f64>, _dir: Vector3<f64>) -> Result<Vec<RayHit>> {
        Err(KernelError::Unsupported("ray queries"))
    }
    fn export_step(&self, _bodies: &[BodyId]) -> Result<Vec<u8>> {
        Err(KernelError::Unsupported("STEP export"))
    }

    /// Reads a STEP or IGES file (P3F.2, [`exchange`]): each distinct part once as a body, with
    /// its occurrences (placements and names) as the file's assembly structure gives them.
    fn import_model(&mut self, _format: exchange::ExchangeFormat, _bytes: &[u8]) -> Result<exchange::ImportedModel> {
        Err(KernelError::Unsupported("importing files"))
    }

    /// Writes `parts` (bodies with names) to STEP or IGES (P3F.2). With `instances`, the file
    /// holds one assembly called `name` with the parts at those placements.
    fn export_model(
        &self,
        _format: exchange::ExchangeFormat,
        _name: &str,
        _parts: &[(BodyId, String)],
        _instances: &[exchange::ExportInstance],
    ) -> Result<Vec<u8>> {
        Err(KernelError::Unsupported("exporting files"))
    }

    /// Hidden-line removal for a drawing view (P3C.2): `bodies` seen along `frame`, as 2D edges
    /// sorted into visible and hidden, sharp, smooth (tangent) and outline, each with the body
    /// edge or face it came from (see [`projection`]). The bodies hide each other.
    fn project(&self, _bodies: &[BodyId], _frame: &ViewFrame, _opts: &ProjectOptions) -> Result<Projection> {
        Err(KernelError::Unsupported("view projection"))
    }

    /// The body's vertices. The default finds them as the distinct ends of [`Kernel::edges`]
    /// (closed edges, such as whole circles, have none); backends with a vertex list override
    /// it.
    fn vertices(&self, body: BodyId) -> Result<Vec<VertexInfo>> {
        let edges = self.edges(body)?;
        let tol = naming::joint_tolerance(&edges);
        let mut out: Vec<VertexInfo> = Vec::new();
        for e in &edges {
            if e.is_closed() || e.curve == CurveKind::Degenerate {
                continue;
            }
            for p in [e.start, e.end] {
                match out.iter_mut().find(|v| (v.point - p).norm() < tol) {
                    Some(v) => {
                        if !v.edges.contains(&e.id) {
                            v.edges.push(e.id);
                        }
                    }
                    None => out.push(VertexInfo {
                        id: VertexId(out.len() as u64),
                        point: p,
                        edges: vec![e.id],
                    }),
                }
            }
        }
        Ok(out)
    }

    /// The faces that share an edge with `face` (each once, in id order).
    fn adjacent_faces(&self, body: BodyId, face: FaceId) -> Result<Vec<FaceId>> {
        let mut out: Vec<FaceId> = self
            .edges(body)?
            .iter()
            .filter(|e| e.faces.contains(&Some(face)))
            .flat_map(|e| e.faces.into_iter().flatten())
            .filter(|f| *f != face)
            .collect();
        out.sort_by_key(|f| f.0);
        out.dedup();
        Ok(out)
    }

    /// The edges tangent-connected to `edge` (Onshape's "tangent propagation"): `edge` and
    /// every edge reached from it through vertices where the two edges' tangents are parallel
    /// (within [`TANGENT_ANGLE`]). In id order.
    fn tangent_chain(&self, body: BodyId, edge: EdgeId) -> Result<Vec<EdgeId>> {
        let edges = self.edges(body)?;
        let vertices = self.vertices(body)?;
        let tol = naming::joint_tolerance(&edges);
        let get = |id: EdgeId| edges.iter().find(|e| e.id == id);
        if get(edge).is_none() {
            return Err(KernelError::OperationFailed(format!("unknown {edge:?}")));
        }
        // The tangent of an edge where it touches the point `p` (either end of a closed edge).
        let tangents_at = |e: &EdgeInfo, p: Point3<f64>| -> Vec<nalgebra::Vector3<f64>> {
            let mut t = Vec::new();
            if (e.start - p).norm() < tol {
                t.push(e.start_tangent);
            }
            if (e.end - p).norm() < tol {
                t.push(e.end_tangent);
            }
            t
        };
        let cos = TANGENT_ANGLE.cos();
        let mut chain = vec![edge];
        let mut todo = vec![edge];
        while let Some(cur) = todo.pop() {
            let Some(e) = get(cur) else { continue };
            for v in vertices.iter().filter(|v| v.edges.contains(&cur)) {
                let mine = tangents_at(e, v.point);
                for &other in &v.edges {
                    if chain.contains(&other) {
                        continue;
                    }
                    let Some(o) = get(other) else { continue };
                    let tangent = tangents_at(o, v.point)
                        .iter()
                        .any(|t| mine.iter().any(|m| m.dot(t).abs() >= cos));
                    if tangent {
                        chain.push(other);
                        todo.push(other);
                    }
                }
            }
        }
        chain.sort_by_key(|e| e.0);
        Ok(chain)
    }
}

/// How far apart (radians) two edges' tangents may be at a shared vertex and still count as
/// tangent-connected: 0.1°.
pub const TANGENT_ANGLE: f64 = 0.1 * std::f64::consts::PI / 180.0;
