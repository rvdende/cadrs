//! Gears (issue #179): a Part Studio of the Gear feature's kinds, laid out three by three.
//!
//! - Back row: a straight bevel gear, a spiral bevel gear, and a spiral bevel gear and pinion
//!   in mesh on shafts at right angles (40 and 16 teeth, their apexes at one point). The
//!   spiral ones are face-milled (Gleason's).
//! - Middle row: a helical gear with a hub and lightening holes (the Gear, then ordinary
//!   sketches and extrudes), a planetary set (a 60-tooth ring, an 18-tooth sun and three
//!   21-tooth planets, in mesh), and a herringbone ring gear.
//! - Front row: a spur gear, a helical gear and a herringbone gear.
//! - Behind the back row: a face-milled spiral bevel gear and pinion (24 and 18 teeth) whose
//!   shaft angle is the variable `#shaft` (60°). Their pitch angles follow from it and their
//!   teeth, both are placed by their apex at one point, and the pinion's axis is tilted by
//!   `#shaft − 90°` about it, so any shaft angle from about 30° to 120° keeps them in mesh;
//!   the pinion is cut with localised contact for that shaft angle.
//!
//! Every id is fixed, so the document is the same each time it is built.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use crate::document::{BooleanOp, Document, ExtrudeFeature, FeatureKind};
use crate::gear::{GearAxis, GearFeature, GearType, ToothStyle};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::studio::{DocHistory, Studio};
use crate::variables::{VariableFeature, VariableType};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x6ea5_0179_0000_0000_0000_0000_0000_0000 + n)
}

/// The grid's pitch (mm).
pub const PITCH: f64 = 120.0;
/// The helical gear that gets a hub and holes.
pub const SPOKED: FeatureId = id(0x40);
/// The spiral bevel pair at right angles: [gear, pinion].
pub const PAIR: [FeatureId; 2] = [id(0x52), id(0x53)];
/// The spiral bevel pair whose shaft angle is `#shaft`: [gear, pinion].
pub const SHAFT_PAIR: [FeatureId; 2] = [id(0x60), id(0x61)];
/// `#shaft`'s value in the sample (degrees).
pub const SHAFT: f64 = 60.0;
/// The shaft-angle pair's teeth: [gear, pinion].
pub const SHAFT_TEETH: [u32; 2] = [24, 18];
/// Where the shaft-angle pair's apexes are.
pub const SHAFT_APEX: [f64; 3] = [0.0, 2.0 * PITCH, 30.0];

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
    out.push((PAIR[0], big));
    out.push((PAIR[1], pinion));
    let [g, p] = shaft_pair(SHAFT);
    out.push((SHAFT_PAIR[0], g));
    out.push((SHAFT_PAIR[1], p));
    out
}

/// The variables the shaft-angle pair is drawn with, `#shaft` at `shaft` degrees: the shaft
/// angle, the two numbers of teeth, and the gear's pitch angle that follows from them
/// (`tan δ₂ = sin Σ / (z₁/z₂ + cos Σ)`; the pinion's is Σ − δ₂).
pub fn shaft_variables(shaft: f64) -> Vec<(FeatureId, VariableFeature)> {
    let [zg, zp] = SHAFT_TEETH;
    let var = |t, name: &str, expr: String, description: &str| VariableFeature {
        description: description.into(),
        ..VariableFeature::typed(t, name, &expr)
    };
    let mut pitch = var(
        VariableType::Angle,
        "gear_pitch",
        "atan2(sin(#shaft), #pinion_teeth / #gear_teeth + cos(#shaft))".into(),
        "The bevel gear's pitch angle (the pinion's is #shaft less it)",
    );
    pitch.value = gear_pitch(shaft);
    vec![
        (
            id(0x01),
            var(
                VariableType::Angle,
                "shaft",
                deg(shaft),
                "The angle between the bevel pair's shafts",
            ),
        ),
        (
            id(0x02),
            var(
                VariableType::Number,
                "gear_teeth",
                zg.to_string(),
                "The bevel gear's teeth",
            ),
        ),
        (
            id(0x03),
            var(
                VariableType::Number,
                "pinion_teeth",
                zp.to_string(),
                "The bevel pinion's teeth",
            ),
        ),
        (id(0x04), pitch),
    ]
}

