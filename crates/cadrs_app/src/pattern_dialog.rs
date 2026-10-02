//! The dialogs of the P3.8 features, laid out as Onshape's, built and kept in step by
//! [`crate::applied_dialog`] (through [`crate::advanced_dialog`], which hands these kinds on):
//!
//! - **Linear pattern** (`ex5-step9.png`): the pattern type (Part, Feature, Face pattern); New |
//!   Add | Remove | Intersect for a part pattern; *Parts/Features/Faces to pattern* (faces with
//!   the Create selection button); *Direction*; *Distance*; *Instance count* with its flip;
//!   *Centered*; *Second direction* (its Direction, Distance, count and Centered); *Skip
//!   instances* with *Instances to skip* and CLEAR; merge.
//! - **Circular pattern** (`ex5-step5.png`): the type; the entities; *Axis of pattern* with the
//!   mate connector button; *Angle*; *Instance count* with its flip; *Equal spacing*;
//!   *Centered*; *Reapply features* (feature pattern); *Skip instances*.
//! - **Curve pattern** (PS25): the type; the entities; *Path to pattern along*; *Instance
//!   count*; *Equal spacing* (else *Distance*); *Tangent to curve*; *Skip instances*.
//! - **Mirror** (`ex5-step12.png`): the mirror type; the entities (faces with Create selection);
//!   *Mirror plane*; for a part mirror, New | Add | Remove | Intersect and merge.
//! - **Mate connector** (`ex4-step5.png`, `ex4-step8.png`; P3B.7): *Origin type* (On entity,
//!   Between entities); *Origin entity* (faces, edges, vertices, sketch points and curves, the
//!   origin); *Between entity*; **Realign** ✓ with *Primary axis* and *Secondary axis*; **Move**
//!   ✓ with *X*, *Y*, *Z translation* and *Rotation*; **Owner entity** ✓ with the owner part; the
//!   flip primary / reorient secondary buttons. The assembly's connector dialog
//!   ([`crate::assembly::connector_tool`]) is built from the same rows ([`connector_rows`]).

use bevy::prelude::*;
use cadrs_core::advanced::PathRef;
use cadrs_core::document::{BooleanOp, DirectionRef};
use cadrs_core::mate::{ConnectorOrigin, OriginType};
use cadrs_core::pattern::{MirrorPlane, PatternKind, PatternType, index_label};
use cadrs_core::{Feature, FeatureId, FeatureKind};
use cadrs_ui::prelude::*;
use cadrs_ui::{NumberField, OptionRow, TabStrip};

use crate::applied::AppliedField;
use crate::applied_dialog::{Role, body_column, index_of, list, number, opts, select_row};
use crate::extrude_dialog::flip_button_any;
use crate::parts::PartCache;

/// The dialog's name.
pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    Some(match kind {
        FeatureKind::Pattern(x) => match x.kind {
            PatternKind::Linear => "linear-pattern",
            PatternKind::Circular => "circular-pattern",
            PatternKind::Curve => "curve-pattern",
        },
        FeatureKind::Mirror(_) => "mirror",
        FeatureKind::MateConnector(_) => "mate-connector",
        _ => return crate::draft_ui::name(kind),
    })
}

/// What decides the rows.
pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    Some(match kind {
        FeatureKind::Pattern(x) => format!(
            "pattern {:?} {:?} {:?} {} {} {} {} {} {} {} {} {}",
            x.kind,
            x.pattern_type,
            x.op,
            x.merge_all,
            x.second_on,
            x.skip_on,
            x.equal_spacing,
            x.first.flip,
            x.second.flip,
            x.first.centered,
            x.reapply,
            x.tangent_to_curve
        ),
        FeatureKind::Mirror(x) => format!("mirror {:?} {:?} {}", x.mirror_type, x.op, x.merge_all),
        FeatureKind::MateConnector(x) => {
            format!("mate {} {} {:?} {} {} {}", x.flip_primary, x.reorient, x.origin_type, x.realign, x.move_on, x.owner_on)
        }
        _ => return crate::draft_ui::layout(kind),
    })
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn name_of(features: &[Feature], id: FeatureId) -> String {
    features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone())
}

fn edge_label(features: &[Feature], e: &cadrs_sketch::EdgeName) -> String {
    format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, e)))
}

fn direction_label(features: &[Feature], d: &DirectionRef) -> String {
    match d {
        DirectionRef::Edge(r) => edge_label(features, &r.edge),
        DirectionRef::SketchLine { sketch, .. } => format!("Line of {}", name_of(features, *sketch)),
        DirectionRef::FaceNormal(f) => format!("Face of {}", op_name(features, f.face.op)),
        DirectionRef::PlaneNormal(p) => crate::viewport::plane_label(features, *p),
        DirectionRef::Connector(c) => c.label(features),
    }
}

