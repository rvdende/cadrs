//! The tools' parameters. Their doc comments become the JSON schema's descriptions, which are
//! what the assistant reads, so they say what each field means and its units.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateDocument {
    /// The document's name.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenDocument {
    /// The document's name or id.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddPartStudio {
    /// The tab's name; omitted: the next default ("Part Studio 2").
    #[serde(default)]
    pub name: Option<String>,
}

/// A default plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Plane {
    #[serde(alias = "top", alias = "TOP")]
    Top,
    #[serde(alias = "front", alias = "FRONT")]
    Front,
    #[serde(alias = "right", alias = "RIGHT")]
    Right,
}

/// A curve or shape of a sketch, in the sketch plane's 2D coordinates (mm).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SketchEntity {
    /// An axis-aligned rectangle from corner (x0, y0) to the opposite corner (x1, y1), with
    /// horizontal and vertical constraints.
    Rectangle { x0: f64, y0: f64, x1: f64, y1: f64 },
    /// A circle about (cx, cy).
    Circle { cx: f64, cy: f64, r: f64 },
    /// A closed polygon through the points ([x, y] each, at least 3).
    Polygon { points: Vec<[f64; 2]> },
    /// A line segment (lines whose ends meet close regions).
    Line { x0: f64, y0: f64, x1: f64, y1: f64 },
    /// An arc about (cx, cy), counter-clockwise from (sx, sy) to (ex, ey).
    Arc { cx: f64, cy: f64, sx: f64, sy: f64, ex: f64, ey: f64 },
    /// A sketch point.
    Point { x: f64, y: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddSketch {
    /// The plane to sketch on.
    pub plane: Plane,
    /// The curves and shapes.
    pub entities: Vec<SketchEntity>,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

/// How an extrude's body meets the parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// A new part.
    #[default]
    New,
    /// Joined to the parts it touches.
    Add,
    /// Cut from the parts it overlaps.
    Remove,
    /// Only the overlap with the parts is kept.
    Intersect,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Extrude {
    /// The sketch's name ("Sketch 1").
    pub sketch: String,
    /// The depth in mm (the total depth when symmetric).
    pub depth: f64,
    /// Points ([x, y] in the sketch's coordinates) inside the regions to extrude, as add_sketch
    /// lists them; omitted: every region of the sketch.
    #[serde(default)]
    pub regions: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    pub operation: Operation,
    /// Extrude the same depth both ways (half each side).
    #[serde(default)]
    pub symmetric: bool,
    /// Extrude against the plane's normal.
    #[serde(default)]
    pub flip: bool,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Screenshot {
    /// The longest side of the image in pixels (it is scaled down to fit); omitted: 1600.
    #[serde(default)]
    pub max_size: Option<u32>,
}
