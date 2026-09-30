//! The "Mates" stand-in (P3B.3, `intro-to-assemblies.md` A9–A12, A16): the course demonstrates
//! the Pin slot, Planar, Ball, Parallel, Tangent and Width mates on models it doesn't share, so
//! the scenarios use these small parts, built from our own features in one Part Studio, "Mate
//! parts" (mm), each at a spot of its own:
//!
//! | Part | Shape | For |
//! |---|---|---|
//! | Slot Plate | 80 × 50 × 10 at x −60…20, a blind straight slot along X centred at (−20, 0): 40 mm between its end centres, 10 wide, floor at z 4 | Pin slot (A9.2) |
//! | Pin | Ø10 × 16 at (−20, 45), a "D" pin: a flat 3.5 mm off its axis facing +Y (so its turn shows) | Pin slot, Planar + Tangent |
//! | Ramp | profile on Front, 50 wide (y ±25): flat top z 10 from x 100 back to a R15 fillet, then a slope up to (40, 30) | Tangent and its propagation (A11) |
//! | Roller | Ø20 × 40 along Y, axis at (x 70, z 60) | Tangent |
//! | Cam Pin | Ø10 × 16 standing in the Cam Plate's slot at (−20, −80), z 4…20 | Planar + Tangent |
//! | Cam Plate | 80 × 60 × 10 at x −60…20, y −120…−60, an arc slot 10 wide round (−20, −110) at R30 from 30° to 150°, floor at z 4 | Planar + Tangent (A10.1) |
//! | Clevis | base 40 × 50 × 8 at x 120…160, two uprights 10 thick to z 40 with inner faces at y ±15 | Width (A12) |
//! | Tab | 30 × 12 × 30 at x 125…155, y 40…52, z 10…40 | Width, one instance's two tabs |
//! | Left Jaw, Right Jaw | 30 × 8 × 20 at x 165…195, y 40…48 and x 165…195, y 64…72, z 10…30 | Width, one tab on each of two instances |
//! | Socket | 40 × 40 × 20 at x 180…220 with a Ø20 hole 10 deep centred at (200, 0) | Ball (A10.2) |
//! | Ball | sphere R10 at (240, 0, 10), a full revolve of a half disc on Front | Ball |
//! | Table | 100 × 80 × 8 at x 280…380 | Planar, Parallel (A10.1, A10.3) |
//! | Puck | Ø30 × 12 at (300, 60) | Planar |
//! | Magnet | Ø20 × 10 at (350, 60) | Parallel |
//!
//! Every id is fixed, so `fixtures/mates_standin.cadrs` is regenerated exactly
//! (`cadrs_core/tests/assembly_mates.rs`).

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::appearance::Appearance;
use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddRevolve, AddSketch, EditSketch, RenamePart, SetExtrude, SetPartAppearance};
use crate::document::{AxisRef, BooleanOp, Document, ExtrudeFeature, Offset, RevolveFeature, RevolveType};
use crate::ids::{ElementId, FeatureId, PartId};

use super::gear_cover::{DocHistory, Studio};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x3b03_0000_0000_0000_0000_0000_0000_0000 + n)
}

/// The Part Studio "Mate parts".
pub const STUDIO: ElementId = ElementId::from_u128(0x3b03_0000_0000_0000_0000_0000_0000_0101);

pub const SLOT_PLATE: PartId = PartId::new(id(0x12), 0);
pub const PIN: PartId = PartId::new(id(0x22), 0);
pub const RAMP: PartId = PartId::new(id(0x32), 0);
pub const ROLLER: PartId = PartId::new(id(0x42), 0);
pub const CLEVIS: PartId = PartId::new(id(0x52), 0);
pub const TAB: PartId = PartId::new(id(0x62), 0);
pub const JAW: PartId = PartId::new(id(0x72), 0);
pub const RIGHT_JAW: PartId = PartId::new(id(0xd2), 0);
pub const SOCKET: PartId = PartId::new(id(0x82), 0);
pub const BALL: PartId = PartId::new(id(0x92), 0);
pub const TABLE: PartId = PartId::new(id(0xa2), 0);
pub const PUCK: PartId = PartId::new(id(0xb2), 0);
pub const MAGNET: PartId = PartId::new(id(0xc2), 0);
pub const CAM_PLATE: PartId = PartId::new(id(0xe2), 0);
pub const CAM_PIN: PartId = PartId::new(id(0xf2), 0);

