//! Frame: structural members (beams, tubes, T-slot extrusions) along sketch curves and edges,
//! one part each, joined at their corners, with a cut list (issue #266; what other CAD tools call
//! weldments, structural members or frames).
//!
//! A Frame feature takes:
//!
//! - **Path**: sketch curves, whole sketches, edges of parts and curve features (a Helix). Each
//!   curve is a member; with **Merge tangent segments** a run of curves that meet tangentially is
//!   one member (a tube bent round an arc), and collinear lines are one straight member.
//! - **Profile**: a standard section ([`ProfileFamily`], [`sizes`]): a T-slot extrusion (20, 30
//!   and 40 series), square, rectangular and round tube, square, round and flat bar, an equal
//!   angle, a channel (UPN) or an I-beam (IPE). Or **Custom**: the regions of a sketch, its
//!   origin on the path.
//! - **Alignment** ([`Anchor`]): which point of the section's box lies on the path (its middle,
//!   a corner or the middle of a side), then the section **mirrored** (left for right) and
//!   turned by an **angle** about the path. The section's up is the path's sketch normal (the
//!   world's Z for an edge), its right `up × along`.
//! - **Corners** ([`CornerType`]): where two members end at one point they are mitred, butted
//!   (one runs on to the other's far side, the other is cut square against it; **flip** swaps
//!   them), coped (as butted, the second cut to the first's shape: a tube saddled round a tube) or
//!   left as they are. **Corner overrides** give single corners another treatment. A member that
//!   ends on another's span (a T) is cut against it; where three or more members end at one
//!   point two in line run through, else the two at the widest angle are joined as a corner of
//!   two (an override's turn picks another pair), and the others are cut against both and
//!   against each other along their bisectors.
//! - **Trim to parts** cuts members back by other parts (the larger piece kept, or the other
//!   side); **End caps** close hollow members' free ends; **Gussets** fill two-member corners
//!   (centred, or flush with the section's top or bottom, their inner corner chamfered), and an
//!   override turns one corner's gusset on or off.
//!
//! Every member is exact B-rep: a straight member is its section extruded, a curved one swept
//! along its curves (straight run-ons at its ends where a corner needs them), each corner a cut
//! by a plane (or a subtraction, coped). The rebuild is `rebuild/kernel_ops/frame.rs`.
//!
//! The standard sections are drawn from their makers' and standards' nominal dimensions:
//! tubes' corner radii as EN 10219 cold-formed sections (outside 2t, inside t), angles (EN
//! 10056-1) and I-beams (EN 10365) with their root and toe radii, channels with parallel
//! flanges of the UPN's mean thickness (a UPN's flanges taper at 8 %), and T-slot extrusions as
//! their outline: slots and centre bores, not their hollow cores (their mass is high).

use serde::{Deserialize, Serialize};

use crate::advanced::PathRef;
use crate::ids::{FeatureId, PartId};
use cadrs_kernel::{Curve2, Loop, Region};
use cadrs_sketch::{PointId, Vec3};
use nalgebra::Point2;

/// The kind of section a frame's members have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ProfileFamily {
    /// An aluminium T-slot extrusion (B-type slots).
    #[default]
    TSlot,
    SquareTube,
    RectTube,
    RoundTube,
    SquareBar,
    RoundBar,
    FlatBar,
    /// An equal-leg angle (EN 10056-1).
    Angle,
    /// A UPN channel.
    Channel,
    /// An IPE I-beam.
    IBeam,
    /// The regions of a sketch.
    Custom,
}

impl ProfileFamily {
    pub const ALL: [ProfileFamily; 11] = [
        ProfileFamily::TSlot,
        ProfileFamily::SquareTube,
        ProfileFamily::RectTube,
        ProfileFamily::RoundTube,
        ProfileFamily::SquareBar,
        ProfileFamily::RoundBar,
        ProfileFamily::FlatBar,
        ProfileFamily::Angle,
        ProfileFamily::Channel,
        ProfileFamily::IBeam,
        ProfileFamily::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ProfileFamily::TSlot => "T-slot extrusion",
            ProfileFamily::SquareTube => "Square tube",
            ProfileFamily::RectTube => "Rectangular tube",
            ProfileFamily::RoundTube => "Round tube",
            ProfileFamily::SquareBar => "Square bar",
            ProfileFamily::RoundBar => "Round bar",
            ProfileFamily::FlatBar => "Flat bar",
            ProfileFamily::Angle => "Angle",
            ProfileFamily::Channel => "Channel (UPN)",
            ProfileFamily::IBeam => "I-beam (IPE)",
            ProfileFamily::Custom => "Custom (sketch)",
        }
    }
}

/// Which point of the section's bounding box lies on the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    /// The section's own origin (a custom sketch's origin; a standard section's middle, an
    /// angle's heel).
    Origin,
}

impl Anchor {
    pub const ALL: [Anchor; 10] = [
        Anchor::Center,
        Anchor::Top,
        Anchor::Bottom,
        Anchor::Left,
        Anchor::Right,
        Anchor::TopLeft,
        Anchor::TopRight,
        Anchor::BottomLeft,
        Anchor::BottomRight,
        Anchor::Origin,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Anchor::TopLeft => "Top left",
            Anchor::Top => "Top",
            Anchor::TopRight => "Top right",
            Anchor::Left => "Left",
            Anchor::Center => "Center",
            Anchor::Right => "Right",
            Anchor::BottomLeft => "Bottom left",
            Anchor::Bottom => "Bottom",
            Anchor::BottomRight => "Bottom right",
            Anchor::Origin => "Section origin",
        }
    }

    /// Where it is in the box, as fractions from its lower left (`None`: the origin).
    fn fractions(self) -> Option<(f64, f64)> {
        Some(match self {
            Anchor::TopLeft => (0.0, 1.0),
            Anchor::Top => (0.5, 1.0),
            Anchor::TopRight => (1.0, 1.0),
            Anchor::Left => (0.0, 0.5),
            Anchor::Center => (0.5, 0.5),
            Anchor::Right => (1.0, 0.5),
            Anchor::BottomLeft => (0.0, 0.0),
            Anchor::Bottom => (0.5, 0.0),
            Anchor::BottomRight => (1.0, 0.0),
            Anchor::Origin => return None,
        })
    }
}

/// How two members ending at one point are joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CornerType {
    /// Both cut on the plane that halves the angle between them.
    #[default]
    Miter,
    /// The first runs on to the second's far side; the second is cut square against it.
    Butt,
    /// As Butt, the second cut to the first's shape.
    CopedButt,
    /// Each ends square where its curve does.
    None,
}

impl CornerType {
    pub const ALL: [CornerType; 4] = [
        CornerType::Miter,
        CornerType::Butt,
        CornerType::CopedButt,
        CornerType::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CornerType::Miter => "Miter",
            CornerType::Butt => "Butt",
            CornerType::CopedButt => "Coped butt",
            CornerType::None => "None",
        }
    }
}

/// A point of a sketch, as a corner override names its corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SketchPointRef {
    pub sketch: FeatureId,
    pub point: PointId,
}

/// One corner joined otherwise than the frame's corners are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CornerOverride {
    /// The sketch point at the corner, if it was picked as one (the corner follows it).
    #[serde(default)]
    pub point: Option<SketchPointRef>,
    /// Where the corner was when it was picked (a vertex's, or the sketch point's if it is gone).
    pub at: Vec3,
    pub corner: CornerType,
    /// Butt and coped: the other member runs on.
    #[serde(default)]
    pub flip: bool,
    /// Where three or more members end: which runs on, counted from the longest (wrapping).
    #[serde(default)]
    pub turn: u8,
    /// A gusset here or not (`None`: as the frame's Gussets).
    #[serde(default)]
    pub gusset: Option<bool>,
}

/// Where a gusset sits across the corner's plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GussetPosition {
    /// In the middle of the section.
    #[default]
    Center,
    /// Flush with the section's top (its up side).
    Top,
    /// Flush with its bottom.
    Bottom,
}

impl GussetPosition {
    pub const ALL: [GussetPosition; 3] = [
        GussetPosition::Center,
        GussetPosition::Top,
        GussetPosition::Bottom,
    ];
}

