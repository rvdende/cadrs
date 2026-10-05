//! The stand-in for PCB Studio's **Board Exercise** start document (P3H.5, `pcb-studio.md` PCB6:
//! the course's public "Exercise: Board" isn't available). In mm; every id is fixed, so the
//! fixture `fixtures/phone_case_standin.cadrs` is regenerated exactly
//! (`cadrs_core/tests/phone_case.rs`).
//!
//! - Part Studio **Enclosure**: a phone case. **Case outline** (Sketch 1 on Top) is a rectangle
//!   centred on the origin whose two driving dimensions are the course's variables: the
//!   horizontal one is **Width** (70 mm) and the vertical one **Length** (140 mm). *cadrs has no
//!   variables yet (P3F.4), and a sketch dimension has no name, so these two dimensions stand in
//!   for #Width and #Length; resizing the case (PCB6 step 12) edits them.* Extrude 1 makes the
//!   **Enclosure** 10 mm tall (z 0…10), Fillet 1 rounds its four vertical edges R10, Shell 1
//!   removes its top face with a 2 mm wall, so the cavity is (W − 4) × (L − 4) with R8 corners
//!   (66 × 136 now, 81 × 146 at 85 × 150: the course's exported outline). A **Battery** block
//!   50 × 60 × 5 lies on the floor (x −25…25, y −55…5, z 2…7) and an **Antenna** block, a
//!   pentagon of 5 edges near the top end (z 2…4).
//! - Assembly **Cell phone**: Enclosure <1> (fixed), Battery <1>, Antenna <1>, all where the
//!   studio has them, and **Group 1** of the three.
//!
//! [`board_in_context`] does PCB6 steps 2–8 through the command layer, as the scenario does
//! them in the UI: Create Part Studio in context at the assembly Origin, Sketch 1 on the
//! battery's top face with Use of the cavity floor (the case's inner outline), the battery's 4
//! edges and the antenna's 5, the board extruded 0.062 mm up (the course's mm quirk, kept) and
//! the two keep-outs 1 mm down, the parts and the studio renamed, Insert and go to Assembly,
//! and Group 1 edited to take the three new instances. [`resize`] is step 12 and
//! [`update_context`] step 13.

use cadrs_sketch::constraint::{ConstraintOf, PointSpec, rectangle_constraints};
use cadrs_sketch::{Dimension, DimensionKind, Link, PlaneRef, SketchOp, Vec2};

use crate::applied::{EdgeOrFace, FilletFeature, ShellFeature};
use crate::assembly::commands::{AddMateFeature, InsertInstance, SetInstancesFixed, SetMateFeature};
use crate::assembly::context::{self, SetStudioContext};
use crate::assembly::managed_context::{CreateStudioInContext, InsertFromStudio, resnapshot};
use crate::assembly::mate::{MateFeature, MateId, MateKind};
use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::appearance::Appearance;
use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, NewElementKind, RenameElement, RenameFeature, RenamePart, SetExtrude, SetPartAppearance, SetPartMaterial};
use crate::document::{BooleanOp, Document, EdgeRef, ExtrudeFeature, FaceRef, Offset};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::links::LinkContext;
use crate::studio::{DocHistory, Studio};

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x3a05_0000_0000_0000_0000_0000_0000_0000 + n)
}

const fn eid(n: u128) -> ElementId {
    ElementId::from_u128(0x3a05_0000_0000_0000_0000_0000_0000_0100 + n)
}

const fn iid(n: u128) -> InstanceId {
    InstanceId::from_u128(0x3a05_0000_0000_0000_0000_0000_0000_1000 + n)
}

pub const DOCUMENT_NAME: &str = "Exercise: Board (stand-in)";
pub const ENCLOSURE_STUDIO: ElementId = eid(1);
pub const CELL_PHONE: ElementId = eid(2);
/// The Part Studio [`board_in_context`] creates in context ("Board & Keep out").
pub const BOARD_STUDIO: ElementId = eid(3);

