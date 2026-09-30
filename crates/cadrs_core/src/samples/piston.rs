//! The piston and hexapod stand-ins of the Linked Documents course (P3G.3, reused by P3G.5;
//! `derived-and-linking-gaps.md` "What each exercise needs", ER Ex1 and ER Ex2).
//!
//! Onshape's public "Linked Document- Piston", "Linked Document- Hexapod" and "Exercise: Move to
//! Document" can't be copied, so these are built through the command layer with fixed ids, in
//! mm, every part centred on its axis:
//!
//! - **Pneumatic Piston** (Part Studio), axis Z, Aluminum - 6061:
//!   - **UJoint**: a 10 × 10 × 10 cube, z −10..0 (1 000 mm³);
//!   - **Cylinder**: **Main Sketch** (Top, a Ø15.725 circle) extruded 60, z 0..60
//!     (60π·7.8625² = 11 652.588 mm³);
//!   - **Rod**: **Rod Sketch** (a Ø6 circle) extruded from z 60 for L = **28**, z 60..88
//!     (9πL = 791.681 mm³). The course revolves a 3 × L rectangle about Z: the same Ø6 × L
//!     cylinder, so the same volume and centroid; the stand-in extrudes it, and L is the
//!     extrude's depth;
//!   - **Eye**: an 8 × 8 square extruded 8 on the Rod's top, z 88..96 (512 mm³).
//!
//!   V_piston = 1 000 + 60π(D/2)² + 9πL + 512 = **13 956.271 mm³** (D 15.725, L 28), centroid
//!   (0, 0, 32.263) = (1 000·(−5) + 11 652.588·30 + 791.681·74 + 512·92) / 13 956.271.
//! - **Piston Assembly**: the four parts at their studio positions (UJoint fixed). The course
//!   mates them (Fastened); the stand-in places them, so its mass properties are fixed.
//! - **Baseplate** Ø200 × 10 (z 0..10) and **Topplate** Ø160 × 10, each with six Ø10 holes on
//!   a Ø140 circle at 0°, 60°, …: 98 500π = 309 446.876 and 62 500π = 196 349.541 mm³.
//! - **Hexapod**: Baseplate <1> (fixed); six Piston Assembly instances, the k-th at
//!   (70 cos 60k°, 70 sin 60k°, 20) (the UJoint's bottom on the baseplate hole's top edge, so it
//!   spans z 10..20); Topplate <1> at z 88 + L = 116 (on the Eyes' tops). The course mates
//!   them (Revolute, Fastened); the stand-in places them.
//!
//!   V = 309 446.876 + 196 349.541 + 6 · 13 956.271 = **589 534.041 mm³**, centroid
//!   **(0, 0, 50.348)** (6-fold symmetric).
//!
//! P3G.5: **ER Ex1**'s two documents are [`piston_document`] ("Linked Document- Piston
//! (stand-in)": Pneumatic Piston and Piston Assembly, `fixtures/linked_piston_standin.cadrs`)
//! and [`project_document`] ("Linked Documents- Project (stand-in)": the Hexapod with the two
//! plates only, `fixtures/linked_hexapod_standin.cadrs`); [`revolute_piston`],
//! [`fasten_topplate`] and [`complete_hexapod`] make the course's mates on implicit connectors,
//! and [`set_diameter`] / [`set_rod_length`] its V2 edits (Ø20, L 75).
//!
//! [`move_to_document`] is **ER Ex2**'s one document with the tabs Hexapod, Pneumatic Piston,
//! Piston Assembly, Topplate and Baseplate (all same-document workspace references):
//! `fixtures/move_to_document_standin.cadrs`.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::assembly::commands::{InsertInstance, SetInstancesFixed};
use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddSketch, EditSketch, RenameFeature, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, Element, ExtrudeFeature, Offset};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};

use super::gear_cover::{DocHistory, Studio};

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_0000 | n)
}

const fn eid(n: u128) -> ElementId {
    ElementId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_0100 | n)
}

const fn iid(n: u128) -> InstanceId {
    InstanceId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_1000 | n)
}

/// ER Ex2's document ("Exercise: Move to Document", stand-in).
pub const MOVE_DOCUMENT: DocumentId = DocumentId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_0001);
pub const MOVE_DOCUMENT_NAME: &str = "Exercise: Move to Document";