/// A Frame feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameFeature {
    /// The curves the members run along.
    #[serde(default)]
    pub path: Vec<PathRef>,
    #[serde(default)]
    pub family: ProfileFamily,
    /// The size's name in [`sizes`] of the family ("20x20", "40x40x3", "IPE 100").
    #[serde(default)]
    pub size: String,
    /// A custom section's sketch (its closed regions; choosing Custom puts its origin on the
    /// path, `Anchor::Origin`).
    #[serde(default)]
    pub sketch: Option<FeatureId>,
    #[serde(default)]
    pub anchor: Anchor,
    /// The section turned about the path (degrees, counter-clockwise looking back along it).
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub angle_expr: String,
    /// The section mirrored left for right (before it is placed and turned).
    #[serde(default)]
    pub mirror: bool,
    /// Curves meeting tangentially are one member.
    #[serde(default = "yes")]
    pub merge_tangent: bool,
    /// How members ending at one point are joined. Butt and coped: the longer member runs on
    /// (of two as long, the one along the path's first member), the other is cut against it.
    /// At three or more, the main pair (the widest angle) is joined so and the others are cut
    /// against both.
    #[serde(default)]
    pub corner: CornerType,
    /// Butt and coped corners: the shorter member runs on, not the longer.
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub overrides: Vec<CornerOverride>,
    /// Parts the members are trimmed by (a wall, a plate, another frame's members): each
    /// member less what overlaps them, the piece holding most of it kept.
    #[serde(default)]
    pub trim: Vec<PartId>,
    /// Keep the other side of a trim (all but the largest piece).
    #[serde(default)]
    pub trim_flip: bool,
    /// Plates closing the members' free ends, as thick as `cap_thickness`, outside the member
    /// (only of sections with holes: tubes, T-slot extrusions, custom sections with holes).
    #[serde(default)]
    pub end_caps: bool,
    #[serde(default = "cap_thickness")]
    pub cap_thickness: f64,
    #[serde(default = "cap_thickness_expr")]
    pub cap_thickness_expr: String,
    /// A triangular plate in the angle of every corner of two members (an override can add or
    /// leave out one): its legs `gusset_size` along the members' inner sides from where they
    /// meet, `gusset_thickness` thick, across the section as `gusset_position` says.
    #[serde(default)]
    pub gussets: bool,
    #[serde(default = "gusset_size")]
    pub gusset_size: f64,
    #[serde(default = "gusset_size_expr")]
    pub gusset_size_expr: String,
    #[serde(default = "gusset_thickness")]
    pub gusset_thickness: f64,
    #[serde(default = "gusset_thickness_expr")]
    pub gusset_thickness_expr: String,
    #[serde(default)]
    pub gusset_position: GussetPosition,
    /// The gusset's inner corner cut back this far along each leg (clear of the weld), 0: none.
    #[serde(default)]
    pub gusset_chamfer: f64,
    #[serde(default = "zero_mm")]
    pub gusset_chamfer_expr: String,
}

fn zero_mm() -> String {
    "0 mm".into()
}

fn cap_thickness() -> f64 {
    3.0
}
fn cap_thickness_expr() -> String {
    "3 mm".into()
}
fn gusset_size() -> f64 {
    60.0
}
fn gusset_size_expr() -> String {
    "60 mm".into()
}
fn gusset_thickness() -> f64 {
    5.0
}
fn gusset_thickness_expr() -> String {
    "5 mm".into()
}

fn yes() -> bool {
    true
}

impl Default for FrameFeature {
    fn default() -> Self {
        Self {
            path: Vec::new(),
            family: ProfileFamily::TSlot,
            size: "20x20".into(),
            sketch: None,
            anchor: Anchor::Center,
            angle: 0.0,
            angle_expr: "0 deg".into(),
            mirror: false,
            merge_tangent: true,
            // Aluminium extrusion (the default section) is butted.
            corner: CornerType::Butt,
            flip: false,
            overrides: Vec::new(),
            trim: Vec::new(),
            trim_flip: false,
            end_caps: false,
            cap_thickness: cap_thickness(),
            cap_thickness_expr: cap_thickness_expr(),
            gussets: false,
            gusset_size: gusset_size(),
            gusset_size_expr: gusset_size_expr(),
            gusset_thickness: gusset_thickness(),
            gusset_thickness_expr: gusset_thickness_expr(),
            gusset_position: GussetPosition::Center,
            gusset_chamfer: 0.0,
            gusset_chamfer_expr: zero_mm(),
        }
    }
}

impl FrameFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.path.is_empty() {
            return Some("Select sketch curves or edges for the frame's members");
        }
        if self.family == ProfileFamily::Custom {
            if self.sketch.is_none() {
                return Some("Select the sketch of the custom profile");
            }
        } else if section(self.family, &self.size).is_none() {
            return Some("Select a profile size");
        }
        if !self.angle.is_finite() {
            return Some("The angle must be a number");
        }
        if self.end_caps
            && self.cap_thickness.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        {
            return Some("The end caps' thickness must be greater than zero");
        }
        if self.gussets
            && [self.gusset_size, self.gusset_thickness]
                .iter()
                .any(|v| v.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater))
        {
            return Some("The gussets' size and thickness must be greater than zero");
        }
        if self.gussets && !(self.gusset_chamfer >= 0.0 && self.gusset_chamfer < self.gusset_size) {
            return Some("A gusset's chamfer must be less than its size");
        }
        None
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out = Vec::new();
        for p in &self.path {
            match p {
                PathRef::Edge(e) => out.push(FeatureId(e.edge.op())),
                PathRef::SketchCurve { sketch, .. } | PathRef::Sketch(sketch) => out.push(*sketch),
                PathRef::Curve(f) => out.push(*f),
            }
        }
        out.extend(self.sketch);
        out.extend(
            self.overrides
                .iter()
                .filter_map(|o| o.point.map(|p| p.sketch)),
        );
        out.extend(self.trim.iter().map(|p| p.feature));
        out.sort_by_key(|f| f.0);
        out.dedup();
        out
    }

    /// The section's name in the cut list: "Square tube 40x40x3", "IPE 100", "Custom (Sketch
    /// 2)" (the custom sketch's name given).
    pub fn profile_label(&self, custom_name: Option<&str>) -> String {
        match self.family {
            ProfileFamily::Custom => format!("Custom ({})", custom_name.unwrap_or("sketch")),
            ProfileFamily::Channel | ProfileFamily::IBeam => self.size.clone(),
            ProfileFamily::TSlot => format!("T-slot {}", self.size),
            f => format!("{} {}", f.label(), self.size),
        }
    }

    /// Switches the family, keeping the size if it has it, else its first (or a commonly used
    /// one).
    /// A custom section sits by its own origin (as drawn), a standard one by its middle.
    pub fn set_family(&mut self, family: ProfileFamily) {
        if family == ProfileFamily::Custom && self.family != ProfileFamily::Custom {
            self.anchor = Anchor::Origin;
        } else if family != ProfileFamily::Custom && self.anchor == Anchor::Origin {
            self.anchor = Anchor::Center;
        }
        self.family = family;
        let names = size_names(family);
        if !names.contains(&self.size.as_str()) {
            self.size = default_size(family).to_string();
        }
    }
}

/// The size a family starts with.
pub fn default_size(family: ProfileFamily) -> &'static str {
    match family {
        ProfileFamily::TSlot => "20x20",
        ProfileFamily::SquareTube => "40x40x3",
        ProfileFamily::RectTube => "60x40x3",
        ProfileFamily::RoundTube => "33.7x3.2",
        ProfileFamily::SquareBar => "20",
        ProfileFamily::RoundBar => "20",
        ProfileFamily::FlatBar => "40x5",
        ProfileFamily::Angle => "40x40x4",
        ProfileFamily::Channel => "UPN 100",
        ProfileFamily::IBeam => "IPE 100",
        ProfileFamily::Custom => "",
    }
}

// ---------------------------------------------------------------------------------------------
// The sizes

/// A T-slot series: cell size, slot opening, lip, cavity width, straight cavity wall, cavity
/// depth, centre bore.
struct TSlotSeries {
    cell: f64,
    opening: f64,
    lip: f64,
    width: f64,
    wall: f64,
    depth: f64,
    bore: f64,
}

