//! The test drive exercise's stand-in (P3E.5, `test-drive.md` "Exercises", TD4.1, TD7–TD12):
//! "Drill (stand-in)", a small copy of the course's DRILL HOTD document
//! (`fixtures/drill_standin.cadrs`), bundled as a sample (`documents_page::SAMPLES`).
//!
//! Tabs, in order (mm, Z up; every part Aluminum 6061, with a part number and a description):
//!
//! - Part Studio **CARBURETOR**:
//!   - **MANIFOLD** (Sketch 1 on Top, Extrude 1, 20 mm up: z 0..20): a 40 × 30 plate centred on
//!     the origin with a Ø20 bore in the middle and two Ø5.5 bolt holes at x = ±15.5, all
//!     through. Its top face (z = 20) is the **mounting face** the exercise's gasket is extruded
//!     from: 40·30 − π(10² + 2·2.75²) = 1200 − 115.125π = 838.3241 mm².
//!   - **CARBURETOR_BODY** (Sketch 2 on Top, Extrude 2, 36 mm down: z −36..0): a 22 × 30 block
//!     under the manifold, clear of its bolt holes (|x| ≤ 11 < 15.5 − 4.25, the M5 heads' radius).
//! - Assembly **CARBURETOR**: MANIFOLD <1> fixed at the origin, CARBURETOR_BODY <1> fastened to
//!   it (Fastened 1, at the shared face's centre).
//! - Part Studio **DRILL BODY**: **DRILL_BODY** (Sketch 1 on Top, Extrude 1, 90 mm down:
//!   z −90..0), a 160 × 80 block with a Ø20 intake port and two Ø4.2 tapping holes for M5 at
//!   x = ±15.5 (the manifold's hole pattern), through.
//! - Assembly **FUEL AND POWER TRAIN**: DRILL_BODY <1> fixed at the origin.
//!
//! The exercise's steps 2–7 in code (the course test, `tests/course_test_drive.rs`; the
//! scenario `course_td_ex1_drill` does them through the UI):
//!
//! - [`add_gasket`]: step 2, the mounting face extruded 2 mm, New: **CARBURETOR_GASKET**
//!   (z 20..22), V = 2·(1200 − 115.125π) = 1676.6483 mm³ ([`gasket_volume`]).
//! - [`insert_gasket`]: step 3, the gasket in CARBURETOR, Fastened to the manifold at a bolt
//!   hole's centre.
//! - [`insert_carburetor`]: step 4, CARBURETOR in FUEL AND POWER TRAIN, flipped (turned half a
//!   turn about X through z = 11) so the gasket's top face (z 22) sits on the drill body's top
//!   (z 0): the gasket z 0..2, the manifold 2..22, the carburetor body 22..58. Fastened at the
//!   bore's centre.
//! - [`insert_screws`]: step 5, two ISO 4762 M5 × 25 socket head cap screws on the manifold's
//!   bolt holes (their edges on its back face, z 22 in FUEL AND POWER TRAIN), each with its
//!   Fastened mate (standard content's batch placement).
//!
//! Every id is fixed, so tests and scenarios can name them.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::Arc;

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::assembly::commands::{AddMateFeature, InsertInstance, SetInstancesFixed};
use crate::assembly::connector::{ConnectorFrame, MateConnector};
use crate::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateType};
use crate::assembly::standard::{StandardPart, StandardSpec, Stacking, plan_insert, site_of_edge};
use crate::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddSketch, EditSketch, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, Element, ExtrudeFeature, FaceRef};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};

use super::gear_cover::{DocHistory, Studio};

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x3e50_0000_0000_0000_0000_0000_0000_0000 | n)
}
const fn iid(n: u128) -> InstanceId {
    InstanceId::from_u128(0x3e50_0000_0000_0000_0000_0000_0000_1000 | n)
}
const fn mid(n: u128) -> MateId {
    MateId::from_u128(0x3e50_0000_0000_0000_0000_0000_0000_2000 | n)
}