pub const CASE_SKETCH: FeatureId = fid(1);
pub const CASE_EXTRUDE: FeatureId = fid(2);
pub const CASE_FILLET: FeatureId = fid(3);
pub const CASE_SHELL: FeatureId = fid(4);
pub const BATTERY_SKETCH: FeatureId = fid(5);
pub const BATTERY_EXTRUDE: FeatureId = fid(6);
pub const ANTENNA_SKETCH: FeatureId = fid(7);
pub const ANTENNA_EXTRUDE: FeatureId = fid(8);

pub const ENCLOSURE: PartId = PartId::new(CASE_EXTRUDE, 0);
pub const BATTERY: PartId = PartId::new(BATTERY_EXTRUDE, 0);
pub const ANTENNA: PartId = PartId::new(ANTENNA_EXTRUDE, 0);

pub const ENCLOSURE_1: InstanceId = iid(1);
pub const BATTERY_1: InstanceId = iid(2);
pub const ANTENNA_1: InstanceId = iid(3);
pub const GROUP_1: MateId = MateId::from_u128(0x3a05_0000_0000_0000_0000_0000_0000_2001);

/// The board studio's features and the instances [`board_in_context`] adds.
pub const BOARD_SKETCH: FeatureId = fid(0x21);
pub const BOARD_EXTRUDE: FeatureId = fid(0x22);
pub const KEEPOUT_EXTRUDE: FeatureId = fid(0x23);
pub const BOARD_PART: PartId = PartId::new(BOARD_EXTRUDE, 0);
pub const BOARD_1: InstanceId = iid(0x21);
pub const KEEPOUT_BATTERY_1: InstanceId = iid(0x22);
pub const KEEPOUT_ANTENNA_1: InstanceId = iid(0x23);

/// The case at the start of the exercise (mm) and after step 12.
pub const WIDTH: f64 = 70.0;
pub const LENGTH: f64 = 140.0;
pub const RESIZED: (f64, f64) = (85.0, 150.0);
pub const WALL: f64 = 2.0;
pub const OUTER_R: f64 = 10.0;
pub const HEIGHT: f64 = 10.0;
/// The battery (x0, y0, x1, y1) and its z range; the antenna's pentagon and its z range.
pub const BATTERY_RECT: [f64; 4] = [-25.0, -55.0, 25.0, 5.0];
pub const BATTERY_Z: (f64, f64) = (WALL, 7.0);
pub const ANTENNA_PENTAGON: [[f64; 2]; 5] = [[6.0, 50.0], [26.0, 50.0], [26.0, 58.0], [18.0, 64.0], [6.0, 64.0]];
pub const ANTENNA_Z: (f64, f64) = (WALL, 4.0);
/// The board's thickness: 0.062 **mm**, as the course's step 5 and its exported file have it.
pub const BOARD_THICKNESS: f64 = 0.062;
pub const KEEPOUT_DEPTH: f64 = 1.0;

pub const BOARD_STUDIO_NAME: &str = "Board & Keep out";

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn poly(points: Vec<Vec2>) -> SketchOp {
    SketchOp::AddPolyline { points, closed: true, construction: false, label: "Add polygon" }
}

fn geometry(s: &dyn Studio, el: ElementId, sketch: FeatureId) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|k| k.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

/// An extrude of the regions of `sketch` under `seeds` (sketch coordinates), New.
fn extrude(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, feature: FeatureId, seeds: &[Vec2], set: impl FnOnce(&mut ExtrudeFeature)) -> Result<(), CommandError> {
    let g = geometry(s, el, sketch)?;
    let regions = super::region_refs(sketch, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid(format!("{} of {} regions found", regions.len(), seeds.len())));
    }
    let mut e = super::extrude_of(regions, 10.0);
    e.op = BooleanOp::New;
    set(&mut e);
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: e, label: "Extrude".into() })
}

fn depth(e: &mut ExtrudeFeature, d: f64) {
    e.depth = d;
    e.depth_expr = format!("{d} mm");
}

