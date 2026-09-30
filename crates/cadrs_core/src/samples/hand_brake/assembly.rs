//! The Hydraulic Brake Unit stand-in (P3C.5, D14, `ex3-drawing.png`, `ex3-step7.png`): the
//! assembly of the Hand Brake exercise, with the course's 14 instances and 10 BOM rows.
//!
//! Every part is built at its assembled place (in mm, the Handle studio's axes: the plate in the
//! Front plane, 8 thick towards −Y; the pivot is the Ø25.4 hole at (225, −23), along Y), so
//! most instances sit at the identity:
//!
//! | Item | Name | Qty | Stand-in |
//! |---|---|---|---|
//! | 1 | Master Cylinder | 1 | "Master Cylinder" studio: a Ø32 × 80 body along X beyond the arm's end, a Ø8 push rod and a Ø18 reservoir on top |
//! | 2 | Handle Grip | 1 | the Handle studio's grip |
//! | 3 | Handle Plate | 1 | the Handle studio's plate |
//! | 4 | Spacer | 2 | "Hardware" studio: a Ø20/Ø8.5 × 5 ring each side of the plate at the pivot |
//! | 5 | Hex flange bolt small ISO 4162 | 1 | "Hardware" studio: an M8 flange bolt through the pivot (plain part: P3B.5 has no ISO 4162) |
//! | 6 | Hex thin nut grade A & B ISO 4035 | 1 | standard content ISO 4035 M8 |
//! | 7 | Plain washer normal grade A ISO 7089 | 2 | standard content ISO 7089 size 8 |
//! | 8 | Hex socket head cap screw ISO 4762 | 3 | standard content ISO 4762 M5 × 16 (→ M6 in D14.7) |
//! | 9 | Enclosure Lower | 1 | "Enclosures" studio: a U channel round the pivot |
//! | 10 | Enclosure Upper | 1 | "Enclosures" studio: the plate that holds the master cylinder |
//!
//! The standard content parts carry the course's names as their Name property (Onshape names
//! standard content by its component and standard, without the size: `ex3-step7.png`'s
//! Component "Hex socket head cap screw ISO 4762"; the size is in the Description). The M6 cap
//! screw configuration is in the document with that name too (it was inserted, named, then
//! edited to M5), so D14.7's Edit standard content instance → M6 keeps the row's name while its
//! Description and the view change. The BOM shows Item, Name and Quantity, as the course's.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::appearance::Appearance;
use crate::assembly::bom::{BomColumn, SetBomSettings};
use crate::assembly::commands::{InsertInstance, SetInstancesFixed};
use crate::assembly::mate::MateId;
use crate::assembly::standard::{EditStandardContent, InsertStandardContent, StandardPart, StandardSpec, StdInsert};
use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::command::CommandError;
use crate::commands::{AddElement, AddExtrude, AddSketch, EditSketch, NewElementKind, RenamePart, SetExtrude, SetPartAppearance};
use crate::document::{BooleanOp, ExtrudeFeature, Offset};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};
use crate::samples::gear_cover::Studio;

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x4a2d_b2a7_3000_0000_0000_0000_0000_0000 | n)
}

const fn inst(n: u128) -> InstanceId {
    InstanceId::from_u128(0x4a2d_b2a7_4000_0000_0000_0000_0000_0000 | n)
}

pub const ASSEMBLY: ElementId = ElementId::from_u128(0x4a2d_b2a7_0000_0000_0000_0000_0000_0103);
pub const MASTER_STUDIO: ElementId = ElementId::from_u128(0x4a2d_b2a7_0000_0000_0000_0000_0000_0104);
pub const ENCLOSURES: ElementId = ElementId::from_u128(0x4a2d_b2a7_0000_0000_0000_0000_0000_0105);
pub const HARDWARE: ElementId = ElementId::from_u128(0x4a2d_b2a7_0000_0000_0000_0000_0000_0106);
pub const ASSEMBLY_NAME: &str = "Hydraulic Brake Unit";

