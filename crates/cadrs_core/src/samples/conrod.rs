//! The Conrod stand-in for the Inspection and Repair exercise (P3D.4, IR6). Onshape's public
//! "Exercise: Conrod" can't be copied and gives only a few dimensions, so the exercise runs on
//! this connecting rod, built from our own features, in **inch** and Steel (0.2836 lb/in³):
//!
//! - **Sketch 1** on Top: the web, a trapezoid 1.0 wide at y = 1 and 0.6 at y = 5 (3.2 in²);
//!   the big end, a ring Ø1.5/Ø0.9 at the origin with a neck (x ±0.5, y 0.5–1) up to the web;
//!   the small end, a ring Ø1.0/Ø0.6 at (0, 5.8) with a neck (x ±0.3, y 5–5.45) down to it.
//!   The necks share the web's short sides, so the web's top face stays the trapezoid.
//! - **Extrude 1** (the web, 0.3, New), **Extrude 2** (the big end, 0.5, Add), **Extrude 3**
//!   (the small end, 0.5, Add): one part, **Conrod**.
//! - **Sketch 2** on the small end's top face ("Face of Extrude 3", imprinting off): a 30°
//!   notch (apex 0.38 from the ring's centre, sides 0.3 long at ±15°, closed outside the
//!   ring), a spur off its left corner (the "Loose end"), and two construction circles over
//!   the bore and the rim, Ø0.6 and Ø1.0, concentric (the pattern's axis).
//! - **Extrude 4** removes the notch through the ring (0.5); **Circular pattern 1** repeats it
//!   three times about the small end's axis.
//! - **Sketch 3** on the web's top face ("Face of Extrude 1"): a slot R0.15 from y 1.55 to
//!   4.45; **Extrude 5** removes it 0.07 deep; **Fillet 1** (the course's Fillet 4) rounds the
//!   pocket's floor, R0.03, from one floor edge with tangent propagation.
//!
//! [`document`] ships it **broken** as the course's copy is: Sketch 2's right notch side
//! redrawn 0.02 in short of the apex (the "Loose ends (2)" pair) and an **Equal 1** between
//! the two construction circles, which the Ø dimensions can't allow. Sketch 2 can't be solved
//! and Extrude 4 and Circular pattern 1 fail. Its history ([`history`]) holds every healthy
//! state it went through, with each feature's last healthy regeneration.
//!
//! [`healthy_document`] is the model as the exercise leaves it (IR6.2–IR6.16 done): Sketch 2
//! whole, Sketch 3 the web face's outer loop offset 0.1 in inward, Extrude 5 on that region
//! and the fillet on the pocket's floor face. Every id is fixed.

use cadrs_sketch::{ConstraintOf, CurveRef, Dimension, DimensionKind, PlaneRef, SketchOp, Vec2};

use super::gear_cover::Studio;
use crate::applied::{EdgeOrFace, FilletFeature};
use crate::command::{Command, CommandError, History};
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, RenamePart, SetExtrude, SetPartMaterial, SetSketchImprinting};
use crate::document::{AxisRef, BooleanOp, Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind};
use crate::history_log::{HistoryLog, Origin};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::pattern::{PatternFeature, PatternKind, PatternType};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0xc047_0d00_0000_0000_0000_0000_0000_0000 | n)
}

pub const DOCUMENT: DocumentId = DocumentId::from_u128(0xc047_0d00_0000_0000_0000_0000_0000_0100);
pub const STUDIO: ElementId = ElementId::from_u128(0xc047_0d00_0000_0000_0000_0000_0000_0101);
pub const SKETCH_1: FeatureId = id(1);
pub const EXTRUDE_1: FeatureId = id(2);
pub const EXTRUDE_2: FeatureId = id(3);
pub const EXTRUDE_3: FeatureId = id(4);
pub const SKETCH_2: FeatureId = id(5);
pub const EXTRUDE_4: FeatureId = id(6);
pub const PATTERN_1: FeatureId = id(7);
pub const SKETCH_3: FeatureId = id(8);
pub const EXTRUDE_5: FeatureId = id(9);
pub const FILLET: FeatureId = id(10);
/// The rod.
pub const PART: PartId = PartId::new(EXTRUDE_1, 0);