/// A block on Top from `z.0` to `z.1`: its own sketch and extrude, the part named.
fn block(s: &mut dyn Studio, el: ElementId, (sk, ex): (FeatureId, FeatureId), points: Vec<Vec2>, seed: Vec2, z: (f64, f64), name: &str) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: sk, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: sk, op: poly(points) })?;
    s.run(&RenameFeature { element: el, feature: sk, name: format!("{name} outline") })?;
    extrude(s, el, sk, ex, &[seed], |e| {
        depth(e, z.1 - z.0);
        e.start_offset = Some(Offset { value: z.0, expr: format!("{} mm", z.0), flip: false });
    })?;
    s.run(&RenamePart { element: el, part: PartId::new(ex, 0), name: name.into() })
}

fn build_of(s: &dyn Studio, el: ElementId) -> Result<std::sync::Arc<crate::rebuild::Build>, CommandError> {
    let features = s.document().element(el).map(|e| e.features().to_vec()).ok_or(CommandError::ElementNotFound(el))?;
    Ok(crate::rebuild::build(&features))
}

/// The Enclosure's edge nearest `p`.
fn case_edge(s: &dyn Studio, p: [f64; 3]) -> Result<EdgeRef, CommandError> {
    let b = build_of(s, ENCLOSURE_STUDIO)?;
    let part = b.part(ENCLOSURE).ok_or_else(|| CommandError::Invalid(format!("the case did not build: {:?}", b.errors)))?;
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).filter(|e| e.distance(p) < 1e-3).ok_or_else(|| CommandError::Invalid(format!("no edge at {p:?}")))?;
    Ok(EdgeRef { part: ENCLOSURE, edge: e.name, seed: p })
}

/// The index of the planar face of `solid` at height `z` facing +Z whose area centre is
/// nearest (x, y).
fn up_face(solid: &crate::solid::Solid, z: f64, near: [f64; 2]) -> Option<usize> {
    (0..solid.faces.len())
        .filter(|&i| solid.faces[i].plane.is_some_and(|p| (p.origin[2] - z).abs() < 1e-6) && solid.face_normal(i).is_some_and(|n| n[2] > 0.999))
        .min_by(|&a, &b| {
            let d = |i: usize| {
                let c = solid.faces[i].center.unwrap_or(solid.faces[i].plane.map(|p| p.origin).unwrap_or([0.0; 3]));
                (c[0] - near[0]).hypot(c[1] - near[1])
            };
            d(a).total_cmp(&d(b))
        })
}

