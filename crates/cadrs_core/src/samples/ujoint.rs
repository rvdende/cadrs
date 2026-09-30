//! The Universal Joint Flange stand-in (P3C.3, D8, `fixtures/ujoint_flange_standin.cadrs`).
//!
//! Onshape's public "Exercise: Universal Joint Drawing" document can't be copied, so the drawing
//! exercise starts from this yoke, built from the course drawing's own dimensions
//! (`ex1-drawing.png`, `ex1-step9.png`) with our own features, in inches. Z is up; the flange's
//! top face (the floor of the slot) is the Top plane (z = 0), so the counterbored holes can be
//! sketched on Top and drilled down from it.
//!
//! - **Main Body** (Sketch 1 / Extrude 1): the Ø4.750 disc on Top, extruded from 0.500 below the
//!   plane (a starting offset) up to z = 5.500, so the part is **6.000** tall overall.
//! - **Slot** (Sketch 2 / Extrude 2): on Front, the region between the lugs, **2.600** wide,
//!   removed through the whole part (symmetric) from the floor up: the U of the front view.
//! - **Lug profile** (Sketch 3 / Extrude 3): on Right, everything outside the lugs' side
//!   profile, removed through the part: the lugs are **2.500** wide, their top corners are cut
//!   at **43.0°** to the top edge (0.500 in from each corner along the top), and at the base
//!   they flare out to the flange at **120.0°** to the flange's top face (the flare runs 0.750
//!   out from each side, at 60° to the floor; the Ø4.750 disc trims its foot just above the
//!   floor).
//! - **Flats** (Sketch 9 / Extrude 8): on Top, everything beyond x = ±2.000 removed from the floor
//!   up: the lugs' outer faces are flat (the flange stays round).
//! - **Bosses** (Sketch 4 / Extrude 4, Extrude 5): a **Ø1.750** boss on each lug's flat, 0.125
//!   proud (x = ±2.125), centred 1.750 below the top (z = 3.750).
//! - **Cross hole** (Sketch 5 / Extrude 6): **Ø1.250** through both lugs and bosses.
//! - **Pocket** (Sketch 6 / Extrude 7): a 2.000 × 2.000 square pocket 0.250 deep in the floor.
//! - **Counterbored holes** (Sketch 7 / Hole 1): four points on a **2.061 × 3.282** rectangle of
//!   hole centres (x ±1.0305, y ±1.641) on Top, a Hole feature: Inch, Counterbore, Clearance
//!   1/4 Normal (**Ø.266**), Through all, the counterbore **Ø.438 × .250** (the table gives
//!   Ø.406 for a 1/4 socket head; the course's drawing shows .438, so it is typed in).
//! - **Lug holes** (Sketch 8 / Hole 2, Hole 3): four points on a Ø2.125 bolt circle around the
//!   cross hole (at 45°, clear of the boss) on Right, and two Hole features, Inch, Simple, Clearance 1/4 Normal
//!   (**Ø.266**), Through all, one drilled towards −x and one flipped towards +x (a hole goes
//!   through what it meets from its start on, so each lug needs its own): 8 holes in all.
//! - **Fillet 1**: R.250 on the four concave edges where the flares meet the lugs' sides (they
//!   give the views their tangent edges).
//! - **Chamfer 1**: .060 × 45° on the flange's bottom rim.
//!
//! The part is "Universal Joint Flange (stand-in)", described "Made by cadrs" (the title block
//! shows both, like the course's "Universal Joint Flange / Made by Onshape"). Assumptions where the course's pictures are ambiguous:
//! the Ø1.750 circle of the right view is read as a boss (not the lug holes' bolt circle, which
//! is Ø2.125 here), the 43.0° angle is between the chamfer and the lug's top edge, the 120.0°
//! angle is between the flare and the flange's top face (measured outside the part), the
//! pocket's size and depth, the lug holes' bolt circle, the fillet and the chamfer are our
//! choices. Every id is fixed, so the fixture is regenerated exactly (see
//! `cadrs_core/tests/ujoint.rs`).

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::applied::{ChamferFeature, ChamferType, EdgeOrFace, FilletFeature, HoleFeature, HolePoint};
use crate::command::{CommandError, History};
use crate::commands::{AddFeature, AddSketch, EditSketch, RenameFeature, RenamePart, SetPartDescription};
use crate::document::{BooleanOp, Document, EdgeRef, Offset};
use crate::hole::{Fit, HoleEnd, HoleSpec, HoleStandard, HoleStart, HoleStyle, HoleType, Length};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::samples::drawing_bracket::extrude;
use crate::samples::gear_cover::{DocHistory, Studio};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x0a1f_1a00_0000_0000_0000_0000_0000_0000 | n)
}