/// Inches in mm.
pub const IN: f64 = 25.4;
/// The web: y range, full widths there, thickness.
pub const WEB_Y: (f64, f64) = (1.0, 5.0);
pub const WEB_W: (f64, f64) = (1.0, 0.6);
pub const WEB_T: f64 = 0.3;
/// The ends: centre y, outer and inner radius, the neck's half width and far y; height.
pub const BIG: (f64, f64, f64, f64, f64) = (0.0, 0.75, 0.45, 0.5, 0.5);
pub const SMALL: (f64, f64, f64, f64, f64) = (5.8, 0.5, 0.3, 0.3, 5.45);
pub const END_H: f64 = 0.5;
/// The notch: its apex's distance from the small end's centre, its sides' length and half
/// angle.
pub const NOTCH_APEX: f64 = 0.38;
pub const NOTCH_SIDE: f64 = 0.3;
pub const NOTCH_HALF_ANGLE: f64 = 15.0;
/// The gap left at the apex: 0.02 in.
pub const GAP: f64 = 0.02;
/// The slot: R0.15 from y 1.55 to 4.45; the pocket's depth; the fillet's radius.
pub const SLOT_R: f64 = 0.15;
pub const SLOT_Y: (f64, f64) = (1.55, 4.45);
pub const POCKET_DEPTH: f64 = 0.07;
pub const FILLET_R: f64 = 0.03;
/// The web face's offset (IR6.10).
pub const OFFSET: f64 = 0.1;
/// Steel's density, lb/in³ (P3.6's library: 7850 kg/m³).
pub const STEEL_LB_IN3: f64 = 0.2836;

/// A point in model inches, in mm.
fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x * IN, y * IN)
}

/// The notch's corners (inch, model): apex, left and right ends.
pub fn notch() -> [Vec2; 3] {
    let c = Vec2::new(0.0, SMALL.0);
    let apex = c + Vec2::new(0.0, NOTCH_APEX);
    let a = NOTCH_HALF_ANGLE.to_radians();
    let left = apex + Vec2::new(-a.sin(), a.cos()) * NOTCH_SIDE;
    let right = apex + Vec2::new(a.sin(), a.cos()) * NOTCH_SIDE;
    [apex, left, right]
}

/// A Studio that also logs a history: [`Recorder::commit`] ends a change (as a dialog's ✓
/// does) with an entry and the healthy features noted.
pub struct Recorder<'a> {
    pub doc: &'a mut Document,
    pub undo: History,
    pub log: HistoryLog,
    pub time: i64,
    pub user: String,
}

impl Studio for Recorder<'_> {
    fn run(&mut self, c: &dyn Command) -> Result<(), CommandError> {
        self.undo.execute(self.doc, c)
    }
    fn document(&self) -> &Document {
        self.doc
    }
}

impl Recorder<'_> {
    /// Logs the changes since the last commit as one entry, a minute later.
    pub fn commit(&mut self) {
        self.time += 60;
        let label = self.undo.undo_label().unwrap_or_default().to_string();
        if self.log.record(self.doc, Origin::Command(label), self.time, &self.user).is_some() {
            note_healthy(&mut self.log, self.doc);
        }
    }
}

