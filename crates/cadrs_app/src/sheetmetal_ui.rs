//! The **Sheet metal model** dialog (P3I.2, SM2; `reference/onshape/sheetmetal/help/`
//! `feature-tools/sheetmetal-dialog-convert-02.png`, `sheetmetal-extrude-02.png`,
//! `sheetmetal-thicken-02.png`) in the applied-feature dialogs:
//!
//! - **Convert | Extrude | Thicken** tabs, then four collapsible sections:
//! - **Selections**: Convert: *Parts and surfaces to convert*, *Faces to exclude*, *Edges or
//!   cylinders to bend* (pick order kept: it decides the flat), *Clearance from input*, *Include
//!   bends*, *Keep input parts*. Extrude: *Sketch curves to extrude*, *Arcs to extrude as bends*,
//!   the end type (Blind, Up to next, Up to face, Up to part, Up to vertex) with the opposite
//!   direction arrow, *Depth* (or the Up to entity), *Symmetric*, *Second end position*. Thicken:
//!   *Faces or sketch regions to thicken*, *Tangent propagation*, *Edges or cylinders to bend*,
//!   *Clearance from input*, *Include bends*. The header is red while nothing is selected.
//! - **General**: *Thickness* with the opposite direction arrow (the side the material goes),
//!   *Bend radius*, *Flip direction up*.
//! - **Material**: *Bend calculation* (K Factor, Bend allowance, Bend deduction) with *Default
//!   bend K Factor* (or the allowance / deduction) and *Rolled K Factor*.
//! - **Relief**: *Minimal gap*, *Corner relief type* with its scale or size, *Bend relief type*
//!   with its depth and width scales.
//!
//! A value out of its range (X5) is kept but its field turns red, with the range as its tooltip
//! (`cadrs_sheetmetal::Params::validate`), and the feature can't be accepted. The toolbar's
//! Sheet metal model button starts it with the selection (parts → Convert, sketch curves →
//! Extrude, faces or regions → Thicken); its ▾ lists the other sheet metal tools in Onshape's
//! order (not built yet: greyed).

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::applied::EdgeOrFace;
use cadrs_core::document::{EndCondition, EndType, FaceRef, RegionRef, UpTo, VertexRef};
use cadrs_core::sheetmetal::{CurveRef, EXTRUDE_ENDS, SheetMetalModelFeature, SheetMetalOp, plain};
use cadrs_core::{Feature, FeatureId, FeatureKind};
use cadrs_sheetmetal::params::{BendCalc, BendReliefKind, CornerReliefKind};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{Collapsible, CollapsibleToggled, NumberField, NumberFieldState, OptionRow, Select, SelectionList, TabStrip};

use crate::ActiveDocument;
use crate::applied::{AppliedField, AppliedSession};
use crate::applied_dialog::{Role, SmNum};
use crate::parts::PartCache;
use crate::viewport::Pick;

/// The sections' names, in order (their open state is kept in the session).
pub const SECTIONS: [(&str, &str); 4] = [("sm-selections", "Selections"), ("sm-general", "General"), ("sm-material", "Material"), ("sm-relief", "Relief")];

/// The other sheet metal tools, in Onshape's order (the ▾ of the toolbar's Sheet metal model
/// button): name, label, icon.
pub const OTHER_TOOLS: [(&str, &str, &str); 12] = [
    ("sheet-metal-finish", "Finish sheet metal model", "sheet-metal-finish"),
    ("sheet-metal-flange", "Flange", "sheet-metal-flange"),
    ("sheet-metal-hem", "Hem", "sheet-metal-hem"),
    ("sheet-metal-tab", "Tab", "sheet-metal-tab"),
    ("sheet-metal-bend", "Bend", "sheet-metal-bend"),
    ("sheet-metal-jog", "Jog", "sheet-metal-jog"),
    ("sheet-metal-form", "Form", "sheet-metal-form"),
    ("sheet-metal-loft", "Loft", "sheet-metal-loft"),
    ("sheet-metal-make-joint", "Make joint", "sheet-metal-make-joint"),
    ("sheet-metal-corner", "Corner", "sheet-metal-corner"),
    ("sheet-metal-bend-relief", "Bend relief", "sheet-metal-bend-relief"),
    ("sheet-metal-corner-break", "Corner break", "sheet-metal-corner-break"),
];

/// Why the other sheet metal tools are greyed.
pub const NOT_YET: &str = "Not available yet: comes with the next sheet metal tools";

// ---------------------------------------------------------------------------------------------
// New features and picks

fn is_sketch(features: &[Feature], f: FeatureId) -> bool {
    features.iter().any(|x| x.id == f && x.sketch().is_some())
}

fn region_of(cache: &PartCache, sketch: FeatureId, index: u32) -> Option<RegionRef> {
    let r = cache.sketch_regions(sketch)?.regions.get(index as usize)?.clone();
    Some(RegionRef::new(sketch, &r))
}

fn face_ref(cache: &PartCache, pick: Pick) -> Option<FaceRef> {
    match crate::applied::entity_of(cache, pick)? {
        EdgeOrFace::Face(f) => Some(f),
        _ => None,
    }
}

/// The field a tab's selections start in.
pub fn first_field(op: SheetMetalOp) -> AppliedField {
    match op {
        SheetMetalOp::Convert => AppliedField::SmParts,
        SheetMetalOp::Extrude => AppliedField::SmCurves,
        SheetMetalOp::Thicken => AppliedField::SmFaces,
    }
}

