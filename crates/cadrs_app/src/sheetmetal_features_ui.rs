//! The dialogs of the sheet metal features that edit an active model (P3I.4;
//! `reference/onshape/sheetmetal/help/feature-tools/`), in the applied-feature dialogs:
//!
//! - **Flange** (`sheetmetalflange-dialog-01.png`, `-03.png`): *Edges or side faces to flange*;
//!   *Flange alignment* (Inner, Outer, Middle, Hold line); *End type* (Blind with *Distance*, Up
//!   to entity, Up to entity with offset); the angle control (Bend angle, Align to geometry, Angle
//!   from direction) with the opposite-direction arrow; *Automatic miter* (off: *Miter angle*);
//!   *Use model bend radius* (off: *Bend radius*); *Partial flange* with **Overall parameters**
//!   (Per edge / Per chain with the flip sides arrow, *Hold adjacent edges*) and **End
//!   conditions** (bound type and distance, *Second bound*). Arrows in the view drag the
//!   distance and both bounds of a partial flange.
//! - **Hem** (`shmetal-hem-dialog.png`): *Edges or side faces to hem*; the hem type (Straight,
//!   Rolled, Tear drop) with the flip arrow; *Flattened* / *Inner radius*, *Total length*,
//!   *Angle*, *Minimal gap* / *Gap*; *Hem alignment* (Outer, In place); *Corner type* (Simple,
//!   Closed). A new hem starts from the last hem's values (SM4.5).
//! - **Make joint** (`sheetmetalmakejoint-dialog.png`): *Edges or side faces to join* (two); Rip
//!   with its style (Edge joint, Butt joint – Direction 1 / 2) or Bend with *Use model bend
//!   radius* / *Bend radius*.
//!
//! The toolbar's Sheet metal model ▾ and Search tools start them ([`SmTool`]).

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::applied::EdgeOrFace;
use cadrs_core::document::{DirectionRef, VertexRef};
use cadrs_core::sheetmetal_features::{
    AngleControl, Bound, ChainType, FlangeEnd, FlangeFeature, HemFeature, MakeJointFeature, MakeJointType, SheetMetalFeature, SmTarget,
};
use cadrs_core::{Feature, FeatureKind};
use cadrs_sheetmetal::model::{HemAlignment, RipStyle};
use cadrs_sheetmetal::sharp_edit::{FlangeAlignment, HemKind};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{Collapsible, NumberField, NumberFieldState, OptionRow, Select, SelectionList};

use crate::ActiveDocument;
use crate::applied::{AppliedField, AppliedKind, AppliedSession, entity_of};
use crate::applied_dialog::Role;
use crate::parts::{PartCache, PickFilter};
use crate::viewport::Pick;
use crate::AppState;

/// The three tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmTool {
    Flange,
    Hem,
    MakeJoint,
}

impl SmTool {
    pub const ALL: [SmTool; 3] = [SmTool::Flange, SmTool::Hem, SmTool::MakeJoint];

    /// The toolbar name (the ▾ menu's and Search tools' id).
    pub fn name(self) -> &'static str {
        match self {
            SmTool::Flange => "sheet-metal-flange",
            SmTool::Hem => "sheet-metal-hem",
            SmTool::MakeJoint => "sheet-metal-make-joint",
        }
    }

    pub fn of_name(name: &str) -> Option<SmTool> {
        SmTool::ALL.into_iter().find(|t| t.name() == name || format!("sheet-metal-menu-{}", t.name().trim_start_matches("sheet-metal-")) == name)
    }

    pub fn of(k: &SheetMetalFeature) -> SmTool {
        match k {
            SheetMetalFeature::Flange(_) => SmTool::Flange,
            SheetMetalFeature::Hem(_) => SmTool::Hem,
            SheetMetalFeature::MakeJoint(_) => SmTool::MakeJoint,
        }
    }
}

/// Whether a sheet metal tool of the toolbar's ▾ is built (the others stay greyed).
pub fn built(name: &str) -> bool {
    SmTool::of_name(name).is_some()
}

/// The dialog's selection fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmfField {
    Edges,
    UpTo,
    ParallelTo,
    Direction,
    BoundUpTo,
    SecondUpTo,
}

/// What an interactive part of these dialogs is for (`Role::Smf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmfRole {
    // Lists
    List(SmfField),
    // Selects
    Alignment,
    EndType,
    AngleControl,
    Chain,
    BoundType,
    SecondType,
    HemType,
    HemAlignment,
    CornerType,
    JointType,
    RipStyle,
    // Numbers
    Num(SmfNum),
    // Buttons
    Flip,
    FlipSides,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmfNum {
    Distance,
    Offset,
    BendAngle,
    DirectionAngle,
    MiterAngle,
    Radius,
    BoundDistance,
    BoundOffset,
    SecondDistance,
    SecondOffset,
    HemRadius,
    HemTotal,
    HemAngle,
    HemGap,
}

/// The lists' roles (the dialog asks each for its items).
pub(crate) const LIST_ROLES: [Role; 6] = [
    Role::Smf(SmfRole::List(SmfField::Edges)),
    Role::Smf(SmfRole::List(SmfField::UpTo)),
    Role::Smf(SmfRole::List(SmfField::ParallelTo)),
    Role::Smf(SmfRole::List(SmfField::Direction)),
    Role::Smf(SmfRole::List(SmfField::BoundUpTo)),
    Role::Smf(SmfRole::List(SmfField::SecondUpTo)),
];

/// The last hem's settings, for the next hem (SM4.5).
#[derive(Resource, Debug, Default)]
pub struct LastHem(pub Option<HemFeature>);

pub struct SheetMetalFeaturesPlugin;

impl Plugin for SheetMetalFeaturesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastHem>()
            .init_resource::<SmArrows>()
            .add_systems(Update, (remember_hem, arrows_pointer).run_if(in_state(AppState::Document)))
            .add_systems(PostUpdate, place_arrows.before(bevy::ui::UiSystems::Layout).run_if(in_state(AppState::Document)));
    }
}

fn sm(kind: &FeatureKind) -> Option<&SheetMetalFeature> {
    match kind {
        FeatureKind::SheetMetal(x) => Some(x),
        _ => None,
    }
}