/// The tabs.
pub const HEXAPOD: ElementId = eid(1);
pub const PISTON_STUDIO: ElementId = eid(2);
pub const PISTON_ASSEMBLY: ElementId = eid(3);
pub const TOPPLATE: ElementId = eid(4);
pub const BASEPLATE: ElementId = eid(5);

pub const HEXAPOD_NAME: &str = "Hexapod";
pub const PISTON_STUDIO_NAME: &str = "Pneumatic Piston";
pub const PISTON_ASSEMBLY_NAME: &str = "Piston Assembly";
pub const TOPPLATE_NAME: &str = "Topplate";
pub const BASEPLATE_NAME: &str = "Baseplate";

/// The piston's features.
pub const UJOINT_SKETCH: FeatureId = fid(0x11);
pub const UJOINT_EXTRUDE: FeatureId = fid(0x12);
pub const MAIN_SKETCH: FeatureId = fid(0x21);
pub const CYLINDER_EXTRUDE: FeatureId = fid(0x22);
pub const ROD_SKETCH: FeatureId = fid(0x31);
pub const ROD_EXTRUDE: FeatureId = fid(0x32);
pub const EYE_SKETCH: FeatureId = fid(0x41);
pub const EYE_EXTRUDE: FeatureId = fid(0x42);
const BASE_SKETCH: FeatureId = fid(0x51);
const BASE_EXTRUDE: FeatureId = fid(0x52);
const TOP_SKETCH: FeatureId = fid(0x61);
const TOP_EXTRUDE: FeatureId = fid(0x62);

pub const UJOINT: PartId = PartId::new(UJOINT_EXTRUDE, 0);
pub const CYLINDER: PartId = PartId::new(CYLINDER_EXTRUDE, 0);
pub const ROD: PartId = PartId::new(ROD_EXTRUDE, 0);
pub const EYE: PartId = PartId::new(EYE_EXTRUDE, 0);
pub const BASE_PART: PartId = PartId::new(BASE_EXTRUDE, 0);
pub const TOP_PART: PartId = PartId::new(TOP_EXTRUDE, 0);

/// The Piston Assembly's instances (UJoint, Cylinder, Rod, Eye).
pub const PISTON_INSTANCES: [InstanceId; 4] = [iid(1), iid(2), iid(3), iid(4)];
/// The Hexapod's Baseplate and Topplate instances.
pub const BASE_INSTANCE: InstanceId = iid(0x11);
pub const TOP_INSTANCE: InstanceId = iid(0x12);
/// The Hexapod's six Piston Assembly instances.
pub const fn piston(k: usize) -> InstanceId {
    iid(0x21 + k as u128)
}

/// mm.
pub const UJOINT_SIDE: f64 = 10.0;
/// The cylinder's diameter (V1; the course edits it to Ø20 for V2).
pub const CYLINDER_D: f64 = 15.725;
pub const CYLINDER_L: f64 = 60.0;
pub const ROD_D: f64 = 6.0;
/// The rod's length (V1; 75 for V2).
pub const ROD_L: f64 = 28.0;
pub const EYE_SIDE: f64 = 8.0;
pub const BASE_D: f64 = 200.0;
pub const TOP_D: f64 = 160.0;
pub const PLATE_T: f64 = 10.0;
pub const HOLE_D: f64 = 10.0;
/// The holes' circle diameter.
pub const HOLE_CIRCLE_D: f64 = 140.0;

/// A sketch on Top of `ops` (named `name` when given) and an extrude (New) of the region at
/// `seed` from z0 to z1.
#[allow(clippy::too_many_arguments)]
fn feature(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, extrude: FeatureId, ops: Vec<SketchOp>, seed: Vec2, z: (f64, f64), name: Option<&str>) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::Batch(ops) })?;
    if let Some(n) = name {
        s.run(&RenameFeature { element: el, feature: sketch, name: n.into() })?;
    }
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sketch, &g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("a region of the piston stand-in is missing".into()));
    }
    let (z0, z1) = z;
    let fmt = |x: f64| format!("{} mm", (x * 1000.0).round() / 1000.0);
    let mut x = super::extrude_of(regions, z1 - z0);
    x.depth_expr = fmt(z1 - z0);
    x.op = BooleanOp::New;
    if z0 != 0.0 {
        x.start_offset = Some(Offset { value: z0.abs(), expr: fmt(z0.abs()), flip: z0 < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature: extrude, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: extrude, extrude: x, label: "Extrude".into() })?;
    Ok(())
}