const MC_E: FeatureId = fid(0x12);
const ROD_E: FeatureId = fid(0x14);
const RES_E: FeatureId = fid(0x16);
const LOWER_E: FeatureId = fid(0x22);
const UPPER_E: FeatureId = fid(0x24);
const SPACER_E: FeatureId = fid(0x32);
const FLANGE_E: FeatureId = fid(0x42);
const HEX_E: FeatureId = fid(0x44);
const SHANK_E: FeatureId = fid(0x46);

pub const MASTER_CYLINDER: PartId = PartId::new(MC_E, 0);
pub const ENCLOSURE_LOWER: PartId = PartId::new(LOWER_E, 0);
pub const ENCLOSURE_UPPER: PartId = PartId::new(UPPER_E, 0);
pub const SPACER: PartId = PartId::new(SPACER_E, 0);
pub const BOLT: PartId = PartId::new(FLANGE_E, 0);

/// The instances, in the Instances list's order (`ex3-step7.png`).
pub const I_MASTER: InstanceId = inst(1);
pub const I_GRIP: InstanceId = inst(2);
pub const I_PLATE: InstanceId = inst(3);
pub const I_SPACERS: [InstanceId; 2] = [inst(4), inst(5)];
pub const I_BOLT: InstanceId = inst(6);
pub const I_NUT: InstanceId = inst(7);
pub const I_WASHERS: [InstanceId; 2] = [inst(8), inst(9)];
/// The three ISO 4762 cap screws (D14.7 edits them).
pub const CAP_SCREWS: [InstanceId; 3] = [inst(10), inst(11), inst(12)];
pub const I_LOWER: InstanceId = inst(13);
pub const I_UPPER: InstanceId = inst(14);

/// The course's BOM (`ex3-drawing.png`): Name, Quantity.
pub const COURSE_BOM: [(&str, u32); 10] = [
    ("Master Cylinder", 1),
    ("Handle Grip", 1),
    ("Handle Plate", 1),
    ("Spacer", 2),
    ("Hex flange bolt small ISO 4162", 1),
    ("Hex thin nut grade A & B ISO 4035", 1),
    ("Plain washer normal grade A ISO 7089", 2),
    ("Hex socket head cap screw ISO 4762", 3),
    ("Enclosure Lower", 1),
    ("Enclosure Upper", 1),
];

/// The pivot (the Ø25.4 hole's centre, x and z) before the course's edits.
pub const PIVOT: (f64, f64) = (225.0, -23.0);
/// The master cylinder's axis (y, z).
pub const MC_AXIS: (f64, f64) = (-4.0, -73.0);
/// The cap screws' size before D14.7 and after.
pub const SCREW_BEFORE: &str = "M5";
pub const SCREW_AFTER: &str = "M6";

fn iso(category: &str, class: &str, component: &str, size: &str, length: Option<f64>) -> StandardSpec {
    let mut s = StandardSpec::new("ISO", category, class, component).expect("in the library");
    s.size = size.into();
    s.length = length;
    s.normalize();
    s
}

/// The cap screws' configuration: ISO 4762 socket head cap screw `size` × 16.
pub fn screw_spec(size: &str) -> StandardSpec {
    iso("Bolts & screws", "Socket head screws", "Socket head cap screw", size, Some(16.0))
}

pub fn nut_spec() -> StandardSpec {
    iso("Nuts", "Hex nuts", "Hex thin nut", "M8", None)
}

pub fn washer_spec() -> StandardSpec {
    iso("Washers", "Plain washers", "Plain washer", "8", None)
}

/// Which plane a sketch is on: its 2D axes are (X, Y) on Top, (X, Z) on Front and (Y, Z) on
/// Right; extrudes run along +Z, −Y and +X.
#[derive(Clone, Copy)]
enum On {
    Top,
    Front,
    Right,
}