/// How a field names an implicit connector's entity.
pub(crate) fn origin_label(features: &[Feature], o: &ConnectorOrigin) -> String {
    match o {
        ConnectorOrigin::Origin => "Origin".into(),
        ConnectorOrigin::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
        ConnectorOrigin::Edge(e) => edge_label(features, &e.edge),
        ConnectorOrigin::Vertex(v) => format!("Vertex of {}", op_name(features, v.vertex.faces[0].op)),
        ConnectorOrigin::SketchPoint { sketch, .. } => format!("Vertex of {}", name_of(features, *sketch)),
        ConnectorOrigin::SketchCurve { sketch, .. } => format!("Edge of {}", name_of(features, *sketch)),
    }
}

fn seeds_label(ty: PatternType, verb: &str) -> String {
    match ty {
        PatternType::Part => format!("Parts to {verb}"),
        PatternType::Feature => format!("Features to {verb}"),
        PatternType::Face => format!("Faces to {verb}"),
    }
}

fn seed_items(features: &[Feature], cache: &PartCache, ty: PatternType, parts: &[cadrs_core::PartId], feats: &[FeatureId], faces: &[cadrs_core::FaceRef]) -> Vec<String> {
    match ty {
        PatternType::Part => crate::applied::part_names(cache, parts),
        PatternType::Feature => feats.iter().map(|f| name_of(features, *f)).collect(),
        PatternType::Face => faces.iter().map(|f| format!("Face of {}", op_name(features, f.face.op))).collect(),
    }
}

/// The items of a list.
pub(crate) fn items(features: &[Feature], cache: &PartCache, kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    let parts = |ids: &[cadrs_core::PartId]| crate::applied::part_names(cache, ids);
    Some(match (kind, role) {
        (FeatureKind::Pattern(x), Role::PatternEntities) => seed_items(features, cache, x.pattern_type, &x.parts, &x.features, &x.faces),
        (FeatureKind::Pattern(x), Role::PatternDirection) => x.first.direction.iter().map(|d| direction_label(features, d)).collect(),
        (FeatureKind::Pattern(x), Role::PatternDirection2) => x.second.direction.iter().map(|d| direction_label(features, d)).collect(),
        (FeatureKind::Pattern(x), Role::PatternAxis) => x.axis.iter().map(|a| crate::revolve::axis_label(features, a)).collect(),
        (FeatureKind::Pattern(x), Role::PatternPath) => x
            .path
            .iter()
            .map(|p| match p {
                PathRef::Edge(e) => edge_label(features, &e.edge),
                PathRef::SketchCurve { sketch, .. } => format!("Curve of {}", name_of(features, *sketch)),
                PathRef::Sketch(s) | PathRef::Curve(s) => name_of(features, *s),
            })
            .collect(),
        (FeatureKind::Pattern(x), Role::SkipList) => x.skipped.iter().map(|i| index_label(*i)).collect(),
        (FeatureKind::Pattern(x), Role::MergeScope) => parts(&x.merge_scope),
        (FeatureKind::Mirror(x), Role::PatternEntities) => seed_items(features, cache, x.mirror_type, &x.parts, &x.features, &x.faces),
        (FeatureKind::Mirror(x), Role::MirrorPlane) => x
            .plane
            .iter()
            .map(|p| match p {
                MirrorPlane::Plane(p) => crate::viewport::plane_label(features, *p),
                MirrorPlane::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
                MirrorPlane::Connector(c) => c.label(features),
            })
            .collect(),
        (FeatureKind::Mirror(x), Role::MergeScope) => parts(&x.merge_scope),
        (FeatureKind::MateConnector(x), Role::ConnectorOrigin) => x.origin.iter().map(|o| origin_label(features, o)).collect(),
        (FeatureKind::MateConnector(x), Role::ConnectorBetween) => x.between.iter().map(|o| origin_label(features, o)).collect(),
        (FeatureKind::MateConnector(x), Role::ConnectorPrimary) => x.primary_axis.iter().map(|o| origin_label(features, o)).collect(),
        (FeatureKind::MateConnector(x), Role::ConnectorSecondary) => x.secondary_axis.iter().map(|o| origin_label(features, o)).collect(),
        (FeatureKind::MateConnector(x), Role::ConnectorAlignment) => x.alignment.iter().map(|d| direction_label(features, d)).collect(),
        (FeatureKind::MateConnector(x), Role::ConnectorOwner) => parts(&x.owner.into_iter().collect::<Vec<_>>()),
        _ => return crate::draft_ui::items(features, cache, kind, role),
    })
}