const SLOT_20: TSlotSeries = TSlotSeries {
    cell: 20.0,
    opening: 6.2,
    lip: 1.8,
    width: 11.0,
    wall: 1.0,
    depth: 6.1,
    bore: 4.2,
};
const SLOT_30: TSlotSeries = TSlotSeries {
    cell: 30.0,
    opening: 8.2,
    lip: 2.2,
    width: 16.5,
    wall: 1.5,
    depth: 9.0,
    bore: 6.8,
};
const SLOT_40: TSlotSeries = TSlotSeries {
    cell: 40.0,
    opening: 8.2,
    lip: 4.3,
    width: 20.0,
    wall: 2.0,
    depth: 12.2,
    bore: 6.8,
};

/// Inch T-slot (the 10 and 15 series): 1 in and 1.5 in cells.
const SLOT_10: TSlotSeries = TSlotSeries {
    cell: 25.4,
    opening: 6.5,
    lip: 2.2,
    width: 14.0,
    wall: 1.3,
    depth: 7.6,
    bore: 5.2,
};
const SLOT_15: TSlotSeries = TSlotSeries {
    cell: 38.1,
    opening: 8.1,
    lip: 4.4,
    width: 19.0,
    wall: 1.8,
    depth: 11.4,
    bore: 7.9,
};

/// The 45 series (slot 10).
const SLOT_45: TSlotSeries = TSlotSeries {
    cell: 45.0,
    opening: 10.2,
    lip: 4.5,
    width: 22.0,
    wall: 2.5,
    depth: 13.5,
    bore: 8.5,
};

/// T-slot sizes: name, series, cells across and up, and the catalogue's mass (kg/m; the
/// outline drawn here has no hollow cores, so its volume would overstate it).
const T_SLOTS: [(&str, &TSlotSeries, u32, u32, f64); 12] = [
    ("20x20", &SLOT_20, 1, 1, 0.50),
    ("20x40", &SLOT_20, 2, 1, 0.88),
    ("30x30", &SLOT_30, 1, 1, 0.92),
    ("30x60", &SLOT_30, 2, 1, 1.65),
    ("40x40", &SLOT_40, 1, 1, 1.50),
    ("40x80", &SLOT_40, 2, 1, 2.80),
    ("45x45", &SLOT_45, 1, 1, 1.90),
    ("45x90", &SLOT_45, 2, 1, 3.40),
    ("1010 (1 in)", &SLOT_10, 1, 1, 0.74),
    ("1020 (1x2 in)", &SLOT_10, 2, 1, 1.34),
    ("1515 (1.5 in)", &SLOT_15, 1, 1, 1.49),
    ("1530 (1.5x3 in)", &SLOT_15, 2, 1, 2.68),
];

/// Square tubes: side, wall.
const SQUARE_TUBES: [(&str, f64, f64); 9] = [
    ("20x20x2", 20.0, 2.0),
    ("25x25x2", 25.0, 2.0),
    ("30x30x2", 30.0, 2.0),
    ("40x40x2", 40.0, 2.0),
    ("40x40x3", 40.0, 3.0),
    ("50x50x3", 50.0, 3.0),
    ("60x60x4", 60.0, 4.0),
    ("80x80x4", 80.0, 4.0),
    ("100x100x5", 100.0, 5.0),
];

/// Rectangular tubes: width, height, wall.
const RECT_TUBES: [(&str, f64, f64, f64); 7] = [
    ("40x20x2", 40.0, 20.0, 2.0),
    ("50x25x2", 50.0, 25.0, 2.0),
    ("50x30x3", 50.0, 30.0, 3.0),
    ("60x40x3", 60.0, 40.0, 3.0),
    ("80x40x3", 80.0, 40.0, 3.0),
    ("100x50x4", 100.0, 50.0, 4.0),
    ("120x60x4", 120.0, 60.0, 4.0),
];

/// Round tubes (EN 10220 outside diameters): outside diameter, wall.
const ROUND_TUBES: [(&str, f64, f64); 8] = [
    ("21.3x2.6", 21.3, 2.6),
    ("26.9x2.6", 26.9, 2.6),
    ("33.7x3.2", 33.7, 3.2),
    ("42.4x3.2", 42.4, 3.2),
    ("48.3x3.2", 48.3, 3.2),
    ("60.3x3.6", 60.3, 3.6),
    ("76.1x3.6", 76.1, 3.6),
    ("88.9x4", 88.9, 4.0),
];

/// Square and round bars: side or diameter.
const BARS: [(&str, f64); 7] = [
    ("10", 10.0),
    ("12", 12.0),
    ("16", 16.0),
    ("20", 20.0),
    ("25", 25.0),
    ("30", 30.0),
    ("40", 40.0),
];

/// Flat bars: width, thickness.
const FLAT_BARS: [(&str, f64, f64); 8] = [
    ("20x3", 20.0, 3.0),
    ("25x5", 25.0, 5.0),
    ("30x5", 30.0, 5.0),
    ("40x5", 40.0, 5.0),
    ("50x6", 50.0, 6.0),
    ("60x8", 60.0, 8.0),
    ("80x10", 80.0, 10.0),
    ("100x10", 100.0, 10.0),
];

/// Equal angles (EN 10056-1): leg, thickness, root radius, toe radius.
const ANGLES: [(&str, f64, f64, f64, f64); 8] = [
    ("20x20x3", 20.0, 3.0, 3.5, 2.0),
    ("25x25x3", 25.0, 3.0, 3.5, 2.0),
    ("30x30x3", 30.0, 3.0, 5.0, 2.5),
    ("40x40x4", 40.0, 4.0, 6.0, 3.0),
    ("50x50x5", 50.0, 5.0, 7.0, 3.5),
    ("60x60x6", 60.0, 6.0, 8.0, 4.0),
    ("80x80x8", 80.0, 8.0, 10.0, 5.0),
    ("100x100x10", 100.0, 10.0, 12.0, 6.0),
];

/// UPN channels: height, flange width, web, flange (mean), root radius.
const CHANNELS: [(&str, f64, f64, f64, f64, f64); 8] = [
    ("UPN 50", 50.0, 38.0, 5.0, 7.0, 7.0),
    ("UPN 65", 65.0, 42.0, 5.5, 7.5, 7.5),
    ("UPN 80", 80.0, 45.0, 6.0, 8.0, 8.0),
    ("UPN 100", 100.0, 50.0, 6.0, 8.5, 8.5),
    ("UPN 120", 120.0, 55.0, 7.0, 9.0, 9.0),
    ("UPN 140", 140.0, 60.0, 7.0, 10.0, 10.0),
    ("UPN 160", 160.0, 65.0, 7.5, 10.5, 10.5),
    ("UPN 200", 200.0, 75.0, 8.5, 11.5, 11.5),
];

/// IPE beams (EN 10365): height, flange width, web, flange, root radius.
const I_BEAMS: [(&str, f64, f64, f64, f64, f64); 9] = [
    ("IPE 80", 80.0, 46.0, 3.8, 5.2, 5.0),
    ("IPE 100", 100.0, 55.0, 4.1, 5.7, 7.0),
    ("IPE 120", 120.0, 64.0, 4.4, 6.3, 7.0),
    ("IPE 140", 140.0, 73.0, 4.7, 6.9, 7.0),
    ("IPE 160", 160.0, 82.0, 5.0, 7.4, 9.0),
    ("IPE 180", 180.0, 91.0, 5.3, 8.0, 9.0),
    ("IPE 200", 200.0, 100.0, 5.6, 8.5, 12.0),
    ("IPE 240", 240.0, 120.0, 6.2, 9.8, 15.0),
    ("IPE 300", 300.0, 150.0, 7.1, 10.7, 15.0),
];

/// The names of a family's sizes, as the Size dropdown lists them.
pub fn size_names(family: ProfileFamily) -> Vec<&'static str> {
    match family {
        ProfileFamily::TSlot => T_SLOTS.iter().map(|s| s.0).collect(),
        ProfileFamily::SquareTube => SQUARE_TUBES.iter().map(|s| s.0).collect(),
        ProfileFamily::RectTube => RECT_TUBES.iter().map(|s| s.0).collect(),
        ProfileFamily::RoundTube => ROUND_TUBES.iter().map(|s| s.0).collect(),
        ProfileFamily::SquareBar | ProfileFamily::RoundBar => BARS.iter().map(|s| s.0).collect(),
        ProfileFamily::FlatBar => FLAT_BARS.iter().map(|s| s.0).collect(),
        ProfileFamily::Angle => ANGLES.iter().map(|s| s.0).collect(),
        ProfileFamily::Channel => CHANNELS.iter().map(|s| s.0).collect(),
        ProfileFamily::IBeam => I_BEAMS.iter().map(|s| s.0).collect(),
        ProfileFamily::Custom => Vec::new(),
    }
}