pub const DOCUMENT: DocumentId = DocumentId::from_u128(0x3e50_0000_0000_0000_0000_0000_0000_0100);
/// Part Studio CARBURETOR.
pub const CARBURETOR_STUDIO: ElementId = ElementId::from_u128(0x3e50_0000_0000_0000_0000_0000_0000_0101);
/// Assembly CARBURETOR.
pub const CARBURETOR: ElementId = ElementId::from_u128(0x3e50_0000_0000_0000_0000_0000_0000_0102);
/// Part Studio DRILL BODY.
pub const DRILL_STUDIO: ElementId = ElementId::from_u128(0x3e50_0000_0000_0000_0000_0000_0000_0103);
/// Assembly FUEL AND POWER TRAIN.
pub const POWER_TRAIN: ElementId = ElementId::from_u128(0x3e50_0000_0000_0000_0000_0000_0000_0104);

pub const MANIFOLD_SKETCH: FeatureId = fid(1);
pub const MANIFOLD_EXTRUDE: FeatureId = fid(2);
pub const BODY_SKETCH: FeatureId = fid(3);
pub const BODY_EXTRUDE: FeatureId = fid(4);
pub const DRILL_SKETCH: FeatureId = fid(5);
pub const DRILL_EXTRUDE: FeatureId = fid(6);
/// The exercise's step 2 ([`add_gasket`]; the app gives it a fresh id).
pub const GASKET_EXTRUDE: FeatureId = fid(7);
pub const MANIFOLD_PART: PartId = PartId::new(MANIFOLD_EXTRUDE, 0);
pub const BODY_PART: PartId = PartId::new(BODY_EXTRUDE, 0);
pub const DRILL_PART: PartId = PartId::new(DRILL_EXTRUDE, 0);
pub const GASKET_PART: PartId = PartId::new(GASKET_EXTRUDE, 0);

/// In CARBURETOR.
pub const MANIFOLD_INSTANCE: InstanceId = iid(1);
pub const BODY_INSTANCE: InstanceId = iid(2);
pub const GASKET_INSTANCE: InstanceId = iid(3);
/// In FUEL AND POWER TRAIN.
pub const DRILL_INSTANCE: InstanceId = iid(4);
pub const CARBURETOR_INSTANCE: InstanceId = iid(5);
pub const SCREW_INSTANCES: [InstanceId; 2] = [iid(6), iid(7)];

pub const NAME: &str = "Drill (stand-in)";
pub const CARBURETOR_NAME: &str = "CARBURETOR";
pub const DRILL_STUDIO_NAME: &str = "DRILL BODY";
pub const POWER_TRAIN_NAME: &str = "FUEL AND POWER TRAIN";
pub const MANIFOLD_NAME: &str = "MANIFOLD";
pub const BODY_NAME: &str = "CARBURETOR_BODY";
pub const DRILL_NAME: &str = "DRILL_BODY";
pub const GASKET_NAME: &str = "CARBURETOR_GASKET";

/// Part numbers (the gasket gets the next one, PRT-000006, from Generate next part number).
pub const PART_NUMBERS: [(&str, &str); 5] = [
    (MANIFOLD_NAME, "PRT-000001"),
    (BODY_NAME, "PRT-000002"),
    (DRILL_NAME, "PRT-000003"),
    (CARBURETOR_NAME, "PRT-000004"),
    (POWER_TRAIN_NAME, "PRT-000005"),
];
/// What Generate next part number gives the gasket.
pub const GASKET_PART_NUMBER: &str = "PRT-000006";