/// A sketch of `ops` and an extrude of the regions at `seeds` over `range` along the plane's
/// axis (Z on Top, Y on Front, X on Right; `range` is (low, high) in that axis).
#[allow(clippy::too_many_arguments)]
fn feature(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, extrude: FeatureId, on: On, ops: Vec<SketchOp>, seeds: &[Vec2], range: (f64, f64), op: BooleanOp, scope: &[PartId]) -> Result<(), CommandError> {
    let plane = match on {
        On::Top => PlaneRef::Top,
        On::Front => PlaneRef::Front,
        On::Right => PlaneRef::Right,
    };
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(plane) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::Batch(ops) })?;
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = crate::samples::region_refs(sketch, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid("a region of the Hydraulic Brake Unit stand-in is missing".into()));
    }
    let (lo, hi) = range;
    // Where the extrude starts along the plane's normal, and whether against it.
    let (start, depth) = match on {
        // Front's normal is −Y: start at the high end.
        On::Front => (-hi, hi - lo),
        _ => (lo, hi - lo),
    };
    let mut x = crate::samples::extrude_of(regions, depth);
    x.depth_expr = format!("{} mm", (depth * 1000.0).round() / 1000.0);
    x.op = op;
    x.merge_scope = scope.to_vec();
    if start.abs() > 1e-12 {
        x.start_offset = Some(Offset { value: start.abs(), expr: format!("{} mm", (start.abs() * 1000.0).round() / 1000.0), flip: start < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature: extrude, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: extrude, extrude: x, label: "Extrude".into() })?;
    Ok(())
}

fn circle(x: f64, y: f64, d: f64) -> SketchOp {
    SketchOp::AddCircle { center: Vec2::new(x, y), radius: d / 2.0, construction: false }
}

fn poly(points: &[(f64, f64)]) -> SketchOp {
    SketchOp::AddPolyline { points: points.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed: true, construction: false, label: "Add polygon" }
}

fn name_part(s: &mut dyn Studio, owner: PropertyOwner, name: &str) -> Result<(), CommandError> {
    s.run(&SetProperties { owners: vec![owner], values: vec![(PropertyKey::Name, PropertyValue::Text(name.into()))], label: "Name".into() })
}

