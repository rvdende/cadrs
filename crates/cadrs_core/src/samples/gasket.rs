//! The branch-and-merge stand-in (P3E.4, TD12.4–TD12.7, the test drive exercise's step 9):
//! "Carburetor (stand-in)", a small document of three tabs.
//!
//! - **Gasket**: Sketch 1 on Top, a 60 × 40 plate centred on the origin with a Ø20 port in the
//!   middle and two Ø8 bolt holes at x = ±20; **Extrude 1**: 2 mm, New, the part
//!   "CARBURETOR_GASKET". Its face is 60·40 − π(10² + 2·4²) = 2400 − 132π ≈ 1985.31 mm², so
//!   2 mm is ≈ 3970.62 mm³ and the exercise's 1 mm gasket ≈ 1985.31 mm³.
//! - **Manifold**: the same plate outline with the port, 20 mm thick (the part "Manifold").
//! - **Assembly 1**: Manifold <1> fixed at the origin and CARBURETOR_GASKET <1> on top of it
//!   (z 20), both by their fixed part ids, so they resolve in any workspace.
//!
//! Every id is fixed, so tests and scenarios can name them.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddSketch, EditSketch, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{Document, Element, ExtrudeFeature};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};

use super::gear_cover::{DocHistory, Studio};

pub const DOCUMENT: DocumentId = DocumentId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0100);
pub const GASKET: ElementId = ElementId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0101);
pub const MANIFOLD: ElementId = ElementId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0102);
pub const ASSEMBLY: ElementId = ElementId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0103);
pub const GASKET_SKETCH: FeatureId = FeatureId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0001);
pub const GASKET_EXTRUDE: FeatureId = FeatureId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0002);
pub const MANIFOLD_SKETCH: FeatureId = FeatureId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0011);
pub const MANIFOLD_EXTRUDE: FeatureId = FeatureId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0012);
pub const GASKET_PART: PartId = PartId::new(GASKET_EXTRUDE, 0);
pub const MANIFOLD_PART: PartId = PartId::new(MANIFOLD_EXTRUDE, 0);
pub const GASKET_INSTANCE: crate::assembly::InstanceId = crate::assembly::InstanceId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0021);
pub const MANIFOLD_INSTANCE: crate::assembly::InstanceId = crate::assembly::InstanceId::from_u128(0x3e40_0000_0000_0000_0000_0000_0000_0022);

pub const NAME: &str = "Carburetor (stand-in)";
pub const GASKET_NAME: &str = "CARBURETOR_GASKET";

/// mm.
pub const LENGTH: f64 = 60.0;
pub const WIDTH: f64 = 40.0;
pub const PORT_R: f64 = 10.0;
pub const BOLT_R: f64 = 4.0;
pub const BOLT_X: f64 = 20.0;
pub const GASKET_T: f64 = 2.0;
/// The branch's gasket (the exercise's 1 mm).
pub const THIN_GASKET_T: f64 = 1.0;
pub const MANIFOLD_T: f64 = 20.0;

/// The gasket's face area, mm².
pub fn gasket_area() -> f64 {
    LENGTH * WIDTH - std::f64::consts::PI * (PORT_R * PORT_R + 2.0 * BOLT_R * BOLT_R)
}

/// The plate outline (and the port, and the bolt holes when `bolts`) in `el`'s Sketch
/// `sketch`, extruded `depth` mm by `extrude`; the part named `part_name`.
fn plate(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, extrude: FeatureId, bolts: bool, depth: f64, part_name: &str) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(PlaneRef::Top) })?;
    let (hx, hy) = (LENGTH / 2.0, WIDTH / 2.0);
    s.run(&EditSketch {
        element: el,
        feature: sketch,
        op: SketchOp::AddPolyline {
            points: vec![Vec2::new(-hx, -hy), Vec2::new(hx, -hy), Vec2::new(hx, hy), Vec2::new(-hx, hy)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    })?;
    let mut circles = vec![(Vec2::new(0.0, 0.0), PORT_R)];
    if bolts {
        circles.push((Vec2::new(-BOLT_X, 0.0), BOLT_R));
        circles.push((Vec2::new(BOLT_X, 0.0), BOLT_R));
    }
    for (center, radius) in circles {
        s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::AddCircle { center, radius, construction: false } })?;
    }
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|k| k.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("the plate's sketch".into()))?;
    let regions = super::region_refs(sketch, &g, &[Vec2::new(0.0, hy - 5.0)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("the plate's region is missing".into()));
    }
    s.run(&AddExtrude { element: el, feature: extrude, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: extrude, extrude: super::extrude_of(regions, depth), label: "Extrude".into() })?;
    let part = PartId::new(extrude, 0);
    s.run(&RenamePart { element: el, part, name: part_name.into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![part], material: crate::material::library("Aluminum - 6061") })?;
    Ok(())
}

/// The document (see the module docs).
pub fn document() -> Result<Document, CommandError> {
    use crate::assembly::commands::{InsertInstance, SetInstancesFixed};
    use crate::assembly::{Instance, InstanceSource, Pose};
    let mut doc = Document::empty(NAME);
    doc.id = DOCUMENT;
    for (id, name) in [(GASKET, "Gasket"), (MANIFOLD, "Manifold")] {
        let mut el = Element::part_studio(name);
        el.id = id;
        doc.elements.push(el);
    }
    let mut asm = Element::assembly("Assembly 1");
    asm.id = ASSEMBLY;
    doc.elements.push(asm);
    let mut h = History::default();
    let mut s = DocHistory(&mut doc, &mut h);
    plate(&mut s, GASKET, GASKET_SKETCH, GASKET_EXTRUDE, true, GASKET_T, GASKET_NAME)?;
    plate(&mut s, MANIFOLD, MANIFOLD_SKETCH, MANIFOLD_EXTRUDE, false, MANIFOLD_T, "Manifold")?;
    s.run(&InsertInstance { element: ASSEMBLY, instance: Instance::new(MANIFOLD_INSTANCE, InstanceSource::Part { element: MANIFOLD, part: MANIFOLD_PART }, Pose::IDENTITY) })?;
    s.run(&SetInstancesFixed { element: ASSEMBLY, instances: vec![MANIFOLD_INSTANCE], fixed: true })?;
    s.run(&InsertInstance {
        element: ASSEMBLY,
        instance: Instance::new(GASKET_INSTANCE, InstanceSource::Part { element: GASKET, part: GASKET_PART }, Pose::translation([0.0, 0.0, MANIFOLD_T])),
    })?;
    Ok(doc)
}

/// Sets a plate's extrude depth (the exercise's edit of the gasket: 2 → 1 mm).
pub fn set_depth(s: &mut dyn Studio, el: ElementId, extrude: FeatureId, depth: f64) -> Result<(), CommandError> {
    let mut e = s
        .document()
        .element(el)
        .and_then(|e| e.feature(extrude))
        .and_then(|f| f.extrude())
        .cloned()
        .ok_or_else(|| CommandError::Invalid("the extrude is missing".into()))?;
    e.depth = depth;
    e.depth_expr = format!("{depth} mm");
    s.run(&SetExtrude { element: el, feature: extrude, extrude: e, label: "Extrude".into() })
}
