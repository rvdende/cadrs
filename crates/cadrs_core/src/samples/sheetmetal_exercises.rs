//! Stand-ins for the sheet metal course's exercises (P3I.8, X7; `simultaneous-sheet-metal.md`
//! E1–E4): the documents the exercises start from (Onshape's public documents we don't have) and
//! the finished parts, built through the same commands a user's clicks make, with fixed ids so
//! `fixtures/sheetmetal/sm_*_standin.cadrs` regenerate (in their own folder: the FEA
//! mesher's course-parts check skips thin sheet) exactly (`cadrs_core/tests/sheetmetal_exercises.rs`).
//!
//! - **E2 "Creating sheet metal parts"** ([`build_e2`]): the exercise done, in a new document
//!   "Exercise: Creating sheet metal" (mm, kg). The Front-plane profile is the exercise's (15 high
//!   walls, 40 + … + 125 wide) with the R35 arc replaced by a trapezoid of lines, since a cadrs
//!   sheet metal Extrude bends only between lines ([`E2_CHAIN`]); extruded 80, 1 mm thick, R1,
//!   minimal gap 0.025, bend relief scales 1.5 × 1. Flange 1 on the two back edges (Inner, 8,
//!   partial: per chain, hold adjacent edges, 10 in), Hem 1 on the flanges' top edges (straight,
//!   flattened, 5, in place), Sketch 2 and Tab 1 (a 20 × 10 tongue off the right base's front
//!   edge), Flange 2 on the rest of that front edge (15, as high as the right wall), Flange 3 on
//!   the right wall's top edge (10), Make joint 1 between Flange 2's side and the right wall's
//!   front edge (rip, butt joint – direction 1), Corner 1 at the rear-left corner (Round – Sized,
//!   Ø3.3); Carbon Steel.
//! - **E3 "Drawings"** ([`build_e3`]): the "Sheet Metal Box": a 200 × 125 × 150 block converted
//!   with its top excluded and its four bottom edges bent (1.5 mm, R1.5).
//! - **E4 "Sheet metal rework"** ([`build_e4`]): the "Lower Enclosure": a U-channel (Front-plane
//!   chain, 70 wide, 60 high walls, 150 long, 1.5 mm, R1.5) with an obround slot cut through both
//!   side walls (a perpendicular cut, in the flat). [`rework_e4`] does the exercise as its
//!   slides do: Finish sheet metal model, Sketch 8 on the right wall's face with Use of the slot's
//!   outline (its sketch's curves), Plane 1 (Plane point: a vertex of Sketch 8 and Front), a 2 × 8 rectangle on it swept
//!   round the slot (Add), mirrored about the Right plane with Reapply features, and the rims'
//!   edges filleted; the flat doesn't change.
//!
//! E1 ("Importing DXF & Bend") starts from a DXF of the flat
//! (`fixtures/sheetmetal/flat_pattern_e1.dxf`), made by P3I.6.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use super::gear_cover::{DocHistory, Studio};
use crate::applied::EdgeOrFace;
use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, EdgeRef, Element, ExtrudeFeature, FaceRef, FeatureKind};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::rebuild::Build;
use crate::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use crate::sheetmetal_features::{Bound, ChainType, FlangeFeature, HemFeature, MakeJointFeature, MakeJointType, SheetMetalFeature};
use crate::sheetmetal_tools::{CornerFeature, SheetMetalTool, SmPick, TabFeature};
use crate::solid::Solid;

/// The material the exercises assign (E1, E2).
pub const CARBON_STEEL: &str = "Carbon Steel";

fn missing(what: &str) -> CommandError {
    CommandError::Invalid(format!("the stand-in's {what} is missing"))
}

/// The element's features rebuilt.
pub fn built(s: &dyn Studio, el: ElementId) -> std::sync::Arc<Build> {
    crate::rebuild::build(&s.document().element(el).map(|e| e.active_features()).unwrap_or_default())
}

/// The edge of `part` nearest `p` (its reference, seeded at `p`).
pub fn edge_near(solid: &Solid, part: PartId, p: [f64; 3]) -> Option<EdgeRef> {
    let e = solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))?;
    (e.distance(p) < 0.5).then_some(EdgeRef { part, edge: e.name, seed: p })
}

/// The face of `part` whose centre is nearest `p`.
pub fn face_near(solid: &Solid, part: PartId, p: [f64; 3]) -> Option<FaceRef> {
    let d = |c: [f64; 3]| (0..3).map(|i| (c[i] - p[i]).powi(2)).sum::<f64>();
    let f = solid.faces.iter().filter(|f| f.center.is_some()).min_by(|a, b| d(a.center.unwrap()).total_cmp(&d(b.center.unwrap())))?;
    Some(FaceRef { part, face: f.name, seed: f.center? })
}

fn sketch(s: &mut dyn Studio, el: ElementId, id: FeatureId, plane: PlaneRef, ops: Vec<SketchOp>) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.run(&AddSketch { element: el, feature: id, plane: Some(plane) })?;
    s.run(&EditSketch { element: el, feature: id, op: SketchOp::Batch(ops) })?;
    s.document().element(el).and_then(|e| e.feature(id)).and_then(|f| f.sketch()).map(|k| k.geometry.clone()).ok_or_else(|| missing("sketch"))
}

