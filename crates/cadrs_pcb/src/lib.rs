//! cadrs_pcb: PCB Studio's domain layer (stage 3H). No Bevy.
//!
//! - [`PcbBoard`]: an IDF board and its library, in mm, with a stable [`ItemId`] per component
//!   and keep area (the handle the PCB Studio tab, sync and export use).
//! - [`geometry`] (X5 forward, P3H.2): the board, its keep areas and its components as kernel
//!   bodies ([`board_geometry`] → [`BoardGeometry`], named bodies with a display [`BodyClass`]
//!   and colour).
//! - [`placement`]: the 3D placement of a component on the board and its exact inverse.
//! - [`names`] (PCB5.1, PCB5.4): which MCAD parts are the board, keep-ins and keep-outs.
//! - [`mcad`] (X5 reverse, PCB5.1): MCAD parts (kernel bodies + names) and component instances
//!   back to an IDF board, for "Sync a Part Studio or assembly with PCB Studio".
//! - [`colors`]: the display colours (board green, components by package kind, keep areas dark
//!   translucent).
//! - [`mesh`] (P3H.3): a board's bodies tessellated for the PCB Studio viewport.
//! - [`sync`] (P3H.5): Sync a Part Studio or assembly with PCB Studio — the tab's parts
//!   gathered, the board made on the kernel thread, the command that adds or updates it.
//! - [`create_assembly`] (P3H.6): Create an assembly from this ECAD data — the board's Part
//!   Studio, its components and an Assembly, as one command.
//! - [`component_docs`] (P3H.7): the component documents Create assembly makes (one stored
//!   document per package in the component folder, versioned) and references by version.
//! - [`sample`]: a PCB board built into a Part Studio through the command layer (sketch +
//!   extrude per body), named and coloured, for the scenarios and later Create assembly.

pub mod board;
pub mod colors;
pub mod component_docs;
pub mod create_assembly;
pub mod geometry;
pub mod mcad;
pub mod mesh;
pub mod names;
pub mod placement;
pub mod sample;
pub mod step;
pub mod sync;

pub use board::{ItemId, KeepArea, KeepIds, KeepKind, PcbBoard};
pub use colors::{BodyClass, ComponentKind, Rgba, component_kind};
pub use geometry::{BoardGeometry, PcbBody, board_geometry, idf_loop_to_kernel};
pub use mcad::{McadBoard, McadInstance, McadPart, SyncPlane, board_from_mcad, face_loops, loops_equivalent};
pub use names::{PartRole, Recognised, recognise, role_of};
pub use mesh::{BoardMesh, BodyMesh};
pub use placement::{Pose, placement_motion, pose_from_motion, pose_motion};
