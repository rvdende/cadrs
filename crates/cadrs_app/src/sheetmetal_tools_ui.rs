//! The dialogs of the sheet metal features after a Sheet metal model (P3I.5,
//! `cadrs_core::sheetmetal_tools`), laid out as Onshape's (`reference/onshape/sheetmetal/help/`
//! `feature-tools/…`):
//!
//! - **Bend** (`bend-dialog.png`): *Bend line* with the Hold opposite side arrow, *Sheet metal
//!   face to bend* (filled in from the line), *Bend alignment*, the angle control (Bend angle,
//!   Align to geometry, Angle from direction) with the opposite angle arrow, *Parallel to* or
//!   *Direction*, *Bend angle*, *Use model bend radius*, *Use model K Factor*.
//! - **Jog** (`sm-jog-01.png`): the Bend's rows, then *Bounding type* (Blind, Up to entity,
//!   Thickness) with *Jog offset*, *Up to entity* and *Offset distance*, or *Thickness factor*;
//!   *Jog offset anchor*; *Preserve material*.
//! - **Tab** (`sheetmetaltab-dialog.png`): *Tab profile*, *Flange to merge*, *Subtraction offset*,
//!   *Subtraction scope*.
//! - **Corner** (`sheetmetalcorner-dialog.png`) and **Bend relief**
//!   (`sheetmetalbendrelief-dialog.png`): the corner or bend end, the relief type and its scale,
//!   size or depth, *Extend bend relief*.
//! - **Corner break** (`sm-cornerbreak-01.png`): **Fillet | Chamfer**; *Entities to fillet or
//!   chamfer*; Fillet: *Measurement* (Radius, Width), *Control* (Distance), the size; Chamfer:
//!   *Measurement*, *Chamfer type*, the distances or angle with the opposite direction arrow.
//! - **Finish sheet metal model** (`smm-finish-01.png`): *Sheet metal parts* and the warning that
//!   later features don't show in the flat (exercise E4).
//!
//! They run in the applied features' session (`crate::applied`): one field at a time takes the
//! view's picks, every change is a command, ✓ / Enter accepts. This module owns their rows: the
//! [`SmtRole`] component marks them, and its own observers and sync system handle them.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::applied::{ChamferMeasurement, ChamferType, EdgeOrFace, FilletMeasurement};
use cadrs_core::document::{FaceRef, RegionRef, VertexRef};
use cadrs_core::sheetmetal::CurveRef;
use cadrs_core::sheetmetal_tools::*;
use cadrs_core::{Feature, FeatureId, FeatureKind};
use cadrs_sheetmetal::model_edit::{BendAlignment, JogAnchor};
use cadrs_sheetmetal::params::{BendReliefKind, CornerReliefKind};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, NumberField, NumberFieldCommit, NumberFieldState, OptionRow, Select, SelectChange, SelectState, SelectionList,
    SelectionListActivate, SelectionListRemove, SelectionListState, TabStrip, TabStripSelect,
};

use crate::applied::{AppliedField, AppliedKind, AppliedSession};
use crate::parts::{PartCache, PickFilter};
use crate::viewport::Pick;
use crate::{ActiveDocument, AppState};

/// Which tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmTool {
    Finish,
    Tab,
    Bend,
    Jog,
    Corner,
    BendRelief,
    CornerBreak,
}

impl SmTool {
    pub fn of(x: &SheetMetalTool) -> SmTool {
        match x {
            SheetMetalTool::Finish(_) => SmTool::Finish,
            SheetMetalTool::Tab(_) => SmTool::Tab,
            SheetMetalTool::Bend(_) => SmTool::Bend,
            SheetMetalTool::Jog(_) => SmTool::Jog,
            SheetMetalTool::Corner(_) => SmTool::Corner,
            SheetMetalTool::BendRelief(_) => SmTool::BendRelief,
            SheetMetalTool::CornerBreak(_) => SmTool::CornerBreak,
        }
    }

    /// Its toolbar name (also its icon).
    pub fn name(self) -> &'static str {
        match self {
            SmTool::Finish => "sheet-metal-finish",
            SmTool::Tab => "sheet-metal-tab",
            SmTool::Bend => "sheet-metal-bend",
            SmTool::Jog => "sheet-metal-jog",
            SmTool::Corner => "sheet-metal-corner",
            SmTool::BendRelief => "sheet-metal-bend-relief",
            SmTool::CornerBreak => "sheet-metal-corner-break",
        }
    }

    pub const ALL: [SmTool; 7] = [SmTool::Finish, SmTool::Tab, SmTool::Bend, SmTool::Jog, SmTool::Corner, SmTool::BendRelief, SmTool::CornerBreak];

    /// The tool a toolbar name starts.
    pub fn named(name: &str) -> Option<SmTool> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// Whether the sheet metal tool `name` (of the Sheet metal model button's ▾) is built here.
pub fn built(name: &str) -> bool {
    SmTool::named(name).is_some()
}

// The fields that take picks (`AppliedField::SmTool`).
pub const F_LINE: u8 = 0;
pub const F_FACE: u8 = 1;
pub const F_REF: u8 = 2;
pub const F_UP_TO: u8 = 3;
pub const F_PROFILE: u8 = 4;
pub const F_FLANGES: u8 = 5;
pub const F_SCOPE: u8 = 6;
pub const F_CORNER: u8 = 7;
pub const F_RELIEF: u8 = 8;
pub const F_ENTITIES: u8 = 9;
pub const F_PARTS: u8 = 10;

// Selects.
const S_ALIGN: u8 = 0;
const S_CONTROL: u8 = 1;
const S_BOUNDING: u8 = 2;
const S_ANCHOR: u8 = 3;
const S_CORNER_TYPE: u8 = 4;
const S_RELIEF_TYPE: u8 = 5;
const S_FILLET_MEAS: u8 = 6;
const S_FILLET_CONTROL: u8 = 7;
const S_CHAMFER_MEAS: u8 = 8;
const S_CHAMFER_TYPE: u8 = 9;

// Numbers.
const N_ANGLE: u8 = 0;
const N_RADIUS: u8 = 1;
const N_K: u8 = 2;
const N_OFFSET: u8 = 3;
const N_UP_TO_OFFSET: u8 = 4;
const N_FACTOR: u8 = 5;
const N_TAB_OFFSET: u8 = 6;
const N_CORNER_SCALE: u8 = 7;
const N_CORNER_SIZE: u8 = 8;
const N_DEPTH_SCALE: u8 = 9;
const N_WIDTH_SCALE: u8 = 10;
const N_DEPTH: u8 = 11;
const N_SIZE: u8 = 12;
const N_DIST: u8 = 13;
const N_DIST2: u8 = 14;
const N_CANGLE: u8 = 15;

// Arrows.
const FL_HOLD: u8 = 0;
const FL_OPPOSITE: u8 = 1;
const FL_CHAMFER: u8 = 2;

/// What a row of these dialogs is for.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtRole {
    List(u8),
    Select(u8),
    Number(u8),
    Flip(u8),
    /// Corner break's Fillet | Chamfer.
    Tab,
}