/// A new model from the selection: parts → Convert, sketch curves → Extrude, faces or regions →
/// Thicken (Convert, empty, otherwise: Onshape's default tab).
pub fn initial(world: &World, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let features = world.get_resource::<ActiveDocument>()?.active_element()?.features().to_vec();
    let cache = world.resource::<PartCache>();
    let mut x = SheetMetalModelFeature::default();
    let units = world.get_resource::<crate::WorkspaceUnits>().map(|u| u.0).unwrap_or_default();
    // The lengths as the document shows them.
    let p = x.params;
    x.exprs.thickness = units.with_unit(p.thickness, Quantity::Length);
    x.exprs.bend_radius = units.with_unit(p.bend_radius, Quantity::Length);
    x.exprs.minimal_gap = units.with_unit(p.minimal_gap, Quantity::Length);
    x.exprs.bend_allowance = units.with_unit(p.bend_allowance, Quantity::Length);
    x.exprs.bend_deduction = units.with_unit(p.bend_deduction, Quantity::Length);
    x.exprs.corner_relief_size = units.with_unit(p.corner_relief.size, Quantity::Length);
    x.clearance_expr = units.with_unit(0.0, Quantity::Length);
    x.depth_expr = units.with_unit(x.depth, Quantity::Length);
    for pk in picked {
        match *pk {
            Pick::Part(p) => x.parts.push(p),
            Pick::SketchCurve(sketch, curve) => x.curves.push(CurveRef { sketch, curve }),
            Pick::Region(s, i) => x.regions.extend(region_of(cache, s, i)),
            Pick::Face(..) => x.faces.extend(face_ref(cache, *pk)),
            Pick::Feature(f) if is_sketch(&features, f) => x.sketches.push(f),
            _ => {}
        }
    }
    x.operation = if !x.parts.is_empty() {
        SheetMetalOp::Convert
    } else if !x.curves.is_empty() || !x.sketches.is_empty() {
        SheetMetalOp::Extrude
    } else if !x.faces.is_empty() || !x.regions.is_empty() {
        SheetMetalOp::Thicken
    } else {
        SheetMetalOp::Convert
    };
    let field = first_field(x.operation);
    Some(("Sheet metal model", FeatureKind::SheetMetalModel(x), field))
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    if let Some(i) = list.iter().position(|y| *y == x) {
        list.remove(i);
    } else {
        list.push(x);
    }
}

/// A pick into the dialog's fields. Returns false if it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let FeatureKind::SheetMetalModel(x) = kind else { return false };
    let Some(features) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.features().to_vec()) else {
        return false;
    };
    let cache = world.resource::<PartCache>();
    match field {
        AppliedField::SmParts => match pick.part() {
            // Not a part this model made (its own result).
            Some(p) if world.get_resource::<AppliedSession>().is_some_and(|s| s.feature == p.feature) => return false,
            Some(p) => toggle(&mut x.parts, p),
            None => return false,
        },
        AppliedField::SmExclude => {
            let Some(f) = face_ref(cache, pick) else { return false };
            match x.exclude.iter().position(|g| g.face == f.face) {
                Some(i) => {
                    x.exclude.remove(i);
                }
                None => x.exclude.push(f),
            }
        }
        AppliedField::SmBends => {
            // Pick order matters (SM2.2): a new pick goes last; picking it again removes it.
            let Some(e) = crate::applied::entity_of(cache, pick) else { return false };
            if let EdgeOrFace::Face(f) = &e {
                // Only cylinders bend.
                let cyl = cache.parts.iter().find_map(|p| p.solid.face(&f.face)).and_then(|g| g.kind) == Some(cadrs_kernel::SurfaceKind::Cylinder);
                if !cyl {
                    return false;
                }
            }
            let same = |a: &EdgeOrFace| match (a, &e) {
                (EdgeOrFace::Edge(a), EdgeOrFace::Edge(b)) => a.edge == b.edge,
                (EdgeOrFace::Face(a), EdgeOrFace::Face(b)) => a.face == b.face,
                _ => false,
            };
            match x.bends.iter().position(same) {
                Some(i) => {
                    x.bends.remove(i);
                }
                None => x.bends.push(e),
            }
        }
        AppliedField::SmCurves => match pick {
            Pick::SketchCurve(sketch, curve) => toggle(&mut x.curves, CurveRef { sketch, curve }),
            Pick::Feature(f) if is_sketch(&features, f) => toggle(&mut x.sketches, f),
            _ => return false,
        },
        AppliedField::SmArcs => match pick {
            Pick::SketchCurve(sketch, curve) => {
                let is_arc = features
                    .iter()
                    .find(|f| f.id == sketch)
                    .and_then(|f| f.sketch())
                    .and_then(|s| s.geometry.curves.get(curve))
                    .is_some_and(|c| matches!(c.kind, cadrs_sketch::CurveKind::Arc { .. }));
                if !is_arc {
                    return false;
                }
                toggle(&mut x.arcs_as_bends, CurveRef { sketch, curve });
            }
            _ => return false,
        },
        AppliedField::SmFaces => match pick {
            Pick::Region(s, i) => {
                let Some(r) = region_of(cache, s, i) else { return false };
                match x.regions.iter().position(|y| y.sketch == r.sketch && y.curves == r.curves) {
                    Some(k) => {
                        x.regions.remove(k);
                    }
                    None => x.regions.push(r),
                }
            }
            Pick::Feature(f) if is_sketch(&features, f) => toggle(&mut x.region_sketches, f),
            Pick::Face(..) => {
                let Some(f) = face_ref(cache, pick) else { return false };
                match x.faces.iter().position(|g| g.face == f.face) {
                    Some(i) => {
                        x.faces.remove(i);
                    }
                    None => x.faces.push(f),
                }
            }
            _ => return false,
        },
        AppliedField::SmUpTo | AppliedField::SmSecondUpTo => {
            let end = if field == AppliedField::SmUpTo { x.end } else { x.second.as_ref().map_or(EndType::Blind, |s| s.end) };
            let target = match (end, pick) {
                (EndType::UpToFace, _) => face_ref(cache, pick).map(UpTo::Face),
                (EndType::UpToPart, _) => pick.part().map(UpTo::Part),
                (EndType::UpToVertex, Pick::Vertex(part, vertex)) => cache
                    .part(part)
                    .and_then(|p| p.solid.vertex(&vertex))
                    .map(|v| UpTo::Vertex(VertexRef { part, vertex, point: v.point })),
                _ => None,
            };
            let Some(t) = target else { return false };
            let slot = if field == AppliedField::SmUpTo {
                &mut x.up_to
            } else {
                match x.second.as_mut() {
                    Some(s) => &mut s.up_to,
                    None => return false,
                }
            };
            *slot = if *slot == Some(t) { None } else { Some(t) };
        }
        _ => return false,
    }
    true
}

