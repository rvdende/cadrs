//! The surfacing features in the applied-feature dialogs (`reference/onshape/surfacing.md`),
//! reached through the end of [`crate::transform_ui`]'s hand-on chain:
//!
//! - **Thicken**: New | Add | Remove | Intersect; *Faces and surfaces to thicken* (part faces,
//!   sketch regions, whole sketches); **Mid plane**; *Thickness 1* with the opposite direction
//!   arrow, *Thickness 2* (or the one *Thickness*); **Keep tools**; merge.
//! - **Helix**: the helix type (Cylinder/Cone, Axis, Circle); the face, axis or circle; the input
//!   type (Turns, Pitch, Turns and pitch) with *Revolutions* and *Pitch*; *Height* (Axis and
//!   Circle) and *Radius* (Axis); *Start angle*; *Clockwise* / *Counterclockwise*; the opposite
//!   direction arrow.
//! - **Fill**: New | Add; *Edges and curves* of the boundary; the continuity (Position only
//!   is built); merge scope for Add.
//!
//! The toolbar's Thicken button (▾: Thicken, Fill, Helix) starts them with the selection.

use bevy::prelude::*;
use cadrs_core::document::{AxisRef, BooleanOp, FaceRef, RegionRef};
use cadrs_core::surfacing::{Continuity, FillEdge, FillFeature, HelixFeature, HelixPath, HelixType, ThickenFeature};
use cadrs_core::{Feature, FeatureId, FeatureKind};
use cadrs_ui::prelude::*;
use cadrs_ui::{OptionRow, TabStrip};

use crate::applied::{AppliedField, AppliedKind};
use crate::applied_dialog::{Role, body_column, index_of, list, number, opts, select_row};
use crate::parts::PartCache;
use crate::viewport::Pick;
use crate::ActiveDocument;

fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    if let Some(i) = list.iter().position(|y| *y == x) {
        list.remove(i);
    } else {
        list.push(x);
    }
}

fn is_sketch(features: &[Feature], f: FeatureId) -> bool {
    features.iter().any(|x| x.id == f && x.sketch().is_some())
}

fn region_of(cache: &PartCache, sketch: FeatureId, index: u32) -> Option<RegionRef> {
    let r = cache.sketch_regions(sketch)?.regions.get(index as usize)?.clone();
    Some(RegionRef::new(sketch, &r))
}

fn face_ref(cache: &PartCache, pick: Pick) -> Option<FaceRef> {
    match crate::applied::entity_of(cache, pick)? {
        cadrs_core::applied::EdgeOrFace::Face(f) => Some(f),
        _ => None,
    }
}

fn edge_ref(cache: &PartCache, pick: Pick) -> Option<cadrs_core::EdgeRef> {
    match crate::applied::entity_of(cache, pick)? {
        cadrs_core::applied::EdgeOrFace::Edge(e) => Some(e),
        _ => None,
    }
}

/// A new feature's base name, parameters and first field, from the selection.
pub fn initial(world: &World, kind: AppliedKind, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let features = world.get_resource::<ActiveDocument>()?.active_element()?.features().to_vec();
    let cache = world.resource::<PartCache>();
    Some(match kind {
        AppliedKind::Thicken => {
            let mut x = ThickenFeature::default();
            for p in picked {
                match *p {
                    Pick::Face(..) => x.faces.extend(face_ref(cache, *p)),
                    Pick::Region(s, i) => x.regions.extend(region_of(cache, s, i)),
                    Pick::Feature(f) if is_sketch(&features, f) => x.sketches.push(f),
                    Pick::Part(part) if cache.part(part).is_some_and(|p| p.kind == cadrs_core::PartKind::Surface) => x.parts.push(part),
                    _ => {}
                }
            }
            ("Thicken", FeatureKind::Thicken(x), AppliedField::ThickenEntities)
        }
        AppliedKind::Helix => {
            let mut x = HelixFeature::default();
            if let Some(f) = picked.iter().find_map(|p| face_ref(cache, *p)) {
                x.face = Some(f);
            }
            ("Helix", FeatureKind::Helix(x), AppliedField::HelixEntity)
        }
        AppliedKind::Fill => {
            let mut x = FillFeature::default();
            for p in picked {
                match *p {
                    Pick::Edge(..) => x.edges.extend(edge_ref(cache, *p).map(FillEdge::Edge)),
                    Pick::SketchCurve(sketch, curve) => x.edges.push(FillEdge::SketchCurve { sketch, curve }),
                    _ => {}
                }
            }
            x.continuity = vec![Continuity::Position; x.edges.len()];
            ("Fill", FeatureKind::Fill(x), AppliedField::FillEdges)
        }
        _ => return None,
    })
}