/// The shaft-angle pair's gear pitch angle at a shaft angle of `shaft` (degrees).
fn gear_pitch(shaft: f64) -> f64 {
    let [zg, zp] = SHAFT_TEETH;
    let s = shaft.to_radians();
    s.sin().atan2(zp as f64 / zg as f64 + s.cos()).to_degrees()
}

/// The face-milled spiral bevel gear and pinion on shafts at `shaft` degrees, as the sample's
/// variables make them at that value: [gear, pinion]. Both are placed by their apex, at
/// [`SHAFT_APEX`]; the gear's axis is Z, the pinion's X tilted by `shaft − 90°` about the
/// apex (so its axis makes the shaft angle with the gear's), and it meets the gear under its
/// own axis, a space of it over a tooth.
pub fn shaft_pair(shaft: f64) -> [GearFeature; 2] {
    use GearType::Bevel;
    use ToothStyle::Helical;
    let [zg, zp] = SHAFT_TEETH;
    let delta = gear_pitch(shaft);
    let mut big = gear(Bevel, Helical, zg, 2.5, 10.0, 16.0, SHAFT_APEX, 0.0);
    big.teeth_expr = "#gear_teeth".into();
    big.pitch_angle = delta;
    big.pitch_angle_expr = "#gear_pitch".into();
    // A space of the pinion under its axis (its frame's y is Z), where the gear's tooth is.
    let turn = 270.0 - 180.0 / zp as f64;
    let mut pinion = gear(Bevel, Helical, zp, 2.5, 10.0, 10.0, SHAFT_APEX, turn);
    pinion.teeth_expr = "#pinion_teeth".into();
    pinion.rotation_expr = "270 deg - 180 deg / #pinion_teeth".into();
    pinion.pitch_angle = shaft - delta;
    pinion.pitch_angle_expr = "#shaft - #gear_pitch".into();
    pinion.left_hand = true;
    pinion.member = crate::gear::BevelMember::Pinion;
    pinion.axis = GearAxis::X;
    pinion.shaft_angle = shaft;
    pinion.shaft_angle_expr = "#shaft".into();
    pinion.tilt = shaft - 90.0;
    pinion.tilt_expr = "#shaft - 90 deg".into();
    for g in [&mut big, &mut pinion] {
        g.at_apex = true;
        g.helix_angle = 35.0;
        g.helix_angle_expr = deg(35.0);
    }
    [big, pinion]
}

/// The hypoid pair of `hypoid.sv`: 24 and 48 teeth, 2 mm at the mean point, shafts square and
/// 25 mm apart, a 25° spiral, a face mill of 75.6122 mm with its pressure shift found by
/// balancing the flanks nearest undercut (8.8549°), 0.05 mm of backlash and 0.2 mm of end
/// relief: [gear, pinion], both laid out about the origin, the pitch plane the x–y plane.
pub fn hypoid_pair() -> [GearFeature; 2] {
    let common = |teeth: u32, mate: u32, member, left_hand| GearFeature {
        gear_type: GearType::Hypoid,
        teeth,
        teeth_expr: teeth.to_string(),
        mate_teeth: mate,
        mate_teeth_expr: mate.to_string(),
        module: 2.0,
        module_expr: mm(2.0),
        style: ToothStyle::Helical,
        helix_angle: 25.0,
        helix_angle_expr: deg(25.0),
        left_hand,
        shaft_angle: 90.0,
        shaft_angle_expr: deg(90.0),
        offset: 25.0,
        offset_expr: mm(25.0),
        cutter_radius: 75.6122,
        cutter_radius_expr: mm(75.6122),
        pressure_shift: 8.854_877_139_886_24,
        pressure_shift_expr: deg(8.854_877_139_886_24),
        backlash: 0.05,
        backlash_expr: mm(0.05),
        end_relief: 0.2,
        end_relief_expr: mm(0.2),
        member,
        ..Default::default()
    };
    [
        common(48, 24, crate::gear::BevelMember::Gear, true),
        common(24, 48, crate::gear::BevelMember::Pinion, false),
    ]
}