/// What a pick of `field` may be.
pub fn pick_filter(field: AppliedField, end: EndType, none: crate::parts::PickFilter) -> Option<crate::parts::PickFilter> {
    use crate::parts::PickFilter;
    Some(match field {
        // A part is picked by any of its faces or edges (or in the Parts list).
        AppliedField::SmParts => PickFilter { faces: true, edges: true, ..none },
        AppliedField::SmExclude => PickFilter { faces: true, ..none },
        AppliedField::SmBends => PickFilter { faces: true, edges: true, ..none },
        AppliedField::SmCurves | AppliedField::SmArcs => PickFilter { sketch_curves: true, ..none },
        AppliedField::SmFaces => PickFilter { faces: true, regions: true, ..none },
        AppliedField::SmUpTo | AppliedField::SmSecondUpTo => match end {
            EndType::UpToVertex => PickFilter { edges: true, ..none },
            _ => PickFilter { faces: true, edges: true, ..none },
        },
        _ => return None,
    })
}

/// What its fields refer to, shown selected in the view while the dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let FeatureKind::SheetMetalModel(x) = kind else { return Vec::new() };
    let face = |f: &FaceRef| cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()).map(|p| Pick::Face(p.id, f.face));
    let mut v: Vec<Pick> = Vec::new();
    match x.operation {
        SheetMetalOp::Convert => {
            v.extend(x.exclude.iter().filter_map(face));
        }
        SheetMetalOp::Thicken => {
            v.extend(x.faces.iter().filter_map(face));
        }
        SheetMetalOp::Extrude => {
            v.extend(x.curves.iter().map(|c| Pick::SketchCurve(c.sketch, c.curve)));
            v.extend(x.arcs_as_bends.iter().map(|c| Pick::SketchCurve(c.sketch, c.curve)));
        }
    }
    if x.operation != SheetMetalOp::Extrude {
        for b in &x.bends {
            match b {
                EdgeOrFace::Edge(e) => {
                    if let Some(p) = cache.parts.iter().find(|p| p.solid.edge(&e.edge).is_some()) {
                        v.push(Pick::Edge(p.id, e.edge));
                    }
                }
                EdgeOrFace::Face(f) => v.extend(face(f)),
            }
        }
    }
    v
}

/// The edges and faces to draw where they were before the feature (amber): the edges to bend
/// and the faces left out, which a Convert consumes with its part.
pub fn drawn_references(kind: &FeatureKind) -> Vec<EdgeOrFace> {
    let FeatureKind::SheetMetalModel(x) = kind else { return Vec::new() };
    let mut v: Vec<EdgeOrFace> = Vec::new();
    if x.operation == SheetMetalOp::Extrude {
        return v;
    }
    v.extend(x.bends.iter().copied());
    if x.operation == SheetMetalOp::Convert {
        v.extend(x.exclude.iter().map(|f| EdgeOrFace::Face(*f)));
    }
    v
}

// ---------------------------------------------------------------------------------------------
// The dialog

/// What decides the dialog's rows.
pub fn layout(x: &SheetMetalModelFeature) -> String {
    let p = &x.params;
    format!(
        "sheet-metal {:?} {} {:?} {} {:?} {:?} {} {} {:?} {:?} {:?} {} {}",
        x.operation,
        x.is_empty(),
        x.end,
        x.flip_extrude,
        x.second.as_ref().map(|s| s.end),
        p.bend_calc,
        x.symmetric,
        x.flip_thickness,
        p.corner_relief.kind,
        p.bend_relief.kind,
        x.up_to.is_some(),
        x.include_bends,
        x.keep_input,
    )
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn name_of(features: &[Feature], id: FeatureId) -> String {
    features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone())
}

fn curve_label(features: &[Feature], c: &CurveRef) -> String {
    let what = features
        .iter()
        .find(|f| f.id == c.sketch)
        .and_then(|f| f.sketch())
        .and_then(|s| s.geometry.curves.get(c.curve))
        .map_or("Curve", |cv| match cv.kind {
            cadrs_sketch::CurveKind::Line { .. } => "Line",
            cadrs_sketch::CurveKind::Arc { .. } => "Arc",
            cadrs_sketch::CurveKind::Circle { .. } => "Circle",
            _ => "Curve",
        });
    format!("{what} of {}", name_of(features, c.sketch))
}

fn up_to_label(features: &[Feature], cache: &PartCache, u: &UpTo) -> String {
    match u {
        UpTo::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
        UpTo::Part(p) => cache.part_name(*p).unwrap_or("Part").to_string(),
        UpTo::Vertex(v) => format!("Vertex of {}", op_name(features, v.vertex.faces[0].op)),
    }
}

/// The items of the dialog's lists.
pub(crate) fn items(features: &[Feature], cache: &PartCache, x: &SheetMetalModelFeature, role: Role) -> Option<Vec<String>> {
    Some(match role {
        Role::SmParts => crate::applied::part_names(cache, &x.parts),
        Role::SmExclude => x.exclude.iter().map(|f| format!("Face of {}", op_name(features, f.face.op))).collect(),
        Role::SmBends => x
            .bends
            .iter()
            .map(|b| match b {
                EdgeOrFace::Edge(e) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &e.edge))),
                EdgeOrFace::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
            })
            .collect(),
        Role::SmCurves => {
            let mut v: Vec<String> = x.curves.iter().map(|c| curve_label(features, c)).collect();
            v.extend(x.sketches.iter().map(|s| name_of(features, *s)));
            v
        }
        Role::SmArcs => x.arcs_as_bends.iter().map(|c| curve_label(features, c)).collect(),
        Role::SmFaces => {
            let mut v: Vec<String> = x.faces.iter().map(|f| format!("Face of {}", op_name(features, f.face.op))).collect();
            v.extend(x.regions.iter().map(|r| format!("Face of {}", name_of(features, r.sketch))));
            v.extend(x.region_sketches.iter().map(|s| name_of(features, *s)));
            v
        }
        Role::SmUpTo => x.up_to.iter().map(|u| up_to_label(features, cache, u)).collect(),
        Role::SmSecondUpTo => x.second.iter().filter_map(|s| s.up_to.as_ref()).map(|u| up_to_label(features, cache, u)).collect(),
        _ => return None,
    })
}

