//! Direct edits (Onshape's Delete face and Move face): changes made to a part's faces
//! themselves, whatever features made them, so they work on imported parts too. Their rebuild
//! is in `rebuild/kernel_ops/applied.rs`.
//!
//! - **Delete face**: the faces are removed and, by [`VoidFill`], their neighbours extended to
//!   close the gap (Heal: a fillet, a chamfer, a hole, a boss or a groove taken away), a face
//!   put over each opening (Cap), or the openings left (Leave open: a surface). With **Delete
//!   fillet faces** the fillets next to them go too.
//! - **Move face**, by its [`MoveType`]: **Translate** (the faces moved along a direction by the
//!   distance, the first face's normal when no direction is picked), **Rotate** (turned about an
//!   axis by the angle) or **Offset** (moved along their outward normals by the distance,
//!   negative: inward); the faces round them follow: a wall moved or tilted, a bore's radius
//!   changed.
//! - **Simplify**: the parts of the picked faces with faces and edges on the same surface or
//!   curve merged (an imported part's seams), nothing else changed.

use serde::{Deserialize, Serialize};

pub use cadrs_kernel::VoidFill;

use crate::document::{AxisRef, DirectionRef, FaceRef};

/// Which direct edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DirectEditKind {
    #[default]
    DeleteFace,
    MoveFace,
    /// Faces and edges on the same surface or curve merged across the picked faces' parts.
    Simplify,
}

impl DirectEditKind {
    pub fn label(self) -> &'static str {
        match self {
            DirectEditKind::DeleteFace => "Delete face",
            DirectEditKind::MoveFace => "Move face",
            DirectEditKind::Simplify => "Simplify",
        }
    }
}

/// How Move face moves its faces (the dialog's first dropdown; Onshape's `moveFaceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MoveType {
    /// Along a direction by the distance.
    Translate,
    /// About an axis by the angle.
    Rotate,
    /// Along each face's own normal by the distance (documents from before the types were added
    /// are offsets).
    #[default]
    Offset,
}

impl MoveType {
    /// In the dialog's order (Onshape's).
    pub const ALL: [MoveType; 3] = [MoveType::Translate, MoveType::Rotate, MoveType::Offset];

    pub fn label(self) -> &'static str {
        match self {
            MoveType::Translate => "Translate",
            MoveType::Rotate => "Rotate",
            MoveType::Offset => "Offset",
        }
    }
}

/// How far Move face's Translate and Offset go: by the distance, or up to an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MoveEnd {
    #[default]
    Blind,
    /// Until the first face's plane goes through a vertex, lies on a face parallel to it, or
    /// touches a part.
    UpTo,
}

impl MoveEnd {
    pub const ALL: [MoveEnd; 2] = [MoveEnd::Blind, MoveEnd::UpTo];

    pub fn label(self) -> &'static str {
        match self {
            MoveEnd::Blind => "Blind",
            MoveEnd::UpTo => "Up to entity",
        }
    }
}

/// A Delete face or Move face feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectEditFeature {
    pub kind: DirectEditKind,
    /// The faces deleted or moved.
    pub faces: Vec<FaceRef>,
    /// Delete face: what is done with the void (Onshape's Heal, Cap, Leave open).
    #[serde(default)]
    pub void: VoidFill,
    /// Delete face: the fillets next to the faces are deleted too (Onshape's "Delete fillet
    /// faces"): on for a new Delete face, off for one saved before it was an option (it
    /// rebuilds as it did).
    #[serde(default)]
    pub delete_fillets: bool,
    /// Move face: how it moves the faces.
    #[serde(default)]
    pub move_type: MoveType,
    /// Move face (Translate and Offset): how far the faces move (mm; negative: the other way,
    /// inward for an offset).
    #[serde(default)]
    pub distance: f64,
    #[serde(default)]
    pub distance_expr: String,
    /// Translate and Offset: Blind (by `distance`) or Up to entity (`up_to`, stopping `offset`
    /// short of it; negative: past it).
    #[serde(default)]
    pub end: MoveEnd,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_to: Option<crate::document::UpTo>,
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "zero_mm")]
    pub offset_expr: String,
    /// The fillets next to the faces (and picked fillets tangent to a face moved) taken off and
    /// put back after the move, at their radius (Onshape's "Reapply fillet", on by default).
    #[serde(default = "yes")]
    pub reapply_fillets: bool,
    /// Translate: the direction (an edge, a sketch line, a face's or plane's normal or a mate
    /// connector's Z axis); `None` is the first face's normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<DirectionRef>,
    /// Rotate: the axis (counter-clockwise seen from its tip) and the angle (degrees).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<AxisRef>,
    #[serde(default)]
    pub angle: f64,
    #[serde(default = "zero_deg")]
    pub angle_expr: String,
}

fn zero_mm() -> String {
    "0 mm".into()
}

fn yes() -> bool {
    true
}

fn zero_deg() -> String {
    "0 deg".into()
}

impl Default for DirectEditFeature {
    fn default() -> Self {
        Self {
            kind: DirectEditKind::DeleteFace,
            faces: Vec::new(),
            void: VoidFill::Heal,
            delete_fillets: true,
            move_type: MoveType::Offset,
            end: MoveEnd::Blind,
            up_to: None,
            offset: 0.0,
            offset_expr: zero_mm(),
            reapply_fillets: true,
            distance: 1.0,
            distance_expr: "1 mm".into(),
            direction: None,
            axis: None,
            angle: 0.0,
            angle_expr: zero_deg(),
        }
    }
}

impl DirectEditFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.faces.is_empty() {
            return Some(match self.kind {
                DirectEditKind::DeleteFace => "Select the faces to delete",
                DirectEditKind::MoveFace => "Select the faces to move",
                DirectEditKind::Simplify => "Select a face of each part to simplify",
            });
        }
        if self.kind == DirectEditKind::MoveFace {
            if self.move_type == MoveType::Rotate {
                if self.axis.is_none() {
                    return Some("Select an axis of rotation");
                }
                if !self.angle.is_finite() {
                    return Some("The angle must be a number");
                }
            } else if self.end == MoveEnd::UpTo {
                if self.up_to.is_none() {
                    return Some("Select a vertex or a face to move up to");
                }
                if !self.offset.is_finite() {
                    return Some("The offset distance must be a number");
                }
            } else if !self.distance.is_finite() {
                return Some("The distance must be a number");
            }
        }
        None
    }
}
