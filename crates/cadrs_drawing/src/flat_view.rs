//! Flat pattern views (P3I.7, SM16): a sheet metal part laid flat, on a drawing sheet.
//!
//! A flat pattern view is an ordinary [`View`] with [`View::flat`] set: it can only be placed
//! from the Insert view dialog's **Flat patterns** filter (SM16.2), and projected views of it are
//! allowed. Its "projection" is not the kernel's hidden-line removal but the flat pattern itself
//! ([`flat_projection`], which `cadrs_core` feeds from `cadrs_sheetmetal::FlatPart`):
//!
//! - **Face on** (the Top or Bottom orientation): the outline and its cut-outs (a round cut-out
//!   is one circle, so it dimensions as a hole), Tear reliefs' slits, the bends' **tangent
//!   lines** (smooth edges: the view's Tangent edges setting draws them hidden, solid or
//!   phantom, SM16.5) and the bends' **centre lines** (bend lines, drawn here with their own
//!   pen, not as view lines).
//! - **Edge on** and isometric (projected views of a flat): the sheet as a slab of its thickness,
//!   its silhouette.
//!
//! Every edge gets a persistent name made from the sheet metal model and what the edge is
//! (outline, hole, slit, tangent or bend line, and a key `cadrs_core` derives from the wall or
//! bend it lies on), so dimensions on a flat view (SM16.6) resolve after the model changes,
//! like dimensions on a part view.
//!
//! **Bend lines** (SM16.3) are chain lines, up and down bends each with their own line weight
//! and colour (View properties, `properties-flatpattern.png`); Show/hide → Hide bend lines hides
//! them (SM16.5).
//!
//! **Bend notes** (SM16.3, SM16.4) come out by themselves next to each bend line, along it:
//! "UP 90.0° R1.5" (direction, angle, inner radius in the drawing's units). Dragging a note's
//! node moves it off the line with a leader to it; dropping it near its bend line puts it back
//! on the line ([`place_note`]). Hide bend notes hides them all. Their places are kept in the
//! view ([`FlatSettings::notes`]), so they move, scale and undo with it.

use std::collections::HashMap;

use cadrs_kernel::naming::{EdgeName, FaceName, FaceOrigin};
use cadrs_kernel::{ProjClass, ProjCurve, ProjEdge, ProjSource, ProjVisibility, Projection as Hlr};
use nalgebra::{Point2, Point3};
use serde::{Deserialize, Serialize};

use crate::annotation::{ModelEdge, PlacedText, text_width};
use crate::style::DrawingStyle;
use crate::view::{Frame3, View, dashes};

type P2 = [f64; 2];

/// The chain pattern of bend lines on paper (mm): long, gap, short, gap.
pub const BEND_PATTERN: [f64; 4] = [6.0, 1.2, 1.5, 1.2];

/// Ink.
pub const INK: [u8; 3] = [0x1a, 0x1a, 0x1a];

// ---------------------------------------------------------------------------------------------
// Settings kept in the view

/// A bend line pen (View properties: "Up bend lines", "Down bend lines").
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BendLineStyle {
    /// Paper width (mm).
    pub weight: f64,
    pub color: [u8; 3],
}

impl Default for BendLineStyle {
    fn default() -> Self {
        Self { weight: 0.25, color: INK }
    }
}

/// The line weights the View properties dialog offers (mm).
pub const WEIGHTS: [f64; 6] = [0.13, 0.18, 0.25, 0.35, 0.5, 0.7];

/// The colours it offers: (name, colour).
pub const COLORS: [(&str, [u8; 3]); 6] = [
    ("Black", INK),
    ("Red", [0xd0, 0x30, 0x20]),
    ("Blue", [0x20, 0x5c, 0xc8]),
    ("Green", [0x20, 0x90, 0x40]),
    ("Orange", [0xe0, 0x7a, 0x10]),
    ("Grey", [0x80, 0x80, 0x80]),
];

/// Where a bend note is: on its bend line (`attached`, at the point of the line nearest `at`),
/// or off it at `at` with a leader to the line. `at` is in the view's 2D frame (model mm), so
/// the note moves and scales with the view.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BendNotePlace {
    /// The bend's joint id.
    pub bend: u32,
    pub at: P2,
    pub attached: bool,
}

/// A flat pattern view's own settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FlatSettings {
    /// Show/hide → Hide bend lines.
    #[serde(default)]
    pub bend_lines_hidden: bool,
    /// Hide bend notes.
    #[serde(default)]
    pub bend_notes_hidden: bool,
    #[serde(default)]
    pub up: BendLineStyle,
    #[serde(default)]
    pub down: BendLineStyle,
    /// Notes moved from where they come out by themselves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<BendNotePlace>,
}

