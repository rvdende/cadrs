//! The "Start an Assembly" stand-in (P3B.1, `intro-to-assemblies.md` A5): Onshape's public
//! "Exercise: Starting an Assembly" document can't be copied, so the exercise starts from this
//! L-bracket, built from our own features, in inch and pound (`intro-to-assemblies-gaps.md`,
//! "Ex1 Start an Assembly"):
//!
//! - **Sketch 1 / Extrude 1**: the base, a 3 × 3 in rectangle on Top (x −1.5..1.5, y 0..3) with a
//!   Ø0.563 in circle at (0, 1.75), extruded 0.5 in: a plate with a through hole (the course's
//!   "centre back hole", Ø0.563 in THRU);
//! - **Sketch 2 / Extrude 2**: the upright, a 3 × 0.5 in rectangle on Top (y 2.5..3) extruded
//!   3 in from a starting offset of 0.5 in, Add (one part).
//!
//! One Part Studio, "Motor Mount", with one part, "DC Motor Mount (stand-in)", in Aluminum -
//! 6061 (2.70 g/cm³). Every id is fixed, so `fixtures/motor_mount_standin.cadrs` is regenerated
//! exactly (`cadrs_core/tests/course_assemblies.rs`).

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddSketch, EditSketch, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, ExtrudeFeature, Offset};
use crate::ids::{ElementId, FeatureId, PartId};

use super::gear_cover::{DocHistory, Studio};

pub const SKETCH_1: FeatureId = FeatureId::from_u128(0x3b01_0000_0000_0000_0000_0000_0000_0001);
pub const EXTRUDE_1: FeatureId = FeatureId::from_u128(0x3b01_0000_0000_0000_0000_0000_0000_0002);
pub const SKETCH_2: FeatureId = FeatureId::from_u128(0x3b01_0000_0000_0000_0000_0000_0000_0003);
pub const EXTRUDE_2: FeatureId = FeatureId::from_u128(0x3b01_0000_0000_0000_0000_0000_0000_0004);
/// The Part Studio "Motor Mount".
pub const STUDIO: ElementId = ElementId::from_u128(0x3b01_0000_0000_0000_0000_0000_0000_0101);
/// The bracket.
pub const PART: PartId = PartId::new(EXTRUDE_1, 0);
pub const PART_NAME: &str = "DC Motor Mount (stand-in)";

/// mm per inch.
pub const IN: f64 = 25.4;
/// Half the width (x), the depth (y) and the thickness of the base (in).
pub const HALF_WIDTH: f64 = 1.5;
pub const DEPTH: f64 = 3.0;
pub const BASE: f64 = 0.5;
/// The upright: y from UPRIGHT_Y to DEPTH, z from BASE to BASE + UPRIGHT_H (in).
pub const UPRIGHT_Y: f64 = 2.5;
pub const UPRIGHT_H: f64 = 3.0;
/// The hole: diameter and centre (in).
pub const HOLE_D: f64 = 0.563;
pub const HOLE_Y: f64 = 1.75;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = |x: f64, y: f64| Vec2::new(x * IN, y * IN);
    SketchOp::AddPolyline {
        points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

/// Adds the bracket's features, its name and its material to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = |x: f64, y: f64| Vec2::new(x * IN, y * IN);
    s.run(&AddSketch { element: el, feature: SKETCH_1, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: SKETCH_1, op: rect(-HALF_WIDTH, 0.0, HALF_WIDTH, DEPTH) })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_1,
        op: SketchOp::AddCircle { center: v(0.0, HOLE_Y), radius: HOLE_D / 2.0 * IN, construction: false },
    })?;
    extrude(s, el, SKETCH_1, EXTRUDE_1, &[v(0.0, 0.5)], |e| {
        e.depth = BASE * IN;
        e.depth_expr = "0.5 in".into();
    })?;
    s.run(&AddSketch { element: el, feature: SKETCH_2, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: SKETCH_2, op: rect(-HALF_WIDTH, UPRIGHT_Y, HALF_WIDTH, DEPTH) })?;
    extrude(s, el, SKETCH_2, EXTRUDE_2, &[v(0.0, (UPRIGHT_Y + DEPTH) / 2.0)], |e| {
        e.op = BooleanOp::Add;
        e.depth = UPRIGHT_H * IN;
        e.depth_expr = "3 in".into();
        e.start_offset = Some(Offset { value: BASE * IN, expr: "0.5 in".into(), flip: false });
    })?;
    s.run(&RenamePart { element: el, part: PART, name: PART_NAME.into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![PART], material: crate::material::library("Aluminum - 6061") })?;
    Ok(())
}

fn extrude(
    s: &mut dyn Studio,
    el: ElementId,
    sketch: FeatureId,
    feature: FeatureId,
    seeds: &[Vec2],
    set: impl FnOnce(&mut ExtrudeFeature),
) -> Result<(), CommandError> {
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sketch, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid("a region of the stand-in is missing".into()));
    }
    let mut e = super::extrude_of(regions, 25.0);
    set(&mut e);
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: e, label: "Extrude".into() })?;
    Ok(())
}

/// A new document holding the stand-in: "Starting an Assembly (stand-in)" with its Part Studio
/// "Motor Mount" (inch, pound), as `fixtures/motor_mount_standin.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Starting an Assembly (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x3b01_0000_0000_0000_0000_0000_0000_0100);
    doc.units = cadrs_sketch::units::Units {
        length: cadrs_sketch::units::LengthUnit::Inch,
        mass: cadrs_sketch::units::MassUnit::Pound,
        ..Default::default()
    };
    let mut el = crate::document::Element::part_studio("Motor Mount");
    el.id = STUDIO;
    doc.elements.push(el);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h), STUDIO)?;
    Ok(doc)
}

/// The stand-in as a document file (`fixtures/motor_mount_standin.cadrs`), dated 2026-09-28.
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
