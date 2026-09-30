//! The "Creating Explicit Mate Connectors" stand-in (P3B.7, `intro-to-assemblies.md` A24): the
//! course's folding step stool isn't public, so the exercise runs on this two-frame stand-in,
//! built from our own features, in inch and pound as the course sets them (the Mass properties
//! panel's own unit is set to grams in the scenario, as the course's panel shows them;
//! `intro-to-assemblies-gaps.md`, "Ex4 Explicit Mate Connectors"). All parts Aluminum - 6061.
//!
//! - Part Studio **Large Frame Bar**: the bar, a box x −6…6, y 0…1, z 0…24 (Sketch 1 on Top,
//!   Extrude 1), and the **Hole Positions** sketch on Right: a Ø0.5 circle at (y −0.5, z 23), the
//!   hinge axis. Nothing uses the sketch, so it stays visible (A24.6).
//! - Part Studio **Base Frame Bar** (three parts, `ex4-step3.png`): **Base Frame Bar**, a plate x
//!   −5…5, y −1.5…−1, z 0…22 with two lugs x −5…−4.5 and 4.5…5, y −1.5…0, z 22…24 (Add), each
//!   with a Ø0.5 hinge hole on the axis (y −0.5, z 23) (Remove, a Right sketch extruded
//!   symmetric). The gap list's first spec had the lugs at y −1…0, touching the plate only along
//!   an edge, which the kernel keeps as separate bodies (so separate parts); here they stand on
//!   the plate's top face;
//!   **Cross Bar**, x −4.5…4.5, y −2.5…−1.5, z 4…5; **Back Foot**, a pad x −5.5…5.5, y −2…−0.5,
//!   z −0.5…0 (in the studio only, not inserted: the course's third part).
//! - Assembly **Step Stool Assembly**: Large Frame Bar <1> (fixed), Base Frame Bar <1>, Cross Bar
//!   <1> (Fastened 1 to the Base Frame Bar), every instance where its studio has it. The hinge
//!   between the frames is the one mate missing (A24.2).
//!
//! The exercise's steps A24.4–A24.10 ([`add_connectors_and_hinge`]): **Mate connector 1** in the
//! Base Frame Bar studio, Between entities (the lug hole's inner edge at x −4.5 and the other
//! lug's inner face at x 4.5; owner Base Frame Bar) at (0, −0.5, 23); **Mate connector 1** in the
//! Large Frame Bar studio, On entity (the Hole Positions circle; owner Large Frame Bar), the
//! same point; both with Z along +X. **Revolute 1** joins them (Large Frame Bar's first), limits
//! −42.5°…0°: the base frame swings towards −Y.
//!
//! Every id is fixed, so `fixtures/step_stool_standin.cadrs` is regenerated exactly
//! (`cadrs_core/tests/course_assemblies.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::assembly::commands::{AddMateFeature, InsertInstance, SetInstancesFixed};
use crate::assembly::connector::{ConnectorFrame, EntityRef, ImplicitPoint, MateConnector, implicit_points};
use crate::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateLimits, MateOffset, MateType};
use crate::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddElement, AddExtrude, AddFeature, AddSketch, EditSketch, NewElementKind, RenameFeature, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, ExtrudeFeature, FeatureKind, Offset};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::mate::{ConnectorOrigin, MateConnectorFeature, OriginType};
use crate::solid::Solid;

use super::gear_cover::{DocHistory, Studio};

/// mm per inch.
pub const IN: f64 = 25.4;

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_0000 + n)
}

/// The Part Studios and the Assembly.
pub const LARGE_STUDIO: ElementId = ElementId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_0101);
pub const BASE_STUDIO: ElementId = ElementId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_0102);
pub const ASSEMBLY: ElementId = ElementId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_0201);

pub const LARGE_FRAME_BAR: PartId = PartId::new(id(0x12), 0);
pub const BASE_FRAME_BAR: PartId = PartId::new(id(0x22), 0);
pub const CROSS_BAR: PartId = PartId::new(id(0x32), 0);
pub const BACK_FOOT: PartId = PartId::new(id(0x42), 0);
/// The Hole Positions sketch (Large Frame Bar studio).
pub const HOLE_POSITIONS: FeatureId = id(0x13);
/// The connectors A24.5 and A24.8 add (as [`add_connectors_and_hinge`] adds them).
pub const BASE_CONNECTOR: FeatureId = id(0x51);
pub const LARGE_CONNECTOR: FeatureId = id(0x52);