fn square(h: f64) -> SketchOp {
    SketchOp::AddPolyline { points: vec![Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)], closed: true, construction: false, label: "Add rectangle" }
}

fn circle(x: f64, y: f64, d: f64) -> SketchOp {
    SketchOp::AddCircle { center: Vec2::new(x, y), radius: d / 2.0, construction: false }
}

/// The six holes' centres (on the Ø140 circle at 0°, 60°, …).
pub fn hole_centres() -> [(f64, f64); 6] {
    let r = HOLE_CIRCLE_D / 2.0;
    std::array::from_fn(|k| {
        let a = (60.0 * k as f64).to_radians();
        (r * a.cos(), r * a.sin())
    })
}

/// Builds the Pneumatic Piston's four parts in the Part Studio `el` (rod length `rod_l`).
///
/// P3G.5: the Main Sketch's circle is centred on the origin with its **Ø15.725** diameter
/// dimension (edited to Ø20 in ER Ex1 step 12), and the Eye is sketched on the Rod's top face,
/// so it follows the rod when its length (Extrude 3's depth, 28 → 75 in step 13) changes.
pub fn build_piston(s: &mut dyn Studio, el: ElementId, rod_l: f64) -> Result<(), CommandError> {
    let top = CYLINDER_L + rod_l;
    feature(s, el, UJOINT_SKETCH, UJOINT_EXTRUDE, vec![square(UJOINT_SIDE / 2.0)], Vec2::new(0.0, 0.0), (-UJOINT_SIDE, 0.0), None)?;
    feature(s, el, MAIN_SKETCH, CYLINDER_EXTRUDE, vec![circle(0.0, 0.0, CYLINDER_D)], Vec2::new(0.0, 0.0), (0.0, CYLINDER_L), Some("Main Sketch"))?;
    dimension_main_sketch(s, el)?;
    feature(s, el, ROD_SKETCH, ROD_EXTRUDE, vec![circle(0.0, 0.0, ROD_D)], Vec2::new(0.0, 0.0), (CYLINDER_L, top), Some("Rod Sketch"))?;
    eye_on_rod(s, el)?;
    for (p, n) in [(UJOINT, "UJoint"), (CYLINDER, "Cylinder"), (ROD, "Rod"), (EYE, "Eye")] {
        s.run(&RenamePart { element: el, part: p, name: n.into() })?;
    }
    s.run(&SetPartMaterial { element: el, parts: vec![UJOINT, CYLINDER, ROD, EYE], material: crate::material::library("Aluminum - 6061") })?;
    Ok(())
}

/// The Main Sketch's circle: its centre on the origin and its diameter dimension.
fn dimension_main_sketch(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use cadrs_sketch::constraint::{ConstraintOf, PointSpec};
    use cadrs_sketch::{CurveKind, Dimension, DimensionKind};
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(MAIN_SKETCH))
        .and_then(|f| f.sketch())
        .map(|k| k.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("Main Sketch not found".into()))?;
    let curve = g
        .curves
        .iter()
        .find_map(|(id, k)| matches!(k.kind, CurveKind::Circle { .. }).then_some(id))
        .ok_or_else(|| CommandError::Invalid("Main Sketch has no circle".into()))?;
    let ops = vec![
        SketchOp::AddConstraints(vec![ConstraintOf::Coincident(PointSpec::At(Vec2::new(0.0, 0.0)), PointSpec::Origin)]),
        SketchOp::SetDimension {
            dimension: Dimension { kind: DimensionKind::Diameter { curve }, value: CYLINDER_D, offset: std::f64::consts::FRAC_PI_4, along: 0.0, driven: false },
            moves: vec![],
            radii: vec![],
        },
    ];
    s.run(&EditSketch { element: el, feature: MAIN_SKETCH, op: SketchOp::Batch(ops) })
}