/// A pick into the surfacing features' fields. Returns false if it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let Some(features) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.features().to_vec()) else {
        return false;
    };
    let cache = world.resource::<PartCache>();
    match (kind, field) {
        (FeatureKind::Thicken(x), AppliedField::ThickenEntities) => match pick {
            Pick::Region(s, i) => {
                let Some(r) = region_of(cache, s, i) else { return false };
                match x.regions.iter().position(|y| y.sketch == r.sketch && y.curves == r.curves) {
                    Some(k) => {
                        x.regions.remove(k);
                    }
                    None => x.regions.push(r),
                }
            }
            Pick::Feature(f) if is_sketch(&features, f) => toggle(&mut x.sketches, f),
            Pick::Face(..) => {
                let Some(f) = face_ref(cache, pick) else { return false };
                match x.faces.iter().position(|g| g.face == f.face) {
                    Some(i) => {
                        x.faces.remove(i);
                    }
                    None => x.faces.push(f),
                }
            }
            Pick::Part(p) => toggle(&mut x.parts, p),
            _ => return false,
        },
        (FeatureKind::Thicken(x), AppliedField::MergeScope) => match pick.part() {
            Some(p) => toggle(&mut x.merge_scope, p),
            None => return false,
        },
        (FeatureKind::Helix(x), AppliedField::HelixEntity) => match x.helix_type {
            HelixType::CylinderCone => {
                let Some(f) = face_ref(cache, pick) else { return false };
                x.face = if x.face.is_some_and(|g| g.face == f.face) { None } else { Some(f) };
            }
            HelixType::Axis | HelixType::Circle => {
                let Some(a) = crate::pattern::axis_of(&features, cache, pick) else { return false };
                x.axis = if x.axis == Some(a) { None } else { Some(a) };
            }
        },
        (FeatureKind::Fill(x), AppliedField::FillEdges) => {
            let e = match pick {
                Pick::Edge(..) => edge_ref(cache, pick).map(FillEdge::Edge),
                Pick::SketchCurve(sketch, curve) => Some(FillEdge::SketchCurve { sketch, curve }),
                _ => None,
            };
            let Some(e) = e else { return false };
            let same = |a: &FillEdge| match (a, &e) {
                (FillEdge::Edge(a), FillEdge::Edge(b)) => a.edge == b.edge,
                (a, b) => a == b,
            };
            match x.edges.iter().position(same) {
                Some(i) => {
                    x.edges.remove(i);
                    if i < x.continuity.len() {
                        x.continuity.remove(i);
                    }
                }
                None => {
                    x.edges.push(e);
                    x.continuity.resize(x.edges.len(), Continuity::Position);
                }
            }
        }
        (FeatureKind::Fill(x), AppliedField::MergeScope) => match pick.part() {
            Some(p) => toggle(&mut x.merge_scope, p),
            None => return false,
        },
        _ => return false,
    }
    true
}

