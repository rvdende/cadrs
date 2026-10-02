//! IR5.5's Suppress by variable stand-in: a plate with a hole that a Number variable switches.
//!
//! - **#withHole** = 1 (a Number Variable, first in the list).
//! - **Sketch 1** on Top: a 60 × 40 rectangle (x 0…60, y 0…40); **Extrude 1** New, 10 mm:
//!   the plate, V = 60·40·10 = 24 000 mm³.
//! - **Sketch 2** on Top: a Ø16 circle at (30, 20); **Extrude 2** Remove, 10 mm: the hole
//!   through the plate, π·8²·10 = 2 010.619 mm³ ([`HOLE_VOLUME`]).
//!
//! Extrude 2 gets its suppression variable (`#withHole`) in the scenario and the tests, not
//! here.

use cadrs_sketch::units::Units;
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use super::gear_cover::Studio;
use crate::command::CommandError;
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use crate::document::{BooleanOp, ExtrudeFeature};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::variables::{VariableFeature, VariableType};

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x1e55_0000_0000_0000_0000_0000_0000_0000 + n)
}

pub const WITH_HOLE: FeatureId = fid(1);
pub const PLATE_SKETCH: FeatureId = fid(2);
pub const PLATE: FeatureId = fid(3);
pub const HOLE_SKETCH: FeatureId = fid(4);
pub const HOLE: FeatureId = fid(5);
/// The plate's part (Extrude 1's).
pub const PLATE_PART: PartId = PartId::new(PLATE, 0);

/// The plate without the hole, mm³.
pub const PLATE_VOLUME: f64 = 60.0 * 40.0 * 10.0;
/// The hole, mm³.
pub const HOLE_VOLUME: f64 = std::f64::consts::PI * 64.0 * 10.0;

/// The `#withHole` Variable: a Number, `expr`.
pub fn with_hole(expr: &str) -> VariableFeature {
    let mut v = VariableFeature { name: "withHole".into(), var_type: VariableType::Number, expr: expr.into(), ..VariableFeature::default() };
    let _ = v.evaluate(&Units::default(), &Vec::new());
    v
}

/// Adds the model to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    s.run(&AddFeature::variable(el, WITH_HOLE, with_hole("1")))?;
    for (sketch, extrude, op, shape, seed) in [
        (
            PLATE_SKETCH,
            PLATE,
            BooleanOp::New,
            SketchOp::AddPolyline {
                points: vec![Vec2::new(0.0, 0.0), Vec2::new(60.0, 0.0), Vec2::new(60.0, 40.0), Vec2::new(0.0, 40.0)],
                closed: true,
                construction: false,
                label: "Add rectangle",
            },
            Vec2::new(5.0, 5.0),
        ),
        (HOLE_SKETCH, HOLE, BooleanOp::Remove, SketchOp::AddCircle { center: Vec2::new(30.0, 20.0), radius: 8.0, construction: false }, Vec2::new(30.0, 20.0)),
    ] {
        s.run(&AddSketch { element: el, feature: sketch, plane: Some(PlaneRef::Top) })?;
        s.run(&EditSketch { element: el, feature: sketch, op: shape })?;
        let g = s
            .document()
            .element(el)
            .and_then(|x| x.feature(sketch))
            .and_then(|f| f.sketch())
            .map(|s| s.geometry.clone())
            .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
        let regions = super::region_refs(sketch, &g, &[seed]);
        if regions.len() != 1 {
            return Err(CommandError::Invalid("no region".into()));
        }
        let mut x = super::extrude_of(regions, 10.0);
        x.op = op;
        s.run(&AddExtrude { element: el, feature: extrude, extrude: ExtrudeFeature::default() })?;
        s.run(&SetExtrude { element: el, feature: extrude, extrude: x, label: "Extrude".into() })?;
    }
    Ok(())
}
