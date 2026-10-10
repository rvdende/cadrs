//! The Rocket Guidance Reflector stand-in (P3.8, PS27). Onshape's public "Rocket Guidance
//! System" document can't be copied, so the exercise starts from this plate, built from our own
//! features, in Aluminum - 1060, mm and kg:
//!
//! - **Sketch 1 / Extrude 1**: a rounded square on Top, 182 mm across (x, y −91..91), corners
//!   R36, 45 mm up;
//! - **Sketch 2 / Revolve 1**: on Front, the region under an arc of radius 400 (centre on the
//!   Z axis 400 up, so the arc touches the origin) out to x = 140, revolved a full turn about
//!   the Z axis and **removed**: the plate's bottom is a sphere, `z = 400 − √(400² − r²)`, 0 at
//!   the centre rising to 16.5 mm under the corners ("Face of Revolve 1", PS27.3's target).
//!   These four sit in a closed folder, "Reflector Surface Features";
//! - **Pattern Axis**: a Mate connector at the centre of the top face (0, 0, 45), Z up (PS27.2);
//! - **Feature Sketch**, on the top face: two right triangles either side of the diagonal of the
//!   +x/−y quadrant, (28, −7), (72, −7), (72, −51) and (7, −28), (7, −72), (51, −72), and a
//!   13 × 32 rectangle at x 75..88, y −16..16 (PS27.3, PS27.10);
//! - the plate is orange, as the course's.
//!
//! Every id is fixed, so the fixture `fixtures/reflector_standin.cadrs` is regenerated exactly
//! (see `cadrs_core/tests/reflector.rs`).

use cadrs_sketch::{FacePlane, PlaneRef, SketchOp, Vec2};

use super::gear_cover::{DocHistory, Studio};
use crate::command::{CommandError, History};
use crate::commands::{
    AddExtrude, AddFeature, AddRevolve, AddSketch, CreateFolder, EditSketch, RenameFeature, RenamePart, SetExtrude,
    SetPartAppearance, SetPartMaterial,
};
use crate::document::{AxisRef, BooleanOp, Document, ExtrudeFeature, FaceRef, FeatureKind, RevolveFeature, RevolveType};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::mate::{ConnectorOrigin, MateConnectorFeature};

pub const SKETCH_1: FeatureId = FeatureId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0001);
pub const EXTRUDE_1: FeatureId = FeatureId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0002);
pub const SKETCH_2: FeatureId = FeatureId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0003);
pub const REVOLVE_1: FeatureId = FeatureId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0004);
pub const PATTERN_AXIS: FeatureId = FeatureId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0005);
pub const FEATURE_SKETCH: FeatureId = FeatureId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0006);
pub const FOLDER: FeatureId = FeatureId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0007);
/// The plate.
pub const PART: PartId = PartId::new(EXTRUDE_1, 0);

/// Half the width, the corner radius and the height.
pub const HALF: f64 = 91.0;
pub const CORNER_R: f64 = 36.0;
pub const HEIGHT: f64 = 45.0;
/// The bottom sphere's radius (its centre on the Z axis at this height; it touches the origin).
pub const SPHERE_R: f64 = 400.0;
/// How far out the revolved region goes (past the corners, 55√2 + 36 ≈ 113.8 from the axis).
pub const REVOLVE_OUT: f64 = 140.0;
/// The two triangles (sketch = model x, y).
pub const TRIANGLE_A: [[f64; 2]; 3] = [[28.0, -7.0], [72.0, -7.0], [72.0, -51.0]];
pub const TRIANGLE_B: [[f64; 2]; 3] = [[7.0, -28.0], [7.0, -72.0], [51.0, -72.0]];
/// The rectangle: x0, y0, x1, y1.
pub const RECT: [f64; 4] = [75.0, -16.0, 88.0, 16.0];

/// The bottom's height at `r` from the axis.
pub fn bottom(r: f64) -> f64 {
    SPHERE_R - (SPHERE_R * SPHERE_R - r * r).sqrt()
}

fn poly(points: &[[f64; 2]]) -> SketchOp {
    SketchOp::AddPolyline {
        points: points.iter().map(|p| Vec2::new(p[0], p[1])).collect(),
        closed: true,
        construction: false,
        label: "Add polygon",
    }
}

/// Adds the stand-in's base features, its folder, the connector, the Feature Sketch, the part's
/// name and its material to the Part Studio `el` of `doc`.
pub fn build(doc: &mut Document, h: &mut History, el: ElementId) -> Result<(), CommandError> {
    build_in(&mut DocHistory(doc, h), el)
}