/// Adds the studios "Master Cylinder", "Enclosures" and "Hardware" after `after`, and the
/// assembly "Hydraulic Brake Unit" of them and the Handle studio `handle`'s parts.
pub fn build_in(s: &mut dyn Studio, handle: ElementId, after: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New};
    let v = Vec2::new;
    s.run(&AddElement { id: ASSEMBLY, kind: NewElementKind::Assembly, name: Some(ASSEMBLY_NAME.into()), after: Some(after) })?;
    s.run(&AddElement { id: MASTER_STUDIO, kind: NewElementKind::PartStudio, name: Some("Master Cylinder".into()), after: Some(ASSEMBLY) })?;
    s.run(&AddElement { id: ENCLOSURES, kind: NewElementKind::PartStudio, name: Some("Enclosures".into()), after: Some(MASTER_STUDIO) })?;
    s.run(&AddElement { id: HARDWARE, kind: NewElementKind::PartStudio, name: Some("Hardware".into()), after: Some(ENCLOSURES) })?;
    // The master cylinder: body, push rod and reservoir.
    let (my, mz) = MC_AXIS;
    feature(s, MASTER_STUDIO, fid(0x11), MC_E, On::Right, vec![circle(my, mz, 32.0)], &[v(my, mz)], (300.0, 380.0), New, &[])?;
    feature(s, MASTER_STUDIO, fid(0x13), ROD_E, On::Right, vec![circle(my, mz, 8.0)], &[v(my, mz)], (288.0, 300.0), Add, &[MASTER_CYLINDER])?;
    feature(s, MASTER_STUDIO, fid(0x15), RES_E, On::Top, vec![circle(350.0, my, 18.0)], &[v(350.0, my)], (mz, mz + 30.0), Add, &[MASTER_CYLINDER])?;
    s.run(&RenamePart { element: MASTER_STUDIO, part: MASTER_CYLINDER, name: COURSE_BOM[0].0.into() })?;
    s.run(&SetPartAppearance { element: MASTER_STUDIO, parts: vec![MASTER_CYLINDER], appearance: Some(Appearance::rgb(140, 182, 226)) })?;
    // The enclosures: a U channel round the pivot, and the plate the cylinder goes through.
    let u = poly(&[(8.0, -10.0), (8.0, -95.0), (-16.0, -95.0), (-16.0, -10.0), (-13.0, -10.0), (-13.0, -92.0), (5.0, -92.0), (5.0, -10.0)]);
    feature(s, ENCLOSURES, fid(0x21), LOWER_E, On::Right, vec![u], &[v(6.5, -50.0)], (195.0, 255.0), New, &[])?;
    let plate = poly(&[(-26.0, -100.0), (18.0, -100.0), (18.0, -50.0), (-26.0, -50.0)]);
    feature(s, ENCLOSURES, fid(0x23), UPPER_E, On::Right, vec![plate, circle(my, mz, 33.0)], &[v(-22.0, -96.0)], (290.0, 296.0), New, &[])?;
    s.run(&RenamePart { element: ENCLOSURES, part: ENCLOSURE_LOWER, name: COURSE_BOM[8].0.into() })?;
    s.run(&RenamePart { element: ENCLOSURES, part: ENCLOSURE_UPPER, name: COURSE_BOM[9].0.into() })?;
    s.run(&SetPartAppearance { element: ENCLOSURES, parts: vec![ENCLOSURE_LOWER, ENCLOSURE_UPPER], appearance: Some(Appearance::rgb(226, 112, 28)) })?;
    // Hardware: the spacer (y −5…0; its instances move it to either side of the plate) and the
    // M8 flange bolt through the pivot, its flange on the outer washer.
    let (px, pz) = PIVOT;
    feature(s, HARDWARE, fid(0x31), SPACER_E, On::Front, vec![circle(px, pz, 20.0), circle(px, pz, 8.5)], &[v(px + 7.0, pz)], (-5.0, 0.0), New, &[])?;
    feature(s, HARDWARE, fid(0x41), FLANGE_E, On::Front, vec![circle(px, pz, 17.0)], &[v(px, pz)], (9.6, 11.6), New, &[])?;
    let r = 13.0 / 3f64.sqrt();
    let hex: Vec<(f64, f64)> = (0..6).map(|k| {
        let a = std::f64::consts::FRAC_PI_3 * k as f64;
        (px + r * a.cos(), pz + r * a.sin())
    }).collect();
    feature(s, HARDWARE, fid(0x43), HEX_E, On::Front, vec![poly(&hex)], &[v(px, pz)], (11.6, 17.6), Add, &[BOLT])?;
    feature(s, HARDWARE, fid(0x45), SHANK_E, On::Front, vec![circle(px, pz, 8.0)], &[v(px, pz)], (-24.4, 9.6), Add, &[BOLT])?;
    s.run(&RenamePart { element: HARDWARE, part: SPACER, name: COURSE_BOM[3].0.into() })?;
    s.run(&RenamePart { element: HARDWARE, part: BOLT, name: COURSE_BOM[4].0.into() })?;
    s.run(&SetPartAppearance { element: HARDWARE, parts: vec![SPACER], appearance: Some(Appearance::rgb(168, 172, 178)) })?;
    s.run(&SetPartAppearance { element: HARDWARE, parts: vec![BOLT], appearance: Some(Appearance::rgb(84, 88, 96)) })?;

    // The instances, in the course's order.
    let at = |element, part| InstanceSource::Part { element, part };
    let add = |s: &mut dyn Studio, id, source, pose| s.run(&InsertInstance { element: ASSEMBLY, instance: Instance::new(id, source, pose) });
    add(s, I_MASTER, at(MASTER_STUDIO, MASTER_CYLINDER), Pose::IDENTITY)?;
    add(s, I_GRIP, at(handle, super::GRIP), Pose::IDENTITY)?;
    add(s, I_PLATE, at(handle, super::PLATE), Pose::IDENTITY)?;
    add(s, I_SPACERS[0], at(HARDWARE, SPACER), Pose::translation([0.0, 5.0, 0.0]))?;
    add(s, I_SPACERS[1], at(HARDWARE, SPACER), Pose::translation([0.0, -8.0, 0.0]))?;
    add(s, I_BOLT, at(HARDWARE, BOLT), Pose::IDENTITY)?;
    use std::f64::consts::FRAC_PI_2;
    // Along the pivot: +Y (a part's Z turned onto +Y) and −Y.
    let out_y = |y: f64| Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], -FRAC_PI_2).then(&Pose::translation([px, y, pz]));
    let in_y = |y: f64| Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], FRAC_PI_2).then(&Pose::translation([px, y, pz]));
    let std_insert = |s: &mut dyn Studio, spec: &StandardSpec, list: Vec<(InstanceId, Pose)>, mate0: u128| -> Result<ElementId, CommandError> {
        let part = StandardPart::new(spec)?;
        let el = part.element.id;
        let inserts = list
            .into_iter()
            .enumerate()
            .map(|(k, (instance, pose))| StdInsert { instance, mate: MateId::from_u128(0x4a2d_b2a7_6000_0000_0000_0000_0000_0000 | (mate0 + k as u128)), pose, hole: None, offset: 0.0 })
            .collect();
        s.run(&InsertStandardContent { element: ASSEMBLY, part, inserts, restack: Vec::new() })?;
        Ok(el)
    };
    let nut = std_insert(s, &nut_spec(), vec![(I_NUT, in_y(-17.6))], 1)?;
    let washer = std_insert(s, &washer_spec(), vec![(I_WASHERS[0], out_y(8.0)), (I_WASHERS[1], in_y(-16.0))], 2)?;
    // The cap screws hold the upper enclosure to the cylinder: heads on its face towards the
    // pivot (x 290), shanks along +X, on a Ø44 circle round the cylinder's axis.
    let screw_pose = |a: f64| {
        let a = a.to_radians();
        Pose::rotation_about([0.0; 3], [0.0, 1.0, 0.0], -FRAC_PI_2).then(&Pose::translation([290.0, my + 22.0 * a.cos(), mz + 22.0 * a.sin()]))
    };
    let screws: Vec<(InstanceId, Pose)> = CAP_SCREWS.iter().zip([90.0, 210.0, 330.0]).map(|(i, a)| (*i, screw_pose(a))).collect();
    // Inserted as M6 and named, then made M5: the M6 configuration keeps its name for D14.7.
    let m6 = std_insert(s, &screw_spec(SCREW_AFTER), screws, 4)?;
    let std_part = crate::assembly::standard::PART;
    name_part(s, PropertyOwner::Part { element: m6, part: std_part }, COURSE_BOM[7].0)?;
    let edit = EditStandardContent::new(s.document(), ASSEMBLY, &CAP_SCREWS, Some(SCREW_BEFORE), None)?;
    let m5 = edit.parts.first().map(|p| p.element.id).ok_or_else(|| CommandError::Invalid("no M5 configuration".into()))?;
    s.run(&edit)?;
    name_part(s, PropertyOwner::Part { element: m5, part: std_part }, COURSE_BOM[7].0)?;
    name_part(s, PropertyOwner::Part { element: nut, part: std_part }, COURSE_BOM[5].0)?;
    name_part(s, PropertyOwner::Part { element: washer, part: std_part }, COURSE_BOM[6].0)?;
    let add = |s: &mut dyn Studio, id, source, pose| s.run(&InsertInstance { element: ASSEMBLY, instance: Instance::new(id, source, pose) });
    add(s, I_LOWER, at(ENCLOSURES, ENCLOSURE_LOWER), Pose::IDENTITY)?;
    add(s, I_UPPER, at(ENCLOSURES, ENCLOSURE_UPPER), Pose::IDENTITY)?;
    // The enclosures are fixed (bolted to the vehicle); every other instance is held by a
    // Fastened mate to the part it sits on: the course's 12 mates (P3C wrap-up).
    s.run(&SetInstancesFixed { element: ASSEMBLY, instances: vec![I_LOWER, I_UPPER], fixed: true })?;
    fasten_all(s)?;
    // The BOM: Item, Name, Quantity (`ex3-drawing.png`).
    let mut settings = s.document().element(ASSEMBLY).and_then(|e| e.assembly_model()).map(|a| a.bom.clone()).unwrap_or_default();
    settings.columns = vec![BomColumn::Item, BomColumn::Property(PropertyKey::Name), BomColumn::Quantity];
    s.run(&SetBomSettings { element: ASSEMBLY, settings, label: "BOM columns".into() })?;
    Ok(())
}

