//! The solid-modeling layer of cadrs.
//!
//! The rest of cadrs talks to the B-rep kernel only through the [`Kernel`] trait and the types in
//! [`types`]. The kernel is [`native`]: exact curves and surfaces, booleans, features,
//! tessellation, hidden-line removal and STEP/IGES exchange in pure Rust (no C or C++).
//! See `crates/cadrs_kernel/README.md` ("Using the kernel").
//!
//! # Using the kernel
//!
//! A session ([`DefaultKernel`]) owns bodies and hands out [`BodyId`]s. Operations never change
//! their inputs: each returns new bodies in an [`OpResult`] with a [`History`] of what became of
//! the input faces (for persistent naming, [`naming`]). Faces and edges are picked by the ids
//! [`Kernel::faces`] and [`Kernel::edges`] report. Lengths are mm, angles radians.
//!
//! ```
//! use cadrs_kernel::{BoolOp, Curve2, DefaultKernel, Extent, Kernel, Loop, Plane, Profile, Region};
//! use nalgebra::Point2;
//!
//! /// The rectangle [x0, x1] × [y0, y1] on the top plane.
//! fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
//!     let c = [Point2::new(x0, y0), Point2::new(x1, y0), Point2::new(x1, y1), Point2::new(x0, y1)];
//!     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
//!     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
//! }
//!
//! let mut k = DefaultKernel::new();
//! let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
//! let pocket = k.extrude(&rect(5.0, 2.0, 10.0, 8.0), Extent::Blind(5.0))?.bodies[0];
//! let cut = k.boolean(BoolOp::Subtract, block, &[pocket])?;
//! assert!((k.mass_properties(cut.bodies[0])?.volume - (1000.0 - 150.0)).abs() < 1e-9);
//! // The block is still there: operations make new bodies.
//! assert!((k.mass_properties(block)?.volume - 1000.0).abs() < 1e-9);
//! # Ok::<(), cadrs_kernel::KernelError>(())
//! ```

#![warn(missing_docs)]

/// Kernel backends by name (`backend::native::NativeKernel`, the only one).
pub mod backend;
pub mod exchange;
pub mod loft;
pub mod naming;
/// The native kernel: [`DefaultKernel`] and the curated B-rep API (data model, geometry,
/// tolerances, body format) it re-exports; its other modules are internal.
// With `testkit`, every internal module is public too, and not all of their items are documented.
#[cfg_attr(feature = "testkit", allow(missing_docs))]
pub mod native;
pub mod projection;
pub mod query;
pub mod thin;
pub mod types;

/// The kernel's internals for its own tests and tools (the `testkit` feature, which the crate's
/// tests turn on): not part of the API, and free to change. The B-rep data model and geometry
/// are in [`native`]'s stable re-exports.
#[cfg(feature = "testkit")]
#[allow(missing_docs)]
pub mod testkit {
    pub use crate::native::{
        blend, boolean, brep_occt, classify, curve, draw, evolved, exchange, face_build, features,
        iges, kernel, local, mass, math, nurbs, offset, oracle, profile, silhouette, smb, ssi,
        ssi_numeric, step, surface, surfacing, tess, topo, unify, xbuild,
    };
}

pub use naming::{BodyNames, EdgeName, FaceName, FaceOrigin, OpId, VertexName};
pub use projection::{
    ProjClass, ProjCurve, ProjEdge, ProjSource, ProjVisibility, ProjectOptions, Projection,
    ViewFrame,
};
pub use query::{
    DistanceLocation, DistanceResult, DistanceTarget, SilhouetteCurve, SilhouetteView, UvRect,
};
pub use types::*;

/// The kernel cadrs builds with: the native (pure Rust) B-rep kernel.
pub use native::NativeKernel as DefaultKernel;

use std::fmt;

use crate::native::tol;
use nalgebra::{Point3, Vector3};

/// Why an operation failed. Each variant carries a message in words for the user; the variant
/// says what a caller can do about it (e.g. read `EmptyResult` of an Intersect as "no
/// interference", retry an `OpenResult` with other options, report `InvalidInput` as a bad
/// file, offer a smaller size on `TooLarge`).
///
/// The failures of an algorithm that gives up ([`KernelError::is_failure`]) say where it gave
/// up: `stage` and `op` are short fixed names (`"shell assembly"`, `"fillet"`) a caller can
/// match on or log; the message is for the user.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum KernelError {
    /// The profile is empty, open or self-intersecting.
    InvalidProfile(String),
    /// The backend could not perform the operation, for a reason none of the variants below
    /// names.
    OperationFailed(String),
    /// A boolean (or an operation run as one) built pieces that don't make a valid result at
    /// `stage`: `"face closure"` (a face whose boundary doesn't close), `"validity"` (a body
    /// that fails its check), `"sheet union"` (sheets meeting three to an edge). A boolean of
    /// solids retries at larger tolerances on the first two before returning one.
    BooleanFailed {
        /// Where the boolean gave up.
        stage: &'static str,
        /// The message in words.
        why: String,
    },
    /// The operation ran out of its work budget at `stage` (a count of steps, the same on
    /// every run, not a time): the input is too complex for it, or the algorithm ran away.
    /// For a boolean the stages are `"surface intersection"`, `"face splitting"`,
    /// `"classification"` and `"shell assembly"`; for a blend, `"sections"`; for a local
    /// fillet, `"fillet"` (its rolling-ball solves).
    BudgetExhausted {
        /// Where the work ran out.
        stage: &'static str,
        /// The message in words.
        why: String,
    },
    /// A local fillet or chamfer (trimmed into its faces) couldn't build its result at `stage`:
    /// `"rolling ball"` (the ball lost along an edge), `"corner"` (where blends meet at a
    /// vertex), `"end"` (where a blend stops on an end face), `"surface"` (the blend surface
    /// from its sections) or `"trimming"` (cutting the blend's rails into its faces). The
    /// operation may still succeed through another method; this is what the local one hit.
    BlendFailed {
        /// Where the blend gave up.
        stage: &'static str,
        /// The message in words.
        why: String,
    },
    /// A size (radius, distance, thickness) too large for the geometry it is applied to: a
    /// fillet larger than the faces at its edge, an offset that makes a face vanish, walls that
    /// leave no cavity. A smaller size may work.
    TooLarge {
        /// The operation (`"fillet"`, `"offset"`, `"shell"`, ...).
        op: &'static str,
        /// The message in words.
        why: String,
    },
    /// The geometry at the selection doesn't allow the operation: faces tangent along the edge
    /// to blend, a section that degenerates, faces that no longer meet after a move. Another
    /// selection (or other options) may work.
    InvalidGeometry {
        /// The operation (`"fillet"`, `"offset"`, `"loft"`, ...).
        op: &'static str,
        /// The message in words.
        why: String,
    },
    /// The operation built a body that fails the validity check (open, inside out, not one
    /// solid where one was due): it is never returned; this is a failure of the algorithm on
    /// this input.
    InvalidResult {
        /// The operation (`"fillet"`, `"sweep"`, `"offset"`, ...).
        op: &'static str,
        /// The message in words.
        why: String,
    },
    /// The operation would leave nothing: an Intersect of bodies that don't meet, a cut that
    /// removes the whole body.
    EmptyResult(String),
    /// The operation built faces that don't close into a solid (a boolean or blend that left an
    /// open shell). Never returned as a body: the caller gets this error instead.
    OpenResult(String),
    /// A parameter the kernel can't build with (e.g. a depth below its modelling tolerance);
    /// the message says why in words for the user.
    InvalidParameter(String),
    /// Input data that isn't well formed: a file that doesn't parse, a B-spline whose knots,
    /// degree or weights don't fit its control points, a body whose topology refers to
    /// entities it doesn't have, non-finite coordinates.
    InvalidInput(String),
    /// This backend does not implement the operation.
    Unsupported(&'static str),
    /// The id names no body of this session (never made here, or released).
    UnknownBody(BodyId),
}