fn add(s: &mut dyn Studio, el: ElementId, id: FeatureId, base: &str, kind: FeatureKind) -> Result<(), CommandError> {
    s.run(&AddFeature { element: el, feature: id, base_name: base.into(), kind })
}

fn chain(points: &[(f64, f64)]) -> SketchOp {
    SketchOp::AddPolyline { points: points.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed: false, construction: false, label: "Add line" }
}

fn polygon(points: &[(f64, f64)]) -> SketchOp {
    SketchOp::AddPolyline { points: points.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed: true, construction: false, label: "Add rectangle" }
}

/// A Part Studio's sole sheet metal part (the first part of `model`).
fn sm_part(b: &Build, model: FeatureId) -> Result<crate::parts::Part, CommandError> {
    if !b.errors.is_empty() {
        return Err(CommandError::Invalid(format!("the stand-in fails: {:?}", b.errors)));
    }
    let id = b.sheet_metal.iter().find(|c| c.feature == model).and_then(|c| c.parts.first()).map(|(p, _)| *p).ok_or_else(|| missing("sheet metal part"))?;
    b.parts.iter().find(|p| p.id == id).cloned().ok_or_else(|| missing("sheet metal part"))
}

fn new_document(name: &str, id: DocumentId, studio: (&str, ElementId)) -> Document {
    let mut doc = Document::empty(name);
    doc.id = id;
    let mut el = Element::part_studio(studio.0);
    el.id = studio.1;
    doc.elements.push(el);
    doc
}

// ---------------------------------------------------------------------------------------------
// E1

const fn e1(n: u128) -> FeatureId {
    FeatureId::from_u128(0x53e1_0000_0000_0000_0000_0000_0000_0000 + n)
}

pub const E1_DOCUMENT: DocumentId = DocumentId::from_u128(0x53e1_0000_0000_0000_0000_0000_0000_0100);
pub const E1_STUDIO: ElementId = ElementId::from_u128(0x53e1_0000_0000_0000_0000_0000_0000_0101);
pub const E1_SKETCH: FeatureId = e1(0x11);
pub const E1_MODEL: FeatureId = e1(0x12);
pub const E1_PART: PartId = PartId::new(E1_MODEL, 0);
/// The DXF E1 inserts: P3I.6's flat export of the stand-in tray.
pub const E1_DXF: &str = "fixtures/sheetmetal/flat_pattern_e1.dxf";

/// E1's Bend features' ids (one per bend line, outermost first).
pub fn e1_bend(k: usize) -> FeatureId {
    e1(0x20 + k as u128)
}

/// E1's model settings (`ex1-importing-dxf-bend/step-05.png`): 1 mm, R1, K 0.45, rolled K 0.5,
/// minimal gap 0.025, corner relief Closed, bend relief Tear.
pub fn e1_params() -> cadrs_sheetmetal::Params {
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 1.0;
    p.bend_radius = 1.0;
    p.k_factor = 0.45;
    p.rolled_k_factor = 0.5;
    p.minimal_gap = 0.025;
    p.corner_relief.kind = cadrs_sheetmetal::CornerReliefKind::Closed;
    p.bend_relief.kind = cadrs_sheetmetal::BendReliefKind::Tear;
    p
}