/// The 12 Fastened mates (child, parent): each at the child's own origin, so every mate is
/// satisfied where the parts are and nothing moves.
pub const MATES: [(InstanceId, InstanceId); 12] = [
    (I_MASTER, I_UPPER),
    (I_PLATE, I_LOWER),
    (I_GRIP, I_PLATE),
    (I_SPACERS[0], I_PLATE),
    (I_SPACERS[1], I_PLATE),
    (I_BOLT, I_PLATE),
    (I_NUT, I_BOLT),
    (I_WASHERS[0], I_BOLT),
    (I_WASHERS[1], I_BOLT),
    (CAP_SCREWS[0], I_UPPER),
    (CAP_SCREWS[1], I_UPPER),
    (CAP_SCREWS[2], I_UPPER),
];

fn fasten_all(s: &mut dyn Studio) -> Result<(), CommandError> {
    use crate::assembly::connector::{ConnectorFrame, MateConnector};
    use crate::assembly::mate::{Mate, MateFeature, MateKind, MateType};
    let pose_of = |s: &dyn Studio, i: InstanceId| {
        s.document()
            .element(ASSEMBLY)
            .and_then(|e| e.assembly_model())
            .and_then(|a| a.instances.iter().find(|x| x.id == i).map(|x| x.pose))
            .ok_or_else(|| CommandError::Invalid(format!("no instance {i:?}")))
    };
    for (k, (child, parent)) in MATES.iter().enumerate() {
        let (pc, pp) = (pose_of(s, *child)?, pose_of(s, *parent)?);
        // The child's origin frame, in the parent's coordinates.
        let rel = pc.then(&pp.inverse());
        let d = ConnectorFrame::default();
        let on_parent = ConnectorFrame { origin: rel.apply(d.origin), z: rel.rotate(d.z), x: rel.rotate(d.x) };
        let mate = Mate::new(MateType::Fastened, MateConnector::at(*child, d), MateConnector::at(*parent, on_parent));
        let id = MateId::from_u128(0x4a2d_b2a7_7000_0000_0000_0000_0000_0000 | (k as u128 + 1));
        s.run(&crate::assembly::commands::AddMateFeature {
            element: ASSEMBLY,
            feature: MateFeature::new(id, format!("Fastened {}", k + 1), MateKind::Mate(mate)),
            poses: Vec::new(),
        })?;
    }
    Ok(())
}

/// D14.7: the three ISO 4762 cap screws edited to M6 (Edit standard content instance…).
pub fn course_d14_7(s: &mut dyn Studio) -> Result<(), CommandError> {
    let edit = EditStandardContent::new(s.document(), ASSEMBLY, &CAP_SCREWS, Some(SCREW_AFTER), None)?;
    s.run(&edit)
}
