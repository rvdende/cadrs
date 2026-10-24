//! The direct edits in the applied-feature dialogs, reached through the end of
//! [`crate::flat_ui`]'s and [`crate::surfacing_ui`]'s hand-on chains:
//!
//! - **Move face**: the move type (Translate, Rotate, Offset, as Onshape orders them); *Faces to
//!   move*; then Translate's *Direction* (the first face's normal when empty), Rotate's *Axis of
//!   rotation*, *Angle* and **Reapply fillet**; Translate and Offset (each face along its own
//!   normal: a wall slides, a hole's radius changes) go Blind, by the *Distance*, or *Up to
//!   entity* (a vertex or a parallel face), stopping the *Offset distance* short of it.
//! - **Delete face**: Heal, Cap or Leave open (what is done with the void); *Faces to delete*;
//!   **Delete fillet faces** (the fillets next to them go too).
//! - **Simplify**: *Faces of the parts to simplify* (faces and edges on the same surface or
//!   curve merged).
//!
//! The toolbar's Move face button (▾: Move face, Delete face, Simplify) starts them with the
//! selection.

use bevy::prelude::*;
use cadrs_core::direct_edit::{DirectEditFeature, DirectEditKind, MoveEnd, MoveType, VoidFill};
use cadrs_core::document::FaceRef;
use cadrs_core::document::{UpTo, VertexRef};
use cadrs_core::{Feature, FeatureKind};
use cadrs_ui::OptionRow;
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
/// The field that takes the picks after the end type changes: Up to entity's.
pub fn end_field(e: MoveEnd) -> AppliedField {
    match e {
        MoveEnd::UpTo => AppliedField::DirectEditUpTo,
        MoveEnd::Blind => AppliedField::DirectEditFaces,
    }
}

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
    if field == AppliedField::DirectEditUpTo {
        let cache = world.resource::<PartCache>();
        let to = match pick {
            Pick::Face(..) => face_ref(cache, pick).map(UpTo::Face),
            Pick::Vertex(part, vertex) => cache
                .part(part)
                .and_then(|p| p.solid.vertex(&vertex))
                .map(|v| {
                    UpTo::Vertex(VertexRef {
                        part,
                        vertex,
                        point: v.point,
                    })
                }),
            _ => None,
        };
        let Some(to) = to else { return false };
        set_or_clear(&mut x.up_to, to);
        return true;
    }
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
            "direct-edit {:?} {:?} {:?} {:?} {} {} {} {}",
            x.kind,
            x.move_type,
            x.end,
            x.void,
            x.delete_fillets,
            x.reapply_fillets,
            // The distance as the Offset arrow reads it; the angle (no arrow) by its sign, so a
            // flipped 0° shows as flipped.
            x.distance < 0.0,
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
        (FeatureKind::DirectEdit(x), Role::DirectEditUpTo) => Some(
            x.up_to
                .iter()
                .map(|t| match t {
                    UpTo::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
                    UpTo::Vertex(v) => {
                        format!("Vertex of {}", op_name(features, v.vertex.faces[0].op))
                    }
                    UpTo::Part(_) => "Part".to_string(),
                })
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
        Role::DirectEditUpTo => AppliedField::DirectEditUpTo,
        _ => return None,
    })
}