/// Notes the features of every Part Studio that regenerate without error in `doc` (the log's
/// current state) as healthy there: part features the rebuild builds, sketches that solve with
/// no broken link.
pub fn note_healthy(log: &mut HistoryLog, doc: &Document) {
    for el in &doc.elements {
        if el.assembly_model().is_some() {
            continue;
        }
        let features = el.active_features();
        if features.is_empty() {
            continue;
        }
        let build = crate::rebuild::build(&features);
        let ok: Vec<FeatureId> = features
            .iter()
            .filter(|f| build.error(f.id).is_none())
            .filter(|f| {
                f.sketch().is_none_or(|s| {
                    s.plane.is_some() && s.geometry.broken.is_empty() && cadrs_sketch::solve::analyze(&s.geometry).conflicting.is_empty()
                })
            })
            .map(|f| f.id)
            .collect();
        log.note_healthy(el.id, ok);
    }
}

fn sketch_of(s: &dyn Studio, sketch: FeatureId) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.document()
        .element(STUDIO)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

fn edit(s: &mut dyn Studio, sketch: FeatureId, op: SketchOp) -> Result<(), CommandError> {
    s.run(&EditSketch { element: STUDIO, feature: sketch, op })
}

fn features(s: &dyn Studio) -> Vec<crate::document::Feature> {
    s.document().element(STUDIO).map(|e| e.features().to_vec()).unwrap_or_default()
}

fn poly(points: Vec<Vec2>, closed: bool) -> SketchOp {
    SketchOp::AddPolyline { points, closed, construction: false, label: if closed { "Add polygon" } else { "Add line" } }
}

/// An extrude of the regions of `sketch` under `seeds` (sketch coordinates, mm).
fn extrude(s: &mut dyn Studio, sketch: FeatureId, feature: FeatureId, seeds: &[Vec2], depth: f64, op: BooleanOp, flip: bool) -> Result<(), CommandError> {
    let g = sketch_of(s, sketch)?;
    let regions = super::region_refs(sketch, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid(format!("a region of {sketch:?} is missing")));
    }
    let e = ExtrudeFeature {
        op,
        flip,
        depth_expr: format!("{} in", depth / IN),
        ..super::extrude_of(regions, depth)
    };
    s.run(&AddExtrude { element: STUDIO, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: STUDIO, feature, extrude: e, label: "Extrude".into() })
}

/// A sketch plane on the top cap of `extrude`'s `region`th region, and its frame.
fn top_of(s: &dyn Studio, extrude: FeatureId, region: usize, seed: Vec2, z: f64) -> Result<PlaneRef, CommandError> {
    let features = features(s);
    let face = crate::parts::cap_name(&features, extrude, region, true).ok_or_else(|| CommandError::Invalid("no top face".into()))?;
    let plane = crate::parts::face_plane(&features, extrude, face).ok_or_else(|| CommandError::Invalid("no top face plane".into()))?;
    match plane {
        PlaneRef::Face(mut fp) => {
            fp.seed = Some([seed.x, seed.y, z]);
            Ok(PlaneRef::Face(fp))
        }
        other => Ok(other),
    }
}

/// Sketch coordinates (mm) of a model point on a sketch plane.
fn local(plane: &PlaneRef, q: Vec2, z: f64) -> Vec2 {
    plane.frame().to_sketch([q.x, q.y, z])
}

