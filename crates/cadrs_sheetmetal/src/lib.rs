//! Sheet metal for cadrs (P3I; `reference/onshape/sheetmetal/simultaneous-sheet-metal.md`). No
//! Bevy and no kernel here: the definition of a sheet metal model and the maths of its flat
//! pattern, testable on their own.
//!
//! - [`params`]: the Sheet metal model's General, Material and Relief settings, with Onshape's
//!   defaults and ranges (SM2.5–SM2.7).
//! - [`bend`]: K factor, bend allowance, bend deduction and setbacks (SM2.6).
//! - [`model`]: walls and joints (bends, rips, tangent joints), relief overrides, and
//!   [`model::SharpBuilder`], which turns walls meeting at virtual sharps into real walls trimmed
//!   to their bends and rips.
//! - [`flat`]: the flat pattern: walls unfolded about their bends, bend lines with Up/Down,
//!   corner and bend reliefs, the outline, and the collision and bend-loop checks (SM1.5).
//! - [`relief`]: the relief cut shapes (SM7, SM8).
//! - [`sharp_edit`]: the features after a Sheet metal model that edit its definition (P3I.4, SM1.6):
//!   Flange, Hem and Make joint, on a [`sharp_edit::SharpDef`] kept with the model.
//! - [`table`]: the Bends and Other joints tables (SM13).
//! - [`edit`]: joint edits (Modify joint, the table) and the recipe a model is rebuilt from;
//!   [`view`]: meshes, picking and label spots for the table and flat view panel (P3I.3).
//! - [`loft`]: the Sheet metal Loft's tessellated walls and mitred folded solid (P3I.9, SM19.2);
//!   [`forms`]: where forms may go and how the flat shows them (P3I.9, SM20).
//! - [`samples`]: small models (an L, a U-channel, an open box, a hem, a tube, …) for tests,
//!   previews and scenarios; [`svg`]: a flat pattern as an SVG picture.
//!
//! Lengths are millimetres and angles radians unless a name says otherwise.

pub mod bend;
pub mod construct;
pub mod joint_edit;
pub mod model_edit;
pub mod sharp_edit;
pub mod flat;
pub mod forms;
pub mod loft;
pub mod model;
pub mod params;
pub mod poly;
pub mod relief;
pub mod samples;
pub mod svg;
pub mod table;
pub mod view;

pub use bend::BendValue;
pub use flat::{FlatError, FlatPattern, flatten};
pub use model::{Bend, BuildError, Joint, JointId, JointKind, Model, RipStyle, SharpBuilder, Wall, WallId};
pub use params::{BendCalc, BendRelief, BendReliefKind, CornerRelief, CornerReliefKind, Params};