/// The parts with their names, in the parts list's order.
pub const PARTS: [(PartId, &str); 15] = [
    (SLOT_PLATE, "Slot Plate"),
    (PIN, "Pin"),
    (RAMP, "Ramp"),
    (ROLLER, "Roller"),
    (CLEVIS, "Clevis"),
    (TAB, "Tab"),
    (JAW, "Left Jaw"),
    (RIGHT_JAW, "Right Jaw"),
    (SOCKET, "Socket"),
    (BALL, "Ball"),
    (TABLE, "Table"),
    (PUCK, "Puck"),
    (MAGNET, "Magnet"),
    (CAM_PLATE, "Cam Plate"),
    (CAM_PIN, "Cam Pin"),
];

/// The slot: its centre, the distance between its end centres, its width and its floor.
pub const SLOT_CENTRE: [f64; 2] = [-20.0, 0.0];
pub const SLOT_LENGTH: f64 = 40.0;
pub const SLOT_WIDTH: f64 = 10.0;
pub const SLOT_FLOOR: f64 = 4.0;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], closed: true, construction: false, label: "Add rectangle" }
}

fn circle(x: f64, y: f64, d: f64) -> SketchOp {
    SketchOp::AddCircle { center: v(x, y), radius: d / 2.0, construction: false }
}

fn sketch(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, plane: PlaneRef, ops: Vec<SketchOp>) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(plane) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::Batch(ops) })?;
    geometry(s, el, sketch)
}

fn geometry(s: &dyn Studio, el: ElementId, sketch: FeatureId) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.document()
        .element(el)
        .and_then(|x| x.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

/// Extrudes the region of `sketch` under `seed`, from `z0` to `z1` along the plane's normal.
#[allow(clippy::too_many_arguments)]
fn extrude(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, g: &cadrs_sketch::Sketch, feature: FeatureId, seed: Vec2, z: (f64, f64), op: BooleanOp, scope: &[PartId]) -> Result<(), CommandError> {
    let regions = super::region_refs(sketch, g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(format!("a region of the mates stand-in is missing ({feature:?})")));
    }
    let (z0, z1) = z;
    let mut x = super::extrude_of(regions, z1 - z0);
    x.op = op;
    x.merge_scope = scope.to_vec();
    if z0 != 0.0 {
        x.start_offset = Some(Offset { value: z0.abs(), expr: format!("{} mm", z0.abs()), flip: z0 < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: x, label: "Extrude".into() })?;
    Ok(())
}

/// Extrudes the region of `sketch` under `seed` symmetric about the sketch plane, `depth` in all.
fn extrude_symmetric(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, g: &cadrs_sketch::Sketch, feature: FeatureId, seed: Vec2, depth: f64) -> Result<(), CommandError> {
    let regions = super::region_refs(sketch, g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(format!("a region of the mates stand-in is missing ({feature:?})")));
    }
    let mut x = super::extrude_of(regions, depth);
    x.symmetric = true;
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: x, label: "Extrude".into() })?;
    Ok(())
}