/// The Eye: an 8 × 8 square sketched on the Rod's top face, centred on the axis, extruded 8.
fn eye_on_rod(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let features = s.document().element(el).map(|e| e.features().to_vec()).unwrap_or_default();
    let top = crate::parts::cap_name(&features, ROD_EXTRUDE, 0, true).ok_or_else(|| CommandError::Invalid("the Rod has no top face".into()))?;
    let plane = crate::parts::face_plane(&features, ROD_EXTRUDE, top).ok_or_else(|| CommandError::Invalid("the Rod's top face can't carry a sketch".into()))?;
    let f = plane.frame();
    // The axis's point on the (level) face, in the face's sketch coordinates.
    let d = [-f.origin[0], -f.origin[1], 0.0];
    let (cx, cy) = (d[0] * f.u[0] + d[1] * f.u[1] + d[2] * f.u[2], d[0] * f.v[0] + d[1] * f.v[1] + d[2] * f.v[2]);
    let h = EYE_SIDE / 2.0;
    let sq = SketchOp::AddPolyline {
        points: vec![Vec2::new(cx - h, cy - h), Vec2::new(cx + h, cy - h), Vec2::new(cx + h, cy + h), Vec2::new(cx - h, cy + h)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    };
    s.run(&AddSketch { element: el, feature: EYE_SKETCH, plane: Some(plane) })?;
    // The rod's Ø6 top edge would split the square into two regions; the Eye is the whole square.
    s.run(&crate::commands::SetSketchImprinting { element: el, feature: EYE_SKETCH, disable_imprinting: true })?;
    s.run(&EditSketch { element: el, feature: EYE_SKETCH, op: SketchOp::Batch(vec![sq]) })?;
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(EYE_SKETCH))
        .and_then(|f| f.sketch())
        .map(|k| k.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("the Eye's sketch not found".into()))?;
    let regions = super::region_refs(EYE_SKETCH, &g, &[Vec2::new(cx, cy)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("the Eye's region is missing".into()));
    }
    let mut x = super::extrude_of(regions, EYE_SIDE);
    x.op = BooleanOp::New;
    s.run(&AddExtrude { element: el, feature: EYE_EXTRUDE, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: EYE_EXTRUDE, extrude: x, label: "Extrude".into() })?;
    Ok(())
}

/// ER Ex1 step 12: the Main Sketch's diameter dimension set to `d` (Ø15.725 → Ø20).
pub fn set_diameter(s: &mut dyn Studio, el: ElementId, d: f64) -> Result<(), CommandError> {
    use cadrs_sketch::DimensionKind;
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(MAIN_SKETCH))
        .and_then(|f| f.sketch())
        .map(|k| k.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("Main Sketch not found".into()))?;
    let id = g
        .dimensions
        .iter()
        .find_map(|(id, x)| matches!(x.kind, DimensionKind::Diameter { .. }).then_some(id))
        .ok_or_else(|| CommandError::Invalid("Main Sketch has no diameter".into()))?;
    s.run(&EditSketch { element: el, feature: MAIN_SKETCH, op: SketchOp::SetDimensionValue { id, value: d } })
}

/// ER Ex1 step 13 on the stand-in: the Rod's length (Extrude 3's depth) set to `l` (28 → 75).
pub fn set_rod_length(s: &mut dyn Studio, el: ElementId, l: f64) -> Result<(), CommandError> {
    let mut e = s
        .document()
        .element(el)
        .and_then(|x| x.feature(ROD_EXTRUDE))
        .and_then(|f| f.extrude())
        .cloned()
        .ok_or_else(|| CommandError::Invalid("the Rod's extrude not found".into()))?;
    e.depth = l;
    e.depth_expr = format!("{l} mm");
    s.run(&SetExtrude { element: el, feature: ROD_EXTRUDE, extrude: e, label: "Extrude".into() })
}

/// Builds a Ø`d` × 10 plate with the six holes in the Part Studio `el`.
pub fn build_plate(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, extrude: FeatureId, d: f64, name: &str) -> Result<(), CommandError> {
    let mut ops = vec![circle(0.0, 0.0, d)];
    ops.extend(hole_centres().into_iter().map(|(x, y)| circle(x, y, HOLE_D)));
    feature(s, el, sketch, extrude, ops, Vec2::new(0.0, 0.0), (0.0, PLATE_T), None)?;
    s.run(&RenamePart { element: el, part: PartId::new(extrude, 0), name: name.into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![PartId::new(extrude, 0)], material: crate::material::library("Aluminum - 6061") })?;
    Ok(())
}

/// Places the Piston Assembly's four parts of the studio `studio` in the assembly `asm`.
pub fn build_piston_assembly(s: &mut dyn Studio, asm: ElementId, studio: ElementId) -> Result<(), CommandError> {
    for (i, p) in PISTON_INSTANCES.into_iter().zip([UJOINT, CYLINDER, ROD, EYE]) {
        s.run(&InsertInstance { element: asm, instance: Instance::new(i, InstanceSource::Part { element: studio, part: p }, Pose::IDENTITY) })?;
    }
    s.run(&SetInstancesFixed { element: asm, instances: vec![PISTON_INSTANCES[0]], fixed: true })?;
    Ok(())
}

