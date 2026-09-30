//! A PCB board built into a Part Studio through the command layer: one sketch on Top and one
//! extrude (New) per body, each part named and coloured as [`crate::geometry`] makes it. This is
//! what makes the P3H.2 geometry visible before the PCB Studio tab exists
//! (`course_pcb_geometry_*` scenarios), and the Part Studio half of Create assembly (PCB7.2:
//! "a feature named Board [<board>]", the part "Board [<board>]").
//!
//! The Part Studio holds the board, the keep areas that concern placement (place keep-outs,
//! place regions and place outlines: the keep-in/keep-out parts that Sync reads back) and the
//! components. Route and via keep-outs are routing rules with no MCAD part; they are left out
//! (they are in [`crate::board_geometry`]). Every feature id is derived from the board name, so
//! a scenario builds the same document every time.

use cadrs_core::appearance::Appearance;
use cadrs_core::command::CommandError;
use cadrs_core::commands::{AddExtrude, AddSketch, CreateFolder, EditSketch, RenameFeature, RenamePart, SetPartAppearance};
use cadrs_core::document::{ExtrudeFeature, Offset};
use cadrs_core::ids::{ElementId, FeatureId, PartId};
use cadrs_core::studio::Studio;
use cadrs_idf::Segment;
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::board::{KeepKind, PcbBoard};
use crate::colors::BodyClass;
use crate::geometry::{BodyPlan, all_keep_plans, board_plan, component_plan};

/// FNV-1a of the board name, so ids differ between boards.
pub(crate) fn name_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub(crate) fn fid(board: u64, n: u64) -> FeatureId {
    FeatureId::from_u128(0x9cb0_0000_0000_0000_0000_0000_0000_0000 | ((board as u128) << 32) | n as u128)
}

/// The plans the Part Studio gets, in feature order: the board, the place keep areas, the
/// components.
pub fn studio_plans(pcb: &PcbBoard) -> Vec<BodyPlan> {
    let mut plans: Vec<BodyPlan> = board_plan(pcb).into_iter().collect();
    plans.extend(all_keep_plans(pcb, |k| matches!(k.kind, KeepKind::PlaceKeepout | KeepKind::PlaceRegion | KeepKind::PlaceOutline)));
    plans.extend(pcb.components().map(|(id, p)| component_plan(pcb, id, p)));
    plans
}

pub(crate) fn loop_ops(l: &cadrs_idf::Loop) -> Vec<SketchOp> {
    let v = |p: [f64; 2]| Vec2::new(p[0], p[1]);
    l.segments()
        .filter_map(|s| match s {
            Segment::Line { start, end } => ((start[0] - end[0]).hypot(start[1] - end[1]) > 1e-9).then(|| SketchOp::AddPolyline {
                points: vec![v(start), v(end)],
                closed: false,
                construction: false,
                label: "Add line",
            }),
            Segment::Arc { start, end, center, sweep, .. } => {
                // The sketch's arcs run counter-clockwise from start to end.
                let (a, b) = if sweep > 0.0 { (start, end) } else { (end, start) };
                Some(SketchOp::AddArc { center: v(center), start: v(a), end: v(b), construction: false })
            }
            Segment::Circle { center, radius } => Some(SketchOp::AddCircle { center: v(center), radius, construction: false }),
        })
        .collect()
}

pub(crate) fn mm(v: f64) -> String {
    format!("{} mm", cadrs_idf::fmt_num(v))
}

/// A part the sample made.
#[derive(Clone, Debug, PartialEq)]
pub struct SamplePart {
    pub part: PartId,
    pub name: String,
    pub class: BodyClass,
    pub extrude: FeatureId,
}