impl KernelError {
    /// The message in words, without the kind's prefix.
    pub fn message(&self) -> String {
        match self {
            Self::BooleanFailed { why, .. }
            | Self::BlendFailed { why, .. }
            | Self::BudgetExhausted { why, .. }
            | Self::TooLarge { why, .. }
            | Self::InvalidGeometry { why, .. }
            | Self::InvalidResult { why, .. } => why.clone(),
            Self::InvalidProfile(why)
            | Self::OperationFailed(why)
            | Self::EmptyResult(why)
            | Self::OpenResult(why)
            | Self::InvalidParameter(why)
            | Self::InvalidInput(why) => why.clone(),
            Self::Unsupported(op) => format!("{op} is not supported by this kernel"),
            Self::UnknownBody(id) => format!("unknown body {id:?}"),
        }
    }

    /// Whether the algorithm gave up on this input ([`KernelError::OperationFailed`] or one of
    /// the typed failures, `BooleanFailed`, `BlendFailed`, `BudgetExhausted`, `TooLarge`,
    /// `InvalidGeometry`, `InvalidResult`), as opposed to refusing its arguments or finding nothing to make.
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            Self::OperationFailed(_)
                | Self::BooleanFailed { .. }
                | Self::BlendFailed { .. }
                | Self::BudgetExhausted { .. }
                | Self::TooLarge { .. }
                | Self::InvalidGeometry { .. }
                | Self::InvalidResult { .. }
        )
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProfile(why) => write!(f, "invalid profile: {why}"),
            Self::OperationFailed(why) | Self::EmptyResult(why) | Self::OpenResult(why) => {
                write!(f, "operation failed: {why}")
            }
            Self::BooleanFailed { why, .. }
            | Self::BlendFailed { why, .. }
            | Self::BudgetExhausted { why, .. }
            | Self::TooLarge { why, .. }
            | Self::InvalidGeometry { why, .. }
            | Self::InvalidResult { why, .. } => write!(f, "operation failed: {why}"),
            Self::InvalidParameter(why) => write!(f, "{why}"),
            Self::InvalidInput(why) => write!(f, "invalid input: {why}"),
            Self::Unsupported(op) => write!(f, "{op} is not supported by this kernel"),
            Self::UnknownBody(id) => write!(f, "unknown body {id:?}"),
        }
    }
}

impl std::error::Error for KernelError {}

/// The result of a kernel call.
pub type Result<T> = std::result::Result<T, KernelError>;

/// The bodies an operation produced, plus what it did to its inputs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpResult {
    /// The new bodies (usually one), owned by the session until [`Kernel::release`]d.
    pub bodies: Vec<BodyId>,
    /// What became of the inputs' faces and where the new ones came from.
    pub history: History,
}

/// One step of [`Kernel::heal`] (shape healing and upgrade, as OCCT's `ShapeFix`,
/// `ShapeCustom` and `ShapeUpgrade` do them). Lengths are mm, angles radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HealOp {
    /// Make the body sound: loops closed (edges shorter than `precision` dropped, gaps up to
    /// `max_tolerance` closed by merging vertices), each face's outer loop first and its sense
    /// agreeing with its loops, each shell's faces oriented alike and closed shells pointing
    /// out, p-curves refitted where they stray, tolerances fitted to the gaps (at most
    /// `max_tolerance`).
    Fix {
        /// The working precision.
        precision: f64,
        /// The largest tolerance a vertex or edge may be given.
        max_tolerance: f64,
    },
    /// Edges shorter than `tolerance` dropped, their ends merged (seams and poles stay).
    DropSmallEdges {
        /// The length below which an edge goes.
        tolerance: f64,
    },
    /// Consecutive edges of a loop whose ends lie within `tolerance` joined at one vertex.
    FixGaps {
        /// The largest gap closed.
        tolerance: f64,
    },
    /// p-curves straying from their edges by more than the edge's tolerance (and
    /// `tolerance`) projected again; tolerances raised to the gaps left.
    SameParameter {
        /// The gap a p-curve may leave without being refitted.
        tolerance: f64,
    },
    /// Every tolerance recomputed from the body's actual gaps (lowered as well as raised).
    UpdateTolerance,
    /// Surfaces on mirrored frames re-expressed on direct ones (the same faces).
    DirectFaces,
    /// Cylinders, cones, spheres and tori as surfaces of revolution.
    ToRevolution,
    /// Every curved surface as an exact B-spline.
    ToBSpline,
    /// Faces of surfaces of revolution cut so none turns through more than `max_angle`.
    SplitAngle {
        /// The largest angle a face may span about its axis.
        max_angle: f64,
    },
    /// Faces that close on themselves (a seam) cut in two across each closed direction.
    SplitClosed,
    /// Faces and edges cut where their B-splines are only C0.
    SplitContinuity,
    /// Faces and edges cut at their B-splines' knots, each piece's B-spline of one span (a
    /// Bézier).
    ToBezier,
    /// Faces on one surface and edges on one curve merged.
    Unify,
    /// Tails of loops (edges running out and back along each other within `max_width`, at
    /// most `max_angle` apart) removed.
    Tails {
        /// The largest angle between the two sides of a tail.
        max_angle: f64,
        /// The largest width of a tail.
        max_width: f64,
    },
}

/// A kernel session: it owns bodies and performs modeling operations on them.
///
/// Operations never mutate their inputs; they return new bodies, so a feature list can be
/// rebuilt from any point.
///
/// The contract every operation shares, unless its own docs say otherwise:
/// - **Inputs**: bodies by [`BodyId`] of this session ([`KernelError::UnknownBody`] otherwise),
///   faces and edges by the ids [`Kernel::faces`] and [`Kernel::edges`] give for that body.
///   Lengths are mm, angles radians.
/// - **Errors**: [`KernelError::InvalidParameter`] for a size, angle or selection that can't
///   build (non-positive radius, unknown face), [`KernelError::InvalidProfile`] for a profile
///   that doesn't close, a failure ([`KernelError::is_failure`]: `TooLarge` for a size the
///   geometry can't hold, `InvalidGeometry`, `BooleanFailed`, `BlendFailed`,
///   `BudgetExhausted`, `InvalidResult` or `OperationFailed`) when the algorithm gives up, and
///   [`KernelError::OpenResult`] when faces didn't close into a solid. An operation never
///   returns a body it found invalid: results pass the native kernel's `Body::check`.
/// - **Tolerance**: exact geometry is within `native::tol::POINT` (1e-6 mm); vertices and edges
///   carry their own tolerance when an operation had to merge within more (read files, healed
///   shapes). Models must fit in the size box `native::tol::SIZE` (±1e8 mm).
/// - **History**: [`OpResult::history`] lists each result face as `generated` (with an
///   [`Origin`]: a profile curve, a cap, an input edge or vertex) or `modified` (continuing an
///   input face, maybe trimmed or split), and the input faces that are gone as `deleted`.
///
/// A backend implements every operation; the few provided methods are built on the others
/// (the dialog forms such as [`Kernel::extrude_with`] on the plain ones, the topology queries on
/// [`Kernel::edges`]), and the native kernel overrides those too.
pub trait Kernel {
    /// Short backend name, for diagnostics.
    fn name(&self) -> &'static str;

