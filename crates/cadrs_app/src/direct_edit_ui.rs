//! The direct edits in the applied-feature dialogs, reached through the end of
//! [`crate::flat_ui`]'s and [`crate::surfacing_ui`]'s hand-on chains:
//!
//! - **Move face**: *Faces to move*; *Distance* with the opposite direction arrow (each face
//!   moves along its own normal: a wall slides, a hole's radius changes).
//! - **Delete face**: *Faces to delete* (the faces around them extend to close the gap).
//! - **Simplify**: *Faces of the parts to simplify* (faces and edges on the same surface or
//!   curve merged).
//!
//! The toolbar's Move face button (▾: Move face, Delete face, Simplify) starts them with the
//! selection.

use bevy::prelude::*;
use cadrs_core::direct_edit::{DirectEditFeature, DirectEditKind};
use cadrs_core::document::FaceRef;
use cadrs_core::{Feature, FeatureKind};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;

use crate::applied::{AppliedField, AppliedKind};
use crate::applied_dialog::{Role, body_column, list, number};
use crate::parts::PartCache;
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

/// A face picked into the faces field: added, or removed if it is there. False if it isn't a face.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let (FeatureKind::DirectEdit(x), AppliedField::DirectEditFaces) = (kind, field) else {
        return false;
    };
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
        // The flip button's icon shows the direction, so a flip rebuilds the rows.
        FeatureKind::DirectEdit(x) => {
            Some(format!("direct-edit {:?} {}", x.kind, x.distance < 0.0))
        }
        _ => None,
    }
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features
        .iter()
        .find(|f| f.id.0 == op)
        .map_or_else(|| tr("model-unknown-part"), |f| f.name.clone())
}

pub(crate) fn items(features: &[Feature], kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    match (kind, role) {
        (FeatureKind::DirectEdit(x), Role::DirectEditFaces) => Some(
            x.faces
                .iter()
                .map(|f| tr_with("common-face-of", &[("name", &op_name(features, f.face.op))]))
                .collect(),
        ),
        _ => None,
    }
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    (role == Role::DirectEditFaces).then_some(AppliedField::DirectEditFaces)
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
            list(
                b,
                t,
                "move-face-faces-field",
                &tr("model-direct-edit-faces-to-move"),
                Role::DirectEditFaces,
                items_of(Role::DirectEditFaces),
                active,
            );
            number(
                b,
                t,
                "move-face-distance",
                &tr("common-distance"),
                Role::DirectEditDistance,
                &x.distance_expr,
                Some(x.distance < 0.0),
            );
        }
        DirectEditKind::DeleteFace => {
            list(
                b,
                t,
                "delete-face-faces-field",
                &tr("model-direct-edit-faces-to-delete"),
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
                &tr("model-direct-edit-faces-to-simplify"),
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
        _ => None,
    }
}

/// The distance: either sign (negative moves the faces inward).
pub(crate) fn is_signed(role: Role) -> bool {
    role == Role::DirectEditDistance
}

pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    (role == Role::DirectEditDistance).then_some("Distance")
}

pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    if let (FeatureKind::DirectEdit(x), Role::DirectEditDistance) = (k, role) {
        x.distance = v;
        x.distance_expr = expr;
    }
}

/// The opposite direction arrow: the distance's sign.
pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    if let (FeatureKind::DirectEdit(x), Role::Flip) = (k, role) {
        x.distance = -x.distance;
        x.distance_expr = negated(&x.distance_expr);
    }
}

/// An item's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    if let (FeatureKind::DirectEdit(x), Role::DirectEditFaces) = (k, role)
        && i < x.faces.len()
    {
        x.faces.remove(i);
    }
}

#[cfg(test)]
mod tests {
    use super::negated;

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