/// The field a list stands for.
pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    Some(match role {
        Role::PatternEntities => AppliedField::PatternEntities,
        Role::PatternDirection => AppliedField::PatternDirection,
        Role::PatternDirection2 => AppliedField::PatternDirection2,
        Role::PatternAxis => AppliedField::PatternAxis,
        Role::PatternPath => AppliedField::PatternPath,
        Role::SkipList => AppliedField::SkipList,
        Role::MirrorPlane => AppliedField::MirrorPlane,
        Role::ConnectorOrigin => AppliedField::ConnectorOrigin,
        Role::ConnectorBetween => AppliedField::ConnectorBetween,
        Role::ConnectorPrimary => AppliedField::ConnectorPrimary,
        Role::ConnectorSecondary => AppliedField::ConnectorSecondary,
        Role::ConnectorAlignment => AppliedField::ConnectorAlignment,
        Role::ConnectorOwner => AppliedField::ConnectorOwner,
        _ => return crate::draft_ui::list_field(role),
    })
}

/// What the mate connector dialog's rows show (the Part Studio feature's or an assembly
/// connector's, P3B.7).
pub(crate) struct ConnectorView {
    pub origin_type: OriginType,
    /// P3.11 (P3.8 judge): the Alignment field shows (the Part Studio feature has one).
    pub alignment: bool,
    pub realign: bool,
    pub move_on: bool,
    pub owner_on: bool,
    pub flip: bool,
    /// X, Y, Z translation and Rotation, with units.
    pub texts: [String; 4],
    /// The Owner field's placeholder ("Select owner entity"; "Select owner instance" in an
    /// assembly).
    pub owner_placeholder: &'static str,
}

/// The mate connector dialog's rows (`ex4-step5.png`): Origin type, Origin entity, Between
/// entity, Realign (Primary axis, Secondary axis), Move (X, Y, Z translation, Rotation), Owner
/// entity, and the flip primary / reorient secondary buttons.
pub(crate) fn connector_rows(b: &mut ChildSpawner, t: &Theme, v: &ConnectorView, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>) {
    let types: Vec<(String, bool)> = OriginType::ALL.iter().map(|o| (o.label().to_string(), true)).collect();
    select_row(b, t, "mate-connector-origin-type", "", Role::ConnectorOriginType, &types, index_of(&OriginType::ALL, &v.origin_type), None);
    list(b, t, "mate-connector-origin-field", "Origin entity", Role::ConnectorOrigin, items_of(Role::ConnectorOrigin), field == AppliedField::ConnectorOrigin);
    if v.origin_type == OriginType::BetweenEntities {
        list(b, t, "mate-connector-between-field", "Between entity", Role::ConnectorBetween, items_of(Role::ConnectorBetween), field == AppliedField::ConnectorBetween);
    }
    if v.alignment {
        // P3.11 (P3.8 judge): the entity whose direction the primary axis takes.
        list(b, t, "mate-connector-alignment-field", "Alignment", Role::ConnectorAlignment, items_of(Role::ConnectorAlignment), field == AppliedField::ConnectorAlignment);
    }
    b.spawn(OptionRow::new("mate-connector-realign", "Realign").checked(v.realign).build(t));
    if v.realign {
        list(b, t, "mate-connector-primary-field", "Primary axis", Role::ConnectorPrimary, items_of(Role::ConnectorPrimary), field == AppliedField::ConnectorPrimary);
        list(b, t, "mate-connector-secondary-field", "Secondary axis", Role::ConnectorSecondary, items_of(Role::ConnectorSecondary), field == AppliedField::ConnectorSecondary);
    }
    b.spawn(OptionRow::new("mate-connector-move", "Move").checked(v.move_on).build(t));
    if v.move_on {
        number(b, t, "mate-connector-x", "X translation", Role::ConnectorX, &v.texts[0], None);
        number(b, t, "mate-connector-y", "Y translation", Role::ConnectorY, &v.texts[1], None);
        number(b, t, "mate-connector-z", "Z translation", Role::ConnectorZ, &v.texts[2], None);
        number(b, t, "mate-connector-rotation", "Rotation", Role::ConnectorRotation, &v.texts[3], None);
    }
    b.spawn(OptionRow::new("mate-connector-owner", "Owner entity").checked(v.owner_on).build(t));
    if v.owner_on {
        list(b, t, "mate-connector-owner-field", v.owner_placeholder, Role::ConnectorOwner, items_of(Role::ConnectorOwner), field == AppliedField::ConnectorOwner);
    }
    // The flip primary / reorient secondary buttons (`ex4-step5.png`).
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), margin: UiRect::new(Val::Px(2.0), Val::ZERO, Val::Px(5.0), Val::Px(2.0)), ..default() }).with_children(|r| {
        icon_button(r, t, "mate-connector-flip", Role::ConnectorFlip, "flip-direction-up", "Flip primary axis", v.flip);
        icon_button(r, t, "mate-connector-reorient", Role::ConnectorReorient, "revolve", "Reorient secondary axis (90°)", false);
    });
}