/// mm.
pub const MANIFOLD_L: f64 = 40.0;
pub const MANIFOLD_W: f64 = 30.0;
pub const BORE_R: f64 = 10.0;
/// The Ø5.5 bolt holes' radius and their centres' x (±).
pub const HOLE_R: f64 = 2.75;
pub const HOLE_X: f64 = 15.5;
pub const MANIFOLD_T: f64 = 20.0;
pub const BODY_L: f64 = 22.0;
pub const BODY_W: f64 = 30.0;
pub const BODY_H: f64 = 36.0;
pub const DRILL_L: f64 = 160.0;
pub const DRILL_W: f64 = 80.0;
pub const DRILL_H: f64 = 90.0;
/// The drill body's Ø4.2 tapping holes (M5).
pub const TAP_R: f64 = 2.1;
/// The exercise's gasket, and the branch's (step 9).
pub const GASKET_T: f64 = 2.0;
pub const THIN_GASKET_T: f64 = 1.0;

/// The manifold's mounting face, mm²: 40·30 − π(10² + 2·2.75²) = 1200 − 115.125π.
pub fn mounting_area() -> f64 {
    MANIFOLD_L * MANIFOLD_W - PI * (BORE_R * BORE_R + 2.0 * HOLE_R * HOLE_R)
}

/// The gasket's volume at `thickness` mm.
pub fn gasket_volume(thickness: f64) -> f64 {
    mounting_area() * thickness
}

fn rect(l: f64, w: f64) -> SketchOp {
    let (hx, hy) = (l / 2.0, w / 2.0);
    SketchOp::AddPolyline {
        points: vec![Vec2::new(-hx, -hy), Vec2::new(hx, -hy), Vec2::new(hx, hy), Vec2::new(-hx, hy)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

/// A sketch on Top of a `l` × `w` rectangle and `circles`, its region (seeded at `seed`)
/// extruded `depth` mm (down when `down`), New: the part `name`, Aluminum 6061, with its part
/// number and description.
#[allow(clippy::too_many_arguments)]
fn block(
    s: &mut dyn Studio,
    el: ElementId,
    (sketch, extrude): (FeatureId, FeatureId),
    (l, w): (f64, f64),
    circles: &[(f64, f64, f64)],
    seed: Vec2,
    (depth, down): (f64, bool),
    (name, number, description): (&str, &str, &str),
) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: rect(l, w) })?;
    for &(x, y, r) in circles {
        s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::AddCircle { center: Vec2::new(x, y), radius: r, construction: false } })?;
    }
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|k| k.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("the sketch".into()))?;
    let regions = super::region_refs(sketch, &g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(format!("{name}: the region is missing")));
    }
    s.run(&AddExtrude { element: el, feature: extrude, extrude: ExtrudeFeature::default() })?;
    let mut e = super::extrude_of(regions, depth);
    e.flip = down;
    s.run(&SetExtrude { element: el, feature: extrude, extrude: e, label: "Extrude".into() })?;
    let part = PartId::new(extrude, 0);
    s.run(&RenamePart { element: el, part, name: name.into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![part], material: crate::material::library("Aluminum - 6061") })?;
    props(s, PropertyOwner::Part { element: el, part }, number, description)
}

fn props(s: &mut dyn Studio, owner: PropertyOwner, number: &str, description: &str) -> Result<(), CommandError> {
    s.run(&SetProperties {
        owners: vec![owner],
        values: vec![(PropertyKey::PartNumber, PropertyValue::Text(number.into())), (PropertyKey::Description, PropertyValue::Text(description.into()))],
        label: "Properties".into(),
    })
}

/// A Fastened mate `name` (`id`) of assembly `el` between connectors `a` and `b` (frames in
/// their instances' coordinates); nothing moves.
fn fastened(s: &mut dyn Studio, el: ElementId, id: MateId, name: &str, a: MateConnector, b: MateConnector) -> Result<(), CommandError> {
    s.run(&AddMateFeature { element: el, feature: MateFeature::new(id, name, MateKind::Mate(Mate::new(MateType::Fastened, a, b))), poses: Vec::new() })
}

fn up(origin: [f64; 3]) -> ConnectorFrame {
    ConnectorFrame::new(origin, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])
}