/// The list roles and the fields they take picks for.
pub(crate) const LISTS: [(Role, AppliedField); 8] = [
    (Role::SmParts, AppliedField::SmParts),
    (Role::SmExclude, AppliedField::SmExclude),
    (Role::SmBends, AppliedField::SmBends),
    (Role::SmCurves, AppliedField::SmCurves),
    (Role::SmArcs, AppliedField::SmArcs),
    (Role::SmFaces, AppliedField::SmFaces),
    (Role::SmUpTo, AppliedField::SmUpTo),
    (Role::SmSecondUpTo, AppliedField::SmSecondUpTo),
];

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    LISTS.iter().find(|(r, _)| *r == role).map(|(_, f)| *f)
}

fn list(b: &mut ChildSpawner, t: &Theme, name: &str, placeholder: &str, role: Role, items: Vec<String>, active: bool) {
    b.spawn((role, SelectionList::new(name.to_string()).placeholder(placeholder).items(items).active(active).build(t)))
        .entry::<Node>()
        .and_modify(|mut n| {
            n.flex_grow = 0.0;
            n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(3.0), Val::Px(3.0));
        });
}

/// A labelled number field, with an opposite-direction arrow after it if `flip` is given.
fn number(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, num: SmNum, text: &str, flip: Option<(Role, bool)>) {
    // The number field keeps room for a chevron on its left: its label lines up with the
    // checkboxes' boxes when pulled back over it (Depth stays indented under its end type).
    let left = if matches!(num, SmNum::Depth | SmNum::SecondDepth) { -4.0 } else { -16.0 };
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), margin: UiRect::new(Val::Px(left), Val::ZERO, Val::Px(1.0), Val::Px(1.0)), ..default() }).with_children(|r| {
        r.spawn((Role::SmNumber(num), NumberField::new(name.to_string(), label.to_string()).text(text.to_string()).label_width(112.0).build(t)))
            .entry::<Node>()
            .and_modify(|mut n| n.flex_grow = 1.0);
        match flip {
            Some((role, on)) => crate::extrude_dialog::flip_button_any(r, t, &format!("{name}-flip"), role, on, "Opposite direction"),
            // Keep the values in line with the ones that have an arrow.
            None => {
                r.spawn(Node { width: Val::Px(22.0), flex_shrink: 0.0, ..default() });
            }
        }
    });
}

/// A labelled select ("Bend calculation  K Factor ▾").
fn select(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, role: Role, options: &[(String, bool)], selected: usize) {
    let mut s = Select::new(name.to_string());
    for (o, enabled) in options {
        s = s.option(o.clone(), *enabled);
    }
    b.spawn(Node { height: Val::Px(28.0), margin: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(1.0), Val::Px(1.0)), align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() })
        .with_children(|r| {
            if !label.is_empty() {
                r.spawn((t.text(label, 11.0, bevy::text::FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(90.0), flex_shrink: 0.0, ..default() }));
            }
            r.spawn((role, s.selected(selected).build(t))).entry::<Node>().and_modify(|mut n| n.flex_grow = 1.0);
        });
}

fn check(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, on: bool) {
    b.spawn(OptionRow::new(name.to_string(), label.to_string()).checked(on).build(t));
}

fn opts<T: Copy>(all: &[T], label: impl Fn(T) -> &'static str) -> Vec<(String, bool)> {
    all.iter().map(|x| (label(*x).to_string(), true)).collect()
}

fn index_of<T: PartialEq>(all: &[T], x: &T) -> usize {
    all.iter().position(|y| y == x).unwrap_or(0)
}

/// The end type row: the select and the opposite-direction arrow.
fn end_row(b: &mut ChildSpawner, t: &Theme, name: &str, role: Role, end: EndType, flip: Option<(Role, bool)>) {
    let mut s = Select::new(name.to_string());
    for e in EXTRUDE_ENDS {
        s = s.option(e.label(), true);
    }
    b.spawn(Node { height: Val::Px(30.0), margin: UiRect::vertical(Val::Px(2.0)), align_items: AlignItems::Center, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
        r.spawn((role, s.selected(index_of(&EXTRUDE_ENDS, &end)).build(t))).entry::<Node>().and_modify(|mut n| n.flex_grow = 1.0);
        match flip {
            Some((fr, on)) => crate::extrude_dialog::flip_button_any(r, t, &format!("{name}-flip"), fr, on, "Opposite direction"),
            None => {
                r.spawn(Node { width: Val::Px(22.0), flex_shrink: 0.0, ..default() });
            }
        }
    });
}

/// The rows under an end type: its depth, or the entity it goes up to.
#[allow(clippy::too_many_arguments)]
fn end_rows(b: &mut ChildSpawner, t: &Theme, prefix: &str, end: EndType, depth: (SmNum, &str), list_role: Role, items: Vec<String>, active: bool) {
    match end {
        EndType::Blind | EndType::ThroughAll => number(b, t, &format!("{prefix}-depth"), "Depth", depth.0, depth.1, None),
        EndType::UpToNext => {}
        EndType::UpToFace => list(b, t, &format!("{prefix}-up-to-field"), "Up to face", list_role, items, active),
        EndType::UpToPart => list(b, t, &format!("{prefix}-up-to-field"), "Up to part", list_role, items, active),
        EndType::UpToVertex => list(b, t, &format!("{prefix}-up-to-field"), "Up to vertex", list_role, items, active),
    }
}