/// The field that takes the picks after a checkbox or the origin type changed (the new field).
pub(crate) fn field_after(name: &str, on: bool) -> Option<AppliedField> {
    match name {
        "mate-connector-realign-checkbox" if on => Some(AppliedField::ConnectorPrimary),
        "mate-connector-owner-checkbox" if on => Some(AppliedField::ConnectorOwner),
        "mate-connector-realign-checkbox" | "mate-connector-owner-checkbox" => Some(AppliedField::ConnectorOrigin),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Building

/// A small icon button after a list's header (Create selection, the mate connector button).
pub(crate) fn icon_button(b: &mut ChildSpawner, t: &Theme, name: &str, role: Role, icon_name: &'static str, tip: &str, selected: bool) {
    let mut v = cadrs_ui::button::visuals_for(t, cadrs_ui::ButtonVariant::Ghost);
    v.foreground = cadrs_ui::StateColors::all(Color::srgb_u8(0x1e, 0x1e, 0x1e));
    b.spawn((role, IconButton::new(name.to_string(), icon_name).icon_size(15.0).tooltip(tip.to_string()).selected(selected).build(t)))
        .insert(v)
        .entry::<Node>()
        .and_modify(|mut n| {
            n.width = Val::Px(22.0);
            n.height = Val::Px(22.0);
            n.flex_shrink = 0.0;
        });
}

/// A list with a small button beside it.
#[allow(clippy::too_many_arguments)]
fn list_with_button(
    b: &mut ChildSpawner,
    t: &Theme,
    name: &str,
    placeholder: &str,
    role: Role,
    items: Vec<String>,
    active: bool,
    button: (&str, Role, &'static str, &str, bool),
) {
    b.spawn(Node { align_items: AlignItems::FlexStart, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
        r.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() })
            .with_children(|c| list(c, t, name, placeholder, role, items, active));
        r.spawn(Node { margin: UiRect::top(Val::Px(4.0)), ..default() }).with_children(|c| {
            icon_button(c, t, button.0, button.1, button.2, button.3, button.4);
        });
    });
}

/// A count as its field shows it: the expression naming a variable (P3F.4), or the number.
fn count_text(d: &cadrs_core::pattern::LinearDirection) -> String {
    if d.count_expr.is_empty() { d.count.to_string() } else { d.count_expr.clone() }
}

/// "Instance count" with its flip button.
fn count_row(b: &mut ChildSpawner, t: &Theme, name: &str, role: Role, count: String, flip_role: Role, flip: bool) {
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
        r.spawn((role, NumberField::new(name.to_string(), "Instance count").text(count).label_width(96.0).build(t)))
            .entry::<Node>()
            .and_modify(|mut n| n.flex_grow = 1.0);
        flip_button_any(r, t, &format!("{name}-flip"), flip_role, flip, "Opposite direction");
    });
}

/// The Skip instances rows: the checkbox, and while on, "Instances to skip" with CLEAR and the
/// list of grid indices.
fn skip_rows(b: &mut ChildSpawner, t: &Theme, name: &str, on: bool, items: Vec<String>, active: bool) {
    b.spawn(OptionRow::new(format!("{name}-skip"), "Skip instances").chevron().checked(on).build(t));
    if !on {
        return;
    }
    b.spawn(Node {
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        margin: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(2.0), Val::ZERO),
        ..default()
    })
    .with_children(|r| {
        r.spawn(t.text("Instances to skip", t.font_sm, bevy::text::FontWeight::NORMAL, t.muted_foreground));
        r.spawn((
            Role::SkipClear,
            cadrs_ui::Button::new(format!("{name}-skip-clear")).label("CLEAR").small().ghost().build(t),
        ));
    });
    list(b, t, &format!("{name}-skip-field"), "Click instances in the view", Role::SkipList, items, active);
}

fn op_tabs(b: &mut ChildSpawner, t: &Theme, name: &str, op: BooleanOp) {
    let mut strip = TabStrip::new(format!("{name}-operation")).compact();
    for o in BooleanOp::ALL {
        strip = strip.tab(o.label());
    }
    b.spawn((Role::OpTab, strip.selected(index_of(&BooleanOp::ALL, &op)).build(t)));
}