/// A standard section in its own coordinates (x across, y up; tubes, bars and beams about their
/// middle, an angle with its heel at the origin), its outer loops counter-clockwise. `None` for
/// a size the family hasn't (and for Custom).
pub fn section(family: ProfileFamily, size: &str) -> Option<Vec<Region>> {
    let find = |names: &[&str]| names.iter().position(|n| *n == size);
    Some(match family {
        ProfileFamily::TSlot => {
            let (_, s, nx, ny, _) = T_SLOTS[find(&T_SLOTS.map(|s| s.0))?];
            t_slot(s, nx, ny)
        }
        ProfileFamily::SquareTube => {
            let (_, b, t) = SQUARE_TUBES[find(&SQUARE_TUBES.map(|s| s.0))?];
            vec![tube(b, b, t)]
        }
        ProfileFamily::RectTube => {
            let (_, b, h, t) = RECT_TUBES[find(&RECT_TUBES.map(|s| s.0))?];
            vec![tube(b, h, t)]
        }
        ProfileFamily::RoundTube => {
            let (_, d, t) = ROUND_TUBES[find(&ROUND_TUBES.map(|s| s.0))?];
            vec![Region {
                outer: circle([0.0, 0.0], d / 2.0),
                holes: vec![circle([0.0, 0.0], d / 2.0 - t)],
                source: None,
            }]
        }
        ProfileFamily::SquareBar => {
            let (_, b) = BARS[find(&BARS.map(|s| s.0))?];
            vec![plain(rounded(&rect(b, b), &[0.0; 4]))]
        }
        ProfileFamily::RoundBar => {
            let (_, d) = BARS[find(&BARS.map(|s| s.0))?];
            vec![plain(circle([0.0, 0.0], d / 2.0))]
        }
        ProfileFamily::FlatBar => {
            let (_, w, t) = FLAT_BARS[find(&FLAT_BARS.map(|s| s.0))?];
            vec![plain(rounded(&rect(w, t), &[0.0; 4]))]
        }
        ProfileFamily::Angle => {
            let (_, a, t, r1, r2) = ANGLES[find(&ANGLES.map(|s| s.0))?];
            let p = [[0.0, 0.0], [a, 0.0], [a, t], [t, t], [t, a], [0.0, a]];
            vec![plain(rounded(&p, &[0.0, 0.0, r2, r1, r2, 0.0]))]
        }
        ProfileFamily::Channel => {
            let (_, h, b, tw, tf, r1) = CHANNELS[find(&CHANNELS.map(|s| s.0))?];
            let r2 = r1 / 2.0;
            // The web's back on the left, the flanges to the right; about its middle.
            let (x0, y0) = (-b / 2.0, -h / 2.0);
            let p = [
                [0.0, 0.0],
                [b, 0.0],
                [b, tf],
                [tw, tf],
                [tw, h - tf],
                [b, h - tf],
                [b, h],
                [0.0, h],
            ]
            .map(|[x, y]| [x + x0, y + y0]);
            vec![plain(rounded(&p, &[0.0, 0.0, r2, r1, r1, r2, 0.0, 0.0]))]
        }
        ProfileFamily::IBeam => {
            let (_, h, b, tw, tf, r) = I_BEAMS[find(&I_BEAMS.map(|s| s.0))?];
            let (hb, hh, hw) = (b / 2.0, h / 2.0, tw / 2.0);
            let p = [
                [-hb, -hh],
                [hb, -hh],
                [hb, -hh + tf],
                [hw, -hh + tf],
                [hw, hh - tf],
                [hb, hh - tf],
                [hb, hh],
                [-hb, hh],
                [-hb, hh - tf],
                [-hw, hh - tf],
                [-hw, -hh + tf],
                [-hb, -hh + tf],
            ];
            let mut radii = [0.0; 12];
            for i in [3, 4, 9, 10] {
                radii[i] = r;
            }
            vec![plain(rounded(&p, &radii))]
        }
        ProfileFamily::Custom => return None,
    })
}

fn plain(outer: Loop) -> Region {
    Region {
        outer,
        holes: Vec::new(),
        source: None,
    }
}

fn circle(c: [f64; 2], r: f64) -> Loop {
    Loop {
        curves: vec![Curve2::Circle {
            center: Point2::new(c[0], c[1]),
            radius: r,
            source: None,
        }],
    }
}

/// A rectangle about the origin, counter-clockwise from its lower left.
fn rect(w: f64, h: f64) -> [[f64; 2]; 4] {
    let (x, y) = (w / 2.0, h / 2.0);
    [[-x, -y], [x, -y], [x, y], [-x, y]]
}

/// A hollow tube's section: outside corners of radius 2t, inside ones of t.
fn tube(b: f64, h: f64, t: f64) -> Region {
    Region {
        outer: rounded(&rect(b, h), &[2.0 * t; 4]),
        holes: vec![rounded(&rect(b - 2.0 * t, h - 2.0 * t), &[t; 4])],
        source: None,
    }
}

/// A polygon (counter-clockwise) with each corner rounded to its radius (0: sharp): convex
/// corners and re-entrant ones alike.
fn rounded(p: &[[f64; 2]], radii: &[f64]) -> Loop {
    let n = p.len();
    let pt = |q: [f64; 2]| Point2::new(q[0], q[1]);
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    let norm = |a: [f64; 2]| (a[0] * a[0] + a[1] * a[1]).sqrt();
    let unit = |a: [f64; 2]| {
        let l = norm(a);
        [a[0] / l, a[1] / l]
    };
    // Each corner's tangent points (equal if sharp) and its arc.
    let mut ends: Vec<([f64; 2], [f64; 2], Option<Curve2>)> = Vec::with_capacity(n);
    for i in 0..n {
        let (prev, here, next) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
        let r = radii[i];
        if r <= 0.0 {
            ends.push((here, here, None));
            continue;
        }
        let (u, w) = (unit(sub(prev, here)), unit(sub(next, here)));
        let cos = (u[0] * w[0] + u[1] * w[1]).clamp(-1.0, 1.0);
        let half = cos.acos() / 2.0;
        let d = r / half.tan();
        let t1 = [here[0] + u[0] * d, here[1] + u[1] * d];
        let t2 = [here[0] + w[0] * d, here[1] + w[1] * d];
        let bis = unit([u[0] + w[0], u[1] + w[1]]);
        let c = [
            here[0] + bis[0] * r / half.sin(),
            here[1] + bis[1] * r / half.sin(),
        ];
        let (d1, d2) = (sub(here, prev), sub(next, here));
        let turn = d1[0] * d2[1] - d1[1] * d2[0];
        let start = (t1[1] - c[1]).atan2(t1[0] - c[0]);
        let size = std::f64::consts::PI - 2.0 * half;
        ends.push((
            t1,
            t2,
            Some(Curve2::Arc {
                center: pt(c),
                radius: r,
                start_angle: start,
                sweep: if turn >= 0.0 { size } else { -size },
                source: None,
            }),
        ));
    }
    let mut curves = Vec::new();
    for i in 0..n {
        let (_, out, arc) = &ends[i];
        if let Some(a) = arc {
            curves.push(a.clone());
        }
        let (into_next, _, _) = ends[(i + 1) % n];
        if norm(sub(into_next, *out)) > 1e-12 {
            curves.push(Curve2::Line {
                a: pt(*out),
                b: pt(into_next),
                source: None,
            });
        }
    }
    Loop { curves }
}

