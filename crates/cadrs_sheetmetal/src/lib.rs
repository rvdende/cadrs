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
//! - [`table`]: the Bends and Other joints tables (SM13).
//! - [`samples`]: small models (an L, a U-channel, an open box, a hem, a tube, …) for tests,
//!   previews and scenarios; [`svg`]: a flat pattern as an SVG picture.
//!
//! Lengths are millimetres and angles radians unless a name says otherwise.

pub mod bend;
pub mod flat;
pub mod model;
pub mod params;
pub mod poly;
pub mod relief;
pub mod samples;
pub mod svg;
pub mod table;

pub use bend::BendValue;
pub use flat::{FlatError, FlatPattern, flatten};
pub use model::{Bend, BuildError, Joint, JointId, JointKind, Model, RipStyle, SharpBuilder, Wall, WallId};
pub use params::{BendCalc, BendRelief, BendReliefKind, CornerRelief, CornerReliefKind, Params};
