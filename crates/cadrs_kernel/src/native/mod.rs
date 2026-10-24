//! The native (pure Rust) B-rep kernel: exact curves and surfaces, boundary representation,
//! booleans, features, tessellation and exchange, with no C or C++ dependency.
//!
//! The stable surface (the README's "Curated API") is what this module re-exports: the session
//! [`NativeKernel`]; the B-rep data model ([`Body`] with its [`Vertex`], [`Edge`], [`Coedge`],
//! [`Face`], [`Shell`], [`Solid`] and [`Wire`], checked at a [`CheckLevel`]); its geometry
//! ([`Curve`], [`Surface`] and its derivatives [`D2`], [`NurbsCurve`], [`NurbsSurface`]) and
//! vector types ([`P3`], [`V3`], [`Frame`], [`Affine`]); the tolerance model [`tol`]; and the
//! exact body format [`bodyio`]. That is what a direct-edit or analysis tool built on the
//! kernel needs.
//!
//! Every other module is the kernel's internals: private to the crate, and public only with the
//! `testkit` feature, which the crate's own tests and tools turn on (they reach the internals
//! through `cadrs_kernel::testkit`, or through `native::<module>` while they move there).

pub mod bodyio;
pub mod tol;

// The kernel's internal modules: `pub(crate)`, or `pub` with the `testkit` feature.
// Without it, what only the test kit reaches (the DRAW harness's commands: the OCCT BRep and
// SMLib readers, 2D offsets, tails, faces from edges, ...) is unused; with it (the crate's
// tests and clippy gate) dead code is found as before.
// Written out (not through a macro) so that rustfmt and other tools see the modules.
#[cfg(feature = "testkit")]
pub mod axes;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod axes;
#[cfg(feature = "testkit")]
pub mod blend;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod blend;
#[cfg(feature = "testkit")]
pub mod blend_chain;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod blend_chain;
#[cfg(feature = "testkit")]
pub mod blend_corner;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod blend_corner;
#[cfg(feature = "testkit")]
pub mod boolean;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod boolean;
#[cfg(feature = "testkit")]
pub mod boolean_kinds;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod boolean_kinds;
#[cfg(feature = "testkit")]
pub mod brep_occt;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod brep_occt;
#[cfg(feature = "testkit")]
pub mod brep_write;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod brep_write;
#[cfg(feature = "testkit")]
pub mod build;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod build;
#[cfg(feature = "testkit")]
pub mod bvh;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod bvh;
#[cfg(feature = "testkit")]
pub mod cells;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod cells;
#[cfg(feature = "testkit")]
pub mod check_faces;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod check_faces;
#[cfg(feature = "testkit")]
pub mod classify;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod classify;
#[cfg(feature = "testkit")]
pub mod curve;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod curve;
#[cfg(feature = "testkit")]
pub mod defeature;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod defeature;
#[cfg(feature = "testkit")]
pub mod distance;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod distance;
#[cfg(feature = "testkit")]
pub mod edge_bop;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod edge_bop;
#[cfg(feature = "testkit")]
pub mod edges_faces;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod edges_faces;
#[cfg(feature = "testkit")]
pub mod evolved;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod evolved;
#[cfg(feature = "testkit")]
pub mod exchange;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod exchange;
#[cfg(feature = "testkit")]
pub mod face_build;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod face_build;
#[cfg(feature = "testkit")]
pub mod features;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod features;
#[cfg(feature = "testkit")]
pub mod fillet;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod fillet;
#[cfg(feature = "testkit")]
pub mod fillet_geom;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod fillet_geom;
#[cfg(feature = "testkit")]
pub mod heal;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod heal;
#[cfg(feature = "testkit")]
pub mod heal_split;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod heal_split;
#[cfg(feature = "testkit")]
pub mod hlr;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod hlr;
#[cfg(feature = "testkit")]
pub mod iges;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod iges;
#[cfg(feature = "testkit")]
pub mod kernel;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod kernel;
#[cfg(feature = "testkit")]
pub mod local;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod local;
#[cfg(feature = "testkit")]
pub mod loft_build;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod loft_build;
#[cfg(feature = "testkit")]
pub mod mass;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod mass;
#[cfg(feature = "testkit")]
pub mod math;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod math;
#[cfg(feature = "testkit")]
pub mod meshsolid;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod meshsolid;
pub mod move_sweep;
#[cfg(feature = "testkit")]
pub mod nurbs;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod nurbs;
#[cfg(feature = "testkit")]
pub mod offset;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod offset;
#[cfg(feature = "testkit")]
pub mod ops;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod ops;
#[cfg(feature = "testkit")]
pub mod pcurve;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod pcurve;
#[cfg(feature = "testkit")]
pub mod profile;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod profile;
#[cfg(feature = "testkit")]
pub mod recognize;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod recognize;
#[cfg(feature = "testkit")]
pub mod silhouette;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod silhouette;
#[cfg(feature = "testkit")]
pub mod smb;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod smb;
#[cfg(feature = "testkit")]
pub mod split;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod split;
#[cfg(feature = "testkit")]
pub mod split_area;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod split_area;
#[cfg(feature = "testkit")]
pub mod ssi;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod ssi;
#[cfg(feature = "testkit")]
pub mod ssi_cci;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod ssi_cci;
#[cfg(feature = "testkit")]
pub mod ssi_coincident;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod ssi_coincident;
#[cfg(feature = "testkit")]
pub mod ssi_numeric;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod ssi_numeric;
#[cfg(feature = "testkit")]
pub mod step;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod step;
#[cfg(feature = "testkit")]
pub mod surface;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod surface;
#[cfg(feature = "testkit")]
pub mod surfacing;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod surfacing;
#[cfg(feature = "testkit")]
pub mod sweep;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod sweep;
#[cfg(feature = "testkit")]
pub mod tails;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod tails;
#[cfg(feature = "testkit")]
pub mod tess;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod tess;
#[cfg(feature = "testkit")]
pub mod topo;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod topo;
#[cfg(feature = "testkit")]
pub mod unify;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod unify;
#[cfg(feature = "testkit")]
pub mod uvdomain;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod uvdomain;
#[cfg(feature = "testkit")]
pub mod xbuild;
#[cfg(not(feature = "testkit"))]
#[allow(dead_code, reason = "items only the test kit uses")]
pub(crate) mod xbuild;

/// The DRAW test harness the generated OCCT suites run on (a test kit, not kernel API): built
/// for tests and with the `testkit` feature.
#[cfg(any(test, feature = "testkit"))]
pub mod draw;
/// Bodies for the SMLib test oracle (test kit, as `draw`).
#[cfg(any(test, feature = "testkit"))]
pub mod oracle;

pub use boolean::take_invalid_results;
pub use curve::Curve;
pub use kernel::NativeKernel;
pub use math::{Affine, Frame, P3, V3};
pub use nurbs::{NurbsCurve, NurbsSurface};
pub use surface::{D2, Surface};
pub use topo::{Body, BodyBuilder, CheckLevel, Coedge, Edge, Face, Shell, Solid, Vertex, Wire};