/// The document (see the module docs): the exercise's starting point.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(NAME);
    doc.id = DOCUMENT;
    for (id, name, assembly) in [
        (CARBURETOR_STUDIO, CARBURETOR_NAME, false),
        (CARBURETOR, CARBURETOR_NAME, true),
        (DRILL_STUDIO, DRILL_STUDIO_NAME, false),
        (POWER_TRAIN, POWER_TRAIN_NAME, true),
    ] {
        let mut el = if assembly { Element::assembly(name) } else { Element::part_studio(name) };
        el.id = id;
        doc.elements.push(el);
    }
    let mut h = History::default();
    let mut st = DocHistory(&mut doc, &mut h);
    let s: &mut dyn Studio = &mut st;
    let holes = [(0.0, 0.0, BORE_R), (-HOLE_X, 0.0, HOLE_R), (HOLE_X, 0.0, HOLE_R)];
    block(
        s,
        CARBURETOR_STUDIO,
        (MANIFOLD_SKETCH, MANIFOLD_EXTRUDE),
        (MANIFOLD_L, MANIFOLD_W),
        &holes,
        Vec2::new(0.0, MANIFOLD_W / 2.0 - 2.0),
        (MANIFOLD_T, false),
        (MANIFOLD_NAME, PART_NUMBERS[0].1, "Intake manifold"),
    )?;
    block(
        s,
        CARBURETOR_STUDIO,
        (BODY_SKETCH, BODY_EXTRUDE),
        (BODY_L, BODY_W),
        &[],
        Vec2::new(0.0, 0.0),
        (BODY_H, true),
        (BODY_NAME, PART_NUMBERS[1].1, "Carburetor body"),
    )?;
    block(
        s,
        DRILL_STUDIO,
        (DRILL_SKETCH, DRILL_EXTRUDE),
        (DRILL_L, DRILL_W),
        &[(0.0, 0.0, BORE_R), (-HOLE_X, 0.0, TAP_R), (HOLE_X, 0.0, TAP_R)],
        Vec2::new(0.0, DRILL_W / 2.0 - 5.0),
        (DRILL_H, true),
        (DRILL_NAME, PART_NUMBERS[2].1, "Drill body casting"),
    )?;
    // CARBURETOR: the manifold fixed, the body fastened under it.
    let part = |element, part| InstanceSource::Part { element, part };
    s.run(&InsertInstance { element: CARBURETOR, instance: Instance::new(MANIFOLD_INSTANCE, part(CARBURETOR_STUDIO, MANIFOLD_PART), Pose::IDENTITY) })?;
    s.run(&SetInstancesFixed { element: CARBURETOR, instances: vec![MANIFOLD_INSTANCE], fixed: true })?;
    s.run(&InsertInstance { element: CARBURETOR, instance: Instance::new(BODY_INSTANCE, part(CARBURETOR_STUDIO, BODY_PART), Pose::IDENTITY) })?;
    fastened(s, CARBURETOR, mid(1), "Fastened 1", MateConnector::at(BODY_INSTANCE, up([0.0; 3])), MateConnector::at(MANIFOLD_INSTANCE, up([0.0; 3])))?;
    props(s, PropertyOwner::Assembly { element: CARBURETOR }, PART_NUMBERS[3].1, "Carburetor assembly")?;
    // FUEL AND POWER TRAIN: the drill body, fixed.
    s.run(&InsertInstance { element: POWER_TRAIN, instance: Instance::new(DRILL_INSTANCE, part(DRILL_STUDIO, DRILL_PART), Pose::IDENTITY) })?;
    s.run(&SetInstancesFixed { element: POWER_TRAIN, instances: vec![DRILL_INSTANCE], fixed: true })?;
    props(s, PropertyOwner::Assembly { element: POWER_TRAIN }, PART_NUMBERS[4].1, "Fuel and power train")?;
    Ok(doc)
}

/// The file of the fixture.
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}