fn sm_mut(kind: &mut FeatureKind) -> Option<&mut SheetMetalFeature> {
    match kind {
        FeatureKind::SheetMetal(x) => Some(x),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// New features and picks

/// A new feature from the selection (edges and faces go into its first field).
pub fn initial(world: &World, tool: SmTool, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let cache = world.resource::<PartCache>();
    let units = world.get_resource::<crate::WorkspaceUnits>().map(|u| u.0).unwrap_or_default();
    let edges: Vec<EdgeOrFace> = picked.iter().filter_map(|p| entity_of(cache, *p)).collect();
    let len = |v: f64| units.with_unit(v, Quantity::Length);
    let x = match tool {
        SmTool::Flange => {
            let mut f = FlangeFeature { edges, ..Default::default() };
            f.distance_expr = len(f.distance);
            f.offset_expr = len(f.offset);
            f.radius_expr = len(f.radius);
            f.bound.distance_expr = len(f.bound.distance);
            f.bound.offset_expr = len(f.bound.offset);
            SheetMetalFeature::Flange(f)
        }
        SmTool::Hem => {
            let mut h = world.resource::<LastHem>().0.clone().unwrap_or_else(|| {
                let mut h = HemFeature::default();
                h.radius_expr = len(h.radius);
                h.total_expr = len(h.total);
                h.gap_expr = len(h.gap);
                h
            });
            h.edges = edges;
            SheetMetalFeature::Hem(h)
        }
        SmTool::MakeJoint => {
            let mut j = MakeJointFeature { edges: edges.into_iter().take(2).collect(), ..Default::default() };
            j.radius_expr = len(j.radius);
            SheetMetalFeature::MakeJoint(j)
        }
    };
    let base = match tool {
        SmTool::Flange => "Flange",
        SmTool::Hem => "Hem",
        SmTool::MakeJoint => "Make joint",
    };
    Some((base, FeatureKind::SheetMetal(x), AppliedField::Smf(SmfField::Edges)))
}

fn target_of(features: &[Feature], cache: &PartCache, pick: Pick) -> Option<SmTarget> {
    Some(match pick {
        Pick::Plane(k) => SmTarget::Plane(k.plane_ref()),
        Pick::Feature(f) => SmTarget::Plane(cadrs_core::parts::plane_feature_ref(features, f)?),
        Pick::Vertex(part, vertex) => {
            let point = cache.part(part)?.solid.vertex(&vertex)?.point;
            SmTarget::Vertex(VertexRef { part, vertex, point })
        }
        _ => match entity_of(cache, pick)? {
            EdgeOrFace::Edge(e) => SmTarget::Edge(e),
            EdgeOrFace::Face(f) => SmTarget::Face(f),
        },
    })
}

fn same_entity(a: &EdgeOrFace, b: &EdgeOrFace) -> bool {
    match (a, b) {
        (EdgeOrFace::Edge(a), EdgeOrFace::Edge(b)) => a.edge == b.edge,
        (EdgeOrFace::Face(a), EdgeOrFace::Face(b)) => a.face == b.face,
        _ => false,
    }
}

fn same_target(a: &SmTarget, b: &SmTarget) -> bool {
    match (a, b) {
        (SmTarget::Edge(a), SmTarget::Edge(b)) => a.edge == b.edge,
        (SmTarget::Face(a), SmTarget::Face(b)) => a.face == b.face,
        (SmTarget::Vertex(a), SmTarget::Vertex(b)) => a.vertex == b.vertex,
        (SmTarget::Plane(a), SmTarget::Plane(b)) => a == b,
        _ => false,
    }
}

fn set_target(slot: &mut Option<SmTarget>, t: SmTarget) {
    *slot = if slot.as_ref().is_some_and(|s| same_target(s, &t)) { None } else { Some(t) };
}

/// A pick into the dialog's fields. False if it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let AppliedField::Smf(field) = field else { return false };
    let Some(x) = sm_mut(kind) else { return false };
    let features = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.features().to_vec()).unwrap_or_default();
    let cache = world.resource::<PartCache>();
    match field {
        SmfField::Edges => {
            let Some(e) = entity_of(cache, pick) else { return false };
            let two = matches!(x, SheetMetalFeature::MakeJoint(_));
            let list = match x {
                SheetMetalFeature::Flange(f) => &mut f.edges,
                SheetMetalFeature::Hem(h) => &mut h.edges,
                SheetMetalFeature::MakeJoint(j) => &mut j.edges,
            };
            match list.iter().position(|y| same_entity(y, &e)) {
                Some(i) => {
                    list.remove(i);
                }
                // Make joint takes two: a third replaces the second.
                None if two && list.len() >= 2 => {
                    list[1] = e;
                }
                None => list.push(e),
            }
        }
        SmfField::UpTo | SmfField::BoundUpTo | SmfField::SecondUpTo => {
            let SheetMetalFeature::Flange(f) = x else { return false };
            let Some(t) = target_of(&features, cache, pick) else { return false };
            match field {
                SmfField::UpTo => set_target(&mut f.up_to, t),
                SmfField::BoundUpTo => set_target(&mut f.bound.up_to, t),
                _ => match f.second.as_mut() {
                    Some(s) => set_target(&mut s.up_to, t),
                    None => return false,
                },
            }
        }
        SmfField::ParallelTo | SmfField::Direction => {
            let SheetMetalFeature::Flange(f) = x else { return false };
            let Some(d) = crate::pattern::direction_of(&features, cache, pick) else { return false };
            let slot = if field == SmfField::ParallelTo { &mut f.parallel_to } else { &mut f.direction };
            *slot = if *slot == Some(d) { None } else { Some(d) };
        }
    }
    true
}

/// What a pick of a field may be.
pub fn pick_filter(field: AppliedField, none: PickFilter) -> Option<PickFilter> {
    let AppliedField::Smf(f) = field else { return None };
    let planes = [true; 3];
    Some(match f {
        SmfField::Edges => PickFilter { faces: true, edges: true, ..none },
        SmfField::UpTo | SmfField::BoundUpTo | SmfField::SecondUpTo => PickFilter { planes, plane_features: true, faces: true, edges: true, ..none },
        SmfField::ParallelTo | SmfField::Direction => {
            PickFilter { planes, plane_features: true, faces: true, planar_only: true, edges: true, sketch_curves: true, connectors: true, ..none }
        }
    })
}

/// What its fields refer to, shown selected in the view while the dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let Some(x) = sm(kind) else { return Vec::new() };
    let mut v = Vec::new();
    if let SheetMetalFeature::Flange(f) = x {
        for t in [&f.up_to, &f.bound.up_to].into_iter().flatten().chain(f.second.as_ref().and_then(|s| s.up_to.as_ref())) {
            match t {
                SmTarget::Face(r) => v.push(Pick::Face(r.part, r.face)),
                SmTarget::Edge(r) => v.push(Pick::Edge(r.part, r.edge)),
                SmTarget::Vertex(r) => v.push(Pick::Vertex(r.part, r.vertex)),
                SmTarget::Plane(p) => v.extend(crate::viewport::plane_pick(*p)),
            }
        }
    }
    let _ = cache;
    v
}