pub struct SheetMetalToolsPlugin;

impl Plugin for SheetMetalToolsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_list_remove)
            .add_observer(on_list_activate)
            .add_observer(on_select)
            .add_observer(on_checkbox)
            .add_observer(on_number)
            .add_observer(on_flip)
            .add_observer(on_tab)
            .add_systems(
                Update,
                sync_tool_dialog.after(crate::applied_dialog::sync_applied_dialog).before(crate::parts::PartsSet).run_if(in_state(AppState::Document)),
            );
    }
}

// ---------------------------------------------------------------------------------------------
// Starting, picks

fn tool(kind: &FeatureKind) -> Option<&SheetMetalTool> {
    match kind {
        FeatureKind::SheetMetalTool(x) => Some(x),
        _ => None,
    }
}

fn tool_mut(kind: &mut FeatureKind) -> Option<&mut SheetMetalTool> {
    match kind {
        FeatureKind::SheetMetalTool(x) => Some(x),
        _ => None,
    }
}

fn bend_mut(x: &mut SheetMetalTool) -> Option<&mut BendFeature> {
    match x {
        SheetMetalTool::Bend(b) => Some(b),
        SheetMetalTool::Jog(j) => Some(&mut j.bend),
        _ => None,
    }
}

fn bend_of(x: &SheetMetalTool) -> Option<&BendFeature> {
    match x {
        SheetMetalTool::Bend(b) => Some(b),
        SheetMetalTool::Jog(j) => Some(&j.bend),
        _ => None,
    }
}

/// The field a tool's selections start in.
pub fn first_field(t: SmTool) -> AppliedField {
    AppliedField::SmTool(match t {
        SmTool::Finish => F_PARTS,
        SmTool::Tab => F_PROFILE,
        SmTool::Bend | SmTool::Jog => F_LINE,
        SmTool::Corner => F_CORNER,
        SmTool::BendRelief => F_RELIEF,
        SmTool::CornerBreak => F_ENTITIES,
    })
}

/// A new feature, with the selection in its first field.
pub fn initial(world: &mut World, kind: AppliedKind, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let AppliedKind::SheetMetalTool(t) = kind else { return None };
    let mut x = SheetMetalTool::for_tool(t.name())?;
    let units = world.get_resource::<crate::WorkspaceUnits>().map(|u| u.0).unwrap_or_default();
    // Lengths in the document's unit.
    let len = |v: f64| units.with_unit(v, Quantity::Length);
    match &mut x {
        SheetMetalTool::Bend(b) => b.radius_expr = len(b.radius),
        SheetMetalTool::Jog(j) => {
            j.bend.radius_expr = len(j.bend.radius);
            j.offset_expr = len(j.offset);
            j.up_to_offset_expr = len(j.up_to_offset);
        }
        SheetMetalTool::Tab(tb) => tb.offset_expr = len(tb.offset),
        SheetMetalTool::Corner(c) => c.size_expr = len(c.relief.size),
        SheetMetalTool::BendRelief(b) => b.depth_expr = len(b.relief.depth),
        SheetMetalTool::CornerBreak(c) => {
            c.size_expr = len(c.size);
            c.distance_expr = len(c.distance);
            c.distance2_expr = len(c.distance2);
        }
        SheetMetalTool::Finish(_) => {}
    }
    let label = x.label();
    let field = first_field(t);
    let mut kind = FeatureKind::SheetMetalTool(x);
    let AppliedField::SmTool(f) = field else { return None };
    for p in picked {
        pick_into(world, &mut kind, f, *p);
    }
    Some((label, kind, field))
}

