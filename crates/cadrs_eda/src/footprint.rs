//! Footprints: the pads, outlines and text a part leaves on the board, and its 3D model.
//!
//! A footprint is always defined as seen from the top, in its own coordinates (Y up, origin
//! at its anchor, angles counter-clockwise). Placing it on the bottom mirrors it (Y negated)
//! and swaps every layer for its bottom twin; see [`FootprintPlacement`].

use crate::graphics::{Shape, Text};
use crate::layer::{Layer, LayerSet, Side};
use crate::units::{Nm, Pt, Size, normalize_deg};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PadKind {
    /// Surface mount: copper on one side.
    #[default]
    Smd,
    /// Plated through-hole.
    ThroughHole,
    /// Non-plated hole (mounting).
    NonPlated,
    /// Copper only, never pasted (edge connector fingers, test points).
    Connector,
}

/// Which corners of a chamfered rectangle are cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Corners {
    pub top_left: bool,
    pub top_right: bool,
    pub bottom_left: bool,
    pub bottom_right: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum PadShape {
    #[default]
    Circle,
    Rect,
    /// A stadium: a rectangle with fully rounded ends.
    Oval,
    /// Corner radius = `ratio` × the smaller side (0 … 0.5).
    RoundRect { ratio: f64 },
    /// Opposite sides shortened by `delta` (x: top/bottom, y: left/right).
    Trapezoid { delta: Size },
    /// Corners cut by `ratio` × the smaller side, the rest optionally rounded.
    Chamfered { ratio: f64, corners: Corners, round_ratio: f64 },
    /// Free shapes on top of an anchor pad (circle or rect).
    Custom { anchor_rect: bool, shapes: Vec<Shape> },
}

/// A pad's hole. Oval when `size.w != size.h`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drill {
    pub size: Size,
    /// The pad's copper centre relative to the hole.
    pub offset: Pt,
}

/// How a pad joins copper zones of its net.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ZoneConnection {
    /// The zone's own setting.
    #[default]
    Inherit,
    None,
    Thermal,
    Solid,
}

/// Per-pad overrides of clearances and zone connection (`None` = inherit).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PadRules {
    pub clearance: Option<Nm>,
    pub mask_margin: Option<Nm>,
    pub paste_margin: Option<Nm>,
    pub paste_ratio: Option<f64>,
    pub zone_connection: ZoneConnection,
    pub thermal_gap: Option<Nm>,
    pub thermal_spoke: Option<Nm>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    pub id: Uuid,
    /// Pad "number" (often a number, sometimes "A1" or empty for mechanical pads).
    pub number: String,
    pub kind: PadKind,
    pub shape: PadShape,
    pub at: Pt,
    pub angle: f64,
    pub size: Size,
    pub drill: Option<Drill>,
    pub layers: LayerSet,
    /// The net on a placed footprint (a name in the board's net list).
    pub net: Option<String>,
    /// The symbol pin's name and type it came from.
    pub pin_function: String,
    pub pin_type: String,
    pub rules: PadRules,
    /// Copper height above the board, for press-fit pins and the 3D view.
    pub die_length: Nm,
}

/// A drawn item of a footprint on one layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FpShape {
    pub id: Uuid,
    pub shape: Shape,
    pub layer: Layer,
}

/// A text item of a footprint (fields are [`FpField`]s).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FpText {
    pub id: Uuid,
    pub text: Text,
    pub layer: Layer,
    /// Reads upright whatever the footprint's rotation.
    pub keep_upright: bool,
}

/// A named value of a footprint (Reference, Value, …), drawn as text on a layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FpField {
    pub name: String,
    pub text: FpText,
}

/// A 3D model: a file in the document's store, and how it sits on the footprint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model3d {
    /// Where it came from (a path or library name), for display and re-linking.
    pub source: String,
    /// The model's content key in the blob store, once imported.
    pub blob: Option<String>,
    /// Millimetres, X/Y in footprint coordinates, Z up from the board surface.
    pub offset: [f64; 3],
    /// Degrees about X, Y, Z.
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
    pub visible: bool,
    pub opacity: f64,
    /// A generated body ([`crate::model3d`]): shown when there is no model file to load (the
    /// built-in footprints have one each).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<crate::model3d::Body>,
}

impl Model3d {
    /// A model from a file `source`, as it sits by default.
    pub fn file(source: &str) -> Model3d {
        Model3d { source: source.into(), blob: None, offset: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3], visible: true, opacity: 1.0, body: None }
    }
}

/// What a footprint is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MountKind {
    #[default]
    Smd,
    ThroughHole,
    /// Neither (logos, fiducials, mechanical).
    Unspecified,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FootprintAttrs {
    pub mount: MountKind,
    pub board_only: bool,
    pub exclude_from_pos: bool,
    pub exclude_from_bom: bool,
    pub dnp: bool,
    pub allow_missing_courtyard: bool,
}

/// A footprint definition (as in a library, top side up).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Footprint {
    /// `library:name`.
    pub id: String,
    pub description: String,
    pub keywords: String,
    pub fields: Vec<FpField>,
    pub attrs: FootprintAttrs,
    pub pads: Vec<Pad>,
    pub shapes: Vec<FpShape>,
    pub texts: Vec<FpText>,
    pub models: Vec<Model3d>,
    /// Keep-out areas and copper zones that belong to the footprint.
    pub zones: Vec<crate::board::Zone>,
}

impl Footprint {
    pub fn field(&self, name: &str) -> Option<&FpField> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn field_mut(&mut self, name: &str) -> Option<&mut FpField> {
        self.fields.iter_mut().find(|f| f.name == name)
    }

    pub fn name(&self) -> &str {
        self.id.rsplit_once(':').map_or(&self.id, |(_, n)| n)
    }
}

/// Where a footprint sits: footprint coordinates → board coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FootprintPlacement {
    pub at: Pt,
    pub angle: f64,
    pub side: Side,
}

impl FootprintPlacement {
    /// Mirror (bottom side), then rotate, then move.
    pub fn apply(&self, p: Pt) -> Pt {
        let p = if self.side == Side::Bottom { p.flip_y() } else { p };
        self.at + p.rotated(self.angle)
    }

    /// A direction in footprint coordinates, on the board.
    pub fn apply_angle(&self, deg: f64) -> f64 {
        let d = if self.side == Side::Bottom { -deg } else { deg };
        normalize_deg(d + self.angle)
    }

    pub fn layer(&self, l: Layer) -> Layer {
        if self.side == Side::Bottom { l.flipped() } else { l }
    }

    pub fn layers(&self, s: LayerSet) -> LayerSet {
        if self.side == Side::Bottom { s.flipped() } else { s }
    }
}
