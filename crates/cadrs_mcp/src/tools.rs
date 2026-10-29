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

/// One segment of an outline: it runs from where the previous one ended.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PathSegment {
    /// A straight line to (x, y).
    Line { to: [f64; 2] },
    /// An arc to (x, y) about `center`, counter-clockwise when `ccw` is true, else clockwise. Its
    /// radius is the distance from `center` to where the previous segment ended, and `to` must be
    /// that distance from `center` too.
    Arc {
        to: [f64; 2],
        center: [f64; 2],
        #[serde(default)]
        ccw: bool,
    },
}

/// A curve or shape of a sketch, in the sketch plane's 2D coordinates (mm).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SketchEntity {
    /// A closed loop of lines and arcs: from `start`, each segment runs on from where the previous
    /// one ended, and the last must end at `start`. For rounded corners and fillets: a corner
    /// rounded with radius r is an arc whose center lies r in from both edges, and its two ends are
    /// where it leaves each edge. Holes are circles inside the loop.
    Outline {
        start: [f64; 2],
        segments: Vec<PathSegment>,
    },
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
    Arc {
        cx: f64,
        cy: f64,
        sx: f64,
        sy: f64,
        ex: f64,
        ey: f64,
    },
    /// A sketch point.
    Point { x: f64, y: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddSketch {
    /// A default plane to sketch on; or give `face` or `plane_feature` instead.
    #[serde(default)]
    pub plane: Option<Plane>,
    /// A planar part face to sketch on: its `ref` as list_faces gives it.
    #[serde(default)]
    pub face: Option<serde_json::Value>,
    /// A Plane feature to sketch on, by name ("Plane 1").
    #[serde(default)]
    pub plane_feature: Option<String>,
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
    /// lists them; omitted: every region of the sketch. A circle inside a loop is a region too (and
    /// the loop's hole), so with holes give the loop's point, one not inside a hole, or the circles
    /// are extruded as pins filling them.
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
    /// Turn the 3D view first and zoom to fit: front, back, left, right, top, bottom or iso.
    /// Omitted: the view as it is.
    #[serde(default)]
    pub view: Option<String>,
    /// Look from an eye point at a target instead (ignores `view`), for close-ups.
    #[serde(default)]
    pub camera: Option<ShotCamera>,
}

/// A camera placed by eye and target, in model millimetres (Z up).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShotCamera {
    /// Where the eye is: [x, y, z].
    pub eye: [f32; 3],
    /// The point centred in the view: [x, y, z]. The view is about as wide as a 50 degree lens
    /// at the eye would see at the target (0.93 x the eye's distance).
    pub target: [f32; 3],
}

/// Renames a feature of the active (or named) Part Studio.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameFeature {
    /// The feature's current name ("Extrude 3").
    pub name: String,
    /// The new name.
    pub to: String,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

/// Shows or hides a sketch or Plane feature with its eye.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetFeatureVisible {
    /// The sketch's or plane's name.
    pub name: String,
    /// Shown (true) or hidden (false).
    pub visible: bool,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

/// What `get_frame_stats` lists.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GetFrameStats {
    /// How many of the slowest schedules, systems and render passes to list; omitted: 25.
    #[serde(default)]
    pub top: Option<usize>,
}

/// Moves a Part Studio's rollback bar: the features below it are not built.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetRollback {
    /// How many features stay above the bar (built), counted from the top of the feature list
    /// without the default geometry. Omitted (and `after` too), or past the end: roll to the end.
    #[serde(default)]
    pub index: Option<usize>,
    /// Instead of `index`: the bar goes just below the feature with this name.
    #[serde(default)]
    pub after: Option<String>,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

/// Lists UI nodes with where they are on screen.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetUiLayout {
    /// Only nodes whose `Name` contains this (case-insensitive); omitted: every named node.
    #[serde(default)]
    pub name: Option<String>,
    /// At most this many nodes, top to bottom; omitted: 200.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Changes this machine's preferences, as the Preferences dialog's OK does (saved).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SetPreferences {
    /// Vertical sync: frames wait for the display's refresh (true) or are drawn as fast as they
    /// are made (false). Omitted: unchanged.
    #[serde(default)]
    pub vsync: Option<bool>,
}

/// Renames a part and/or sets its colour.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetPart {
    /// The part's current name ("Part 3"), as get_document lists it.
    pub part: String,
    /// A new name for the part.
    #[serde(default)]
    pub name: Option<String>,
    /// Its colour as [r, g, b] (0-255).
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    /// Its opacity, 0-255 (255: opaque, the default); with `color`.
    #[serde(default)]
    pub alpha: Option<u8>,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