/// What a click can pick for a field.
pub fn pick_filter(field: AppliedField, none: PickFilter) -> PickFilter {
    let AppliedField::SmTool(f) = field else { return none };
    let planes = [true; 3];
    match f {
        F_LINE => PickFilter { edges: true, sketch_curves: true, skip_op: None, ..none },
        F_FACE | F_FLANGES => PickFilter { faces: true, planar_only: true, skip_op: None, ..none },
        F_REF => PickFilter { planes, plane_features: true, faces: true, planar_only: true, edges: true, sketch_curves: true, skip_op: None, ..none },
        F_UP_TO => PickFilter { faces: true, planar_only: true, skip_op: None, ..none },
        F_PROFILE => PickFilter { regions: true, ..none },
        F_SCOPE | F_PARTS => PickFilter { faces: true, edges: true, skip_op: None, ..none },
        F_CORNER | F_RELIEF | F_ENTITIES => PickFilter { faces: true, edges: true, skip_op: None, ..none },
        _ => none,
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    if let Some(i) = list.iter().position(|y| *y == x) {
        list.remove(i);
    } else {
        list.push(x);
    }
}

fn sm_pick(cache: &PartCache, pick: Pick) -> Option<SmPick> {
    match pick {
        Pick::Vertex(part, vertex) => cache.part(part).and_then(|p| p.solid.vertex(&vertex)).map(|v| SmPick::Vertex(VertexRef { part, vertex, point: v.point })),
        _ => match crate::applied::entity_of(cache, pick)? {
            EdgeOrFace::Edge(e) => Some(SmPick::Edge(e)),
            EdgeOrFace::Face(f) => Some(SmPick::Face(f)),
        },
    }
}

fn face_ref(cache: &PartCache, pick: Pick) -> Option<FaceRef> {
    match crate::applied::entity_of(cache, pick)? {
        EdgeOrFace::Face(f) => Some(f),
        _ => None,
    }
}

fn same_pick(a: &SmPick, b: &SmPick) -> bool {
    match (a, b) {
        (SmPick::Face(x), SmPick::Face(y)) => x.face == y.face,
        (SmPick::Edge(x), SmPick::Edge(y)) => x.edge == y.edge,
        (SmPick::Vertex(x), SmPick::Vertex(y)) => x.vertex == y.vertex,
        _ => false,
    }
}

/// A line's end points (a sketch line or a straight part edge).
fn line_ends(features: &[Feature], cache: &PartCache, l: &LineRef) -> Option<([f64; 3], [f64; 3])> {
    match l {
        LineRef::Sketch(c) => {
            let sk = features.iter().find(|f| f.id == c.sketch)?.sketch()?;
            let frame = sk.plane?.frame();
            let g = &sk.geometry;
            match g.curves.get(c.curve)?.kind {
                cadrs_sketch::CurveKind::Line { a, b } => Some((frame.to_world(g.pos(a)), frame.to_world(g.pos(b)))),
                _ => None,
            }
        }
        LineRef::Edge(e) => {
            let edge = cache.part(e.part)?.solid.edge(&e.edge)?;
            Some((*edge.points.first()?, *edge.points.last()?))
        }
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The flat face a bend line lies over (the rebuild checks it is a sheet metal wall): parallel to the line, the line's
/// middle inside it (projected), the nearest such face (Onshape fills the field in).
fn face_under(cache: &PartCache, ends: ([f64; 3], [f64; 3])) -> Option<FaceRef> {
    let d = sub(ends.1, ends.0);
    let len = dot(d, d).sqrt();
    if len < 1e-9 {
        return None;
    }
    let mid = [(ends.0[0] + ends.1[0]) / 2.0, (ends.0[1] + ends.1[1]) / 2.0, (ends.0[2] + ends.1[2]) / 2.0];
    let mut best: Option<(FaceRef, f64)> = None;
    for part in &cache.parts {
        for (i, f) in part.solid.faces.iter().enumerate() {
            let Some(pl) = f.plane else { continue };
            let n = pl.normal();
            if (dot(d, n) / len).abs() > 1e-6 {
                continue;
            }
            let dist = dot(sub(mid, pl.origin), n).abs();
            let q = pl.to_sketch(mid);
            let Some(outer) = f.loops.iter().max_by(|a, b| a.len().cmp(&b.len())) else { continue };
            let poly: Vec<cadrs_sketch::Vec2> = outer.iter().map(|p| pl.to_sketch(*p)).collect();
            // A long line may stick out past the face; its middle or a point of it inside will do.
            let inside = |p: cadrs_sketch::Vec2| {
                let n = poly.len();
                let mut c = false;
                let mut j = n.wrapping_sub(1);
                for k in 0..n {
                    let (a, b) = (poly[k], poly[j]);
                    if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                        c = !c;
                    }
                    j = k;
                }
                c
            };
            let hit = (0..=20).any(|k| {
                let t = k as f64 / 20.0;
                inside(pl.to_sketch([ends.0[0] + d[0] * t, ends.0[1] + d[1] * t, ends.0[2] + d[2] * t]))
            }) || inside(q);
            if !hit {
                continue;
            }
            let Some(seed) = part.solid.face_point(i) else { continue };
            // Prefer the face the line lies on, then the bigger face.
            let score = dist - 1e-6 * f.area.unwrap_or(0.0);
            if best.as_ref().is_none_or(|(_, s)| score < *s) {
                best = Some((FaceRef { part: part.id, face: f.name, seed }, score));
            }
        }
    }
    best.map(|(f, _)| f)
}

/// A pick into field `f`. Returns whether it fitted.
fn pick_into(world: &mut World, kind: &mut FeatureKind, f: u8, pick: Pick) -> bool {
    let features: Vec<Feature> = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.features().to_vec()).unwrap_or_default();
    let cache = world.resource::<PartCache>();
    let Some(x) = tool_mut(kind) else { return false };
    match (x, f) {
        (x, F_LINE) => {
            let Some(b) = bend_mut(x) else { return false };
            let line = match pick {
                Pick::SketchCurve(sketch, curve) => {
                    let is_line = features
                        .iter()
                        .find(|f| f.id == sketch)
                        .and_then(|f| f.sketch())
                        .and_then(|s| s.geometry.curves.get(curve))
                        .is_some_and(|c| matches!(c.kind, cadrs_sketch::CurveKind::Line { .. }));
                    if !is_line {
                        return false;
                    }
                    LineRef::Sketch(CurveRef { sketch, curve })
                }
                Pick::Edge(..) => match crate::applied::entity_of(cache, pick) {
                    Some(EdgeOrFace::Edge(e)) => LineRef::Edge(e),
                    _ => return false,
                },
                _ => return false,
            };
            b.line = if b.line == Some(line) { None } else { Some(line) };
            // Fill in the face under it (SM9.3).
            if b.face.is_none()
                && let Some(l) = &b.line
                && let Some(ends) = line_ends(&features, cache, l)
            {
                b.face = face_under(cache, ends);
            }
        }
        (x, F_FACE) => {
            let Some(b) = bend_mut(x) else { return false };
            let Some(fr) = face_ref(cache, pick) else { return false };
            b.face = if b.face.is_some_and(|g| g.face == fr.face) { None } else { Some(fr) };
        }
        (x, F_REF) => {
            let Some(b) = bend_mut(x) else { return false };
            let Some(e) = crate::applied::entity_of(cache, pick) else { return false };
            b.reference = Some(e);
        }
        (SheetMetalTool::Jog(j), F_UP_TO) => {
            let Some(fr) = face_ref(cache, pick) else { return false };
            j.up_to = Some(fr);
        }
        (SheetMetalTool::Tab(t), F_PROFILE) => match pick {
            Pick::Region(s, i) => {
                let Some(r) = cache.sketch_regions(s).and_then(|rs| rs.regions.get(i as usize).cloned()) else { return false };
                let r = RegionRef::new(s, &r);
                match t.regions.iter().position(|y| y.sketch == r.sketch && y.curves == r.curves) {
                    Some(k) => {
                        t.regions.remove(k);
                    }
                    None => t.regions.push(r),
                }
            }
            Pick::Feature(fid) if features.iter().any(|g| g.id == fid && g.sketch().is_some()) => toggle(&mut t.sketches, fid),
            _ => return false,
        },
        (SheetMetalTool::Tab(t), F_FLANGES) => {
            let Some(fr) = face_ref(cache, pick) else { return false };
            match t.flanges.iter().position(|g| g.face == fr.face) {
                Some(k) => {
                    t.flanges.remove(k);
                }
                None => t.flanges.push(fr),
            }
        }
        (SheetMetalTool::Tab(t), F_SCOPE) => match pick.part() {
            Some(p) => toggle(&mut t.scope, p),
            None => return false,
        },
        (SheetMetalTool::Corner(c), F_CORNER) => {
            let Some(p) = sm_pick(cache, pick) else { return false };
            c.corner = if c.corner.is_some_and(|q| same_pick(&q, &p)) { None } else { Some(p) };
        }
        (SheetMetalTool::BendRelief(b), F_RELIEF) => {
            let Some(p) = sm_pick(cache, pick) else { return false };
            b.end = if b.end.is_some_and(|q| same_pick(&q, &p)) { None } else { Some(p) };
        }
        (SheetMetalTool::CornerBreak(c), F_ENTITIES) => {
            let Some(p) = sm_pick(cache, pick) else { return false };
            if matches!(p, SmPick::Face(_)) {
                return false;
            }
            match c.entities.iter().position(|q| same_pick(q, &p)) {
                Some(k) => {
                    c.entities.remove(k);
                }
                None => c.entities.push(p),
            }
        }
        (SheetMetalTool::Finish(fin), F_PARTS) => match pick.part() {
            Some(p) => toggle(&mut fin.parts, p),
            None => return false,
        },
        _ => return false,
    }
    true
}

/// A pick in the open dialog's active field.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let AppliedField::SmTool(f) = field else { return false };
    let ok = pick_into(world, kind, f, pick);
    // A bend line picked with its face filled in: the dialog is complete; else the face next.
    if ok
        && f == F_LINE
        && let Some(b) = tool(kind).and_then(bend_of)
        && b.line.is_some()
        && b.face.is_none()
        && let Some(mut s) = world.get_resource_mut::<AppliedSession>()
    {
        s.field = AppliedField::SmTool(F_FACE);
    }
    ok
}

