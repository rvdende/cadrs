//! cadrs_cam: computer-aided manufacturing for CAM Studio. No Bevy, no kernel.
//!
//! CAM Studio (`cadrs_core::cam`) follows Onshape's: a **job** names the parts to machine and
//! their stock, runs on a **machine** (a mill, router, laser, plasma table or waterjet) and
//! holds **setups** (a work coordinate system each, one per fixturing) of **tools** and the
//! **toolpaths** that use them. This crate is the machining itself:
//!
//! - [`geom`], [`region`]: points, frames, boxes; 2D areas with offsets and booleans
//!   (Clipper2).
//! - [`model`]: the parts in a setup's coordinates: triangles, exact faces, edges, holes and
//!   sections (the studio fills them from the kernel).
//! - [`tool`], [`machine`]: cutters and beams, the tool library; the machine catalogue.
//! - [`strategy`]: the toolpath patterns (face, roughing, profiles, chamfers, engraving,
//!   flatlands, 3D finishing, drilling, boring; laser, plasma and waterjet cutting, etching and
//!   filling) and their linking.
//! - [`toolpath`]: what the patterns make, its motion, statistics and back-plot timeline.
//! - [`dropcutter`]: the exact drop-cutter the 3D finishing patterns follow the surfaces with.
//! - [`heightmap`], [`sim`]: the in-process stock (a Z-map), gouge and collision checks,
//!   and the kerf simulation of sheet cutting.
//! - [`post`]: G-code for Fanuc, Haas-style, LinuxCNC, GRBL and Mach3 controllers.
//!
//! Lengths are mm, feeds mm/min, angles degrees unless a name says otherwise.

pub mod dropcutter;
pub mod feeds;
pub mod geom;
pub mod heightmap;
pub mod machine;
pub mod model;
pub mod post;
pub mod region;
pub mod sim;
pub mod strategy;
pub mod tool;
pub mod toolpath;
pub mod voxels;

pub use geom::{Aabb, Frame, P2, P3};
pub use machine::{Indexing, Machine, MachineKind};
pub use model::Model;
pub use post::{Dialect, NcOperation, NcProgram, PostSettings, post};
pub use strategy::{Input, Params, Pattern, Stock, Type, generate};
pub use tool::{Tool, ToolKind, ToolLibrary};
pub use toolpath::{Step, Toolpath};