/// The k-th piston's placement in the Hexapod.
pub fn piston_pose(k: usize) -> Pose {
    let (x, y) = hole_centres()[k];
    Pose::translation([x, y, PLATE_T + UJOINT_SIDE])
}

/// Places the Hexapod in `asm`: the baseplate (fixed), six instances of `piston_source` (an
/// assembly, here or linked) and the topplate for a rod of length `rod_l`.
pub fn build_hexapod(s: &mut dyn Studio, asm: ElementId, base: InstanceSource, top: InstanceSource, piston_source: InstanceSource, rod_l: f64) -> Result<(), CommandError> {
    s.run(&InsertInstance { element: asm, instance: Instance::new(BASE_INSTANCE, base, Pose::IDENTITY) })?;
    s.run(&InsertInstance { element: asm, instance: Instance::new(TOP_INSTANCE, top, Pose::translation([0.0, 0.0, PLATE_T + UJOINT_SIDE + CYLINDER_L + rod_l + EYE_SIDE])) })?;
    for k in 0..6 {
        s.run(&InsertInstance { element: asm, instance: Instance::new(piston(k), piston_source, piston_pose(k)) })?;
    }
    s.run(&SetInstancesFixed { element: asm, instances: vec![BASE_INSTANCE], fixed: true })?;
    Ok(())
}

/// ER Ex2's document: Hexapod, Pneumatic Piston, Piston Assembly, Topplate, Baseplate.
pub fn move_to_document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(MOVE_DOCUMENT_NAME);
    doc.id = MOVE_DOCUMENT;
    for (id, name, studio) in [
        (HEXAPOD, HEXAPOD_NAME, false),
        (PISTON_STUDIO, PISTON_STUDIO_NAME, true),
        (PISTON_ASSEMBLY, PISTON_ASSEMBLY_NAME, false),
        (TOPPLATE, TOPPLATE_NAME, true),
        (BASEPLATE, BASEPLATE_NAME, true),
    ] {
        let mut el = if studio { Element::part_studio(name) } else { Element::assembly(name) };
        el.id = id;
        doc.elements.push(el);
    }
    let mut h = History::default();
    let mut s = DocHistory(&mut doc, &mut h);
    build_piston(&mut s, PISTON_STUDIO, ROD_L)?;
    build_plate(&mut s, BASEPLATE, BASE_SKETCH, BASE_EXTRUDE, BASE_D, BASEPLATE_NAME)?;
    build_plate(&mut s, TOPPLATE, TOP_SKETCH, TOP_EXTRUDE, TOP_D, TOPPLATE_NAME)?;
    build_piston_assembly(&mut s, PISTON_ASSEMBLY, PISTON_STUDIO)?;
    build_hexapod(
        &mut s,
        HEXAPOD,
        InstanceSource::Part { element: BASEPLATE, part: BASE_PART },
        InstanceSource::Part { element: TOPPLATE, part: TOP_PART },
        InstanceSource::Assembly { element: PISTON_ASSEMBLY },
        ROD_L,
    )?;
    Ok(doc)
}

/// The piston's volume for a cylinder of diameter `d` and a rod of length `l` (mm³).
pub fn piston_volume(d: f64, l: f64) -> f64 {
    use std::f64::consts::PI;
    UJOINT_SIDE.powi(3) + CYLINDER_L * PI * (d / 2.0).powi(2) + PI * (ROD_D / 2.0).powi(2) * l + EYE_SIDE.powi(3)
}

// ---------------------------------------------------------------------------------------------
// P3G.5: ER Ex1 ("Using Linked Documents") as two documents

/// ER Ex1's Piston document: Pneumatic Piston and Piston Assembly
/// (`fixtures/linked_piston_standin.cadrs`).
pub const PISTON_DOCUMENT: DocumentId = DocumentId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_0002);
pub const PISTON_DOCUMENT_NAME: &str = "Linked Document- Piston (stand-in)";
pub const LINKED_PISTON_STUDIO: ElementId = eid(0x11);
pub const LINKED_PISTON_ASSEMBLY: ElementId = eid(0x12);