/// What its fields refer to, shown selected in the view.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let Some(x) = tool(kind) else { return Vec::new() };
    let face = |f: &FaceRef| cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()).map(|p| Pick::Face(p.id, f.face));
    let smp = |p: &SmPick| match p {
        SmPick::Face(f) => face(f),
        SmPick::Edge(e) => cache.parts.iter().find(|p| p.solid.edge(&e.edge).is_some()).map(|p| Pick::Edge(p.id, e.edge)),
        SmPick::Vertex(v) => Some(Pick::Vertex(v.part, v.vertex)),
    };
    let mut v = Vec::new();
    match x {
        SheetMetalTool::Bend(_) | SheetMetalTool::Jog(_) => {
            let b = bend_of(x).expect("bend");
            match &b.line {
                Some(LineRef::Sketch(c)) => v.push(Pick::SketchCurve(c.sketch, c.curve)),
                Some(LineRef::Edge(e)) => v.extend(cache.parts.iter().find(|p| p.solid.edge(&e.edge).is_some()).map(|p| Pick::Edge(p.id, e.edge))),
                None => {}
            }
            if let SheetMetalTool::Jog(j) = x {
                v.extend(j.up_to.iter().filter_map(face));
            }
        }
        SheetMetalTool::Tab(t) => v.extend(t.flanges.iter().filter_map(face)),
        SheetMetalTool::Corner(c) => v.extend(c.corner.iter().filter_map(smp)),
        SheetMetalTool::BendRelief(b) => v.extend(b.end.iter().filter_map(smp)),
        SheetMetalTool::CornerBreak(c) => v.extend(c.entities.iter().filter_map(smp)),
        SheetMetalTool::Finish(f) => v.extend(f.parts.iter().map(|p| Pick::Part(*p))),
    }
    v
}

// ---------------------------------------------------------------------------------------------
// The dialog

/// The dialog's name ("sheet-metal-bend-dialog", …).
pub fn dialog_name(x: &SheetMetalTool) -> &'static str {
    SmTool::of(x).name()
}

/// What decides the dialog's rows.
pub fn layout(x: &SheetMetalTool) -> String {
    match x {
        SheetMetalTool::Bend(b) => format!("sm-bend {}", bend_layout(b)),
        SheetMetalTool::Jog(j) => format!("sm-jog {} {:?} {}", bend_layout(&j.bend), j.bounding, j.up_to_offset_on),
        SheetMetalTool::Tab(_) => "sm-tab".into(),
        SheetMetalTool::Corner(c) => format!("sm-corner {:?}", c.relief.kind),
        SheetMetalTool::BendRelief(b) => format!("sm-bend-relief {:?}", b.relief.kind),
        SheetMetalTool::CornerBreak(c) => format!("sm-corner-break {} {:?} {:?} {}", c.chamfer, c.fillet_measurement, c.chamfer_type, c.flip),
        SheetMetalTool::Finish(_) => "sm-finish".into(),
    }
}

fn bend_layout(b: &BendFeature) -> String {
    format!("{} {:?} {} {} {}", b.hold_opposite, b.control, b.opposite, b.use_model_radius, b.use_model_k)
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn name_of(features: &[Feature], id: FeatureId) -> String {
    features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone())
}

fn pick_label(features: &[Feature], p: &SmPick) -> String {
    let op = match p {
        SmPick::Edge(e) => crate::parts::edge_maker(features, &e.edge),
        _ => p.op(),
    };
    format!("{} of {}", p.what(), op_name(features, op))
}

/// Every list's items (by field).
pub fn items(features: &[Feature], cache: &PartCache, x: &SheetMetalTool) -> Vec<(u8, Vec<String>)> {
    let face = |f: &FaceRef| format!("Face of {}", op_name(features, f.face.op));
    let mut out: Vec<(u8, Vec<String>)> = Vec::new();
    if let Some(b) = bend_of(x) {
        out.push((
            F_LINE,
            b.line
                .iter()
                .map(|l| match l {
                    LineRef::Sketch(c) => format!("Line of {}", name_of(features, c.sketch)),
                    LineRef::Edge(e) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &e.edge))),
                })
                .collect(),
        ));
        out.push((F_FACE, b.face.iter().map(face).collect()));
        out.push((
            F_REF,
            b.reference
                .iter()
                .map(|r| match r {
                    EdgeOrFace::Edge(e) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &e.edge))),
                    EdgeOrFace::Face(f) => face(f),
                })
                .collect(),
        ));
    }
    match x {
        SheetMetalTool::Jog(j) => out.push((F_UP_TO, j.up_to.iter().map(face).collect())),
        SheetMetalTool::Tab(t) => {
            let mut v: Vec<String> = t.regions.iter().map(|r| format!("Face of {}", name_of(features, r.sketch))).collect();
            v.extend(t.sketches.iter().map(|s| name_of(features, *s)));
            out.push((F_PROFILE, v));
            out.push((F_FLANGES, t.flanges.iter().map(face).collect()));
            out.push((F_SCOPE, crate::applied::part_names(cache, &t.scope)));
        }
        SheetMetalTool::Corner(c) => out.push((F_CORNER, c.corner.iter().map(|p| pick_label(features, p)).collect())),
        SheetMetalTool::BendRelief(b) => out.push((F_RELIEF, b.end.iter().map(|p| pick_label(features, p)).collect())),
        SheetMetalTool::CornerBreak(c) => out.push((F_ENTITIES, c.entities.iter().map(|p| pick_label(features, p)).collect())),
        SheetMetalTool::Finish(f) => out.push((F_PARTS, crate::applied::part_names(cache, &f.parts))),
        SheetMetalTool::Bend(_) => {}
    }
    out
}

fn list(b: &mut ChildSpawner, t: &Theme, name: &str, placeholder: &str, f: u8, items: Vec<String>, active: bool) {
    b.spawn((SmtRole::List(f), SelectionList::new(name.to_string()).placeholder(placeholder).items(items).active(active).build(t)))
        .entry::<Node>()
        .and_modify(|mut n| {
            n.flex_grow = 1.0;
            n.margin = UiRect::vertical(Val::Px(2.0));
        });
}

/// A list with an arrow after it (the Bend line and its Hold opposite side arrow).
#[allow(clippy::too_many_arguments)]
fn list_with_flip(b: &mut ChildSpawner, t: &Theme, name: &str, placeholder: &str, f: u8, items: Vec<String>, active: bool, flip: (u8, bool, &str)) {
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
        r.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() })
            .with_children(|c| list(c, t, name, placeholder, f, items, active));
        crate::extrude_dialog::flip_button_any(r, t, &format!("{name}-flip"), SmtRole::Flip(flip.0), flip.1, flip.2);
    });
}