/// The picked edges and faces, drawn where they were before the feature.
pub fn drawn_references(kind: &FeatureKind) -> Vec<EdgeOrFace> {
    sm(kind).map(|x| x.entities().to_vec()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// The dialog

/// The dialog's name ("sheet-metal-flange-dialog").
pub fn name(kind: &FeatureKind) -> Option<&'static str> {
    sm(kind).map(|x| SmTool::of(x).name())
}

/// What decides the dialog's rows.
pub fn layout(kind: &FeatureKind) -> Option<String> {
    Some(match sm(kind)? {
        SheetMetalFeature::Flange(f) => format!(
            "flange {:?} {:?} {:?} {} {} {} {} {:?} {} {} {:?} {:?}",
            f.alignment,
            f.end,
            f.angle_control,
            f.flip,
            f.auto_miter,
            f.use_model_radius,
            f.partial,
            f.chain,
            f.flip_sides,
            f.hold_adjacent,
            f.bound.kind,
            f.second.as_ref().map(|s| s.kind)
        ),
        SheetMetalFeature::Hem(h) => format!("hem {:?} {} {} {} {:?} {}", h.kind, h.flip, h.flattened, h.use_minimal_gap, h.alignment, h.closed),
        SheetMetalFeature::MakeJoint(j) => format!("joint {:?} {:?} {}", j.kind, j.style, j.use_model_radius),
    })
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn entity_label(features: &[Feature], e: &EdgeOrFace) -> String {
    match e {
        EdgeOrFace::Edge(r) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &r.edge))),
        EdgeOrFace::Face(r) => format!("Face of {}", op_name(features, r.face.op)),
    }
}

fn target_label(features: &[Feature], t: &SmTarget) -> String {
    match t {
        SmTarget::Edge(r) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &r.edge))),
        SmTarget::Face(r) => format!("Face of {}", op_name(features, r.face.op)),
        SmTarget::Vertex(v) => format!("Vertex of {}", op_name(features, v.vertex.faces[0].op)),
        SmTarget::Plane(p) => crate::viewport::plane_label(features, *p),
    }
}

fn direction_label(features: &[Feature], d: &DirectionRef) -> String {
    match d {
        DirectionRef::Edge(r) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &r.edge))),
        DirectionRef::SketchLine { sketch, .. } => format!("Line of {}", features.iter().find(|f| f.id == *sketch).map_or("sketch".into(), |f| f.name.clone())),
        DirectionRef::FaceNormal(f) => format!("Face of {}", op_name(features, f.face.op)),
        DirectionRef::PlaneNormal(p) => crate::viewport::plane_label(features, *p),
        DirectionRef::Connector(c) => c.label(features),
    }
}

/// The items of a list.
pub(crate) fn items(features: &[Feature], kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    let Role::Smf(SmfRole::List(field)) = role else { return None };
    let x = sm(kind)?;
    Some(match (x, field) {
        (_, SmfField::Edges) => x.entities().iter().map(|e| entity_label(features, e)).collect(),
        (SheetMetalFeature::Flange(f), SmfField::UpTo) => f.up_to.iter().map(|t| target_label(features, t)).collect(),
        (SheetMetalFeature::Flange(f), SmfField::BoundUpTo) => f.bound.up_to.iter().map(|t| target_label(features, t)).collect(),
        (SheetMetalFeature::Flange(f), SmfField::SecondUpTo) => f.second.iter().filter_map(|s| s.up_to.as_ref()).map(|t| target_label(features, t)).collect(),
        (SheetMetalFeature::Flange(f), SmfField::ParallelTo) => f.parallel_to.iter().map(|d| direction_label(features, d)).collect(),
        (SheetMetalFeature::Flange(f), SmfField::Direction) => f.direction.iter().map(|d| direction_label(features, d)).collect(),
        _ => Vec::new(),
    })
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    match role {
        Role::Smf(SmfRole::List(f)) => Some(AppliedField::Smf(f)),
        _ => None,
    }
}

fn list(b: &mut ChildSpawner, t: &Theme, name: &str, placeholder: &str, field: SmfField, items: Vec<String>, active: bool) {
    b.spawn((Role::Smf(SmfRole::List(field)), SelectionList::new(name.to_string()).placeholder(placeholder).items(items).active(active).build(t)))
        .entry::<Node>()
        .and_modify(|mut n| {
            n.flex_grow = 0.0;
            n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(3.0), Val::Px(3.0));
        });
}

/// The flip arrow's place (after a select or a number), or a spacer.
fn flip_or_space(r: &mut ChildSpawner, t: &Theme, name: &str, flip: Option<(SmfRole, bool, &str)>) {
    match flip {
        Some((role, on, tip)) => crate::extrude_dialog::flip_button_any(r, t, &format!("{name}-flip"), Role::Smf(role), on, tip),
        None => {
            r.spawn(Node { width: Val::Px(22.0), flex_shrink: 0.0, ..default() });
        }
    }
}

fn number(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, num: SmfNum, text: &str) {
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), margin: UiRect::new(Val::Px(-16.0), Val::ZERO, Val::Px(1.0), Val::Px(1.0)), ..default() }).with_children(|r| {
        r.spawn((Role::Smf(SmfRole::Num(num)), NumberField::new(name.to_string(), label.to_string()).text(text.to_string()).label_width(96.0).build(t)))
            .entry::<Node>()
            .and_modify(|mut n| n.flex_grow = 1.0);
        flip_or_space(r, t, name, None);
    });
}

/// A select, labelled ("Flange alignment  Inner ▾") or not ("Bend angle ▾"), with an arrow.
#[allow(clippy::too_many_arguments)]
fn select(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, role: SmfRole, options: Vec<&str>, selected: usize, flip: Option<(SmfRole, bool, &str)>) {
    let mut s = Select::new(name.to_string());
    for o in options {
        s = s.option(o.to_string(), true);
    }
    b.spawn(Node { height: Val::Px(30.0), margin: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(1.0), Val::Px(1.0)), align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() })
        .with_children(|r| {
            if !label.is_empty() {
                r.spawn((t.text(label, 11.0, bevy::text::FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(98.0), flex_shrink: 0.0, ..default() }));
            }
            r.spawn((Role::Smf(role), s.selected(selected).build(t))).entry::<Node>().and_modify(|mut n| n.flex_grow = 1.0);
            flip_or_space(r, t, name, flip);
        });
}

fn check(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, on: bool) {
    b.spawn(OptionRow::new(name.to_string(), label.to_string()).checked(on).build(t));
}

fn index_of<T: PartialEq>(all: &[T], x: &T) -> usize {
    all.iter().position(|y| y == x).unwrap_or(0)
}

const HEM_ALIGNMENTS: [HemAlignment; 2] = [HemAlignment::Outer, HemAlignment::InPlace];

fn hem_alignment_label(a: HemAlignment) -> &'static str {
    match a {
        HemAlignment::Outer => "Outer",
        HemAlignment::InPlace => "In place",
    }
}