    // Creating bodies.

    /// A solid prism: the profile's regions swept along the plane normal by `extent`.
    ///
    /// Regions that overlap or touch are merged; holes stay holes. `InvalidProfile` for a
    /// profile without regions or with a loop that doesn't close. History: every face is
    /// generated, sides as [`Origin::ProfileCurve`], caps as [`Origin::StartCap`] (on the
    /// sketch plane, or where the sweep starts) and [`Origin::EndCap`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel, Origin};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let r = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Symmetric(5.0))?;
    /// let block = r.bodies[0];
    /// assert_eq!(k.faces(block)?.len(), 6);
    /// assert_eq!(r.history.generated.iter().filter(|(_, o)| matches!(o, Origin::ProfileCurve { .. })).count(), 4);
    /// let m = k.mass_properties(block)?;
    /// assert!((m.volume - 1000.0).abs() < 1e-9 && m.center_of_mass.z.abs() < 1e-9);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn extrude(&mut self, profile: &Profile, extent: Extent) -> Result<OpResult>;

    /// The full extrude of Onshape's dialog (P3.3): Solid, Surface or Thin; Blind, Up to
    /// next/face/part/vertex (with offset) or Through all; Symmetric, a second end, a starting
    /// offset and any direction out of the plane; planar faces as input.
    ///
    /// The default handles what [`Kernel::extrude`] can (a solid, blind or symmetric, along the
    /// plane normal, from the plane, regions only) and refuses the rest.
    fn extrude_with(&mut self, profile: &Profile, spec: &ExtrudeSpec) -> Result<OpResult> {
        let along = spec.direction.dot(&profile.plane.normal);
        let normal = (along.abs() - 1.0).abs() < tol::ANGULAR_FINE;
        let simple = spec.body == BodyKind::Solid
            && spec.start_offset == 0.0
            && spec.faces.is_empty()
            && normal;
        match (simple, spec.end, spec.symmetric, spec.second) {
            (true, ExtrudeEnd::Blind(d), false, None) => {
                self.extrude(profile, Extent::Blind(d * along.signum()))
            }
            (true, ExtrudeEnd::Blind(d), true, None) => self.extrude(profile, Extent::Symmetric(d)),
            (true, ExtrudeEnd::Blind(f), false, Some(ExtrudeEnd::Blind(b))) if along > 0.0 => self
                .extrude(
                    profile,
                    Extent::TwoSided {
                        forward: f,
                        backward: b,
                    },
                ),
            _ => Err(KernelError::Unsupported("this extrude")),
        }
    }
    /// A solid of revolution: the profile's regions turned about `axis` (in or off the
    /// profile's plane, not crossing the regions) by `angle` radians, counter-clockwise about
    /// `axis.dir`; a whole turn at 2π or more. An axis square to the plane or through a region
    /// is an error. History as [`Kernel::extrude`]'s.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{Axis, DefaultKernel, Kernel};
    /// # use nalgebra::{Point3, Vector3};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// // The rectangle 2 ≤ x ≤ 6, 0 ≤ y ≤ 5 on the XZ plane (its y is z), turned once about Z: a
    /// // ring of volume π(6² − 2²)·5.
    /// let xz = Plane { origin: Point3::origin(), x_dir: Vector3::x_axis(), normal: -Vector3::y_axis() };
    /// let profile = Profile { plane: xz, ..rect(2.0, 0.0, 6.0, 5.0) };
    /// let z = Axis { origin: Point3::origin(), dir: Vector3::z_axis() };
    /// let ring = k.revolve(&profile, z, std::f64::consts::TAU)?.bodies[0];
    /// let want = std::f64::consts::PI * 32.0 * 5.0;
    /// assert!((k.mass_properties(ring)?.volume - want).abs() < 1e-6 * want);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn revolve(&mut self, profile: &Profile, axis: Axis, angle: f64) -> Result<OpResult>;

    /// The full revolve of Onshape's dialog (P3.4, PS7): Solid, Surface or Thin; Full, an angle,
    /// Symmetric, Up to next/face/part/vertex (with an offset angle) and a second end.
    ///
    /// The default handles what [`Kernel::revolve`] can (a solid of regions, a whole turn or one
    /// angle) and refuses the rest.
    fn revolve_with(&mut self, profile: &Profile, spec: &RevolveSpec) -> Result<OpResult> {
        let simple =
            spec.body == BodyKind::Solid && profile.chains.is_empty() && spec.faces.is_empty();
        match (simple, spec.full, spec.end, spec.symmetric, spec.second) {
            (true, true, ..) => self.revolve(profile, spec.axis, std::f64::consts::TAU),
            (true, false, RevolveEnd::Angle(a), false, None) => self.revolve(profile, spec.axis, a),
            _ => Err(KernelError::Unsupported("this revolve")),
        }
    }
    /// A sweep of a profile along a path (P3.7, PS19): Solid, Surface or Thin, with a profile
    /// control. Faces are named as an extrude's: the side of each profile curve, the start cap
    /// (on the profile) and the end cap.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{BodyKind, DefaultKernel, Kernel, PathCurve, SweepControl, SweepSpec};
    /// # use nalgebra::{Point2, Point3, Vector3};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// // A unit disc square to X, swept 10 along X: a cylinder of volume 10π.
    /// let across = Plane { origin: Point3::origin(), x_dir: Vector3::y_axis(), normal: Vector3::x_axis() };
    /// let disc = Curve2::Circle { center: Point2::origin(), radius: 1.0, source: None };
    /// let profile = Profile::new(across, vec![Region { outer: Loop { curves: vec![disc] }, holes: vec![], source: None }]);
    /// let line = Curve2::Line { a: Point2::new(0.0, 0.0), b: Point2::new(10.0, 0.0), source: None };
    /// let path = vec![PathCurve::Sketch { plane: Plane::top(), curve: line }];
    /// let spec = SweepSpec { body: BodyKind::Solid, path, control: SweepControl::None, faces: vec![] };
    /// let rod = k.sweep_with(&profile, &spec)?.bodies[0];
    /// assert!((k.mass_properties(rod)?.volume - 10.0 * std::f64::consts::PI).abs() < 1e-6);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn sweep_with(&mut self, profile: &Profile, spec: &SweepSpec) -> Result<OpResult>;

    /// A loft through sections (P3.7, PS20), with start and end conditions. Faces: the side of
    /// each curve of the first profile section (`ProfileCurve { region: spec.source, .. }`), and
    /// the start and end caps (`StartCap`/`EndCap { region: spec.source }`).
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{BodyKind, DefaultKernel, Kernel, LoftEnd, LoftSection, LoftSpec};
    /// # use nalgebra::Point3;
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// // Two 10 × 10 squares 20 apart: the loft between them is the prism, 2000.
    /// let at = |z: f64| Profile { plane: Plane { origin: Point3::new(0.0, 0.0, z), ..Plane::top() }, ..rect(-5.0, -5.0, 5.0, 5.0) };
    /// let sections = vec![LoftSection::Profile(at(0.0)), LoftSection::Profile(at(20.0))];
    /// let spec = LoftSpec { body: BodyKind::Solid, sections, start: LoftEnd::default(), end: LoftEnd::default(), source: 1 };
    /// let prism = k.loft_with(&spec)?.bodies[0];
    /// assert!((k.mass_properties(prism)?.volume - 2000.0).abs() < 1e-6);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn loft_with(&mut self, spec: &LoftSpec) -> Result<OpResult>;