/// Adds the parts, their names and looks to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New, Remove};
    let top = PlaneRef::Top;
    // Slot Plate, and its slot round a construction line.
    let g = sketch(s, el, id(0x11), top, vec![rect(-60.0, -25.0, 20.0, 25.0)])?;
    extrude(s, el, id(0x11), &g, id(0x12), v(10.0, 20.0), (0.0, 10.0), New, &[])?;
    let (cx, cy) = (SLOT_CENTRE[0], SLOT_CENTRE[1]);
    let h = SLOT_LENGTH / 2.0;
    let g = sketch(
        s,
        el,
        id(0x13),
        top,
        vec![SketchOp::AddPolyline { points: vec![v(cx - h, cy), v(cx + h, cy)], closed: false, construction: true, label: "Add line" }],
    )?;
    let line = g.curves.keys().next().ok_or_else(|| CommandError::Invalid("slot line".into()))?;
    s.run(&EditSketch { element: el, feature: id(0x13), op: SketchOp::Slot { source: line, width: SLOT_WIDTH, equal_to: None, construction: false } })?;
    let g = geometry(s, el, id(0x13))?;
    extrude(s, el, id(0x13), &g, id(0x14), v(cx, cy), (SLOT_FLOOR, 10.0), Remove, &[SLOT_PLATE])?;
    // Pin.
    let g = sketch(
        s,
        el,
        id(0x21),
        top,
        vec![
            circle(cx, 45.0, 10.0),
            SketchOp::AddPolyline { points: vec![v(cx - 7.0, 48.5), v(cx + 7.0, 48.5)], closed: false, construction: false, label: "Add line" },
        ],
    )?;
    extrude(s, el, id(0x21), &g, id(0x22), v(cx, 44.0), (0.0, 16.0), New, &[])?;
    // Ramp and Roller, on Front (sketch x = model X, y = model Z), extruded symmetric along Y.
    let front = PlaneRef::Front;
    let g = sketch(
        s,
        el,
        id(0x31),
        front,
        vec![SketchOp::AddPolyline {
            points: vec![v(40.0, 0.0), v(100.0, 0.0), v(100.0, 10.0), v(70.0, 10.0), v(40.0, 30.0)],
            closed: true,
            construction: false,
            label: "Add line",
        }],
    )?;
    let corner = g.point_at(v(70.0, 10.0), 1e-9).ok_or_else(|| CommandError::Invalid("ramp corner".into()))?;
    s.run(&EditSketch { element: el, feature: id(0x31), op: SketchOp::Fillet { corner, radius: 15.0, equal_to: None } })?;
    let g = geometry(s, el, id(0x31))?;
    extrude_symmetric(s, el, id(0x31), &g, id(0x32), v(90.0, 5.0), 50.0)?;
    let g = sketch(s, el, id(0x41), front, vec![circle(70.0, 60.0, 20.0)])?;
    extrude_symmetric(s, el, id(0x41), &g, id(0x42), v(70.0, 60.0), 40.0)?;
    // Clevis: the base, then the two uprights added.
    let g = sketch(s, el, id(0x51), top, vec![rect(120.0, -25.0, 160.0, 25.0)])?;
    extrude(s, el, id(0x51), &g, id(0x52), v(140.0, 0.0), (0.0, 8.0), New, &[])?;
    let g = sketch(s, el, id(0x53), top, vec![rect(120.0, -25.0, 160.0, -15.0), rect(120.0, 15.0, 160.0, 25.0)])?;
    extrude(s, el, id(0x53), &g, id(0x54), v(140.0, -20.0), (8.0, 40.0), Add, &[CLEVIS])?;
    extrude(s, el, id(0x53), &g, id(0x55), v(140.0, 20.0), (8.0, 40.0), Add, &[CLEVIS])?;
    // Tab and Jaw.
    let g = sketch(s, el, id(0x61), top, vec![rect(125.0, 40.0, 155.0, 52.0)])?;
    extrude(s, el, id(0x61), &g, id(0x62), v(140.0, 46.0), (10.0, 40.0), New, &[])?;
    let g = sketch(s, el, id(0x71), top, vec![rect(165.0, 40.0, 195.0, 48.0)])?;
    extrude(s, el, id(0x71), &g, id(0x72), v(180.0, 44.0), (10.0, 30.0), New, &[])?;
    let g = sketch(s, el, id(0xd1), top, vec![rect(165.0, 64.0, 195.0, 72.0)])?;
    extrude(s, el, id(0xd1), &g, id(0xd2), v(180.0, 68.0), (10.0, 30.0), New, &[])?;
    // Socket with its hole.
    let g = sketch(s, el, id(0x81), top, vec![rect(180.0, -20.0, 220.0, 20.0)])?;
    extrude(s, el, id(0x81), &g, id(0x82), v(185.0, 15.0), (0.0, 20.0), New, &[])?;
    let g = sketch(s, el, id(0x83), top, vec![circle(200.0, 0.0, 20.0)])?;
    extrude(s, el, id(0x83), &g, id(0x84), v(200.0, 0.0), (10.0, 20.0), Remove, &[SOCKET])?;
    // Ball: a half disc on Front (sketch x = model X, y = model Z) turned about its diameter.
    let (bx, bz, r) = (240.0, 10.0, 10.0);
    s.run(&AddSketch { element: el, feature: id(0x91), plane: Some(PlaneRef::Front) })?;
    s.run(&EditSketch {
        element: el,
        feature: id(0x91),
        op: SketchOp::Batch(vec![
            SketchOp::AddPolyline { points: vec![v(bx, bz - r - 5.0), v(bx, bz + r + 5.0)], closed: false, construction: true, label: "Add line" },
            SketchOp::AddArc { center: v(bx, bz), start: v(bx, bz - r), end: v(bx, bz + r), construction: false },
            SketchOp::AddPolyline { points: vec![v(bx, bz + r), v(bx, bz - r)], closed: false, construction: false, label: "Add line" },
        ]),
    })?;
    let g = geometry(s, el, id(0x91))?;
    let axis = g.curves.iter().find(|(_, c)| c.construction).map(|(k, _)| k).ok_or_else(|| CommandError::Invalid("ball axis".into()))?;
    let regions = super::region_refs(id(0x91), &g, &[v(bx + r / 2.0, bz)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("the ball's half disc is missing".into()));
    }
    let revolve = RevolveFeature {
        regions,
        axis: Some(AxisRef::SketchCurve { sketch: id(0x91), curve: axis }),
        kind: RevolveType::Full,
        angle: 360.0,
        angle_expr: "360 deg".into(),
        ..RevolveFeature::default()
    };
    s.run(&AddRevolve { element: el, feature: id(0x92), revolve })?;
    // Table, Puck, Magnet.
    let g = sketch(s, el, id(0xa1), top, vec![rect(280.0, -40.0, 380.0, 40.0)])?;
    extrude(s, el, id(0xa1), &g, id(0xa2), v(330.0, 0.0), (0.0, 8.0), New, &[])?;
    let g = sketch(s, el, id(0xb1), top, vec![circle(300.0, 60.0, 30.0)])?;
    extrude(s, el, id(0xb1), &g, id(0xb2), v(300.0, 60.0), (0.0, 12.0), New, &[])?;
    let g = sketch(s, el, id(0xc1), top, vec![circle(350.0, 60.0, 20.0)])?;
    extrude(s, el, id(0xc1), &g, id(0xc2), v(350.0, 60.0), (0.0, 10.0), New, &[])?;
    // Cam Plate and its arc slot (round a construction arc).
    let g = sketch(s, el, id(0xe1), top, vec![rect(-60.0, -120.0, 20.0, -60.0)])?;
    extrude(s, el, id(0xe1), &g, id(0xe2), v(10.0, -115.0), (0.0, 10.0), New, &[])?;
    let (ax, ay, ar) = (-20.0, -110.0, 30.0);
    let at = |deg: f64| v(ax + ar * deg.to_radians().cos(), ay + ar * deg.to_radians().sin());
    let g = sketch(s, el, id(0xe3), top, vec![SketchOp::AddArc { center: v(ax, ay), start: at(30.0), end: at(150.0), construction: true }])?;
    let arc = g.curves.keys().next().ok_or_else(|| CommandError::Invalid("cam arc".into()))?;
    s.run(&EditSketch { element: el, feature: id(0xe3), op: SketchOp::Slot { source: arc, width: 10.0, equal_to: None, construction: false } })?;
    let g = geometry(s, el, id(0xe3))?;
    extrude(s, el, id(0xe3), &g, id(0xe4), v(ax, ay + ar), (SLOT_FLOOR, 10.0), Remove, &[CAM_PLATE])?;
    let g = sketch(s, el, id(0xf1), top, vec![circle(ax, ay + ar, 10.0)])?;
    extrude(s, el, id(0xf1), &g, id(0xf2), v(ax, ay + ar), (SLOT_FLOOR, SLOT_FLOOR + 16.0), New, &[])?;
    for (part, name) in PARTS {
        s.run(&RenamePart { element: el, part, name: name.into() })?;
    }
    for (parts, a) in [
        (vec![SLOT_PLATE, RAMP, CLEVIS, SOCKET, TABLE, CAM_PLATE], Appearance::rgb(150, 156, 163)),
        (vec![PIN, ROLLER, BALL, CAM_PIN], Appearance::rgb(70, 74, 80)),
        (vec![TAB, JAW, RIGHT_JAW, PUCK, MAGNET], Appearance::rgb(74, 124, 186)),
    ] {
        s.run(&SetPartAppearance { element: el, parts, appearance: Some(a) })?;
    }
    Ok(())
}

/// A new document holding the stand-in: "Mates (stand-in)" with its Part Studio "Mate parts"
/// (mm), as `fixtures/mates_standin.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Mates (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x3b03_0000_0000_0000_0000_0000_0000_0100);
    let mut el = crate::document::Element::part_studio("Mate parts");
    el.id = STUDIO;
    doc.elements.push(el);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h), STUDIO)?;
    Ok(doc)
}

/// The stand-in as a document file (`fixtures/mates_standin.cadrs`).
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