/// A T-slot extrusion of `nx` by `ny` cells: a slot in the middle of each cell's outer sides,
/// a bore in each cell's middle.
fn t_slot(s: &TSlotSeries, nx: u32, ny: u32) -> Vec<Region> {
    let (w, h) = (s.cell * nx as f64, s.cell * ny as f64);
    let (x0, y0) = (-w / 2.0, -h / 2.0);
    // The cavity narrows at 45° below its straight wall.
    let floor = (s.width - 2.0 * (s.depth - s.lip - s.wall)).max(s.opening);
    // A slot's outline from the outer face inwards, as (along, in) from the slot's middle.
    let notch = [
        (-s.opening / 2.0, 0.0),
        (-s.opening / 2.0, s.lip),
        (-s.width / 2.0, s.lip),
        (-s.width / 2.0, s.lip + s.wall),
        (-floor / 2.0, s.depth),
        (floor / 2.0, s.depth),
        (s.width / 2.0, s.lip + s.wall),
        (s.width / 2.0, s.lip),
        (s.opening / 2.0, s.lip),
        (s.opening / 2.0, 0.0),
    ];
    let mut p: Vec<[f64; 2]> = Vec::new();
    // Counter-clockwise: bottom (+x, inwards +y), right (+y, inwards -x), top (-x, inwards -y),
    // left (-y, inwards +x).
    // Each side: its start, the way along it, the way in, and its cells.
    type Side = ([f64; 2], [f64; 2], [f64; 2], u32);
    let sides: [Side; 4] = [
        ([x0, y0], [1.0, 0.0], [0.0, 1.0], nx),
        ([x0 + w, y0], [0.0, 1.0], [-1.0, 0.0], ny),
        ([x0 + w, y0 + h], [-1.0, 0.0], [0.0, -1.0], nx),
        ([x0, y0 + h], [0.0, -1.0], [1.0, 0.0], ny),
    ];
    for (corner, along, inward, cells) in sides {
        p.push(corner);
        for c in 0..cells {
            let mid = (c as f64 + 0.5) * s.cell;
            for (a, i) in notch {
                p.push([
                    corner[0] + along[0] * (mid + a) + inward[0] * i,
                    corner[1] + along[1] * (mid + a) + inward[1] * i,
                ]);
            }
        }
    }
    let radii = vec![0.0; p.len()];
    let holes = (0..nx)
        .flat_map(|i| (0..ny).map(move |j| (i, j)))
        .map(|(i, j)| {
            circle(
                [
                    x0 + (i as f64 + 0.5) * s.cell,
                    y0 + (j as f64 + 0.5) * s.cell,
                ],
                s.bore / 2.0,
            )
        })
        .collect();
    vec![Region {
        outer: rounded(&p, &radii),
        holes,
        source: None,
    }]
}

// ---------------------------------------------------------------------------------------------
// Placing a section

/// The furthest a curve reaches in the direction `d` (2D; exact for lines, arcs and circles,
/// the control polygon's for the rest).
fn curve_support(c: &Curve2, d: [f64; 2]) -> f64 {
    let dot = |p: &Point2<f64>| p.x * d[0] + p.y * d[1];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    match c {
        Curve2::Line { a, b, .. } => dot(a).max(dot(b)),
        Curve2::Circle { center, radius, .. } => dot(center) + radius.abs() * len,
        Curve2::Arc {
            center,
            radius,
            start_angle,
            sweep,
            ..
        } => {
            let at = |t: f64| dot(center) + radius * (t.cos() * d[0] + t.sin() * d[1]);
            let mut best = at(*start_angle).max(at(start_angle + sweep));
            // The direction's own angle, if the arc passes it.
            let want = d[1].atan2(d[0]);
            let (lo, hi) = if *sweep >= 0.0 {
                (*start_angle, start_angle + sweep)
            } else {
                (start_angle + sweep, *start_angle)
            };
            let tau = std::f64::consts::TAU;
            let mut t = want + ((lo - want) / tau).ceil() * tau;
            while t <= hi {
                best = best.max(at(t));
                t += tau;
            }
            best
        }
        other => sample(other).iter().map(dot).fold(f64::MIN, f64::max),
    }
}

/// Points along a curve (its ends and inner points; a spline's or Bézier's control points,
/// which bound it).
fn sample(c: &Curve2) -> Vec<Point2<f64>> {
    match c {
        Curve2::Line { a, b, .. } => vec![*a, *b],
        Curve2::Bezier { poles, .. } => poles.to_vec(),
        Curve2::Spline { poles, .. } => poles.clone(),
        other => {
            let (center, rx, ry, rot, t0, sweep, off) = match *other {
                Curve2::Circle { center, radius, .. } => {
                    (center, radius, radius, 0.0, 0.0, std::f64::consts::TAU, 0.0)
                }
                Curve2::Arc {
                    center,
                    radius,
                    start_angle,
                    sweep,
                    ..
                } => (center, radius, radius, 0.0, start_angle, sweep, 0.0),
                Curve2::Ellipse {
                    center,
                    major_radius,
                    minor_radius,
                    rotation,
                    ..
                } => (
                    center,
                    major_radius,
                    minor_radius,
                    rotation,
                    0.0,
                    std::f64::consts::TAU,
                    0.0,
                ),
                Curve2::EllipseArc {
                    center,
                    major_radius,
                    minor_radius,
                    rotation,
                    start,
                    sweep,
                    ..
                }
                | Curve2::OffsetEllipseArc {
                    center,
                    major_radius,
                    minor_radius,
                    rotation,
                    start,
                    sweep,
                    ..
                } => (
                    center,
                    major_radius,
                    minor_radius,
                    rotation,
                    start,
                    sweep,
                    0.0,
                ),
                _ => return Vec::new(),
            };
            let off = match other {
                Curve2::OffsetEllipseArc { offset, .. } => offset.abs(),
                _ => off,
            };
            (0..=64)
                .map(|i| {
                    let t = t0 + sweep * i as f64 / 64.0;
                    let (x, y) = (rx.abs() * t.cos(), ry.abs() * t.sin());
                    let (c, s) = (rot.cos(), rot.sin());
                    let r = (x * x + y * y).sqrt().max(1e-300);
                    let k = 1.0 + off / r;
                    Point2::new(
                        center.x + k * (c * x - s * y),
                        center.y + k * (s * x + c * y),
                    )
                })
                .collect()
        }
    }
}

/// The furthest a section reaches in the direction `d` (its outer loops').
pub fn support(regions: &[Region], d: [f64; 2]) -> f64 {
    regions
        .iter()
        .flat_map(|r| &r.outer.curves)
        .map(|c| curve_support(c, d))
        .fold(f64::MIN, f64::max)
}

/// A section's box: (min x, min y, max x, max y).
pub fn bounds(regions: &[Region]) -> [f64; 4] {
    [
        -support(regions, [-1.0, 0.0]),
        -support(regions, [0.0, -1.0]),
        support(regions, [1.0, 0.0]),
        support(regions, [0.0, 1.0]),
    ]
}

/// How far a section reaches from the origin, at most.
pub fn radius(regions: &[Region]) -> f64 {
    (0..32)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 32.0;
            support(regions, [a.cos(), a.sin()])
        })
        .fold(0.0, f64::max)
        // The 32 directions' support polygon contains the section; its corners are this much
        // further out at most.
        / (std::f64::consts::PI / 32.0).cos()
}

/// A rigid motion of the plane (and a mirror): `p → R(angle)·(M·p + shift)`, `M` flipping x
/// when `mirror`.
#[derive(Debug, Clone, Copy)]
struct Place {
    mirror: bool,
    shift: [f64; 2],
    angle: f64,
}

impl Place {
    fn point(&self, p: Point2<f64>) -> Point2<f64> {
        let x = if self.mirror { -p.x } else { p.x };
        let (c, s) = (self.angle.cos(), self.angle.sin());
        let (x, y) = (x + self.shift[0], p.y + self.shift[1]);
        Point2::new(c * x - s * y, s * x + c * y)
    }

    /// An angle in the plane (a direction's).
    fn turn(&self, a: f64) -> f64 {
        let a = if self.mirror {
            std::f64::consts::PI - a
        } else {
            a
        };
        a + self.angle
    }