/// A feature of the active (or named) Part Studio, by name.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeatureName {
    /// The feature's name ("Fillet 1").
    pub name: String,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddFeature {
    /// The feature type: Extrude, Revolve, Fillet, Chamfer, Hole, Shell, Plane, Sweep, Loft,
    /// Split, Pattern, Mirror, Draft, Boolean, DeletePart, Transform, Thicken, Helix,
    /// DeleteFace ({"faces": [refs]}: faces removed, their neighbours healing the gap: a fillet,
    /// chamfer, hole, boss or groove taken away) or MoveFace ({"faces": [refs], "distance": mm}:
    /// faces offset along their outward normals, negative inward: a wall moved, a bore's radius
    /// changed), or Simplify ({"faces": [a face of each part]}: faces and edges on the same
    /// surface merged, which repairs imported parts split along seams).
    #[serde(rename = "type")]
    pub kind: String,
    /// The feature's fields as JSON, merged over the type's defaults (so only the fields that
    /// differ are needed). A Plane offset from a default plane, to sketch on with add_sketch's
    /// plane_feature: {"entities": [{"Plane": "Front"}], "offset": 22, "offset_expr": "22 mm",
    /// "flip": true} (flip: against the plane's normal). get_feature shows a feature's full JSON; face and edge references
    /// come from list_faces and list_edges ("ref"). Instead of `regions`, `region_points`:
    /// {"sketch": "Sketch 1", "points": [[x, y], …]} picks the sketch's regions containing the
    /// points (all of them without "points"). A revolve about a sketch line: "axis":
    /// {"SketchCurve": {"sketch": <sketch id>, "curve": <curve id>}} (from add_sketch).
    #[serde(default)]
    pub params: serde_json::Value,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EditFeature {
    /// The feature's name.
    pub name: String,
    /// The fields to change, as JSON merged over the feature's current fields.
    pub params: serde_json::Value,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EditSketch {
    /// The sketch's name ("Sketch 1").
    pub sketch: String,
    /// One edit, as an object: "type" (snake_case, below) and that edit's fields. Or an array of
    /// edits, applied as one undoable step. Points are {"x", "y"} in the sketch's mm. Curves, points,
    /// dimensions and constraints are referred to by their ids, {"idx": n, "version": n}, as get_feature
    /// shows them.
    ///
    /// Geometry: add_polyline {points, closed, construction}; add_circle {center, radius, construction};
    /// add_center_rectangle {center, corners: [4], construction}; add_arc {center, start, end,
    /// construction} (counter-clockwise from start to end); add_point {pos}; add_ellipse {center,
    /// major, minor, construction}; add_bezier {points: [4], construction}; add_spline {points,
    /// periodic, start_tangent?, end_tangent?, construction}; set_spline_tangents {curve, start?, end?};
    /// add_polygon {center, radius, angle (radians), sides, inscribed, construction}; slot {source,
    /// width, equal_to?, construction}; text: add_text {origin, dir, height, style}, edit_text {id,
    /// style} (style: {text, font, bold, italic, mirror_h, mirror_v}).
    ///
    /// Editing: fillet {corner (a point id), radius, equal_to?}; chamfer {corner, d1, d2, equal_to?};
    /// trim {picks: [[curve, pos]], points}; extend {curve, end (a point id), to, by?}; split {curve,
    /// at: [pos]}; mirror {axis (a curve id), curves}; scale {center, factor}; offset {chain: [[curve,
    /// runs_backwards]], distance, left, label: [x, y]}; delete {curves, points, dimensions,
    /// constraints}; set_construction {curves, construction}; move_points {moves: [[point, pos]]};
    /// set_geometry {points: [[point, pos]], radii: [[curve, r]]}.
    ///
    /// Constraints: add_constraint {constraints: [...], label?}; add_constraints {specs: [...]} (the
    /// specs resolve by position, see below). Each constraint is the engine's serde form, e.g.
    /// {"Coincident": [{"Point": id}, {"Point": id}]}, {"Horizontal": {"Line": {"Curve": id}}},
    /// {"Equal": [{"Curve": id}, {"Curve": id}]}, {"FixPoint": {"Point": id}}. Points: {"Point": id} or
    /// "Origin"; curves: {"Curve": id}, "XAxis" or "YAxis". Specs: points {"At": pos}, "Origin",
    /// {"Point": id}, {"Linked": link}; curves {"Id": id}, {"Between": [pos, pos]}, {"Circle": [pos, r]},
    /// "XAxis", "YAxis". Constraint types: Coincident, PointOnCurve, Midpoint, Horizontal, Vertical,
    /// Parallel, Perpendicular, Tangent, Equal, Normal, Concentric, FixPoint, FixCurve, SymmetricPoints,
    /// SymmetricCurves, EqualOffset, Center, EqualDistance, Use, Pierce, TextAspect, Curvature.
    ///
    /// Dimensions: set_dimension {dimension: {kind, value, offset, along, driven}, moves, radii}, where
    /// kind is e.g. {"Horizontal": {"a": id, "b": id}}, {"Vertical": {"a", "b"}}, {"Aligned": {"a", "b"}},
    /// {"Diameter": {"curve"}}, {"Radius": {"curve"}}, {"Angle": {"a", "b", "flip_a", "flip_b"}},
    /// {"PointLine": {"p", "line"}}, {"Offset": {"source", "target"}}, {"Sides": {"circle", "inscribed"}};
    /// set_dimension_value {id, value}; set_dimension_driven {id, driven}; set_dimension_expr {id,
    /// expr (a string, or null)}; move_dimension_label {id, offset, along}.
    ///
    /// Links and projections (for references to other features): normal_to_plane {line, trace: [pos,
    /// pos], link}; use {items: [[projected, link]]}; use_construction {items}; offset_loop {items,
    /// distance, left, label}; paste {sketch, offset} (a copied sketch as get_feature shows it).
    pub op: serde_json::Value,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

/// A sketch entity's id: as get_feature shows it, {"idx": …, "version": …}.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct EntityId {
    pub idx: u32,
    pub version: u32,
}

/// The ids of each kind of sketch entity.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchIds {
    pub curves: Vec<EntityId>,
    pub points: Vec<EntityId>,
    pub dimensions: Vec<EntityId>,
    pub constraints: Vec<EntityId>,
}

/// A closed region of a sketch, with a point inside it to pass to extrude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchRegion {
    /// A point inside the region, in the sketch's mm.
    pub point: [f64; 2],
    pub area_mm2: f64,
}