impl FlatSettings {
    pub fn note(&self, bend: u32) -> Option<&BendNotePlace> {
        self.notes.iter().find(|n| n.bend == bend)
    }

    /// Sets (or adds) a bend note's place.
    pub fn set_note(&mut self, place: BendNotePlace) {
        match self.notes.iter_mut().find(|n| n.bend == place.bend) {
            Some(n) => *n = place,
            None => self.notes.push(place),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The geometry

/// A bend as the flat view shows it (flat 2D, model mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlatBendInfo {
    /// The joint id in the sheet metal model.
    pub joint: u32,
    pub name: String,
    /// Bends up towards the viewer of the flat (seen from the Top).
    pub up: bool,
    pub angle_deg: f64,
    /// Inner radius (mm).
    pub radius: f64,
    /// The whole centre line.
    pub center: [P2; 2],
    /// The parts of the centre line and of the two tangent lines over material.
    pub center_visible: Vec<[P2; 2]>,
    pub tangent_visible: Vec<[P2; 2]>,
}

/// One closed loop of the outline (an outer boundary or a cut-out), its points in order and a
/// naming key per edge (`keys[i]` for the edge from point `i` to point `i + 1`), or a circle.
#[derive(Debug, Clone, PartialEq)]
pub enum FlatLoop {
    Polygon { points: Vec<P2>, keys: Vec<u64> },
    Circle { center: P2, radius: f64, key: u64 },
}

/// What a flat pattern view is made from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlatInput {
    pub loops: Vec<FlatLoop>,
    /// Tear reliefs' slits, with naming keys.
    pub slits: Vec<([P2; 2], u64)>,
    pub bends: Vec<FlatBendInfo>,
    pub thickness: f64,
}

/// What a flat pattern view's geometry says beyond its edges.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlatData {
    pub bends: Vec<FlatBendInfo>,
    pub thickness: f64,
    /// Seen face on (Top or Bottom): bend lines and notes are drawn.
    pub face_on: bool,
    /// Seen from the other side (Bottom): up bends are down.
    pub flipped: bool,
    /// The bends' lines in the view's 2D frame (face on only).
    pub lines: Vec<(u32, Vec<[P2; 2]>)>,
}

impl FlatData {
    pub fn bend(&self, joint: u32) -> Option<&FlatBendInfo> {
        self.bends.iter().find(|b| b.joint == joint)
    }

    /// The bend's visible centre line pieces in the view's 2D frame.
    pub fn lines_of(&self, joint: u32) -> &[[P2; 2]] {
        self.lines.iter().find(|(j, _)| *j == joint).map(|(_, l)| l.as_slice()).unwrap_or(&[])
    }

    /// Whether the bend shows as up in this view.
    pub fn shown_up(&self, b: &FlatBendInfo) -> bool {
        b.up != self.flipped
    }
}

/// What a flat edge is (in its name).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatEdgeKind {
    Outline = 1,
    Hole = 2,
    Slit = 3,
    Tangent = 4,
    Bend = 5,
    Slab = 6,
}

/// The high bits of a flat edge's face origins' region: "a flat pattern view's edge".
const TAG: u64 = 0xF1A7_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

/// The persistent name of a flat edge.
pub fn edge_name(op: uuid::Uuid, kind: FlatEdgeKind, key: u64, index: u32) -> EdgeName {
    let region = TAG | kind as u64;
    EdgeName::new(
        FaceName::new(op, FaceOrigin::Cap { region, end: false }),
        FaceName::new(op, FaceOrigin::Side { region, curve: key }),
        index,
    )
}

/// The kind of a flat edge's name, if it is one.
pub fn kind_of(name: &EdgeName) -> Option<FlatEdgeKind> {
    name.faces.iter().find_map(|f| match f.origin {
        FaceOrigin::Cap { region, .. } if region & TAG_MASK == TAG => Some(match region & 0xff {
            1 => FlatEdgeKind::Outline,
            2 => FlatEdgeKind::Hole,
            3 => FlatEdgeKind::Slit,
            4 => FlatEdgeKind::Tangent,
            5 => FlatEdgeKind::Bend,
            _ => FlatEdgeKind::Slab,
        }),
        _ => None,
    })
}

/// The joint of a bend line's name.
pub fn bend_of(name: &EdgeName) -> Option<u32> {
    if kind_of(name) != Some(FlatEdgeKind::Bend) {
        return None;
    }
    name.faces.iter().find_map(|f| match f.origin {
        FaceOrigin::Side { curve, .. } => Some(curve as u32),
        _ => None,
    })
}

