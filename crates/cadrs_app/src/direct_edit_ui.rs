//! The direct edits in the applied-feature dialogs, reached through the end of
//! [`crate::flat_ui`]'s and [`crate::surfacing_ui`]'s hand-on chains:
//!
//! - **Move face**: the move type (Translate, Rotate, Offset, as Onshape orders them); *Faces to
//!   move*; then Translate's *Direction* (the first face's normal when empty) and *Distance*,
//!   Rotate's *Axis of rotation* and *Angle*, or Offset's *Distance* (each face along its own
//!   normal: a wall slides, a hole's radius changes), each with the opposite direction arrow.
//! - **Delete face**: *Faces to delete* (the faces around them extend to close the gap).
//! - **Simplify**: *Faces of the parts to simplify* (faces and edges on the same surface or
//!   curve merged).
//!
//! The toolbar's Move face button (▾: Move face, Delete face, Simplify) starts them with the
//! selection.

use bevy::prelude::*;
use cadrs_core::direct_edit::{DirectEditFeature, DirectEditKind, MoveType};
use cadrs_core::document::FaceRef;
use cadrs_core::{Feature, FeatureKind};
use cadrs_ui::prelude::*;

use crate::ActiveDocument;
use crate::applied::{AppliedField, AppliedKind};
use crate::applied_dialog::{Role, body_column, index_of, list, number, opts, select_row};
use crate::parts::PartCache;
use crate::transform_ui::{axis_pick, direction_pick, set_or_clear};
use crate::viewport::Pick;

fn face_ref(cache: &PartCache, pick: Pick) -> Option<FaceRef> {
    match crate::applied::entity_of(cache, pick)? {
        cadrs_core::applied::EdgeOrFace::Face(f) => Some(f),
        _ => None,
    }
}

/// A new direct edit's base name, parameters and first field, from the selected faces.
pub fn initial(
    world: &World,
    kind: AppliedKind,
    picked: &[Pick],
) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let AppliedKind::DirectEdit(k) = kind else {
        return None;
    };
    let cache = world.resource::<PartCache>();
    let x = DirectEditFeature {
        kind: k,
        faces: picked.iter().filter_map(|p| face_ref(cache, *p)).collect(),
        ..DirectEditFeature::default()
    };
    Some((
        k.label(),
        FeatureKind::DirectEdit(x),
        AppliedField::DirectEditFaces,
    ))
}

/// The field that takes the picks after the move type changes: Rotate's axis (it has none
/// until picked), else the faces (a field the dialog shows).
pub fn first_field(t: MoveType) -> AppliedField {
    match t {
        MoveType::Rotate => AppliedField::DirectEditAxis,
        MoveType::Translate | MoveType::Offset => AppliedField::DirectEditFaces,
    }
}

/// A pick into a direct edit's fields: a face added to the faces, or removed if it is there; a
/// direction or axis set, or cleared if it is the same. False if it doesn't fit the field.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let FeatureKind::DirectEdit(x) = kind else {
        return false;
    };
    if matches!(
        field,
        AppliedField::DirectEditDirection | AppliedField::DirectEditAxis
    ) {
        let Some(features) = world
            .get_resource::<ActiveDocument>()
            .and_then(|d| d.active_element())
            .map(|e| e.features().to_vec())
        else {
            return false;
        };
        let cache = world.resource::<PartCache>();
        if field == AppliedField::DirectEditDirection {
            let Some(d) = crate::pattern::direction_of(&features, cache, pick) else {
                return false;
            };
            set_or_clear(&mut x.direction, d);
        } else {
            let Some(a) = crate::pattern::axis_of(&features, cache, pick) else {
                return false;
            };
            set_or_clear(&mut x.axis, a);
        }
        return true;
    }
    if field != AppliedField::DirectEditFaces {
        return false;
    }
    let Some(f) = face_ref(world.resource::<PartCache>(), pick) else {
        return false;
    };
    match x.faces.iter().position(|g| g.face == f.face) {
        Some(i) => {
            x.faces.remove(i);
        }
        None => x.faces.push(f),
    }
    true
}

/// `expr` with the opposite sign ("5 mm" ⇄ "-5 mm", "-(#d + 1 mm)" ⇄ "#d + 1 mm").
fn negated(expr: &str) -> String {
    let e = expr.trim();
    if let Some(inner) = e.strip_prefix("-(").and_then(|r| r.strip_suffix(')')) {
        return inner.trim().to_string();
    }
    let simple = |s: &str| !s.contains(['+', '-', '*', '/', '(', ')']);
    match e.strip_prefix('-') {
        Some(rest) if simple(rest) => rest.trim().to_string(),
        _ if simple(e) => format!("-{e}"),
        _ => format!("-({e})"),
    }
}

// ---------------------------------------------------------------------------------------------
// The dialog (called at the end of the applied dialogs' chain)

pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    Some(match kind {
        FeatureKind::DirectEdit(x) => match x.kind {
            DirectEditKind::MoveFace => "move-face",
            DirectEditKind::DeleteFace => "delete-face",
            DirectEditKind::Simplify => "simplify",
        },
        _ => return None,
    })
}

pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    match kind {
        // The flip button's icon shows the direction, so a flip rebuilds the rows, as does the
        // move type (its fields).
        FeatureKind::DirectEdit(x) => Some(format!(
            "direct-edit {:?} {:?} {} {}",
            x.kind,
            x.move_type,
            x.distance.is_sign_negative(),
            x.angle.is_sign_negative()
        )),
        _ => None,
    }
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features
        .iter()
        .find(|f| f.id.0 == op)
        .map_or("part".into(), |f| f.name.clone())
}

pub(crate) fn items(features: &[Feature], kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    match (kind, role) {
        (FeatureKind::DirectEdit(x), Role::DirectEditFaces) => Some(
            x.faces
                .iter()
                .map(|f| format!("Face of {}", op_name(features, f.face.op)))
                .collect(),
        ),
        (FeatureKind::DirectEdit(x), Role::DirectEditDirection) => Some(
            x.direction
                .iter()
                .map(|d| crate::transform_ui::direction_label(features, d))
                .collect(),
        ),
        (FeatureKind::DirectEdit(x), Role::DirectEditAxis) => Some(
            x.axis
                .iter()
                .map(|a| crate::revolve::axis_label(features, a))
                .collect(),
        ),
        _ => None,
    }
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    Some(match role {
        Role::DirectEditFaces => AppliedField::DirectEditFaces,
        Role::DirectEditDirection => AppliedField::DirectEditDirection,
        Role::DirectEditAxis => AppliedField::DirectEditAxis,
        _ => return None,
    })
}

/// What a Move face's direction or axis refers to, shown selected in the view while its dialog
/// is open (its faces are drawn on the parts before it).
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let FeatureKind::DirectEdit(x) = kind else {
        return Vec::new();
    };
    match x.move_type {
        MoveType::Translate => x
            .direction
            .iter()
            .filter_map(|d| direction_pick(cache, d))
            .collect(),
        MoveType::Rotate => x.axis.iter().filter_map(|a| axis_pick(cache, a)).collect(),
        MoveType::Offset => Vec::new(),
    }
}

/// True when the active field takes mate connectors (their hover preview shows).
pub fn takes_connectors(kind: &FeatureKind, field: AppliedField) -> bool {
    matches!(kind, FeatureKind::DirectEdit(_))
        && matches!(
            field,
            AppliedField::DirectEditDirection | AppliedField::DirectEditAxis
        )
}

pub(crate) fn body(
    b: &mut ChildSpawner,
    t: &Theme,
    kind: &FeatureKind,
    field: AppliedField,
    items_of: &dyn Fn(Role) -> Vec<String>,
) {
    let FeatureKind::DirectEdit(x) = kind else {
        return;
    };
    let active = field == AppliedField::DirectEditFaces;
    body_column(b, |b| match x.kind {
        DirectEditKind::MoveFace => {
            select_row(
                b,
                t,
                "move-face-type",
                "",
                Role::DirectEditType,
                &opts(&MoveType::ALL, |m| m.label(), |_| true),
                index_of(&MoveType::ALL, &x.move_type),
                None,
            );
            list(
                b,
                t,
                "move-face-faces-field",
                "Faces to move",
                Role::DirectEditFaces,
                items_of(Role::DirectEditFaces),
                active,
            );
            match x.move_type {
                MoveType::Translate => list(
                    b,
                    t,
                    "move-face-direction-field",
                    "Direction (face normal)",
                    Role::DirectEditDirection,
                    items_of(Role::DirectEditDirection),
                    field == AppliedField::DirectEditDirection,
                ),
                MoveType::Rotate => list(
                    b,
                    t,
                    "move-face-axis-field",
                    "Axis of rotation",
                    Role::DirectEditAxis,
                    items_of(Role::DirectEditAxis),
                    field == AppliedField::DirectEditAxis,
                ),
                MoveType::Offset => {}
            }
            match x.move_type {
                MoveType::Rotate => number(
                    b,
                    t,
                    "move-face-angle",
                    "Angle",
                    Role::DirectEditAngle,
                    &x.angle_expr,
                    Some(x.angle.is_sign_negative()),
                ),
                MoveType::Translate | MoveType::Offset => number(
                    b,
                    t,
                    "move-face-distance",
                    "Distance",
                    Role::DirectEditDistance,
                    &x.distance_expr,
                    Some(x.distance.is_sign_negative()),
                ),
            }
        }
        DirectEditKind::DeleteFace => {
            list(
                b,
                t,
                "delete-face-faces-field",
                "Faces to delete",
                Role::DirectEditFaces,
                items_of(Role::DirectEditFaces),
                active,
            );
        }
        DirectEditKind::Simplify => {
            list(
                b,
                t,
                "simplify-faces-field",
                "Faces of the parts to simplify",
                Role::DirectEditFaces,
                items_of(Role::DirectEditFaces),
                active,
            );
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    match (kind, role) {
        (FeatureKind::DirectEdit(x), Role::DirectEditDistance) => Some(x.distance_expr.clone()),
        (FeatureKind::DirectEdit(x), Role::DirectEditAngle) => Some(x.angle_expr.clone()),
        _ => None,
    }
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    match (kind, role) {
        (FeatureKind::DirectEdit(x), Role::DirectEditType) => {
            Some(index_of(&MoveType::ALL, &x.move_type))
        }
        _ => None,
    }
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    if let (FeatureKind::DirectEdit(x), Role::DirectEditType) = (k, role) {
        x.move_type = MoveType::ALL[i.min(MoveType::ALL.len() - 1)];
    }
}

/// The distance and the angle: either sign (negative moves the faces the other way, inward
/// for an offset).
pub(crate) fn is_signed(role: Role) -> bool {
    matches!(role, Role::DirectEditDistance | Role::DirectEditAngle)
}

pub(crate) fn is_angle(role: Role) -> bool {
    role == Role::DirectEditAngle
}

pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    match role {
        Role::DirectEditDistance => Some("Distance"),
        Role::DirectEditAngle => Some("Angle"),
        _ => None,
    }
}

pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    let FeatureKind::DirectEdit(x) = k else {
        return;
    };
    match role {
        Role::DirectEditDistance => {
            x.distance = v;
            x.distance_expr = expr;
        }
        Role::DirectEditAngle => {
            x.angle = v;
            x.angle_expr = expr;
        }
        _ => {}
    }
}

/// The opposite direction arrow: the angle's sign for a rotation, else the distance's.
pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    let (FeatureKind::DirectEdit(x), Role::Flip) = (k, role) else {
        return;
    };
    if x.move_type == MoveType::Rotate {
        x.angle = -x.angle;
        x.angle_expr = negated(&x.angle_expr);
    } else {
        x.distance = -x.distance;
        x.distance_expr = negated(&x.distance_expr);
    }
}