const fn inst(n: u128) -> InstanceId {
    InstanceId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_1000 + n)
}

pub const LARGE: InstanceId = inst(1);
pub const BASE: InstanceId = inst(2);
pub const CROSS: InstanceId = inst(3);

pub const FASTENED_1: MateId = MateId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_2001);
pub const HINGE: MateId = MateId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_2002);

/// The hinge axis: (y, z) in inches, along X.
pub const HINGE_AXIS: [f64; 2] = [-0.5, 23.0];
/// The hinge's opening limit (degrees).
pub const OPEN_ANGLE: f64 = -42.5;

pub(crate) fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x * IN, y * IN)
}

pub(crate) fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], closed: true, construction: false, label: "Add rectangle" }
}

pub(crate) fn circle(x: f64, y: f64, d: f64) -> SketchOp {
    SketchOp::AddCircle { center: v(x, y), radius: d / 2.0 * IN, construction: false }
}

pub(crate) fn sketch(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, plane: PlaneRef, ops: Vec<SketchOp>) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(plane) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::Batch(ops) })?;
    s.document()
        .element(el)
        .and_then(|x| x.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

/// Extrudes the regions of `sketch` under `seeds` (in), from `z0` to `z1` in along the plane's
/// normal (or `z1 − z0` symmetric about the plane when `symmetric`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn extrude(
    s: &mut dyn Studio,
    el: ElementId,
    sketch: FeatureId,
    g: &cadrs_sketch::Sketch,
    feature: FeatureId,
    seeds: &[Vec2],
    z: (f64, f64),
    op: BooleanOp,
    scope: &[PartId],
    symmetric: bool,
) -> Result<(), CommandError> {
    let regions = super::region_refs(sketch, g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid(format!("a region of the step stool stand-in is missing ({feature:?})")));
    }
    let (z0, z1) = z;
    let mut x = super::extrude_of(regions, (z1 - z0) * IN);
    x.depth_expr = format!("{} in", z1 - z0);
    x.op = op;
    x.merge_scope = scope.to_vec();
    x.symmetric = symmetric;
    if z0 != 0.0 {
        x.start_offset = Some(Offset { value: z0.abs() * IN, expr: format!("{} in", z0.abs()), flip: z0 < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: x, label: "Extrude".into() })?;
    Ok(())
}

/// The Large Frame Bar studio's features.
pub fn build_large(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let g = sketch(s, el, id(0x11), PlaneRef::Top, vec![rect(-6.0, 0.0, 6.0, 1.0)])?;
    extrude(s, el, id(0x11), &g, id(0x12), &[v(0.0, 0.5)], (0.0, 24.0), BooleanOp::New, &[], false)?;
    // Right: sketch x = model Y, y = model Z.
    sketch(s, el, HOLE_POSITIONS, PlaneRef::Right, vec![circle(HINGE_AXIS[0], HINGE_AXIS[1], 0.5)])?;
    s.run(&RenameFeature { element: el, feature: HOLE_POSITIONS, name: "Hole Positions".into() })?;
    s.run(&RenamePart { element: el, part: LARGE_FRAME_BAR, name: "Large Frame Bar".into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![LARGE_FRAME_BAR], material: crate::material::library("Aluminum - 6061") })?;
    Ok(())
}

