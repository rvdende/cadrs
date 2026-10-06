//! Stand-ins for the exercises of the Managed In-Context Design course
//! (`managed-in-context-design.md` MCX1–MCX3; the course's documents aren't public).
//!
//! - [`slide_document`] (`fixtures/mic_slide_standin.cadrs`, MCX2 "Slide"): a **Rail** 200 × 30 ×
//!   10 mm (x −100…100) with Ø6 end-stop holes at x ±80, and two instances of a **Carriage** 40 ×
//!   30 × 12 mm riding on it, Carriage <1> at x −40 and Carriage <2> at x 40 (z 10). Edit
//!   Carriage <1> in context, delete it, set Carriage <2> as the primary instance, update.
//! - [`finger_document`] (`fixtures/mic_gripper_finger_standin.cadrs` with its history: V1,
//!   MCX3 "Gripper"): "Gripper finger (stand-in)", its Part Studio **Finger**: a plate 60 × 10 ×
//!   30 mm (x 0…60, y −5…5, z 0…30).
//! - [`gripper_document`] (`fixtures/mic_gripper_standin.cadrs`): "Gripper (stand-in)", its
//!   Part Studio **Palm** (a block 100 × 40 × 20 mm, x −50…50, y −20…20, z −20…0) and the
//!   assembly **Gripper**: Palm <1> fixed, and the Finger at V1 of the finger's document (a
//!   linked instance) at x −50 on the Palm's top.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::assembly::commands::{InsertInstance, SetInstancesFixed};
use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddElement, AddExtrude, AddSketch, EditSketch, NewElementKind, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, ExtrudeFeature, Offset};
use crate::external::{InsertLinked, SourceRef};
use crate::history_log::VersionId;
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};

use super::gear_cover::{DocHistory, Studio};

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x4c00_0000_0000_0000_0000_0000_0000_0000 + n)
}

const fn eid(n: u128) -> ElementId {
    ElementId::from_u128(0x4c00_0000_0000_0000_0000_0000_0000_0100 + n)
}

const fn iid(n: u128) -> InstanceId {
    InstanceId::from_u128(0x4c00_0000_0000_0000_0000_0000_0000_1000 + n)
}

const fn did(n: u128) -> DocumentId {
    DocumentId::from_u128(0x4c00_0000_0000_0000_0000_0000_0000_0000 + n)
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], closed: true, construction: false, label: "Add rectangle" }
}

fn circle(x: f64, y: f64, r: f64) -> SketchOp {
    SketchOp::AddCircle { center: v(x, y), radius: r, construction: false }
}

fn studio(doc: &mut Document, el: ElementId, name: &str) {
    let mut e = crate::document::Element::part_studio(name);
    e.id = el;
    doc.elements.push(e);
}

/// A sketch of `ops` on `plane` and its extrude from `z0` to `z1` (mm) along the plane's normal,
/// New: one part, named.
#[allow(clippy::too_many_arguments)]
fn part(s: &mut dyn Studio, el: ElementId, n: u128, plane: PlaneRef, ops: Vec<SketchOp>, seed: Vec2, z: (f64, f64), name: &str) -> Result<PartId, CommandError> {
    let (sk, ex) = (fid(n), fid(n + 1));
    s.run(&AddSketch { element: el, feature: sk, plane: Some(plane) })?;
    s.run(&EditSketch { element: el, feature: sk, op: SketchOp::Batch(ops) })?;
    let g = s.document().element(el).and_then(|x| x.feature(sk)).and_then(|f| f.sketch()).map(|s| s.geometry.clone()).ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sk, &g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(format!("no region for {name}")));
    }
    let (z0, z1) = z;
    let mut x = super::extrude_of(regions, z1 - z0);
    x.op = BooleanOp::New;
    if z0 != 0.0 {
        x.start_offset = Some(Offset { value: z0.abs(), expr: format!("{} mm", z0.abs()), flip: z0 < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature: ex, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: ex, extrude: x, label: "Extrude".into() })?;
    let p = PartId::new(ex, 0);
    s.run(&RenamePart { element: el, part: p, name: name.into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![p], material: crate::material::library("Aluminum - 6061") })?;
    Ok(p)
}

fn insert(doc: &mut Document, h: &mut History, asm: ElementId, i: InstanceId, el: ElementId, p: PartId, pose: Pose) -> Result<(), CommandError> {
    h.execute(doc, &InsertInstance { element: asm, instance: Instance::new(i, InstanceSource::Part { element: el, part: p }, pose) })
}

fn assembly_first(doc: &mut Document, asm: ElementId) {
    if let Some(k) = doc.elements.iter().position(|e| e.id == asm) {
        let a = doc.elements.remove(k);
        doc.elements.insert(0, a);
    }
}

// ---------------------------------------------------------------------------------------------
// MCX2 Slide

pub const SLIDE_DOCUMENT: DocumentId = did(1);
pub const RAIL_STUDIO: ElementId = eid(1);
pub const CARRIAGE_STUDIO: ElementId = eid(2);
pub const SLIDE: ElementId = eid(3);
pub const RAIL: InstanceId = iid(1);
pub const CARRIAGE_1: InstanceId = iid(2);
pub const CARRIAGE_2: InstanceId = iid(3);
pub const RAIL_PART: PartId = PartId::new(fid(0x11), 0);
pub const CARRIAGE_PART: PartId = PartId::new(fid(0x21), 0);
/// The carriages' x and the rail's height (mm).
pub const CARRIAGE_X: [f64; 2] = [-40.0, 40.0];
pub const RAIL_TOP: f64 = 10.0;

