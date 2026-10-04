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
//!   edge), Flange 2 (a 10 lip on the right wall's top edge, turned in), Flange 3 (10 on the right
//!   wall's vertical front end edge, turned in), Make joint 1 between the lip's end and Flange 3's
//!   top edge (rip, butt joint – direction 1), as the slides' steps 9–11; Corner 1 at the
//!   rear-left corner (Round – Sized, Ø3.3); Carbon Steel.
//! - **E3 "Drawings"** ([`build_e3`]): the "Sheet Metal Box", the exercise's sloped enclosure
//!   ([`E3`]): a side profile (200 high at the back, 125 at the front, a 75 deep shelf, 250 deep)
//!   extruded 200 and converted (1.5 mm, R1.5, material inside) with its slope left open, then
//!   flanged (35 mm) along both sloping edges: seven bends, two of them oblique in the flat.
//! - **E4 "Sheet metal rework"** ([`build_e4`]): the "Lower Enclosure": a U-channel (Front-plane
//!   chain, 70 wide, 60 high walls, 150 long, 1.5 mm, R1.5) with an obround slot cut through both
//!   side walls (a perpendicular cut, in the flat). [`rework_e4`] does the exercise: Finish sheet
//!   metal model, a sketch of the slot's outline on the right wall, a plane at its lowest point
//!   normal to it, a 2 × 8 rectangle swept round the outline (Add), mirrored about the Right
//!   plane, and the rims' edges filleted; the flat doesn't change.
//!
//! E1 ("Importing DXF & Bend") starts from a DXF of the flat (`fixtures/sm_e1_flat.dxf`), made by
//! P3I.6.

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