fn merge_rows(b: &mut ChildSpawner, t: &Theme, name: &str, merge_all: bool, items: Vec<String>, active: bool) {
    b.spawn(OptionRow::new(format!("{name}-merge-all"), "Merge with all").checked(merge_all).build(t));
    if !merge_all {
        list(b, t, &format!("{name}-merge-scope-field"), "Merge scope", Role::MergeScope, items, active);
    }
}

/// The rows of the dialog's body.
pub(crate) fn body(b: &mut ChildSpawner, t: &Theme, kind: &FeatureKind, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>) {
    if matches!(kind, FeatureKind::Draft(_)) {
        crate::draft_ui::body(b, t, kind, field, items_of);
        return;
    }
    if matches!(kind, FeatureKind::Transform(_)) {
        crate::transform_ui::body(b, t, kind, field, items_of);
        return;
    }
    if matches!(kind, FeatureKind::Thicken(_) | FeatureKind::Helix(_) | FeatureKind::Fill(_) | FeatureKind::FlatExtrude(_)) {
        crate::surfacing_ui::body(b, t, kind, field, items_of);
        return;
    }
    let Some(name) = name(kind) else { return };
    match kind {
        FeatureKind::Pattern(x) => {
            body_column(b, |b| {
                let types = opts(&PatternType::ALL, |m| m.label(), |_| true);
                select_row(b, t, &format!("{name}-type"), "", Role::PatternType, &types, index_of(&PatternType::ALL, &x.pattern_type), None);
            });
            if x.pattern_type == PatternType::Part {
                op_tabs(b, t, name, x.op);
            }
            body_column(b, |b| {
                let what = seeds_label(x.pattern_type, "pattern");
                let active = field == AppliedField::PatternEntities;
                if x.pattern_type == PatternType::Face {
                    list_with_button(
                        b,
                        t,
                        &format!("{name}-entities-field"),
                        &what,
                        Role::PatternEntities,
                        items_of(Role::PatternEntities),
                        active,
                        (&format!("{name}-create-selection"), Role::CreateSelectionFaces, "plus", "Create selection", false),
                    );
                } else {
                    list(b, t, &format!("{name}-entities-field"), &what, Role::PatternEntities, items_of(Role::PatternEntities), active);
                }
                match x.kind {
                    PatternKind::Linear => {
                        list(b, t, &format!("{name}-direction-field"), "Direction", Role::PatternDirection, items_of(Role::PatternDirection), field == AppliedField::PatternDirection);
                        number(b, t, &format!("{name}-distance"), "Distance", Role::PatternDistance, &x.first.distance_expr, None);
                        count_row(b, t, &format!("{name}-count"), Role::PatternCount, count_text(&x.first), Role::PatternFlip, x.first.flip);
                        b.spawn(OptionRow::new(format!("{name}-centered"), "Centered").checked(x.first.centered).build(t));
                        b.spawn(OptionRow::new(format!("{name}-second"), "Second direction").chevron().checked(x.second_on).build(t));
                        if x.second_on {
                            list(b, t, &format!("{name}-direction2-field"), "Direction", Role::PatternDirection2, items_of(Role::PatternDirection2), field == AppliedField::PatternDirection2);
                            number(b, t, &format!("{name}-distance2"), "Distance", Role::PatternDistance2, &x.second.distance_expr, None);
                            count_row(b, t, &format!("{name}-count2"), Role::PatternCount2, count_text(&x.second), Role::PatternFlip2, x.second.flip);
                            b.spawn(OptionRow::new(format!("{name}-centered2"), "Centered").checked(x.second.centered).build(t));
                        }
                    }
                    PatternKind::Circular => {
                        list_with_button(
                            b,
                            t,
                            &format!("{name}-axis-field"),
                            "Axis of pattern",
                            Role::PatternAxis,
                            items_of(Role::PatternAxis),
                            field == AppliedField::PatternAxis,
                            (&format!("{name}-axis-connector"), Role::ConnectorButton, "mate-connector", "Select mate connector", false),
                        );
                        number(b, t, &format!("{name}-angle"), "Angle", Role::PatternAngle, &x.angle_expr, None);
                        count_row(b, t, &format!("{name}-count"), Role::PatternCount, count_text(&x.first), Role::PatternFlip, x.first.flip);
                        b.spawn(OptionRow::new(format!("{name}-equal-spacing"), "Equal spacing").checked(x.equal_spacing).build(t));
                        b.spawn(OptionRow::new(format!("{name}-centered"), "Centered").checked(x.first.centered).build(t));
                    }
                    PatternKind::Curve => {
                        list(b, t, &format!("{name}-path-field"), "Path to pattern along", Role::PatternPath, items_of(Role::PatternPath), field == AppliedField::PatternPath);
                        count_row(b, t, &format!("{name}-count"), Role::PatternCount, count_text(&x.first), Role::PatternFlip, x.first.flip);
                        b.spawn(OptionRow::new(format!("{name}-equal-spacing"), "Equal spacing").checked(x.equal_spacing).build(t));
                        if !x.equal_spacing {
                            number(b, t, &format!("{name}-distance"), "Distance", Role::PatternDistance, &x.first.distance_expr, None);
                        }
                        b.spawn(OptionRow::new(format!("{name}-tangent"), "Tangent to curve").checked(x.tangent_to_curve).build(t));
                    }
                }
                if x.pattern_type == PatternType::Feature {
                    b.spawn(OptionRow::new(format!("{name}-reapply"), "Reapply features").checked(x.reapply).build(t));
                }
                skip_rows(b, t, name, x.skip_on, items_of(Role::SkipList), field == AppliedField::SkipList);
                if x.pattern_type == PatternType::Part && x.op != BooleanOp::New {
                    merge_rows(b, t, name, x.merge_all, items_of(Role::MergeScope), field == AppliedField::MergeScope);
                }
            });
        }
        FeatureKind::Mirror(x) => {
            body_column(b, |b| {
                let types: Vec<(String, bool)> = PatternType::ALL.iter().map(|m| (m.mirror_label().to_string(), true)).collect();
                select_row(b, t, "mirror-type", "", Role::PatternType, &types, index_of(&PatternType::ALL, &x.mirror_type), None);
            });
            if x.mirror_type == PatternType::Part {
                op_tabs(b, t, "mirror", x.op);
            }
            body_column(b, |b| {
                let what = seeds_label(x.mirror_type, "mirror");
                let active = field == AppliedField::PatternEntities;
                if x.mirror_type == PatternType::Face {
                    list_with_button(
                        b,
                        t,
                        "mirror-entities-field",
                        &what,
                        Role::PatternEntities,
                        items_of(Role::PatternEntities),
                        active,
                        ("mirror-create-selection", Role::CreateSelectionFaces, "plus", "Create selection", false),
                    );
                } else {
                    list(b, t, "mirror-entities-field", &what, Role::PatternEntities, items_of(Role::PatternEntities), active);
                }
                list_with_button(
                    b,
                    t,
                    "mirror-plane-field",
                    "Mirror plane",
                    Role::MirrorPlane,
                    items_of(Role::MirrorPlane),
                    field == AppliedField::MirrorPlane,
                    ("mirror-plane-connector", Role::ConnectorButton, "mate-connector", "Select mate connector", false),
                );
                if x.mirror_type == PatternType::Part && x.op != BooleanOp::New {
                    merge_rows(b, t, "mirror", x.merge_all, items_of(Role::MergeScope), field == AppliedField::MergeScope);
                }
            });
        }
        FeatureKind::MateConnector(x) => body_column(b, |b| {
            let v = ConnectorView {
                origin_type: x.origin_type,
                alignment: true,
                realign: x.realign,
                move_on: x.move_on,
                owner_on: x.owner_on,
                flip: x.flip_primary,
                texts: [x.offset_expr[0].clone(), x.offset_expr[1].clone(), x.offset_expr[2].clone(), x.rotation_expr.clone()],
                owner_placeholder: "Select owner entity",
            };
            connector_rows(b, t, &v, field, items_of);
        }),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    Some(match (kind, role) {
        (FeatureKind::Pattern(x), Role::PatternDistance) => x.first.distance_expr.clone(),
        (FeatureKind::Pattern(x), Role::PatternDistance2) => x.second.distance_expr.clone(),
        (FeatureKind::Pattern(x), Role::PatternCount) => count_text(&x.first),
        (FeatureKind::Pattern(x), Role::PatternCount2) => count_text(&x.second),
        (FeatureKind::Pattern(x), Role::PatternAngle) => x.angle_expr.clone(),
        (FeatureKind::MateConnector(x), Role::ConnectorX) => x.offset_expr[0].clone(),
        (FeatureKind::MateConnector(x), Role::ConnectorY) => x.offset_expr[1].clone(),
        (FeatureKind::MateConnector(x), Role::ConnectorZ) => x.offset_expr[2].clone(),
        (FeatureKind::MateConnector(x), Role::ConnectorRotation) => x.rotation_expr.clone(),
        _ => return crate::draft_ui::number_text(kind, role),
    })
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    Some(match (kind, role) {
        (FeatureKind::Pattern(x), Role::PatternType) => index_of(&PatternType::ALL, &x.pattern_type),
        (FeatureKind::Mirror(x), Role::PatternType) => index_of(&PatternType::ALL, &x.mirror_type),
        (FeatureKind::MateConnector(x), Role::ConnectorOriginType) => index_of(&OriginType::ALL, &x.origin_type),
        _ => return crate::draft_ui::select_index(kind, role),
    })
}

// ---------------------------------------------------------------------------------------------
// Input

pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    if role != Role::OpTab {
        return;
    }
    match k {
        FeatureKind::Pattern(x) => x.op = BooleanOp::ALL[i.min(3)],
        FeatureKind::Mirror(x) => x.op = BooleanOp::ALL[i.min(3)],
        k => crate::surfacing_ui::tab(k, role, i),
    }
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::Pattern(x), Role::PatternType) => x.pattern_type = PatternType::ALL[i.min(2)],
        (FeatureKind::Mirror(x), Role::PatternType) => x.mirror_type = PatternType::ALL[i.min(2)],
        (FeatureKind::MateConnector(x), Role::ConnectorOriginType) => x.origin_type = OriginType::ALL[i.min(1)],
        (k, r) => crate::transform_ui::select(k, r, i),
    }
}