    /// An evolved solid (OCCT's `BRepOffsetAPI_MakeEvolved` in volume mode): the `profile`'s
    /// regions swept along the planar `spine`, held square to it, with the profile's x measured
    /// outwards from the spine in the spine's plane and its y up from that plane. The spine's
    /// curves (lines and circular arcs) are chained end to end; a closed spine is taken
    /// counter-clockwise about its plane's normal, which is "up". Where the spine turns away from
    /// the profile's side, the corner is filled by the profile turned about the corner (arc
    /// joins, as OCCT's offsets); where it turns in, the sweeps overlap. The profile must lie on
    /// one side of x = 0. No history.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{Curve2, DefaultKernel, Kernel, Loop, PathCurve, Plane, Region};
    /// # use nalgebra::Point2;
    /// let line = |a: (f64, f64), b: (f64, f64)| Curve2::Line { a: Point2::new(a.0, a.1), b: Point2::new(b.0, b.1), source: None };
    /// // A 40 × 20 rectangle on the top plane, and a 5 × 3 square outside it.
    /// let c = [(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)];
    /// let spine: Vec<PathCurve> = (0..4).map(|i| PathCurve::Sketch { plane: Plane::top(), curve: line(c[i], c[(i + 1) % 4]) }).collect();
    /// let s = [(0.0, 0.0), (5.0, 0.0), (5.0, 3.0), (0.0, 3.0)];
    /// let square = Region { outer: Loop { curves: (0..4).map(|i| line(s[i], s[(i + 1) % 4])).collect() }, holes: vec![], source: None };
    /// let mut k = DefaultKernel::new();
    /// let r = Kernel::evolved(&mut k, &spine, &[square])?;
    /// // Four prisms along the sides and four quarter turns at the corners: h (2w(a + b) + πw²).
    /// let want = 3.0 * (2.0 * 5.0 * 60.0 + std::f64::consts::PI * 25.0);
    /// assert!((k.mass_properties(r.bodies[0])?.volume - want).abs() < 1e-6 * want);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn evolved(&mut self, spine: &[PathCurve], profile: &[Region]) -> Result<OpResult>;

    /// Splits a body with a tool (P3.7, PS18.5) into one body holding every piece (a boolean
    /// feature then makes each solid a part with [`Kernel::split_solids`]). The body's faces are
    /// `modified`; the faces the tool cut are generated as `StartCap { region: source }`.
    fn split(&mut self, body: BodyId, tool: &SplitTool, source: u64) -> Result<OpResult>;

    /// Splits the faces `faces` of a body where a tool crosses them (P3.8, a Face split): still
    /// one body, with more faces. Each split face is `modified` into its pieces; no face is
    /// generated. An error when the tool crosses none of them.
    fn split_faces(&mut self, body: BodyId, faces: &[FaceId], tool: &SplitTool)
    -> Result<OpResult>;

    // Combining and editing bodies.

    /// Unites, subtracts or intersects `tools` with `target` (Onshape's Add, Remove and
    /// Intersect), giving one body (it may hold several solids: [`Kernel::split_solids`]).
    ///
    /// Faces of the inputs that meet within the run tolerance are merged (coplanar and
    /// coincident faces included). Errors: `EmptyResult` when nothing is left (an Intersect of
    /// bodies that don't meet, a Subtract that removes everything), `OpenResult` or
    /// `BooleanFailed` when the pieces don't close into a valid solid, `BudgetExhausted` for
    /// inputs too complex for the boolean's work budget. History: every result
    /// face is `modified` from the input face it is a piece of (target or tool); input faces
    /// with no piece left are `deleted`; nothing is generated.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{BoolOp, DefaultKernel, Extent, Kernel, KernelError};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let a = k.extrude(&rect(0.0, 0.0, 10.0, 10.0), Extent::Blind(10.0))?.bodies[0];
    /// let b = k.extrude(&rect(5.0, 5.0, 15.0, 15.0), Extent::Blind(10.0))?.bodies[0];
    /// let union = k.boolean(BoolOp::Union, a, &[b])?;
    /// assert!((k.mass_properties(union.bodies[0])?.volume - 1750.0).abs() < 1e-9);
    /// let common = k.boolean(BoolOp::Intersect, a, &[b])?;
    /// assert!((k.mass_properties(common.bodies[0])?.volume - 250.0).abs() < 1e-9);
    /// assert!(!common.history.modified.is_empty() && common.history.generated.is_empty());
    /// // Bodies that don't meet have no common part.
    /// let far = k.extrude(&rect(50.0, 50.0, 60.0, 60.0), Extent::Blind(1.0))?.bodies[0];
    /// assert!(matches!(k.boolean(BoolOp::Intersect, a, &[far]), Err(KernelError::EmptyResult(_))));
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn boolean(&mut self, op: BoolOp, target: BodyId, tools: &[BodyId]) -> Result<OpResult>;

    /// The cells of an arrangement of solids (OCCT's `BOPAlgo_CellsBuilder`): the connected
    /// solids `bodies` split one another into, each inside some of the bodies and outside the
    /// rest, as one body each. `select` keeps the cells inside the bodies it marks `true` and
    /// outside those it marks `false` (by index into `bodies`; the others either way); with an
    /// empty `select`, every cell inside at least one body. No history.
    ///
    /// `InvalidParameter` for an index out of range; `Unsupported` for more than twelve bodies
    /// `select` leaves free (each of their combinations is a run of booleans).
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let a = k.extrude(&rect(0.0, 0.0, 10.0, 10.0), Extent::Blind(10.0))?.bodies[0];
    /// let b = k.extrude(&rect(5.0, 0.0, 15.0, 10.0), Extent::Blind(10.0))?.bodies[0];
    /// // a only, both, b only.
    /// assert_eq!(k.cells(&[a, b], &[])?.len(), 3);
    /// // Inside a, outside b.
    /// let only_a = k.cells(&[a, b], &[(0, true), (1, false)])?;
    /// assert_eq!(only_a.len(), 1);
    /// assert!((k.mass_properties(only_a[0])?.volume - 500.0).abs() < 1e-9);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn cells(&mut self, bodies: &[BodyId], select: &[(usize, bool)]) -> Result<Vec<BodyId>>;

    /// A copy of the body moved by a rigid motion. Every face of the copy is `modified` from
    /// the face of `body` with the same id; topology and ids are kept.
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
    fn scale(&mut self, body: BodyId, center: Point3<f64>, factor: f64) -> Result<OpResult>;

    /// The solid bounded by `faces` of `body` and a flat cap across each loop of their free
    /// edges (P3.8): the pocket (or boss) the faces bound, which a face pattern or a face mirror
    /// copies. Each face of the tool is `modified` from the face it continues; the caps are
    /// generated as `StartCap { region: source }`. Faces whose opening isn't flat are an error.
    fn face_tool(&mut self, body: BodyId, faces: &[FaceId], source: u64) -> Result<OpResult>;