pub const SKETCH_1: FeatureId = id(1);
pub const EXTRUDE_1: FeatureId = id(2);
pub const SKETCH_2: FeatureId = id(3);
pub const EXTRUDE_2: FeatureId = id(4);
pub const SKETCH_3: FeatureId = id(5);
pub const EXTRUDE_3: FeatureId = id(6);
pub const SKETCH_4: FeatureId = id(7);
pub const EXTRUDE_4: FeatureId = id(8);
pub const EXTRUDE_5: FeatureId = id(9);
pub const SKETCH_5: FeatureId = id(10);
pub const EXTRUDE_6: FeatureId = id(11);
pub const SKETCH_6: FeatureId = id(12);
pub const EXTRUDE_7: FeatureId = id(13);
pub const SKETCH_7: FeatureId = id(14);
pub const HOLE_1: FeatureId = id(15);
pub const SKETCH_8: FeatureId = id(16);
pub const HOLE_2: FeatureId = id(17);
pub const HOLE_3: FeatureId = id(18);
pub const FILLET_1: FeatureId = id(19);
pub const CHAMFER_1: FeatureId = id(20);
pub const SKETCH_9: FeatureId = id(21);
pub const EXTRUDE_8: FeatureId = id(22);
/// The part.
pub const PART: PartId = PartId::new(EXTRUDE_1, 0);
/// The part's name (the title block shows it).
pub const PART_NAME: &str = "Universal Joint Flange (stand-in)";
/// Its description (the title block's second line).
pub const PART_DESCRIPTION: &str = "Made by cadrs";

/// One inch in mm.
pub const IN: f64 = 25.4;

// The model values the drawing's dimensions read (inches, degrees).
/// Flange diameter (Ø4.750).
pub const FLANGE_D: f64 = 4.75;
/// Flange thickness: the floor is at z = 0, the flange's bottom at −0.500.
pub const FLANGE_T: f64 = 0.5;
/// Overall height (6.000): from the flange's bottom to the lugs' tops.
pub const HEIGHT: f64 = 6.0;
/// Slot width between the lugs (2.600).
pub const SLOT: f64 = 2.6;
/// Lug width (2.500).
pub const LUG_W: f64 = 2.5;
/// The top corners' chamfer: its angle to the top edge (43.0°) and its run along the top.
pub const CHAMFER_ANGLE: f64 = 43.0;
pub const CHAMFER_RUN: f64 = 0.5;
/// The flare at the lugs' base: its angle to the flange's top face, outside the part (120.0°),
/// and how far it runs out from each side.
pub const FLARE_ANGLE: f64 = 120.0;
pub const FLARE_RUN: f64 = 0.75;
/// The lugs' flat outer faces (x = ±2.000).
pub const FLAT_X: f64 = 2.0;
/// Boss diameter (Ø1.750) and its face (x = ±2.125: 0.125 proud of the flat).
pub const BOSS_D: f64 = 1.75;
pub const BOSS_X: f64 = 2.125;
/// Cross hole diameter (Ø1.250) and height of the cross hole's axis.
pub const CROSS_D: f64 = 1.25;
pub const CROSS_Z: f64 = HEIGHT - FLANGE_T - 1.75;
/// The counterbored holes' centres: a 2.061 × 3.282 rectangle.
pub const HOLES_X: f64 = 2.061;
pub const HOLES_Y: f64 = 3.282;
/// Hole diameter (Ø.266, 1/4 clearance Normal) and the counterbore (Ø.438 × .250).
pub const HOLE_D: f64 = 0.266;
pub const CBORE_D: f64 = 0.438;
pub const CBORE_DEPTH: f64 = 0.25;
/// The lug holes' bolt circle.
pub const BOLT_D: f64 = 2.125;
/// Pocket: side and depth.
pub const POCKET: f64 = 2.0;
pub const POCKET_DEPTH: f64 = 0.25;
/// The flares' fillet radius and the rim chamfer.
pub const FILLET_R: f64 = 0.25;
pub const RIM_CHAMFER: f64 = 0.06;

/// The top of the lugs (z).
pub const TOP: f64 = HEIGHT - FLANGE_T;