/// Sketch 1 and Extrudes 1–3.
fn body(s: &mut Recorder) -> Result<(), CommandError> {
    let (y0, y1) = WEB_Y;
    let (w0, w1) = (WEB_W.0 / 2.0, WEB_W.1 / 2.0);
    s.run(&AddSketch { element: STUDIO, feature: SKETCH_1, plane: Some(PlaneRef::Top) })?;
    // The web.
    edit(s, SKETCH_1, poly(vec![p(-w0, y0), p(w0, y0), p(w1, y1), p(-w1, y1)], true))?;
    // The ends: the rings and the necks, open U's on the web's short sides.
    for (cy, ro, ri, nw, ny, y) in [(BIG.0, BIG.1, BIG.2, BIG.3, BIG.4, y0), (SMALL.0, SMALL.1, SMALL.2, SMALL.3, SMALL.4, y1)] {
        for r in [ro, ri] {
            edit(s, SKETCH_1, SketchOp::AddCircle { center: p(0.0, cy), radius: r * IN, construction: false })?;
        }
        edit(s, SKETCH_1, poly(vec![p(-nw, y), p(-nw, ny), p(nw, ny), p(nw, y)], false))?;
    }
    s.commit();
    extrude(s, SKETCH_1, EXTRUDE_1, &[p(0.0, 3.0)], WEB_T * IN, BooleanOp::New, false)?;
    s.commit();
    // The big end: the ring less the neck, the neck in the ring, the neck outside it.
    extrude(s, SKETCH_1, EXTRUDE_2, &[p(0.0, -0.6), p(0.0, 0.6), p(0.0, 0.9)], END_H * IN, BooleanOp::Add, false)?;
    s.commit();
    extrude(s, SKETCH_1, EXTRUDE_3, &[p(0.0, 6.2), p(0.0, 5.4), p(0.0, 5.1)], END_H * IN, BooleanOp::Add, false)?;
    s.commit();
    s.run(&RenamePart { element: STUDIO, part: PART, name: "Conrod".into() })?;
    s.commit();
    s.run(&SetPartMaterial { element: STUDIO, parts: vec![PART], material: crate::material::library("Steel") })?;
    s.commit();
    Ok(())
}

/// Sketch 2 (healthy), Extrude 4 and Circular pattern 1. Returns the Ø0.6 construction circle.
fn notch_features(s: &mut Recorder) -> Result<(), CommandError> {
    let plane = top_of(s, EXTRUDE_3, 0, p(0.0, 6.2), END_H * IN)?;
    s.run(&AddSketch { element: STUDIO, feature: SKETCH_2, plane: Some(plane) })?;
    s.run(&SetSketchImprinting { element: STUDIO, feature: SKETCH_2, disable_imprinting: true })?;
    let z = END_H * IN;
    let at = |q: Vec2| local(&plane, q * IN, z);
    let [apex, left, right] = notch();
    // The spur first (its loose end is the Profile inspector's first row), then the notch.
    edit(s, SKETCH_2, poly(vec![at(left), at(left + Vec2::new(-0.12, 0.1))], false))?;
    edit(s, SKETCH_2, poly(vec![at(apex), at(left), at(right)], true))?;
    // The apex is fixed, so closing the gap later (IR6.8) brings the loose end back to it.
    let g = sketch_of(s, SKETCH_2)?;
    let tip = g.point_at(at(apex), 1e-6).ok_or_else(|| CommandError::Invalid("no notch apex".into()))?;
    edit(
        s,
        SKETCH_2,
        SketchOp::AddConstraint { constraints: vec![ConstraintOf::FixPoint(cadrs_sketch::PointRef::Point(tip))], label: "Add fix" },
    )?;
    // The construction circles over the bore and the rim, dimensioned and concentric.
    let center = at(Vec2::new(0.0, SMALL.0));
    for r in [SMALL.2, SMALL.1] {
        edit(s, SKETCH_2, SketchOp::AddCircle { center, radius: r * IN, construction: true })?;
    }
    let g = sketch_of(s, SKETCH_2)?;
    let (small, large) = construction_circles(&g).ok_or_else(|| CommandError::Invalid("no construction circles".into()))?;
    // Their labels on either side, so they don't overlap.
    for (c, r, at) in [(small, SMALL.2, std::f64::consts::FRAC_PI_4), (large, SMALL.1, -std::f64::consts::FRAC_PI_4)] {
        edit(
            s,
            SKETCH_2,
            SketchOp::SetDimension {
                dimension: Dimension::new(DimensionKind::Diameter { curve: c }, 2.0 * r * IN, at),
                moves: vec![],
                radii: vec![],
            },
        )?;
    }
    edit(
        s,
        SKETCH_2,
        SketchOp::AddConstraint { constraints: vec![ConstraintOf::Concentric(CurveRef::Curve(small), CurveRef::Curve(large))], label: "Add concentric" },
    )?;
    s.commit();
    // The notch through the ring, from the top face down.
    let seed = at((apex + left + right) / 3.0);
    extrude(s, SKETCH_2, EXTRUDE_4, &[seed], END_H * IN, BooleanOp::Remove, true)?;
    s.commit();
    let mut pat = PatternFeature::new(PatternKind::Circular);
    pat.pattern_type = PatternType::Feature;
    pat.features = vec![EXTRUDE_4];
    pat.axis = Some(AxisRef::SketchCurve { sketch: SKETCH_2, curve: small });
    pat.first.count = 3;
    s.run(&AddFeature { element: STUDIO, feature: PATTERN_1, base_name: "Circular pattern".into(), kind: FeatureKind::Pattern(pat) })?;
    s.commit();
    Ok(())
}