    /// Where a point is relative to a solid body (P3.8: whether a face tool is a pocket or a
    /// boss), within `tol`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel, PointClass};
    /// # use nalgebra::Point3;
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// assert_eq!(k.classify(block, Point3::new(10.0, 5.0, 2.5), 1e-6)?, PointClass::Inside);
    /// assert_eq!(k.classify(block, Point3::new(10.0, 5.0, 5.0), 1e-6)?, PointClass::OnBoundary);
    /// assert_eq!(k.classify(block, Point3::new(30.0, 5.0, 2.5), 1e-6)?, PointClass::Outside);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn classify(&self, body: BodyId, point: Point3<f64>, tol: f64) -> Result<PointClass>;
    /// Rounds `edges` (and the edges tangent to them) with a constant `radius`: the rolling-ball
    /// fillet, [`Kernel::fillet_with`] with [`FilletSpec::radius`].
    ///
    /// `InvalidParameter` for a radius that isn't above zero; `TooLarge` for a radius the faces
    /// can't hold (too large for the edge's neighbourhood); another failure
    /// ([`KernelError::is_failure`]) for a blend the kernel can't close. History: each fillet face is generated [`Origin::FromEdge`] of its edge, corner
    /// patches [`Origin::FromVertex`]; the trimmed faces are `modified`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel, Origin};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// // One of the four edges 20 mm long.
    /// let edge = k.edges(block)?.into_iter().find(|e| (e.length - 20.0).abs() < 1e-9).unwrap().id;
    /// let r = k.fillet(block, &[edge], 1.0)?;
    /// // The fillet takes (1 − π/4) r² of the cross section along the edge.
    /// let want = 1000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 20.0;
    /// assert!((k.mass_properties(r.bodies[0])?.volume - want).abs() < 1e-6);
    /// assert!(r.history.generated.iter().any(|(_, o)| matches!(o, Origin::FromEdge { edge: e, .. } if *e == edge)));
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn fillet(&mut self, body: BodyId, edges: &[EdgeId], radius: f64) -> Result<OpResult>;
    /// Bevels `edges` (and the edges tangent to them) by `spec`'s distances measured along each
    /// face ([`Kernel::chamfer_with`] with the default options). Errors and history as
    /// [`Kernel::fillet`]'s: chamfer faces are generated [`Origin::FromEdge`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{ChamferSpec, DefaultKernel, Extent, Kernel};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// let edge = k.edges(block)?.into_iter().find(|e| (e.length - 20.0).abs() < 1e-9).unwrap().id;
    /// let r = k.chamfer(block, &[edge], ChamferSpec::EqualDistance(1.0))?;
    /// // The bevel takes a right triangle of legs 1 along the 20 mm edge: 10.
    /// assert!((k.mass_properties(r.bodies[0])?.volume - 990.0).abs() < 1e-6);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn chamfer(&mut self, body: BodyId, edges: &[EdgeId], spec: ChamferSpec) -> Result<OpResult>;
    /// Hollows a solid inward to walls `thickness` thick, open where the faces `remove` were
    /// ([`Kernel::shell_with`] inward, rounded joins).
    ///
    /// `InvalidParameter` for a thickness that isn't above zero, no face to remove, an unknown
    /// face, or walls so thick they leave no cavity (walls that cross only where the body is
    /// thinner than two of them leave it solid there); a failure ([`KernelError::is_failure`])
    /// when the offset can't be built. History: the faces kept and the inner walls offset from them are
    /// `modified` from their input face; the removed faces are `deleted`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// let top = k.faces(block)?.into_iter().find(|f| (f.center.z - 5.0).abs() < 1e-9).unwrap().id;
    /// let cup = k.shell(block, &[top], 1.0)?.bodies[0];
    /// // The cavity is 18 × 8 × 4.
    /// assert!((k.mass_properties(cup)?.volume - (1000.0 - 18.0 * 8.0 * 4.0)).abs() < 1e-6);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn shell(&mut self, body: BodyId, remove: &[FaceId], thickness: f64) -> Result<OpResult>;
    /// Delete face (Heal): the faces removed and their neighbours extended to close the gap.
    /// Faces that can't be healed over are an error (`Unsupported` or `InvalidResult`), never a
    /// wrong body.
    fn delete_faces(&mut self, body: BodyId, faces: &[FaceId]) -> Result<OpResult>;
    /// Simplify: faces and edges lying on the same surface or curve merged (an imported part's
    /// seams and split faces), the body otherwise unchanged.
    fn simplify(&mut self, body: BodyId) -> Result<OpResult>;
    /// Heal: the body made sound and its geometry upgraded by `ops`, in order ([`HealOp`]),
    /// for an imported or damaged body. Each result face is `modified` from the input faces it
    /// continues (a face split in pieces lists each piece; merged faces list each input);
    /// input faces left in no result face are `deleted`. The result is a body `Body::check`
    /// passes, or an `InvalidResult` error.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, HealOp, Kernel};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// // A sound body comes through unchanged in shape.
    /// let ops = [HealOp::Fix { precision: 1e-7, max_tolerance: 1e-3 }, HealOp::UpdateTolerance];
    /// let healed = Kernel::heal(&mut k, block, &ops)?.bodies[0];
    /// assert_eq!(k.faces(healed)?.len(), 6);
    /// assert!((k.mass_properties(healed)?.volume - 1000.0).abs() < 1e-9);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn heal(&mut self, body: BodyId, ops: &[HealOp]) -> Result<OpResult>;

    /// The fillet of Onshape's Edge tab (P3.6, PS14.2–14.6): a radius or a constant width, and
    /// whether it may run over onto neighbouring faces. The edges' tangent chains are filleted
    /// too (OpenCascade continues a fillet along tangent edges). The default handles a radius
    /// with overflow allowed.
    fn fillet_with(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        spec: &FilletSpec,
    ) -> Result<OpResult> {
        match (spec.size, spec.allow_overflow) {
            (FilletSize::Radius(r), true) => self.fillet(body, edges, r),
            _ => Err(KernelError::Unsupported("this fillet")),
        }
    }

    /// Onshape's Full round fillet (P3.6, PS14.1): the `center` face is replaced by a round
    /// tangent to the two `side` faces. Built for a flat rectangular centre face between two
    /// parallel flat side faces at least half their distance apart deep (radius = half the
    /// distance); anything else is an `InvalidParameter` error saying so.
    fn full_round(
        &mut self,
        body: BodyId,
        side1: FaceId,
        center: FaceId,
        side2: FaceId,
    ) -> Result<OpResult>;

    /// The chamfer of Onshape's dialog (P3.6, PS14.7–14.8): its type, Offset or Tangent
    /// measurement, and which face takes the first distance (flipped for all edges, or per
    /// edge). The default handles Offset without flips.
    fn chamfer_with(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        opts: &ChamferOpts,
    ) -> Result<OpResult> {
        if opts.measurement == ChamferMeasure::Offset && !opts.flip && opts.flipped.is_empty() {
            self.chamfer(body, edges, opts.spec)
        } else {
            Err(KernelError::Unsupported("this chamfer"))
        }
    }

    /// The shell of Onshape's dialog (P3.6, PS16): faces removed, inward or outward, or a
    /// closed hollow body. A shell whose walls would leave no cavity (or, with sharp corners,
    /// would cross anywhere) is an `InvalidParameter` error, never a wrong body. The default
    /// handles an inward shell.
    fn shell_with(&mut self, body: BodyId, spec: &ShellSpec) -> Result<OpResult> {
        if spec.outward || spec.hollow {
            return Err(KernelError::Unsupported("this shell"));
        }
        self.shell(body, &spec.remove, spec.thickness)
    }