/// Exercise E1 done in Part Studio `el` from the flat DXF's text (see the module docs): Sketch 1
/// on Top with the DXF inserted (mm), Sheet metal model → Thicken of its seven sheet regions
/// (1 mm, R1), then one Bend per bend line (the short end flanges' first, then outermost first, so
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
    let p = {
        let mut p = SheetMetalModelFeature::default_params();
        p.thickness = 1.0;
        p.bend_radius = 1.0;
        p
    };
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
/// Step 9: the right wall's top edge, on its inside face (x = 124).
pub const E2_FLANGE_2_EDGE: [f64; 3] = [124.0, -40.0, 15.0];
/// Step 10: the right wall's vertical front end edge, on its inside face.
pub const E2_FLANGE_3_EDGE: [f64; 3] = [124.0, -80.0, 8.0];
/// Step 11: the lip's front end (its top edge there) and Flange 3's top edge (its inside).
pub const E2_JOINT_EDGES: [[f64; 3]; 2] = [[119.0, -80.0, 16.0], [119.0, -80.0, 14.0]];

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
    // Steps 9–10 (`step-09.png`, `-10.png`): Flange 2, a 10 lip on the right wall's top edge,
    // turned in over the base (its inside edge picked); Flange 3, 10 on the right wall's vertical
    // end edge at the front, turned in too, as tall as the wall between its bends.
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let fl2 = FlangeFeature { edges: vec![e(&part, E2_FLANGE_2_EDGE)?], distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() };
    add(s, el, E2_FLANGE_2, "Flange", FeatureKind::SheetMetal(SheetMetalFeature::Flange(fl2)))?;
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let fl3 = FlangeFeature { edges: vec![e(&part, E2_FLANGE_3_EDGE)?], distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() };
    add(s, el, E2_FLANGE_3, "Flange", FeatureKind::SheetMetal(SheetMetalFeature::Flange(fl3)))?;
    // Step 11 (`step-11.png`): Make joint 1 between "Edge of Flange 2" (the lip's front end) and
    // "Edge of Flange 3" (its top edge): rip, butt joint – direction 1.
    let part = sm_part(&built(s, el), E2_MODEL)?;
    let mj = MakeJointFeature {
        edges: vec![e(&part, E2_JOINT_EDGES[0])?, e(&part, E2_JOINT_EDGES[1])?],
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
pub const E3_FLANGE: FeatureId = e3(0x14);
pub const E3_PART: PartId = PartId::new(E3_MODEL, 0);

/// The enclosure's sizes (mm): the exercise's "Enclosure for widget" (`ex3-drawings/step-06`:
/// 200 wide, 200 high at the back, 125 at the front, a 75 deep shelf at the top of the back, the
/// top sloping down to the front at 23.2°; 250 deep, so the flat's 642.25 long strip in
/// `goal.png` is 125 + 250 + 200 + 75 less three bend deductions), and its slope flanges (35).
pub struct E3Size {
    pub width: f64,
    pub depth: f64,
    pub back: f64,
    pub front: f64,
    pub shelf: f64,
    pub flange: f64,
}

pub const E3: E3Size = E3Size { width: 200.0, depth: 250.0, back: 200.0, front: 125.0, shelf: 75.0, flange: 35.0 };

/// The Sheet Metal Box in Part Studio `el` (see the module docs): the side profile on Right
/// (front at y = 0), extruded 200 along X; converted (1.5 mm, R1.5, the material inside) with
/// the slope excluded, the front, back and both sides bent off the bottom and the shelf off the
/// back; then a 35 mm Flange (Hold line, folded in; `goal.png`'s top view) on each side wall's
/// sloping edge.
/// Seven bends, two of them oblique in the flat (`ex3-drawings/goal.png`).
pub fn build_e3(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let E3Size { width: w, depth: d, back: hb, front: hf, shelf: sh, flange } = E3;
    let profile = [(0.0, 0.0), (d, 0.0), (d, hb), (d - sh, hb), (0.0, hf)];
    let g = sketch(s, el, E3_SKETCH, PlaneRef::Right, vec![polygon(&profile)])?;
    let mut x = super::extrude_of(super::region_refs(E3_SKETCH, &g, &[Vec2::new(d / 2.0, hf / 2.0)]), w);
    x.depth_expr = format!("{w} mm");
    s.run(&AddExtrude { element: el, feature: E3_BLOCK, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: E3_BLOCK, extrude: x, label: "Extrude".into() })?;
    let b = built(s, el);
    let block = b.parts.first().ok_or_else(|| missing("block"))?;
    // The slope's middle (its face is left out: the top is open there).
    let slope_mid = [(d - sh) / 2.0, (hf + hb) / 2.0];
    let slope = face_near(&block.solid, block.id, [w / 2.0, slope_mid[0], slope_mid[1]]).ok_or_else(|| missing("slope"))?;
    // The front, the back, the two sides off the bottom; the shelf off the back.
    let bends: Vec<EdgeOrFace> = [[w / 2.0, 0.0, 0.0], [w / 2.0, d, 0.0], [0.0, d / 2.0, 0.0], [w, d / 2.0, 0.0], [w / 2.0, d, hb]]
        .iter()
        .map(|q| edge_near(&block.solid, block.id, *q).map(EdgeOrFace::Edge).ok_or_else(|| missing("edge")))
        .collect::<Result<_, _>>()?;
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 1.5;
    p.bend_radius = 1.5;
    // Bends read DOWN seen from the flat's top, as `goal.png`'s.
    p.flip_direction_up = true;
    // The material inside the block (its faces are the sheet's outside), so the box's sizes are
    // the exercise's outer ones.
    let x = SheetMetalModelFeature { operation: SheetMetalOp::Convert, parts: vec![block.id], exclude: vec![slope], bends, params: p, exprs: SheetMetalExprs::of(&p), flip_thickness: true, ..Default::default() };
    add(s, el, E3_MODEL, "Sheet metal model", FeatureKind::SheetMetalModel(x))?;
    // Flange 1 on both side walls' sloping edges.
    let part = sm_part(&built(s, el), E3_MODEL)?;
    let edges = [0.0, w]
        .iter()
        .map(|x| edge_near(&part.solid, part.id, [*x, slope_mid[0], slope_mid[1]]).map(EdgeOrFace::Edge).ok_or_else(|| missing("sloping edge")))
        .collect::<Result<Vec<_>, _>>()?;
    // Folded in over the opening, the same way as the walls (all seven bends DOWN, `goal.png`).
    // Bent from the edge (Hold line): an Inner flange folded inward runs into the front wall
    // and the shelf at its ends here (a Flange bug on sloping edges, being fixed in P3I.4).
    let fl = FlangeFeature {
        edges,
        distance: flange,
        distance_expr: format!("{flange} mm"),
        flip: true,
        alignment: cadrs_sheetmetal::sharp_edit::FlangeAlignment::HoldLine,
        ..Default::default()
    };
    add(s, el, E3_FLANGE, "Flange", FeatureKind::SheetMetal(SheetMetalFeature::Flange(fl)))?;
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

/// [`obround`] with its long side along the sketch's v axis.
pub fn obround_v(c: (f64, f64), len: f64, wid: f64) -> Vec<SketchOp> {
    let (hy, r) = ((len - wid) / 2.0, wid / 2.0);
    let v = |x: f64, y: f64| Vec2::new(c.0 + x, c.1 + y);
    vec![
        SketchOp::AddPolyline { points: vec![v(r, -hy), v(r, hy)], closed: false, construction: false, label: "Add line" },
        SketchOp::AddArc { center: v(0.0, hy), start: v(r, hy), end: v(-r, hy), construction: false },
        SketchOp::AddPolyline { points: vec![v(-r, hy), v(-r, -hy)], closed: false, construction: false, label: "Add line" },
        SketchOp::AddArc { center: v(0.0, -hy), start: v(-r, -hy), end: v(r, -hy), construction: false },
    ]
}

/// The Lower Enclosure in Part Studio `el` (see the module docs).
pub fn build_e4(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    sketch(s, el, E4_SKETCH_1, PlaneRef::Front, vec![chain(&E4_CHAIN)])?;
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 1.5;
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
pub const E4_WALL_PLANE: FeatureId = e4(0x28);
pub const E4_PATH_SKETCH: FeatureId = e4(0x29);
pub const E4_PLANE: FeatureId = e4(0x22);
pub const E4_RIM_SKETCH: FeatureId = e4(0x23);
pub const E4_SWEEP: FeatureId = e4(0x24);
pub const E4_MIRROR: FeatureId = e4(0x25);
pub const E4_FILLET_1: FeatureId = e4(0x26);
pub const E4_FILLET_2: FeatureId = e4(0x27);
/// The rim the rework sweeps round the slot: 8 out of the wall, 2 thick.
pub const E4_RIM: (f64, f64) = (8.0, 2.0);

/// Exercise E4 done on the Lower Enclosure in Part Studio `el` (see the module docs): Finish
/// sheet metal model, a plane through the slot's lowest point parallel to Front, a 2 × 8
/// rectangle on it swept (Add) along the slot's outer edges, the sweep mirrored about Right
/// (Feature mirror), the rims' outer and inner edges filleted (0.5).
pub fn rework_e4(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use crate::sheetmetal_tools::FinishFeature;
    add(s, el, E4_FINISH, "Finish sheet metal model", FeatureKind::SheetMetalTool(SheetMetalTool::Finish(FinishFeature { parts: vec![E4_PART] })))?;
    rework_after_finish(s, el)
}

/// E4 steps 3–9 (after Finish sheet metal model): the slot's outline sketched, Plane 1, the
/// rectangle, Sweep 1, Mirror 1 and the two fillets.
pub fn rework_after_finish(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use crate::advanced::{PathRef, SweepFeature};
    use crate::applied::FilletFeature;
    use crate::pattern::{MirrorFeature, MirrorPlane, PatternType};
    use crate::plane::{PlaneEntity, PlaneFeature, PlaneType};
    // Sketch 8: the slot's outline on the right wall's outer face (x = 35), as the exercise's
    // Use of the obround's edge gives it: on a plane offset from Right.
    let (cy, cz) = E4_SLOT_CENTRE;
    let r = E4_SLOT_SIZE.1 / 2.0;
    let x0 = E4_CHAIN[3].0;
    let wall = PlaneFeature { kind: PlaneType::Offset, entities: vec![PlaneEntity::Plane(PlaneRef::Right)], offset: x0, offset_expr: format!("{x0} mm"), ..Default::default() };
    add(s, el, E4_WALL_PLANE, "Plane", FeatureKind::Plane(wall))?;
    let wf = built(s, el).planes.get(&E4_WALL_PLANE).copied().ok_or_else(|| missing("wall plane"))?;
    let on_wall = |p: [f64; 3]| {
        let d = [p[0] - wf.origin[0], p[1] - wf.origin[1], p[2] - wf.origin[2]];
        let dot = |a: [f64; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
        (dot(wf.u), dot(wf.v))
    };
    let c = on_wall([x0, cy, cz]);
    // The obround in the plane's coordinates (its long side along Y, whichever way u runs).
    let along_u = wf.u[1].abs() > 0.5;
    let (len, wid) = E4_SLOT_SIZE;
    let ops = if along_u { obround(c, len, wid) } else { obround_v(c, len, wid) };
    sketch(s, el, E4_PATH_SKETCH, PlaneRef::Feature(cadrs_sketch::FeaturePlane::new(E4_WALL_PLANE.0, wf)), ops)?;
    let path = vec![PathRef::Sketch(E4_PATH_SKETCH)];
    // Plane 1: Front through the slot's lowest point (Plane point), at y = cy.
    let plane = PlaneFeature { kind: PlaneType::Offset, entities: vec![PlaneEntity::Plane(PlaneRef::Front)], offset: -cy, offset_expr: format!("{} mm", -cy), ..Default::default() };
    add(s, el, E4_PLANE, "Plane", FeatureKind::Plane(plane))?;
    let b = built(s, el);
    let frame = b.planes.get(&E4_PLANE).copied().ok_or_else(|| missing("plane"))?;
    let on = cadrs_sketch::FeaturePlane::new(E4_PLANE.0, frame);
    // The 8 × 2 rectangle below the slot's lowest edge, out from the wall (in the plane's own
    // coordinates).
    let local = |p: [f64; 3]| {
        let d = [p[0] - frame.origin[0], p[1] - frame.origin[1], p[2] - frame.origin[2]];
        let dot = |a: [f64; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
        (dot(frame.u), dot(frame.v))
    };
    let (w, t) = E4_RIM;
    let corners = [[x0, cy, cz - r - t], [x0 + w, cy, cz - r - t], [x0 + w, cy, cz - r], [x0, cy, cz - r]].map(local);
    let g = sketch(s, el, E4_RIM_SKETCH, PlaneRef::Feature(on), vec![polygon(&corners)])?;
    let mid = local([x0 + w / 2.0, cy, cz - r - t / 2.0]);
    let regions = super::region_refs(E4_RIM_SKETCH, &g, &[Vec2::new(mid.0, mid.1)]);
    let sweep = SweepFeature { regions, path, op: BooleanOp::Add, merge_scope: vec![E4_PART], ..Default::default() };
    add(s, el, E4_SWEEP, "Sweep", FeatureKind::Sweep(sweep))?;
    let mirror = MirrorFeature { mirror_type: PatternType::Feature, features: vec![E4_SWEEP], plane: Some(MirrorPlane::Plane(PlaneRef::Right)), ..Default::default() };
    add(s, el, E4_MIRROR, "Mirror", FeatureKind::Mirror(mirror))?;
    // The rims' outer and inner top edges, both sides.
    for (id, z) in [(E4_FILLET_1, cz - r - t), (E4_FILLET_2, cz - r)] {
        let b = built(s, el);
        let part = b.parts.iter().find(|p| p.id == E4_PART).ok_or_else(|| missing("part"))?;
        let entities = [x0 + w, -(x0 + w)]
            .iter()
            .map(|x| {
                edge_near(&part.solid, part.id, [*x, cy, z]).map(EdgeOrFace::Edge).ok_or_else(|| {
                    let near = part.solid.edges.iter().map(|e| e.distance([*x, cy, z])).fold(f64::MAX, f64::min);
                    let xs = part.solid.positions.iter().map(|p| p[0]).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
                    CommandError::Invalid(format!("the stand-in's rim edge is missing (nearest {near}, x {xs:?}, errors {:?})", b.errors))
                })
            })
            .collect::<Result<_, _>>()?;
        let f = FilletFeature { entities, size: 0.5, size_expr: "0.5 mm".into(), ..Default::default() };
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
    Ok(vec![("sm_e1_completed_standin", file(document_e1(e1_dxf)?)), ("sm_e2_completed_standin", file(document_e2()?)), ("sm_e3_standin", file(document_e3()?)), ("sm_e4_standin", file(document_e4()?)), ("sm_topdown_standin", file(super::sheetmetal_topdown::document()?.0))])
}
