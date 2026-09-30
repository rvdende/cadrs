//! The Replicate stand-in (P3B.8, `intro-to-assemblies.md` X16): a flange with seven matching
//! holes and a screw in its centre hole, in inch, Aluminum - 6061.
//!
//! - Part Studio **Flange**: a Ø4 × 0.25 in disc on Top (z 0…0.25) with seven Ø0.25 in holes:
//!   one at the centre and six on a Ø3 in bolt circle (at 0°, 60°, … from +X).
//! - Part Studio **Screw**: a Ø0.25 in shank, z −1…0, and a Ø0.4 in head, z 0…0.2 (Add).
//! - Assembly **Flange Assembly**: Flange <1> (fixed) and Screw <1> in the centre hole, held by
//!   **Fastened 1** between the screw's shank rim under its head and the centre hole's
//!   top rim. Replicate places the screw on the six bolt-circle holes ([`replicate`]).
//!
//! Every id is fixed, so `fixtures/flange_standin.cadrs` is regenerated exactly
//! (`cadrs_core/tests/course_assemblies.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_sketch::PlaneRef;

use crate::assembly::commands::{AddMateFeature, InsertInstance, SetInstancesFixed};
use crate::assembly::connector::{EntityRef, ImplicitPoint, MateConnector, implicit_points};
use crate::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateType};
use crate::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddElement, NewElementKind, RenamePart, SetPartMaterial};
use crate::document::{BooleanOp, Document};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::solid::Solid;

use super::gear_cover::{DocHistory, Studio};
use super::step_stool::{IN, circle, circle_edge, extrude, sketch, v};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_0000 + n)
}

pub const FLANGE_STUDIO: ElementId = ElementId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_0101);
pub const SCREW_STUDIO: ElementId = ElementId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_0102);
pub const ASSEMBLY: ElementId = ElementId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_0201);

pub const FLANGE_PART: PartId = PartId::new(id(0x12), 0);
pub const SCREW_PART: PartId = PartId::new(id(0x22), 0);

pub const FLANGE: InstanceId = InstanceId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_1001);
pub const SCREW: InstanceId = InstanceId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_1002);
pub const FASTENED_1: MateId = MateId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_2001);

/// The flange's thickness, the holes' diameter and the bolt circle's radius (in).
pub const THICKNESS: f64 = 0.25;
pub const HOLE_D: f64 = 0.25;
pub const BOLT_R: f64 = 1.5;

/// The holes' centres (in, on Top): the centre, then the bolt circle from +X counter-clockwise.
pub fn hole_centres() -> Vec<[f64; 2]> {
    let mut out = vec![[0.0, 0.0]];
    for k in 0..6 {
        let a = (k as f64 * 60.0).to_radians();
        out.push([BOLT_R * a.cos(), BOLT_R * a.sin()]);
    }
    out
}

/// The Flange studio's features.
pub fn build_flange(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let mut ops = vec![circle(0.0, 0.0, 4.0)];
    ops.extend(hole_centres().into_iter().map(|c| circle(c[0], c[1], HOLE_D)));
    let g = sketch(s, el, id(0x11), PlaneRef::Top, ops)?;
    extrude(s, el, id(0x11), &g, id(0x12), &[v(0.75, 0.4)], (0.0, THICKNESS), BooleanOp::New, &[], false)?;
    s.run(&RenamePart { element: el, part: FLANGE_PART, name: "Flange".into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![FLANGE_PART], material: crate::material::library("Aluminum - 6061") })?;
    Ok(())
}

/// The Screw studio's features.
pub fn build_screw(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let g = sketch(s, el, id(0x21), PlaneRef::Top, vec![circle(0.0, 0.0, HOLE_D)])?;
    extrude(s, el, id(0x21), &g, id(0x22), &[v(0.0, 0.0)], (-1.0, 0.0), BooleanOp::New, &[], false)?;
    let g = sketch(s, el, id(0x23), PlaneRef::Top, vec![circle(0.0, 0.0, 0.4)])?;
    extrude(s, el, id(0x23), &g, id(0x24), &[v(0.0, 0.0)], (0.0, 0.2), BooleanOp::Add, &[SCREW_PART], false)?;
    s.run(&RenamePart { element: el, part: SCREW_PART, name: "Screw".into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![SCREW_PART], material: crate::material::library("Aluminum - 6061") })?;
    Ok(())
}

/// The planar face of `s` at height `z` (in) facing along `sign`·Z that has the circle `edge`.
fn face_of_rim(s: &Solid, edge: cadrs_sketch::EdgeName, sign: f64) -> Option<cadrs_sketch::FaceName> {
    edge.faces.iter().copied().find(|f| {
        s.faces.iter().position(|x| x.name == *f).is_some_and(|i| s.faces[i].plane.is_some() && s.face_normal(i).is_some_and(|n| n[2] * sign > 0.999))
    })
}

/// The centre connector of the circle `edge`, owned by its face along `sign`·Z.
fn rim_connector(instance: InstanceId, s: &Solid, c: [f64; 3], sign: f64) -> Result<MateConnector, CommandError> {
    let e = circle_edge(s, c, HOLE_D).ok_or_else(|| CommandError::Invalid(format!("no Ø{HOLE_D} rim at {c:?}")))?;
    let f = face_of_rim(s, e, sign).ok_or_else(|| CommandError::Invalid("no face at the rim".into()))?;
    let p = implicit_points(s, &EntityRef::Face(f))
        .into_iter()
        .find(|p| p.point == ImplicitPoint::CircleCenter(e))
        .ok_or_else(|| CommandError::Invalid("no centre point at the rim".into()))?;
    Ok(MateConnector::implicit(instance, &p))
}