    /// A draft (P3.10, PS4.9): the faces turned by the angle about where they meet the neutral
    /// plane, for the pull direction; the other faces follow. Faces kept or drafted are
    /// `modified`. Only planes, cylinders and cones can be drafted.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, DraftSpec, Extent, Kernel};
    /// # use nalgebra::{Point3, Vector3};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(-10.0, -10.0, 10.0, 10.0), Extent::Blind(10.0))?.bodies[0];
    /// let sides = k.faces(block)?.into_iter().filter(|f| f.plane.is_some_and(|p| p.normal.z.abs() < 1e-9)).map(|f| f.id).collect();
    /// // The four sides leaned in 5° about the bottom (pull +Z): a square frustum.
    /// let angle = 5f64.to_radians();
    /// let spec = DraftSpec { faces: sides, angle, pull: Vector3::z_axis(), neutral: Point3::origin(), neutral_normal: None, tangent_propagation: true };
    /// let drafted = k.draft(block, &spec)?.bodies[0];
    /// let (a, b) = (20.0, 20.0 - 2.0 * 10.0 * angle.tan());
    /// let want = 10.0 / 3.0 * (a * a + b * b + a * b);
    /// assert!((k.mass_properties(drafted)?.volume - want).abs() < 1e-6 * want);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn draft(&mut self, body: BodyId, spec: &DraftSpec) -> Result<OpResult>;

    /// The solid with its faces offset (P3.10, the Boolean feature's Subtract offset): every
    /// face moved outward by `distance`, or by its own distance. Faces are `modified`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel, OffsetSpec};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// // Every face out by 1 with sharp corners: a 22 × 12 × 7 block.
    /// let grown = k.offset(block, &OffsetSpec { distance: 1.0, faces: vec![], sharp: true })?.bodies[0];
    /// assert!((k.mass_properties(grown)?.volume - 22.0 * 12.0 * 7.0).abs() < 1e-6);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn offset(&mut self, body: BodyId, spec: &OffsetSpec) -> Result<OpResult>;

    /// The solid with faces given new surfaces (SMLib's `SmBrep::LocalOperation`, a direct
    /// edit): the edges and vertices round them recomputed where the new surfaces meet their
    /// neighbours, the topology kept, each face keeping the side its material is on. Faces are
    /// `modified`.
    fn replace_faces(&mut self, body: BodyId, faces: &[FaceReplacement]) -> Result<OpResult>;

    /// Move face's Translate and Rotate (Onshape's Move face): the faces moved rigidly by
    /// `motion` (a translation or a rotation, not a reflection) and the faces round them
    /// extended or trimmed to meet them where they now are, the topology kept. Moving faces
    /// along their normals is [`Kernel::offset`] with per-face distances. Faces are `modified`.
    /// A move that would change the topology (a face shrinking to nothing, or running into a
    /// face it doesn't border) is an `InvalidParameter` error, never a wrong body.
    fn move_faces(&mut self, body: BodyId, faces: &[FaceId], motion: &Motion) -> Result<OpResult> {
        let _ = (body, faces, motion);
        Err(KernelError::Unsupported(
            "Move face by a translation or rotation",
        ))
    }

    /// A fillet whose radius varies along each edge (P3.10, PS14.6 Variable fillet). Every edge
    /// tangent to a listed edge must be listed too (OCCT continues a fillet along them). With
    /// `allow_overflow` off it is refused where it runs onto another face, as `fillet_with`.
    fn fillet_variable(
        &mut self,
        body: BodyId,
        laws: &[FilletLaw],
        allow_overflow: bool,
    ) -> Result<OpResult>;

    /// A partial fillet (P3.11, PS14.6): `spec`'s fillet on `edge` only between the fractions
    /// `from < to` of its length (in the direction the edge runs, 0 at its start). The fillet
    /// stops at a flat end face square to the edge at each bound, as Onshape's does; at a bound
    /// of 0 or 1 it ends as the whole edge's fillet would. The fillet's face is generated
    /// `FromEdge`, the end faces `StartCap`/`EndCap` with region 0.
    fn fillet_partial(
        &mut self,
        body: BodyId,
        edge: EdgeId,
        spec: &FilletSpec,
        from: f64,
        to: f64,
    ) -> Result<OpResult>;

    /// Onshape's *Smooth fillet corners* (Final, PS14.6): the fillet of `edges` as
    /// [`Kernel::fillet_with`] (a constant-radius circular one), then at every vertex of `body`
    /// where three or more filleted edges meet, the corner set back: everything of the fillet
    /// within `ρ = √(setback² + r²)` of the vertex (the fillets' ends and the default corner
    /// patch; the contact lines on the faces end `setback` from the vertex) is replaced by one
    /// N-sided patch meeting every face around it tangentially (G1). The patch is generated
    /// `FromVertex`. A corner the kernel can't fill is an error saying so (`InvalidGeometry`, or
    /// `InvalidResult` for a patch that doesn't close).
    fn fillet_smooth(
        &mut self,
        body: BodyId,
        edges: &[EdgeId],
        spec: &FilletSpec,
        setback: f64,
    ) -> Result<OpResult>;

    /// The outward unit normals of `face` at the points of it nearest `points` (exact, from
    /// its surface; Final: to check that faces meet tangentially).
    fn face_normals_at(
        &self,
        body: BodyId,
        face: FaceId,
        points: &[Point3<f64>],
    ) -> Result<Vec<Vector3<f64>>>;

    /// Splits a body into one body per solid (a boolean can leave several pieces; Onshape makes
    /// each a part). Each result's history has every face `modified` from the same face of
    /// `body`. A body with one solid, or a surface body, comes back as one copy.
    fn split_solids(&mut self, body: BodyId) -> Result<Vec<OpResult>>;

    /// [`Kernel::split_solids`] for an imported file's shape, which may also hold loose shells
    /// (CAD files often write a part's solids as bare shells): each closed loose shell becomes a
    /// solid of its own and each open one a surface body, as Onshape's import makes them.
    fn split_imported(&mut self, body: BodyId) -> Result<Vec<OpResult>> {
        self.split_solids(body)
    }

    /// Gathers copies of `bodies` into one body without merging them (a compound: P3H.6's
    /// Composite part). Its faces are the inputs' faces in order (the first body's, then the
    /// next's); the history has each face `modified` from its input face. Volume and area are
    /// the sums.
    fn compound(&mut self, bodies: &[BodyId]) -> Result<OpResult>;

    /// Reads a STEP file (the Import feature): one body per solid in the file, in the reader's
    /// order (lengths converted to mm). A file without solids gives its shells or faces as one
    /// surface body.
    fn import_step(&mut self, bytes: &[u8]) -> Result<Vec<BodyId>>;

    /// A solid bounded by a closed triangle mesh (an STL file's triangles, mm): the triangles
    /// become planar faces, sewn within `tol` into a closed shell and made solid.
    fn mesh_solid(&mut self, triangles: &[[Point3<f64>; 3]], tol: f64) -> Result<BodyId>;

    /// Thickens surfaces and faces into a solid (Onshape's Thicken).
    fn thicken_surfaces(&mut self, spec: &ThickenSpec) -> Result<OpResult>;