/// The Base Frame Bar studio's features (Base Frame Bar, Cross Bar, Back Foot).
pub fn build_base(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New, Remove};
    let g = sketch(s, el, id(0x21), PlaneRef::Top, vec![rect(-5.0, -1.5, 5.0, -1.0)])?;
    extrude(s, el, id(0x21), &g, id(0x22), &[v(0.0, -1.25)], (0.0, 22.0), New, &[], false)?;
    let g = sketch(s, el, id(0x23), PlaneRef::Top, vec![rect(-5.0, -1.5, -4.5, 0.0), rect(4.5, -1.5, 5.0, 0.0)])?;
    extrude(s, el, id(0x23), &g, id(0x24), &[v(-4.75, -0.75), v(4.75, -0.75)], (22.0, 24.0), Add, &[BASE_FRAME_BAR], false)?;
    // The hinge holes: a Right sketch cut symmetric through both lugs.
    let g = sketch(s, el, id(0x25), PlaneRef::Right, vec![circle(HINGE_AXIS[0], HINGE_AXIS[1], 0.5)])?;
    extrude(s, el, id(0x25), &g, id(0x26), &[v(HINGE_AXIS[0], HINGE_AXIS[1])], (0.0, 12.0), Remove, &[BASE_FRAME_BAR], true)?;
    let g = sketch(s, el, id(0x31), PlaneRef::Top, vec![rect(-4.5, -2.5, 4.5, -1.5)])?;
    extrude(s, el, id(0x31), &g, id(0x32), &[v(0.0, -2.0)], (4.0, 5.0), New, &[], false)?;
    let g = sketch(s, el, id(0x41), PlaneRef::Top, vec![rect(-5.5, -2.0, 5.5, -0.5)])?;
    extrude(s, el, id(0x41), &g, id(0x42), &[v(0.0, -1.25)], (-0.5, 0.0), New, &[], false)?;
    for (part, name) in [(BASE_FRAME_BAR, "Base Frame Bar"), (CROSS_BAR, "Cross Bar"), (BACK_FOOT, "Back Foot")] {
        s.run(&RenamePart { element: el, part, name: name.into() })?;
    }
    s.run(&SetPartMaterial {
        element: el,
        parts: vec![BASE_FRAME_BAR, CROSS_BAR, BACK_FOOT],
        material: crate::material::library("Aluminum - 6061"),
    })?;
    Ok(())
}

/// The implicit connector of the planar face of `s` whose centroid is at `c` (in).
fn face_centroid(s: &Solid, c: [f64; 3]) -> Result<crate::assembly::connector::ImplicitConnector, CommandError> {
    s.faces
        .iter()
        .find_map(|f| {
            implicit_points(s, &EntityRef::Face(f.name))
                .into_iter()
                .find(|p| matches!(p.point, ImplicitPoint::FaceCentroid(_)) && (0..3).all(|i| (p.frame.origin[i] / IN - c[i]).abs() < 1e-6))
        })
        .ok_or_else(|| CommandError::Invalid(format!("no face centred at {c:?}")))
}

/// The source part solids of the assembly's instances.
pub fn solids(doc: &Document) -> HashMap<InstanceId, Arc<Solid>> {
    let builds: HashMap<ElementId, Arc<crate::rebuild::Build>> = [LARGE_STUDIO, BASE_STUDIO]
        .into_iter()
        .filter_map(|e| Some((e, crate::rebuild::build(doc.element(e)?.features()))))
        .collect();
    let asm = doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).cloned().unwrap_or_default();
    assembly::source_solids(&asm, |e| builds.get(&e).cloned())
}

/// Where the base frame (and its Cross Bar) starts before the hinge (P3B.7 judge): turned 8°
/// about the hinge axis and moved 1.5 in back and 1 in down, so accepting the Revolute visibly
/// snaps it into place.
pub fn base_start() -> Pose {
    let axis = [0.0, HINGE_AXIS[0] * IN, HINGE_AXIS[1] * IN];
    Pose::rotation_about(axis, [1.0, 0.0, 0.0], 8f64.to_radians()).then(&Pose::translation([0.0, -1.5 * IN, -IN]))
}