/// The dialog's body: the tabs and the four sections.
pub(crate) fn body(b: &mut ChildSpawner, t: &Theme, x: &SheetMetalModelFeature, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>, open: [bool; 4]) {
    let mut strip = TabStrip::new("sheet-metal-model-operation").compact();
    for op in SheetMetalOp::ALL {
        strip = strip.tab(op.label());
    }
    b.spawn((Role::SmOpTab, strip.selected(index_of(&SheetMetalOp::ALL, &x.operation)).build(t)));
    let lists: Vec<(Role, Vec<String>)> = LISTS.iter().map(|(r, _)| (*r, items_of(*r))).collect();
    let item = move |r: Role| lists.iter().find(|(x, _)| *x == r).map(|(_, v)| v.clone()).unwrap_or_default();
    let x1 = x.clone();
    let t1 = t.clone();
    let selections = Collapsible::new(SECTIONS[0].0, SECTIONS[0].1)
        .section()
        .open(open[0])
        .error(x.is_empty())
        .content(move |c| {
            let (x, t) = (&x1, &t1);
            match x.operation {
                SheetMetalOp::Convert => {
                    list(c, t, "sm-parts-field", "Parts and surfaces to convert", Role::SmParts, item(Role::SmParts), field == AppliedField::SmParts);
                    list(c, t, "sm-exclude-field", "Faces to exclude", Role::SmExclude, item(Role::SmExclude), field == AppliedField::SmExclude);
                    list(c, t, "sm-bends-field", "Edges or cylinders to bend", Role::SmBends, item(Role::SmBends), field == AppliedField::SmBends);
                    number(c, t, "sm-clearance", "Clearance from input", SmNum::Clearance, &x.clearance_expr, None);
                    check(c, t, "sm-include-bends", "Include bends", x.include_bends);
                    check(c, t, "sm-keep-input", "Keep input parts", x.keep_input);
                }
                SheetMetalOp::Extrude => {
                    list(c, t, "sm-curves-field", "Sketch curves to extrude", Role::SmCurves, item(Role::SmCurves), field == AppliedField::SmCurves);
                    list(c, t, "sm-arcs-field", "Arcs to extrude as bends", Role::SmArcs, item(Role::SmArcs), field == AppliedField::SmArcs);
                    end_row(c, t, "sm-end-type", Role::SmEndType, x.end, Some((Role::SmExtrudeFlip, x.flip_extrude)));
                    end_rows(c, t, "sm", x.end, (SmNum::Depth, &x.depth_expr), Role::SmUpTo, item(Role::SmUpTo), field == AppliedField::SmUpTo);
                    if x.end == EndType::Blind {
                        check(c, t, "sm-symmetric", "Symmetric", x.symmetric);
                    }
                    if !x.symmetric || x.end != EndType::Blind {
                        check(c, t, "sm-second-end", "Second end position", x.second.is_some());
                        if let Some(s) = &x.second {
                            end_row(c, t, "sm-second-end-type", Role::SmSecondEndType, s.end, None);
                            end_rows(c, t, "sm-second", s.end, (SmNum::SecondDepth, &s.depth_expr), Role::SmSecondUpTo, item(Role::SmSecondUpTo), field == AppliedField::SmSecondUpTo);
                        }
                    }
                }
                SheetMetalOp::Thicken => {
                    list(c, t, "sm-faces-field", "Faces or sketch regions to thicken", Role::SmFaces, item(Role::SmFaces), field == AppliedField::SmFaces);
                    check(c, t, "sm-tangent-propagation", "Tangent propagation", x.tangent_propagation);
                    list(c, t, "sm-bends-field", "Edges or cylinders to bend", Role::SmBends, item(Role::SmBends), field == AppliedField::SmBends);
                    number(c, t, "sm-clearance", "Clearance from input", SmNum::Clearance, &x.clearance_expr, None);
                    check(c, t, "sm-include-bends", "Include bends", x.include_bends);
                }
            }
        })
        .build(t);
    let x2 = x.clone();
    let t2 = t.clone();
    let general = Collapsible::new(SECTIONS[1].0, SECTIONS[1].1)
        .section()
        .open(open[1])
        .content(move |c| {
            let (x, t) = (&x2, &t2);
            number(c, t, "sm-thickness", "Thickness", SmNum::Thickness, &x.exprs.thickness, Some((Role::SmThicknessFlip, x.flip_thickness)));
            number(c, t, "sm-bend-radius", "Bend radius", SmNum::BendRadius, &x.exprs.bend_radius, None);
            check(c, t, "sm-flip-direction-up", "Flip direction up", x.params.flip_direction_up);
        })
        .build(t);
    let x3 = x.clone();
    let t3 = t.clone();
    let material = Collapsible::new(SECTIONS[2].0, SECTIONS[2].1)
        .section()
        .open(open[2])
        .content(move |c| {
            let (x, t) = (&x3, &t3);
            let p = &x.params;
            select(c, t, "sm-bend-calculation", "Bend calculation", Role::SmBendCalc, &opts(&BendCalc::ALL, BendCalc::label), index_of(&BendCalc::ALL, &p.bend_calc));
            match p.bend_calc {
                BendCalc::KFactor => number(c, t, "sm-k-factor", "Default bend K Factor", SmNum::KFactor, &x.exprs.k_factor, None),
                BendCalc::BendAllowance => number(c, t, "sm-bend-allowance", "Default bend allowance", SmNum::Allowance, &x.exprs.bend_allowance, None),
                BendCalc::BendDeduction => number(c, t, "sm-bend-deduction", "Default bend deduction", SmNum::Deduction, &x.exprs.bend_deduction, None),
            }
            number(c, t, "sm-rolled-k-factor", "Rolled K Factor", SmNum::RolledK, &x.exprs.rolled_k_factor, None);
        })
        .build(t);
    let x4 = x.clone();
    let t4 = t.clone();
    let relief = Collapsible::new(SECTIONS[3].0, SECTIONS[3].1)
        .section()
        .open(open[3])
        .content(move |c| {
            let (x, t) = (&x4, &t4);
            let p = &x.params;
            number(c, t, "sm-minimal-gap", "Minimal gap", SmNum::MinimalGap, &x.exprs.minimal_gap, None);
            select(c, t, "sm-corner-relief-type", "Corner relief type", Role::SmCornerType, &opts(&CornerReliefKind::ALL, CornerReliefKind::label), index_of(&CornerReliefKind::ALL, &p.corner_relief.kind));
            match p.corner_relief.kind {
                k if k.is_scaled() => number(c, t, "sm-corner-relief-scale", "Corner relief scale", SmNum::CornerScale, &x.exprs.corner_relief_scale, None),
                CornerReliefKind::SquareSized => number(c, t, "sm-corner-relief-size", "Corner relief width", SmNum::CornerSize, &x.exprs.corner_relief_size, None),
                CornerReliefKind::RoundSized => number(c, t, "sm-corner-relief-size", "Corner relief diameter", SmNum::CornerSize, &x.exprs.corner_relief_size, None),
                _ => {}
            }
            select(c, t, "sm-bend-relief-type", "Bend relief type", Role::SmBendReliefType, &opts(&BendReliefKind::MODEL, BendReliefKind::label), index_of(&BendReliefKind::MODEL, &p.bend_relief.kind));
            if p.bend_relief.kind.is_scaled() {
                number(c, t, "sm-bend-relief-depth-scale", "Bend relief depth scale", SmNum::BendDepthScale, &x.exprs.bend_relief_depth_scale, None);
                number(c, t, "sm-bend-relief-width-scale", "Bend relief width scale", SmNum::BendWidthScale, &x.exprs.bend_relief_width_scale, None);
            }
        })
        .build(t);
    b.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(2.0), Val::Px(4.0), Val::Px(4.0), Val::ZERO), ..default() })
        .with_children(|s| {
            s.spawn(selections);
            s.spawn(general);
            s.spawn(material);
            s.spawn(relief);
        });
}