/// A checkbox by its name. Returns its undo label, or `None` if it isn't one of these.
pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    let base = name.strip_suffix("-checkbox")?;
    match k {
        FeatureKind::Pattern(x) => {
            let part = base.split_once("-pattern-").map(|(_, p)| p)?;
            match part {
                "centered" => {
                    x.first.centered = on;
                    Some("Centered")
                }
                "centered2" => {
                    x.second.centered = on;
                    Some("Centered")
                }
                "second" => {
                    x.second_on = on;
                    Some("Second direction")
                }
                "equal-spacing" => {
                    x.equal_spacing = on;
                    Some("Equal spacing")
                }
                "tangent" => {
                    x.tangent_to_curve = on;
                    Some("Tangent to curve")
                }
                "reapply" => {
                    x.reapply = on;
                    Some("Reapply features")
                }
                "skip" => {
                    x.skip_on = on;
                    Some("Skip instances")
                }
                "merge-all" => {
                    x.merge_all = on;
                    Some("Merge with all")
                }
                _ => None,
            }
        }
        FeatureKind::Mirror(x) if base == "mirror-merge-all" => {
            x.merge_all = on;
            Some("Merge with all")
        }
        FeatureKind::MateConnector(x) => match base {
            "mate-connector-flip" => {
                x.flip_primary = on;
                Some("Flip primary axis")
            }
            "mate-connector-realign" => {
                x.realign = on;
                Some("Realign")
            }
            "mate-connector-move" => {
                x.move_on = on;
                Some("Move")
            }
            "mate-connector-owner" => {
                x.owner_on = on;
                Some("Owner entity")
            }
            _ => None,
        },
        k => crate::transform_ui::checkbox(k, name, on),
    }
}