    /// A surface filling a closed chain of curves (Onshape's Fill).
    fn fill(&mut self, spec: &FillSpec) -> Result<OpResult>;

    /// Sews surface bodies that meet along their edges into one solid, if they close it.
    fn sew_solid(&mut self, bodies: &[BodyId], tol: f64) -> Result<OpResult>;

    /// Forgets a body (for example when a rebuild cache evicts a feature's output). Unknown ids
    /// are ignored. Ids are never reused, so a released id stays unknown.
    fn release(&mut self, body: BodyId);

    /// A body as bytes (the backend's own exact format), which [`Self::read_body`] gives back
    /// with the same faces, edges and vertices in the same order: names indexed by them stay
    /// valid (cadrs caches built bodies, and they may travel between peers).
    fn write_body(&self, body: BodyId) -> Result<Vec<u8>>;

    /// Reads a body written by [`Self::write_body`] into this session.
    fn read_body(&mut self, bytes: &[u8]) -> Result<BodyId>;

    // Body kinds (Phase 1b: Onshape's solid, sheet, wire and point bodies).

    /// What a body is: solids, sheets, wires, points, a mix of them (a compound), or nothing.
    fn body_class(&self, body: BodyId) -> Result<BodyClass>;

    /// A wire body: the curves (edges of bodies, sketch curves, Bézier curves in space) joined
    /// end to end into one chain, open or closed. Its edges have no faces in
    /// [`Kernel::edges`]; its mass properties give its `length`.
    fn create_wire(&mut self, curves: &[PathCurve]) -> Result<OpResult>;

    /// A point body: acorn points (vertices without edges), in order.
    fn create_points(&mut self, points: &[Point3<f64>]) -> Result<OpResult>;

    /// A body swept one kind higher along a vector or about an axis (Onshape's extrude and
    /// revolve of points, curves and surfaces): points into a wire, a wire into a sheet, a sheet
    /// into a solid. The input's vertices and edges keep their ids in the result; side faces are
    /// generated `FromEdge` of the edge of `body` that swept them, a sheet's start and end
    /// copies `StartCap`/`EndCap { region: 0 }`. A sheet that doesn't cross the sweep the same
    /// way everywhere (its solid would fold through itself) is an `InvalidParameter` error.
    fn sweep_body(&mut self, body: BodyId, sweep: &BodySweep) -> Result<OpResult>;

    /// A cylindrical box (SMLib's `CreateCylindricalBox`): an annular sector extruded along the
    /// frame's normal. Its faces are generated as a revolve's (region 0). Refuses
    /// (`InvalidParameter`) a length or inner radius that isn't above zero, an outer radius not
    /// above the inner, a start angle outside [0, 2π), and an end angle not after the start or
    /// more than a turn past it.
    fn cylindrical_box(&mut self, spec: &CylindricalBox) -> Result<OpResult>;

    // Queries.

    /// The body's faces in id order: [`FaceId`] `i` is the `i`-th face, with its surface kind,
    /// plane (planar faces), area, centroid, and axis and radius (surfaces of revolution).
    fn faces(&self, body: BodyId) -> Result<Vec<FaceInfo>>;

    /// The body's edges in id order, with the faces on either side, exact length, ends,
    /// tangents and (circular edges) the exact circle. A wire body's edges have no faces.
    fn edges(&self, body: BodyId) -> Result<Vec<EdgeInfo>>;

    /// Volume, area, centroid and inertia (unit density) from the exact geometry: analytic
    /// faces integrated in closed form or by Gauss quadrature to near machine precision, not
    /// from a mesh. A sheet has zero volume; a wire body gives its `length`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// let m = k.mass_properties(block)?;
    /// assert!((m.volume - 1000.0).abs() < 1e-9);
    /// assert!((m.surface_area - 700.0).abs() < 1e-9);
    /// assert!((m.center_of_mass - nalgebra::Point3::new(10.0, 5.0, 2.5)).norm() < 1e-9);
    /// // Ixx = V (b² + c²) / 12 about the centroid.
    /// assert!((m.inertia[(0, 0)] - 1000.0 * (100.0 + 25.0) / 12.0).abs() < 1e-6);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn mass_properties(&self, body: BodyId) -> Result<MassProperties>;

    /// A triangle mesh of the body for display and export: within `quality.deflection` mm of
    /// the exact surfaces and `quality.angle` between neighbouring normals, each triangle
    /// tagged with its face, plus edge polylines and a point body's points. `InvalidInput` for
    /// a deflection or angle that isn't finite and above zero. Deterministic: the same body
    /// gives the same mesh whatever the thread count.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel, Tessellation};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// let mesh = k.tessellate(block, Tessellation::default())?;
    /// assert_eq!(mesh.indices.len(), mesh.triangle_faces.len());
    /// assert_eq!(mesh.edges.len(), 12);
    /// let area: f64 = mesh.indices.iter().map(|t| {
    ///     let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
    ///     (b - a).cross(&(c - a)).norm() / 2.0
    /// }).sum();
    /// assert!((area - 700.0).abs() < 1e-9); // flat faces mesh exactly
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn tessellate(&self, body: BodyId, quality: Tessellation) -> Result<TriMesh>;

    /// The number of solids in a body (0 for a surface body).
    fn solid_count(&self, body: BodyId) -> Result<usize>;

    /// Whether two solid bodies touch or overlap: their union is one solid. Only asks the
    /// question (the automatic merge scope of a new body), so a backend can skip the work a
    /// real union does to make a clean result.
    fn joins(&mut self, a: BodyId, b: BodyId) -> Result<bool> {
        let r = self.boolean(BoolOp::Union, a, &[b])?;
        let joined = self.solid_count(r.bodies[0]).unwrap_or(2) == 1;
        self.release(r.bodies[0]);
        Ok(joined)
    }

    /// Whether two solid bodies overlap with volume: their intersection has some. Only asks the
    /// question (the parts a new body's Remove cuts), like [`Kernel::joins`].
    fn overlaps(&mut self, a: BodyId, b: BodyId) -> Result<bool> {
        let r = self.boolean(BoolOp::Intersect, a, &[b])?;
        let v = self
            .mass_properties(r.bodies[0])
            .map(|m| m.volume)
            .unwrap_or(0.0);
        self.release(r.bodies[0]);
        Ok(v > tol::ROUNDOFF)
    }

    /// A tight axis-aligned bounding box of the body's exact geometry.
    fn bounding_box(&self, body: BodyId) -> Result<Aabb>;