/// Adds "Step Stool Assembly" (A24.2: pre-mated except the hinge).
pub fn build_assembly(doc: &mut Document, h: &mut History) -> Result<(), CommandError> {
    h.execute(doc, &AddElement { id: ASSEMBLY, kind: NewElementKind::Assembly, name: Some("Step Stool Assembly".into()), after: None })?;
    for (i, el, p, pose) in [
        (LARGE, LARGE_STUDIO, LARGE_FRAME_BAR, Pose::IDENTITY),
        (BASE, BASE_STUDIO, BASE_FRAME_BAR, base_start()),
        (CROSS, BASE_STUDIO, CROSS_BAR, base_start()),
    ] {
        let source = InstanceSource::Part { element: el, part: p };
        h.execute(doc, &InsertInstance { element: ASSEMBLY, instance: Instance::new(i, source, pose) })?;
    }
    h.execute(doc, &SetInstancesFixed { element: ASSEMBLY, instances: vec![LARGE], fixed: true })?;
    // Fastened 1: the Cross Bar's face against the frame (y −1.5, Z +Y), flipped, on the
    // centroid of the frame's back face (y −1.5, Z −Y), offset to where the Cross Bar is.
    let s = solids(doc);
    let mut c1 = MateConnector::implicit(CROSS, &face_centroid(&s[&CROSS], [0.0, -1.5, 4.5])?);
    c1.flip = true;
    let back = s[&BASE]
        .faces
        .iter()
        .enumerate()
        .filter(|(i, f)| f.plane.is_some() && s[&BASE].face_normal(*i).is_some_and(|n| n[1] < -0.999))
        .flat_map(|(_, f)| implicit_points(&s[&BASE], &EntityRef::Face(f.name)))
        .find(|p| matches!(p.point, ImplicitPoint::FaceCentroid(_)) && (p.frame.origin[1] / IN + 1.5).abs() < 1e-6)
        .ok_or_else(|| CommandError::Invalid("no back face on the Base Frame Bar".into()))?;
    let c2 = MateConnector::implicit(BASE, &back);
    let f2 = c2.local_frame(Some(&s[&BASE]));
    let d = [0, 1, 2].map(|k| c1.local_frame(Some(&s[&CROSS])).origin[k] - f2.origin[k]);
    let along = |a: [f64; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
    let mut m = Mate::new(MateType::Fastened, c1, c2);
    // (Both frames are in their parts' own coordinates; the two instances start at the same
    // placement, so the offset is the same in the assembly.)
    m.offset = Some(MateOffset { translation: [along(f2.x), along(f2.y()), along(f2.z)], ..Default::default() });
    let f = MateFeature::new(FASTENED_1, "Fastened 1", MateKind::Mate(m));
    // Check that it holds where the parts are.
    let mut model = doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).cloned().unwrap_or_default();
    model.mates.push(f.clone());
    let sol = assembly::solve(&model, &s, &Default::default());
    if !sol.converged || !sol.changed(&model).is_empty() {
        return Err(CommandError::Invalid(format!("Fastened 1 doesn't hold where the parts are ({})", sol.residual)));
    }
    h.execute(doc, &AddMateFeature { element: ASSEMBLY, feature: f, poses: Vec::new() })
}

/// The stand-in document (`fixtures/step_stool_standin.cadrs`): "Creating Mate Connectors
/// (stand-in)", inch and pound (A24.1).
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Creating Mate Connectors (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_0100);
    doc.units = cadrs_sketch::units::Units {
        length: cadrs_sketch::units::LengthUnit::Inch,
        mass: cadrs_sketch::units::MassUnit::Pound,
        ..Default::default()
    };
    let mut h = History::default();
    for (el, name) in [(LARGE_STUDIO, "Large Frame Bar"), (BASE_STUDIO, "Base Frame Bar")] {
        let mut e = crate::document::Element::part_studio(name);
        e.id = el;
        doc.elements.push(e);
    }
    build_large(&mut DocHistory(&mut doc, &mut h), LARGE_STUDIO)?;
    build_base(&mut DocHistory(&mut doc, &mut h), BASE_STUDIO)?;
    build_assembly(&mut doc, &mut h)?;
    // The assembly tab first, as the course's document has it.
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

/// The edge of `s` that is a circle of diameter `d` (in) centred at `c` (in).
pub fn circle_edge(s: &Solid, c: [f64; 3], d: f64) -> Option<cadrs_sketch::EdgeName> {
    s.edges
        .iter()
        .find(|e| e.circle.is_some_and(|k| (k.radius * 2.0 / IN - d).abs() < 1e-6 && (0..3).all(|i| (k.center[i] / IN - c[i]).abs() < 1e-6)))
        .map(|e| e.name)
}

/// The planar face of `s` whose plane is `x = x0` (in) and whose outward normal is along
/// `sign`·X.
pub fn x_face(s: &Solid, x0: f64, sign: f64) -> Option<cadrs_sketch::FaceName> {
    s.faces
        .iter()
        .enumerate()
        .find(|(i, f)| {
            f.plane.is_some()
                && s.face_normal(*i).is_some_and(|n| n[0] * sign > 0.999)
                && f.loops.iter().flatten().all(|p| (p[0] / IN - x0).abs() < 1e-6)
        })
        .map(|(_, f)| f.name)
}