/// Builds one plan into the Part Studio `el` as a sketch on Top (`sketch`) and a New extrude
/// (`extrude`) named after the plan, its part renamed and coloured likewise. Returns the part.
pub fn build_plan(s: &mut dyn Studio, el: ElementId, plan: &BodyPlan, sketch: FeatureId, extrude: FeatureId) -> Result<PartId, CommandError> {
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(PlaneRef::Top) })?;
    let ops: Vec<SketchOp> = plan.loops.iter().flat_map(loop_ops).collect();
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::Batch(ops) })?;
    let depth = plan.depth.abs();
    let down = plan.depth < 0.0;
    // The start offset runs along the extrude direction: up for a body above the board,
    // down (flipped extrude) for one below it.
    let start = if down { -plan.z0 } else { plan.z0 };
    let start_offset = (start.abs() > 1e-9).then(|| Offset { value: start.abs(), expr: mm(start.abs()), flip: start < 0.0 });
    let e = ExtrudeFeature { sketches: vec![sketch], depth, depth_expr: mm(depth), flip: down, start_offset, ..ExtrudeFeature::default() };
    s.run(&AddExtrude { element: el, feature: extrude, extrude: e })?;
    s.run(&RenameFeature { element: el, feature: extrude, name: plan.name.clone() })?;
    let part = PartId::new(extrude, 0);
    s.run(&RenamePart { element: el, part, name: plan.name.clone() })?;
    let [r, g, b, a] = plan.class.color();
    s.run(&SetPartAppearance { element: el, parts: vec![part], appearance: Some(Appearance::rgb(r, g, b).with_alpha(a)) })?;
    Ok(part)
}