/// Whether a projected edge is a flat view's bend line (drawn by [`bend_lines`], not as a view
/// line).
pub fn is_bend_edge(e: &ProjEdge) -> bool {
    e.source.as_ref().and_then(|s| s.edge_name.as_ref()).is_some_and(|n| kind_of(n) == Some(FlatEdgeKind::Bend))
}

/// A stable 64-bit key (FNV-1a) of some bytes: for naming keys.
pub fn key_of(parts: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// Drops the middle points of straight runs of a closed loop (`tol` mm off the line): pieces of
/// one straight outline edge (a wall's side, its bend region's and the next wall's) become one.
pub fn simplify_loop(points: &[P2], tol: f64) -> Vec<P2> {
    let mut pts: Vec<P2> = Vec::with_capacity(points.len());
    for p in points {
        if pts.last().is_none_or(|q: &P2| (q[0] - p[0]).hypot(q[1] - p[1]) > tol) {
            pts.push(*p);
        }
    }
    while pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) <= tol) {
        pts.pop();
    }
    loop {
        let n = pts.len();
        if n <= 3 {
            return pts;
        }
        let mut removed = false;
        for i in 0..n {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let d = [c[0] - a[0], c[1] - a[1]];
            let l = d[0].hypot(d[1]);
            if l < 1e-12 {
                continue;
            }
            let off = ((b[0] - a[0]) * d[1] - (b[1] - a[1]) * d[0]).abs() / l;
            let t = ((b[0] - a[0]) * d[0] + (b[1] - a[1]) * d[1]) / (l * l);
            if off <= tol && t > 0.0 && t < 1.0 {
                pts.remove(i);
                removed = true;
                break;
            }
        }
        if !removed {
            return pts;
        }
    }
}

/// The circle a closed loop is, if it is one: at least 12 points, all within 0.1 % of the mean
/// distance from their centroid, evenly turning.
pub fn circle_of(points: &[P2]) -> Option<(P2, f64)> {
    let n = points.len();
    if n < 12 {
        return None;
    }
    let c = points.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n as f64, a[1] + p[1] / n as f64]);
    let r = points.iter().map(|p| (p[0] - c[0]).hypot(p[1] - c[1])).sum::<f64>() / n as f64;
    if r < 1e-9 || points.iter().any(|p| ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() > 1e-3 * r) {
        return None;
    }
    // The sides all about as long (a regular polygon, not a few points on an arc).
    let sides: Vec<f64> = (0..n).map(|i| {
        let (a, b) = (points[i], points[(i + 1) % n]);
        (a[0] - b[0]).hypot(a[1] - b[1])
    }).collect();
    let mean = sides.iter().sum::<f64>() / n as f64;
    sides.iter().all(|s| (s - mean).abs() <= 0.2 * mean).then_some((c, r))
}

fn line_edge(name: EdgeName, a: P2, b: P2, class: ProjClass) -> ProjEdge {
    let (a, b) = (Point2::new(a[0], a[1]), Point2::new(b[0], b[1]));
    ProjEdge {
        visibility: ProjVisibility::Visible,
        class,
        curve: ProjCurve::Line { start: a, end: b },
        points: vec![a, b],
        source: Some(ProjSource { edge_name: Some(name), ..Default::default() }),
    }
}

fn p3(p: P2, z: f64) -> [f64; 3] {
    [p[0], p[1], z]
}