/// Exercise E1 done in Part Studio `el` from the flat DXF's text (see the module docs): Sketch 1
/// on Top with the DXF inserted (mm), Sheet metal model → Thicken of its seven sheet regions
/// ([`e1_params`]), then one Bend per bend line (the short end flanges' first, then outermost first, so
/// each line still lies on a flat wall), Inner alignment, the picked face on the line's up side, the smaller side moving;
/// Carbon Steel.
pub fn build_e1(s: &mut dyn Studio, el: ElementId, dxf: &str) -> Result<(), CommandError> {
    use crate::dxf_import::{DxfUnits, InsertDxf, sketch_of};
    use crate::sheetmetal::CurveRef;
    use crate::sheetmetal_tools::{BendFeature, LineRef};
    let d = cadrs_drawing::dxf::read_dxf(dxf).map_err(|e| CommandError::Invalid(format!("the E1 DXF: {e}")))?;
    let (geometry, _) = sketch_of(&d, DxfUnits::Millimeter, true);
    s.run(&AddSketch { element: el, feature: E1_SKETCH, plane: Some(PlaneRef::Top) })?;
    s.run(&InsertDxf { element: el, feature: E1_SKETCH, geometry })?;
    let g = s.document().element(el).and_then(|e| e.feature(E1_SKETCH)).and_then(|f| f.sketch()).map(|k| k.geometry.clone()).ok_or_else(|| missing("sketch"))?;
    // The bend lines (the DXF's BEND_UP / BEND_DOWN layers) and the sheet: the regions on either
    // side of them and every region not inside another (the cut-outs are holes).
    use cadrs_drawing::sheet_sketch::Entity;
    let bends: Vec<([f64; 2], [f64; 2], bool)> = d
        .entities
        .iter()
        .zip(&d.layers)
        .filter_map(|(e, l)| match e {
            Entity::Line { a, b } if l == "BEND_UP" || l == "BEND_DOWN" => Some((*a, *b, l == "BEND_UP")),
            _ => None,
        })
        .collect();
    let all = cadrs_sketch::region::regions(&g);
    let inside_other = |i: usize| {
        let p = crate::document::interior_point(&all[i]);
        // Inside another region's outer loop (a cut-out is a hole of the sheet around it).
        let within = |l: &[Vec2]| {
            let mut inside = false;
            for k in 0..l.len() {
                let (a, b) = (l[k], l[(k + 1) % l.len()]);
                if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                    inside = !inside;
                }
            }
            inside
        };
        all.iter().enumerate().any(|(j, r)| j != i && r.area() > all[i].area() && within(&r.outer))
    };
    let sheet: Vec<&cadrs_sketch::Region> = (0..all.len()).filter(|i| !inside_other(*i)).map(|i| &all[i]).collect();
    let p = e1_params();
    let x = SheetMetalModelFeature {
        operation: SheetMetalOp::Thicken,
        regions: sheet.iter().map(|r| crate::document::RegionRef::new(E1_SKETCH, r)).collect(),
        params: p,
        exprs: SheetMetalExprs::of(&p),
        ..Default::default()
    };
    add(s, el, E1_MODEL, "Sheet metal model", FeatureKind::SheetMetalModel(x))?;
    // The flat's centre; bend lines outermost first.
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for (a, b, _) in &bends {
        for q in [a, b] {
            for i in 0..2 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
    }
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let mid = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let far = |a: [f64; 2], b: [f64; 2]| {
        let m = mid(a, b);
        (m[0] - c[0]).hypot(m[1] - c[1])
    };
    let mut order: Vec<usize> = (0..bends.len()).collect();
    // The end flanges' short lines first (a long line's band would cross their joints), then
    // the long ones outermost first (a lip's line lies on its side wall only while that is flat).
    let len = |i: usize| (bends[i].1[0] - bends[i].0[0]).hypot(bends[i].1[1] - bends[i].0[1]);
    order.sort_by(|i, j| {
        let (li, lj) = (len(*i).round(), len(*j).round());
        li.total_cmp(&lj).then(far(bends[*j].0, bends[*j].1).total_cmp(&far(bends[*i].0, bends[*i].1)))
    });
    for (k, i) in order.into_iter().enumerate() {
        let (a, b, up) = bends[i];
        let curve = g
            .curves
            .iter()
            .find(|(id, cv)| {
                let cadrs_sketch::CurveKind::Line { a: pa, b: pb } = cv.kind else { return false };
                let _ = id;
                let (qa, qb) = (g.pos(pa), g.pos(pb));
                let near = |q: Vec2, r: [f64; 2]| (q.x - r[0]).hypot(q.y - r[1]) < 1e-6;
                (near(qa, a) && near(qb, b)) || (near(qa, b) && near(qb, a))
            })
            .map(|(id, _)| id)
            .ok_or_else(|| missing("bend line"))?;
        // The face: the staying side (towards the centre), 2 mm off the line, on the up side.
        let m = mid(a, b);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        let mut n = [-dy / len, dx / len];
        if (c[0] - m[0]) * n[0] + (c[1] - m[1]) * n[1] < 0.0 {
            n = [-n[0], -n[1]];
        }
        let seed = [m[0] + 2.0 * n[0], m[1] + 2.0 * n[1], if up { p.thickness } else { 0.0 }];
        let part = sm_part(&built(s, el), E1_MODEL)?;
        let face = FaceRef { seed, ..face_near(&part.solid, part.id, seed).ok_or_else(|| missing("face"))? };
        let bf = BendFeature {
            line: Some(LineRef::Sketch(CurveRef { sketch: E1_SKETCH, curve })),
            face: Some(face),
            alignment: cadrs_sheetmetal::model_edit::BendAlignment::Inner,
            ..Default::default()
        };
        add(s, el, e1_bend(k), "Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(bf)))?;
    }
    s.run(&SetPartMaterial { element: el, parts: vec![E1_PART], material: crate::material::library(CARBON_STEEL) })?;
    Ok(())
}

/// The finished E1 as a document ("Exercise: Import DXF (completed stand-in)"), from the DXF's
/// text.
pub fn document_e1(dxf: &str) -> Result<Document, CommandError> {
    let mut doc = new_document("Exercise: Import DXF (completed stand-in)", E1_DOCUMENT, ("Part Studio 1", E1_STUDIO));
    let mut h = History::default();
    build_e1(&mut DocHistory(&mut doc, &mut h), E1_STUDIO, dxf)?;
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// E2

const fn e2(n: u128) -> FeatureId {
    FeatureId::from_u128(0x53e2_0000_0000_0000_0000_0000_0000_0000 + n)
}

pub const E2_DOCUMENT: DocumentId = DocumentId::from_u128(0x53e2_0000_0000_0000_0000_0000_0000_0100);
pub const E2_STUDIO: ElementId = ElementId::from_u128(0x53e2_0000_0000_0000_0000_0000_0000_0101);
pub const E2_SKETCH_1: FeatureId = e2(0x11);
pub const E2_MODEL: FeatureId = e2(0x12);
pub const E2_FLANGE_1: FeatureId = e2(0x13);
pub const E2_HEM_1: FeatureId = e2(0x14);
pub const E2_SKETCH_2: FeatureId = e2(0x15);
pub const E2_TAB_1: FeatureId = e2(0x16);
pub const E2_FLANGE_2: FeatureId = e2(0x17);
pub const E2_FLANGE_3: FeatureId = e2(0x18);
pub const E2_MAKE_JOINT_1: FeatureId = e2(0x19);
pub const E2_CORNER_1: FeatureId = e2(0x1a);
pub const E2_PART: PartId = PartId::new(E2_MODEL, 0);

/// The profile on Front (x, z): left wall, left base, the trapezoid standing in for the R35 arc,
/// right base, right wall.
pub const E2_CHAIN: [(f64, f64); 8] = [(0.0, 15.0), (0.0, 0.0), (40.0, 0.0), (52.0, 10.0), (73.0, 10.0), (85.0, 0.0), (125.0, 0.0), (125.0, 15.0)];
/// The extrude's depth (along −Y from Front).
pub const E2_DEPTH: f64 = 80.0;

/// E2's model settings (`ex2-creating-sheet-metal-parts/step-03.png`).
pub fn e2_params() -> cadrs_sheetmetal::Params {
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 1.0;
    p.bend_radius = 1.0;
    p.k_factor = 0.45;
    p.rolled_k_factor = 0.5;
    p.minimal_gap = 0.025;
    p.bend_relief.depth_scale = 1.5;
    p.bend_relief.width_scale = 1.0;
    p
}

/// Exercise E2 done, in Part Studio `el` (see the module docs).
pub fn build_e2(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    sketch(s, el, E2_SKETCH_1, PlaneRef::Front, vec![chain(&E2_CHAIN)])?;
    let p = e2_params();
    let x = SheetMetalModelFeature {
        operation: SheetMetalOp::Extrude,
        sketches: vec![E2_SKETCH_1],
        depth: E2_DEPTH,
        depth_expr: "80 mm".into(),
        params: p,
        exprs: SheetMetalExprs::of(&p),
        ..Default::default()
    };
    add(s, el, E2_MODEL, "Sheet metal model", FeatureKind::SheetMetalModel(x))?;
    let part = sm_part(&built(s, el), E2_MODEL)?;
    // Steps 4–5: Flange 1 on the two back edges (the left base's and the left wall's).
    let e = |part: &crate::parts::Part, p: [f64; 3]| edge_near(&part.solid, part.id, p).map(EdgeOrFace::Edge).ok_or_else(|| missing("edge"));
    let fl = FlangeFeature {
        edges: vec![e(&part, [20.0, 0.0, 1.0])?, e(&part, [1.0, 0.0, 8.0])?],
        distance: 8.0,
        distance_expr: "8 mm".into(),
        partial: true,
        chain: ChainType::PerChain,
        hold_adjacent: true,
        bound: Bound { distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() },
        ..Default::default()
    };
    add(s, el, E2_FLANGE_1, "Flange", FeatureKind::SheetMetal(SheetMetalFeature::Flange(fl)))?;
    // Step 6: Hem 1 on the flange's two top edges.
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let h = HemFeature {
        edges: vec![e(&part, [15.0, 0.0, 8.0])?, e(&part, [8.0, 0.0, 13.0])?],
        flattened: true,
        total: 5.0,
        total_expr: "5 mm".into(),
        alignment: cadrs_sheetmetal::model::HemAlignment::InPlace,
        ..Default::default()
    };
    add(s, el, E2_HEM_1, "Hem", FeatureKind::SheetMetal(SheetMetalFeature::Hem(h)))?;
    // Steps 7–8: a 20 × 10 rectangle centred on the right base's front edge, on the base's
    // plane; Tab 1 merges it onto the right base.
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let g = sketch(s, el, E2_SKETCH_2, PlaneRef::Top, vec![polygon(&[(95.0, -90.0), (115.0, -90.0), (115.0, -80.0), (95.0, -80.0)])])?;
    let regions = super::region_refs(E2_SKETCH_2, &g, &[Vec2::new(105.0, -85.0)]);
    let base = face_near(&part.solid, part.id, [105.0, -40.0, 1.0]).ok_or_else(|| missing("right base"))?;
    let tab = TabFeature { regions, flanges: vec![base], ..Default::default() };
    add(s, el, E2_TAB_1, "Tab", FeatureKind::SheetMetalTool(SheetMetalTool::Tab(tab)))?;
    // Steps 9–10: Flange 2 on the front edge beside the tab (as high as the wall), Flange 3 on
    // the right wall's top edge.
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let fl2 = FlangeFeature { edges: vec![e(&part, [120.0, -80.0, 1.0])?], distance: 15.0, distance_expr: "15 mm".into(), ..Default::default() };
    add(s, el, E2_FLANGE_2, "Flange", FeatureKind::SheetMetal(SheetMetalFeature::Flange(fl2)))?;
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let fl3 = FlangeFeature { edges: vec![e(&part, [124.0, -40.0, 15.0])?], distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() };
    add(s, el, E2_FLANGE_3, "Flange", FeatureKind::SheetMetal(SheetMetalFeature::Flange(fl3)))?;
    // Step 11: Make joint 1, rip, butt joint – direction 1.
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let mj = MakeJointFeature {
        edges: vec![e(&part, [123.0, -80.0, 10.0])?, e(&part, [125.0, -80.0, 8.0])?],
        kind: MakeJointType::Rip,
        style: cadrs_sheetmetal::RipStyle::ButtDirection1,
        ..Default::default()
    };
    add(s, el, E2_MAKE_JOINT_1, "Make joint", FeatureKind::SheetMetal(SheetMetalFeature::MakeJoint(mj)))?;
    // Step 12: Corner 1 at the rear-left corner: Round – Sized, Ø3.3.
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let corner = face_near(&part.solid, part.id, [20.0, -10.0, 1.0]).ok_or_else(|| missing("base"))?;
    let mut c = CornerFeature { corner: Some(SmPick::Face(FaceRef { seed: [1.0, -1.0, 1.0], ..corner })), ..Default::default() };
    c.relief.kind = cadrs_sheetmetal::CornerReliefKind::RoundSized;
    c.relief.size = 3.3;
    c.size_expr = "3.3 mm".into();
    add(s, el, E2_CORNER_1, "Corner", FeatureKind::SheetMetalTool(SheetMetalTool::Corner(c)))?;
    // Step 13: Carbon Steel.
    s.run(&SetPartMaterial { element: el, parts: vec![E2_PART], material: crate::material::library(CARBON_STEEL) })?;
    Ok(())
}

/// The finished E2 as a document: "Exercise: Creating sheet metal (completed stand-in)".
pub fn document_e2() -> Result<Document, CommandError> {
    let mut doc = new_document("Exercise: Creating sheet metal (completed stand-in)", E2_DOCUMENT, ("Part Studio 1", E2_STUDIO));
    let mut h = History::default();
    build_e2(&mut DocHistory(&mut doc, &mut h), E2_STUDIO)?;
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// E3

const fn e3(n: u128) -> FeatureId {
    FeatureId::from_u128(0x53e3_0000_0000_0000_0000_0000_0000_0000 + n)
}

pub const E3_DOCUMENT: DocumentId = DocumentId::from_u128(0x53e3_0000_0000_0000_0000_0000_0000_0100);
pub const E3_STUDIO: ElementId = ElementId::from_u128(0x53e3_0000_0000_0000_0000_0000_0000_0101);
pub const E3_SKETCH: FeatureId = e3(0x11);
pub const E3_BLOCK: FeatureId = e3(0x12);
pub const E3_MODEL: FeatureId = e3(0x13);
pub const E3_PART: PartId = PartId::new(E3_MODEL, 0);
/// The box: width, depth, height (mm).
pub const E3_BOX: (f64, f64, f64) = (200.0, 125.0, 150.0);

/// The Sheet Metal Box in Part Studio `el` (see the module docs).
pub fn build_e3(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let (w, d, h) = E3_BOX;
    let g = sketch(s, el, E3_SKETCH, PlaneRef::Top, vec![polygon(&[(0.0, 0.0), (w, 0.0), (w, d), (0.0, d)])])?;
    let mut x = super::extrude_of(super::region_refs(E3_SKETCH, &g, &[Vec2::new(w / 2.0, d / 2.0)]), h);
    x.depth_expr = format!("{h} mm");
    s.run(&AddExtrude { element: el, feature: E3_BLOCK, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: E3_BLOCK, extrude: x, label: "Extrude".into() })?;
    let b = built(s, el);
    let block = b.parts.first().ok_or_else(|| missing("block"))?;
    let top = face_near(&block.solid, block.id, [w / 2.0, d / 2.0, h]).ok_or_else(|| missing("top"))?;
    let bends: Vec<EdgeOrFace> = [[w / 2.0, 0.0, 0.0], [w, d / 2.0, 0.0], [w / 2.0, d, 0.0], [0.0, d / 2.0, 0.0]]
        .iter()
        .map(|q| edge_near(&block.solid, block.id, *q).map(EdgeOrFace::Edge).ok_or_else(|| missing("edge")))
        .collect::<Result<_, _>>()?;
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 1.5;
    p.bend_radius = 1.5;
    let x = SheetMetalModelFeature { operation: SheetMetalOp::Convert, parts: vec![block.id], exclude: vec![top], bends, params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
    add(s, el, E3_MODEL, "Sheet metal model", FeatureKind::SheetMetalModel(x))?;
    s.run(&RenamePart { element: el, part: E3_PART, name: "Sheet Metal Box".into() })?;
    Ok(())
}

/// The document E3 starts from: "Exercise: Drawings (stand-in)".
pub fn document_e3() -> Result<Document, CommandError> {
    let mut doc = new_document("Exercise: Drawings (stand-in)", E3_DOCUMENT, ("Part Studio 1", E3_STUDIO));
    let mut h = History::default();
    build_e3(&mut DocHistory(&mut doc, &mut h), E3_STUDIO)?;
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// E4

const fn e4(n: u128) -> FeatureId {
    FeatureId::from_u128(0x53e4_0000_0000_0000_0000_0000_0000_0000 + n)
}

pub const E4_DOCUMENT: DocumentId = DocumentId::from_u128(0x53e4_0000_0000_0000_0000_0000_0000_0100);
pub const E4_STUDIO: ElementId = ElementId::from_u128(0x53e4_0000_0000_0000_0000_0000_0000_0101);
pub const E4_SKETCH_1: FeatureId = e4(0x11);
pub const E4_MODEL: FeatureId = e4(0x12);
pub const E4_SKETCH_2: FeatureId = e4(0x13);
pub const E4_SLOT: FeatureId = e4(0x14);
pub const E4_PART: PartId = PartId::new(E4_MODEL, 0);
/// The channel on Front (x, z) and its length (along −Y).
pub const E4_CHAIN: [(f64, f64); 4] = [(-35.0, 60.0), (-35.0, 0.0), (35.0, 0.0), (35.0, 60.0)];
pub const E4_LENGTH: f64 = 150.0;
/// The Lower Enclosure's sheet thickness (its walls run from x = ±(35 − 1.5) to ±35).
pub const E4_WALL: f64 = 1.5;
/// The slot on Right (y, z): its centre, length and width.
pub const E4_SLOT_CENTRE: (f64, f64) = (-75.0, 30.0);
pub const E4_SLOT_SIZE: (f64, f64) = (40.0, 12.0);

/// An obround on a sketch plane: two lines and two arcs (counter-clockwise).
pub fn obround(c: (f64, f64), len: f64, wid: f64) -> Vec<SketchOp> {
    let (hx, r) = ((len - wid) / 2.0, wid / 2.0);
    let v = |x: f64, y: f64| Vec2::new(c.0 + x, c.1 + y);
    vec![
        SketchOp::AddPolyline { points: vec![v(-hx, -r), v(hx, -r)], closed: false, construction: false, label: "Add line" },
        SketchOp::AddArc { center: v(hx, 0.0), start: v(hx, -r), end: v(hx, r), construction: false },
        SketchOp::AddPolyline { points: vec![v(hx, r), v(-hx, r)], closed: false, construction: false, label: "Add line" },
        SketchOp::AddArc { center: v(-hx, 0.0), start: v(-hx, r), end: v(-hx, -r), construction: false },
    ]
}

/// The Lower Enclosure in Part Studio `el` (see the module docs).
pub fn build_e4(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    sketch(s, el, E4_SKETCH_1, PlaneRef::Front, vec![chain(&E4_CHAIN)])?;
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = E4_WALL;
    p.bend_radius = 1.5;
    let x = SheetMetalModelFeature {
        operation: SheetMetalOp::Extrude,
        sketches: vec![E4_SKETCH_1],
        depth: E4_LENGTH,
        depth_expr: format!("{E4_LENGTH} mm"),
        params: p,
        exprs: SheetMetalExprs::of(&p),
        ..Default::default()
    };
    add(s, el, E4_MODEL, "Sheet metal model", FeatureKind::SheetMetalModel(x))?;
    // The slot through both side walls: an Extrude → Remove from Right, symmetric, through.
    let g = sketch(s, el, E4_SKETCH_2, PlaneRef::Right, obround(E4_SLOT_CENTRE, E4_SLOT_SIZE.0, E4_SLOT_SIZE.1))?;
    let mut x = super::extrude_of(super::region_refs(E4_SKETCH_2, &g, &[Vec2::new(E4_SLOT_CENTRE.0, E4_SLOT_CENTRE.1)]), 100.0);
    x.depth_expr = "100 mm".into();
    x.symmetric = true;
    x.op = BooleanOp::Remove;
    s.run(&AddExtrude { element: el, feature: E4_SLOT, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: E4_SLOT, extrude: x, label: "Extrude".into() })?;
    s.run(&RenamePart { element: el, part: E4_PART, name: "Lower Enclosure".into() })?;
    // The model named after its part, so the context dropdown lists "Lower Enclosure" (E4 step 10).
    s.run(&crate::commands::RenameFeature { element: el, feature: E4_MODEL, name: "Lower Enclosure".into() })?;
    Ok(())
}

pub const E4_FINISH: FeatureId = e4(0x21);
pub const E4_PATH_SKETCH: FeatureId = e4(0x29);
pub const E4_PLANE: FeatureId = e4(0x22);
pub const E4_RIM_SKETCH: FeatureId = e4(0x23);
pub const E4_SWEEP: FeatureId = e4(0x24);
pub const E4_MIRROR: FeatureId = e4(0x25);
pub const E4_FILLET_1: FeatureId = e4(0x26);
pub const E4_FILLET_2: FeatureId = e4(0x27);
/// The rim (collar) the rework sweeps round the slot: 8 out of the wall, 2 thick.
pub const E4_RIM: (f64, f64) = (8.0, 2.0);
/// How far the collar's profile reaches into the slot and back through the wall: it lines the
/// slot (0.5 in from the cut, so the folded part's polygon slot edge is covered by the sweep's
/// exact faces) from the wall's inside face out.
pub const E4_LINING: f64 = 0.5;
/// Fillet 1 (step 8): where the lined slot meets the wall's inside face; Fillet 2 (step 9): the
/// collar's top outer edge (`ex4-sheet-metal-rework/step-08.png`, `step-09.png`).
pub const E4_FILLETS: (f64, f64) = (3.0, 1.0);

/// Exercise E4 done on the Lower Enclosure in Part Studio `el` (see the module docs): Finish
/// sheet metal model, then [`rework_after_finish`].
pub fn rework_e4(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use crate::sheetmetal_tools::FinishFeature;
    add(s, el, E4_FINISH, "Finish sheet metal model", FeatureKind::SheetMetalTool(SheetMetalTool::Finish(FinishFeature { parts: vec![E4_PART] })))?;
    rework_after_finish(s, el)
}

/// The right wall's outer face (normal +X) round the slot: its name and the X it lies at.
pub fn e4_wall_face(solid: &Solid) -> Option<(cadrs_sketch::FaceName, f64)> {
    solid
        .faces
        .iter()
        .filter_map(|f| {
            let pl = f.plane?;
            let n = pl.normal();
            (n[0] > 0.999 && pl.origin[0] > 0.0 && f.loops.len() > 1).then_some((f.name, pl.origin[0]))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// E4 steps 3–9 after Finish sheet metal model, as the slides make them:
/// - **Sketch 8** on the right wall's outer face, the slot's outline taken with **Use** (two
///   lines, two arcs). The folded part's slot edges are a polygon (cadrs cuts sheet metal in the
///   flat and folds the polygon), so Use takes the slot sketch's curves, which project to the
///   same outline exactly;
/// - **Plane 1**, *Plane point*: the lower line's end (a vertex of Sketch 8) and the Front plane;
/// - **Sketch 9** on Plane 1: the collar's rectangle, from the wall's inside face to [`E4_RIM`]
///   8 out of it, from 2 below the slot's edge to [`E4_LINING`] above it;
/// - **Sweep 1**, Solid, Add: Sketch 9 along Sketch 8, merged into the Lower Enclosure;
/// - **Mirror 1**, Feature mirror of Sweep 1 about the Right plane, *Reapply features*;
/// - **Fillet 1**, 3 mm, where each lined slot meets its wall's inside face; **Fillet 2**, 1 mm,
///   on each collar's top outer edge (tangent propagation takes each loop).
pub fn rework_after_finish(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use crate::advanced::{PathRef, SweepFeature};
    use crate::applied::FilletFeature;
    use crate::pattern::{MirrorFeature, MirrorPlane, PatternType};
    use crate::plane::{PlaneEntity, PlaneFeature, PlaneType};
    let (cy, cz) = E4_SLOT_CENTRE;
    let (len, wid) = E4_SLOT_SIZE;
    let (hx, r) = ((len - wid) / 2.0, wid / 2.0);
    // Step 3: Sketch 8 on the wall face; Use of the slot's four outside edges.
    let b = built(s, el);
    let part = b.parts.iter().find(|p| p.id == E4_PART).ok_or_else(|| missing("part"))?;
    let (wall, x0) = e4_wall_face(&part.solid).ok_or_else(|| missing("wall face"))?;
    let features = s.document().element(el).map(|e| e.active_features()).unwrap_or_default();
    let plane = crate::parts::face_plane(&features, E4_PART.feature, wall).ok_or_else(|| missing("wall face plane"))?;
    let frame = plane.frame();
    // The folded part's slot is a polygon (sheet metal cuts are made in the flat), so Use takes
    // the slot sketch's four curves (two lines, two arcs), projected onto the face: the same
    // outline, exact.
    let slot = features.iter().find(|f| f.id == E4_SKETCH_2).and_then(|f| f.sketch()).ok_or_else(|| missing("slot sketch"))?;
    let ctx = crate::links::LinkContext { solids: b.parts.iter().map(|p| (p.id.feature, &*p.solid)).collect(), features: &features };
    let items = slot
        .geometry
        .curves
        .keys()
        .map(|curve| {
            let link = cadrs_sketch::Link::SketchCurve { feature: E4_SKETCH_2.0, curve };
            ctx.shape(link, &frame).map(|shape| (shape, link)).ok_or_else(|| missing("slot curve's projection"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let g = sketch(s, el, E4_PATH_SKETCH, plane, vec![SketchOp::Use { items }])?;
    // Step 4: Plane 1, Plane point: the lower line's end towards +Y, and Front.
    let corner = frame.to_sketch([x0, cy + hx, cz - r]);
    let point = g.points.iter().min_by(|a, b| (a.1.pos - corner).length().total_cmp(&(b.1.pos - corner).length())).map(|(id, _)| id).ok_or_else(|| missing("slot vertex"))?;
    let pf = PlaneFeature {
        kind: PlaneType::PlanePoint,
        entities: vec![PlaneEntity::SketchPoint { sketch: E4_PATH_SKETCH, point }, PlaneEntity::Plane(PlaneRef::Front)],
        ..Default::default()
    };
    add(s, el, E4_PLANE, "Plane", FeatureKind::Plane(pf))?;
    let b = built(s, el);
    let pframe = b.planes.get(&E4_PLANE).copied().ok_or_else(|| missing("Plane 1"))?;
    // Step 5: Sketch 9 on Plane 1: the rim's 8 × 2 rectangle, out from the wall, under the slot.
    let (w, t) = E4_RIM;
    let y = cy + hx;
    let local = |p: [f64; 3]| {
        let q = pframe.to_sketch(p);
        (q.x, q.y)
    };
    let (xi, zl) = (x0 - E4_WALL, cz - r + E4_LINING);
    let corners = [[xi, y, cz - r - t], [x0 + w, y, cz - r - t], [x0 + w, y, zl], [xi, y, zl]].map(local);
    let on = PlaneRef::Feature(cadrs_sketch::FeaturePlane::new(E4_PLANE.0, pframe));
    let g = sketch(s, el, E4_RIM_SKETCH, on, vec![polygon(&corners)])?;
    let mid = local([x0 + w / 2.0, y, cz - r - t / 2.0]);
    // Step 6: Sweep 1, Add, merged into the Lower Enclosure.
    let regions = super::region_refs(E4_RIM_SKETCH, &g, &[Vec2::new(mid.0, mid.1)]);
    let sweep = SweepFeature { regions, path: vec![PathRef::Sketch(E4_PATH_SKETCH)], op: BooleanOp::Add, merge_scope: vec![E4_PART], ..Default::default() };
    add(s, el, E4_SWEEP, "Sweep", FeatureKind::Sweep(sweep))?;
    // Step 7: Mirror 1, Feature mirror about Right, Reapply features.
    let mirror = MirrorFeature { mirror_type: PatternType::Feature, features: vec![E4_SWEEP], plane: Some(MirrorPlane::Plane(PlaneRef::Right)), reapply: true, ..Default::default() };
    add(s, el, E4_MIRROR, "Mirror", FeatureKind::Mirror(mirror))?;
    // Steps 8–9: the slot's entry on the wall's inside face (3), the collar's top outer edge
    // (1), both sides.
    for (id, x, z, size) in [(E4_FILLET_1, xi, zl, E4_FILLETS.0), (E4_FILLET_2, x0 + w, cz - r - t, E4_FILLETS.1)] {
        let b = built(s, el);
        let part = b.parts.iter().find(|p| p.id == E4_PART).ok_or_else(|| missing("part"))?;
        let entities = [x, -x]
            .iter()
            .map(|x| {
                edge_near(&part.solid, part.id, [*x, cy, z]).map(EdgeOrFace::Edge).ok_or_else(|| {
                    let near = part.solid.edges.iter().map(|e| e.distance([*x, cy, z])).fold(f64::MAX, f64::min);
                    CommandError::Invalid(format!("the stand-in's rim edge is missing (nearest {near}, errors {:?})", b.errors))
                })
            })
            .collect::<Result<_, _>>()?;
        let f = FilletFeature { entities, size, size_expr: format!("{size} mm"), ..Default::default() };
        add(s, el, id, "Fillet", FeatureKind::Fillet(f))?;
    }
    Ok(())
}

/// The document E4 starts from: "Exercise: Sheet metal rework (stand-in)".
pub fn document_e4() -> Result<Document, CommandError> {
    let mut doc = new_document("Exercise: Sheet metal rework (stand-in)", E4_DOCUMENT, ("Enclosures", E4_STUDIO));
    let mut h = History::default();
    build_e4(&mut DocHistory(&mut doc, &mut h), E4_STUDIO)?;
    Ok(doc)
}

/// Every stand-in document file (`fixtures/sheetmetal/<name>.cadrs`), dated 2026-10-03.
pub fn files(e1_dxf: &str) -> Result<Vec<(&'static str, crate::store::DocumentFile)>, CommandError> {
    let file = |document: Document| crate::store::DocumentFile { version: crate::store::SCHEMA_VERSION, meta: crate::library::DocumentMeta::new("cadrs", 1_791_000_000), document };
    Ok(vec![("sm_e1_completed_standin", file(document_e1(e1_dxf)?)), ("sm_e2_completed_standin", file(document_e2()?)), ("sm_e3_standin", file(document_e3()?)), ("sm_e4_standin", file(document_e4()?)), ("sm_topdown_standin", file(super::sheetmetal_topdown::document()?.0)), ("sm_topdown_master", file(super::sheetmetal_topdown::master_document()?.0))])
}