/// An item's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    let FeatureKind::DirectEdit(x) = k else {
        return;
    };
    match role {
        Role::DirectEditFaces if i < x.faces.len() => {
            x.faces.remove(i);
        }
        Role::DirectEditDirection => x.direction = None,
        Role::DirectEditAxis => x.axis = None,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flip_reverses_a_rotations_angle_and_a_translations_distance() {
        let mut k = FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Rotate,
            angle: 15.0,
            angle_expr: "15 deg".into(),
            ..DirectEditFeature::default()
        });
        flip(&mut k, Role::Flip);
        let FeatureKind::DirectEdit(x) = &mut k else {
            unreachable!()
        };
        assert_eq!((x.angle, x.angle_expr.as_str()), (-15.0, "-15 deg"));
        assert_eq!(x.distance, 1.0, "the distance is left alone");
        x.move_type = MoveType::Translate;
        flip(&mut k, Role::Flip);
        let FeatureKind::DirectEdit(x) = &k else {
            unreachable!()
        };
        assert_eq!((x.distance, x.distance_expr.as_str()), (-1.0, "-1 mm"));
        assert_eq!(x.angle, -15.0, "the angle is left alone");
    }

    #[test]
    fn a_flipped_zero_angle_shows_as_flipped() {
        // Rotate starts at 0°: the flip still turns the arrow (its layout changes).
        let mut k = FeatureKind::DirectEdit(DirectEditFeature {
            kind: DirectEditKind::MoveFace,
            move_type: MoveType::Rotate,
            ..DirectEditFeature::default()
        });
        let before = layout(&k);
        flip(&mut k, Role::Flip);
        assert_ne!(layout(&k), before);
    }

    #[test]
    fn a_type_change_never_leaves_the_picks_on_a_hidden_field() {
        // Back from Rotate (its axis field gone), the faces take the picks again.
        assert_eq!(first_field(MoveType::Rotate), AppliedField::DirectEditAxis);
        assert_eq!(
            first_field(MoveType::Translate),
            AppliedField::DirectEditFaces
        );
        assert_eq!(first_field(MoveType::Offset), AppliedField::DirectEditFaces);
    }

    #[test]
    fn negating_a_distance_keeps_its_expression() {
        assert_eq!(negated("5 mm"), "-5 mm");
        assert_eq!(negated("-5 mm"), "5 mm");
        assert_eq!(negated("#d"), "-#d");
        assert_eq!(negated("#d + 1 mm"), "-(#d + 1 mm)");
        assert_eq!(negated("-(#d + 1 mm)"), "#d + 1 mm");
        assert_eq!(negated("-5 mm + #d"), "-(-5 mm + #d)");
    }
}
