//! Direct edits (Onshape's Delete face and Move face): changes made to a part's faces
//! themselves, whatever features made them, so they work on imported parts too. Their rebuild
//! is in `rebuild/kernel_ops/applied.rs`.
//!
//! - **Delete face** (Heal): the faces are removed and their neighbours extended to close the
//!   gap: a fillet, a chamfer, a hole, a boss or a groove taken away.
//! - **Move face** (Offset): the faces move along their outward normals by the distance
//!   (negative: inward) and the faces round them follow: a wall moved, a bore's radius changed.
//! - **Simplify**: the parts of the picked faces with faces and edges on the same surface or
//!   curve merged (an imported part's seams), nothing else changed.

use serde::{Deserialize, Serialize};

use crate::document::FaceRef;

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

/// A Delete face or Move face feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectEditFeature {
    pub kind: DirectEditKind,
    /// The faces deleted or moved.
    pub faces: Vec<FaceRef>,
    /// Move face: how far the faces move outward (mm; negative moves them inward).
    #[serde(default)]
    pub distance: f64,
    #[serde(default)]
    pub distance_expr: String,
}

impl Default for DirectEditFeature {
    fn default() -> Self {
        Self { kind: DirectEditKind::DeleteFace, faces: Vec::new(), distance: 1.0, distance_expr: "1 mm".into() }
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
        if self.kind == DirectEditKind::MoveFace && !self.distance.is_finite() {
            return Some("The distance must be a number");
        }
        None
    }
}