/// The height above the floor where the flare meets the lug's side.
pub fn flare_rise() -> f64 {
    FLARE_RUN * (180.0 - FLARE_ANGLE).to_radians().tan()
}

/// How far below the top the chamfer meets the lug's side.
pub fn chamfer_drop() -> f64 {
    CHAMFER_RUN * CHAMFER_ANGLE.to_radians().tan()
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x * IN, y * IN)
}

fn inch(x: f64) -> String {
    format!("{} in", crate::hole::fmt(x))
}

fn polyline(points: Vec<Vec2>, label: &'static str) -> SketchOp {
    SketchOp::AddPolyline {
        points,
        closed: true,
        construction: false,
        label,
    }
}

fn circle(c: Vec2, d: f64) -> SketchOp {
    SketchOp::AddCircle {
        center: c,
        radius: d / 2.0 * IN,
        construction: false,
    }
}

fn sketch(s: &mut dyn Studio, el: ElementId, f: FeatureId, plane: PlaneRef, ops: Vec<SketchOp>) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: f, plane: Some(plane) })?;
    s.run(&EditSketch { element: el, feature: f, op: SketchOp::Batch(ops) })
}

fn rename(s: &mut dyn Studio, el: ElementId, f: FeatureId, name: &str) -> Result<(), CommandError> {
    s.run(&RenameFeature { element: el, feature: f, name: name.into() })
}

/// The points of sketch `f` (every point, in the order they were made).
fn points_of(s: &dyn Studio, el: ElementId, f: FeatureId) -> Result<Vec<HolePoint>, CommandError> {
    let g = &s
        .document()
        .element(el)
        .and_then(|e| e.feature(f))
        .and_then(|f| f.sketch())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?
        .geometry;
    Ok(g.points.keys().map(|point| HolePoint { sketch: f, point }).collect())
}

/// The edge of the built part nearest `p` (inches).
fn edge_at(s: &dyn Studio, el: ElementId, p: [f64; 3]) -> Result<EdgeRef, CommandError> {
    let features = s
        .document()
        .element(el)
        .map(|e| e.features().to_vec())
        .ok_or_else(|| CommandError::Invalid("studio not found".into()))?;
    let build = crate::rebuild::build(&features);
    let part = build
        .parts
        .iter()
        .find(|q| q.id == PART)
        .ok_or_else(|| CommandError::Invalid(format!("the flange did not build: {:?}", build.errors)))?;
    let p = [p[0] * IN, p[1] * IN, p[2] * IN];
    let e = part
        .solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        .filter(|e| e.distance(p) < 1e-3)
        .ok_or_else(|| CommandError::Invalid(format!("no edge at {p:?}")))?;
    Ok(EdgeRef { part: PART, edge: e.name, seed: p })
}

fn ansi_clearance(style: HoleStyle) -> HoleSpec {
    let mut spec = HoleSpec {
        standard: HoleStandard::Ansi,
        style,
        hole_type: HoleType::Clearance,
        size: "1/4".into(),
        fit: Fit::Normal,
        start: HoleStart::Part,
        end: HoleEnd::ThroughAll,
        ..HoleSpec::default()
    };
    spec.apply_table();
    spec
}

/// The counterbored holes' spec: Ø.266 THRU, ⌴Ø.438 ↧.250.
pub fn counterbore_spec() -> HoleSpec {
    let mut spec = ansi_clearance(HoleStyle::Counterbore);
    spec.cbore_diameter = Length::of(HoleStandard::Ansi, CBORE_D);
    spec.cbore_depth = Length::of(HoleStandard::Ansi, CBORE_DEPTH);
    spec
}

/// The lug holes' spec: Ø.266 THRU.
pub fn lug_hole_spec() -> HoleSpec {
    ansi_clearance(HoleStyle::Simple)
}

/// Adds the flange's features to the Part Studio `el` of `doc`.
pub fn build(doc: &mut Document, h: &mut History, el: ElementId) -> Result<(), CommandError> {
    build_in(&mut DocHistory(doc, h), el)
}