#[allow(clippy::too_many_arguments)]
fn select(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, s: u8, options: Vec<String>, selected: usize, flip: Option<(u8, bool, &str)>) {
    let mut sel = Select::new(name.to_string());
    for o in options {
        sel = sel.option(o, true);
    }
    b.spawn(Node { height: Val::Px(28.0), margin: UiRect::top(Val::Px(2.0)), align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
        if !label.is_empty() {
            r.spawn((t.text(label, t.font_sm, bevy::text::FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(96.0), flex_shrink: 0.0, ..default() }));
        }
        r.spawn((SmtRole::Select(s), sel.selected(selected).build(t))).entry::<Node>().and_modify(|mut n| n.flex_grow = 1.0);
        if let Some((fl, on, tip)) = flip {
            crate::extrude_dialog::flip_button_any(r, t, &format!("{name}-flip"), SmtRole::Flip(fl), on, tip);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn number(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, n: u8, text: &str, flip: Option<(u8, bool, &str)>) {
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
        r.spawn((SmtRole::Number(n), NumberField::new(name.to_string(), label.to_string()).text(text.to_string()).label_width(112.0).build(t)))
            .entry::<Node>()
            .and_modify(|mut n| n.flex_grow = 1.0);
        if let Some((fl, on, tip)) = flip {
            crate::extrude_dialog::flip_button_any(r, t, &format!("{name}-flip"), SmtRole::Flip(fl), on, tip);
        }
    });
}

fn check(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, on: bool) {
    b.spawn(OptionRow::new(name.to_string(), label.to_string()).checked(on).build(t));
}

fn labels<T: Copy>(all: &[T], f: impl Fn(T) -> &'static str) -> Vec<String> {
    all.iter().map(|x| f(*x).to_string()).collect()
}

fn index_of<T: PartialEq>(all: &[T], x: &T) -> usize {
    all.iter().position(|y| y == x).unwrap_or(0)
}

fn column(b: &mut ChildSpawner, f: impl FnOnce(&mut ChildSpawner)) {
    b.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(6.0), Val::ZERO), ..default() }).with_children(f);
}

fn bend_rows(c: &mut ChildSpawner, t: &Theme, p: &str, b: &BendFeature, active: u8, item: &dyn Fn(u8) -> Vec<String>) {
    list_with_flip(c, t, &format!("{p}-line-field"), "Bend line", F_LINE, item(F_LINE), active == F_LINE, (FL_HOLD, b.hold_opposite, "Hold opposite side"));
    list(c, t, &format!("{p}-face-field"), "Sheet metal face to bend", F_FACE, item(F_FACE), active == F_FACE);
    select(c, t, &format!("{p}-alignment"), "Bend alignment", S_ALIGN, labels(&BendAlignment::ALL, BendAlignment::label), index_of(&BendAlignment::ALL, &b.alignment), None);
    select(c, t, &format!("{p}-angle-control"), "", S_CONTROL, labels(&AngleControl::ALL, AngleControl::label), index_of(&AngleControl::ALL, &b.control), Some((FL_OPPOSITE, b.opposite, "Opposite angle")));
    match b.control {
        AngleControl::AlignToGeometry => list(c, t, &format!("{p}-reference-field"), "Parallel to", F_REF, item(F_REF), active == F_REF),
        AngleControl::AngleFromDirection => list(c, t, &format!("{p}-reference-field"), "Direction", F_REF, item(F_REF), active == F_REF),
        AngleControl::BendAngle => {}
    }
    if b.control != AngleControl::AlignToGeometry {
        let label = if b.control == AngleControl::BendAngle { "Bend angle" } else { "Angle" };
        number(c, t, &format!("{p}-angle"), label, N_ANGLE, &b.angle_expr, None);
    }
    check(c, t, "smt-use-model-radius", "Use model bend radius", b.use_model_radius);
    if !b.use_model_radius {
        number(c, t, &format!("{p}-radius"), "Bend radius", N_RADIUS, &b.radius_expr, None);
    }
    check(c, t, "smt-use-model-k", "Use model K Factor", b.use_model_k);
    if !b.use_model_k {
        number(c, t, &format!("{p}-k-factor"), "K Factor", N_K, &b.k_expr, None);
    }
}