/// A constraint, dimension or arc that the sketch's solve cannot satisfy together with the others.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchConflict {
    Constraint { id: EntityId },
    Dimension { id: EntityId },
    Arc { curve: EntityId },
    Other { description: String },
}

/// What a sketch edit left: the ids it created and removed, the regions, and the solve's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchEdited {
    /// The sketch's name.
    pub sketch: String,
    pub created: SketchIds,
    pub removed: SketchIds,
    pub regions: Vec<SketchRegion>,
    /// True if the constraints and dimensions pin every point and curve.
    pub fully_constrained: bool,
    pub has_conflicts: bool,
    pub conflicts: Vec<SketchConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PartStudio {
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImportStep {
    /// The STEP file's path on this computer.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportStep {
    /// Where to write the STEP file (every part of the Part Studio in one file).
    pub path: String,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunScenario {
    /// The steps in the app's scenario RON: a list like `[Click(ui("extrude")), Key("Enter"),
    /// Type("25"), Wait(5), Screenshot("after")]`, or a whole `Scenario(steps: [...])`.
    pub scenario: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_outline_takes_lines_and_arcs_and_a_clockwise_arc_by_default() {
        let e: SketchEntity = serde_json::from_value(json!({
            "type": "outline",
            "start": [0, 0],
            "segments": [
                { "type": "line", "to": [10, 0] },
                { "type": "arc", "to": [0, 10], "center": [0, 0], "ccw": true },
                { "type": "arc", "to": [0, 0], "center": [5, 5] }
            ]
        }))
        .unwrap();
        let SketchEntity::Outline { segments, .. } = e else {
            panic!("not an outline")
        };
        assert!(matches!(segments[2], PathSegment::Arc { ccw: false, .. }));
    }
}

/// A drawing of a Part Studio on a sheet: the Create Drawing dialog's four views, without the dialog.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateDrawing {
    /// The drawing tab's name; omitted: "Drawing N".
    #[serde(default)]
    pub name: Option<String>,
    /// The sheet template's file name: ANSI_C_MM.dwt (the default, third angle), ANSI_A_MM.dwt,
    /// ISO_A4_MM.dwt (first angle), ISO_A0_MM.dwt, ANSI_B_INCH.dwt (inches) …
    #[serde(default)]
    pub template: Option<String>,
    /// The Part Studio tab's name; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
    /// Place the front, top, side and isometric views (the default); false: an empty sheet.
    #[serde(default)]
    pub four_views: Option<bool>,
    /// The scale of every view as "1:1" or "2:1"; omitted: the largest scale that fits the sheet.
    #[serde(default)]
    pub scale: Option<String>,
}

/// An annotation on a drawing view: a dimension, a centerline, a hole callout and the like.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddAnnotation {
    /// The drawing tab's name; omitted: the active tab (which must be a drawing).
    #[serde(default)]
    pub drawing: Option<String>,
    /// The view's name on the sheet: "Front", "Top", "Right" or "Isometric".
    pub view: String,
    /// The annotation as the drawing file stores it, e.g. {"Dimension": {"kind": {"Distance":
    /// {"a": <pick>, "b": <pick>, "orient": "Horizontal"}}, "text": [x, y]}} or {"Dimension":
    /// {"kind": {"Diameter": <edge ref>}, "text": [x, y]}}. A pick is {"Edge": <edge ref>} or
    /// {"Point": …}; an edge ref is the "ref" that list_edges gives. Positions are view 2D in mm,
    /// before the view's scale. Orient: "Horizontal", "Vertical" or "Aligned".
    pub annotation: serde_json::Value,
}