/// ER Ex1's Project document: the Hexapod assembly (the two plates only) and the Topplate and
/// Baseplate Part Studios (`fixtures/linked_hexapod_standin.cadrs`).
pub const PROJECT_DOCUMENT: DocumentId = DocumentId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_0003);
pub const PROJECT_DOCUMENT_NAME: &str = "Linked Documents- Project (stand-in)";
pub const PROJECT_HEXAPOD: ElementId = eid(0x21);
pub const PROJECT_TOPPLATE: ElementId = eid(0x22);
pub const PROJECT_BASEPLATE: ElementId = eid(0x23);

/// The edited piston (ER Ex1 steps 12–13, V2).
pub const CYLINDER_D_V2: f64 = 20.0;
pub const ROD_L_V2: f64 = 75.0;

/// The Revolute (UJoint on a baseplate hole) and Fastened (Topplate hole on an Eye) mates of
/// the k-th piston.
pub const fn revolute_mate(k: usize) -> crate::assembly::mate::MateId {
    crate::assembly::mate::MateId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_3000 | k as u128)
}
pub const fn fastened_mate(k: usize) -> crate::assembly::mate::MateId {
    crate::assembly::mate::MateId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_3100 | k as u128)
}

/// "Linked Document- Piston (stand-in)": Pneumatic Piston (V1 geometry) and Piston Assembly.
pub fn piston_document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(PISTON_DOCUMENT_NAME);
    doc.id = PISTON_DOCUMENT;
    let mut st = Element::part_studio(PISTON_STUDIO_NAME);
    st.id = LINKED_PISTON_STUDIO;
    doc.elements.push(st);
    let mut asm = Element::assembly(PISTON_ASSEMBLY_NAME);
    asm.id = LINKED_PISTON_ASSEMBLY;
    doc.elements.push(asm);
    let mut h = History::default();
    let mut s = DocHistory(&mut doc, &mut h);
    build_piston(&mut s, LINKED_PISTON_STUDIO, ROD_L)?;
    build_piston_assembly(&mut s, LINKED_PISTON_ASSEMBLY, LINKED_PISTON_STUDIO)?;
    Ok(doc)
}

/// "Linked Documents- Project (stand-in)": the Hexapod with Baseplate <1> (fixed) and Topplate
/// <1> (where the V1 pistons will hold it, z 116), and the two plates' Part Studios. The course's
/// project has only the plates; the pistons are linked in (ER Ex1 step 4).
pub fn project_document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(PROJECT_DOCUMENT_NAME);
    doc.id = PROJECT_DOCUMENT;
    for (id, name, studio) in [(PROJECT_HEXAPOD, HEXAPOD_NAME, false), (PROJECT_TOPPLATE, TOPPLATE_NAME, true), (PROJECT_BASEPLATE, BASEPLATE_NAME, true)] {
        let mut el = if studio { Element::part_studio(name) } else { Element::assembly(name) };
        el.id = id;
        doc.elements.push(el);
    }
    let mut h = History::default();
    let mut s = DocHistory(&mut doc, &mut h);
    build_plate(&mut s, PROJECT_BASEPLATE, BASE_SKETCH, BASE_EXTRUDE, BASE_D, BASEPLATE_NAME)?;
    build_plate(&mut s, PROJECT_TOPPLATE, TOP_SKETCH, TOP_EXTRUDE, TOP_D, TOPPLATE_NAME)?;
    s.run(&InsertInstance { element: PROJECT_HEXAPOD, instance: Instance::new(BASE_INSTANCE, InstanceSource::Part { element: PROJECT_BASEPLATE, part: BASE_PART }, Pose::IDENTITY) })?;
    s.run(&InsertInstance {
        element: PROJECT_HEXAPOD,
        instance: Instance::new(TOP_INSTANCE, InstanceSource::Part { element: PROJECT_TOPPLATE, part: TOP_PART }, Pose::translation([0.0, 0.0, PLATE_T + UJOINT_SIDE + CYLINDER_L + ROD_L + EYE_SIDE])),
    })?;
    s.run(&SetInstancesFixed { element: PROJECT_HEXAPOD, instances: vec![BASE_INSTANCE], fixed: true })?;
    Ok(doc)
}

/// The occurrence of the piston part `inner` (one of [`PISTON_INSTANCES`]) in the Piston
/// Assembly instance `piston`.
pub fn piston_part(piston: InstanceId, inner: InstanceId) -> InstanceId {
    crate::assembly::structure::derive(piston, inner)
}