/// The dialog's body.
pub fn body(b: &mut ChildSpawner, t: &Theme, x: &SheetMetalTool, field: AppliedField, items: &[(u8, Vec<String>)]) {
    let active = match field {
        AppliedField::SmTool(f) => f,
        _ => u8::MAX,
    };
    let item = |f: u8| items.iter().find(|(k, _)| *k == f).map(|(_, v)| v.clone()).unwrap_or_default();
    match x {
        SheetMetalTool::Bend(bf) => column(b, |c| bend_rows(c, t, "sm-bend", bf, active, &item)),
        SheetMetalTool::Jog(j) => column(b, |c| {
            bend_rows(c, t, "sm-jog", &j.bend, active, &item);
            select(c, t, "sm-jog-bounding", "Bounding type", S_BOUNDING, labels(&JogBounding::ALL, JogBounding::label), index_of(&JogBounding::ALL, &j.bounding), None);
            match j.bounding {
                JogBounding::Blind => number(c, t, "sm-jog-offset", "Jog offset", N_OFFSET, &j.offset_expr, None),
                JogBounding::UpToEntity => {
                    list(c, t, "sm-jog-up-to-field", "Up to entity", F_UP_TO, item(F_UP_TO), active == F_UP_TO);
                    check(c, t, "smt-up-to-offset", "Offset distance", j.up_to_offset_on);
                    if j.up_to_offset_on {
                        number(c, t, "sm-jog-up-to-offset", "Offset distance", N_UP_TO_OFFSET, &j.up_to_offset_expr, None);
                    }
                }
                JogBounding::Thickness => number(c, t, "sm-jog-factor", "Thickness factor", N_FACTOR, &j.factor_expr, None),
            }
            select(c, t, "sm-jog-anchor", "Jog offset anchor", S_ANCHOR, labels(&JogAnchor::ALL, JogAnchor::label), index_of(&JogAnchor::ALL, &j.anchor), None);
            check(c, t, "smt-preserve-material", "Preserve material", j.preserve_material);
        }),
        SheetMetalTool::Tab(tb) => column(b, |c| {
            list(c, t, "sm-tab-profile-field", "Tab profile", F_PROFILE, item(F_PROFILE), active == F_PROFILE);
            list(c, t, "sm-tab-flanges-field", "Flange to merge", F_FLANGES, item(F_FLANGES), active == F_FLANGES);
            number(c, t, "sm-tab-offset", "Subtraction offset", N_TAB_OFFSET, &tb.offset_expr, None);
            list(c, t, "sm-tab-scope-field", "Subtraction scope", F_SCOPE, item(F_SCOPE), active == F_SCOPE);
        }),
        SheetMetalTool::Corner(cf) => column(b, |c| {
            list(c, t, "sm-corner-field", "Corner", F_CORNER, item(F_CORNER), active == F_CORNER);
            select(c, t, "sm-corner-type", "Corner relief type", S_CORNER_TYPE, labels(&CornerReliefKind::ALL, CornerReliefKind::label), index_of(&CornerReliefKind::ALL, &cf.relief.kind), None);
            match cf.relief.kind {
                k if k.is_scaled() => number(c, t, "sm-corner-scale", "Corner relief scale", N_CORNER_SCALE, &cf.scale_expr, None),
                CornerReliefKind::SquareSized => number(c, t, "sm-corner-size", "Corner relief width", N_CORNER_SIZE, &cf.size_expr, None),
                CornerReliefKind::RoundSized => number(c, t, "sm-corner-size", "Corner relief diameter", N_CORNER_SIZE, &cf.size_expr, None),
                _ => {}
            }
        }),
        SheetMetalTool::BendRelief(br) => column(b, |c| {
            list(c, t, "sm-bend-relief-field", "Bend relief", F_RELIEF, item(F_RELIEF), active == F_RELIEF);
            // Onshape's dialog puts the type under its label (`sheetmetalbendrelief-dialog.png`).
            c.spawn((t.text("Bend relief type", t.font_sm, bevy::text::FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::new(Val::Px(2.0), Val::ZERO, Val::Px(6.0), Val::ZERO), ..default() }));
            select(c, t, "sm-bend-relief-type", "", S_RELIEF_TYPE, labels(&BendReliefKind::ALL, BendReliefKind::label), index_of(&BendReliefKind::ALL, &br.relief.kind), None);
            if br.relief.kind.is_scaled() {
                number(c, t, "sm-bend-relief-depth-scale", "Bend relief depth scale", N_DEPTH_SCALE, &br.depth_scale_expr, None);
                number(c, t, "sm-bend-relief-width-scale", "Bend relief width scale", N_WIDTH_SCALE, &br.width_scale_expr, None);
            } else if br.relief.kind.is_sized() {
                number(c, t, "sm-bend-relief-depth", "Bend relief depth", N_DEPTH, &br.depth_expr, None);
            }
            check(c, t, "smt-extend-bend-relief", "Extend bend relief", br.relief.extend);
        }),
        SheetMetalTool::CornerBreak(cb) => {
            b.spawn((SmtRole::Tab, TabStrip::new("sm-corner-break-type").compact().tab("Fillet").tab("Chamfer").selected(usize::from(cb.chamfer)).build(t)));
            column(b, |c| {
                list(c, t, "sm-corner-break-field", "Entities to fillet or chamfer", F_ENTITIES, item(F_ENTITIES), active == F_ENTITIES);
                if cb.chamfer {
                    select(c, t, "sm-corner-break-measurement", "Measurement", S_CHAMFER_MEAS, labels(&ChamferMeasurement::ALL, ChamferMeasurement::label), index_of(&ChamferMeasurement::ALL, &cb.chamfer_measurement), None);
                    select(c, t, "sm-corner-break-chamfer-type", "Chamfer type", S_CHAMFER_TYPE, labels(&ChamferType::ALL, ChamferType::label), index_of(&ChamferType::ALL, &cb.chamfer_type), None);
                    let flip = (cb.chamfer_type != ChamferType::EqualDistance).then_some((FL_CHAMFER, cb.flip, "Opposite direction"));
                    let first = if cb.chamfer_type == ChamferType::TwoDistances { "Distance 1" } else { "Distance" };
                    number(c, t, "sm-corner-break-distance", first, N_DIST, &cb.distance_expr, flip);
                    match cb.chamfer_type {
                        ChamferType::TwoDistances => number(c, t, "sm-corner-break-distance2", "Distance 2", N_DIST2, &cb.distance2_expr, None),
                        ChamferType::DistanceAngle => number(c, t, "sm-corner-break-angle", "Angle", N_CANGLE, &cb.angle_expr, None),
                        ChamferType::EqualDistance => {}
                    }
                } else {
                    select(c, t, "sm-corner-break-measurement", "Measurement", S_FILLET_MEAS, labels(&FilletMeasurement::ALL, FilletMeasurement::label), index_of(&FilletMeasurement::ALL, &cb.fillet_measurement), None);
                    // Conic and Curvature are out of scope (the user's decision): Distance only.
                    select(c, t, "sm-corner-break-control", "Control", S_FILLET_CONTROL, vec!["Distance".into()], 0, None);
                    number(c, t, "sm-corner-break-size", cb.fillet_measurement.label(), N_SIZE, &cb.size_expr, None);
                }
            });
        }
        SheetMetalTool::Finish(_) => column(b, |c| {
            list(c, t, "sm-finish-parts-field", "Sheet metal parts", F_PARTS, item(F_PARTS), active == F_PARTS);
            c.spawn((
                Name::new("sm-finish-warning"),
                Node { flex_direction: FlexDirection::Row, align_items: AlignItems::FlexStart, column_gap: Val::Px(6.0), margin: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(6.0), Val::Px(4.0)), ..default() },
            ))
            .with_children(|r| {
                r.spawn(icon("warning-filled", 14.0, Color::srgb_u8(0xd9, 0x8c, 0x00))).entry::<Node>().and_modify(|mut n| {
                    n.flex_shrink = 0.0;
                    n.margin = UiRect::top(Val::Px(1.0));
                });
                r.spawn((
                    t.text(FINISH_WARNING, t.font_sm, bevy::text::FontWeight::NORMAL, t.foreground),
                    Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), ..default() },
                ))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            });
        }),
    }
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step

fn number_text(x: &SheetMetalTool, n: u8) -> Option<String> {
    let b = bend_of(x);
    Some(match (x, n) {
        (_, N_ANGLE) => b?.angle_expr.clone(),
        (_, N_RADIUS) => b?.radius_expr.clone(),
        (_, N_K) => b?.k_expr.clone(),
        (SheetMetalTool::Jog(j), N_OFFSET) => j.offset_expr.clone(),
        (SheetMetalTool::Jog(j), N_UP_TO_OFFSET) => j.up_to_offset_expr.clone(),
        (SheetMetalTool::Jog(j), N_FACTOR) => j.factor_expr.clone(),
        (SheetMetalTool::Tab(t), N_TAB_OFFSET) => t.offset_expr.clone(),
        (SheetMetalTool::Corner(c), N_CORNER_SCALE) => c.scale_expr.clone(),
        (SheetMetalTool::Corner(c), N_CORNER_SIZE) => c.size_expr.clone(),
        (SheetMetalTool::BendRelief(r), N_DEPTH_SCALE) => r.depth_scale_expr.clone(),
        (SheetMetalTool::BendRelief(r), N_WIDTH_SCALE) => r.width_scale_expr.clone(),
        (SheetMetalTool::BendRelief(r), N_DEPTH) => r.depth_expr.clone(),
        (SheetMetalTool::CornerBreak(c), N_SIZE) => c.size_expr.clone(),
        (SheetMetalTool::CornerBreak(c), N_DIST) => c.distance_expr.clone(),
        (SheetMetalTool::CornerBreak(c), N_DIST2) => c.distance2_expr.clone(),
        (SheetMetalTool::CornerBreak(c), N_CANGLE) => c.angle_expr.clone(),
        _ => return None,
    })
}

fn quantity(n: u8) -> Quantity {
    match n {
        N_ANGLE | N_CANGLE => Quantity::Angle,
        N_K | N_FACTOR | N_CORNER_SCALE | N_DEPTH_SCALE | N_WIDTH_SCALE => Quantity::Count,
        _ => Quantity::Length,
    }
}