/// A24.4–A24.10 through the commands: the two explicit connectors, then Revolute 1 between them
/// with limits −42.5°…0° (at 0°).
pub fn add_connectors_and_hinge(doc: &mut Document, h: &mut History) -> Result<(), CommandError> {
    use crate::document::{EdgeRef, FaceRef};
    // A24.5: Between entities in the Base Frame Bar studio.
    let base_build = crate::rebuild::build(doc.element(BASE_STUDIO).ok_or(CommandError::ElementNotFound(BASE_STUDIO))?.features());
    let bar = &base_build.part(BASE_FRAME_BAR).ok_or_else(|| CommandError::Invalid("no Base Frame Bar".into()))?.solid;
    let rim = circle_edge(bar, [-4.5, HINGE_AXIS[0], HINGE_AXIS[1]], 0.5).ok_or_else(|| CommandError::Invalid("no hinge hole rim".into()))?;
    let face = x_face(bar, 4.5, -1.0).ok_or_else(|| CommandError::Invalid("no inner lug face".into()))?;
    let seed = bar.face(&face).and_then(|f| f.loops.first()?.first().copied()).unwrap_or([0.0; 3]);
    let between = MateConnectorFeature {
        origin_type: OriginType::BetweenEntities,
        origin: Some(ConnectorOrigin::Edge(EdgeRef { part: BASE_FRAME_BAR, edge: rim, seed: bar.edge(&rim).map(|e| e.midpoint()).unwrap_or([0.0; 3]) })),
        between: Some(ConnectorOrigin::Face(FaceRef { part: BASE_FRAME_BAR, face, seed })),
        owner_on: true,
        owner: Some(BASE_FRAME_BAR),
        move_on: false,
        ..MateConnectorFeature::default()
    };
    h.execute(doc, &AddFeature { element: BASE_STUDIO, feature: BASE_CONNECTOR, base_name: "Mate connector".into(), kind: FeatureKind::MateConnector(between) })?;
    // A24.8: On entity, the Hole Positions circle.
    let curve = doc
        .element(LARGE_STUDIO)
        .and_then(|e| e.feature(HOLE_POSITIONS))
        .and_then(|f| f.sketch())
        .and_then(|s| s.geometry.curves.keys().next())
        .ok_or_else(|| CommandError::Invalid("no Hole Positions circle".into()))?;
    let on = MateConnectorFeature {
        origin: Some(ConnectorOrigin::SketchCurve { sketch: HOLE_POSITIONS, curve }),
        owner_on: true,
        owner: Some(LARGE_FRAME_BAR),
        move_on: false,
        ..MateConnectorFeature::default()
    };
    h.execute(doc, &AddFeature { element: LARGE_STUDIO, feature: LARGE_CONNECTOR, base_name: "Mate connector".into(), kind: FeatureKind::MateConnector(on) })?;
    // A24.10: Revolute 1 between them, the Large Frame Bar's first.
    let s = solids(doc);
    let frame = |i: InstanceId, f: FeatureId| -> Result<ConnectorFrame, CommandError> {
        let c = s[&i].connectors.iter().find(|c| c.feature == f).ok_or_else(|| CommandError::Invalid(format!("{f:?} isn't on its part")))?;
        Ok(ConnectorFrame::new(c.frame.origin, c.frame.normal(), c.frame.u))
    };
    let a = MateConnector::explicit(LARGE, LARGE_CONNECTOR, frame(LARGE, LARGE_CONNECTOR)?);
    let b = MateConnector::explicit(BASE, BASE_CONNECTOR, frame(BASE, BASE_CONNECTOR)?);
    let mut m = Mate::new(MateType::Revolute, a, b);
    m.limits = Some(MateLimits { angle: Some((OPEN_ANGLE.to_radians(), 0.0)), ..Default::default() });
    let f = MateFeature::new(HINGE, "Revolute 1", MateKind::Mate(m));
    let mut model = doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).cloned().unwrap_or_default();
    model.mates.push(f.clone());
    let sol = assembly::solve(&model, &s, &crate::assembly::solver::SolveOptions { movers: vec![BASE], snap: Some(HINGE), ..Default::default() });
    if !sol.converged {
        return Err(CommandError::Invalid(format!("the hinge doesn't solve ({})", sol.residual)));
    }
    let poses = sol.changed(&model);
    h.execute(doc, &AddMateFeature { element: ASSEMBLY, feature: f, poses })
}

/// The stand-in after A24.4–A24.10 (`fixtures/step_stool_hinged.cadrs`): both connectors and
/// Revolute 1, at 0° (the Named positions scenario starts here, P3B.8).
pub fn hinged_document() -> Result<Document, CommandError> {
    let mut doc = document()?;
    let mut h = History::default();
    add_connectors_and_hinge(&mut doc, &mut h)?;
    doc.name = "Step Stool (stand-in, hinged)".into();
    doc.id = crate::ids::DocumentId::from_u128(0x3b07_0000_0000_0000_0000_0000_0000_0102);
    Ok(doc)
}