/// What their fields refer to, shown selected in the view while the dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let face = |f: &FaceRef| cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()).map(|p| Pick::Face(p.id, f.face));
    let edge = |e: &cadrs_core::EdgeRef| cache.parts.iter().find(|p| p.solid.edge(&e.edge).is_some()).map(|p| Pick::Edge(p.id, e.edge));
    let axis = |a: &AxisRef| match a {
        AxisRef::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
        AxisRef::Edge(e) => edge(e),
        AxisRef::Face(f) => face(f),
        AxisRef::Connector(c) => crate::pattern::connector_pick(c),
    };
    match kind {
        FeatureKind::Thicken(x) => {
            let mut v: Vec<Pick> = x.faces.iter().filter_map(face).collect();
            v.extend(x.parts.iter().map(|p| Pick::Part(*p)));
            v
        }
        FeatureKind::Helix(x) => x.face.iter().filter_map(face).chain(x.axis.iter().filter_map(axis)).collect(),
        FeatureKind::Fill(x) => x
            .edges
            .iter()
            .filter_map(|e| match e {
                FillEdge::Edge(r) => edge(r),
                FillEdge::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
            })
            .collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog

pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    Some(match kind {
        FeatureKind::Thicken(_) => "thicken",
        FeatureKind::Helix(_) => "helix",
        FeatureKind::Fill(_) => "fill",
        // P3I.6: the flat pattern extrude ends the chain.
        k => return crate::flat_ui::name(k),
    })
}

pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    Some(match kind {
        FeatureKind::Thicken(x) => format!("thicken {:?} {} {} {} {}", x.op, x.mid_plane, x.flip, x.keep_tools, x.merge_all),
        FeatureKind::Helix(x) => format!("helix {:?} {:?} {} {}", x.helix_type, x.path, x.clockwise, x.flip),
        FeatureKind::Fill(x) => format!("fill {} {:?}", x.add, x.continuity.first()),
        k => return crate::flat_ui::layout(k),
    })
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn name_of(features: &[Feature], id: FeatureId) -> String {
    features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone())
}

pub(crate) fn items(features: &[Feature], cache: &PartCache, kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    Some(match (kind, role) {
        (FeatureKind::Thicken(x), Role::ThickenEntities) => {
            let mut v: Vec<String> = x.faces.iter().map(|f| format!("Face of {}", op_name(features, f.face.op))).collect();
            v.extend(x.regions.iter().map(|r| format!("Face of {}", name_of(features, r.sketch))));
            v.extend(x.sketches.iter().map(|s| name_of(features, *s)));
            v.extend(crate::applied::part_names(cache, &x.parts));
            v
        }
        (FeatureKind::Thicken(x), Role::MergeScope) => crate::applied::part_names(cache, &x.merge_scope),
        (FeatureKind::Helix(x), Role::HelixEntity) => {
            let mut v: Vec<String> = x.face.iter().map(|f| format!("Face of {}", op_name(features, f.face.op))).collect();
            v.extend(x.axis.iter().map(|a| crate::revolve::axis_label(features, a)));
            v
        }
        (FeatureKind::Fill(x), Role::FillEdges) => x
            .edges
            .iter()
            .map(|e| match e {
                FillEdge::Edge(r) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &r.edge))),
                FillEdge::SketchCurve { sketch, .. } => format!("Curve of {}", name_of(features, *sketch)),
            })
            .collect(),
        (FeatureKind::Fill(x), Role::MergeScope) => crate::applied::part_names(cache, &x.merge_scope),
        (k, r) => return crate::flat_ui::items(features, k, r),
    })
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    Some(match role {
        Role::ThickenEntities => AppliedField::ThickenEntities,
        Role::HelixEntity => AppliedField::HelixEntity,
        Role::FillEdges => AppliedField::FillEdges,
        r => return crate::flat_ui::list_field(r),
    })
}

fn op_tabs(b: &mut ChildSpawner, t: &Theme, name: &str, labels: &[&str], selected: usize) {
    let mut strip = TabStrip::new(format!("{name}-operation")).compact();
    for l in labels {
        strip = strip.tab(*l);
    }
    b.spawn((Role::OpTab, strip.selected(selected).build(t)));
}