/// Builds the Enclosure studio's features in `el` (see the module docs).
pub fn build_enclosure(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let (hw, hl) = (WIDTH / 2.0, LENGTH / 2.0);
    let corners = [v(-hw, -hl), v(hw, -hl), v(hw, hl), v(-hw, hl)];
    s.run(&AddSketch { element: el, feature: CASE_SKETCH, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: CASE_SKETCH, op: poly(corners.to_vec()) })?;
    let mut specs = rectangle_constraints(corners);
    specs.push(ConstraintOf::Center(PointSpec::Origin, PointSpec::At(corners[0]), PointSpec::At(corners[2])));
    s.run(&EditSketch { element: el, feature: CASE_SKETCH, op: SketchOp::AddConstraints(specs) })?;
    let g = geometry(s, el, CASE_SKETCH)?;
    let point = |p: Vec2| g.point_at(p, 1e-9).ok_or_else(|| CommandError::Invalid("a corner is missing".into()));
    let (a, b, c) = (point(corners[0])?, point(corners[1])?, point(corners[2])?);
    let dim = |kind: DimensionKind, value: f64, offset: f64| SketchOp::SetDimension {
        dimension: Dimension { kind, value, offset, along: 0.0, driven: false },
        moves: vec![],
        radii: vec![],
    };
    // Width (horizontal, under the case) and Length (vertical, right of it).
    s.run(&EditSketch { element: el, feature: CASE_SKETCH, op: dim(DimensionKind::Horizontal { a, b }, WIDTH, -15.0) })?;
    s.run(&EditSketch { element: el, feature: CASE_SKETCH, op: dim(DimensionKind::Vertical { a: b, b: c }, LENGTH, 15.0) })?;
    s.run(&RenameFeature { element: el, feature: CASE_SKETCH, name: "Case outline (Width, Length)".into() })?;
    extrude(s, el, CASE_SKETCH, CASE_EXTRUDE, &[v(0.0, 0.0)], |e| depth(e, HEIGHT))?;
    s.run(&RenamePart { element: el, part: ENCLOSURE, name: "Enclosure".into() })?;
    let edges = [(-hw, -hl), (hw, -hl), (hw, hl), (-hw, hl)]
        .into_iter()
        .map(|(x, y)| case_edge(s, [x, y, HEIGHT / 2.0]).map(EdgeOrFace::Edge))
        .collect::<Result<Vec<_>, _>>()?;
    s.run(&AddFeature::fillet(el, CASE_FILLET, FilletFeature { entities: edges, size: OUTER_R, size_expr: format!("{OUTER_R} mm"), ..FilletFeature::default() }))?;
    let b = build_of(s, el)?;
    let part = b.part(ENCLOSURE).ok_or_else(|| CommandError::Invalid(format!("the case did not build: {:?}", b.errors)))?;
    let top = up_face(&part.solid, HEIGHT, [0.0, 0.0]).ok_or_else(|| CommandError::Invalid("the case has no top face".into()))?;
    let seed = part.solid.face_point(top).unwrap_or([0.0, 0.0, HEIGHT]);
    let face = FaceRef { part: ENCLOSURE, face: part.solid.faces[top].name, seed };
    s.run(&AddFeature::shell(el, CASE_SHELL, ShellFeature { faces: vec![face], thickness: WALL, thickness_expr: format!("{WALL} mm"), ..ShellFeature::default() }))?;
    s.run(&SetPartMaterial { element: el, parts: vec![ENCLOSURE], material: crate::material::library("ABS") })?;
    s.run(&SetPartAppearance { element: el, parts: vec![ENCLOSURE], appearance: Some(Appearance::rgb(200, 202, 206)) })?;
    let [x0, y0, x1, y1] = BATTERY_RECT;
    block(s, el, (BATTERY_SKETCH, BATTERY_EXTRUDE), vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], v((x0 + x1) / 2.0, (y0 + y1) / 2.0), BATTERY_Z, "Battery")?;
    let pts: Vec<Vec2> = ANTENNA_PENTAGON.iter().map(|p| v(p[0], p[1])).collect();
    block(s, el, (ANTENNA_SKETCH, ANTENNA_EXTRUDE), pts, v(15.0, 56.0), ANTENNA_Z, "Antenna")?;
    s.run(&SetPartAppearance { element: el, parts: vec![BATTERY], appearance: Some(Appearance::rgb(88, 92, 98)) })?;
    s.run(&SetPartAppearance { element: el, parts: vec![ANTENNA], appearance: Some(Appearance::rgb(150, 120, 90)) })?;
    Ok(())
}

/// "Exercise: Board (stand-in)": see the module docs.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(DOCUMENT_NAME);
    doc.id = crate::ids::DocumentId::from_u128(0x3a05_0000_0000_0000_0000_0000_0000_0001);
    let mut h = History::default();
    let mut e = crate::document::Element::part_studio("Enclosure");
    e.id = ENCLOSURE_STUDIO;
    doc.elements.push(e);
    build_enclosure(&mut DocHistory(&mut doc, &mut h), ENCLOSURE_STUDIO)?;
    h.execute(&mut doc, &crate::commands::AddElement { id: CELL_PHONE, kind: NewElementKind::Assembly, name: Some("Cell phone".into()), after: Some(ENCLOSURE_STUDIO) })?;
    for (i, p) in [(ENCLOSURE_1, ENCLOSURE), (BATTERY_1, BATTERY), (ANTENNA_1, ANTENNA)] {
        h.execute(&mut doc, &InsertInstance { element: CELL_PHONE, instance: Instance::new(i, InstanceSource::Part { element: ENCLOSURE_STUDIO, part: p }, Pose::IDENTITY) })?;
    }
    h.execute(&mut doc, &SetInstancesFixed { element: CELL_PHONE, instances: vec![ENCLOSURE_1], fixed: true })?;
    let group = MateFeature::new(GROUP_1, "Group 1", MateKind::Group { instances: vec![ENCLOSURE_1, BATTERY_1, ANTENNA_1] });
    h.execute(&mut doc, &AddMateFeature { element: CELL_PHONE, feature: group, poses: Vec::new() })?;
    // A document opens on its first tab: the Enclosure, then Cell phone (as the course's tabs).
    doc.elements.retain(|e| e.id == ENCLOSURE_STUDIO || e.id == CELL_PHONE);
    Ok(doc)
}