fn occurrence_solid(doc: &Document, asm: ElementId, occ: InstanceId) -> Result<std::sync::Arc<crate::solid::Solid>, CommandError> {
    crate::assembly::document_occurrence_solids(doc, asm).remove(&occ).ok_or_else(|| CommandError::Invalid(format!("no part for {occ}")))
}

/// The centre of the Ø10 hole edge of `s` at (x, y, z) (part coordinates), as a connector point.
pub fn hole_connector(s: &crate::solid::Solid, x: f64, y: f64, z: f64) -> Result<crate::assembly::connector::ImplicitConnector, CommandError> {
    use crate::assembly::connector::{EntityRef, ImplicitPoint, implicit_points};
    let e = s
        .edges
        .iter()
        .find(|e| e.circle.is_some_and(|c| (c.radius - HOLE_D / 2.0).abs() < 1e-6 && (c.center[0] - x).abs() < 1e-6 && (c.center[1] - y).abs() < 1e-6 && (c.center[2] - z).abs() < 1e-6))
        .ok_or_else(|| CommandError::Invalid(format!("no hole edge at ({x}, {y}, {z})")))?
        .name;
    implicit_points(s, &EntityRef::Edge(e)).into_iter().find(|p| matches!(p.point, ImplicitPoint::CircleCenter(_))).ok_or_else(|| CommandError::Invalid("no hole centre".into()))
}

/// The centroid of the planar face of `s` whose outward normal is `n`, as a connector point.
pub fn face_connector(s: &crate::solid::Solid, n: [f64; 3]) -> Result<crate::assembly::connector::ImplicitConnector, CommandError> {
    use crate::assembly::connector::{EntityRef, ImplicitPoint, face_normal, implicit_points};
    let f = s
        .faces
        .iter()
        .find(|f| face_normal(s, &f.name).is_some_and(|m| (0..3).all(|i| (m[i] - n[i]).abs() < 1e-6)))
        .ok_or_else(|| CommandError::Invalid(format!("no face facing {n:?}")))?
        .name;
    implicit_points(s, &EntityRef::Face(f)).into_iter().find(|p| matches!(p.point, ImplicitPoint::FaceCentroid(_))).ok_or_else(|| CommandError::Invalid("no face centroid".into()))
}

/// Adds the mate `f` to the assembly `asm`, solved with `mover` doing the moving (as the mate
/// dialog's ✓ does).
pub fn add_mate(s: &mut dyn Studio, asm: ElementId, f: crate::assembly::mate::MateFeature, mover: InstanceId) -> Result<(), CommandError> {
    use crate::assembly::commands::AddMateFeature;
    use crate::assembly::solver::SolveOptions;
    let doc = s.document().clone();
    let model = doc.element(asm).and_then(|e| e.assembly_model()).cloned().ok_or(CommandError::ElementNotFound(asm))?;
    let solids = crate::assembly::document_occurrence_solids(&doc, asm);
    let mut flat = crate::assembly::structure::solver_model(&doc, &model);
    flat.mates.push(f.clone());
    let sol = crate::assembly::solve(&flat, &solids, &SolveOptions { movers: vec![mover], snap: Some(f.id), ..Default::default() });
    if !sol.converged {
        return Err(CommandError::Invalid(format!("{} doesn't solve (residual {})", f.name, sol.residual)));
    }
    let poses = sol.changed(&flat);
    s.run(&AddMateFeature { element: asm, feature: f, poses })
}

/// ER Ex1 step 6/8: **Revolute k+1** between the k-th baseplate hole's top edge centre and the
/// bottom-face centre of the UJoint of the Piston Assembly instance `piston` in the assembly
/// `asm` (the piston moves; the UJoint spans z 10..20 after it).
pub fn revolute_piston(s: &mut dyn Studio, asm: ElementId, piston: InstanceId, k: usize) -> Result<(), CommandError> {
    use crate::assembly::connector::MateConnector;
    use crate::assembly::mate::{Mate, MateFeature, MateKind, MateType};
    let ujoint = piston_part(piston, PISTON_INSTANCES[0]);
    let (x, y) = hole_centres()[k];
    let base = occurrence_solid(s.document(), asm, BASE_INSTANCE)?;
    let uj = occurrence_solid(s.document(), asm, ujoint)?;
    let hole = hole_connector(&base, x, y, PLATE_T)?;
    let bottom = face_connector(&uj, [0.0, 0.0, -1.0])?;
    let mut c1 = MateConnector::implicit(ujoint, &bottom);
    // The two frames agree once the piston stands on the hole (its bottom faces down).
    c1.flip = hole.frame.z[2] > 0.0;
    let m = Mate::new(MateType::Revolute, c1, MateConnector::implicit(BASE_INSTANCE, &hole));
    let name = next_mate_name(s.document(), asm, "Revolute");
    add_mate(s, asm, MateFeature::new(revolute_mate(k), name, MateKind::Mate(m)), piston)
}