/// A section opened or closed: remembered while the dialog is open (it is rebuilt as its rows
/// change).
pub fn on_section_toggled(ev: On<CollapsibleToggled>, q: Query<&Name>, mut session: Option<ResMut<AppliedSession>>) {
    let Ok(name) = q.get(ev.entity) else { return };
    let Some(i) = SECTIONS.iter().position(|(n, _)| *n == name.as_str()) else { return };
    if let Some(s) = session.as_mut()
        && s.sections[i] != ev.open
    {
        s.sections[i] = ev.open;
    }
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

/// The selects' current choices.
pub(crate) fn select_index(x: &SheetMetalModelFeature, role: Role) -> Option<usize> {
    let p = &x.params;
    Some(match role {
        Role::SmEndType => index_of(&EXTRUDE_ENDS, &x.end),
        Role::SmSecondEndType => index_of(&EXTRUDE_ENDS, &x.second.as_ref().map_or(EndType::Blind, |s| s.end)),
        Role::SmBendCalc => index_of(&BendCalc::ALL, &p.bend_calc),
        Role::SmCornerType => index_of(&CornerReliefKind::ALL, &p.corner_relief.kind),
        Role::SmBendReliefType => index_of(&BendReliefKind::MODEL, &p.bend_relief.kind),
        _ => return None,
    })
}

pub(crate) fn owns_select(role: Role) -> bool {
    matches!(role, Role::SmEndType | Role::SmSecondEndType | Role::SmBendCalc | Role::SmCornerType | Role::SmBendReliefType)
}

/// A select changed: the label of the change and the field that takes the next picks.
pub(crate) fn set_select(x: &mut SheetMetalModelFeature, role: Role, i: usize) -> (&'static str, Option<AppliedField>) {
    let p = &mut x.params;
    match role {
        Role::SmEndType => {
            let e = EXTRUDE_ENDS[i.min(EXTRUDE_ENDS.len() - 1)];
            if e != x.end {
                x.end = e;
                x.up_to = None;
            }
            ("End type", e.needs_target().then_some(AppliedField::SmUpTo))
        }
        Role::SmSecondEndType => {
            let e = EXTRUDE_ENDS[i.min(EXTRUDE_ENDS.len() - 1)];
            if let Some(s) = x.second.as_mut()
                && s.end != e
            {
                s.end = e;
                s.up_to = None;
            }
            ("Second end type", e.needs_target().then_some(AppliedField::SmSecondUpTo))
        }
        Role::SmBendCalc => {
            p.bend_calc = BendCalc::ALL[i.min(2)];
            ("Bend calculation", None)
        }
        Role::SmCornerType => {
            p.corner_relief.kind = CornerReliefKind::ALL[i.min(5)];
            ("Corner relief type", None)
        }
        Role::SmBendReliefType => {
            p.bend_relief.kind = BendReliefKind::MODEL[i.min(2)];
            ("Bend relief type", None)
        }
        _ => ("", None),
    }
}

pub fn is_checkbox(name: &str) -> bool {
    name.starts_with("sm-") && name.ends_with("-checkbox")
}

/// A checkbox changed: the label of the change and the field that takes the next picks.
pub fn checkbox(x: &mut SheetMetalModelFeature, name: &str, on: bool) -> Option<(&'static str, Option<AppliedField>)> {
    Some(match name {
        "sm-include-bends-checkbox" => {
            x.include_bends = on;
            ("Include bends", None)
        }
        "sm-keep-input-checkbox" => {
            x.keep_input = on;
            ("Keep input parts", None)
        }
        "sm-symmetric-checkbox" => {
            x.symmetric = on;
            if on {
                x.second = None;
            }
            ("Symmetric", None)
        }
        "sm-second-end-checkbox" => {
            x.second = on.then(|| EndCondition { depth: x.depth, depth_expr: x.depth_expr.clone(), ..Default::default() });
            ("Second end position", None)
        }
        "sm-tangent-propagation-checkbox" => {
            x.tangent_propagation = on;
            ("Tangent propagation", None)
        }
        "sm-flip-direction-up-checkbox" => {
            x.params.flip_direction_up = on;
            ("Flip direction up", None)
        }
        _ => return None,
    })
}

/// The opposite direction arrows.
pub(crate) fn flip(x: &mut SheetMetalModelFeature, role: Role) -> Option<&'static str> {
    match role {
        Role::SmExtrudeFlip => {
            x.flip_extrude = !x.flip_extrude;
            Some("Opposite direction")
        }
        Role::SmThicknessFlip => {
            x.flip_thickness = !x.flip_thickness;
            Some("Flip thickness direction")
        }
        _ => None,
    }
}