/// True for the checkboxes of these dialogs.
pub(crate) fn is_checkbox(name: &str) -> bool {
    let Some(base) = name.strip_suffix("-checkbox") else { return false };
    let pattern = ["linear-pattern-", "circular-pattern-", "curve-pattern-"]
        .iter()
        .any(|p| base.strip_prefix(p).is_some_and(|rest| {
            matches!(rest, "centered" | "centered2" | "second" | "equal-spacing" | "tangent" | "reapply" | "skip" | "merge-all")
        }));
    pattern
        || matches!(base, "mirror-merge-all" | "mate-connector-flip" | "mate-connector-realign" | "mate-connector-move" | "mate-connector-owner")
        || crate::transform_ui::is_checkbox(name)
}

/// Plain numbers (the instance counts: whole numbers of at least 1).
pub(crate) fn is_count(role: Role) -> bool {
    matches!(role, Role::PatternCount | Role::PatternCount2)
}

/// Numbers that may be zero or negative (a connector's move).
pub(crate) fn is_signed(role: Role) -> bool {
    matches!(role, Role::ConnectorX | Role::ConnectorY | Role::ConnectorZ | Role::ConnectorRotation) || crate::transform_ui::is_signed(role)
}

/// A number's quantity: an angle (true) or a length.
pub(crate) fn is_angle(role: Role) -> bool {
    matches!(role, Role::PatternAngle | Role::ConnectorRotation) || crate::transform_ui::is_angle(role)
}

/// The undo label of a number field.
pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::PatternDistance | Role::PatternDistance2 => "Distance",
        Role::PatternCount | Role::PatternCount2 => "Instance count",
        Role::PatternAngle => "Angle",
        Role::ConnectorX => "X translation",
        Role::ConnectorY => "Y translation",
        Role::ConnectorZ => "Z translation",
        Role::ConnectorRotation => "Rotation",
        r => return crate::transform_ui::number_label(r),
    })
}