/// Renames a tab: a Part Studio (the drawing's title block shows its name) or a drawing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameTab {
    /// The tab's current name.
    pub name: String,
    /// The new name.
    pub to: String,
}

/// A tab, by name.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TabName {
    /// The tab's name.
    pub name: String,
}

/// An Assembly tab, by name.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssemblyName {
    /// The Assembly tab's name; omitted: the active tab.
    #[serde(default)]
    pub assembly: Option<String>,
}

/// Inserts a Part Studio, or one of its parts, into an assembly.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InsertInstance {
    /// The Assembly tab's name; omitted: the active tab.
    #[serde(default)]
    pub assembly: Option<String>,
    /// The Part Studio tab to insert from.
    pub part_studio: String,
    /// One part of it, by name ("Part 1"); omitted: the whole Part Studio as one rigid
    /// instance (all its parts, moving as one).
    #[serde(default)]
    pub part: Option<String>,
    /// Where the instance's origin goes in the assembly, [x, y, z] in mm; omitted: the
    /// assembly's origin, so the parts sit where they are in their Part Studio.
    #[serde(default)]
    pub translation: Option<[f64; 3]>,
    /// Fix the instance in place (the solver keeps it put, as ground); default false.
    #[serde(default)]
    pub fixed: bool,
}

/// A PCB Studio board, by name.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardName {
    /// The board's name (as its PCB Studio's Boards list shows it).
    pub board: String,
    /// The PCB Studio tab's name; omitted: any PCB Studio.
    #[serde(default)]
    pub pcb_studio: Option<String>,
}

/// Derives a PCB Studio board into a Part Studio: the board's solid and its components.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeriveBoard {
    #[serde(flatten)]
    pub board: BoardName,
    /// The Part Studio tab; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
    /// Where the board goes: a mate connector feature's name; omitted (and no `on_sketch`): the
    /// origin.
    #[serde(default)]
    pub location: Option<String>,
    /// Or: a sketch whose plane the board lies on, its layout's (0, 0) on the sketch's origin.
    #[serde(default)]
    pub on_sketch: Option<String>,
    /// The board's point on the location: "fixed" (its outline's centre now; the default with a
    /// location), "origin" (the layout's 0, 0; the default on a sketch), "centre", or a
    /// footprint's reference ("J1").
    #[serde(default)]
    pub anchor: Option<String>,
    /// The board's face on the location: "bottom" (default) or "top".
    #[serde(default)]
    pub seat: Option<String>,
    /// "3d" (default) or "layout" (the layout drawn on the board's faces).
    #[serde(default)]
    pub view: Option<String>,
}