    fn curve(&self, c: &Curve2) -> Curve2 {
        let m = self.mirror;
        let sweep_sign = if m { -1.0 } else { 1.0 };
        match c.clone() {
            Curve2::Line { a, b, source } => Curve2::Line {
                a: self.point(a),
                b: self.point(b),
                source,
            },
            Curve2::Arc {
                center,
                radius,
                start_angle,
                sweep,
                source,
            } => Curve2::Arc {
                center: self.point(center),
                radius,
                start_angle: self.turn(start_angle),
                sweep: sweep * sweep_sign,
                source,
            },
            Curve2::Circle {
                center,
                radius,
                source,
            } => Curve2::Circle {
                center: self.point(center),
                radius: radius * sweep_sign,
                source,
            },
            // Mirrored, a whole ellipse runs the other way: a whole turn of arc with its minor
            // axis flipped (a whole Ellipse has no way of its own).
            Curve2::Ellipse {
                center,
                major_radius,
                minor_radius,
                rotation,
                source,
            } if m => Curve2::EllipseArc {
                center: self.point(center),
                major_radius,
                minor_radius: -minor_radius,
                rotation: self.turn(rotation),
                start: 0.0,
                sweep: std::f64::consts::TAU,
                source,
            },
            Curve2::Ellipse {
                center,
                major_radius,
                minor_radius,
                rotation,
                source,
            } => Curve2::Ellipse {
                center: self.point(center),
                major_radius,
                minor_radius,
                rotation: self.turn(rotation),
                source,
            },
            Curve2::EllipseArc {
                center,
                major_radius,
                minor_radius,
                rotation,
                start,
                sweep,
                source,
            } => Curve2::EllipseArc {
                center: self.point(center),
                major_radius,
                minor_radius: minor_radius * sweep_sign,
                rotation: self.turn(rotation),
                start,
                sweep,
                source,
            },
            Curve2::OffsetEllipseArc {
                center,
                major_radius,
                minor_radius,
                rotation,
                start,
                sweep,
                offset,
                source,
            } => Curve2::OffsetEllipseArc {
                center: self.point(center),
                major_radius,
                minor_radius: minor_radius * sweep_sign,
                rotation: self.turn(rotation),
                start,
                sweep,
                offset,
                source,
            },
            Curve2::Bezier { poles, source } => Curve2::Bezier {
                poles: poles.map(|p| self.point(p)),
                source,
            },
            Curve2::Spline {
                degree,
                knots,
                poles,
                weights,
                source,
            } => Curve2::Spline {
                degree,
                knots,
                poles: poles.into_iter().map(|p| self.point(p)).collect(),
                weights,
                source,
            },
        }
    }

    fn region(&self, r: &Region) -> Region {
        let lp = |l: &Loop| Loop {
            curves: l.curves.iter().map(|c| self.curve(c)).collect(),
        };
        Region {
            outer: lp(&r.outer),
            holes: r.holes.iter().map(lp).collect(),
            source: r.source,
        }
    }
}

/// A section placed for the path: mirrored, its anchor moved to the origin, then turned by
/// `angle` about it.
pub fn place(regions: &[Region], anchor: Anchor, mirror: bool, angle: f64) -> Vec<Region> {
    let flipped: Vec<Region> = regions
        .iter()
        .map(|r| {
            Place {
                mirror,
                shift: [0.0, 0.0],
                angle: 0.0,
            }
            .region(r)
        })
        .collect();
    let shift = match anchor.fractions() {
        None => [0.0, 0.0],
        Some((fx, fy)) => {
            let [x0, y0, x1, y1] = bounds(&flipped);
            [-(x0 + fx * (x1 - x0)), -(y0 + fy * (y1 - y0))]
        }
    };
    flipped
        .iter()
        .map(|r| {
            Place {
                mirror: false,
                shift,
                angle,
            }
            .region(r)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The cut list

/// How a member's end is cut.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EndCut {
    /// Square to the member.
    Square,
    /// On a plane at this angle from square (degrees; a 90° corner's miter is 45°).
    Angle(f64),
    /// To another member's shape.
    Coped,
    /// By a part it was trimmed to.
    Trimmed,
}

impl EndCut {
    /// "0°", "45°", "Coped".
    pub fn text(self) -> String {
        match self {
            EndCut::Square => "0°".into(),
            EndCut::Angle(a) => format!("{}°", trim(a, 1)),
            EndCut::Coped => "Coped".into(),
            EndCut::Trimmed => "Trimmed".into(),
        }
    }

    fn key(self) -> i64 {
        match self {
            EndCut::Square => 0,
            EndCut::Angle(a) => (a * 10.0).round() as i64,
            EndCut::Coped => -1,
            EndCut::Trimmed => -2,
        }
    }
}

/// An end cap's name in the cut list: "End cap 3 mm (Square tube 40x40x3)".
pub fn cap_label(thickness: f64, profile: &str) -> String {
    format!("End cap {} mm ({profile})", trim(thickness, 2))
}

/// A gusset's name in the cut list: "Gusset 60 x 5 mm".
pub fn gusset_label(size: f64, thickness: f64) -> String {
    format!("Gusset {} x {} mm", trim(size, 2), trim(thickness, 2))
}

/// A member the Frame feature made, as the cut list counts it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemberInfo {
    /// The Frame feature.
    pub feature: FeatureId,
    /// Its part.
    pub part: PartId,
    /// The section's name ([`FrameFeature::profile_label`]).
    pub profile: String,
    /// The stock it is cut from (mm): a straight member's overall length along it, a curved
    /// one's along its path.
    pub length: f64,
    /// How its start and end are cut.
    pub ends: [EndCut; 2],
    /// It is bent (follows arcs or other curves).
    pub curved: bool,
    /// Its mass properties' volume (mm³).
    pub volume: f64,
    /// An end cap or a gusset (a plate, its size in `profile`, `length` 0), not a member.
    #[serde(default)]
    pub plate: bool,
    /// Each end's cut plane's normal across the member (along its section's right and up at
    /// that end; zero for a square or no cut): mitres the same angle but on other sides of the
    /// section are different pieces.
    #[serde(default)]
    pub end_normals: [[f64; 2]; 2],
    /// A standard section's family and size (to name it in another language); `None` for a
    /// custom section or a plate.
    #[serde(default)]
    pub family: Option<ProfileFamily>,
    #[serde(default)]
    pub size: String,
    /// A plate's kind and size (to name it in another language).
    #[serde(default)]
    pub plate_kind: Option<PlateKind>,
}

/// An end cap or a gusset, with its size (mm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PlateKind {
    Cap { thickness: f64 },
    Gusset { size: f64, thickness: f64 },
}

/// The material a family is usually made of, its name and density (kg/m³): T-slot extrusion
/// aluminium 6063-T5, the rest structural steel S235.
pub fn nominal_material(family: ProfileFamily) -> (&'static str, f64) {
    match family {
        ProfileFamily::TSlot => ("Aluminium 6063-T5", 2700.0),
        _ => ("Steel S235", 7850.0),
    }
}

/// What a row of the cut list is made of and weighs.
#[derive(Debug, Clone, PartialEq)]
pub struct RowMaterial {
    /// Its parts' material (if they share one), else the family's usual one.
    pub material: Option<String>,
    /// kg, the row's members together.
    pub mass: Option<f64>,
    /// Not the parts' own materials: the family's usual one (T-slot by its catalogue kg/m).
    pub nominal: bool,
}

/// The members among `parts` as they are now: each member whose part is still there, and each
/// copy a Pattern or Mirror made of one (a part whose `source` leads back to a member's), as a
/// member of its own. A member a later feature joined to another part, or deleted, is gone.
pub fn live_members(frames: &[MemberInfo], parts: &[crate::Part]) -> Vec<MemberInfo> {
    let member = |id: PartId| frames.iter().find(|m| m.part == id);
    let source = |id: PartId| parts.iter().find(|p| p.id == id).and_then(|p| p.source);
    let mut out = Vec::new();
    for p in parts {
        // The member it is, or was copied from (copies of copies too).
        let mut at = Some(p.id);
        let mut steps = 0;
        while let Some(id) = at {
            if let Some(m) = member(id) {
                out.push(MemberInfo {
                    part: p.id,
                    ..m.clone()
                });
                break;
            }
            steps += 1;
            at = if steps > 64 { None } else { source(id) };
        }
    }
    out
}

/// One row of the cut list: the members alike in section, length (to 0.1 mm) and end cuts.
#[derive(Debug, Clone, PartialEq)]
pub struct CutListRow {
    /// 1, 2, …
    pub item: usize,
    pub profile: String,
    pub length: f64,
    pub ends: [EndCut; 2],
    pub curved: bool,
    /// End caps or gussets (their size in `profile`).
    pub plate: bool,
    /// Its members' parts.
    pub parts: Vec<PartId>,
}

impl CutListRow {
    pub fn quantity(&self) -> usize {
        self.parts.len()
    }
}

