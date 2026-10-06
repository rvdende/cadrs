//! A two-part STEP assembly for the import scenarios and tests (P3F.2): two drawing brackets
//! ([`super::drawing_bracket`]) facing each other 200 mm apart along Y, and a Ø24 × 240 shaft
//! through their upright holes. The file holds 2 parts ("Bracket", "Shaft") and 3 instances
//! ("Bracket <1>", "Bracket <2>", "Shaft <1>") in an assembly called "Bracket pair".

use std::sync::Arc;

use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

use super::drawing_bracket;
use super::gear_cover::DocHistory;
use crate::assembly::Pose;
use crate::command::History;
use crate::document::{Document, Element, Feature, FeatureKind, SketchFeature};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::rebuild::Rebuilder;
use crate::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};

/// The shaft: a Ø24 circle on Top extruded 240 mm (along Z; the assembly turns it along Y).
pub fn shaft() -> Vec<Feature> {
    let mut g = Sketch::new();
    SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 12.0, construction: false }.apply(&mut g).expect("a circle");
    let sketch = Feature {
        id: FeatureId::from_u128(0x5a_f700_0000_0000_0000_0000_0000_0001),
        name: "Sketch 1".into(),
        kind: FeatureKind::Sketch(SketchFeature { plane: Some(PlaneRef::Top), disable_imprinting: false, geometry: g.clone() }),
        suppress_by: None,
    };
    let extrude = Feature {
        id: FeatureId::from_u128(0x5a_f700_0000_0000_0000_0000_0000_0002),
        name: "Extrude 1".into(),
        kind: FeatureKind::Extrude(super::extrude_of(super::region_refs(sketch.id, &g, &[Vec2::new(0.0, 0.0)]), 240.0)),
        suppress_by: None,
    };
    vec![sketch, extrude]
}

/// The bracket's features.
pub fn bracket() -> Vec<Feature> {
    let mut doc = Document::empty("Bracket");
    let el = Element::part_studio("Bracket");
    let id = el.id;
    doc.elements.push(el);
    let mut history = History::default();
    drawing_bracket::build_in(&mut DocHistory(&mut doc, &mut history), id).expect("the bracket builds");
    doc.element(id).expect("the studio").active_features()
}

/// The three placements: a bracket at the origin, one turned half round about Z at y = 200,
/// and the shaft along +Y through the upright holes (at z = 48), from y = −20.
pub fn placements() -> [Pose; 3] {
    let turned = Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], std::f64::consts::PI).then(&Pose::translation([0.0, 200.0, 0.0]));
    let shaft = Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], -std::f64::consts::FRAC_PI_2).then(&Pose::translation([0.0, -20.0, 48.0]));
    [Pose::IDENTITY, turned, shaft]
}

/// The STEP file (built with `r`, the rebuild session).
pub fn step(r: &mut Rebuilder) -> Result<Vec<u8>, String> {
    let b = Arc::new(bracket());
    let s = Arc::new(shaft());
    let bp = PartId::new(drawing_bracket::EXTRUDE_1, 0);
    let sp = PartId::new(s[1].id, 0);
    let (be, se) = (ElementId::from_u128(0xb7ac), ElementId::from_u128(0x5af7));
    let [p0, p1, p2] = placements();
    let item = |features: &Arc<Vec<Feature>>, part, name: &str, pose, source, source_name: &str| ExportItem {
        features: features.clone(),
        part,
        name: name.into(),
        pose: Some(pose),
        source,
        source_name: source_name.into(),
    };
    let items = vec![
        item(&b, bp, "Bracket <1>", p0, (be, bp), "Bracket"),
        item(&b, bp, "Bracket <2>", p1, (be, bp), "Bracket"),
        item(&s, sp, "Shaft <1>", p2, (se, sp), "Shaft"),
    ];
    let mut req = ExportRequest::new(ModelFormat::Step, "Bracket pair", items);
    req.assembly = true;
    let files = r.export_files(&req)?;
    files.into_iter().next().map(|f| f.bytes).ok_or_else(|| "no file".into())
}