/// "Slide (stand-in)": see the module docs.
pub fn slide_document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Slide (stand-in)");
    doc.id = SLIDE_DOCUMENT;
    let mut h = History::default();
    studio(&mut doc, RAIL_STUDIO, "Rail");
    studio(&mut doc, CARRIAGE_STUDIO, "Carriage");
    {
        let s = &mut DocHistory(&mut doc, &mut h);
        part(s, RAIL_STUDIO, 0x10, PlaneRef::Top, vec![rect(-100.0, -15.0, 100.0, 15.0), circle(-80.0, 0.0, 3.0), circle(80.0, 0.0, 3.0)], v(0.0, 0.0), (0.0, RAIL_TOP), "Rail")?;
        part(s, CARRIAGE_STUDIO, 0x20, PlaneRef::Top, vec![rect(-20.0, -15.0, 20.0, 15.0)], v(0.0, 0.0), (0.0, 12.0), "Carriage")?;
    }
    h.execute(&mut doc, &AddElement { id: SLIDE, kind: NewElementKind::Assembly, name: Some("Slide".into()), after: None })?;
    insert(&mut doc, &mut h, SLIDE, RAIL, RAIL_STUDIO, RAIL_PART, Pose::IDENTITY)?;
    insert(&mut doc, &mut h, SLIDE, CARRIAGE_1, CARRIAGE_STUDIO, CARRIAGE_PART, Pose::translation([CARRIAGE_X[0], 0.0, RAIL_TOP]))?;
    insert(&mut doc, &mut h, SLIDE, CARRIAGE_2, CARRIAGE_STUDIO, CARRIAGE_PART, Pose::translation([CARRIAGE_X[1], 0.0, RAIL_TOP]))?;
    h.execute(&mut doc, &SetInstancesFixed { element: SLIDE, instances: vec![RAIL], fixed: true })?;
    assembly_first(&mut doc, SLIDE);
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// MCX3 Gripper (linked documents)

pub const FINGER_DOCUMENT: DocumentId = did(2);
pub const FINGER_STUDIO: ElementId = eid(11);
pub const FINGER_PART: PartId = PartId::new(fid(0x31), 0);
/// The finger's V1 in `fixtures/mic_gripper_finger_standin.history.ron`.
pub const FINGER_V1: VersionId = VersionId(uuid::Uuid::from_u128(0x4c00_0000_0000_0000_0000_0000_0000_0a01));
pub const GRIPPER_DOCUMENT: DocumentId = did(3);
pub const PALM_STUDIO: ElementId = eid(21);
pub const GRIPPER: ElementId = eid(22);
pub const PALM: InstanceId = iid(21);
pub const FINGER: InstanceId = iid(22);
pub const PALM_PART: PartId = PartId::new(fid(0x41), 0);
/// Where the finger sits on the palm (x, mm).
pub const FINGER_X: f64 = -50.0;

/// "Gripper finger (stand-in)": see the module docs.
pub fn finger_document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Gripper finger (stand-in)");
    doc.id = FINGER_DOCUMENT;
    let mut h = History::default();
    studio(&mut doc, FINGER_STUDIO, "Finger");
    part(&mut DocHistory(&mut doc, &mut h), FINGER_STUDIO, 0x30, PlaneRef::Top, vec![rect(0.0, -5.0, 60.0, 5.0)], v(30.0, 0.0), (0.0, 30.0), "Finger")?;
    Ok(doc)
}

/// The finger document's history: its start and V1 ([`FINGER_V1`]).
pub fn finger_history(doc: &Document) -> crate::history_log::HistoryLog {
    super::linked_block::history_with_v1(doc, FINGER_V1, "The finger, 60 × 10 × 30")
}

/// "Gripper (stand-in)": see the module docs.
pub fn gripper_document() -> Result<Document, CommandError> {
    let finger = finger_document()?;
    let mut doc = Document::empty("Gripper (stand-in)");
    doc.id = GRIPPER_DOCUMENT;
    let mut h = History::default();
    studio(&mut doc, PALM_STUDIO, "Palm");
    part(&mut DocHistory(&mut doc, &mut h), PALM_STUDIO, 0x40, PlaneRef::Top, vec![rect(-50.0, -20.0, 50.0, 20.0)], v(0.0, 0.0), (-20.0, 0.0), "Palm")?;
    h.execute(&mut doc, &AddElement { id: GRIPPER, kind: NewElementKind::Assembly, name: Some("Gripper".into()), after: None })?;
    insert(&mut doc, &mut h, GRIPPER, PALM, PALM_STUDIO, PALM_PART, Pose::IDENTITY)?;
    h.execute(&mut doc, &SetInstancesFixed { element: GRIPPER, instances: vec![PALM], fixed: true })?;
    // The finger at V1 of its document.
    let r = SourceRef::version(Some(FINGER_DOCUMENT), FINGER_STUDIO, FINGER_V1);
    let snapshot = crate::external::snapshot(&finger, r, "V1").map_err(|e| CommandError::Invalid(e.to_string()))?;
    let inst = Instance::new(FINGER, InstanceSource::Part { element: snapshot.root, part: FINGER_PART }, Pose::translation([FINGER_X, 0.0, 0.0]));
    h.execute(&mut doc, &InsertLinked { element: GRIPPER, snapshot, instances: vec![inst], reference: r })?;
    assembly_first(&mut doc, GRIPPER);
    Ok(doc)
}

/// Any of the stand-ins as a document file.
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