/// The Ø0.6 and Ø1.0 construction circles of Sketch 2.
pub fn construction_circles(g: &cadrs_sketch::Sketch) -> Option<(cadrs_sketch::CurveId, cadrs_sketch::CurveId)> {
    let find = |r: f64| {
        g.curves.iter().find_map(|(k, c)| match c.kind {
            cadrs_sketch::CurveKind::Circle { radius, .. } if c.construction && (radius - r * IN).abs() < 1e-6 => Some(k),
            _ => None,
        })
    };
    Some((find(SMALL.2)?, find(SMALL.1)?))
}

/// The pocket's floor edge the fillet was picked on (x = +0.15, halfway along), on the parts
/// as they are.
fn floor_edge(s: &dyn Studio) -> Result<EdgeRef, CommandError> {
    let build = crate::rebuild::build(&features(s));
    let part = build.part(PART).ok_or_else(|| CommandError::Invalid("the rod didn't build".into()))?;
    let q = [SLOT_R * IN, 3.0 * IN, (WEB_T - POCKET_DEPTH) * IN];
    let e = part
        .solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(q).total_cmp(&b.distance(q)))
        .filter(|e| e.distance(q) < 1e-3)
        .ok_or_else(|| CommandError::Invalid("no floor edge in the pocket".into()))?;
    Ok(EdgeRef { part: part.id, edge: e.name, seed: q })
}

/// The pocket's floor face (the face at the pocket's depth, round (0, 3)), on the parts as
/// they are.
pub fn floor_face(features: &[crate::document::Feature]) -> Option<FaceRef> {
    let build = crate::rebuild::build(features);
    let part = build.part(PART)?;
    let z = (WEB_T - POCKET_DEPTH) * IN;
    let q = [0.0, 3.0 * IN, z];
    let i = part.solid.faces.iter().position(|f| {
        f.plane.is_some_and(|pl| pl.distance(q).abs() < 1e-4 && pl.normal()[2].abs() > 0.9)
    })?;
    Some(FaceRef { part: part.id, face: part.solid.faces[i].name, seed: q })
}

fn fillet_of(entities: Vec<EdgeOrFace>) -> FilletFeature {
    FilletFeature {
        entities,
        size: FILLET_R * IN,
        size_expr: format!("{FILLET_R} in"),
        ..FilletFeature::default()
    }
}