    /// Where the line through `origin` along the unit `dir` crosses the body's faces, sorted by
    /// `t` (hits behind the origin have negative `t`). Up to next uses it to find what lies
    /// ahead of a profile.
    fn ray_hits(&self, body: BodyId, origin: Point3<f64>, dir: Vector3<f64>)
    -> Result<Vec<RayHit>>;
    /// Writes bodies as OpenCASCADE text BREP (geometry only, one compound of their solids,
    /// sheets, wires and points), as `BRepTools::Read` and DRAW's `restore` read it. The
    /// native kernel writes exact geometry and its tolerances; a backend without a writer
    /// returns [`KernelError::Unsupported`].
    fn export_brep(&self, _bodies: &[BodyId]) -> Result<Vec<u8>> {
        Err(KernelError::Unsupported("OpenCASCADE BREP export"))
    }
    /// Imported surface colors, in face order; absent entries keep the application's palette.
    /// Empty when the body has none or the backend keeps no colors. The native kernel keeps
    /// them per body and passes them on to the faces an operation's history says continue
    /// colored faces (moves, copies, compounds, splits, booleans, local operations).
    fn face_colors(&self, _body: BodyId) -> Vec<Option<[u8; 4]>> {
        Vec::new()
    }
    /// Sets a body's surface colors, in face order (see [`Kernel::face_colors`]). Ignored when
    /// the backend keeps no colors.
    fn set_face_colors(&mut self, _body: BodyId, _colors: Vec<Option<[u8; 4]>>) {}
    /// Writes bodies to a STEP AP214 file (exact B-rep: analytic and B-spline geometry, one
    /// `MANIFOLD_SOLID_BREP` per solid), named "Body 1", "Body 2", ... in order.
    /// [`Kernel::import_step`] reads it back with the same geometry within the file's
    /// tolerance. Use [`Kernel::export_model`] for names, IGES or assemblies.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel};
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// let step = k.export_step(&[block])?;
    /// assert!(step.starts_with(b"ISO-10303-21;"));
    /// let back = k.import_step(&step)?;
    /// assert_eq!(back.len(), 1);
    /// assert_eq!(k.faces(back[0])?.len(), 6);
    /// assert!((k.mass_properties(back[0])?.volume - 1000.0).abs() < 1e-9);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn export_step(&self, bodies: &[BodyId]) -> Result<Vec<u8>>;

    /// Reads a STEP or IGES file (P3F.2, [`exchange`]): each distinct part once as a body, with
    /// its occurrences (placements and names) as the file's assembly structure gives them.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cadrs_kernel::{DefaultKernel, Extent, Kernel};
    /// # use cadrs_kernel::exchange::ExchangeFormat;
    /// # use cadrs_kernel::{Curve2, Loop, Plane, Profile, Region};
    /// # fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
    /// #     let c = [nalgebra::Point2::new(x0, y0), nalgebra::Point2::new(x1, y0), nalgebra::Point2::new(x1, y1), nalgebra::Point2::new(x0, y1)];
    /// #     let curves = (0..4).map(|i| Curve2::Line { a: c[i], b: c[(i + 1) % 4], source: None }).collect();
    /// #     Profile::new(Plane::top(), vec![Region { outer: Loop { curves }, holes: vec![], source: None }])
    /// # }
    /// let mut k = DefaultKernel::new();
    /// let block = k.extrude(&rect(0.0, 0.0, 20.0, 10.0), Extent::Blind(5.0))?.bodies[0];
    /// // Written as STEP and read back: the same solid.
    /// let bytes = k.export_model(ExchangeFormat::Step, "model", &[(block, "block".into())], &[])?;
    /// let m = k.import_model(ExchangeFormat::Step, &bytes)?;
    /// assert_eq!(m.parts.len(), 1);
    /// assert!((k.mass_properties(m.parts[0].body)?.volume - 1000.0).abs() < 1e-6);
    /// # Ok::<(), cadrs_kernel::KernelError>(())
    /// ```
    fn import_model(
        &mut self,
        format: exchange::ExchangeFormat,
        bytes: &[u8],
    ) -> Result<exchange::ImportedModel>;

    /// Writes `parts` (bodies with names) to STEP or IGES (P3F.2). With `instances`, the file
    /// holds one assembly called `name` with the parts at those placements.
    fn export_model(
        &self,
        format: exchange::ExchangeFormat,
        name: &str,
        parts: &[(BodyId, String)],
        instances: &[exchange::ExportInstance],
    ) -> Result<Vec<u8>>;

    /// Hidden-line removal for a drawing view (P3C.2): `bodies` seen along `frame`, as 2D edges
    /// sorted into visible and hidden, sharp, smooth (tangent) and outline, each with the body
    /// edge or face it came from (see [`projection`]). The bodies hide each other.
    fn project(
        &self,
        bodies: &[BodyId],
        frame: &ViewFrame,
        opts: &ProjectOptions,
    ) -> Result<Projection>;

    /// The silhouette curves of every face of `body` seen along a direction or from an eye
    /// point: where the face's normal is square to the line of sight (SMLib's
    /// `CreateSilhouetteCurves` clipped to the face), hidden or not, as model-space polylines
    /// within `tolerance` (mm), with their (u, v). A face seen edge-on (wholly on its
    /// silhouette) gives the sides of its uv box that lie in it, as SMLib does.
    fn silhouettes(
        &self,
        body: BodyId,
        view: &SilhouetteView,
        tolerance: f64,
    ) -> Result<Vec<SilhouetteCurve>>;

    /// The minimum distance between two points, edges, faces or bodies (their boundaries), and
    /// the closest points. `EmptyResult` when either side has no geometry.
    fn min_distance(&self, a: &DistanceTarget, b: &DistanceTarget) -> Result<DistanceResult>;

    /// The maximum distance between two points, edges, faces or bodies, and the farthest
    /// points.
    fn max_distance(&self, a: &DistanceTarget, b: &DistanceTarget) -> Result<DistanceResult>;

    /// A sheet body of one face: the surface of `face` over its natural uv domain (a
    /// B-spline's range, a period of a periodic direction) or over `domain`, bounded by
    /// iso-curves, with poles as degenerate edges and seams where the domain closes round
    /// (SMLib's `CreateFaceFromSurface`). The result has no history. `InvalidParameter` for an
    /// unbounded surface without a domain, or a domain outside the surface.
    fn face_from_surface(
        &mut self,
        body: BodyId,
        face: FaceId,
        domain: Option<UvRect>,
    ) -> Result<OpResult>;

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
    /// (within [`TANGENT_ANGLE`]), but for smooth edges (their faces tangent: nothing to
    /// round). In id order.
    fn tangent_chain(&self, body: BodyId, edge: EdgeId) -> Result<Vec<EdgeId>> {
        tangent_chain_of(&self.edges(body)?, &self.vertices(body)?, edge, &|_, _| {
            true
        })
    }
}

/// [`Kernel::tangent_chain`] over a body's edges and vertices (their lengths aren't used: a
/// kernel may leave them out). `joins(a, b)` says whether a blend along edge `a` may run on
/// along tangent edge `b` (a kernel that knows: not from a concave edge onto a convex one).
pub(crate) fn tangent_chain_of(
    edges: &[EdgeInfo],
    vertices: &[VertexInfo],
    edge: EdgeId,
    joins: &dyn Fn(EdgeId, EdgeId) -> bool,
) -> Result<Vec<EdgeId>> {
    let tol = naming::joint_tolerance(edges);
    let get = |id: EdgeId| edges.iter().find(|e| e.id == id);
    if get(edge).is_none() {
        return Err(KernelError::InvalidParameter(format!("unknown {edge:?}")));
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
                if o.smooth {
                    continue;
                }
                let tangent = tangents_at(o, v.point)
                    .iter()
                    .any(|t| mine.iter().any(|m| m.dot(t).abs() >= cos));
                if tangent && joins(cur, other) {
                    chain.push(other);
                    todo.push(other);
                }
            }
        }
    }
    chain.sort_by_key(|e| e.0);
    Ok(chain)
}

/// How far apart (radians) two edges' tangents may be at a shared vertex and still count as
/// tangent-connected: 0.1°.
pub const TANGENT_ANGLE: f64 = 0.1 * std::f64::consts::PI / 180.0;