/// Builds the board into the Part Studio `el` (see the module docs). Returns the parts in
/// feature order.
pub fn build_in(s: &mut dyn Studio, el: ElementId, pcb: &PcbBoard) -> Result<Vec<SamplePart>, CommandError> {
    let bh = name_hash(pcb.name());
    let mut out = Vec::new();
    let (mut keep_features, mut comp_features) = (Vec::new(), Vec::new());
    for (i, plan) in studio_plans(pcb).iter().enumerate() {
        let sketch = fid(bh, 16 + 2 * i as u64);
        let extrude = fid(bh, 17 + 2 * i as u64);
        let part = build_plan(s, el, plan, sketch, extrude)?;
        match plan.class {
            BodyClass::Board => {}
            BodyClass::KeepIn | BodyClass::KeepOut | BodyClass::Other => keep_features.extend([sketch, extrude]),
            _ => comp_features.extend([sketch, extrude]),
        }
        out.push(SamplePart { part, name: plan.name.clone(), class: plan.class, extrude });
    }
    for (n, name, features) in [(1, "Keep areas", keep_features), (2, "Components", comp_features)] {
        if !features.is_empty() {
            s.run(&CreateFolder { element: el, folder: fid(bh, n), name: Some(name.into()), features })?;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// A custom representation (P3H.4, X10)

/// The name of [`custom_part_document`] and of its part.
pub const CUSTOM_PART_DOCUMENT: &str = "QFP100 heatsink model";
pub const CUSTOM_PART_NAME: &str = "QFP100 with heatsink";

fn rect_ops(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let p = |x: f64, y: f64| Vec2::new(x, y);
    SketchOp::AddPolyline { points: vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)], closed: true, construction: false, label: "Add rectangle" }
}

/// A small Part Studio document to pick a custom part from (the `course_pcb_custom_part`
/// scenario's fixture): one part, a 14 × 14 × 1.4 mm package body with an 8 × 9 × 2 mm heatsink
/// block on top, off centre towards −x so a rotation shows (3.4 mm tall), modelled with a corner at the origin, so it needs Center (or a
/// translate of −7, −7) to sit on a QFP100 footprint. Built through the command layer; the ids
/// are fixed so the document is the same every time.
pub fn custom_part_document() -> Result<(cadrs_core::Document, ElementId, PartId), CommandError> {
    let mut doc = cadrs_core::Document::new(CUSTOM_PART_DOCUMENT);
    doc.elements.truncate(1);
    let el = doc.elements[0].id;
    doc.elements[0].name = "QFP100 model".into();
    let mut h = cadrs_core::History::default();
    let mut s = cadrs_core::studio::DocHistory(&mut doc, &mut h);
    let f = |n: u64| fid(0x51, n);
    let (base, top) = (1.4, 2.0);
    s.run(&AddSketch { element: el, feature: f(1), plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: f(1), op: rect_ops(0.0, 0.0, 14.0, 14.0) })?;
    let body = ExtrudeFeature { sketches: vec![f(1)], depth: base, depth_expr: mm(base), ..ExtrudeFeature::default() };
    s.run(&AddExtrude { element: el, feature: f(2), extrude: body })?;
    s.run(&AddSketch { element: el, feature: f(3), plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: f(3), op: rect_ops(1.5, 2.5, 9.5, 11.5) })?;
    let sink = ExtrudeFeature {
        sketches: vec![f(3)],
        depth: top,
        depth_expr: mm(top),
        start_offset: Some(Offset { value: base, expr: mm(base), flip: false }),
        op: cadrs_core::document::BooleanOp::Add,
        merge_all: true,
        ..ExtrudeFeature::default()
    };
    s.run(&AddExtrude { element: el, feature: f(4), extrude: sink })?;
    let part = PartId::new(f(2), 0);
    s.run(&RenamePart { element: el, part, name: CUSTOM_PART_NAME.into() })?;
    s.run(&SetPartAppearance { element: el, parts: vec![part], appearance: Some(Appearance::rgb(70, 74, 82)) })?;
    Ok((doc, el, part))
}

// ---------------------------------------------------------------------------------------------
// A Part Studio to sync (P3H.5, PCB5.1, PCB5.4)

/// Builds into the Part Studio `el` a **Mainboard** (60 × 40 × 1.6 mm, x −30…30, y −20…20,
/// z 0…1.6, green), a **Keepout** (10 × 10 × 3 mm on the board's top face at x 10…20,
/// y −5…5, dark translucent) and an **Enclosure** (a 70 × 50 × 12 mm box round them, z −3…9,
/// translucent grey): what `course_pcb_sync_partstudio` syncs (a board with one keep-out; the
/// Enclosure not translated). The ids are fixed.
pub fn sync_demo_studio(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let f = |n: u64| fid(0x5d, n);
    type Block<'a> = (&'a str, [f64; 4], f64, f64, [u8; 4]);
    let blocks: [Block; 3] = [
        ("Mainboard", [-30.0, -20.0, 30.0, 20.0], 0.0, 1.6, [30, 120, 40, 255]),
        ("Keepout", [10.0, -5.0, 20.0, 5.0], 1.6, 3.0, [40, 40, 40, 150]),
        ("Enclosure", [-35.0, -25.0, 35.0, 25.0], -3.0, 12.0, [190, 194, 200, 70]),
    ];
    for (i, (name, [x0, y0, x1, y1], z0, depth, [r, g, b, a])) in blocks.into_iter().enumerate() {
        let (sk, ex) = (f(1 + 2 * i as u64), f(2 + 2 * i as u64));
        s.run(&AddSketch { element: el, feature: sk, plane: Some(PlaneRef::Top) })?;
        s.run(&EditSketch { element: el, feature: sk, op: rect_ops(x0, y0, x1, y1) })?;
        let start_offset = (z0 != 0.0).then(|| Offset { value: z0.abs(), expr: mm(z0.abs()), flip: z0 < 0.0 });
        let e = ExtrudeFeature { sketches: vec![sk], depth, depth_expr: mm(depth), start_offset, ..ExtrudeFeature::default() };
        s.run(&AddExtrude { element: el, feature: ex, extrude: e })?;
        let part = PartId::new(ex, 0);
        s.run(&RenamePart { element: el, part, name: name.into() })?;
        s.run(&SetPartAppearance { element: el, parts: vec![part], appearance: Some(Appearance::rgb(r, g, b).with_alpha(a)) })?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// PCB10 step 6: the keep-out sketch (P3H.6)

/// The Ex3 keep-out profile's size (mm): 0.5 × 0.375 in with an R0.25 in fillet.
pub const EX3_KEEPOUT: [f64; 3] = [12.7, 9.525, 6.35];

/// PCB10 step 6 in the board's Part Studio `el` (made by Create assembly): a sketch `sketch` on
/// the **top face of the board part** ("Board [<board>]") with a 0.5 × 0.375 in rectangle at
/// the board's bottom-left corner (the lowest-x, lowest-y corner of its outline), its inner
/// corner filleted R0.25 in, dimensioned 0.5 (bottom), 0.375 (left) and R0.25 like
/// `ex3-step6-keepout-sketch.png`. Several commands; the caller makes them one undo step.
pub fn ex3_keepout_sketch(s: &mut dyn Studio, el: ElementId, sketch: FeatureId) -> Result<(), CommandError> {
    use cadrs_sketch::{Dimension, DimensionKind};
    let e = s.document().element(el).ok_or(CommandError::ElementNotFound(el))?;
    let features = e.active_features();
    let props = e.part_props().to_vec();
    let build = cadrs_core::rebuild::build(&features);
    let board = build
        .parts
        .iter()
        .find(|p| cadrs_core::parts::display_name(p, &props).starts_with("Board ["))
        .ok_or_else(|| CommandError::Invalid("no board part in this Part Studio".into()))?;
    let top = board
        .solid
        .faces
        .iter()
        .filter(|f| f.plane.is_some_and(|fr| fr.normal()[2] > 0.999))
        .max_by(|a, b| a.plane.unwrap().origin[2].total_cmp(&b.plane.unwrap().origin[2]))
        .ok_or_else(|| CommandError::Invalid("the board has no top face".into()))?;
    let plane = cadrs_core::parts::face_plane(&features, board.feature, top.name).ok_or_else(|| CommandError::Invalid("no plane on the board's top face".into()))?;
    let frame = plane.frame();
    let z = top.plane.unwrap().origin[2];
    // The outline's bottom-left corner.
    let (x0, y0) = board.solid.positions.iter().fold((f64::INFINITY, f64::INFINITY), |(x, y), p| (x.min(p[0]), y.min(p[1])));
    let [w, d, r] = EX3_KEEPOUT;
    let at = |x: f64, y: f64| frame.to_sketch([x, y, z]);
    let corners = [at(x0, y0), at(x0 + w, y0), at(x0 + w, y0 + d), at(x0, y0 + d)];
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(plane) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::AddPolyline { points: corners.to_vec(), closed: true, construction: false, label: "Add rectangle" } })?;
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::AddConstraints(cadrs_sketch::constraint::rectangle_constraints(corners)) })?;
    let g = || -> Result<cadrs_sketch::Sketch, CommandError> {
        s.document()
            .element(el)
            .and_then(|e| e.feature(sketch))
            .and_then(|f| f.sketch())
            .map(|k| k.geometry.clone())
            .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
    };
    let geo = g()?;
    let point = |p: Vec2| geo.point_at(p, 1e-6).ok_or_else(|| CommandError::Invalid("a corner is missing".into()));
    let (a, b, c, dd) = (point(corners[0])?, point(corners[1])?, point(corners[2])?, point(corners[3])?);
    let dim = |kind: DimensionKind, value: f64, offset: f64| SketchOp::SetDimension { dimension: Dimension { kind, value, offset, along: 0.0, driven: false }, moves: vec![], radii: vec![] };
    s.run(&EditSketch { element: el, feature: sketch, op: dim(DimensionKind::Horizontal { a, b }, w, -4.0) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: dim(DimensionKind::Vertical { a, b: dd }, d, -4.0) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::Fillet { corner: c, radius: r, equal_to: None } })?;
    Ok(())
}