/// [`build`] through any [`Studio`].
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let r = FLANGE_D / 2.0;
    // Main Body: the disc from z = −0.5 up to the lugs' top.
    sketch(s, el, SKETCH_1, PlaneRef::Top, vec![circle(v(0.0, 0.0), FLANGE_D)])?;
    rename(s, el, SKETCH_1, "Main Body")?;
    extrude(s, el, SKETCH_1, EXTRUDE_1, &[v(0.0, 0.0)], |e| {
        e.depth = HEIGHT * IN;
        e.depth_expr = inch(HEIGHT);
        e.start_offset = Some(Offset { value: FLANGE_T * IN, expr: inch(FLANGE_T), flip: true });
    })?;
    // The slot, on Front (sketch x = X, y = Z), through the part along Y.
    let h = SLOT / 2.0;
    sketch(
        s,
        el,
        SKETCH_2,
        PlaneRef::Front,
        vec![polyline(vec![v(-h, 0.0), v(h, 0.0), v(h, TOP + 1.0), v(-h, TOP + 1.0)], "Add rectangle")],
    )?;
    rename(s, el, SKETCH_2, "Slot")?;
    extrude(s, el, SKETCH_2, EXTRUDE_2, &[v(0.0, 1.0)], |e| {
        e.op = BooleanOp::Remove;
        e.symmetric = true;
        e.depth = 6.0 * IN;
        e.depth_expr = inch(6.0);
    })?;
    // The lugs' side profile, on Right (sketch x = Y, y = Z): the region outside it goes.
    let w = LUG_W / 2.0;
    let (rise, drop) = (flare_rise(), chamfer_drop());
    let foot = w + FLARE_RUN;
    let (out, up) = (r + 1.0, TOP + 1.0);
    sketch(
        s,
        el,
        SKETCH_3,
        PlaneRef::Right,
        vec![polyline(
            vec![
                v(-out, 0.0),
                v(-foot, 0.0),
                v(-w, rise),
                v(-w, TOP - drop),
                v(-w + CHAMFER_RUN, TOP),
                v(w - CHAMFER_RUN, TOP),
                v(w, TOP - drop),
                v(w, rise),
                v(foot, 0.0),
                v(out, 0.0),
                v(out, up),
                v(-out, up),
            ],
            "Add polygon",
        )],
    )?;
    rename(s, el, SKETCH_3, "Lug profile")?;
    extrude(s, el, SKETCH_3, EXTRUDE_3, &[v(r + 0.5, 3.0)], |e| {
        e.op = BooleanOp::Remove;
        e.symmetric = true;
        e.depth = 6.0 * IN;
        e.depth_expr = inch(6.0);
    })?;
    // Fillet 1 on the concave edges between the flares and the lugs' sides (along x, halfway
    // out on each lug).
    let mid = (SLOT / 2.0 + FLAT_X) / 2.0;
    let mut edges = Vec::new();
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            edges.push(EdgeOrFace::Edge(edge_at(s, el, [sx * mid, sy * w, rise])?));
        }
    }
    s.run(&AddFeature::fillet(
        el,
        FILLET_1,
        FilletFeature {
            entities: edges,
            size: FILLET_R * IN,
            size_expr: inch(FILLET_R),
            ..FilletFeature::default()
        },
    ))?;
    // The flats: beyond x = ±2.0, from the floor up.
    let big = r + 1.0;
    sketch(
        s,
        el,
        SKETCH_9,
        PlaneRef::Top,
        vec![
            polyline(vec![v(FLAT_X, -big), v(big, -big), v(big, big), v(FLAT_X, big)], "Add rectangle"),
            polyline(vec![v(-big, -big), v(-FLAT_X, -big), v(-FLAT_X, big), v(-big, big)], "Add rectangle"),
        ],
    )?;
    rename(s, el, SKETCH_9, "Flats")?;
    extrude(s, el, SKETCH_9, EXTRUDE_8, &[v(FLAT_X + 0.5, 0.0), v(-FLAT_X - 0.5, 0.0)], |e| {
        e.op = BooleanOp::Remove;
        e.depth = HEIGHT * IN;
        e.depth_expr = inch(HEIGHT);
    })?;
    // The bosses: one extrude out of each lug's flat.
    sketch(s, el, SKETCH_4, PlaneRef::Right, vec![circle(v(0.0, CROSS_Z), BOSS_D)])?;
    rename(s, el, SKETCH_4, "Bosses")?;
    for (f, flip) in [(EXTRUDE_4, false), (EXTRUDE_5, true)] {
        extrude(s, el, SKETCH_4, f, &[v(0.0, CROSS_Z)], |e| {
            e.op = BooleanOp::Add;
            e.flip = flip;
            e.depth = (BOSS_X - FLAT_X + 0.1) * IN;
            e.depth_expr = inch(BOSS_X - FLAT_X + 0.1);
            e.start_offset = Some(Offset { value: (FLAT_X - 0.1) * IN, expr: inch(FLAT_X - 0.1), flip: false });
        })?;
    }
    // The cross hole through both lugs.
    sketch(s, el, SKETCH_5, PlaneRef::Right, vec![circle(v(0.0, CROSS_Z), CROSS_D)])?;
    rename(s, el, SKETCH_5, "Cross hole")?;
    extrude(s, el, SKETCH_5, EXTRUDE_6, &[v(0.0, CROSS_Z)], |e| {
        e.op = BooleanOp::Remove;
        e.symmetric = true;
        e.depth = 6.0 * IN;
        e.depth_expr = inch(6.0);
    })?;
    // The square pocket in the floor.
    let p = POCKET / 2.0;
    sketch(
        s,
        el,
        SKETCH_6,
        PlaneRef::Top,
        vec![polyline(vec![v(-p, -p), v(p, -p), v(p, p), v(-p, p)], "Add rectangle")],
    )?;
    rename(s, el, SKETCH_6, "Pocket")?;
    extrude(s, el, SKETCH_6, EXTRUDE_7, &[v(0.0, 0.0)], |e| {
        e.op = BooleanOp::Remove;
        e.flip = true;
        e.depth = POCKET_DEPTH * IN;
        e.depth_expr = inch(POCKET_DEPTH);
    })?;
    // The counterbored holes: four points on Top, drilled down from the floor.
    let (hx, hy) = (HOLES_X / 2.0, HOLES_Y / 2.0);
    sketch(
        s,
        el,
        SKETCH_7,
        PlaneRef::Top,
        [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]
            .into_iter()
            .map(|(x, y)| SketchOp::AddPoint { pos: v(x, y) })
            .collect(),
    )?;
    rename(s, el, SKETCH_7, "Counterbore holes")?;
    let points = points_of(s, el, SKETCH_7)?;
    s.run(&AddFeature::hole(
        el,
        HOLE_1,
        HoleFeature { points, merge_scope: vec![PART], spec: counterbore_spec(), ..HoleFeature::default() },
    ))?;
    // The lug holes: four points on the bolt circle, drilled both ways from the Right plane.
    let b = BOLT_D / 2.0 * std::f64::consts::FRAC_1_SQRT_2;
    sketch(
        s,
        el,
        SKETCH_8,
        PlaneRef::Right,
        [(-b, -b), (b, -b), (b, b), (-b, b)]
            .into_iter()
            .map(|(y, z)| SketchOp::AddPoint { pos: v(y, CROSS_Z + z) })
            .collect(),
    )?;
    rename(s, el, SKETCH_8, "Lug holes")?;
    for (f, flip) in [(HOLE_2, false), (HOLE_3, true)] {
        let points = points_of(s, el, SKETCH_8)?;
        s.run(&AddFeature::hole(
            el,
            f,
            HoleFeature { points, merge_scope: vec![PART], spec: lug_hole_spec(), flip, ..HoleFeature::default() },
        ))?;
    }
    // Chamfer 1 on the flange's bottom rim.
    let rim = edge_at(s, el, [r, 0.0, -FLANGE_T])?;
    s.run(&AddFeature::chamfer(
        el,
        CHAMFER_1,
        ChamferFeature {
            entities: vec![EdgeOrFace::Edge(rim)],
            kind: ChamferType::EqualDistance,
            distance: RIM_CHAMFER * IN,
            distance_expr: inch(RIM_CHAMFER),
            ..ChamferFeature::default()
        },
    ))?;
    s.run(&RenamePart { element: el, part: PART, name: PART_NAME.into() })?;
    s.run(&SetPartDescription { element: el, part: PART, description: Some(PART_DESCRIPTION.into()) })?;
    Ok(())
}

/// A new document holding the stand-in: "Universal Joint Drawing (stand-in)", its Part Studio
/// "Universal Joint" in inches, as `fixtures/ujoint_flange_standin.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Universal Joint Drawing (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x0a1f_1a00_0000_0000_0000_0000_0000_0100);
    doc.units.length = cadrs_sketch::units::LengthUnit::Inch;
    let mut el = crate::document::Element::part_studio("Universal Joint");
    el.id = ElementId::from_u128(0x0a1f_1a00_0000_0000_0000_0000_0000_0101);
    let id = el.id;
    doc.elements.push(el);
    let mut h = History::default();
    build(&mut doc, &mut h, id)?;
    Ok(doc)
}