pub(crate) fn body(b: &mut ChildSpawner, t: &Theme, kind: &FeatureKind, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>) {
    match kind {
        FeatureKind::Thicken(x) => {
            let labels: Vec<&str> = BooleanOp::ALL.iter().map(|o| o.label()).collect();
            op_tabs(b, t, "thicken", &labels, index_of(&BooleanOp::ALL, &x.op));
            body_column(b, |b| {
                list(
                    b,
                    t,
                    "thicken-entities-field",
                    "Faces and surfaces to thicken",
                    Role::ThickenEntities,
                    items_of(Role::ThickenEntities),
                    field == AppliedField::ThickenEntities,
                );
                b.spawn(OptionRow::new("thicken-mid-plane", "Mid plane").checked(x.mid_plane).build(t));
                if x.mid_plane {
                    number(b, t, "thicken-thickness", "Thickness", Role::ThickenThickness1, &x.thickness1_expr, None);
                } else {
                    number(b, t, "thicken-thickness1", "Thickness 1", Role::ThickenThickness1, &x.thickness1_expr, Some(x.flip));
                    number(b, t, "thicken-thickness2", "Thickness 2", Role::ThickenThickness2, &x.thickness2_expr, None);
                }
                b.spawn(OptionRow::new("thicken-keep-tools", "Keep tools").checked(x.keep_tools).build(t));
                if x.op != BooleanOp::New {
                    b.spawn(OptionRow::new("thicken-merge-all", "Merge with all").checked(x.merge_all).build(t));
                    if !x.merge_all {
                        list(b, t, "thicken-merge-scope-field", "Merge scope", Role::MergeScope, items_of(Role::MergeScope), field == AppliedField::MergeScope);
                    }
                }
            });
        }
        FeatureKind::Helix(x) => body_column(b, |b| {
            let types = opts(&HelixType::ALL, |m| m.label(), |_| true);
            select_row(b, t, "helix-type", "", Role::HelixType, &types, index_of(&HelixType::ALL, &x.helix_type), None);
            let (placeholder, flip) = match x.helix_type {
                HelixType::CylinderCone => ("Cylindrical or conical face", x.flip),
                HelixType::Axis => ("Axis", x.flip),
                HelixType::Circle => ("Circle", x.flip),
            };
            b.spawn(Node { align_items: AlignItems::FlexStart, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
                r.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() })
                    .with_children(|c| list(c, t, "helix-entity-field", placeholder, Role::HelixEntity, items_of(Role::HelixEntity), field == AppliedField::HelixEntity));
                r.spawn(Node { margin: UiRect::top(Val::Px(4.0)), ..default() }).with_children(|c| {
                    crate::extrude_dialog::flip_button_any(c, t, "helix-entity-field-flip", Role::Flip, flip, "Opposite direction");
                });
            });
            let paths = opts(&HelixPath::ALL, |m| m.label(), |_| true);
            select_row(b, t, "helix-input", "Input type", Role::HelixPathType, &paths, index_of(&HelixPath::ALL, &x.path), None);
            if matches!(x.path, HelixPath::Turns | HelixPath::TurnsAndPitch) {
                number(b, t, "helix-revolutions", "Revolutions", Role::HelixRevolutions, &x.revolutions_expr, None);
            }
            if matches!(x.path, HelixPath::Pitch | HelixPath::TurnsAndPitch) {
                number(b, t, "helix-pitch", "Pitch", Role::HelixPitch, &x.pitch_expr, None);
            }
            if x.helix_type != HelixType::CylinderCone && x.path != HelixPath::TurnsAndPitch {
                number(b, t, "helix-height", "Height", Role::HelixHeight, &x.height_expr, None);
            }
            if x.helix_type == HelixType::Axis {
                number(b, t, "helix-radius", "Radius", Role::HelixRadius, &x.radius_expr, None);
            }
            number(b, t, "helix-start-angle", "Start angle", Role::HelixStartAngle, &x.start_angle_expr, None);
            let hands = [("Clockwise".to_string(), true), ("Counterclockwise".to_string(), true)];
            select_row(b, t, "helix-direction", "Direction", Role::HelixHandedness, &hands, usize::from(!x.clockwise), None);
        }),
        FeatureKind::Fill(x) => {
            op_tabs(b, t, "fill", &["New", "Add"], usize::from(x.add));
            body_column(b, |b| {
                list(b, t, "fill-edges-field", "Edges and curves", Role::FillEdges, items_of(Role::FillEdges), field == AppliedField::FillEdges);
                let c = opts(&Continuity::ALL, |m| m.label(), |m| m == Continuity::Position);
                let now = x.continuity.first().copied().unwrap_or_default();
                select_row(b, t, "fill-continuity", "Continuity", Role::FillContinuity, &c, index_of(&Continuity::ALL, &now), None);
                if x.add {
                    list(b, t, "fill-merge-scope-field", "Merge scope (surfaces it meets)", Role::MergeScope, items_of(Role::MergeScope), field == AppliedField::MergeScope);
                }
            });
        }
        k => crate::flat_ui::body(b, t, k, field, items_of),
    }
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    Some(match (kind, role) {
        (FeatureKind::Thicken(x), Role::ThickenThickness1) => x.thickness1_expr.clone(),
        (FeatureKind::Thicken(x), Role::ThickenThickness2) => x.thickness2_expr.clone(),
        (FeatureKind::Helix(x), Role::HelixRevolutions) => x.revolutions_expr.clone(),
        (FeatureKind::Helix(x), Role::HelixPitch) => x.pitch_expr.clone(),
        (FeatureKind::Helix(x), Role::HelixHeight) => x.height_expr.clone(),
        (FeatureKind::Helix(x), Role::HelixRadius) => x.radius_expr.clone(),
        (FeatureKind::Helix(x), Role::HelixStartAngle) => x.start_angle_expr.clone(),
        _ => return None,
    })
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    Some(match (kind, role) {
        (FeatureKind::Helix(x), Role::HelixType) => index_of(&HelixType::ALL, &x.helix_type),
        (FeatureKind::Helix(x), Role::HelixPathType) => index_of(&HelixPath::ALL, &x.path),
        (FeatureKind::Helix(x), Role::HelixHandedness) => usize::from(!x.clockwise),
        (FeatureKind::Fill(x), Role::FillContinuity) => index_of(&Continuity::ALL, &x.continuity.first().copied().unwrap_or_default()),
        _ => return None,
    })
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::Helix(x), Role::HelixType) => {
            let t = HelixType::ALL[i.min(HelixType::ALL.len() - 1)];
            if t != x.helix_type {
                x.helix_type = t;
                x.face = None;
                x.axis = None;
            }
        }
        (FeatureKind::Helix(x), Role::HelixPathType) => x.path = HelixPath::ALL[i.min(HelixPath::ALL.len() - 1)],
        (FeatureKind::Helix(x), Role::HelixHandedness) => x.clockwise = i == 0,
        (FeatureKind::Fill(x), Role::FillContinuity) => {
            let c = Continuity::ALL[i.min(2)];
            x.continuity = vec![c; x.edges.len().max(1)];
        }
        _ => {}
    }
}