/// The rows of one end condition of a partial flange.
fn bound_rows(c: &mut ChildSpawner, t: &Theme, prefix: &str, b: &Bound, second: bool, field: AppliedField, item: &dyn Fn(Role) -> Vec<String>) {
    let (type_role, dist, off, list_field) = if second {
        (SmfRole::SecondType, SmfNum::SecondDistance, SmfNum::SecondOffset, SmfField::SecondUpTo)
    } else {
        (SmfRole::BoundType, SmfNum::BoundDistance, SmfNum::BoundOffset, SmfField::BoundUpTo)
    };
    select(c, t, &format!("{prefix}-type"), "", type_role, FlangeEnd::ALL.iter().map(|e| e.label()).collect(), index_of(&FlangeEnd::ALL, &b.kind), None);
    match b.kind {
        FlangeEnd::Blind => number(c, t, &format!("{prefix}-distance"), if second { "Second distance" } else { "Distance" }, dist, &b.distance_expr),
        k => {
            let role = Role::Smf(SmfRole::List(list_field));
            list(c, t, &format!("{prefix}-up-to-field"), "Up to entity", list_field, item(role), field == AppliedField::Smf(list_field));
            if k == FlangeEnd::UpToEntityOffset {
                number(c, t, &format!("{prefix}-offset"), "Offset value", off, &b.offset_expr);
            }
        }
    }
}