/// Adds the gears, and the hub and holes of the helical one, to the Part Studio `el` of `doc`.
pub fn build(doc: &mut Document, h: &mut History, el: ElementId) -> Result<(), CommandError> {
    build_in(&mut DocHistory(doc, h), el)
}

/// [`build`] through any [`Studio`].
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    for (feature, v) in shaft_variables(SHAFT) {
        s.run(&AddFeature::variable(el, feature, v))?;
    }
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

/// A worm drive: a single-start worm of module 2, diameter quotient 10, 50 long, and its
/// 30-tooth wheel 12 wide, the wheel's axis Z through `at` and the worm's along X, 40 mm to its
/// side (the centre distance): [wheel, worm].
pub fn worm_drive(at: [f64; 3]) -> [GearFeature; 2] {
    let common = |teeth: u32, mate: u32, member, bore: f64| GearFeature {
        gear_type: GearType::Worm,
        teeth,
        teeth_expr: teeth.to_string(),
        mate_teeth: mate,
        mate_teeth_expr: mate.to_string(),
        module: 2.0,
        module_expr: mm(2.0),
        width: 12.0,
        width_expr: mm(12.0),
        length: 50.0,
        length_expr: mm(50.0),
        bore,
        bore_expr: mm(bore),
        backlash: 0.05,
        backlash_expr: mm(0.05),
        position: at,
        position_expr: at.map(mm),
        member,
        ..Default::default()
    };
    [
        common(30, 1, crate::gear::BevelMember::Gear, 12.0),
        common(1, 30, crate::gear::BevelMember::Pinion, 8.0),
    ]
}

/// The hypoid pair's features.
pub const HYPOID_PAIR: [FeatureId; 2] = [id(0x70), id(0x71)];

/// Adds `hypoid.sv`'s pair ([`hypoid_pair`]) to the Part Studio `el`.
pub fn build_hypoid_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    for (feature, g) in HYPOID_PAIR.into_iter().zip(hypoid_pair()) {
        s.run(&AddFeature {
            element: el,
            feature,
            base_name: "Gear".into(),
            kind: FeatureKind::Gear(g),
        })?;
    }
    Ok(())
}

/// The worm drive's features: [wheel, worm].
pub const WORM_DRIVE: [FeatureId; 2] = [id(0x80), id(0x81)];

/// Adds the worm drive ([`worm_drive`], about the origin) to the Part Studio `el`.
pub fn build_worm_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    for (feature, g) in WORM_DRIVE.into_iter().zip(worm_drive([0.0; 3])) {
        s.run(&AddFeature {
            element: el,
            feature,
            base_name: "Gear".into(),
            kind: FeatureKind::Gear(g),
        })?;
    }
    Ok(())
}

/// A new document holding the sample: "Gears" with its Part Studios "Gears", "Hypoid" (the
/// pair of `hypoid.sv`) and "Worm drive", as `fixtures/gears.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Gears");
    doc.id = crate::ids::DocumentId::from_u128(0x6ea5_0179_0000_0000_0000_0000_0000_0100);
    let mut el = crate::document::Element::part_studio("Gears");
    el.id = ElementId::from_u128(0x6ea5_0179_0000_0000_0000_0000_0000_0101);
    let id = el.id;
    doc.elements.push(el);
    let mut hypoid = crate::document::Element::part_studio("Hypoid");
    hypoid.id = ElementId::from_u128(0x6ea5_0179_0000_0000_0000_0000_0000_0102);
    let hypoid_id = hypoid.id;
    doc.elements.push(hypoid);
    let mut worm = crate::document::Element::part_studio("Worm drive");
    worm.id = ElementId::from_u128(0x6ea5_0179_0000_0000_0000_0000_0000_0103);
    let worm_id = worm.id;
    doc.elements.push(worm);
    let mut h = History::default();
    build(&mut doc, &mut h, id)?;
    build_hypoid_in(&mut DocHistory(&mut doc, &mut h), hypoid_id)?;
    build_worm_in(&mut DocHistory(&mut doc, &mut h), worm_id)?;
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