/// The OpTab strip.
pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    if role != Role::OpTab {
        return;
    }
    match k {
        FeatureKind::Thicken(x) => x.op = BooleanOp::ALL[i.min(3)],
        FeatureKind::Fill(x) => x.add = i == 1,
        k => crate::flat_ui::tab(k, role, i),
    }
}

pub(crate) fn is_checkbox(name: &str) -> bool {
    matches!(name, "thicken-mid-plane-checkbox" | "thicken-keep-tools-checkbox" | "thicken-merge-all-checkbox")
}

pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    let FeatureKind::Thicken(x) = k else { return None };
    Some(match name {
        "thicken-mid-plane-checkbox" => {
            x.mid_plane = on;
            "Mid plane"
        }
        "thicken-keep-tools-checkbox" => {
            x.keep_tools = on;
            "Keep tools"
        }
        "thicken-merge-all-checkbox" => {
            x.merge_all = on;
            "Merge with all"
        }
        _ => return None,
    })
}

/// Numbers that may be zero or of any sign (the second thickness may be 0; the start angle).
pub(crate) fn is_signed(role: Role) -> bool {
    matches!(role, Role::ThickenThickness2 | Role::HelixStartAngle)
}

/// The revolutions: a plain number greater than zero.
pub(crate) fn is_plain(role: Role) -> bool {
    role == Role::HelixRevolutions
}