/// The document as a file (the fixture).
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}

// ---------------------------------------------------------------------------------------------
// The exercise, through the command layer

/// PCB6 steps 2–8 (see the module docs). The board studio is [`BOARD_STUDIO`].
pub fn board_in_context(s: &mut dyn Studio) -> Result<(), CommandError> {
    let el = BOARD_STUDIO;
    // Step 2: Create Part Studio in context, at the assembly Origin.
    s.run(&CreateStudioInContext { assembly: CELL_PHONE, studio: el, name: None, origin: crate::assembly::Pose::IDENTITY })?;
    let solids = context::solids(s.document(), el);
    let of = |inst: InstanceId| {
        let id = context::context_id(0, inst);
        solids.iter().find(|(f, _)| *f == id).cloned().ok_or_else(|| CommandError::Invalid(format!("no context part for {inst}")))
    };
    let (case_id, case) = of(ENCLOSURE_1)?;
    let (battery_id, battery) = of(BATTERY_1)?;
    let (antenna_id, antenna) = of(ANTENNA_1)?;
    // Step 3: Sketch 1 on the battery's top face.
    let [x0, y0, x1, y1] = BATTERY_RECT;
    let bat_top = up_face(&battery, BATTERY_Z.1, [(x0 + x1) / 2.0, (y0 + y1) / 2.0]).ok_or_else(|| CommandError::Invalid("the battery has no top face".into()))?;
    let plane = context::face_plane_on(&battery, battery_id, battery.faces[bat_top].name).ok_or_else(|| CommandError::Invalid("no plane on the battery".into()))?;
    s.run(&AddSketch { element: el, feature: BOARD_SKETCH, plane: Some(plane) })?;
    let frame = plane.frame();
    // Use: the cavity floor (the case's inner outline), the battery's 4 edges, the antenna's 5.
    let floor = up_face(&case, WALL, [0.0, 0.0]).ok_or_else(|| CommandError::Invalid("the case has no floor".into()))?;
    let ant_top = up_face(&antenna, ANTENNA_Z.1, [15.0, 56.0]).ok_or_else(|| CommandError::Invalid("the antenna has no top face".into()))?;
    let lc = LinkContext { solids: vec![(case_id, &*case), (battery_id, &*battery), (antenna_id, &*antenna)], features: &[] };
    let mut items = Vec::new();
    for (feature, solid, face) in [(case_id, &case, floor), (battery_id, &battery, bat_top), (antenna_id, &antenna, ant_top)] {
        for edge in solid.face_edges(&solid.faces[face].name) {
            let link = Link::Edge { feature: feature.0, edge };
            if let Some(shape) = lc.shape(link, &frame) {
                items.push((shape, link));
            }
        }
    }
    if items.len() != 8 + 4 + 5 {
        return Err(CommandError::Invalid(format!("Use found {} edges, not 17", items.len())));
    }
    s.run(&EditSketch { element: el, feature: BOARD_SKETCH, op: SketchOp::Use { items } })?;
    // Step 5: all three regions 0.062 mm up, New: the Board.
    let at = |x: f64, y: f64| frame.to_sketch([x, y, BATTERY_Z.1]);
    let (ring, bat, ant) = (at(-28.0, 40.0), at(0.0, -25.0), at(15.0, 56.0));
    extrude(s, el, BOARD_SKETCH, BOARD_EXTRUDE, &[ring, bat, ant], |e| depth(e, BOARD_THICKNESS))?;
    s.run(&RenamePart { element: el, part: BOARD_PART, name: "Board".into() })?;
    // Step 6: the battery and antenna regions 1 mm down, New: two keep-outs.
    extrude(s, el, BOARD_SKETCH, KEEPOUT_EXTRUDE, &[bat, ant], |e| {
        depth(e, KEEPOUT_DEPTH);
        e.flip = true;
    })?;
    let (ko_battery, ko_antenna) = keepout_parts(s)?;
    s.run(&RenamePart { element: el, part: ko_battery, name: "Keep-out Battery".into() })?;
    s.run(&RenamePart { element: el, part: ko_antenna, name: "Keep-out Antenna".into() })?;
    s.run(&RenameElement { id: el, name: BOARD_STUDIO_NAME.into() })?;
    // Step 7: Insert and go to Assembly.
    s.run(&InsertFromStudio { studio: el, parts: vec![BOARD_PART, ko_battery, ko_antenna], instances: vec![BOARD_1, KEEPOUT_BATTERY_1, KEEPOUT_ANTENNA_1] })?;
    // Step 8: Group 1 takes the three.
    let group = MateFeature::new(GROUP_1, "Group 1", MateKind::Group { instances: vec![ENCLOSURE_1, BATTERY_1, ANTENNA_1, BOARD_1, KEEPOUT_BATTERY_1, KEEPOUT_ANTENNA_1] });
    s.run(&SetMateFeature { element: CELL_PHONE, feature: group, poses: Vec::new() })
}