/// An item's ✕.
pub(crate) fn remove(x: &mut SheetMetalModelFeature, role: Role, i: usize) {
    fn at<T>(v: &mut Vec<T>, i: usize) {
        if i < v.len() {
            v.remove(i);
        }
    }
    match role {
        Role::SmParts => at(&mut x.parts, i),
        Role::SmExclude => at(&mut x.exclude, i),
        Role::SmBends => at(&mut x.bends, i),
        Role::SmCurves => {
            if i < x.curves.len() {
                x.curves.remove(i);
            } else {
                at(&mut x.sketches, i - x.curves.len());
            }
        }
        Role::SmArcs => at(&mut x.arcs_as_bends, i),
        Role::SmFaces => {
            let (a, b) = (x.faces.len(), x.regions.len());
            if i < a {
                x.faces.remove(i);
            } else if i < a + b {
                x.regions.remove(i - a);
            } else {
                at(&mut x.region_sketches, i - a - b);
            }
        }
        Role::SmUpTo => x.up_to = None,
        Role::SmSecondUpTo => {
            if let Some(s) = x.second.as_mut() {
                s.up_to = None;
            }
        }
        _ => {}
    }
}

/// A number's text and its quantity.
fn text_of(x: &SheetMetalModelFeature, n: SmNum) -> (String, Quantity) {
    let e = &x.exprs;
    let (s, q) = match n {
        SmNum::Clearance => (&x.clearance_expr, Quantity::Length),
        SmNum::Depth => (&x.depth_expr, Quantity::Length),
        SmNum::SecondDepth => return (x.second.as_ref().map_or(String::new(), |s| s.depth_expr.clone()), Quantity::Length),
        SmNum::Thickness => (&e.thickness, Quantity::Length),
        SmNum::BendRadius => (&e.bend_radius, Quantity::Length),
        SmNum::KFactor => (&e.k_factor, Quantity::Count),
        SmNum::RolledK => (&e.rolled_k_factor, Quantity::Count),
        SmNum::Allowance => (&e.bend_allowance, Quantity::Length),
        SmNum::Deduction => (&e.bend_deduction, Quantity::Length),
        SmNum::MinimalGap => (&e.minimal_gap, Quantity::Length),
        SmNum::CornerScale => (&e.corner_relief_scale, Quantity::Count),
        SmNum::CornerSize => (&e.corner_relief_size, Quantity::Length),
        SmNum::BendDepthScale => (&e.bend_relief_depth_scale, Quantity::Count),
        SmNum::BendWidthScale => (&e.bend_relief_width_scale, Quantity::Count),
    };
    (s.clone(), q)
}

/// Sets a number (value and expression).
pub fn set_number(x: &mut SheetMetalModelFeature, n: SmNum, v: f64, expr: String) {
    let p = &mut x.params;
    let e = &mut x.exprs;
    let (value, text) = match n {
        SmNum::Clearance => (&mut x.clearance, &mut x.clearance_expr),
        SmNum::Depth => (&mut x.depth, &mut x.depth_expr),
        SmNum::SecondDepth => match x.second.as_mut() {
            Some(s) => (&mut s.depth, &mut s.depth_expr),
            None => return,
        },
        SmNum::Thickness => (&mut p.thickness, &mut e.thickness),
        SmNum::BendRadius => (&mut p.bend_radius, &mut e.bend_radius),
        SmNum::KFactor => (&mut p.k_factor, &mut e.k_factor),
        SmNum::RolledK => (&mut p.rolled_k_factor, &mut e.rolled_k_factor),
        SmNum::Allowance => (&mut p.bend_allowance, &mut e.bend_allowance),
        SmNum::Deduction => (&mut p.bend_deduction, &mut e.bend_deduction),
        SmNum::MinimalGap => (&mut p.minimal_gap, &mut e.minimal_gap),
        SmNum::CornerScale => (&mut p.corner_relief.scale, &mut e.corner_relief_scale),
        SmNum::CornerSize => (&mut p.corner_relief.size, &mut e.corner_relief_size),
        SmNum::BendDepthScale => (&mut p.bend_relief.depth_scale, &mut e.bend_relief_depth_scale),
        SmNum::BendWidthScale => (&mut p.bend_relief.width_scale, &mut e.bend_relief_width_scale),
    };
    *value = v;
    *text = expr;
}

pub fn number_label(n: SmNum) -> &'static str {
    match n {
        SmNum::Clearance => "Clearance from input",
        SmNum::Depth => "Depth",
        SmNum::SecondDepth => "Second depth",
        SmNum::Thickness => "Thickness",
        SmNum::BendRadius => "Bend radius",
        SmNum::KFactor => "Default bend K Factor",
        SmNum::RolledK => "Rolled K Factor",
        SmNum::Allowance => "Bend allowance",
        SmNum::Deduction => "Bend deduction",
        SmNum::MinimalGap => "Minimal gap",
        SmNum::CornerScale => "Corner relief scale",
        SmNum::CornerSize => "Corner relief size",
        SmNum::BendDepthScale => "Bend relief depth scale",
        SmNum::BendWidthScale => "Bend relief width scale",
    }
}