/// The flat pattern seen through `frame`: its edges, the exact model geometry of each named
/// edge (lines and circles at z = 0, the flat's plane, so dimensions measure true lengths), and
/// the bends. `op` names the edges (the sheet metal model feature).
pub fn flat_projection(op: uuid::Uuid, input: &FlatInput, frame: &Frame3) -> (Hlr, HashMap<EdgeName, ModelEdge>, FlatData) {
    let vf = frame.view_frame();
    let to2 = |p: P2, z: f64| {
        let q = vf.to_2d(&Point3::new(p[0], p[1], z));
        [q.x, q.y]
    };
    let face_on = vf.dir.z.abs() > 1.0 - 1e-9;
    let flipped = face_on && vf.dir.z > 0.0;
    let mut hlr = Hlr::default();
    let mut edges = HashMap::new();
    let mut data = FlatData { bends: input.bends.clone(), thickness: input.thickness, face_on, flipped, lines: Vec::new() };
    // Several edges with one key get indices in order.
    let mut used: HashMap<(u8, u64), u32> = HashMap::new();
    let mut next = |kind: FlatEdgeKind, key: u64| {
        let i = used.entry((kind as u8, key)).or_insert(0);
        let n = edge_name(op, kind, key, *i);
        *i += 1;
        n
    };
    if !face_on {
        // The slab's silhouette: the hull of the outline at both faces of the sheet.
        let mut pts: Vec<P2> = Vec::new();
        for l in &input.loops {
            let ring: Vec<P2> = match l {
                FlatLoop::Polygon { points, .. } => points.clone(),
                FlatLoop::Circle { center, radius, .. } => (0..32)
                    .map(|i| {
                        let t = std::f64::consts::TAU * i as f64 / 32.0;
                        [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
                    })
                    .collect(),
            };
            for p in ring {
                pts.push(to2(p, 0.0));
                pts.push(to2(p, -input.thickness));
            }
        }
        let hull = convex_hull(&pts);
        for i in 0..hull.len() {
            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
            if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9 {
                continue;
            }
            let name = next(FlatEdgeKind::Slab, i as u64);
            hlr.edges.push(line_edge(name, a, b, ProjClass::Sharp));
        }
        return (hlr, edges, data);
    }
    for l in &input.loops {
        match l {
            FlatLoop::Polygon { points, keys } => {
                let n = points.len();
                for i in 0..n {
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9 {
                        continue;
                    }
                    let name = next(FlatEdgeKind::Outline, keys.get(i).copied().unwrap_or(i as u64));
                    edges.insert(name, ModelEdge::Line { a: p3(a, 0.0), b: p3(b, 0.0) });
                    hlr.edges.push(line_edge(name, to2(a, 0.0), to2(b, 0.0), ProjClass::Sharp));
                }
            }
            FlatLoop::Circle { center, radius, key } => {
                let name = next(FlatEdgeKind::Hole, *key);
                let m = 96;
                let ring: Vec<P2> = (0..=m)
                    .map(|i| {
                        let t = std::f64::consts::TAU * i as f64 / m as f64;
                        [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
                    })
                    .collect();
                edges.insert(
                    name,
                    ModelEdge::Circle { center: p3(*center, 0.0), normal: [0.0, 0.0, 1.0], radius: *radius, points: ring.iter().map(|p| p3(*p, 0.0)).collect() },
                );
                let pts: Vec<Point2<f64>> = ring.iter().map(|p| {
                    let q = to2(*p, 0.0);
                    Point2::new(q[0], q[1])
                }).collect();
                let c = to2(*center, 0.0);
                hlr.edges.push(ProjEdge {
                    visibility: ProjVisibility::Visible,
                    class: ProjClass::Sharp,
                    curve: ProjCurve::Arc {
                        center: Point2::new(c[0], c[1]),
                        radius: *radius,
                        start: pts[0],
                        mid: pts[m / 2],
                        end: pts[0],
                        full: true,
                    },
                    points: pts,
                    source: Some(ProjSource { edge_name: Some(name), ..Default::default() }),
                });
            }
        }
    }
    for (s, key) in &input.slits {
        let name = next(FlatEdgeKind::Slit, *key);
        edges.insert(name, ModelEdge::Line { a: p3(s[0], 0.0), b: p3(s[1], 0.0) });
        hlr.edges.push(line_edge(name, to2(s[0], 0.0), to2(s[1], 0.0), ProjClass::Sharp));
    }
    for b in &input.bends {
        for s in &b.tangent_visible {
            let name = next(FlatEdgeKind::Tangent, b.joint as u64);
            edges.insert(name, ModelEdge::Line { a: p3(s[0], 0.0), b: p3(s[1], 0.0) });
            hlr.edges.push(line_edge(name, to2(s[0], 0.0), to2(s[1], 0.0), ProjClass::Smooth));
        }
        let mut lines = Vec::new();
        for s in &b.center_visible {
            let name = next(FlatEdgeKind::Bend, b.joint as u64);
            edges.insert(name, ModelEdge::Line { a: p3(s[0], 0.0), b: p3(s[1], 0.0) });
            let (a2, b2) = (to2(s[0], 0.0), to2(s[1], 0.0));
            hlr.edges.push(line_edge(name, a2, b2, ProjClass::Sharp));
            lines.push([a2, b2]);
        }
        data.lines.push((b.joint, lines));
    }
    (hlr, edges, data)
}

/// The convex hull of points (counter-clockwise).
fn convex_hull(points: &[P2]) -> Vec<P2> {
    let mut p: Vec<P2> = points.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12);
    if p.len() < 3 {
        return p;
    }
    let cross = |o: P2, a: P2, b: P2| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lower: Vec<P2> = Vec::new();
    for q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *q) <= 1e-12 {
            lower.pop();
        }
        lower.push(*q);
    }
    let mut upper: Vec<P2> = Vec::new();
    for q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *q) <= 1e-12 {
            upper.pop();
        }
        upper.push(*q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

// ---------------------------------------------------------------------------------------------
// Bend lines and notes on the sheet

/// A bend line on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct BendLine {
    pub bend: u32,
    pub up: bool,
    /// Sheet mm.
    pub points: Vec<P2>,
    pub style: BendLineStyle,
}