/// The dialog's body.
pub(crate) fn body(b: &mut ChildSpawner, t: &Theme, kind: &FeatureKind, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>) {
    let Some(x) = sm(kind) else { return };
    let item = |f: SmfField| items_of(Role::Smf(SmfRole::List(f)));
    let active = |f: SmfField| field == AppliedField::Smf(f);
    b.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(6.0), Val::ZERO), ..default() }).with_children(|c| match x {
        SheetMetalFeature::Flange(f) => {
            list(c, t, "smf-edges-field", "Edges or side faces to flange", SmfField::Edges, item(SmfField::Edges), active(SmfField::Edges));
            select(c, t, "smf-alignment", "Flange alignment", SmfRole::Alignment, FlangeAlignment::ALL.iter().map(|a| a.label()).collect(), index_of(&FlangeAlignment::ALL, &f.alignment), None);
            select(c, t, "smf-end-type", "End type", SmfRole::EndType, FlangeEnd::ALL.iter().map(|e| e.label()).collect(), index_of(&FlangeEnd::ALL, &f.end), None);
            match f.end {
                FlangeEnd::Blind => number(c, t, "smf-distance", "Distance", SmfNum::Distance, &f.distance_expr),
                k => {
                    list(c, t, "smf-up-to-field", "Up to entity", SmfField::UpTo, item(SmfField::UpTo), active(SmfField::UpTo));
                    if k == FlangeEnd::UpToEntityOffset {
                        number(c, t, "smf-offset", "Offset value", SmfNum::Offset, &f.offset_expr);
                    }
                }
            }
            select(c, t, "smf-angle-control", "", SmfRole::AngleControl, AngleControl::ALL.iter().map(|a| a.label()).collect(), index_of(&AngleControl::ALL, &f.angle_control), Some((SmfRole::Flip, f.flip, "Opposite direction")));
            match f.angle_control {
                AngleControl::BendAngle => number(c, t, "smf-bend-angle", "Bend angle", SmfNum::BendAngle, &f.angle_expr),
                AngleControl::AlignToGeometry => list(c, t, "smf-parallel-field", "Parallel to", SmfField::ParallelTo, item(SmfField::ParallelTo), active(SmfField::ParallelTo)),
                AngleControl::AngleFromDirection => {
                    list(c, t, "smf-direction-field", "Direction", SmfField::Direction, item(SmfField::Direction), active(SmfField::Direction));
                    number(c, t, "smf-direction-angle", "Angle", SmfNum::DirectionAngle, &f.direction_angle_expr);
                }
            }
            check(c, t, "smf-auto-miter", "Automatic miter", f.auto_miter);
            if !f.auto_miter {
                number(c, t, "smf-miter-angle", "Miter angle", SmfNum::MiterAngle, &f.miter_angle_expr);
            }
            check(c, t, "smf-model-radius", "Use model bend radius", f.use_model_radius);
            if !f.use_model_radius {
                number(c, t, "smf-radius", "Bend radius", SmfNum::Radius, &f.radius_expr);
            }
            check(c, t, "smf-partial", "Partial flange", f.partial);
            if f.partial {
                let (f1, t1) = (f.clone(), t.clone());
                c.spawn(
                    Collapsible::new("smf-overall", "Overall parameters")
                        .open(true)
                        .content(move |c| {
                            select(c, &t1, "smf-chain", "", SmfRole::Chain, ChainType::ALL.iter().map(|x| x.label()).collect(), index_of(&ChainType::ALL, &f1.chain), Some((SmfRole::FlipSides, f1.flip_sides, "Flip sides")));
                            check(c, &t1, "smf-hold-adjacent", "Hold adjacent edges", f1.hold_adjacent);
                        })
                        .build(t),
                );
                let (f2, t2) = (f.clone(), t.clone());
                let lists: Vec<(Role, Vec<String>)> = LIST_ROLES.iter().map(|r| (*r, items_of(*r))).collect();
                c.spawn(
                    Collapsible::new("smf-end-conditions", "End conditions")
                        .open(true)
                        .content(move |c| {
                            let item = |r: Role| lists.iter().find(|(x, _)| *x == r).map(|(_, v)| v.clone()).unwrap_or_default();
                            bound_rows(c, &t2, "smf-bound", &f2.bound, false, field, &item);
                            check(c, &t2, "smf-second-bound", "Second bound", f2.second.is_some());
                            if let Some(s) = &f2.second {
                                bound_rows(c, &t2, "smf-second", s, true, field, &item);
                            }
                        })
                        .build(t),
                );
            }
        }
        SheetMetalFeature::Hem(h) => {
            list(c, t, "smf-edges-field", "Edges or side faces to hem", SmfField::Edges, item(SmfField::Edges), active(SmfField::Edges));
            select(c, t, "smf-hem-type", "", SmfRole::HemType, HemKind::ALL.iter().map(|k| k.label()).collect(), index_of(&HemKind::ALL, &h.kind), Some((SmfRole::Flip, h.flip, "Flip")));
            match h.kind {
                HemKind::Straight => {
                    check(c, t, "smf-flattened", "Flattened", h.flattened);
                    if !h.flattened {
                        number(c, t, "smf-hem-radius", "Inner radius", SmfNum::HemRadius, &h.radius_expr);
                    }
                    number(c, t, "smf-hem-total", "Total length", SmfNum::HemTotal, &h.total_expr);
                }
                HemKind::Rolled => {
                    number(c, t, "smf-hem-radius", "Inner radius", SmfNum::HemRadius, &h.radius_expr);
                    number(c, t, "smf-hem-angle", "Angle", SmfNum::HemAngle, &h.angle_expr);
                }
                HemKind::TearDrop => {
                    number(c, t, "smf-hem-radius", "Inner radius", SmfNum::HemRadius, &h.radius_expr);
                    check(c, t, "smf-minimal-gap", "Minimal gap", h.use_minimal_gap);
                    if !h.use_minimal_gap {
                        number(c, t, "smf-hem-gap", "Gap", SmfNum::HemGap, &h.gap_expr);
                    }
                    number(c, t, "smf-hem-total", "Total length", SmfNum::HemTotal, &h.total_expr);
                }
            }
            select(c, t, "smf-hem-alignment", "Hem alignment", SmfRole::HemAlignment, HEM_ALIGNMENTS.iter().map(|a| hem_alignment_label(*a)).collect(), index_of(&HEM_ALIGNMENTS, &h.alignment), None);
            select(c, t, "smf-corner-type", "Corner type", SmfRole::CornerType, vec!["Simple", "Closed"], usize::from(h.closed), None);
        }
        SheetMetalFeature::MakeJoint(j) => {
            list(c, t, "smf-edges-field", "Edges or side faces to join", SmfField::Edges, item(SmfField::Edges), active(SmfField::Edges));
            select(c, t, "smf-joint-type", "", SmfRole::JointType, MakeJointType::ALL.iter().map(|k| k.label()).collect(), index_of(&MakeJointType::ALL, &j.kind), None);
            match j.kind {
                MakeJointType::Rip => select(c, t, "smf-rip-style", "", SmfRole::RipStyle, RipStyle::ALL.iter().map(|s| s.label()).collect(), index_of(&RipStyle::ALL, &j.style), None),
                MakeJointType::Bend => {
                    check(c, t, "smf-model-radius", "Use model bend radius", j.use_model_radius);
                    if !j.use_model_radius {
                        number(c, t, "smf-radius", "Bend radius", SmfNum::Radius, &j.radius_expr);
                    }
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    let Role::Smf(r) = role else { return None };
    Some(match (sm(kind)?, r) {
        (SheetMetalFeature::Flange(f), SmfRole::Alignment) => index_of(&FlangeAlignment::ALL, &f.alignment),
        (SheetMetalFeature::Flange(f), SmfRole::EndType) => index_of(&FlangeEnd::ALL, &f.end),
        (SheetMetalFeature::Flange(f), SmfRole::AngleControl) => index_of(&AngleControl::ALL, &f.angle_control),
        (SheetMetalFeature::Flange(f), SmfRole::Chain) => index_of(&ChainType::ALL, &f.chain),
        (SheetMetalFeature::Flange(f), SmfRole::BoundType) => index_of(&FlangeEnd::ALL, &f.bound.kind),
        (SheetMetalFeature::Flange(f), SmfRole::SecondType) => index_of(&FlangeEnd::ALL, &f.second.as_ref()?.kind),
        (SheetMetalFeature::Hem(h), SmfRole::HemType) => index_of(&HemKind::ALL, &h.kind),
        (SheetMetalFeature::Hem(h), SmfRole::HemAlignment) => index_of(&HEM_ALIGNMENTS, &h.alignment),
        (SheetMetalFeature::Hem(h), SmfRole::CornerType) => usize::from(h.closed),
        (SheetMetalFeature::MakeJoint(j), SmfRole::JointType) => index_of(&MakeJointType::ALL, &j.kind),
        (SheetMetalFeature::MakeJoint(j), SmfRole::RipStyle) => index_of(&RipStyle::ALL, &j.style),
        _ => return None,
    })
}

pub(crate) fn owns(role: Role) -> bool {
    matches!(role, Role::Smf(_))
}

/// A select changed: the change's label and the field that takes the next picks.
pub(crate) fn set_select(kind: &mut FeatureKind, role: Role, i: usize) -> (&'static str, Option<AppliedField>) {
    let (Some(x), Role::Smf(r)) = (sm_mut(kind), role) else { return ("", None) };
    let pick = |e: FlangeEnd, f: SmfField| (e != FlangeEnd::Blind).then_some(AppliedField::Smf(f));
    match (x, r) {
        (SheetMetalFeature::Flange(f), SmfRole::Alignment) => {
            f.alignment = FlangeAlignment::ALL[i.min(3)];
            ("Flange alignment", None)
        }
        (SheetMetalFeature::Flange(f), SmfRole::EndType) => {
            f.end = FlangeEnd::ALL[i.min(2)];
            ("End type", pick(f.end, SmfField::UpTo))
        }
        (SheetMetalFeature::Flange(f), SmfRole::AngleControl) => {
            f.angle_control = AngleControl::ALL[i.min(2)];
            let field = match f.angle_control {
                AngleControl::BendAngle => None,
                AngleControl::AlignToGeometry => Some(AppliedField::Smf(SmfField::ParallelTo)),
                AngleControl::AngleFromDirection => Some(AppliedField::Smf(SmfField::Direction)),
            };
            ("Angle control", field)
        }
        (SheetMetalFeature::Flange(f), SmfRole::Chain) => {
            f.chain = ChainType::ALL[i.min(1)];
            ("Chain type", None)
        }
        (SheetMetalFeature::Flange(f), SmfRole::BoundType) => {
            f.bound.kind = FlangeEnd::ALL[i.min(2)];
            ("Bound type", pick(f.bound.kind, SmfField::BoundUpTo))
        }
        (SheetMetalFeature::Flange(f), SmfRole::SecondType) => match f.second.as_mut() {
            Some(s) => {
                s.kind = FlangeEnd::ALL[i.min(2)];
                ("Second bound type", pick(s.kind, SmfField::SecondUpTo))
            }
            None => ("", None),
        },
        (SheetMetalFeature::Hem(h), SmfRole::HemType) => {
            h.kind = HemKind::ALL[i.min(2)];
            ("Hem type", None)
        }
        (SheetMetalFeature::Hem(h), SmfRole::HemAlignment) => {
            h.alignment = HEM_ALIGNMENTS[i.min(1)];
            ("Hem alignment", None)
        }
        (SheetMetalFeature::Hem(h), SmfRole::CornerType) => {
            h.closed = i == 1;
            ("Corner type", None)
        }
        (SheetMetalFeature::MakeJoint(j), SmfRole::JointType) => {
            j.kind = MakeJointType::ALL[i.min(1)];
            ("Joint type", None)
        }
        (SheetMetalFeature::MakeJoint(j), SmfRole::RipStyle) => {
            j.style = RipStyle::ALL[i.min(2)];
            ("Rip style", None)
        }
        _ => ("", None),
    }
}

pub fn is_checkbox(name: &str) -> bool {
    name.starts_with("smf-") && name.ends_with("-checkbox")
}

/// A checkbox changed: its label and the field that takes the next picks.
pub fn checkbox(kind: &mut FeatureKind, name: &str, on: bool) -> Option<(&'static str, Option<AppliedField>)> {
    let x = sm_mut(kind)?;
    Some(match (x, name) {
        (SheetMetalFeature::Flange(f), "smf-auto-miter-checkbox") => {
            f.auto_miter = on;
            ("Automatic miter", None)
        }
        (SheetMetalFeature::Flange(f), "smf-model-radius-checkbox") => {
            f.use_model_radius = on;
            ("Use model bend radius", None)
        }
        (SheetMetalFeature::MakeJoint(j), "smf-model-radius-checkbox") => {
            j.use_model_radius = on;
            ("Use model bend radius", None)
        }
        (SheetMetalFeature::Flange(f), "smf-partial-checkbox") => {
            f.partial = on;
            ("Partial flange", None)
        }
        (SheetMetalFeature::Flange(f), "smf-hold-adjacent-checkbox") => {
            f.hold_adjacent = on;
            ("Hold adjacent edges", None)
        }
        (SheetMetalFeature::Flange(f), "smf-second-bound-checkbox") => {
            f.second = on.then(|| Bound { distance: f.bound.distance, distance_expr: f.bound.distance_expr.clone(), ..Default::default() });
            ("Second bound", None)
        }
        (SheetMetalFeature::Hem(h), "smf-flattened-checkbox") => {
            h.flattened = on;
            ("Flattened", None)
        }
        (SheetMetalFeature::Hem(h), "smf-minimal-gap-checkbox") => {
            h.use_minimal_gap = on;
            ("Minimal gap", None)
        }
        _ => return None,
    })
}

/// The arrows.
pub(crate) fn flip(kind: &mut FeatureKind, role: Role) -> Option<&'static str> {
    let (Some(x), Role::Smf(r)) = (sm_mut(kind), role) else { return None };
    match (x, r) {
        (SheetMetalFeature::Flange(f), SmfRole::Flip) => {
            f.flip = !f.flip;
            Some("Opposite direction")
        }
        (SheetMetalFeature::Flange(f), SmfRole::FlipSides) => {
            f.flip_sides = !f.flip_sides;
            Some("Flip sides")
        }
        (SheetMetalFeature::Hem(h), SmfRole::Flip) => {
            h.flip = !h.flip;
            Some("Flip")
        }
        _ => None,
    }
}

/// An item's ✕.
pub(crate) fn remove(kind: &mut FeatureKind, role: Role, i: usize) {
    let (Some(x), Role::Smf(SmfRole::List(field))) = (sm_mut(kind), role) else { return };
    match (x, field) {
        (SheetMetalFeature::Flange(f), SmfField::Edges) if i < f.edges.len() => {
            f.edges.remove(i);
        }
        (SheetMetalFeature::Hem(h), SmfField::Edges) if i < h.edges.len() => {
            h.edges.remove(i);
        }
        (SheetMetalFeature::MakeJoint(j), SmfField::Edges) if i < j.edges.len() => {
            j.edges.remove(i);
        }
        (SheetMetalFeature::Flange(f), SmfField::UpTo) => f.up_to = None,
        (SheetMetalFeature::Flange(f), SmfField::BoundUpTo) => f.bound.up_to = None,
        (SheetMetalFeature::Flange(f), SmfField::SecondUpTo) => {
            if let Some(s) = f.second.as_mut() {
                s.up_to = None;
            }
        }
        (SheetMetalFeature::Flange(f), SmfField::ParallelTo) => f.parallel_to = None,
        (SheetMetalFeature::Flange(f), SmfField::Direction) => f.direction = None,
        _ => {}
    }
}

fn quantity(n: SmfNum) -> Quantity {
    match n {
        SmfNum::BendAngle | SmfNum::DirectionAngle | SmfNum::MiterAngle | SmfNum::HemAngle => Quantity::Angle,
        _ => Quantity::Length,
    }
}

fn label(n: SmfNum) -> &'static str {
    match n {
        SmfNum::Distance => "Distance",
        SmfNum::Offset => "Offset value",
        SmfNum::BendAngle => "Bend angle",
        SmfNum::DirectionAngle => "Angle",
        SmfNum::MiterAngle => "Miter angle",
        SmfNum::Radius => "Bend radius",
        SmfNum::BoundDistance => "Distance",
        SmfNum::BoundOffset => "Offset value",
        SmfNum::SecondDistance => "Second distance",
        SmfNum::SecondOffset => "Offset value",
        SmfNum::HemRadius => "Inner radius",
        SmfNum::HemTotal => "Total length",
        SmfNum::HemAngle => "Angle",
        SmfNum::HemGap => "Gap",
    }
}

/// A number's slot: its value and its expression.
fn slot(x: &mut SheetMetalFeature, n: SmfNum) -> Option<(&mut f64, &mut String)> {
    Some(match (x, n) {
        (SheetMetalFeature::Flange(f), SmfNum::Distance) => (&mut f.distance, &mut f.distance_expr),
        (SheetMetalFeature::Flange(f), SmfNum::Offset) => (&mut f.offset, &mut f.offset_expr),
        (SheetMetalFeature::Flange(f), SmfNum::BendAngle) => (&mut f.angle, &mut f.angle_expr),
        (SheetMetalFeature::Flange(f), SmfNum::DirectionAngle) => (&mut f.direction_angle, &mut f.direction_angle_expr),
        (SheetMetalFeature::Flange(f), SmfNum::MiterAngle) => (&mut f.miter_angle, &mut f.miter_angle_expr),
        (SheetMetalFeature::Flange(f), SmfNum::Radius) => (&mut f.radius, &mut f.radius_expr),
        (SheetMetalFeature::Flange(f), SmfNum::BoundDistance) => (&mut f.bound.distance, &mut f.bound.distance_expr),
        (SheetMetalFeature::Flange(f), SmfNum::BoundOffset) => (&mut f.bound.offset, &mut f.bound.offset_expr),
        (SheetMetalFeature::Flange(f), SmfNum::SecondDistance) => {
            let s = f.second.as_mut()?;
            (&mut s.distance, &mut s.distance_expr)
        }
        (SheetMetalFeature::Flange(f), SmfNum::SecondOffset) => {
            let s = f.second.as_mut()?;
            (&mut s.offset, &mut s.offset_expr)
        }
        (SheetMetalFeature::MakeJoint(j), SmfNum::Radius) => (&mut j.radius, &mut j.radius_expr),
        (SheetMetalFeature::Hem(h), SmfNum::HemRadius) => (&mut h.radius, &mut h.radius_expr),
        (SheetMetalFeature::Hem(h), SmfNum::HemTotal) => (&mut h.total, &mut h.total_expr),
        (SheetMetalFeature::Hem(h), SmfNum::HemAngle) => (&mut h.angle, &mut h.angle_expr),
        (SheetMetalFeature::Hem(h), SmfNum::HemGap) => (&mut h.gap, &mut h.gap_expr),
        _ => return None,
    })
}

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    let Role::Smf(SmfRole::Num(n)) = role else { return None };
    let mut x = sm(kind)?.clone();
    slot(&mut x, n).map(|(_, e)| e.clone())
}

/// A number typed: parsed and set (a value that doesn't parse turns the field red).
pub(crate) fn commit_number(world: &mut World, entity: Entity, role: Role, text: String, enter: bool) {
    let Role::Smf(SmfRole::Num(n)) = role else { return };
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let parsed = crate::sheetmetal_ui::parse(&text, quantity(n), &units, world.resource::<crate::variables_ui::ActiveVariables>());
    if let Some(mut st) = world.get_mut::<NumberFieldState>(entity) {
        st.error = parsed.is_none();
        if parsed.is_none() {
            st.text = text.clone();
        }
    }
    let Some((v, expr)) = parsed else { return };
    // A plain number typed for an angle reads "90 deg", as Onshape shows it.
    let expr = if quantity(n) == Quantity::Angle && text.trim().parse::<f64>().is_ok() { units.with_unit(v, Quantity::Angle) } else { expr };
    crate::applied::change_kind(world, label(n), |k| {
        if let Some(x) = sm_mut(k)
            && let Some((value, e)) = slot(x, n)
        {
            *value = v;
            *e = expr;
        }
    });
    if enter {
        world.resource_mut::<InputFocus>().clear();
        crate::applied::accept(world);
    }
}

/// SM4.5: the hem being edited is what the next hem starts from.
fn remember_hem(session: Option<Res<AppliedSession>>, doc: Option<Res<ActiveDocument>>, mut last: ResMut<LastHem>) {
    let Some(s) = session.filter(|s| s.kind == AppliedKind::SmFeature(SmTool::Hem)) else { return };
    let Some(FeatureKind::SheetMetal(SheetMetalFeature::Hem(h))) = doc.as_ref().and_then(|d| d.doc.element(s.element)?.feature(s.feature)).map(|f| &f.kind) else {
        return;
    };
    let want = HemFeature { edges: Vec::new(), ..h.clone() };
    if last.0.as_ref() != Some(&want) {
        last.0 = Some(want);
    }
}

// ---------------------------------------------------------------------------------------------
// Arrows in the view: the flange's distance and a partial flange's two bounds (SM3.3, SM3.7,
// `sheetmetalflange-dialog-03.png`)

const ARROW_LEN: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrowValue {
    Distance,
    Bound,
    Second,
}

impl ArrowValue {
    fn name(self) -> &'static str {
        match self {
            ArrowValue::Distance => "smf-distance-arrow",
            ArrowValue::Bound => "smf-bound-arrow",
            ArrowValue::Second => "smf-second-arrow",
        }
    }

    fn num(self) -> SmfNum {
        match self {
            ArrowValue::Distance => SmfNum::Distance,
            ArrowValue::Bound => SmfNum::BoundDistance,
            ArrowValue::Second => SmfNum::SecondDistance,
        }
    }
}