// ---------------------------------------------------------------------------------------------
// The exercise in code

/// The planar face of `part` (built in `el`) with outward normal `n` that contains `p`.
pub fn face_at(doc: &Document, el: ElementId, part: PartId, n: [f64; 3], p: [f64; 3]) -> Result<FaceRef, CommandError> {
    let e = doc.element(el).ok_or(CommandError::ElementNotFound(el))?;
    let build = crate::rebuild::build(e.features());
    let part = build.part(part).ok_or_else(|| CommandError::Invalid("the part isn't built".into()))?;
    let s = &part.solid;
    let i = (0..s.faces.len())
        .find(|&i| {
            s.faces[i].plane.is_some_and(|pl| {
                let m = pl.normal();
                let l = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                (m[0] * n[0] + m[1] * n[1] + m[2] * n[2]) / l > 0.999
            }) && s.face_contains(i, p)
        })
        .ok_or_else(|| CommandError::Invalid("no face there".into()))?;
    Ok(FaceRef { part: part.id, face: s.faces[i].name, seed: p })
}

/// Step 2: the MANIFOLD's mounting face extruded `thickness` mm, New, renamed
/// CARBURETOR_GASKET.
pub fn add_gasket(s: &mut dyn Studio, thickness: f64) -> Result<(), CommandError> {
    let face = face_at(s.document(), CARBURETOR_STUDIO, MANIFOLD_PART, [0.0, 0.0, 1.0], [0.0, MANIFOLD_W / 2.0 - 2.0, MANIFOLD_T])?;
    s.run(&AddExtrude { element: CARBURETOR_STUDIO, feature: GASKET_EXTRUDE, extrude: ExtrudeFeature::default() })?;
    let e = ExtrudeFeature { faces: vec![face], depth: thickness, depth_expr: format!("{thickness} mm"), op: BooleanOp::New, ..ExtrudeFeature::default() };
    s.run(&SetExtrude { element: CARBURETOR_STUDIO, feature: GASKET_EXTRUDE, extrude: e, label: "Extrude".into() })?;
    s.run(&RenamePart { element: CARBURETOR_STUDIO, part: GASKET_PART, name: GASKET_NAME.into() })
}

/// Step 9's edit: the gasket's extrude `thickness` mm deep.
pub fn set_gasket_thickness(s: &mut dyn Studio, thickness: f64) -> Result<(), CommandError> {
    let mut e = s
        .document()
        .element(CARBURETOR_STUDIO)
        .and_then(|e| e.feature(GASKET_EXTRUDE))
        .and_then(|f| f.extrude())
        .cloned()
        .ok_or_else(|| CommandError::Invalid("the gasket's extrude is missing".into()))?;
    e.depth = thickness;
    e.depth_expr = format!("{thickness} mm");
    s.run(&SetExtrude { element: CARBURETOR_STUDIO, feature: GASKET_EXTRUDE, extrude: e, label: "Extrude".into() })
}

/// Step 3: the gasket in CARBURETOR where it was made, Fastened to the manifold at the centre
/// of the bolt hole at x = +15.5 on the faces they share (z 20).
pub fn insert_gasket(s: &mut dyn Studio) -> Result<(), CommandError> {
    let source = InstanceSource::Part { element: CARBURETOR_STUDIO, part: GASKET_PART };
    s.run(&InsertInstance { element: CARBURETOR, instance: Instance::new(GASKET_INSTANCE, source, Pose::IDENTITY) })?;
    let hole = up([HOLE_X, 0.0, MANIFOLD_T]);
    fastened(s, CARBURETOR, mid(2), "Fastened 2", MateConnector::at(GASKET_INSTANCE, hole), MateConnector::at(MANIFOLD_INSTANCE, hole))
}