/// [`build`] through any [`Studio`].
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = Vec2::new;
    let sketch_of = |s: &dyn Studio, id: FeatureId| {
        s.document()
            .element(el)
            .and_then(|e| e.feature(id))
            .and_then(|f| f.sketch())
            .map(|x| x.geometry.clone())
            .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
    };
    // Sketch 1: the rounded square.
    s.run(&AddSketch { element: el, feature: SKETCH_1, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: SKETCH_1, op: poly(&[[-HALF, -HALF], [HALF, -HALF], [HALF, HALF], [-HALF, HALF]]) })?;
    for corner in [v(-HALF, -HALF), v(HALF, -HALF), v(HALF, HALF), v(-HALF, HALF)] {
        let p = sketch_of(s, SKETCH_1)?.point_at(corner, 1e-9).ok_or_else(|| CommandError::Invalid("corner not found".into()))?;
        s.run(&EditSketch { element: el, feature: SKETCH_1, op: SketchOp::Fillet { corner: p, radius: CORNER_R, equal_to: None } })?;
    }
    let g = sketch_of(s, SKETCH_1)?;
    let regions = super::region_refs(SKETCH_1, &g, &[v(0.0, 0.0)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("the plate's region is missing".into()));
    }
    let e = ExtrudeFeature {
        op: BooleanOp::New,
        depth_expr: "45 mm".into(),
        ..super::extrude_of(regions, HEIGHT)
    };
    s.run(&AddExtrude { element: el, feature: EXTRUDE_1, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: EXTRUDE_1, extrude: e, label: "Extrude".into() })?;
    // Sketch 2 on Front (sketch x = model X, sketch y = model Z): the region under the arc.
    let edge = bottom(REVOLVE_OUT);
    s.run(&AddSketch { element: el, feature: SKETCH_2, plane: Some(PlaneRef::Front) })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_2,
        op: SketchOp::AddPolyline {
            points: vec![v(0.0, 0.0), v(0.0, -10.0), v(REVOLVE_OUT, -10.0), v(REVOLVE_OUT, edge)],
            closed: false,
            construction: false,
            label: "Add line",
        },
    })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_2,
        op: SketchOp::AddArc { center: v(0.0, SPHERE_R), start: v(0.0, 0.0), end: v(REVOLVE_OUT, edge), construction: false },
    })?;
    let g = sketch_of(s, SKETCH_2)?;
    let regions = super::region_refs(SKETCH_2, &g, &[v(REVOLVE_OUT / 2.0, -2.0)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("the revolve's region is missing".into()));
    }
    // The axis: the line along x = 0 (the region's side on the Z axis).
    let axis = g
        .curves
        .iter()
        .find(|(_, c)| match c.kind {
            cadrs_sketch::CurveKind::Line { a, b } => g.pos(a).x.abs() < 1e-9 && g.pos(b).x.abs() < 1e-9,
            _ => false,
        })
        .map(|(id, _)| id)
        .ok_or_else(|| CommandError::Invalid("the revolve axis is missing".into()))?;
    let r = RevolveFeature {
        regions,
        axis: Some(AxisRef::SketchCurve { sketch: SKETCH_2, curve: axis }),
        kind: RevolveType::Full,
        op: BooleanOp::Remove,
        merge_all: true,
        ..RevolveFeature::default()
    };
    s.run(&AddRevolve { element: el, feature: REVOLVE_1, revolve: r })?;
    // The top face (Extrude 1's end cap) for the connector and the Feature Sketch.
    let features = s.document().element(el).map(|e| e.features().to_vec()).unwrap_or_default();
    let top = crate::parts::cap_name(&features, EXTRUDE_1, 0, true).ok_or_else(|| CommandError::Invalid("no top face".into()))?;
    let frame = crate::parts::face_frame(&features, EXTRUDE_1, &top).ok_or_else(|| CommandError::Invalid("no top face frame".into()))?;
    let center = [0.0, 0.0, HEIGHT];
    s.run(&AddFeature {
        element: el,
        feature: PATTERN_AXIS,
        base_name: "Mate connector".into(),
        kind: FeatureKind::MateConnector(MateConnectorFeature {
            origin: Some(ConnectorOrigin::Face(FaceRef { part: PART, face: top, seed: center })),
            ..MateConnectorFeature::default()
        }),
    })?;
    s.run(&RenameFeature { element: el, feature: PATTERN_AXIS, name: "Pattern Axis".into() })?;
    let plane = PlaneRef::Face(FacePlane { feature: EXTRUDE_1.0, face: top, origin: frame.origin, u: frame.u, v: frame.v, seed: Some(center), upright: false });
    s.run(&AddSketch { element: el, feature: FEATURE_SKETCH, plane: Some(plane) })?;
    for op in [poly(&TRIANGLE_A), poly(&TRIANGLE_B), poly(&[[RECT[0], RECT[1]], [RECT[2], RECT[1]], [RECT[2], RECT[3]], [RECT[0], RECT[3]]])] {
        s.run(&EditSketch { element: el, feature: FEATURE_SKETCH, op })?;
    }
    s.run(&RenameFeature { element: el, feature: FEATURE_SKETCH, name: "Feature Sketch".into() })?;
    s.run(&RenamePart { element: el, part: PART, name: "Reflector".into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![PART], material: crate::material::library("Aluminum - 1060") })?;
    // The course's plate is orange (`ex5-step2.png`).
    s.run(&SetPartAppearance { element: el, parts: vec![PART], appearance: Some(crate::appearance::Appearance::rgb(0xd8, 0x86, 0x2a)) })?;
    s.run(&CreateFolder {
        element: el,
        folder: FOLDER,
        name: Some("Reflector Surface Features".into()),
        features: vec![SKETCH_1, EXTRUDE_1, SKETCH_2, REVOLVE_1],
    })?;
    Ok(())
}

/// A new document holding the stand-in: "Rocket Guidance System (stand-in)" with its Part
/// Studio "Reflector" (mm, kg), as `fixtures/reflector_standin.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Rocket Guidance System (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0100);
    let mut el = crate::document::Element::part_studio("Reflector");
    el.id = ElementId::from_u128(0x7e11_ec70_0000_0000_0000_0000_0000_0101);
    let id = el.id;
    doc.elements.push(el);
    let mut h = History::default();
    build(&mut doc, &mut h, id)?;
    Ok(doc)
}

/// The stand-in as a document file (`fixtures/reflector_standin.cadrs`), dated 2026-09-28.
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