/// Sketch 3 (the slot), Extrude 5 and the fillet.
fn pocket_features(s: &mut Recorder) -> Result<(), CommandError> {
    let z = WEB_T * IN;
    let plane = top_of(s, EXTRUDE_1, 0, p(0.0, 3.0), z)?;
    s.run(&AddSketch { element: STUDIO, feature: SKETCH_3, plane: Some(plane) })?;
    let at = |q: Vec2| local(&plane, q * IN, z);
    edit(
        s,
        SKETCH_3,
        SketchOp::AddPolyline {
            points: vec![at(Vec2::new(0.0, SLOT_Y.0)), at(Vec2::new(0.0, SLOT_Y.1))],
            closed: false,
            construction: true,
            label: "Add line",
        },
    )?;
    let g = sketch_of(s, SKETCH_3)?;
    let source = g.curves.iter().find(|(_, c)| c.construction).map(|(k, _)| k).ok_or_else(|| CommandError::Invalid("no slot line".into()))?;
    edit(s, SKETCH_3, SketchOp::Slot { source, width: 2.0 * SLOT_R * IN, equal_to: None, construction: false })?;
    s.commit();
    extrude(s, SKETCH_3, EXTRUDE_5, &[at(Vec2::new(0.0, 3.0))], POCKET_DEPTH * IN, BooleanOp::Remove, true)?;
    s.commit();
    let edge = floor_edge(s)?;
    s.run(&AddFeature::fillet(STUDIO, FILLET, fillet_of(vec![EdgeOrFace::Edge(edge)])))?;
    s.commit();
    Ok(())
}

/// Breaks Sketch 2 as the course's copy is: the right side redrawn 0.02 in short of the apex,
/// and Equal 1 between the construction circles.
fn break_sketch_2(s: &mut Recorder) -> Result<(), CommandError> {
    let g = sketch_of(s, SKETCH_2)?;
    let plane = features(s).iter().find(|f| f.id == SKETCH_2).and_then(|f| f.sketch()).and_then(|k| k.plane).ok_or_else(|| CommandError::Invalid("no Sketch 2 plane".into()))?;
    let z = END_H * IN;
    let at = |q: Vec2| local(&plane, q * IN, z);
    let [apex, _, right] = notch();
    let (a, r) = (at(apex), at(right));
    let side = g
        .curves
        .keys()
        .find(|k| {
            g.curve_ends(*k).is_some_and(|(x, y)| {
                let (x, y) = (g.pos(x), g.pos(y));
                (x.distance(a) < 1e-6 && y.distance(r) < 1e-6) || (x.distance(r) < 1e-6 && y.distance(a) < 1e-6)
            })
        })
        .ok_or_else(|| CommandError::Invalid("no right side on the notch".into()))?;
    edit(s, SKETCH_2, SketchOp::Delete { curves: vec![side], points: vec![], dimensions: vec![], constraints: vec![] })?;
    let short = a + (r - a).normalize() * (GAP * IN);
    edit(s, SKETCH_2, poly(vec![r, short], false))?;
    let g = sketch_of(s, SKETCH_2)?;
    let (small, large) = construction_circles(&g).ok_or_else(|| CommandError::Invalid("no construction circles".into()))?;
    edit(
        s,
        SKETCH_2,
        SketchOp::AddConstraint { constraints: vec![ConstraintOf::Equal(CurveRef::Curve(small), CurveRef::Curve(large))], label: "Add equal" },
    )?;
    s.commit();
    Ok(())
}

/// The stand-in's empty document (inch, pound).
fn empty() -> Document {
    let mut doc = Document::empty("Exercise: Conrod (stand-in)");
    doc.id = DOCUMENT;
    doc.units = cadrs_sketch::units::Units {
        length: cadrs_sketch::units::LengthUnit::Inch,
        mass: cadrs_sketch::units::MassUnit::Pound,
        ..Default::default()
    };
    let mut el = crate::document::Element::part_studio("Conrod");
    el.id = STUDIO;
    doc.elements.push(el);
    doc
}

/// When the stand-in's history starts: 2026-09-21 00:00 UTC (before the scenarios' clock).
pub const CREATED: i64 = 1_789_948_800;

/// The stand-in as shipped (broken) and its history: every healthy state it went through,
/// then the break ("Conrod :: Edit : Sketch 2").
pub fn document_and_history() -> Result<(Document, HistoryLog), CommandError> {
    let mut doc = empty();
    let log = HistoryLog::start(&doc, CREATED, "cadrs");
    let mut r = Recorder { doc: &mut doc, undo: History::new(10_000), log, time: CREATED, user: "cadrs".into() };
    body(&mut r)?;
    notch_features(&mut r)?;
    pocket_features(&mut r)?;
    break_sketch_2(&mut r)?;
    let log = r.log;
    Ok((doc, log))
}