/// A number field's value (mm, degrees or a count) and its text.
pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    match (k, role) {
        (FeatureKind::Pattern(x), Role::PatternDistance) => {
            x.first.distance = v;
            x.first.distance_expr = expr;
        }
        (FeatureKind::Pattern(x), Role::PatternDistance2) => {
            x.second.distance = v;
            x.second.distance_expr = expr;
        }
        // P3F.4: a count typed as an expression naming a variable keeps it (`#bolts`).
        (FeatureKind::Pattern(x), Role::PatternCount) => {
            x.first.count = v.round().max(1.0) as u32;
            x.first.count_expr = if expr.contains('#') { expr } else { String::new() };
        }
        (FeatureKind::Pattern(x), Role::PatternCount2) => {
            x.second.count = v.round().max(1.0) as u32;
            x.second.count_expr = if expr.contains('#') { expr } else { String::new() };
        }
        (FeatureKind::Pattern(x), Role::PatternAngle) => {
            x.angle = v;
            x.angle_expr = expr;
        }
        (FeatureKind::MateConnector(x), r @ (Role::ConnectorX | Role::ConnectorY | Role::ConnectorZ)) => {
            let i = match r {
                Role::ConnectorX => 0,
                Role::ConnectorY => 1,
                _ => 2,
            };
            x.offset[i] = v;
            x.offset_expr[i] = expr;
        }
        (FeatureKind::MateConnector(x), Role::ConnectorRotation) => {
            x.rotation = v;
            x.rotation_expr = expr;
        }
        (k, r) => crate::transform_ui::set_number(k, r, v, expr),
    }
}

/// A flip button (the instance counts' direction).
pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    if let FeatureKind::Pattern(x) = k {
        match role {
            Role::PatternFlip => x.first.flip = !x.first.flip,
            Role::PatternFlip2 => x.second.flip = !x.second.flip,
            _ => {}
        }
    }
    crate::transform_ui::flip(k, role);
}

/// An item's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    fn take<T>(v: &mut Vec<T>, i: usize) {
        if i < v.len() {
            v.remove(i);
        }
    }
    match (k, role) {
        (FeatureKind::Pattern(x), Role::PatternEntities) => match x.pattern_type {
            PatternType::Part => take(&mut x.parts, i),
            PatternType::Feature => take(&mut x.features, i),
            PatternType::Face => take(&mut x.faces, i),
        },
        (FeatureKind::Pattern(x), Role::PatternDirection) => x.first.direction = None,
        (FeatureKind::Pattern(x), Role::PatternDirection2) => x.second.direction = None,
        (FeatureKind::Pattern(x), Role::PatternAxis) => x.axis = None,
        (FeatureKind::Pattern(x), Role::PatternPath) => take(&mut x.path, i),
        (FeatureKind::Pattern(x), Role::SkipList) => take(&mut x.skipped, i),
        (FeatureKind::Pattern(x), Role::MergeScope) => take(&mut x.merge_scope, i),
        (FeatureKind::Mirror(x), Role::PatternEntities) => match x.mirror_type {
            PatternType::Part => take(&mut x.parts, i),
            PatternType::Feature => take(&mut x.features, i),
            PatternType::Face => take(&mut x.faces, i),
        },
        (FeatureKind::Mirror(x), Role::MirrorPlane) => x.plane = None,
        (FeatureKind::Mirror(x), Role::MergeScope) => take(&mut x.merge_scope, i),
        (FeatureKind::MateConnector(x), Role::ConnectorOrigin) => x.origin = None,
        (FeatureKind::MateConnector(x), Role::ConnectorBetween) => x.between = None,
        (FeatureKind::MateConnector(x), Role::ConnectorPrimary) => x.primary_axis = None,
        (FeatureKind::MateConnector(x), Role::ConnectorSecondary) => x.secondary_axis = None,
        (FeatureKind::MateConnector(x), Role::ConnectorAlignment) => x.alignment = None,
        (FeatureKind::MateConnector(x), Role::ConnectorOwner) => x.owner = None,
        (k, r) => crate::draft_ui::remove(k, r, i),
    }
}

/// The hole's places list (PS15.2): its sketch points, whole sketches and mate connectors.
pub(crate) fn hole_connector_items(features: &[Feature], x: &cadrs_core::applied::HoleFeature) -> Vec<String> {
    x.connectors.iter().map(|c| c.label(features)).collect()
}

/// The ✕ of a hole's mate connector (after its points and sketches in the list).
pub(crate) fn remove_hole_connector(x: &mut cadrs_core::applied::HoleFeature, i: usize) {
    if i < x.connectors.len() {
        x.connectors.remove(i);
    }
}