/// The two keep-out parts (battery, antenna), told apart by where they are.
pub fn keepout_parts(s: &dyn Studio) -> Result<(PartId, PartId), CommandError> {
    let b = build_of(s, BOARD_STUDIO)?;
    let parts: Vec<_> = b.parts.iter().filter(|p| p.features.first() == Some(&KEEPOUT_EXTRUDE) || p.id.feature == KEEPOUT_EXTRUDE).collect();
    let centre_y = |p: &crate::parts::Part| {
        p.solid.bounds().map_or(0.0, |(lo, hi)| (lo[1] + hi[1]) / 2.0)
    };
    let bat = parts.iter().min_by(|a, b| centre_y(a).total_cmp(&centre_y(b))).ok_or_else(|| CommandError::Invalid(format!("no keep-out parts: {:?}", b.errors)))?;
    let ant = parts.iter().max_by(|a, b| centre_y(a).total_cmp(&centre_y(b))).ok_or_else(|| CommandError::Invalid("no keep-out parts".into()))?;
    if bat.id == ant.id {
        return Err(CommandError::Invalid("the keep-out extrude made one part".into()));
    }
    Ok((bat.id, ant.id))
}

/// Step 12: sets the case's Width and Length (the Case outline's two dimensions).
pub fn resize(s: &mut dyn Studio, width: f64, length: f64) -> Result<(), CommandError> {
    let g = geometry(s, ENCLOSURE_STUDIO, CASE_SKETCH)?;
    let mut ids = Vec::new();
    for (id, d) in g.dimensions.iter() {
        match d.kind {
            DimensionKind::Horizontal { .. } => ids.push((id, width)),
            DimensionKind::Vertical { .. } => ids.push((id, length)),
            _ => {}
        }
    }
    ids.sort_by_key(|(_, v)| (*v * 1000.0) as i64);
    for (id, value) in ids {
        s.run(&EditSketch { element: ENCLOSURE_STUDIO, feature: CASE_SKETCH, op: SketchOp::SetDimensionValue { id, value } })?;
    }
    Ok(())
}

/// Step 13: Update context of the board studio (one undo step; nothing when it's current).
pub fn update_context(s: &mut dyn Studio) -> Result<bool, CommandError> {
    let ctx = s.document().element(BOARD_STUDIO).and_then(|e| e.contexts.first().cloned()).ok_or_else(|| CommandError::Invalid("no context".into()))?;
    let now = resnapshot(s.document(), BOARD_STUDIO, &ctx)?;
    if now == ctx {
        return Ok(false);
    }
    s.run(&SetStudioContext { studio: BOARD_STUDIO, context: Some(now) })?;
    Ok(true)
}