pub(crate) fn is_angle(role: Role) -> bool {
    role == Role::HelixStartAngle
}

pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::ThickenThickness1 => "Thickness 1",
        Role::ThickenThickness2 => "Thickness 2",
        Role::HelixRevolutions => "Revolutions",
        Role::HelixPitch => "Pitch",
        Role::HelixHeight => "Height",
        Role::HelixRadius => "Radius",
        Role::HelixStartAngle => "Start angle",
        _ => return None,
    })
}

pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    let (value, text) = match (k, role) {
        (FeatureKind::Thicken(x), Role::ThickenThickness1) => (&mut x.thickness1, &mut x.thickness1_expr),
        (FeatureKind::Thicken(x), Role::ThickenThickness2) => (&mut x.thickness2, &mut x.thickness2_expr),
        (FeatureKind::Helix(x), Role::HelixRevolutions) => (&mut x.revolutions, &mut x.revolutions_expr),
        (FeatureKind::Helix(x), Role::HelixPitch) => (&mut x.pitch, &mut x.pitch_expr),
        (FeatureKind::Helix(x), Role::HelixHeight) => (&mut x.height, &mut x.height_expr),
        (FeatureKind::Helix(x), Role::HelixRadius) => (&mut x.radius, &mut x.radius_expr),
        (FeatureKind::Helix(x), Role::HelixStartAngle) => (&mut x.start_angle, &mut x.start_angle_expr),
        _ => return,
    };
    *value = v;
    *text = expr;
}

/// The opposite direction arrows.
pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    if role != Role::Flip {
        return;
    }
    match k {
        FeatureKind::Thicken(x) => x.flip = !x.flip,
        FeatureKind::Helix(x) => x.flip = !x.flip,
        _ => {}
    }
}

/// An item's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::Thicken(x), Role::ThickenEntities) => {
            // The list shows faces, regions, sketches, then parts.
            let (a, b, c) = (x.faces.len(), x.regions.len(), x.sketches.len());
            if i < a {
                x.faces.remove(i);
            } else if i < a + b {
                x.regions.remove(i - a);
            } else if i < a + b + c {
                x.sketches.remove(i - a - b);
            } else if i - a - b - c < x.parts.len() {
                x.parts.remove(i - a - b - c);
            }
        }
        (FeatureKind::Thicken(x), Role::MergeScope) if i < x.merge_scope.len() => {
            x.merge_scope.remove(i);
        }
        (FeatureKind::Helix(x), Role::HelixEntity) => {
            x.face = None;
            x.axis = None;
        }
        (FeatureKind::Fill(x), Role::FillEdges) if i < x.edges.len() => {
            x.edges.remove(i);
            if i < x.continuity.len() {
                x.continuity.remove(i);
            }
        }
        (FeatureKind::Fill(x), Role::MergeScope) if i < x.merge_scope.len() => {
            x.merge_scope.remove(i);
        }
        (k, r) => crate::flat_ui::remove(k, r, i),
    }
}