impl BendLine {
    /// Its dashes (sheet mm polylines).
    pub fn dashes(&self) -> Vec<Vec<P2>> {
        dashes(&self.points, &BEND_PATTERN)
    }
}

/// The bend lines `view` shows (none when hidden, or seen edge on).
pub fn bend_lines(view: &View, data: &FlatData) -> Vec<BendLine> {
    let Some(fs) = &view.flat else { return Vec::new() };
    if fs.bend_lines_hidden || !data.face_on {
        return Vec::new();
    }
    let mut out = Vec::new();
    for b in &data.bends {
        let up = data.shown_up(b);
        let style = if up { fs.up } else { fs.down };
        for s in data.lines_of(b.joint) {
            out.push(BendLine { bend: b.joint, up, points: vec![view.to_sheet(s[0]), view.to_sheet(s[1])], style });
        }
    }
    out
}

/// A bend note's text: "UP 90.0° R1.5" (the radius in the drawing's units and precision,
/// without trailing zeros).
pub fn bend_note_text(style: &DrawingStyle, b: &FlatBendInfo, up: bool) -> String {
    let r = crate::style::format_number(b.radius / style.units.mm(), style.precision, style.length_leading_zeros, false, style.decimal_separator);
    let a = crate::style::format_number(b.angle_deg, 1, true, true, style.decimal_separator);
    format!("{} {a}° R{r}", if up { "UP" } else { "DOWN" })
}

/// A bend note on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteGraphics {
    pub bend: u32,
    /// The text (its left end at the capitals' middle) and its turn (degrees counter-clockwise).
    pub text: PlacedText,
    pub rotation: f64,
    /// The leader (detached notes).
    pub strokes: Vec<Vec<P2>>,
    pub fills: Vec<[P2; 3]>,
    /// The text's box, turned with it (for picking).
    pub corners: [P2; 4],
    /// The node to drag: the text's centre.
    pub node: P2,
}

impl NoteGraphics {
    /// Distance from a sheet point to the note (0 inside its text box).
    pub fn distance(&self, p: P2) -> f64 {
        let c = &self.corners;
        let inside = (0..4).all(|i| {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= 0.0
        });
        if inside {
            return 0.0;
        }
        let mut d = f64::MAX;
        for i in 0..4 {
            d = d.min(seg_distance(p, c[i], c[(i + 1) % 4]));
        }
        for s in &self.strokes {
            for w in s.windows(2) {
                d = d.min(seg_distance(p, w[0], w[1]));
            }
        }
        d
    }
}

fn seg_distance(p: P2, a: P2, b: P2) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    if l2 < 1e-300 {
        return (p[0] - a[0]).hypot(p[1] - a[1]);
    }
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
    (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t)
}