/// Which way a member's end cuts face across it, the same for the member seen from either end
/// (turned end for end its right is its left).
fn cuts_key(m: &MemberInfo) -> [[i64; 2]; 2] {
    let k = |v: [f64; 2]| v.map(|c| (c * 100.0).round() as i64);
    let ahead = [k(m.end_normals[0]), k(m.end_normals[1])];
    let mirror = |v: [f64; 2]| [-v[0], v[1]];
    let back = [k(mirror(m.end_normals[1])), k(mirror(m.end_normals[0]))];
    ahead.min(back)
}

/// The cut list of these members: alike ones (section, length to 0.1 mm, end cuts, which way
/// the cuts face across the section) counted together, longest first within a section,
/// sections in the order they first come.
pub fn cut_list(members: &[MemberInfo]) -> Vec<CutListRow> {
    let mut rows: Vec<CutListRow> = Vec::new();
    let mut keys: Vec<[[i64; 2]; 2]> = Vec::new();
    let length_key = |l: f64| (l * 10.0).round() as i64;
    for m in members {
        let mut ends = m.ends;
        if ends[0].key() > ends[1].key() {
            ends.swap(0, 1);
        }
        let key = cuts_key(m);
        match rows.iter_mut().zip(&keys).find(|(r, k)| {
            **k == key
                && r.profile == m.profile
                && length_key(r.length) == length_key(m.length)
                && r.ends.map(EndCut::key) == ends.map(EndCut::key)
                && r.curved == m.curved
                && r.plate == m.plate
        }) {
            Some((r, _)) => r.parts.push(m.part),
            None => {
                keys.push(key);
                rows.push(CutListRow {
                    item: 0,
                    profile: m.profile.clone(),
                    length: m.length,
                    ends,
                    curved: m.curved,
                    plate: m.plate,
                    parts: vec![m.part],
                });
            }
        }
    }
    let order: Vec<String> = {
        let mut o: Vec<String> = Vec::new();
        for r in &rows {
            if !o.contains(&r.profile) {
                o.push(r.profile.clone());
            }
        }
        o
    };
    rows.sort_by(|a, b| {
        let pa = order.iter().position(|p| *p == a.profile);
        let pb = order.iter().position(|p| *p == b.profile);
        pa.cmp(&pb).then(b.length.total_cmp(&a.length))
    });
    for (i, r) in rows.iter_mut().enumerate() {
        r.item = i + 1;
    }
    rows
}

/// The cut list's total length of stock for each section (mm), in the rows' order (plates
/// left out).
pub fn totals(rows: &[CutListRow]) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = Vec::new();
    for r in rows.iter().filter(|r| !r.plate) {
        let l = r.length * r.quantity() as f64;
        match out.iter_mut().find(|(p, _)| *p == r.profile) {
            Some((_, t)) => *t += l,
            None => out.push((r.profile.clone(), l)),
        }
    }
    out
}

