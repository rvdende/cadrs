//! Direct edits (Onshape's Delete face and Move face): changes made to a part's faces
//! themselves, whatever features made them, so they work on imported parts too. Their rebuild
//! is in `rebuild/kernel_ops/applied.rs`.
//!
//! - **Delete face** (Heal): the faces are removed and their neighbours extended to close the
//!   gap: a fillet, a chamfer, a hole, a boss or a groove taken away.
//! - **Move face**, by its [`MoveType`]: **Translate** (the faces moved along a direction by the
//!   distance, the first face's normal when no direction is picked), **Rotate** (turned about an
//!   axis by the angle) or **Offset** (moved along their outward normals by the distance,
//!   negative: inward); the faces round them follow: a wall moved or tilted, a bore's radius
//!   changed.
//! - **Simplify**: the parts of the picked faces with faces and edges on the same surface or
//!   curve merged (an imported part's seams), nothing else changed.

use serde::{Deserialize, Serialize};

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

/// A Delete face or Move face feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectEditFeature {
    pub kind: DirectEditKind,
    /// The faces deleted or moved.
    pub faces: Vec<FaceRef>,
    /// Move face: how it moves the faces.
    #[serde(default)]
    pub move_type: MoveType,
    /// Move face (Translate and Offset): how far the faces move (mm; negative: the other way,
    /// inward for an offset).
    #[serde(default)]
    pub distance: f64,
    #[serde(default)]
    pub distance_expr: String,
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

fn zero_deg() -> String {
    "0 deg".into()
}

impl Default for DirectEditFeature {
    fn default() -> Self {
        Self {
            kind: DirectEditKind::DeleteFace,
            faces: Vec::new(),
            move_type: MoveType::Offset,
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
            } else if !self.distance.is_finite() {
                return Some("The distance must be a number");
            }
        }
        None
    }
}
