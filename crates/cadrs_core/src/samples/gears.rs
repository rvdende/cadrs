//! Gears (issue #179): a Part Studio of the Gear feature's kinds, laid out three by three.
//!
//! - Back row: a straight bevel gear, a spiral bevel gear, and a spiral bevel gear and pinion
//!   in mesh on shafts at right angles (40 and 16 teeth, their apexes at one point). The
//!   spiral ones are face-milled (Gleason's).
//! - Middle row: a helical gear with a hub and lightening holes (the Gear, then ordinary
//!   sketches and extrudes), a planetary set (a 60-tooth ring, an 18-tooth sun and three
//!   21-tooth planets, in mesh), and a herringbone ring gear.
//! - Front row: a spur gear, a helical gear and a herringbone gear.
//!
//! Every id is fixed, so the document is the same each time it is built.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use crate::document::{BooleanOp, Document, ExtrudeFeature, FeatureKind};
use crate::gear::{GearAxis, GearFeature, GearType, ToothStyle};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::studio::{DocHistory, Studio};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x6ea5_0179_0000_0000_0000_0000_0000_0000 + n)
}

/// The grid's pitch (mm).
pub const PITCH: f64 = 120.0;
/// The helical gear that gets a hub and holes.
pub const SPOKED: FeatureId = id(0x40);

fn mm(v: f64) -> String {
    format!("{} mm", crate::hole::fmt(v))
}

fn deg(v: f64) -> String {
    format!("{} deg", crate::hole::fmt(v))
}

/// A gear with its numbers and the texts they were typed as.
#[allow(clippy::too_many_arguments)]
fn gear(
    gear_type: GearType,
    style: ToothStyle,
    teeth: u32,
    module: f64,
    width: f64,
    bore: f64,
    at: [f64; 3],
    rotation: f64,
) -> GearFeature {
    GearFeature {
        gear_type,
        style,
        teeth,
        teeth_expr: teeth.to_string(),
        module,
        module_expr: mm(module),
        helix_angle: 30.0,
        helix_angle_expr: deg(30.0),
        width,
        width_expr: mm(width),
        bore,
        bore_expr: mm(bore),
        rim: 6.0,
        rim_expr: mm(6.0),
        position: at,
        position_expr: at.map(mm),
        rotation,
        rotation_expr: deg(rotation),
        ..Default::default()
    }
}

/// The gears, in the order they are made, each with its feature id.
// (Row by row, with what each row is said before it.)
#[allow(clippy::vec_init_then_push)]
pub fn gears() -> Vec<(FeatureId, GearFeature)> {
    use GearType::{Bevel, Ring, Spur};
    use ToothStyle::{Helical, Herringbone, Straight};
    let p = PITCH;
    let mut out = Vec::new();
    // Front row.
    out.push((
        id(0x10),
        gear(Spur, Straight, 20, 2.0, 12.0, 8.0, [-p, -p, 0.0], 0.0),
    ));
    out.push((
        id(0x11),
        gear(Spur, Helical, 30, 1.5, 12.0, 10.0, [0.0, -p, 0.0], 0.0),
    ));
    out.push((
        id(0x12),
        gear(Spur, Herringbone, 40, 1.5, 14.0, 12.0, [p, -p, 0.0], 0.0),
    ));
    // Middle row: the gear that gets a hub, the planetary set, the herringbone ring.
    out.push((
        SPOKED,
        gear(Spur, Helical, 60, 1.5, 12.0, 10.0, [-p, 0.0, 0.0], 0.0),
    ));
    out.push((
        id(0x20),
        gear(Ring, Straight, 60, 1.5, 10.0, 0.0, [0.0, 0.0, 0.0], 0.0),
    ));
    out.push((
        id(0x21),
        gear(Spur, Straight, 18, 1.5, 10.0, 8.0, [0.0, 0.0, 0.0], 0.0),
    ));
    // The planets, 29.25 from the sun (1.5 × (18 + 21) / 2), each turned so that a space of it
    // takes the sun's tooth that points at it.
    for (k, n) in [0x22, 0x23, 0x24].into_iter().enumerate() {
        let a = 120.0 * k as f64;
        let (s, c) = a.to_radians().sin_cos();
        let turn = a + 180.0 - 180.0 / 21.0;
        out.push((
            id(n),
            gear(
                Spur,
                Straight,
                21,
                1.5,
                10.0,
                6.0,
                [29.25 * c, 29.25 * s, 0.0],
                turn,
            ),
        ));
    }
    out.push((
        id(0x30),
        gear(Ring, Herringbone, 60, 1.5, 16.0, 0.0, [p, 0.0, 0.0], 0.0),
    ));
    // Back row: the bevel gears.
    out.push((
        id(0x50),
        gear(Bevel, Straight, 12, 4.0, 14.0, 10.0, [-p, p, 0.0], 0.0),
    ));
    out.push((
        id(0x51),
        gear(Bevel, Helical, 24, 2.5, 14.0, 12.0, [0.0, p, 0.0], 0.0),
    ));
    // The pair: 40 and 16 teeth on shafts at right angles, so their pitch angles are
    // atan(40/16) and atan(16/40), and their apexes are one point. The gear's axis is Z, the
    // pinion's X; the pinion meets the gear under its own axis, a space of it over a tooth.
    let (zg, zp) = (40u32, 16u32);
    let pitch_angle = (zg as f64 / zp as f64).atan().to_degrees();
    let mut big = gear(Bevel, Helical, zg, 2.0, 12.0, 30.0, [p, p, 0.0], 0.0);
    big.pitch_angle = pitch_angle;
    big.pitch_angle_expr = deg(pitch_angle);
    let apex = big.apex_height().unwrap_or(0.0);
    let mut pinion = gear(
        Bevel,
        Helical,
        zp,
        2.0,
        12.0,
        6.0,
        [0.0; 3],
        180.0 / zp as f64,
    );
    pinion.pitch_angle = 90.0 - pitch_angle;
    pinion.pitch_angle_expr = deg(90.0 - pitch_angle);
    pinion.left_hand = true;
    pinion.member = crate::gear::BevelMember::Pinion;
    pinion.axis = GearAxis::X;
    let reach = pinion.apex_height().unwrap_or(0.0);
    pinion.position = [p - reach, p, apex];
    pinion.position_expr = pinion.position.map(mm);
    // Face-milled teeth (the default spiral), at Gleason's usual 35°.
    for g in [&mut big, &mut pinion] {
        g.helix_angle = 35.0;
        g.helix_angle_expr = deg(35.0);
    }
    out.push((id(0x52), big));
    out.push((id(0x53), pinion));
    out
}