/// `v` with at most `places` decimals, trailing zeros dropped.
fn trim(v: f64, places: usize) -> String {
    let s = format!("{v:.places$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// A length for the cut list: "1234.5".
pub fn length_text(mm: f64) -> String {
    trim(mm, 1)
}

/// A row's material and mass: its parts' own where every part has a material (or a mass of its
/// own), else the family's usual material (a T-slot extrusion by its catalogue kg/m; a plate by
/// its frame's family), marked nominal. A custom section's members have none.
pub fn row_material(
    row: &CutListRow,
    members: &[MemberInfo],
    parts: &[crate::Part],
    props: &[crate::PartProps],
) -> RowMaterial {
    let of: Vec<&crate::Part> = row
        .parts
        .iter()
        .filter_map(|id| parts.iter().find(|p| p.id == *id))
        .collect();
    let names: Vec<Option<String>> = of
        .iter()
        .map(|p| crate::parts::part_material(p, props).map(|m| m.name.clone()))
        .collect();
    let mass = crate::parts::mass_report(&of, props)
        .and_then(|r| r.mass)
        .map(|m| m.mass);
    if let Some(mass) = mass {
        let material = match names.first() {
            Some(Some(n)) if names.iter().all(|m| m.as_ref() == Some(n)) => Some(n.clone()),
            _ => None,
        };
        return RowMaterial {
            material,
            mass: Some(mass),
            nominal: false,
        };
    }
    // The family's usual material.
    let member = |id: &PartId| members.iter().find(|m| m.part == *id);
    let family = |m: &MemberInfo| {
        m.family.or_else(|| {
            members
                .iter()
                .find(|o| o.feature == m.feature && !o.plate)
                .and_then(|o| o.family)
        })
    };
    let mut total = 0.0;
    let mut name = None;
    for id in &row.parts {
        let Some(m) = member(id) else {
            return RowMaterial {
                material: None,
                mass: None,
                nominal: true,
            };
        };
        let Some(f) = family(m).filter(|f| *f != ProfileFamily::Custom) else {
            return RowMaterial {
                material: None,
                mass: None,
                nominal: true,
            };
        };
        let (n, density) = nominal_material(f);
        name = Some(n.to_string());
        let per_metre = (f == ProfileFamily::TSlot && !m.plate)
            .then(|| T_SLOTS.iter().find(|t| t.0 == m.size).map(|t| t.4))
            .flatten();
        total += match per_metre {
            Some(kg_m) => kg_m * m.length / 1000.0,
            None => m.volume * density * 1e-9,
        };
    }
    RowMaterial {
        material: name,
        mass: Some(total),
        nominal: true,
    }
}

/// The cut list as CSV (comma separated, quoted where needed): Item, Qty, Profile, Length (mm),
/// Angle 1, Angle 2, Bent, Material, Mass (kg, the row's members together), Parts. A plate's
/// length and angles are left empty (its size is in its name).
pub fn cut_list_csv(
    rows: &[CutListRow],
    name: impl Fn(PartId) -> String,
    material: impl Fn(&CutListRow) -> RowMaterial,
) -> String {
    let q = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let mut out = String::from(
        "Item,Qty,Profile,Length (mm),Angle 1,Angle 2,Bent,Material,Mass (kg),Parts\n",
    );
    for r in rows {
        let parts: Vec<String> = r.parts.iter().map(|p| name(*p)).collect();
        let RowMaterial {
            material: mat,
            mass,
            ..
        } = material(r);
        let (length, a1, a2) = if r.plate {
            Default::default()
        } else {
            (
                length_text(r.length),
                q(&r.ends[0].text()),
                q(&r.ends[1].text()),
            )
        };
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            r.item,
            r.quantity(),
            q(&r.profile),
            length,
            a1,
            a2,
            if r.curved { "yes" } else { "no" },
            q(mat.as_deref().unwrap_or("")),
            mass.map(|m| trim(m, 3)).unwrap_or_default(),
            q(&parts.join("; ")),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The area of a section (Green's theorem along its loops; holes subtracted).
    fn area(regions: &[Region]) -> f64 {
        let loop_area = |l: &Loop| -> f64 {
            let mut a = 0.0;
            for c in &l.curves {
                match c {
                    Curve2::Line { a: p, b: q, .. } => a += (p.x * q.y - q.x * p.y) / 2.0,
                    Curve2::Arc {
                        center,
                        radius,
                        start_angle,
                        sweep,
                        ..
                    } => {
                        // ∮ x dy − y dx / 2 over the arc.
                        let (t0, t1) = (*start_angle, start_angle + sweep);
                        let r = *radius;
                        a += (r * r * (t1 - t0) + center.x * r * (t1.sin() - t0.sin())
                            - center.y * r * (t1.cos() - t0.cos()))
                            / 2.0;
                    }
                    Curve2::Circle { radius, .. } => {
                        a += std::f64::consts::PI * radius * radius * radius.signum()
                    }
                    _ => unreachable!(),
                }
            }
            a.abs()
        };
        regions
            .iter()
            .map(|r| loop_area(&r.outer) - r.holes.iter().map(loop_area).sum::<f64>())
            .sum()
    }

    fn ends_join(l: &Loop) {
        let ends = |c: &Curve2| -> (Point2<f64>, Point2<f64>) {
            match c {
                Curve2::Line { a, b, .. } => (*a, *b),
                Curve2::Arc {
                    center,
                    radius,
                    start_angle,
                    sweep,
                    ..
                } => {
                    let at = |t: f64| center + nalgebra::Vector2::new(t.cos(), t.sin()) * *radius;
                    (at(*start_angle), at(start_angle + sweep))
                }
                _ => unreachable!(),
            }
        };
        let n = l.curves.len();
        for i in 0..n {
            let (_, b) = ends(&l.curves[i]);
            let (a, _) = ends(&l.curves[(i + 1) % n]);
            assert!(
                (a - b).norm() < 1e-9,
                "a gap of {} after curve {i}",
                (a - b).norm()
            );
        }
    }

    #[test]
    fn every_size_is_a_closed_section() {
        for f in ProfileFamily::ALL {
            for s in size_names(f) {
                let r = section(f, s).unwrap_or_else(|| panic!("{f:?} {s}"));
                for region in &r {
                    if region.outer.curves.len() > 1 {
                        ends_join(&region.outer);
                    }
                    for h in &region.holes {
                        if h.curves.len() > 1 {
                            ends_join(h);
                        }
                    }
                }
                assert!(area(&r) > 0.0, "{f:?} {s}");
            }
        }
    }

    #[test]
    fn areas_match_the_tables() {
        // IPE 100: 10.3 cm²; EN 10056 40x40x4: 3.08 cm²; a 40x40x3 tube (EN 10219): 4.21 cm².
        let a = area(&section(ProfileFamily::IBeam, "IPE 100").unwrap());
        assert!((a - 1032.3).abs() < 0.5, "{a}");
        let a = area(&section(ProfileFamily::Angle, "40x40x4").unwrap());
        assert!((a - 308.0).abs() < 2.0, "{a}");
        let a = area(&section(ProfileFamily::SquareTube, "40x40x3").unwrap());
        assert!((a - 421.0).abs() < 1.0, "{a}");
        let a = area(&section(ProfileFamily::RoundTube, "33.7x3.2").unwrap());
        let want = std::f64::consts::PI * (16.85f64.powi(2) - 13.65f64.powi(2));
        assert!((a - want).abs() < 1e-9);
    }

    #[test]
    fn t_slot_outlines_dont_cross_themselves() {
        for (name, ..) in T_SLOTS {
            let r = section(ProfileFamily::TSlot, name).unwrap();
            let pts: Vec<Point2<f64>> = r[0]
                .outer
                .curves
                .iter()
                .map(|c| match c {
                    Curve2::Line { a, .. } => *a,
                    _ => unreachable!(),
                })
                .collect();
            let n = pts.len();
            let cross = |a: Point2<f64>, b: Point2<f64>, c: Point2<f64>| {
                (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
            };
            for i in 0..n {
                for j in i + 2..n {
                    if i == 0 && j == n - 1 {
                        continue;
                    }
                    let (a, b, c, d) = (pts[i], pts[(i + 1) % n], pts[j], pts[(j + 1) % n]);
                    let hit = cross(a, b, c) * cross(a, b, d) < 0.0
                        && cross(c, d, a) * cross(c, d, b) < 0.0;
                    assert!(!hit, "{name}: edges {i} and {j} cross");
                }
            }
            // Bores clear of the cavities: the section's area is its box less slots and bores.
            assert!(area(&r) > 0.0);
        }
    }

    #[test]
    fn anchors_put_the_box_where_asked() {
        let r = section(ProfileFamily::RectTube, "60x40x3").unwrap();
        let b = bounds(&place(&r, Anchor::BottomLeft, false, 0.0));
        assert!(b[0].abs() < 1e-9 && b[1].abs() < 1e-9 && (b[2] - 60.0).abs() < 1e-9);
        let b = bounds(&place(&r, Anchor::Top, false, 0.0));
        assert!((b[3]).abs() < 1e-9 && (b[1] + 40.0).abs() < 1e-9 && (b[0] + 30.0).abs() < 1e-9);
        // Turned a quarter about the anchor.
        let b = bounds(&place(
            &r,
            Anchor::Center,
            false,
            std::f64::consts::FRAC_PI_2,
        ));
        assert!((b[2] - 20.0).abs() < 1e-9 && (b[3] - 30.0).abs() < 1e-9);
        // An angle's heel at the origin; mirrored, its legs run to the left.
        let a = section(ProfileFamily::Angle, "40x40x4").unwrap();
        let b = bounds(&place(&a, Anchor::Origin, true, 0.0));
        assert!((b[0] + 40.0).abs() < 1e-9 && b[2].abs() < 1e-9);
        assert!((area(&place(&a, Anchor::Center, true, 0.3)) - area(&a)).abs() < 1e-6);
    }

    #[test]
    fn support_is_exact_on_round_sections() {
        let r = section(ProfileFamily::RoundTube, "48.3x3.2").unwrap();
        let s = support(&r, [0.6, 0.8]);
        assert!((s - 24.15).abs() < 1e-12);
        assert!((radius(&r) - 24.15).abs() / 24.15 < 0.01);
        let a = section(ProfileFamily::SquareTube, "40x40x3").unwrap();
        // Along the diagonal: the corner arc's furthest point.
        let d = std::f64::consts::FRAC_1_SQRT_2;
        let s = support(&a, [d, d]);
        let want = (20.0 - 6.0) * 2f64.sqrt() + 6.0;
        assert!((s - want).abs() < 1e-9, "{s} vs {want}");
    }

    #[test]
    fn the_cut_list_counts_alike_members() {
        let f = FeatureId::new();
        let m = |i: u32, l: f64, ends: [EndCut; 2]| MemberInfo {
            feature: f,
            part: PartId::new(f, i),
            profile: "Square tube 40x40x3".into(),
            length: l,
            ends,
            curved: false,
            volume: 0.0,
            plate: false,
            end_normals: [[0.0; 2]; 2],
            family: Some(ProfileFamily::SquareTube),
            plate_kind: None,
            size: "40x40x3".into(),
        };
        let miter = EndCut::Angle(45.0);
        let rows = cut_list(&[
            m(0, 500.0, [miter, miter]),
            m(1, 300.0, [miter, miter]),
            m(2, 500.02, [miter, miter]),
            m(3, 300.0, [miter, miter]),
            m(4, 300.0, [EndCut::Square, miter]),
            m(5, 300.0, [miter, EndCut::Square]),
        ]);
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].length, rows[0].quantity()), (500.0, 2));
        assert_eq!(rows[1].quantity(), 2);
        assert_eq!(rows[2].quantity(), 2, "an end cut either way round");
        assert_eq!(rows.iter().map(|r| r.item).collect::<Vec<_>>(), [1, 2, 3]);
        let t = totals(&rows);
        assert!((t[0].1 - 2200.0).abs() < 1e-9);
        let csv = cut_list_csv(
            &rows,
            |p| format!("Part {}", p.index + 1),
            |r| RowMaterial {
                material: Some("Steel, mild".into()),
                mass: Some(r.length * 1e-3),
                nominal: false,
            },
        );
        assert!(csv.starts_with("Item,Qty,Profile,Length (mm)"));
        assert!(
            csv.contains(
                "1,2,Square tube 40x40x3,500,45°,45°,no,\"Steel, mild\",0.5,Part 1; Part 3"
            ),
            "{csv}"
        );
    }

    #[test]
    fn pattern_copies_of_members_are_members() {
        use crate::{Part, PartKind};
        let f = FeatureId::new();
        let pattern = FeatureId::new();
        let part = |id: PartId, source: Option<PartId>| Part {
            id,
            feature: id.feature,
            name: String::new(),
            kind: PartKind::Solid,
            palette: 0,
            solid: Default::default(),
            mass: None,
            features: vec![id.feature],
            source,
            derived: None,
        };
        let (a, b) = (PartId::new(f, 0), PartId::new(f, 1));
        let copy = PartId::new(pattern, 0);
        let copy_of_copy = PartId::new(pattern, 1);
        let member = |p: PartId| MemberInfo {
            feature: f,
            part: p,
            profile: "T-slot 20x20".into(),
            length: 300.0,
            ends: [EndCut::Square; 2],
            curved: false,
            volume: 1.0,
            plate: false,
            end_normals: [[0.0; 2]; 2],
            family: Some(ProfileFamily::TSlot),
            plate_kind: None,
            size: "20x20".into(),
        };
        // `b` was joined to another part (gone); `a` was patterned twice over.
        let live = live_members(
            &[member(a), member(b)],
            &[
                part(a, None),
                part(copy, Some(a)),
                part(copy_of_copy, Some(copy)),
            ],
        );
        let ids: Vec<PartId> = live.iter().map(|m| m.part).collect();
        assert_eq!(ids, [a, copy, copy_of_copy]);
        assert_eq!(cut_list(&live)[0].quantity(), 3);
    }
}