/// The point of segment `s` nearest `p`.
fn nearest_on(s: [P2; 2], p: P2) -> P2 {
    let d = [s[1][0] - s[0][0], s[1][1] - s[0][1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    if l2 < 1e-300 {
        return s[0];
    }
    let t = (((p[0] - s[0][0]) * d[0] + (p[1] - s[0][1]) * d[1]) / l2).clamp(0.0, 1.0);
    [s[0][0] + d[0] * t, s[0][1] + d[1] * t]
}

/// The point of the bend's (visible) line nearest `p` (view 2D), and that piece.
fn nearest_line(data: &FlatData, bend: u32, p: P2) -> Option<(P2, [P2; 2])> {
    data.lines_of(bend)
        .iter()
        .map(|s| (nearest_on(*s, p), *s))
        .min_by(|a, b| {
            let da = (a.0[0] - p[0]).hypot(a.0[1] - p[1]);
            let db = (b.0[0] - p[0]).hypot(b.0[1] - p[1]);
            da.total_cmp(&db)
        })
}

/// Where a bend note sits by default: the middle of the longest piece of its line (view 2D).
pub fn default_place(data: &FlatData, bend: u32) -> Option<BendNotePlace> {
    let s = data.lines_of(bend).iter().max_by(|a, b| {
        let la = (a[1][0] - a[0][0]).hypot(a[1][1] - a[0][1]);
        let lb = (b[1][0] - b[0][0]).hypot(b[1][1] - b[0][1]);
        la.total_cmp(&lb)
    })?;
    Some(BendNotePlace { bend, at: [(s[0][0] + s[1][0]) / 2.0, (s[0][1] + s[1][1]) / 2.0], attached: true })
}

/// How near its bend line (sheet mm) a dropped note goes back onto it.
pub const REATTACH: f64 = 4.0;

/// Where a note dropped with its node at sheet point `drop` goes: back on its bend line when
/// the drop is within [`REATTACH`] of it, else off it there.
pub fn place_note(view: &View, data: &FlatData, bend: u32, drop: P2) -> BendNotePlace {
    let at = view.from_sheet(drop);
    let near = nearest_line(data, bend, at).map(|(q, _)| {
        let s = view.to_sheet(q);
        (s[0] - drop[0]).hypot(s[1] - drop[1])
    });
    match near {
        Some(d) if d <= REATTACH => BendNotePlace { bend, at, attached: true },
        _ => BendNotePlace { bend, at, attached: false },
    }
}

/// The bend notes `view` shows (none when hidden, or seen edge on).
pub fn bend_notes(style: &DrawingStyle, view: &View, data: &FlatData) -> Vec<NoteGraphics> {
    let Some(fs) = &view.flat else { return Vec::new() };
    if fs.bend_notes_hidden || !data.face_on {
        return Vec::new();
    }
    let h = style.dim_text_height;
    let arrow = style.dim_arrow_length;
    let mut out = Vec::new();
    for b in &data.bends {
        let Some(place) = fs.note(b.joint).copied().or_else(|| default_place(data, b.joint)) else { continue };
        let Some((on, seg)) = nearest_line(data, b.joint, place.at) else { continue };
        let text = bend_note_text(style, b, data.shown_up(b));
        let w = text_width(&text) * h;
        if place.attached {
            // Along the line, reading left to right or bottom to top, just above it.
            let (a, c) = (view.to_sheet(seg[0]), view.to_sheet(seg[1]));
            let mut u = [c[0] - a[0], c[1] - a[1]];
            let l = u[0].hypot(u[1]).max(1e-12);
            u = [u[0] / l, u[1] / l];
            if u[0] < -1e-9 || (u[0].abs() <= 1e-9 && u[1] < 0.0) {
                u = [-u[0], -u[1]];
            }
            let n = [-u[1], u[0]];
            let at = view.to_sheet(on);
            let mid = [at[0] + n[0] * 0.9 * h, at[1] + n[1] * 0.9 * h];
            let left = [mid[0] - u[0] * w / 2.0, mid[1] - u[1] * w / 2.0];
            let rotation = u[1].atan2(u[0]).to_degrees();
            let pad = 0.35 * h;
            let corner = |s: f64, t: f64| [mid[0] + u[0] * s + n[0] * t, mid[1] + u[1] * s + n[1] * t];
            let hw = w / 2.0 + pad;
            let hh = h / 2.0 + pad;
            out.push(NoteGraphics {
                bend: b.joint,
                text: PlacedText { pos: left, height: h, text },
                rotation,
                strokes: Vec::new(),
                fills: Vec::new(),
                corners: [corner(-hw, -hh), corner(hw, -hh), corner(hw, hh), corner(-hw, hh)],
                node: mid,
            });
        } else {
            // Off the line: level text with a landing and a leader to the line.
            let t = view.to_sheet(place.at);
            let tip = view.to_sheet(on);
            let right = t[0] >= tip[0];
            let pad = 0.35 * h;
            let left_x = t[0] - w / 2.0;
            let land_end = if right { [left_x - pad, t[1]] } else { [left_x + w + pad, t[1]] };
            let land_start = [land_end[0] + if right { -arrow } else { arrow }, t[1]];
            let d = [tip[0] - land_start[0], tip[1] - land_start[1]];
            let dl = d[0].hypot(d[1]).max(1e-12);
            let dir = [d[0] / dl, d[1] / dl];
            let base = [tip[0] - dir[0] * arrow, tip[1] - dir[1] * arrow];
            let nn = [-dir[1] * arrow * 0.18, dir[0] * arrow * 0.18];
            let hw = w / 2.0 + pad;
            let hh = h / 2.0 + pad;
            out.push(NoteGraphics {
                bend: b.joint,
                text: PlacedText { pos: [left_x, t[1]], height: h, text },
                rotation: 0.0,
                strokes: vec![vec![tip, land_start, land_end]],
                fills: vec![[tip, [base[0] + nn[0], base[1] + nn[1]], [base[0] - nn[0], base[1] - nn[1]]]],
                corners: [[t[0] - hw, t[1] - hh], [t[0] + hw, t[1] - hh], [t[0] + hw, t[1] + hh], [t[0] - hw, t[1] + hh]],
                node: t,
            });
        }
    }
    out
}

/// The note under sheet point `p` within `tol`, nearest first.
pub fn note_at(notes: &[NoteGraphics], p: P2, tol: f64) -> Option<u32> {
    notes
        .iter()
        .map(|n| (n.distance(p), n.bend))
        .filter(|(d, _)| *d <= tol)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, b)| b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standard::Scale;
    use crate::{NamedView, ObjectRef};

    fn rect_input() -> FlatInput {
        // A 100 × 50 strip with one up bend across it at x = 40, its tangent lines 2 mm either
        // side, and a 10 mm hole.
        let ring: Vec<P2> = (0..32)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 32.0;
                [70.0 + 5.0 * t.cos(), 25.0 + 5.0 * t.sin()]
            })
            .collect();
        let hole = circle_of(&ring).unwrap();
        FlatInput {
            loops: vec![
                FlatLoop::Polygon {
                    points: simplify_loop(&[[0.0, 0.0], [40.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]], 1e-6),
                    keys: vec![1, 2, 3, 4],
                },
                FlatLoop::Circle { center: hole.0, radius: hole.1, key: 9 },
            ],
            slits: Vec::new(),
            bends: vec![FlatBendInfo {
                joint: 3,
                name: "Bend A".into(),
                up: true,
                angle_deg: 90.0,
                radius: 1.5,
                center: [[40.0, 0.0], [40.0, 50.0]],
                center_visible: vec![[[40.0, 0.0], [40.0, 50.0]]],
                tangent_visible: vec![[[38.0, 0.0], [38.0, 50.0]], [[42.0, 0.0], [42.0, 50.0]]],
            }],
            thickness: 1.0,
        }
    }

    fn view() -> View {
        let mut v = View::base(ObjectRef { element: uuid::Uuid::nil(), part: None }, NamedView::Top, Scale::new(1, 2), [100.0, 100.0]);
        v.flat = Some(FlatSettings::default());
        v
    }

    #[test]
    fn loops_simplify_and_circles_are_found() {
        let s = simplify_loop(&[[0.0, 0.0], [40.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]], 1e-6);
        assert_eq!(s, vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]]);
        assert!(circle_of(&s).is_none());
        let (c, r) = circle_of(&(0..24).map(|i| {
            let t = std::f64::consts::TAU * i as f64 / 24.0;
            [3.0 + 2.0 * t.cos(), -1.0 + 2.0 * t.sin()]
        }).collect::<Vec<_>>()).unwrap();
        assert!((c[0] - 3.0).abs() < 1e-9 && (c[1] + 1.0).abs() < 1e-9 && (r - 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_projection_names_every_edge_and_keeps_bend_lines_apart() {
        let op = uuid::Uuid::from_u128(7);
        let (hlr, edges, data) = flat_projection(op, &rect_input(), &NamedView::Top.frame());
        // 4 outline lines, the hole, 2 tangent lines, 1 bend line.
        assert_eq!(hlr.edges.len(), 8);
        assert_eq!(edges.len(), 8);
        assert!(data.face_on && !data.flipped);
        let bends: Vec<&ProjEdge> = hlr.edges.iter().filter(|e| is_bend_edge(e)).collect();
        assert_eq!(bends.len(), 1);
        assert_eq!(bend_of(bends[0].source.unwrap().edge_name.as_ref().unwrap()), Some(3));
        assert_eq!(hlr.edges.iter().filter(|e| e.class == ProjClass::Smooth).count(), 2);
        // The hole is a whole circle with its exact radius.
        let hole = hlr.edges.iter().find(|e| matches!(e.curve, ProjCurve::Arc { full: true, .. })).unwrap();
        let ProjCurve::Arc { radius, .. } = hole.curve else { unreachable!() };
        assert!((radius - 5.0).abs() < 1e-9);
        // Names are stable: the same input names the same edges.
        let (again, _, _) = flat_projection(op, &rect_input(), &NamedView::Top.frame());
        assert_eq!(again, hlr);
        // Bend lines are not drawn as view lines.
        let v = view();
        assert_eq!(crate::view::view_lines(&v, &hlr).len(), 7);
    }

    #[test]
    fn seen_from_the_side_the_flat_is_a_slab() {
        let (hlr, _, data) = flat_projection(uuid::Uuid::nil(), &rect_input(), &NamedView::Front.frame());
        assert!(!data.face_on);
        let (lo, hi) = hlr.bounds().unwrap();
        assert!((hi.x - lo.x - 100.0).abs() < 1e-9, "{lo:?} {hi:?}");
        assert!((hi.y - lo.y - 1.0).abs() < 1e-9, "{lo:?} {hi:?}");
        assert_eq!(hlr.edges.len(), 4);
        // Seen from below, up bends are down.
        let (_, _, below) = flat_projection(uuid::Uuid::nil(), &rect_input(), &NamedView::Bottom.frame());
        assert!(below.face_on && below.flipped && !below.shown_up(&below.bends[0]));
    }

    #[test]
    fn bend_notes_read_like_onshape() {
        let style = DrawingStyle::default();
        let b = &rect_input().bends[0];
        let mm = DrawingStyle { units: cadrs_sketch::units::LengthUnit::Millimeter, precision: 2, ..style.clone() };
        assert_eq!(bend_note_text(&mm, b, true), "UP 90.0° R1.5");
        assert_eq!(bend_note_text(&mm, b, false), "DOWN 90.0° R1.5");
        let comma = DrawingStyle { decimal_separator: crate::style::DecimalSeparator::Comma, ..mm.clone() };
        let b2 = FlatBendInfo { radius: 2.29, ..b.clone() };
        assert_eq!(bend_note_text(&comma, &b2, true), "UP 90,0° R2,29");
        let inch = DrawingStyle { units: cadrs_sketch::units::LengthUnit::Inch, precision: 3, length_leading_zeros: false, ..mm };
        let b3 = FlatBendInfo { radius: 0.0508, ..b.clone() };
        assert_eq!(bend_note_text(&inch, &b3, true), "UP 90.0° R.002");
    }

    #[test]
    fn notes_sit_on_their_line_until_dragged_off_and_go_back_when_dropped_near_it() {
        let style = DrawingStyle::default();
        let (_, _, data) = flat_projection(uuid::Uuid::nil(), &rect_input(), &NamedView::Top.frame());
        let mut v = view();
        let notes = bend_notes(&style, &v, &data);
        assert_eq!(notes.len(), 1);
        // On a vertical line: turned 90°, beside the line's middle (x = 40 → sheet 120).
        let n = &notes[0];
        assert!((n.rotation - 90.0).abs() < 1e-9);
        assert!(n.node[0] < 120.0 && n.node[0] > 115.0, "{:?}", n.node);
        assert!((n.node[1] - (100.0 + 12.5)).abs() < 1e-9);
        assert_eq!(note_at(&notes, n.node, 0.1), Some(3));
        // Dragged away: level, with a leader ending on the line.
        let drop = [160.0, 140.0];
        let p = place_note(&v, &data, 3, drop);
        assert!(!p.attached);
        v.flat.as_mut().unwrap().set_note(p);
        let n = &bend_notes(&style, &v, &data)[0];
        assert_eq!(n.rotation, 0.0);
        assert_eq!(n.node, drop);
        let tip = n.strokes[0][0];
        assert!((tip[0] - 120.0).abs() < 1e-9, "{tip:?}");
        // Dropped near the line again: back on it, where it was dropped along it.
        let p = place_note(&v, &data, 3, [121.5, 105.0]);
        assert!(p.attached);
        v.flat.as_mut().unwrap().set_note(p);
        let n = &bend_notes(&style, &v, &data)[0];
        assert!((n.rotation - 90.0).abs() < 1e-9 && (n.node[1] - 105.0).abs() < 1e-9);
        // Hidden.
        v.flat.as_mut().unwrap().bend_notes_hidden = true;
        assert!(bend_notes(&style, &v, &data).is_empty());
    }

    #[test]
    fn bend_lines_take_their_direction_pen_and_hide() {
        let (_, _, data) = flat_projection(uuid::Uuid::nil(), &rect_input(), &NamedView::Top.frame());
        let mut v = view();
        v.flat.as_mut().unwrap().up = BendLineStyle { weight: 0.5, color: [0xd0, 0x30, 0x20] };
        let lines = bend_lines(&v, &data);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].up && lines[0].style.weight == 0.5);
        assert_eq!(lines[0].points, vec![[120.0, 100.0], [120.0, 125.0]]);
        assert!(lines[0].dashes().len() > 2);
        v.flat.as_mut().unwrap().bend_lines_hidden = true;
        assert!(bend_lines(&v, &data).is_empty());
    }
}