/// Where step 4 puts CARBURETOR in FUEL AND POWER TRAIN: half a turn about X through
/// z = (20 + 2)/2, so the 2 mm gasket's top face lands on the drill body's top (z 0).
pub fn carburetor_pose() -> Pose {
    Pose::rotation_about([0.0, 0.0, (MANIFOLD_T + GASKET_T) / 2.0], [1.0, 0.0, 0.0], PI)
}

/// Step 4: CARBURETOR in FUEL AND POWER TRAIN, flipped onto the drill body, Fastened at the
/// bore's centre (the gasket's top face to the drill body's top face).
pub fn insert_carburetor(s: &mut dyn Studio) -> Result<(), CommandError> {
    s.run(&InsertInstance { element: POWER_TRAIN, instance: Instance::new(CARBURETOR_INSTANCE, InstanceSource::Assembly { element: CARBURETOR }, carburetor_pose()) })?;
    let gasket = assembly::structure::derive(CARBURETOR_INSTANCE, GASKET_INSTANCE);
    let mut a = MateConnector::at(gasket, up([0.0, 0.0, MANIFOLD_T + GASKET_T]));
    a.flip = true;
    fastened(s, POWER_TRAIN, mid(3), "Fastened 1", a, MateConnector::at(DRILL_INSTANCE, up([0.0; 3])))
}

/// ISO 4762 M5 × 25.
pub fn screw_spec() -> StandardSpec {
    let mut s = StandardSpec::new("ISO", "Bolts & screws", "Socket head screws", "Socket head cap screw").expect("in the library");
    s.size = "M5".into();
    s.length = Some(25.0);
    s.normalize();
    s
}

/// Step 5: two ISO 4762 M5 × 25 screws on the manifold's bolt holes, on its back face (the
/// hole edges at z = 22 in FUEL AND POWER TRAIN), batch placed with their Fastened mates.
pub fn insert_screws(doc: &mut Document, h: &mut History) -> Result<(), CommandError> {
    let asm = doc.element(POWER_TRAIN).and_then(|e| e.assembly_model()).cloned().ok_or(CommandError::ElementNotFound(POWER_TRAIN))?;
    let mut builds: HashMap<ElementId, Arc<crate::rebuild::Build>> = HashMap::new();
    for o in assembly::structure::occurrences(doc, &asm) {
        if let std::collections::hash_map::Entry::Vacant(e) = builds.entry(o.element)
            && let Some(el) = doc.element(o.element)
        {
            e.insert(crate::rebuild::build(el.features()));
        }
    }
    let solids = assembly::occurrence_solids(doc, &asm, |e| builds.get(&e).cloned());
    let manifold = assembly::structure::derive(CARBURETOR_INSTANCE, MANIFOLD_INSTANCE);
    let solid = solids.get(&manifold).ok_or_else(|| CommandError::Invalid("no manifold".into()))?;
    // The back face is z = 0 in the manifold's own coordinates (the occurrence's solid is in
    // its studio's).
    let mut sites: Vec<_> = solid
        .edges
        .iter()
        .filter(|e| e.circle.is_some_and(|c| (c.radius - HOLE_R).abs() < 1e-6 && c.center[2].abs() < 1e-6))
        .filter_map(|e| site_of_edge(solid, manifold, &e.name).map(|s| (e.circle.map_or(0.0, |c| c.center[0]), s)))
        .collect();
    sites.sort_by(|a, b| a.0.total_cmp(&b.0));
    if sites.len() != 2 {
        return Err(CommandError::Invalid(format!("the manifold has {} bolt hole edges on its back", sites.len())));
    }
    let sites: Vec<_> = sites.into_iter().map(|(_, s)| s).collect();
    let part = StandardPart::new(&screw_spec())?;
    let mut cmd = plan_insert(doc, POWER_TRAIN, part, &sites, false, Stacking::Plain, &solids)?;
    for (k, ins) in cmd.inserts.iter_mut().enumerate() {
        ins.instance = SCREW_INSTANCES[k];
        ins.mate = mid(10 + k as u128);
    }
    h.execute(doc, &cmd)
}