/// The stand-in as shipped (broken).
pub fn document() -> Result<Document, CommandError> {
    document_and_history().map(|(d, _)| d)
}

/// Its history (see [`document_and_history`]).
pub fn history() -> Result<HistoryLog, CommandError> {
    document_and_history().map(|(_, h)| h)
}

/// The model as the exercise leaves it, built directly (see the module docs).
pub fn healthy_document() -> Result<Document, CommandError> {
    let mut doc = empty();
    let log = HistoryLog::start(&doc, CREATED, "cadrs");
    let mut r = Recorder { doc: &mut doc, undo: History::new(10_000), log, time: CREATED, user: "cadrs".into() };
    body(&mut r)?;
    notch_features(&mut r)?;
    // Sketch 3: the web face's outer loop offset 0.1 in inward; Extrude 5 on the region inside
    // it; the fillet on the pocket's floor face.
    let z = WEB_T * IN;
    let plane = top_of(&r, EXTRUDE_1, 0, p(0.0, 3.0), z)?;
    r.run(&AddSketch { element: STUDIO, feature: SKETCH_3, plane: Some(plane) })?;
    let op = web_offset(&r, OFFSET * IN)?;
    edit(&mut r, SKETCH_3, op)?;
    extrude(&mut r, SKETCH_3, EXTRUDE_5, &[local(&plane, p(0.0, 3.0), z)], POCKET_DEPTH * IN, BooleanOp::Remove, true)?;
    let floor = floor_face(&features(&r)).ok_or_else(|| CommandError::Invalid("no pocket floor".into()))?;
    r.run(&AddFeature::fillet(STUDIO, FILLET, fillet_of(vec![EdgeOrFace::Face(floor)])))?;
    r.commit();
    Ok(doc)
}

/// IR6.10: the Offset of Sketch 3's face (the web's top face) by `distance` mm inward, as the
/// sketch op the Offset tool makes (on the document as it is: Sketch 3 must exist).
pub fn web_offset(s: &dyn Studio, distance: f64) -> Result<SketchOp, CommandError> {
    let features = features(s);
    let i = features.iter().position(|f| f.id == SKETCH_3).ok_or_else(|| CommandError::Invalid("no Sketch 3".into()))?;
    let plane = features[i].sketch().and_then(|k| k.plane).ok_or_else(|| CommandError::Invalid("Sketch 3 has no plane".into()))?;
    let build = crate::rebuild::build(&features[..i]);
    let items = crate::repair::face_loop(&features[..i], &build.parts, &plane);
    if items.len() != 4 {
        return Err(CommandError::Invalid(format!("the web face's loop has {} edges", items.len())));
    }
    // Inward: the side whose offset is smaller.
    let area = |left: bool| {
        let mut g = cadrs_sketch::Sketch::new();
        cadrs_sketch::face_offset::offset_loop(&mut g, &items, distance, left, (0.0, 0.0))
            .ok()
            .and_then(|_| cadrs_sketch::region::regions(&g).first().map(|r| r.area()))
            .unwrap_or(f64::MAX)
    };
    let left = area(true) < area(false);
    Ok(SketchOp::OffsetLoop { items, distance, left, label: (0.0, 0.0) })
}

/// The stand-in as a document file (`fixtures/conrod_standin.cadrs`), dated as its history.
pub fn file(document: Document) -> crate::store::DocumentFile {
    let mut meta = crate::library::DocumentMeta::new("cadrs", CREATED);
    meta.modified = CREATED + 13 * 60;
    crate::store::DocumentFile { version: crate::store::SCHEMA_VERSION, meta, document }
}