/// Adds the gears, and the hub and holes of the helical one, to the Part Studio `el` of `doc`.
pub fn build(doc: &mut Document, h: &mut History, el: ElementId) -> Result<(), CommandError> {
    build_in(&mut DocHistory(doc, h), el)
}

/// [`build`] through any [`Studio`].
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    for (feature, g) in gears() {
        s.run(&AddFeature {
            element: el,
            feature,
            base_name: "Gear".into(),
            kind: FeatureKind::Gear(g),
        })?;
    }
    // The helical gear's hub, six lightening holes and its bore through the hub.
    let c = Vec2::new(-PITCH, 0.0);
    let part = PartId::new(SPOKED, 0);
    let circle = |center: Vec2, radius: f64| SketchOp::AddCircle {
        center,
        radius,
        construction: false,
    };
    let holes: Vec<SketchOp> = (0..6)
        .map(|k| {
            let a = (60.0 * k as f64).to_radians();
            circle(Vec2::new(c.x + 26.0 * a.cos(), c.y + 26.0 * a.sin()), 9.0)
        })
        .collect();
    let steps: [(u128, Vec<SketchOp>, BooleanOp, f64); 3] = [
        (0x41, vec![circle(c, 12.0)], BooleanOp::Add, 22.0),
        (0x43, holes, BooleanOp::Remove, 30.0),
        (0x45, vec![circle(c, 5.0)], BooleanOp::Remove, 30.0),
    ];
    for (n, ops, op, depth) in steps {
        let (sketch, extrude) = (id(n), id(n + 1));
        s.run(&AddSketch {
            element: el,
            feature: sketch,
            plane: Some(PlaneRef::Top),
        })?;
        for op in ops {
            s.run(&EditSketch {
                element: el,
                feature: sketch,
                op,
            })?;
        }
        s.run(&AddExtrude {
            element: el,
            feature: extrude,
            extrude: ExtrudeFeature::default(),
        })?;
        s.run(&SetExtrude {
            element: el,
            feature: extrude,
            extrude: ExtrudeFeature {
                sketches: vec![sketch],
                op,
                merge_scope: vec![part],
                merge_scope_set: true,
                depth,
                depth_expr: mm(depth),
                ..Default::default()
            },
            label: "Extrude".into(),
        })?;
    }
    Ok(())
}

/// A new document holding the sample: "Gears" with its Part Studio "Gears", as
/// `fixtures/gears.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Gears");
    doc.id = crate::ids::DocumentId::from_u128(0x6ea5_0179_0000_0000_0000_0000_0000_0100);
    let mut el = crate::document::Element::part_studio("Gears");
    el.id = ElementId::from_u128(0x6ea5_0179_0000_0000_0000_0000_0000_0101);
    let id = el.id;
    doc.elements.push(el);
    let mut h = History::default();
    build(&mut doc, &mut h, id)?;
    Ok(doc)
}

/// The sample as a document file (`fixtures/gears.cadrs`), dated 2026-11-12.
pub fn file(document: Document) -> crate::store::DocumentFile {
    let now = 1_794_441_600;
    crate::store::DocumentFile {
        version: crate::store::SCHEMA_VERSION,
        meta: crate::library::DocumentMeta::new("cadrs", now),
        document,
    }
}