/// Creates a PCB board whose outline is a Part Studio sketch's largest region (it keeps
/// following the sketch), and derives it onto the sketch.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateBoardFromSketch {
    /// The sketch's name.
    pub sketch: String,
    /// The board's name.
    pub name: String,
    /// The Part Studio tab; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
    /// The PCB Studio to add it to; omitted: a new one.
    #[serde(default)]
    pub pcb_studio: Option<String>,
    /// Its thickness (mm); omitted: 1.6.
    #[serde(default)]
    pub thickness: Option<f64>,
    /// Its face on the sketch: "bottom" (default) or "top".
    #[serde(default)]
    pub seat: Option<String>,
    /// The sketch's circles inside the outline as mounting holes (non-plated hole footprints)
    /// rather than cut-outs; default false.
    #[serde(default)]
    pub circles_as_holes: Option<bool>,
    /// A point (sketch x, y in mm) inside the region to take; omitted: the largest region.
    #[serde(default)]
    pub inside: Option<[f64; 2]>,
}

/// Moves a footprint on a board: to a place (mm, its origin), turned, flipped; or so one of its
/// pads is centred on a pin or hole of a part, in a Part Studio the board is derived into.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MoveFootprint {
    #[serde(flatten)]
    pub board: BoardName,
    /// The footprint's reference ("J4").
    pub reference: String,
    /// Its origin's x and y on the layout (mm).
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    /// Its angle (degrees, counter-clockwise).
    #[serde(default)]
    pub angle: Option<f64>,
    /// "top" or "bottom".
    #[serde(default)]
    pub side: Option<String>,
    /// With `onto_part`: the pad to centre ("1").
    #[serde(default)]
    pub pad: Option<String>,
    /// A part (its name in the Parts list) with pins or holes square to the board: the pad goes
    /// onto the axis nearest it (or `axis`, by its index in get_board_fit's list).
    #[serde(default)]
    pub onto_part: Option<String>,
    /// Or a mate connector (its name in the Feature list) whose Z axis stands square to the
    /// board: the pad goes onto that axis.
    #[serde(default)]
    pub onto_connector: Option<String>,
    /// Which of the part's axes (0 first); omitted: the nearest to the pad.
    #[serde(default)]
    pub axis: Option<usize>,
    /// The Part Studio for `onto_part`; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
    /// With `onto_part`, a board derived onto several locations: which copy's place the part's
    /// axes are read against (1 first); omitted: 1.
    #[serde(default)]
    pub copy: Option<usize>,
}

/// Adds a footprint to a board: a copy of one already on it, with its own reference, value,
/// place and nets.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddFootprint {
    #[serde(flatten)]
    pub board: BoardName,
    /// The footprint to copy ("J4").
    pub copy_of: String,
    /// The new reference ("J6") and value ("AC Earth IN").
    pub reference: String,
    #[serde(default)]
    pub value: String,
    /// Its origin on the layout (mm).
    pub x: f64,
    pub y: f64,
    /// Its pads' nets: {"1": "EARTH"} (pads not named are on no net).
    #[serde(default)]
    pub nets: std::collections::BTreeMap<String, String>,
}

/// Removes a footprint from a board, and its symbol from the schematic.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemoveFootprint {
    #[serde(flatten)]
    pub board: BoardName,
    pub reference: String,
}

/// Links a board's outline to a Part Studio sketch (its largest region), or breaks the link.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinkOutline {
    #[serde(flatten)]
    pub board: BoardName,
    /// The sketch; omitted: break the link (the outline stays as it is).
    #[serde(default)]
    pub sketch: Option<String>,
    /// The sketch's Part Studio; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
    /// Move everything on the layout by the outline's move (its box's centre), so the parts
    /// keep their place on the board; default true.
    #[serde(default)]
    pub keep_parts: Option<bool>,
    /// A point (sketch x, y in mm) inside the region to take; omitted: the largest region.
    #[serde(default)]
    pub inside: Option<[f64; 2]>,
}

/// A board and the Part Studio to fit it in: the pins and holes of the parts square to it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardFit {
    #[serde(flatten)]
    pub board: BoardName,
    /// The Part Studio the board is derived into; omitted: the active tab.
    #[serde(default)]
    pub part_studio: Option<String>,
    /// A board derived onto several locations: which copy (1 first); omitted: 1.
    #[serde(default)]
    pub copy: Option<usize>,
}

/// Makes a named version of the open document as it is now (P3D.3).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateVersion {
    /// The version's name ("Before the stairs").
    pub name: String,
    /// What it holds; omitted: none.
    #[serde(default)]
    pub description: Option<String>,
}

/// Names a version of the open document.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VersionName {
    /// The version's name, as list_versions shows it (the newest of that name).
    pub name: String,
}
