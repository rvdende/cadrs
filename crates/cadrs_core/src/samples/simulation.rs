//! The P3F.5 simulation stand-ins (`intro-to-parametric-cad-gaps.md`, "P3.5 simulation"): the
//! course names simulation without a model, so the gap list's cantilever is ours, in mm.
//!
//! - [`beam_in`]: **Beam**, 100 × 10 × 10 mm along X (a 100 × 10 rectangle on Top extruded
//!   10 mm: x 0…100, y 0…10, z 0…10), **Steel - A36** (E = 200 GPa). Fixed at x = 0 with 100 N
//!   down (−Z) on the end x = 100, Euler–Bernoulli gives a tip deflection PL³/(3EI) = 0.200 mm
//!   and a mid-span top-fibre stress M·c/I = 30.0 MPa (see `cadrs_fea/tests/acceptance.rs`).
//! - [`halves_document`] (`fixtures/simulation_halves.cadrs`): the same beam as two 50 mm
//!   halves in Part Studio **Halves**, and an assembly **Beam Assembly** with **Half 1 <1>**
//!   fixed and **Fastened 1** joining **Half 2 <1>** to it at the joint, Simulation connection
//!   checked: loaded like the solid beam, it deflects the same.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::assembly::commands::{AddMateFeature, InsertInstance, SetInstancesFixed};
use crate::assembly::connector::{ConnectorFrame, MateConnector};
use crate::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateType};
use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddElement, AddExtrude, AddSketch, EditSketch, NewElementKind, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, ExtrudeFeature};
use crate::ids::{ElementId, FeatureId, PartId};

use super::gear_cover::{DocHistory, Studio};

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x3f05_0000_0000_0000_0000_0000_0000_0000 + n)
}

/// The beam's material.
pub const MATERIAL: &str = "Steel - A36";

/// A box part: a rectangle on Top from (`x0`, 0) to (`x1`, 10) extruded 10 mm, named, in
/// [`MATERIAL`]. `n` numbers its sketch and extrude (`fid(n)`, `fid(n + 1)`).
fn bar(s: &mut dyn Studio, el: ElementId, n: u128, x0: f64, x1: f64, name: &str, ids: bool) -> Result<PartId, CommandError> {
    let (sk, ex) = if ids { (fid(n), fid(n + 1)) } else { (FeatureId::new(), FeatureId::new()) };
    s.run(&AddSketch { element: el, feature: sk, plane: Some(PlaneRef::Top) })?;
    let pts = vec![Vec2::new(x0, 0.0), Vec2::new(x1, 0.0), Vec2::new(x1, 10.0), Vec2::new(x0, 10.0)];
    s.run(&EditSketch { element: el, feature: sk, op: SketchOp::Batch(vec![SketchOp::AddPolyline { points: pts, closed: true, construction: false, label: "Add rectangle" }]) })?;
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(sk))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sk, &g, &[Vec2::new((x0 + x1) / 2.0, 5.0)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(format!("no region for {name}")));
    }
    let mut x = super::extrude_of(regions, 10.0);
    x.op = BooleanOp::New;
    s.run(&AddExtrude { element: el, feature: ex, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: ex, extrude: x, label: "Extrude".into() })?;
    let p = PartId::new(ex, 0);
    s.run(&RenamePart { element: el, part: p, name: name.into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![p], material: crate::material::library(MATERIAL) })?;
    Ok(p)
}

/// Builds **Beam** in a Part Studio (fresh feature ids: it can be built into any studio).
pub fn beam_in(s: &mut dyn Studio, el: ElementId) -> Result<PartId, CommandError> {
    bar(s, el, 0, 0.0, 100.0, "Beam", false)
}

pub const HALVES: ElementId = ElementId::from_u128(0x3f05_0000_0000_0000_0000_0000_0000_0101);
pub const BEAM_ASSEMBLY: ElementId = ElementId::from_u128(0x3f05_0000_0000_0000_0000_0000_0000_0102);
pub const HALF_1: InstanceId = InstanceId::from_u128(0x3f05_0000_0000_0000_0000_0000_0000_1001);
pub const HALF_2: InstanceId = InstanceId::from_u128(0x3f05_0000_0000_0000_0000_0000_0000_1002);
pub const FASTENED_1: MateId = MateId::from_u128(0x3f05_0000_0000_0000_0000_0000_0000_2001);

/// "Beam halves (stand-in)": see the module doc.
pub fn halves_document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Beam halves (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x3f05_0000_0000_0000_0000_0000_0000_0001);
    let mut e = crate::document::Element::part_studio("Halves");
    e.id = HALVES;
    doc.elements.push(e);
    let mut h = History::default();
    let (p1, p2) = {
        let s = &mut DocHistory(&mut doc, &mut h);
        (bar(s, HALVES, 0x10, 0.0, 50.0, "Half 1", true)?, bar(s, HALVES, 0x20, 50.0, 100.0, "Half 2", true)?)
    };
    h.execute(&mut doc, &AddElement { id: BEAM_ASSEMBLY, kind: NewElementKind::Assembly, name: Some("Beam Assembly".into()), after: None })?;
    for (i, p) in [(HALF_1, p1), (HALF_2, p2)] {
        h.execute(&mut doc, &InsertInstance { element: BEAM_ASSEMBLY, instance: Instance::new(i, InstanceSource::Part { element: HALVES, part: p }, Pose::IDENTITY) })?;
    }
    h.execute(&mut doc, &SetInstancesFixed { element: BEAM_ASSEMBLY, instances: vec![HALF_1], fixed: true })?;
    // The joint's centre, Z along the beam.
    let f = ConnectorFrame::new([50.0, 5.0, 5.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut m = Mate::new(MateType::Fastened, MateConnector::at(HALF_2, f), MateConnector::at(HALF_1, f));
    m.simulation = true;
    h.execute(&mut doc, &AddMateFeature { element: BEAM_ASSEMBLY, feature: MateFeature::new(FASTENED_1, "Fastened 1", MateKind::Mate(m)), poses: Vec::new() })?;
    if let Some(k) = doc.elements.iter().position(|e| e.id == BEAM_ASSEMBLY) {
        let a = doc.elements.remove(k);
        doc.elements.insert(0, a);
    }
    Ok(doc)
}

/// The fixture file of a document (as the other stand-ins store theirs).
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