fn select_index(x: &SheetMetalTool, s: u8) -> Option<usize> {
    let b = bend_of(x);
    Some(match (x, s) {
        (_, S_ALIGN) => index_of(&BendAlignment::ALL, &b?.alignment),
        (_, S_CONTROL) => index_of(&AngleControl::ALL, &b?.control),
        (SheetMetalTool::Jog(j), S_BOUNDING) => index_of(&JogBounding::ALL, &j.bounding),
        (SheetMetalTool::Jog(j), S_ANCHOR) => index_of(&JogAnchor::ALL, &j.anchor),
        (SheetMetalTool::Corner(c), S_CORNER_TYPE) => index_of(&CornerReliefKind::ALL, &c.relief.kind),
        (SheetMetalTool::BendRelief(r), S_RELIEF_TYPE) => index_of(&BendReliefKind::ALL, &r.relief.kind),
        (SheetMetalTool::CornerBreak(c), S_FILLET_MEAS) => index_of(&FilletMeasurement::ALL, &c.fillet_measurement),
        (SheetMetalTool::CornerBreak(_), S_FILLET_CONTROL) => 0,
        (SheetMetalTool::CornerBreak(c), S_CHAMFER_MEAS) => index_of(&ChamferMeasurement::ALL, &c.chamfer_measurement),
        (SheetMetalTool::CornerBreak(c), S_CHAMFER_TYPE) => index_of(&ChamferType::ALL, &c.chamfer_type),
        _ => return None,
    })
}

