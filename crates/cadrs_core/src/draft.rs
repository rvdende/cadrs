//! Draft (P3.10, PS4.9, X3): Onshape's Draft feature (Neutral plane type) and the extrude's
//! Draft option.
//!
//! The **Draft feature** turns the picked faces by the draft angle about where they meet the
//! neutral plane (a default plane, a Plane feature, a flat face or a mate connector's XY
//! plane); the pull direction is the neutral plane's normal (flipped by the Opposite direction
//! arrow). A positive angle leans the faces in towards the material as they run along the pull
//! direction (a boss narrows as it rises from its base). Tangent propagation (on by default)
//! drafts the faces tangent to the picked ones too. Onshape's **Parting line** type is shown
//! but not built (it needs the parting-line split OCCT's `BRepOffsetAPI_DraftAngle` doesn't do).
//!
//! The **extrude's Draft** ([`ExtrudeDraft`]) drafts its side faces about the sketch plane (the
//! starting offset's plane), each end away from it: a symmetric or two-ended extrude narrows
//! both ways. Solid extrudes only (PS4.9).

use serde::{Deserialize, Serialize};

use crate::document::FaceRef;
use crate::pattern::MirrorPlane;

/// Neutral plane or Parting line (the dialog's tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DraftType {
    #[default]
    NeutralPlane,
    PartingLine,
}

impl DraftType {
    pub const ALL: [DraftType; 2] = [DraftType::NeutralPlane, DraftType::PartingLine];

    pub fn label(self) -> &'static str {
        match self {
            DraftType::NeutralPlane => "Neutral plane",
            DraftType::PartingLine => "Parting line",
        }
    }
}

/// The Draft feature (PS4.9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftFeature {
    #[serde(default)]
    pub draft_type: DraftType,
    /// The neutral plane: its faces stay where they cross it; its normal is the pull direction.
    pub neutral: Option<MirrorPlane>,
    /// Faces to draft.
    pub faces: Vec<FaceRef>,
    /// The draft angle (degrees).
    pub angle: f64,
    pub angle_expr: String,
    /// Opposite direction: the pull direction against the neutral plane's normal.
    #[serde(default)]
    pub flip: bool,
    /// Draft the faces tangent to the picked ones too (on by default).
    #[serde(default = "yes")]
    pub tangent_propagation: bool,
}

fn yes() -> bool {
    true
}

impl Default for DraftFeature {
    fn default() -> Self {
        Self {
            draft_type: DraftType::NeutralPlane,
            neutral: None,
            faces: Vec::new(),
            angle: 3.0,
            angle_expr: "3 deg".into(),
            flip: false,
            tangent_propagation: true,
        }
    }
}

impl DraftFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.draft_type == DraftType::PartingLine {
            return Some("The Parting line draft isn't available; use Neutral plane");
        }
        if self.neutral.is_none() {
            return Some("Select a neutral plane");
        }
        if self.faces.is_empty() {
            return Some("Select faces to draft");
        }
        if !(self.angle.is_finite() && self.angle > 0.0 && self.angle < 90.0) {
            return Some("The draft angle must be between 0 and 90°");
        }
        None
    }
}

/// The extrude's Draft option (PS4.9): an angle and its flip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeDraft {
    /// Degrees, greater than zero.
    pub angle: f64,
    pub expr: String,
    /// Opposite direction: the sides lean out instead of in.
    #[serde(default)]
    pub flip: bool,
}

impl Default for ExtrudeDraft {
    fn default() -> Self {
        Self { angle: 3.0, expr: "3 deg".into(), flip: false }
    }
}

impl ExtrudeDraft {
    /// The signed angle (radians) the kernel takes: positive leans in.
    pub fn signed(&self) -> f64 {
        let a = self.angle.to_radians();
        if self.flip { -a } else { a }
    }
}