/// The arrows on screen and a drag in progress.
#[derive(Resource, Debug, Default)]
pub struct SmArrows {
    /// Each arrow: what it drags, its point and direction (world), its base and tip on screen.
    arrows: Vec<(ArrowValue, Vec3, Vec3, Option<(Vec2, Vec2)>)>,
    hovered: Option<ArrowValue>,
    drag: Option<SmDrag>,
}

#[derive(Debug, Clone)]
struct SmDrag {
    which: ArrowValue,
    start: Vec2,
    start_value: f64,
    dir_px: Vec2,
    kind: FeatureKind,
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// Where the arrows stand: the first picked edge (on the parts before the flange), its outward
/// direction out of the wall (the narrower face's normal) and the broad face's.
fn arrow_targets(f: &FlangeFeature, before: &crate::applied::BeforeParts) -> Vec<(ArrowValue, Vec3, Vec3)> {
    let Some(EdgeOrFace::Edge(e)) = f.edges.first() else { return Vec::new() };
    let Some(part) = before.parts.iter().find(|p| p.id == e.part) else { return Vec::new() };
    let Some(edge) = part.solid.edge(&e.edge) else { return Vec::new() };
    let (a, b) = (v(edge.points[0]), v(*edge.points.last().expect("points")));
    let along = (b - a).normalize_or_zero();
    // Its two faces: the side face (smaller) and the broad face.
    let mut faces: Vec<(f64, Vec3)> = e
        .edge
        .faces
        .iter()
        .filter_map(|n| {
            let fc = part.solid.face(n)?;
            Some((fc.area.unwrap_or(0.0), v(fc.plane?.normal()).normalize_or_zero()))
        })
        .collect();
    if faces.len() < 2 {
        return Vec::new();
    }
    faces.sort_by(|x, y| x.0.total_cmp(&y.0));
    let (out, broad) = (faces[0].1, faces[1].1);
    let theta = (f.angle as f32).to_radians();
    let side = if f.flip { -broad } else { broad };
    let dir = if f.angle_control == AngleControl::BendAngle { out * theta.cos() + side * theta.sin() } else { out };
    let mut v = Vec::new();
    let len = (b - a).length();
    let (d0, d1) = if f.partial { (f.bound.distance as f32, f.second.as_ref().map_or(0.0, |s| s.distance as f32)) } else { (0.0, 0.0) };
    let (d0, d1) = if f.flip_sides { (d1, d0) } else { (d0, d1) };
    let mid = a + along * ((d0 + len - d1) / 2.0);
    if f.end == FlangeEnd::Blind {
        v.push((ArrowValue::Distance, mid + dir * f.distance as f32, dir));
    }
    if f.partial {
        let (first, second) = if f.flip_sides { (b - along * d1, a + along * d0) } else { (a + along * d0, b - along * d1) };
        let (fd, sd) = if f.flip_sides { (-along, along) } else { (along, -along) };
        if f.bound.kind == FlangeEnd::Blind {
            v.push((ArrowValue::Bound, first, fd));
        }
        if f.second.as_ref().is_some_and(|s| s.kind == FlangeEnd::Blind) {
            v.push((ArrowValue::Second, second, sd));
        }
    }
    v
}

fn arrow_value(k: &FeatureKind, which: ArrowValue) -> Option<f64> {
    let mut x = sm(k)?.clone();
    slot(&mut x, which.num()).map(|(v, _)| *v)
}

#[derive(Component)]
struct SmArrowNode(ArrowValue);

#[derive(Component)]
struct SmArrowLine(ArrowValue);

#[allow(clippy::too_many_arguments)]
fn place_arrows(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    before: Res<crate::applied::BeforeParts>,
    view: Res<crate::viewport::ViewportView>,
    rect: Res<crate::viewport::ViewportRect>,
    mut arrows: ResMut<SmArrows>,
    q_area: Query<Entity, With<crate::viewport::ViewportArea>>,
    mut q: Query<(Entity, &SmArrowNode, &mut Node, &mut bevy::ui::UiTransform, &mut Visibility)>,
    mut q_line: Query<(&SmArrowLine, &mut ImageNode)>,
    mut commands: Commands,
) {
    let f = session.as_ref().filter(|s| s.kind == AppliedKind::SmFeature(SmTool::Flange)).and_then(|s| {
        let kind = match arrows.drag.as_ref() {
            Some(d) => d.kind.clone(),
            None => doc.as_ref()?.doc.element(s.element)?.feature(s.feature)?.kind.clone(),
        };
        match kind {
            FeatureKind::SheetMetal(SheetMetalFeature::Flange(f)) => Some(f),
            _ => None,
        }
    });
    let targets = f.as_ref().map(|f| arrow_targets(f, &before)).unwrap_or_default();
    arrows.arrows = targets
        .iter()
        .map(|(w, p, d)| {
            let dp = view.view.project_vector(*d);
            let bt = (dp.length() >= 0.05).then(|| {
                let b = rect.to_screen(view.view.project(*p));
                (b, b + dp.normalize() * ARROW_LEN)
            });
            (*w, *p, *d, bt)
        })
        .collect();
    let shown: Vec<ArrowValue> = arrows.arrows.iter().filter(|a| a.3.is_some()).map(|a| a.0).collect();
    for (e, n, ..) in &q {
        if !shown.contains(&n.0) {
            commands.entity(e).try_despawn();
        }
    }
    let Some(area) = q_area.iter().next() else { return };
    for (w, _, _, bt) in arrows.arrows.clone() {
        let Some((base, tip)) = bt else { continue };
        if !q.iter().any(|(_, n, ..)| n.0 == w) {
            let e = commands
                .spawn((
                    Name::new(w.name()),
                    SmArrowNode(w),
                    Node { position_type: PositionType::Absolute, width: Val::Px(ARROW_LEN), height: Val::Px(ARROW_LEN), ..default() },
                    bevy::ui::UiTransform::default(),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    ZIndex(-3),
                    DespawnOnExit(AppState::Document),
                    children![
                        (cadrs_ui::icon::icon_in("manipulator-arrow-halo", ARROW_LEN, Color::srgba_u8(0x3c, 0x46, 0x4e, 0xb0), Node { position_type: PositionType::Absolute, ..default() }), Pickable::IGNORE),
                        (SmArrowLine(w), cadrs_ui::icon::icon_in("manipulator-arrow-line", ARROW_LEN, Color::WHITE, Node { position_type: PositionType::Absolute, ..default() }), Pickable::IGNORE),
                    ],
                ))
                .id();
            commands.entity(area).add_child(e);
            continue;
        }
        let u = (tip - base).normalize_or_zero();
        let center = (base + tip) / 2.0 - rect.0.min;
        for (_, n, mut node, mut tr, mut vis) in &mut q {
            if n.0 != w {
                continue;
            }
            let (l, t) = (Val::Px(center.x - ARROW_LEN / 2.0), Val::Px(center.y - ARROW_LEN / 2.0));
            if node.left != l || node.top != t {
                node.left = l;
                node.top = t;
            }
            let want = bevy::ui::UiTransform { rotation: Rot2::radians(u.x.atan2(-u.y)), ..default() };
            if *tr != want {
                *tr = want;
            }
            vis.set_if_neq(Visibility::Inherited);
        }
    }
    let hot = arrows.drag.as_ref().map(|d| d.which).or(arrows.hovered);
    for (l, mut img) in &mut q_line {
        let c = if hot == Some(l.0) { Color::srgb_u8(0xff, 0xb4, 0x5a) } else { Color::WHITE };
        if img.color != c {
            img.color = c;
        }
    }
}

/// Grabs and drags an arrow: its value follows the pointer, one undo step on release.
#[allow(clippy::too_many_arguments)]
fn arrows_pointer(
    mut inputs: MessageReader<bevy::picking::pointer::PointerInput>,
    session: Option<Res<AppliedSession>>,
    mut arrows: ResMut<SmArrows>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<crate::viewport::ViewportView>,
    drag: Res<crate::viewport::ViewportDrag>,
    units: Res<crate::WorkspaceUnits>,
    mut over: ResMut<crate::parts::PartOverride>,
    mut commands: Commands,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton, PointerId};
    let Some(s) = session.filter(|s| s.kind == AppliedKind::SmFeature(SmTool::Flange)) else {
        inputs.clear();
        arrows.drag = None;
        return;
    };
    let near = |p: Vec2, (a, b): (Vec2, Vec2)| {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        p.distance(a + ab * t) <= 8.0
    };
    let hit = |p: Vec2, arrows: &SmArrows| arrows.arrows.iter().find(|a| a.3.is_some_and(|bt| near(p, bt))).map(|a| (a.0, a.2));
    arrows.hovered = hit(drag.pointer(), &arrows).map(|h| h.0);
    let kind = doc.as_ref().and_then(|d| Some(d.doc.element(s.element)?.feature(s.feature)?.kind.clone()));
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                if let (Some((which, dir)), Some(k)) = (hit(pos, &arrows), kind.clone())
                    && let Some(value) = arrow_value(&k, which)
                {
                    let dir_px = view.view.project_vector(dir);
                    if dir_px.length() > 0.05 {
                        arrows.drag = Some(SmDrag { which, start: pos, start_value: value, dir_px, kind: k });
                    }
                }
            }
            PointerAction::Move { .. } => {
                if let Some(d) = arrows.drag.as_mut() {
                    let along = (pos - d.start).dot(d.dir_px.normalize()) / d.dir_px.length();
                    let step = crate::extrude::snap_step(d.dir_px.length());
                    let min = if d.which == ArrowValue::Distance { step } else { 0.0 };
                    let value = (((d.start_value + along as f64) / step).round() * step).max(min);
                    let which = d.which;
                    if let Some(x) = sm_mut(&mut d.kind)
                        && let Some((val, e)) = slot(x, which.num())
                        && (*val - value).abs() > 1e-9
                    {
                        *val = value;
                        *e = units.0.with_unit(value, Quantity::Length);
                    }
                    let want = Some((s.feature, d.kind.clone()));
                    if over.applied != want {
                        over.applied = want;
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) if arrows.drag.is_some() => {
                commands.queue(|world: &mut World| {
                    world.resource_mut::<crate::parts::PartOverride>().applied = None;
                    let Some(d) = world.resource_mut::<SmArrows>().drag.take() else { return };
                    if crate::applied::current(world).is_some_and(|f| f.kind != d.kind) {
                        crate::applied::set(world, d.kind, if d.which == ArrowValue::Distance { "Drag distance" } else { "Drag bound" });
                    }
                });
            }
            PointerAction::Cancel => {
                arrows.drag = None;
                over.applied = None;
            }
            _ => {}
        }
    }
}