/// Why a number is out of range (its field is red, this its tooltip), if it is.
pub fn range_error(x: &SheetMetalModelFeature, n: SmNum) -> Option<String> {
    match n {
        // (NaN counts as out of range too.)
        SmNum::Clearance => (x.clearance.is_nan() || x.clearance < 0.0).then(|| "Clearance from input must be at least 0".into()),
        SmNum::Depth => (x.end == EndType::Blind && (x.depth.is_nan() || x.depth <= 0.0)).then(|| "Depth must be greater than 0".into()),
        SmNum::SecondDepth => x
            .second
            .as_ref()
            .filter(|s| s.end == EndType::Blind && (s.depth.is_nan() || s.depth <= 0.0))
            .map(|_| "Depth must be greater than 0".into()),
        n => {
            let label = number_label(n);
            x.params.validate().into_iter().find(|e| e.field == label).map(|e| e.message())
        }
    }
}

/// Parses a typed number: its value (in mm for lengths) and the expression to keep.
pub fn parse(text: &str, q: Quantity, units: &cadrs_sketch::units::Units, vars: &crate::variables_ui::ActiveVariables) -> Option<(f64, String)> {
    let text = text.trim();
    let v = vars.eval(units, text, q).ok().filter(|v| v.is_finite())?;
    let expr = if text.parse::<f64>().is_ok() {
        match q {
            Quantity::Length => units.with_unit(v, q),
            _ => plain(v),
        }
    } else {
        text.to_string()
    };
    Some((v, expr))
}

/// The quantity of a number.
pub fn quantity(n: SmNum) -> Quantity {
    let dummy = SheetMetalModelFeature::default();
    text_of(&dummy, n).1
}

/// Keeps the numbers' text, red state and range tooltips in step with the feature.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_numbers(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    focus: Res<InputFocus>,
    q_edit: Query<&cadrs_ui::NumberFieldEdit>,
    mut q: Query<(Entity, &Role, &mut NumberFieldState, Option<&Tooltip>)>,
    mut commands: Commands,
) {
    let Some(s) = session else { return };
    let Some(FeatureKind::SheetMetalModel(x)) = doc.as_ref().and_then(|d| d.doc.element(s.element)?.feature(s.feature)).map(|f| &f.kind) else {
        return;
    };
    let editing = focus.get().and_then(|f| q_edit.get(f).ok()).map(|e| e.0);
    for (entity, role, mut st, tip) in &mut q {
        let Role::SmNumber(n) = *role else { continue };
        let err = range_error(x, n);
        if editing != Some(entity) {
            let (text, _) = text_of(x, n);
            // A typed value that didn't parse stays as typed, red, until the next edit.
            let unparsed = st.error && err.is_none() && st.text != text;
            if !unparsed {
                let want = NumberFieldState { text, error: err.is_some() };
                if *st != want {
                    *st = want;
                }
            }
        }
        let want_tip = err.clone().or_else(|| (st.error).then(|| "Not a valid value".to_string()));
        match (want_tip, tip) {
            (Some(m), Some(t)) if t.text == m => {}
            (Some(m), _) => {
                commands.entity(entity).insert(Tooltip::error(m));
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<Tooltip>();
            }
            (None, None) => {}
        }
    }
}

/// A number typed: parsed and set (out-of-range values too: the field shows why).
pub fn commit_number(world: &mut World, entity: Entity, n: SmNum, text: String, enter: bool) {
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let parsed = parse(&text, quantity(n), &units, world.resource::<crate::variables_ui::ActiveVariables>());
    if let Some(mut st) = world.get_mut::<NumberFieldState>(entity) {
        st.error = parsed.is_none();
        if parsed.is_none() {
            st.text = text.clone();
        }
    }
    let Some((v, expr)) = parsed else { return };
    crate::applied::change_kind(world, number_label(n), |k| {
        if let FeatureKind::SheetMetalModel(x) = k {
            set_number(x, n, v, expr);
        }
    });
    if enter {
        world.resource_mut::<InputFocus>().clear();
        crate::applied::accept(world);
    }
}

// ---------------------------------------------------------------------------------------------
// The extrude's depth arrow

/// Where the Extrude's depth arrow stands: on the first picked curve's middle, at the depth,
/// pointing along the extrude.
pub fn depth_arrow(features: &[Feature], x: &SheetMetalModelFeature, depth: f64) -> Option<(Vec3, Vec3)> {
    if x.operation != SheetMetalOp::Extrude || x.end != EndType::Blind {
        return None;
    }
    let (sketch, curve) = match (x.curves.first(), x.sketches.first()) {
        (Some(c), _) => (c.sketch, Some(c.curve)),
        (None, Some(s)) => (*s, None),
        _ => return None,
    };
    let sk = features.iter().find(|f| f.id == sketch)?.sketch()?;
    let frame = sk.plane?.frame();
    let g = &sk.geometry;
    let id = curve.or_else(|| g.curves.iter().find(|(_, c)| !c.construction).map(|(id, _)| id))?;
    let mid = match g.curves.get(id)?.kind {
        cadrs_sketch::CurveKind::Line { a, b } => {
            let (a, b) = (g.pos(a), g.pos(b));
            cadrs_sketch::Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
        }
        cadrs_sketch::CurveKind::Arc { .. } => {
            let a = g.arc_geom(id)?;
            let m = a.start_angle + a.sweep / 2.0;
            cadrs_sketch::Vec2::new(a.center.x + a.radius * m.cos(), a.center.y + a.radius * m.sin())
        }
        _ => return None,
    };
    let p = frame.to_world(mid);
    let n = frame.normal();
    let s = if x.flip_extrude { -1.0 } else { 1.0 };
    let dir = Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32).normalize_or_zero() * s;
    let base = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let along = if x.symmetric { depth / 2.0 } else { depth };
    Some((base + dir * along as f32, dir))
}