/// The source part solids of the assembly's instances.
pub fn solids(doc: &Document) -> HashMap<InstanceId, Arc<Solid>> {
    let builds: HashMap<ElementId, Arc<crate::rebuild::Build>> = [FLANGE_STUDIO, SCREW_STUDIO]
        .into_iter()
        .filter_map(|e| Some((e, crate::rebuild::build(doc.element(e)?.features()))))
        .collect();
    let asm = doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).cloned().unwrap_or_default();
    assembly::occurrence_solids(doc, &asm, |e| builds.get(&e).cloned())
}

/// The seed mate's connectors: the screw's shank rim under its head (flipped if need be so its
/// Z is up), the centre hole's top rim.
pub fn seed_connectors(doc: &Document) -> Result<[MateConnector; 2], CommandError> {
    let s = solids(doc);
    let mut a = rim_connector(SCREW, &s[&SCREW], [0.0, 0.0, 0.0], -1.0)?;
    let b = rim_connector(FLANGE, &s[&FLANGE], [0.0, 0.0, THICKNESS], 1.0)?;
    // Z along the screw's axis, up, as the hole's.
    let (za, zb) = (a.local_frame(Some(&s[&SCREW])).z, b.local_frame(Some(&s[&FLANGE])).z);
    a.flip = za[2] * zb[2] < 0.0;
    Ok([a, b])
}

/// Adds "Flange Assembly": the fixed flange and the screw Fastened in its centre hole.
pub fn build_assembly(doc: &mut Document, h: &mut History) -> Result<(), CommandError> {
    h.execute(doc, &AddElement { id: ASSEMBLY, kind: NewElementKind::Assembly, name: Some("Flange Assembly".into()), after: None })?;
    let seated = Pose::translation([0.0, 0.0, THICKNESS * IN]);
    for (i, el, p, pose) in [(FLANGE, FLANGE_STUDIO, FLANGE_PART, Pose::IDENTITY), (SCREW, SCREW_STUDIO, SCREW_PART, seated)] {
        h.execute(doc, &InsertInstance { element: ASSEMBLY, instance: Instance::new(i, InstanceSource::Part { element: el, part: p }, pose) })?;
    }
    h.execute(doc, &SetInstancesFixed { element: ASSEMBLY, instances: vec![FLANGE], fixed: true })?;
    let [a, b] = seed_connectors(doc)?;
    let f = MateFeature::new(FASTENED_1, "Fastened 1", MateKind::Mate(Mate::new(MateType::Fastened, a, b)));
    let mut model = doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).cloned().unwrap_or_default();
    model.mates.push(f.clone());
    let sol = assembly::solve(&model, &solids(doc), &Default::default());
    if !sol.converged || !sol.changed(&model).is_empty() {
        return Err(CommandError::Invalid(format!("Fastened 1 doesn't hold where the screw is ({}, {:?})", sol.residual, sol.changed(&model))));
    }
    h.execute(doc, &AddMateFeature { element: ASSEMBLY, feature: f, poses: Vec::new() })
}

/// The stand-in document (`fixtures/flange_standin.cadrs`).
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Replicate (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_0100);
    doc.units = cadrs_sketch::units::Units { length: cadrs_sketch::units::LengthUnit::Inch, mass: cadrs_sketch::units::MassUnit::Pound, ..Default::default() };
    let mut h = History::default();
    for (el, name) in [(FLANGE_STUDIO, "Flange"), (SCREW_STUDIO, "Screw")] {
        let mut e = crate::document::Element::part_studio(name);
        e.id = el;
        doc.elements.push(e);
    }
    build_flange(&mut DocHistory(&mut doc, &mut h), FLANGE_STUDIO)?;
    build_screw(&mut DocHistory(&mut doc, &mut h), SCREW_STUDIO)?;
    build_assembly(&mut doc, &mut h)?;
    if let Some(k) = doc.elements.iter().position(|e| e.id == ASSEMBLY) {
        let a = doc.elements.remove(k);
        doc.elements.insert(0, a);
    }
    Ok(doc)
}

/// The stand-in as a document file.
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}

/// Replicate 1 (X16) through the commands: the screw and Fastened 1 copied onto every matching
/// hole of the flange ("all matching": the six bolt-circle holes). Returns the feature's id.
pub fn replicate(doc: &mut Document, h: &mut History) -> Result<MateId, CommandError> {
    let s = solids(doc);
    let asm = doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).cloned().ok_or(CommandError::ElementNotFound(ASSEMBLY))?;
    let flat = crate::assembly::structure::solver_model(doc, &asm);
    let seed = asm.mate(FASTENED_1).and_then(|f| f.mate()).ok_or_else(|| CommandError::Invalid("no Fastened 1".into()))?;
    let targets = crate::assembly::replicate::matching_targets(&s[&FLANGE], &seed.connectors[1]);
    let feature = MateId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_2002);
    let (r, instances) = crate::assembly::replicate::plan(
        &asm,
        &flat,
        &s,
        SCREW,
        FASTENED_1,
        targets,
        |k| InstanceId::from_u128(0x3b08_0000_0000_0000_0000_0000_0000_1100 + k as u128),
        feature,
    )?;
    let f = MateFeature::new(feature, "Replicate 1", MateKind::Replicate(r));
    h.execute(doc, &crate::assembly::replicate::SetReplicate { element: ASSEMBLY, feature: f, instances })?;
    Ok(feature)
}