/// Keeps the lists, numbers and selects of the open dialog in step with the feature.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_tool_dialog(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    focus: Res<InputFocus>,
    q_edit: Query<&cadrs_ui::NumberFieldEdit>,
    mut q_lists: Query<(&SmtRole, &mut SelectionListState)>,
    mut q_numbers: Query<(Entity, &SmtRole, &mut NumberFieldState), Without<SelectionListState>>,
    mut q_selects: Query<(&SmtRole, &mut SelectState)>,
) {
    let Some(s) = session else { return };
    let Some(el) = doc.as_ref().and_then(|d| d.doc.element(s.element)) else { return };
    let Some(x) = el.feature(s.feature).and_then(|f| tool(&f.kind)) else { return };
    let all = items(el.features(), &cache, x);
    for (role, mut l) in &mut q_lists {
        let SmtRole::List(f) = *role else { continue };
        let want = SelectionListState {
            items: all.iter().find(|(k, _)| *k == f).map(|(_, v)| v.clone()).unwrap_or_default(),
            active: s.field == AppliedField::SmTool(f),
            error: false,
            red_items: false,
            red: Vec::new(),
        };
        if *l != want {
            *l = want;
        }
    }
    let editing = focus.get().and_then(|f| q_edit.get(f).ok()).map(|e| e.0);
    for (entity, role, mut st) in &mut q_numbers {
        let SmtRole::Number(n) = *role else { continue };
        let Some(text) = number_text(x, n) else { continue };
        if editing == Some(entity) || st.error {
            continue;
        }
        let want = NumberFieldState { text, error: false };
        if *st != want {
            *st = want;
        }
    }
    for (role, mut sel) in &mut q_selects {
        let SmtRole::Select(k) = *role else { continue };
        if let Some(i) = select_index(x, k)
            && sel.selected != i
        {
            sel.selected = i;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Input

fn change(commands: &mut Commands, label: &'static str, f: impl FnOnce(&mut SheetMetalTool) + Send + 'static) {
    commands.queue(move |world: &mut World| {
        crate::applied::change_kind(world, label, |k| {
            if let Some(x) = tool_mut(k) {
                f(x);
            }
        });
    });
}

fn set_field(commands: &mut Commands, f: u8) {
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<AppliedSession>()
            && s.field != AppliedField::SmTool(f)
        {
            s.field = AppliedField::SmTool(f);
        }
    });
}

fn on_list_activate(ev: On<SelectionListActivate>, q: Query<&SmtRole>, mut commands: Commands) {
    if let Ok(SmtRole::List(f)) = q.get(ev.entity) {
        set_field(&mut commands, *f);
    }
}

fn on_list_remove(ev: On<SelectionListRemove>, q: Query<&SmtRole>, mut commands: Commands) {
    let Ok(SmtRole::List(f)) = q.get(ev.entity).copied() else { return };
    let i = ev.index;
    change(&mut commands, "Remove selection", move |x| {
        fn at<T>(v: &mut Vec<T>, i: usize) {
            if i < v.len() {
                v.remove(i);
            }
        }
        match (x, f) {
            (x, F_LINE) => {
                if let Some(b) = bend_mut(x) {
                    b.line = None;
                }
            }
            (x, F_FACE) => {
                if let Some(b) = bend_mut(x) {
                    b.face = None;
                }
            }
            (x, F_REF) => {
                if let Some(b) = bend_mut(x) {
                    b.reference = None;
                }
            }
            (SheetMetalTool::Jog(j), F_UP_TO) => j.up_to = None,
            (SheetMetalTool::Tab(t), F_PROFILE) => {
                if i < t.regions.len() {
                    t.regions.remove(i);
                } else {
                    at(&mut t.sketches, i - t.regions.len());
                }
            }
            (SheetMetalTool::Tab(t), F_FLANGES) => at(&mut t.flanges, i),
            (SheetMetalTool::Tab(t), F_SCOPE) => at(&mut t.scope, i),
            (SheetMetalTool::Corner(c), F_CORNER) => c.corner = None,
            (SheetMetalTool::BendRelief(b), F_RELIEF) => b.end = None,
            (SheetMetalTool::CornerBreak(c), F_ENTITIES) => at(&mut c.entities, i),
            (SheetMetalTool::Finish(fin), F_PARTS) => at(&mut fin.parts, i),
            _ => {}
        }
    });
}

fn on_select(ev: On<SelectChange>, q: Query<&SmtRole>, mut commands: Commands) {
    let Ok(SmtRole::Select(s)) = q.get(ev.entity).copied() else { return };
    let i = ev.index;
    let label = match s {
        S_ALIGN => "Bend alignment",
        S_CONTROL => "Angle control",
        S_BOUNDING => "Bounding type",
        S_ANCHOR => "Jog offset anchor",
        S_CORNER_TYPE => "Corner relief type",
        S_RELIEF_TYPE => "Bend relief type",
        S_FILLET_MEAS | S_CHAMFER_MEAS => "Measurement",
        S_CHAMFER_TYPE => "Chamfer type",
        _ => "Control",
    };
    change(&mut commands, label, move |x| {
        if let Some(b) = bend_mut(x) {
            match s {
                S_ALIGN => b.alignment = BendAlignment::ALL[i.min(5)],
                S_CONTROL => b.control = AngleControl::ALL[i.min(2)],
                _ => {}
            }
        }
        match (x, s) {
            (SheetMetalTool::Jog(j), S_BOUNDING) => j.bounding = JogBounding::ALL[i.min(2)],
            (SheetMetalTool::Jog(j), S_ANCHOR) => j.anchor = JogAnchor::ALL[i.min(2)],
            (SheetMetalTool::Corner(c), S_CORNER_TYPE) => c.relief.kind = CornerReliefKind::ALL[i.min(5)],
            (SheetMetalTool::BendRelief(r), S_RELIEF_TYPE) => r.relief.kind = BendReliefKind::ALL[i.min(4)],
            (SheetMetalTool::CornerBreak(c), S_FILLET_MEAS) => c.fillet_measurement = FilletMeasurement::ALL[i.min(1)],
            (SheetMetalTool::CornerBreak(c), S_CHAMFER_MEAS) => c.chamfer_measurement = ChamferMeasurement::ALL[i.min(1)],
            (SheetMetalTool::CornerBreak(c), S_CHAMFER_TYPE) => c.chamfer_type = ChamferType::ALL[i.min(2)],
            _ => {}
        }
    });
    // A reference to pick next.
    if s == S_CONTROL && i > 0 {
        set_field(&mut commands, F_REF);
    }
    if s == S_BOUNDING && i == 1 {
        set_field(&mut commands, F_UP_TO);
    }
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let on = ev.checked;
    let (label, which): (&'static str, u8) = match name.as_str() {
        "smt-use-model-radius-checkbox" => ("Use model bend radius", 0),
        "smt-use-model-k-checkbox" => ("Use model K Factor", 1),
        "smt-preserve-material-checkbox" => ("Preserve material", 2),
        "smt-up-to-offset-checkbox" => ("Offset distance", 3),
        "smt-extend-bend-relief-checkbox" => ("Extend bend relief", 4),
        _ => return,
    };
    change(&mut commands, label, move |x| {
        match which {
            0 => {
                if let Some(b) = bend_mut(x) {
                    b.use_model_radius = on;
                }
            }
            1 => {
                if let Some(b) = bend_mut(x) {
                    b.use_model_k = on;
                }
            }
            _ => {}
        }
        match (x, which) {
            (SheetMetalTool::Jog(j), 2) => j.preserve_material = on,
            (SheetMetalTool::Jog(j), 3) => j.up_to_offset_on = on,
            (SheetMetalTool::BendRelief(r), 4) => r.relief.extend = on,
            _ => {}
        }
    });
}

fn on_flip(a: On<Activate>, q: Query<&SmtRole>, mut commands: Commands) {
    let Ok(SmtRole::Flip(f)) = q.get(a.entity).copied() else { return };
    let label = match f {
        FL_HOLD => "Hold opposite side",
        FL_OPPOSITE => "Opposite angle",
        _ => "Opposite direction",
    };
    change(&mut commands, label, move |x| match (x, f) {
        (SheetMetalTool::CornerBreak(c), FL_CHAMFER) => c.flip = !c.flip,
        (x, FL_HOLD) => {
            if let Some(b) = bend_mut(x) {
                b.hold_opposite = !b.hold_opposite;
            }
        }
        (x, FL_OPPOSITE) => {
            if let Some(b) = bend_mut(x) {
                b.opposite = !b.opposite;
            }
        }
        _ => {}
    });
}

fn on_tab(ev: On<TabStripSelect>, q: Query<&SmtRole>, mut commands: Commands) {
    if q.get(ev.entity).copied() != Ok(SmtRole::Tab) {
        return;
    }
    let chamfer = ev.index == 1;
    change(&mut commands, "Corner break type", move |x| {
        if let SheetMetalTool::CornerBreak(c) = x {
            c.chamfer = chamfer;
        }
    });
}

/// Sets a number (value and expression).
fn set_number(x: &mut SheetMetalTool, n: u8, v: f64, expr: String) {
    if let Some(b) = bend_mut(x) {
        match n {
            N_ANGLE => {
                b.angle = v;
                b.angle_expr = expr;
                return;
            }
            N_RADIUS => {
                b.radius = v;
                b.radius_expr = expr;
                return;
            }
            N_K => {
                b.k_factor = v;
                b.k_expr = expr;
                return;
            }
            _ => {}
        }
    }
    let (value, text) = match (x, n) {
        (SheetMetalTool::Jog(j), N_OFFSET) => (&mut j.offset, &mut j.offset_expr),
        (SheetMetalTool::Jog(j), N_UP_TO_OFFSET) => (&mut j.up_to_offset, &mut j.up_to_offset_expr),
        (SheetMetalTool::Jog(j), N_FACTOR) => (&mut j.factor, &mut j.factor_expr),
        (SheetMetalTool::Tab(t), N_TAB_OFFSET) => (&mut t.offset, &mut t.offset_expr),
        (SheetMetalTool::Corner(c), N_CORNER_SCALE) => (&mut c.relief.scale, &mut c.scale_expr),
        (SheetMetalTool::Corner(c), N_CORNER_SIZE) => (&mut c.relief.size, &mut c.size_expr),
        (SheetMetalTool::BendRelief(r), N_DEPTH_SCALE) => (&mut r.relief.depth_scale, &mut r.depth_scale_expr),
        (SheetMetalTool::BendRelief(r), N_WIDTH_SCALE) => (&mut r.relief.width_scale, &mut r.width_scale_expr),
        (SheetMetalTool::BendRelief(r), N_DEPTH) => (&mut r.relief.depth, &mut r.depth_expr),
        (SheetMetalTool::CornerBreak(c), N_SIZE) => (&mut c.size, &mut c.size_expr),
        (SheetMetalTool::CornerBreak(c), N_DIST) => (&mut c.distance, &mut c.distance_expr),
        (SheetMetalTool::CornerBreak(c), N_DIST2) => (&mut c.distance2, &mut c.distance2_expr),
        (SheetMetalTool::CornerBreak(c), N_CANGLE) => (&mut c.angle, &mut c.angle_expr),
        _ => return,
    };
    *value = v;
    *text = expr;
}

fn on_number(ev: On<NumberFieldCommit>, q: Query<&SmtRole>, mut commands: Commands) {
    let Ok(SmtRole::Number(n)) = q.get(ev.entity).copied() else { return };
    let (entity, text, enter) = (ev.entity, ev.text.clone(), ev.enter);
    commands.queue(move |world: &mut World| {
        let units = world.resource::<crate::WorkspaceUnits>().0;
        let parsed = crate::sheetmetal_ui::parse(&text, quantity(n), &units, world.resource::<crate::variables_ui::ActiveVariables>());
        if let Some(mut st) = world.get_mut::<NumberFieldState>(entity) {
            st.error = parsed.is_none();
            if parsed.is_none() {
                st.text = text.clone();
            }
        }
        let Some((v, expr)) = parsed else { return };
        crate::applied::change_kind(world, "Change value", |k| {
            if let Some(x) = tool_mut(k) {
                set_number(x, n, v, expr);
            }
        });
        if enter {
            world.resource_mut::<InputFocus>().clear();
            crate::applied::accept(world);
        }
    });
}