/// ER Ex1 step 9/10: **Fastened k+1** between the k-th topplate hole's bottom edge centre and
/// the top-face centre of the Eye of `piston` (the Topplate moves).
pub fn fasten_topplate(s: &mut dyn Studio, asm: ElementId, piston: InstanceId, k: usize) -> Result<(), CommandError> {
    use crate::assembly::connector::MateConnector;
    use crate::assembly::mate::{Mate, MateFeature, MateKind, MateType};
    let eye = piston_part(piston, PISTON_INSTANCES[3]);
    let (x, y) = hole_centres()[k];
    let top = occurrence_solid(s.document(), asm, TOP_INSTANCE)?;
    let ey = occurrence_solid(s.document(), asm, eye)?;
    let hole = hole_connector(&top, x, y, 0.0)?;
    let face = face_connector(&ey, [0.0, 0.0, 1.0])?;
    let mut c1 = MateConnector::implicit(TOP_INSTANCE, &hole);
    c1.flip = hole.frame.z[2] < 0.0;
    let m = Mate::new(MateType::Fastened, c1, MateConnector::implicit(eye, &face));
    let name = next_mate_name(s.document(), asm, "Fastened");
    add_mate(s, asm, MateFeature::new(fastened_mate(k), name, MateKind::Mate(m)), TOP_INSTANCE)
}

fn next_mate_name(doc: &Document, asm: ElementId, label: &str) -> String {
    let mates = doc.element(asm).and_then(|e| e.assembly_model()).map(|a| a.mates.clone()).unwrap_or_default();
    crate::assembly::mate::next_name(&mates, label)
}

/// ER Ex1 steps 8–10 (a scenario's shortcut after the first mate and the pastes made in the
/// UI): a Revolute for every Piston Assembly instance of `asm` without one, on the next free
/// baseplate hole (in instance order), then a Fastened from each piston's Eye to the Topplate
/// hole above its baseplate hole.
pub fn complete_hexapod(s: &mut dyn Studio, asm: ElementId) -> Result<(), CommandError> {
    use crate::assembly::mate::MateType;
    let model = s.document().element(asm).and_then(|e| e.assembly_model()).cloned().ok_or(CommandError::ElementNotFound(asm))?;
    let pistons: Vec<InstanceId> = model.instances.iter().filter(|i| i.source.is_assembly()).map(|i| i.id).collect();
    let hole_of = |p: [f64; 3]| -> Option<usize> { hole_centres().iter().position(|(x, y)| (x - p[0]).abs() < 1e-3 && (y - p[1]).abs() < 1e-3) };
    // The holes the pistons already stand on.
    let mut on: Vec<(InstanceId, usize)> = Vec::new();
    for f in &model.mates {
        let Some(m) = f.mate().filter(|m| m.mate_type == MateType::Revolute) else { continue };
        let (Some(base), Some(other)) = (m.connectors.iter().find(|c| c.instance == BASE_INSTANCE), m.connectors.iter().find(|c| c.instance != BASE_INSTANCE)) else { continue };
        if let (Some(k), Some(p)) = (hole_of(base.frame.origin), pistons.iter().find(|p| piston_part(**p, PISTON_INSTANCES[0]) == other.instance)) {
            on.push((*p, k));
        }
    }
    for p in &pistons {
        if on.iter().any(|(q, _)| q == p) {
            continue;
        }
        let k = (0..6).find(|k| on.iter().all(|(_, j)| j != k)).ok_or_else(|| CommandError::Invalid("more pistons than holes".into()))?;
        revolute_piston(s, asm, *p, k)?;
        on.push((*p, k));
    }
    on.sort_by_key(|(_, k)| *k);
    for (p, k) in on {
        fasten_topplate(s, asm, p, k)?;
    }
    Ok(())
}