/// What a Move face's direction or axis refers to, shown selected in the view while its dialog
/// is open (its faces are drawn on the parts before it).
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let FeatureKind::DirectEdit(x) = kind else {
        return Vec::new();
    };
    let mut out: Vec<Pick> = match x.move_type {
        MoveType::Translate => x
            .direction
            .iter()
            .filter_map(|d| direction_pick(cache, d))
            .collect(),
        MoveType::Rotate => x.axis.iter().filter_map(|a| axis_pick(cache, a)).collect(),
        MoveType::Offset => Vec::new(),
    };
    if x.move_type != MoveType::Rotate && x.end == MoveEnd::UpTo {
        out.extend(x.up_to.iter().filter_map(|t| {
            match t {
                UpTo::Face(f) => cache
                    .parts
                    .iter()
                    .find(|p| p.solid.face(&f.face).is_some())
                    .map(|p| Pick::Face(p.id, f.face)),
                UpTo::Vertex(v) => Some(Pick::Vertex(v.part, v.vertex)),
                UpTo::Part(p) => Some(Pick::Part(*p)),
            }
        }));
    }
    out
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
            faces_list(
                b,
                t,
                "move-face",
                "Faces to move",
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
            match (x.move_type, x.end) {
                (MoveType::Rotate, _) => {
                    number(
                        b,
                        t,
                        "move-face-angle",
                        "Angle",
                        Role::DirectEditAngle,
                        &x.angle_expr,
                        Some(x.angle.is_sign_negative()),
                    );
                    b.spawn(
                        OptionRow::new("move-face-reapply-fillets", "Reapply fillet")
                            .checked(x.reapply_fillets)
                            .build(t),
                    );
                }
                (_, end) => {
                    select_row(
                        b,
                        t,
                        "move-face-end",
                        "",
                        Role::DirectEditEnd,
                        &opts(&MoveEnd::ALL, |e| e.label(), |_| true),
                        index_of(&MoveEnd::ALL, &end),
                        None,
                    );
                    if end == MoveEnd::UpTo {
                        list(
                            b,
                            t,
                            "move-face-up-to-field",
                            "Up to entity",
                            Role::DirectEditUpTo,
                            items_of(Role::DirectEditUpTo),
                            field == AppliedField::DirectEditUpTo,
                        );
                        number(
                            b,
                            t,
                            "move-face-offset",
                            "Offset distance",
                            Role::DirectEditOffset,
                            &x.offset_expr,
                            None,
                        );
                    } else {
                        number(
                            b,
                            t,
                            "move-face-distance",
                            "Distance",
                            Role::DirectEditDistance,
                            &x.distance_expr,
                            Some(x.distance < 0.0),
                        );
                    }
                }
            }
        }
        DirectEditKind::DeleteFace => {
            select_row(
                b,
                t,
                "delete-face-void",
                "",
                Role::DirectEditVoid,
                &opts(&VOIDS, void_label, |_| true),
                index_of(&VOIDS, &x.void),
                None,
            );
            faces_list(
                b,
                t,
                "delete-face",
                "Faces to delete",
                items_of(Role::DirectEditFaces),
                active,
            );
            b.spawn(
                OptionRow::new("delete-face-fillets", "Delete fillet faces")
                    .checked(x.delete_fillets)
                    .build(t),
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

/// The faces field, with Create selection beside it (the faces of a pocket, a protrusion, a
/// hole or the fillets in one go: an imported part's features).
fn faces_list(
    b: &mut ChildSpawner,
    t: &Theme,
    name: &str,
    label: &str,
    items: Vec<String>,
    active: bool,
) {
    crate::pattern_dialog::list_with_button(
        b,
        t,
        &format!("{name}-faces-field"),
        label,
        Role::DirectEditFaces,
        items,
        active,
        (
            &format!("{name}-create-selection"),
            Role::CreateSelectionFaces,
            "plus",
            "Create selection",
            false,
        ),
    );
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    match (kind, role) {
        (FeatureKind::DirectEdit(x), Role::DirectEditDistance) => Some(x.distance_expr.clone()),
        (FeatureKind::DirectEdit(x), Role::DirectEditAngle) => Some(x.angle_expr.clone()),
        (FeatureKind::DirectEdit(x), Role::DirectEditOffset) => Some(x.offset_expr.clone()),
        _ => None,
    }
}

/// Delete face's treatments of the void, in Onshape's order.
const VOIDS: [VoidFill; 3] = [VoidFill::Heal, VoidFill::Cap, VoidFill::Open];

fn void_label(v: VoidFill) -> &'static str {
    match v {
        VoidFill::Heal => "Heal",
        VoidFill::Cap => "Cap",
        VoidFill::Open => "Leave open",
    }
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    match (kind, role) {
        (FeatureKind::DirectEdit(x), Role::DirectEditType) => {
            Some(index_of(&MoveType::ALL, &x.move_type))
        }
        (FeatureKind::DirectEdit(x), Role::DirectEditVoid) => Some(index_of(&VOIDS, &x.void)),
        (FeatureKind::DirectEdit(x), Role::DirectEditEnd) => Some(index_of(&MoveEnd::ALL, &x.end)),
        _ => None,
    }
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::DirectEdit(x), Role::DirectEditType) => {
            x.move_type = MoveType::ALL[i.min(MoveType::ALL.len() - 1)];
        }
        (FeatureKind::DirectEdit(x), Role::DirectEditVoid) => {
            x.void = VOIDS[i.min(VOIDS.len() - 1)];
        }
        (FeatureKind::DirectEdit(x), Role::DirectEditEnd) => {
            x.end = MoveEnd::ALL[i.min(MoveEnd::ALL.len() - 1)];
        }
        _ => {}
    }
}

pub(crate) fn is_checkbox(name: &str) -> bool {
    matches!(
        name,
        "delete-face-fillets-checkbox" | "move-face-reapply-fillets-checkbox"
    )
}

/// A checkbox by its name: its undo label, or `None` if it isn't one of these.
pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    match (k, name) {
        (FeatureKind::DirectEdit(x), "delete-face-fillets-checkbox") => {
            x.delete_fillets = on;
            Some("Delete fillet faces")
        }
        (FeatureKind::DirectEdit(x), "move-face-reapply-fillets-checkbox") => {
            x.reapply_fillets = on;
            Some("Reapply fillet")
        }
        _ => None,
    }
}

/// The distance and the angle: either sign (negative moves the faces the other way, inward
/// for an offset).
pub(crate) fn is_signed(role: Role) -> bool {
    matches!(
        role,
        Role::DirectEditDistance | Role::DirectEditAngle | Role::DirectEditOffset
    )
}

pub(crate) fn is_angle(role: Role) -> bool {
    role == Role::DirectEditAngle
}

pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    match role {
        Role::DirectEditDistance => Some("Distance"),
        Role::DirectEditAngle => Some("Angle"),
        Role::DirectEditOffset => Some("Offset distance"),
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
        Role::DirectEditOffset => {
            x.offset = v;
            x.offset_expr = expr;
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
        Role::DirectEditUpTo => x.up_to = None,
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
