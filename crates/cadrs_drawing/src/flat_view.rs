//! Flat pattern views (P3I.7, SM16): a sheet metal part laid flat, on a drawing sheet.
//!
//! A flat pattern view is an ordinary [`View`] with [`View::flat`] set: it can only be placed
//! from the Insert view dialog's **Flat patterns** filter (SM16.2), and projected views of it are
//! allowed. Its "projection" is not the kernel's hidden-line removal but the flat pattern itself
//! ([`flat_projection`], which `cadrs_core` feeds from `cadrs_sheetmetal::FlatPart`):
//!
//! - **Face on** (the Top or Bottom orientation): the outline and its cut-outs (a round cut-out
//!   is one circle, so it dimensions as a hole; runs of outline edges on one circle, such as a
//!   corner break's round, are true arcs, [`find_arcs`]), Tear reliefs' slits, forms' outlines
//!   and counterbores' and countersinks' outer diameters (SM16.3), the bends' **tangent lines**
//!   (smooth edges: the view's Tangent edges setting draws them hidden, as a new flat view does,
//!   solid or phantom, SM16.5) and the bends' **centre lines** (bend lines, drawn here with
//!   their own pen, not as view lines). Round holes, counterbores and forms get a centermark
//!   ([`centermarks`]).
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
//! "UP 90.0° R1.5" (direction, angle, inner radius in the drawing's units), keeping clear of
//! each other and of the outline where two short lines meet at a corner. Dragging a note's
//! node moves it off the line with a leader to the point it was attached at; dropping it near
//! its bend line puts it back on the line there ([`place_note`]). Hide bend notes hides them
//! all. Their places are kept in the view ([`FlatSettings::notes`]), so they move, scale and
//! undo with it.

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

/// Where a bend note is: on its bend line (`attached`), or off it at `at` with a leader to the
/// line. `at` is in the view's 2D frame (model mm), so the note moves and scales with the view.
///
/// `along` is the note's attach point: how far along its bend line (model mm, over the line's
/// visible pieces in order) the note sits, or its leader ends. Only reattaching changes it, so a
/// note dragged about keeps its leader on the point it was attached at. Files from before it
/// (`None`) attach at the point of the line nearest `at`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BendNotePlace {
    /// The bend's joint id.
    pub bend: u32,
    pub at: P2,
    pub attached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub along: Option<f64>,
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
/// `arcs` are runs of a polygon's edges that are pieces of one circle (a corner break's round, a
/// round relief): they are drawn, dimensioned and exported as true arcs ([`find_arcs`]).
#[derive(Debug, Clone, PartialEq)]
pub enum FlatLoop {
    Polygon { points: Vec<P2>, keys: Vec<u64>, arcs: Vec<FlatArc> },
    Circle { center: P2, radius: f64, key: u64 },
}

/// A run of a polygon loop's edges on one circle: edges `from`, `from + 1`, … (`edges` of them,
/// indices wrapping round the loop), from point `from` to point `from + edges`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatArc {
    pub from: usize,
    pub edges: usize,
    pub center: P2,
    pub radius: f64,
}

/// A form on the flat (SM16.3, SM20.3): its outline (polylines, closed or not) and its centre
/// (the centermark), with a naming key.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlatFormInfo {
    pub lines: Vec<(Vec<P2>, bool)>,
    pub center: P2,
    pub key: u64,
}

/// What a flat pattern view is made from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlatInput {
    pub loops: Vec<FlatLoop>,
    /// Tear reliefs' slits, with naming keys.
    pub slits: Vec<([P2; 2], u64)>,
    pub bends: Vec<FlatBendInfo>,
    pub thickness: f64,
    /// Forms' outlines and centres (SM16.3).
    pub forms: Vec<FlatFormInfo>,
    /// Counterbores' and countersinks' outer diameters (SM16.3): centre, radius, naming key.
    pub hole_marks: Vec<(P2, f64, u64)>,
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
    /// Where centermarks go (view 2D, face on only): round holes, counterbores and countersinks,
    /// forms.
    /// Each with the radius of the largest circle there (0 for a form's centre).
    pub centers: Vec<(P2, f64)>,
    /// The outline's edges (arcs and circles as chords) in the view's 2D frame (face on only):
    /// what bend notes keep clear of.
    pub outline: Vec<[P2; 2]>,
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
    /// A form's outline (SM16.3).
    Form = 7,
    /// A counterbore's or countersink's outer diameter (SM16.3).
    HoleMark = 8,
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
            7 => FlatEdgeKind::Form,
            8 => FlatEdgeKind::HoleMark,
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

/// The circle through three points (`None` when they are in line).
fn circumcircle(a: P2, b: P2, c: P2) -> Option<(P2, f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
    let x = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let y = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    Some(([x, y], (a[0] - x).hypot(a[1] - y)))
}

/// The runs of a closed polygon's edges that are pieces of one circle: at least three edges in
/// a row, turning the same way by at most 30° at each point, of about the same length (a
/// corner break's round, a round relief), every point on the circle within 0.1 % of its radius.
/// A polygon with no sharp corner is left alone (a whole circle is [`circle_of`]'s).
pub fn find_arcs(points: &[P2]) -> Vec<FlatArc> {
    let n = points.len();
    if n < 4 {
        return Vec::new();
    }
    let edge = |i: usize| {
        let (a, b) = (points[i % n], points[(i + 1) % n]);
        [b[0] - a[0], b[1] - a[1]]
    };
    let len = |i: usize| {
        let d = edge(i);
        d[0].hypot(d[1])
    };
    // The turn at point `j`, from edge j − 1 to edge j.
    let turn = |j: usize| {
        let (a, b) = (edge(j + n - 1), edge(j));
        (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1])
    };
    let step = 30f64.to_radians();
    let smooth = |j: usize| {
        let t = turn(j).abs();
        t > 1e-9 && t <= step
    };
    let Some(start) = (0..n).find(|j| !smooth(*j)) else { return Vec::new() };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let a = start + i;
        let sign = turn(a + 1).signum();
        let mut k = 1;
        while i + k < n && smooth(a + k) && turn(a + k).signum() == sign && {
            let r = len(a + k) / len(a + k - 1).max(1e-12);
            (0.2..=5.0).contains(&r)
        } {
            k += 1;
        }
        if k >= 3
            && let Some((c, r)) = circumcircle(points[a % n], points[(a + k / 2) % n], points[(a + k) % n])
            && (0..=k).all(|m| {
                let p = points[(a + m) % n];
                ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() <= 1e-3 * r + 1e-6
            })
        {
            out.push(FlatArc { from: a % n, edges: k, center: c, radius: r });
        }
        i += k;
    }
    out
}

/// Points along an arc of circle (`center`, `radius`) from `from` to `to` (both on it), turning
/// the way `via` (a point on the arc between them) says.
fn arc_points(center: P2, radius: f64, from: P2, via: P2, to: P2) -> Vec<P2> {
    let ang = |p: P2| (p[1] - center[1]).atan2(p[0] - center[0]);
    let wrap = |d: f64| (d + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    let (a0, am, a1) = (ang(from), ang(via), ang(to));
    let sweep = wrap(am - a0) + wrap(a1 - am);
    let m = ((sweep.abs() / 3f64.to_radians()).ceil() as usize).max(8);
    (0..=m)
        .map(|i| {
            let t = a0 + sweep * i as f64 / m as f64;
            [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
        })
        .collect()
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
    let mut data = FlatData { bends: input.bends.clone(), thickness: input.thickness, face_on, flipped, ..Default::default() };
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
    // An arc (or a whole circle) of the flat as a named edge: its model geometry and its
    // projection (a true arc, so it dimensions and exports as one).
    let arc_edge = |hlr: &mut Hlr, edges: &mut HashMap<EdgeName, ModelEdge>, outline: Option<&mut Vec<[P2; 2]>>, name: EdgeName, center: P2, radius: f64, ring: Vec<P2>, full: bool| {
        edges.insert(name, ModelEdge::Circle { center: p3(center, 0.0), normal: [0.0, 0.0, 1.0], radius, points: ring.iter().map(|p| p3(*p, 0.0)).collect() });
        let pts: Vec<Point2<f64>> = ring.iter().map(|p| {
            let q = to2(*p, 0.0);
            Point2::new(q[0], q[1])
        }).collect();
        if let Some(o) = outline {
            o.extend(pts.windows(2).map(|w| [[w[0].x, w[0].y], [w[1].x, w[1].y]]));
        }
        let c = to2(center, 0.0);
        let m = pts.len() - 1;
        hlr.edges.push(ProjEdge {
            visibility: ProjVisibility::Visible,
            class: ProjClass::Sharp,
            curve: ProjCurve::Arc { center: Point2::new(c[0], c[1]), radius, start: pts[0], mid: pts[m / 2], end: pts[m], full },
            points: pts,
            source: Some(ProjSource { edge_name: Some(name), ..Default::default() }),
        });
    };
    let ring = |center: P2, radius: f64| -> Vec<P2> {
        let m = 96;
        (0..=m)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / m as f64;
                [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
            })
            .collect()
    };
    for l in &input.loops {
        match l {
            FlatLoop::Polygon { points, keys, arcs } => {
                let n = points.len();
                let mut covered = vec![false; n];
                for a in arcs {
                    for k in 0..a.edges {
                        covered[(a.from + k) % n] = true;
                    }
                }
                for i in 0..n {
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    if covered[i] || (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9 {
                        continue;
                    }
                    let name = next(FlatEdgeKind::Outline, keys.get(i).copied().unwrap_or(i as u64));
                    edges.insert(name, ModelEdge::Line { a: p3(a, 0.0), b: p3(b, 0.0) });
                    let (a2, b2) = (to2(a, 0.0), to2(b, 0.0));
                    hlr.edges.push(line_edge(name, a2, b2, ProjClass::Sharp));
                    data.outline.push([a2, b2]);
                }
                for a in arcs {
                    let name = next(FlatEdgeKind::Outline, keys.get(a.from).copied().unwrap_or(a.from as u64));
                    let pts = arc_points(a.center, a.radius, points[a.from], points[(a.from + 1) % n], points[(a.from + a.edges) % n]);
                    arc_edge(&mut hlr, &mut edges, Some(&mut data.outline), name, a.center, a.radius, pts, false);
                }
            }
            FlatLoop::Circle { center, radius, key } => {
                let name = next(FlatEdgeKind::Hole, *key);
                arc_edge(&mut hlr, &mut edges, Some(&mut data.outline), name, *center, *radius, ring(*center, *radius), true);
                data.centers.push((to2(*center, 0.0), *radius));
            }
        }
    }
    // Counterbores' and countersinks' outer diameters, and forms' outlines (SM16.3).
    for (center, radius, key) in &input.hole_marks {
        let name = next(FlatEdgeKind::HoleMark, *key);
        arc_edge(&mut hlr, &mut edges, None, name, *center, *radius, ring(*center, *radius), true);
        let c = to2(*center, 0.0);
        match data.centers.iter_mut().find(|(q, _)| (q[0] - c[0]).hypot(q[1] - c[1]) < 1e-6) {
            Some((_, r)) => *r = r.max(*radius),
            None => data.centers.push((c, *radius)),
        }
    }
    for f in &input.forms {
        for (pts, closed) in &f.lines {
            if *closed && let Some((c, r)) = circle_of(pts) {
                let name = next(FlatEdgeKind::Form, f.key);
                arc_edge(&mut hlr, &mut edges, None, name, c, r, ring(c, r), true);
                continue;
            }
            let n = pts.len();
            let segs = if *closed { n } else { n.saturating_sub(1) };
            for i in 0..segs {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9 {
                    continue;
                }
                let name = next(FlatEdgeKind::Form, f.key);
                edges.insert(name, ModelEdge::Line { a: p3(a, 0.0), b: p3(b, 0.0) });
                hlr.edges.push(line_edge(name, to2(a, 0.0), to2(b, 0.0), ProjClass::Sharp));
            }
        }
        data.centers.push((to2(f.center, 0.0), 0.0));
    }
    for (s, key) in &input.slits {
        data.outline.push([to2(s[0], 0.0), to2(s[1], 0.0)]);
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

fn seg_len(s: &[P2; 2]) -> f64 {
    (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1])
}

/// How far along the bend's (visible) line, over its pieces in order, the point of the line
/// nearest `p` (view 2D) is: a note's attach parameter ([`BendNotePlace::along`]).
pub fn line_param(data: &FlatData, bend: u32, p: P2) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    let mut run = 0.0;
    for s in data.lines_of(bend) {
        let q = nearest_on(*s, p);
        let d = (q[0] - p[0]).hypot(q[1] - p[1]);
        let t = run + (q[0] - s[0][0]).hypot(q[1] - s[0][1]);
        if best.is_none_or(|(bd, _)| d < bd - 1e-12) {
            best = Some((d, t));
        }
        run += seg_len(s);
    }
    best.map(|(_, t)| t)
}

/// The point at parameter `along` of the bend's line (clamped to it), and the piece it is on.
pub fn point_at(data: &FlatData, bend: u32, along: f64) -> Option<(P2, [P2; 2])> {
    let lines = data.lines_of(bend);
    let mut run = 0.0;
    for (i, s) in lines.iter().enumerate() {
        let l = seg_len(s);
        if along <= run + l || i + 1 == lines.len() {
            let t = if l < 1e-12 { 0.0 } else { ((along - run) / l).clamp(0.0, 1.0) };
            return Some(([s[0][0] + (s[1][0] - s[0][0]) * t, s[0][1] + (s[1][1] - s[0][1]) * t], *s));
        }
        run += l;
    }
    None
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

/// Where a note's leader ends, or where it sits on its line: its attach parameter, else (files
/// from before it) the point of the line nearest `at`.
fn attach_point(data: &FlatData, place: &BendNotePlace) -> Option<(P2, [P2; 2])> {
    match place.along {
        Some(a) => point_at(data, place.bend, a),
        None => nearest_line(data, place.bend, place.at),
    }
}

/// Where a bend note sits by default: the middle of the longest piece of its line (view 2D).
pub fn default_place(data: &FlatData, bend: u32) -> Option<BendNotePlace> {
    let s = data.lines_of(bend).iter().max_by(|a, b| seg_len(a).total_cmp(&seg_len(b)))?;
    let at = [(s[0][0] + s[1][0]) / 2.0, (s[0][1] + s[1][1]) / 2.0];
    Some(BendNotePlace { bend, at, attached: true, along: line_param(data, bend, at) })
}

/// How near its bend line (sheet mm) a dropped note goes back onto it.
pub const REATTACH: f64 = 4.0;

/// Where a note dropped with its node at sheet point `drop` goes: back on its bend line when
/// the drop is within [`REATTACH`] of it (attached where it was dropped along the line), else
/// off it there, its leader still ending where the note was attached (`current`, the note's
/// place before the drag; its default place if it had none).
pub fn place_note(view: &View, data: &FlatData, bend: u32, drop: P2, current: Option<BendNotePlace>) -> BendNotePlace {
    let at = view.from_sheet(drop);
    let near = nearest_line(data, bend, at).map(|(q, _)| {
        let s = view.to_sheet(q);
        (s[0] - drop[0]).hypot(s[1] - drop[1])
    });
    match near {
        Some(d) if d <= REATTACH => BendNotePlace { bend, at, attached: true, along: line_param(data, bend, at) },
        _ => {
            let before = current.or_else(|| default_place(data, bend));
            let along = before.and_then(|p| p.along.or_else(|| attach_point(data, &p).and_then(|(q, _)| line_param(data, bend, q))));
            BendNotePlace { bend, at, attached: false, along }
        }
    }
}

/// Whether two convex quadrilaterals (sheet mm) overlap (separating axes).
fn boxes_overlap(a: &[P2; 4], b: &[P2; 4]) -> bool {
    for poly in [a, b] {
        for i in 0..4 {
            let (p, q) = (poly[i], poly[(i + 1) % 4]);
            let n = [q[1] - p[1], p[0] - q[0]];
            let proj = |r: &[P2; 4]| {
                let v: Vec<f64> = r.iter().map(|x| x[0] * n[0] + x[1] * n[1]).collect();
                (v.iter().cloned().fold(f64::MAX, f64::min), v.iter().cloned().fold(f64::MIN, f64::max))
            };
            let ((a0, a1), (b0, b1)) = (proj(a), proj(b));
            if a1 <= b0 + 1e-9 || b1 <= a0 + 1e-9 {
                return false;
            }
        }
    }
    true
}

fn segs_cross(a: P2, b: P2, c: P2, d: P2) -> bool {
    let cr = |o: P2, p: P2, q: P2| (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0]);
    let (d1, d2, d3, d4) = (cr(c, d, a), cr(c, d, b), cr(a, b, c), cr(a, b, d));
    // Properly crossing (not just touching: a note's corner on a line's end is clear).
    let eps = 1e-9 * (1.0 + (b[0] - a[0]).hypot(b[1] - a[1]) * (d[0] - c[0]).hypot(d[1] - c[1]));
    d1 * d2 < -eps && d3 * d4 < -eps
}

/// Whether segment `s` (sheet mm) touches the inside of box `b`.
fn seg_hits_box(s: [P2; 2], b: &[P2; 4]) -> bool {
    let inside = |p: P2| {
        (0..4).all(|i| {
            let (a, c) = (b[i], b[(i + 1) % 4]);
            (c[0] - a[0]) * (p[1] - a[1]) - (c[1] - a[1]) * (p[0] - a[0]) > 1e-9
        })
    };
    inside(s[0]) || inside(s[1]) || (0..4).any(|i| segs_cross(s[0], s[1], b[i], b[(i + 1) % 4]))
}

/// A note along its line: (text's left end, rotation, box corners, node) for the attach point
/// `on` (sheet), the line's unit direction `u` (reading left to right or bottom to top), the side
/// `side` (+1 above / left of it, −1 below / right), text width `w` and height `h`.
fn along_line(on: P2, u: P2, side: f64, w: f64, h: f64) -> (P2, f64, [P2; 4], P2) {
    let n = [-u[1] * side, u[0] * side];
    let mid = [on[0] + n[0] * 0.9 * h, on[1] + n[1] * 0.9 * h];
    let left = [mid[0] - u[0] * w / 2.0, mid[1] - u[1] * w / 2.0];
    let rotation = u[1].atan2(u[0]).to_degrees();
    let pad = 0.35 * h;
    let corner = |s: f64, t: f64| [mid[0] + u[0] * s - u[1] * t, mid[1] + u[1] * s + u[0] * t];
    let (hw, hh) = (w / 2.0 + pad, h / 2.0 + pad);
    // Counter-clockwise whatever the side.
    (left, rotation, [corner(-hw, -hh), corner(hw, -hh), corner(hw, hh), corner(-hw, hh)], mid)
}

/// A note along its line: its text's left end, its turn (degrees), its box and its node.
type NotePlace = (P2, f64, [P2; 4], P2);

/// A bend line's reading direction on the sheet (left to right, or bottom to top) and its two
/// ends' parameters along it from `at`.
fn line_frame(view: &View, seg: [P2; 2], at: P2) -> (P2, f64, f64) {
    let (a, c) = (view.to_sheet(seg[0]), view.to_sheet(seg[1]));
    let mut u = [c[0] - a[0], c[1] - a[1]];
    let l = u[0].hypot(u[1]).max(1e-12);
    u = [u[0] / l, u[1] / l];
    if u[0] < -1e-9 || (u[0].abs() <= 1e-9 && u[1] < 0.0) {
        u = [-u[0], -u[1]];
    }
    let t = |p: P2| (p[0] - at[0]) * u[0] + (p[1] - at[1]) * u[1];
    let (ta, tc) = (t(a), t(c));
    (u, ta.min(tc), ta.max(tc))
}

/// How far along its line (from `at`) a note `w` wide may sit and keep within the line's ends:
/// `(lo, hi)`; a note longer than its line can only sit at the line's middle.
fn note_range(t0: f64, t1: f64, w: f64, h: f64) -> (f64, f64) {
    let half = w / 2.0 + 0.35 * h;
    let (lo, hi) = (t0 + half, t1 - half);
    if lo > hi {
        let m = (t0 + t1) / 2.0;
        (m, m)
    } else {
        (lo, hi)
    }
}

/// Where the notes nobody placed sit (`(bend, along_line(…))`): each above its line (its
/// reading side, as Onshape's), at the middle of its line's longest piece, slid along the line
/// (never past its ends) only as far as it takes to clear the notes before it, the outline and
/// the other bend lines; where nothing is clear, at the middle. The layout depends on the flat
/// alone, not on where other notes were dragged, so moving one note never moves another.
fn auto_notes(style: &DrawingStyle, view: &View, data: &FlatData) -> Vec<(u32, NotePlace)> {
    let h = style.dim_text_height;
    let outline: Vec<[P2; 2]> = data.outline.iter().map(|s| [view.to_sheet(s[0]), view.to_sheet(s[1])]).collect();
    let mut out: Vec<(u32, NotePlace)> = Vec::new();
    for b in &data.bends {
        let Some(place) = default_place(data, b.joint) else { continue };
        let Some((on, seg)) = attach_point(data, &place) else { continue };
        let w = text_width(&bend_note_text(style, b, data.shown_up(b))) * h;
        let at = view.to_sheet(on);
        let (u, t0, t1) = line_frame(view, seg, at);
        let (lo, hi) = note_range(t0, t1, w, h);
        let others: Vec<[P2; 2]> = data
            .lines
            .iter()
            .filter(|(j, _)| *j != b.joint)
            .flat_map(|(_, ls)| ls.iter().map(|s| [view.to_sheet(s[0]), view.to_sheet(s[1])]))
            .collect();
        let clear = |bx: &[P2; 4]| !out.iter().any(|(_, n)| boxes_overlap(bx, &n.2)) && !outline.iter().chain(&others).any(|s| seg_hits_box(*s, bx));
        let place_at = |k: f64| along_line([at[0] + u[0] * k, at[1] + u[1] * k], u, 1.0, w, h);
        let mid = (lo + hi) / 2.0;
        let mut best = place_at(mid);
        if !clear(&best.2) {
            let step = h / 2.0;
            let mut d = step;
            'search: while d <= hi - lo + step {
                for k in [mid - d, mid + d] {
                    if k < lo - 1e-9 || k > hi + 1e-9 {
                        continue;
                    }
                    let c = place_at(k);
                    if clear(&c.2) {
                        best = c;
                        break 'search;
                    }
                }
                d += step;
            }
        }
        out.push((b.joint, best));
    }
    out
}

/// The bend notes `view` shows (none when hidden, or seen edge on).
///
/// A note on its line sits along it, just above it, in the drawing's text height (one longer
/// than its line overhangs it, centred). Notes nobody placed are laid out by [`auto_notes`]; a
/// note put back on its line sits where it was dropped along it, within the line's ends.
pub fn bend_notes(style: &DrawingStyle, view: &View, data: &FlatData) -> Vec<NoteGraphics> {
    let Some(fs) = &view.flat else { return Vec::new() };
    if fs.bend_notes_hidden || !data.face_on {
        return Vec::new();
    }
    let h = style.dim_text_height;
    let arrow = style.dim_arrow_length;
    let auto = auto_notes(style, view, data);
    let mut out: Vec<NoteGraphics> = Vec::new();
    for b in &data.bends {
        let placed = fs.note(b.joint).copied();
        let text = bend_note_text(style, b, data.shown_up(b));
        let w = text_width(&text) * h;
        let Some(place) = placed else {
            if let Some((_, (left, rotation, corners, node))) = auto.iter().find(|(j, _)| *j == b.joint) {
                out.push(NoteGraphics { bend: b.joint, text: PlacedText { pos: *left, height: h, text }, rotation: *rotation, strokes: Vec::new(), fills: Vec::new(), corners: *corners, node: *node });
            }
            continue;
        };
        let Some((on, seg)) = attach_point(data, &place) else { continue };
        if place.attached {
            let at = view.to_sheet(on);
            let (u, t0, t1) = line_frame(view, seg, at);
            let (lo, hi) = note_range(t0, t1, w, h);
            let k = 0f64.clamp(lo, hi);
            let (left, rotation, corners, node) = along_line([at[0] + u[0] * k, at[1] + u[1] * k], u, 1.0, w, h);
            out.push(NoteGraphics { bend: b.joint, text: PlacedText { pos: left, height: h, text }, rotation, strokes: Vec::new(), fills: Vec::new(), corners, node });
        } else {
            // Off the line: level text with a landing and a leader to its attach point.
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

/// The centermarks of a flat view (SM16.3): a cross at each round hole, counterbore,
/// countersink and form centre, `style.centermark_size` across, turned with the view; at a
/// circle the cross's arms carry on as dashes past its rim (`16-drawings/t0026.7`), as part
/// views' centermarks do. Sheet mm polylines; none seen edge on.
pub fn centermarks(style: &DrawingStyle, view: &View, data: &FlatData) -> Vec<Vec<P2>> {
    if view.flat.is_none() || !data.face_on {
        return Vec::new();
    }
    let half = style.centermark_size / 2.0;
    let k = view.scale.factor();
    let (ax, ay) = (crate::view::rotate([1.0, 0.0], view.rotation), crate::view::rotate([0.0, 1.0], view.rotation));
    let mut out = Vec::new();
    for (c, r) in &data.centers {
        let c = view.to_sheet(*c);
        let at = |d: P2, t: f64| [c[0] + d[0] * t, c[1] + d[1] * t];
        for d in [ax, ay] {
            out.push(vec![at(d, -half), at(d, half)]);
            let (from, to) = (half + 1.2, r * k + style.centerline_extension);
            if *r > 0.0 && to > from + 0.5 {
                for sgn in [1.0, -1.0] {
                    out.push(vec![at(d, sgn * from), at(d, sgn * to)]);
                }
            }
        }
    }
    out
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
                    arcs: Vec::new(),
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
            ..Default::default()
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
        let p = place_note(&v, &data, 3, drop, None);
        assert!(!p.attached);
        v.flat.as_mut().unwrap().set_note(p);
        let n = &bend_notes(&style, &v, &data)[0];
        assert_eq!(n.rotation, 0.0);
        assert_eq!(n.node, drop);
        let tip = n.strokes[0][0];
        assert!((tip[0] - 120.0).abs() < 1e-9, "{tip:?}");
        // Dropped near the line again: back on it, where it was dropped along it.
        let p = place_note(&v, &data, 3, [121.5, 105.0], v.flat.as_ref().unwrap().note(3).copied());
        assert!(p.attached);
        v.flat.as_mut().unwrap().set_note(p);
        let n = &bend_notes(&style, &v, &data)[0];
        // There, as far along it as the note fits within the line's ends (it fills this short
        // line: its middle).
        assert!((n.rotation - 90.0).abs() < 1e-9 && (100.0..=125.0).contains(&n.node[1]), "{:?}", n.node);
        // Dragged off again, twice: the leader stays on the point it was attached at (y = 105
        // on the sheet), wherever the note goes, until it is reattached.
        for drop in [[170.0, 150.0], [60.0, 80.0]] {
            let cur = v.flat.as_ref().unwrap().note(3).copied();
            let p = place_note(&v, &data, 3, drop, cur);
            assert!(!p.attached && (p.along.unwrap() - 2.5 * 4.0).abs() < 1e-9, "{p:?}");
            v.flat.as_mut().unwrap().set_note(p);
            let n = &bend_notes(&style, &v, &data)[0];
            let tip = n.strokes[0][0];
            assert!((tip[0] - 120.0).abs() < 1e-9 && (tip[1] - 105.0).abs() < 1e-9, "{tip:?}");
        }
        // A note saved before the attach parameter: nearest the place.
        let old = BendNotePlace { bend: 3, at: [10.0, 40.0], attached: false, along: None };
        let back: BendNotePlace = ron::from_str("(bend: 3, at: (10.0, 40.0), attached: false)").unwrap();
        assert_eq!(back, old);
        v.flat.as_mut().unwrap().set_note(old);
        let tip = bend_notes(&style, &v, &data)[0].strokes[0][0];
        assert!((tip[0] - 120.0).abs() < 1e-9 && (tip[1] - 120.0).abs() < 1e-9, "{tip:?}");
        // Hidden.
        v.flat.as_mut().unwrap().bend_notes_hidden = true;
        assert!(bend_notes(&style, &v, &data).is_empty());
    }

    #[test]
    fn notes_keep_their_side_and_size_and_dragging_one_moves_no_other() {
        // An open box's flat: a 200 × 125 base with a bend on each side and 150 walls.
        let (w, d, hgt) = (200.0, 125.0, 150.0);
        let base: Vec<[P2; 2]> = vec![[[0.0, 0.0], [w, 0.0]], [[w, 0.0], [w, d]], [[w, d], [0.0, d]], [[0.0, d], [0.0, 0.0]]];
        let bend = |j: u32, s: [P2; 2]| FlatBendInfo { joint: j, name: format!("Bend {j}"), up: false, angle_deg: 90.0, radius: 1.5, center: s, center_visible: vec![s], tangent_visible: Vec::new() };
        let outline = vec![
            [-hgt, 0.0], [0.0, 0.0], [0.0, -hgt], [w, -hgt], [w, 0.0], [w + hgt, 0.0], [w + hgt, d], [w, d], [w, d + hgt], [0.0, d + hgt], [0.0, d], [-hgt, d],
        ];
        let input = FlatInput {
            loops: vec![FlatLoop::Polygon { keys: (0..outline.len() as u64).collect(), points: outline, arcs: Vec::new() }],
            bends: base.iter().enumerate().map(|(i, s)| bend(i as u32 + 1, *s)).collect(),
            thickness: 1.5,
            ..Default::default()
        };
        let (_, _, data) = flat_projection(uuid::Uuid::nil(), &input, &NamedView::Top.frame());
        let style = DrawingStyle::default();
        for scale in [Scale::new(1, 2), Scale::new(1, 5)] {
            let mut v = view();
            v.scale = scale;
            let notes = bend_notes(&style, &v, &data);
            assert_eq!(notes.len(), 4);
            for n in &notes {
                // One text height (Onshape's), above its line (its reading side), the note's
                // middle on the line's span.
                assert_eq!(n.text.height, style.dim_text_height);
                let s = data.lines_of(n.bend)[0];
                let (p, q) = (v.to_sheet(s[0]), v.to_sheet(s[1]));
                let len = seg_len(&[p, q]);
                let (u, _, _) = line_frame(&v, s, p);
                let side = (n.node[0] - p[0]) * -u[1] + (n.node[1] - p[1]) * u[0];
                assert!((side - 0.9 * n.text.height).abs() < 1e-9, "note {} on the wrong side: {side}", n.bend);
                let t = ((n.node[0] - p[0]) * (q[0] - p[0]) + (n.node[1] - p[1]) * (q[1] - p[1])) / len;
                assert!((-1e-9..=len + 1e-9).contains(&t));
            }
            // At 1:2 the lines are long enough: the notes clear each other.
            if scale == Scale::new(1, 2) {
                for (i, a) in notes.iter().enumerate() {
                    for b in &notes[i + 1..] {
                        assert!(!boxes_overlap(&a.corners, &b.corners), "{} and {} overlap", a.bend, b.bend);
                    }
                }
            }
            // Dragging one note off its line leaves every other where it was.
            let mut moved = v.clone();
            moved.flat.as_mut().unwrap().set_note(place_note(&v, &data, 2, [300.0, 300.0], None));
            let after = bend_notes(&style, &moved, &data);
            for (a, b) in notes.iter().zip(&after) {
                if a.bend != 2 {
                    assert_eq!(a, b);
                }
            }
        }
    }

    #[test]
    fn arcs_forms_and_counterbores_on_the_flat() {
        // A 60 × 40 plate whose top-right corner is rounded R8 (a corner break's polyline, 7.5°
        // steps), a form (a 10 mm round dimple and a louver's slot) and a counterbored hole.
        let mut pts = vec![[0.0, 0.0], [60.0, 0.0]];
        for i in 0..=12 {
            let t = (7.5 * i as f64).to_radians();
            pts.push([52.0 + 8.0 * t.cos(), 32.0 + 8.0 * t.sin()]);
        }
        pts.push([0.0, 40.0]);
        let pts = simplify_loop(&pts, 1e-6);
        let arcs = find_arcs(&pts);
        assert_eq!(arcs.len(), 1, "{arcs:?}");
        assert!((arcs[0].radius - 8.0).abs() < 1e-9 && (arcs[0].center[0] - 52.0).abs() < 1e-9 && arcs[0].edges == 12);
        // A rectangle, a chamfered corner and an octagon have none.
        assert!(find_arcs(&[[0.0, 0.0], [10.0, 0.0], [10.0, 8.0], [8.0, 10.0], [0.0, 10.0]]).is_empty());
        let oct: Vec<P2> = (0..8).map(|i| [(i as f64 * std::f64::consts::FRAC_PI_4).cos(), (i as f64 * std::f64::consts::FRAC_PI_4).sin()]).collect();
        assert!(find_arcs(&oct).is_empty());
        let circle = |c: P2, r: f64| -> Vec<P2> { (0..32).map(|i| [c[0] + r * (i as f64 / 32.0 * std::f64::consts::TAU).cos(), c[1] + r * (i as f64 / 32.0 * std::f64::consts::TAU).sin()]).collect() };
        let input = FlatInput {
            loops: vec![
                FlatLoop::Polygon { keys: (0..pts.len() as u64).collect(), points: pts.clone(), arcs: arcs.clone() },
                FlatLoop::Circle { center: [15.0, 20.0], radius: 2.5, key: 40 },
            ],
            forms: vec![FlatFormInfo { lines: vec![(circle([40.0, 15.0], 5.0), true), (vec![[30.0, 30.0], [40.0, 30.0]], false)], center: [40.0, 15.0], key: 77 }],
            hole_marks: vec![([15.0, 20.0], 4.5, 41)],
            thickness: 1.0,
            ..Default::default()
        };
        let (hlr, edges, data) = flat_projection(uuid::Uuid::nil(), &input, &NamedView::Top.frame());
        let kind = |e: &ProjEdge| kind_of(e.source.as_ref().unwrap().edge_name.as_ref().unwrap());
        // The round is one arc edge, R8, its ends where the straight edges end.
        let rounds: Vec<&ProjEdge> = hlr.edges.iter().filter(|e| kind(e) == Some(FlatEdgeKind::Outline) && matches!(e.curve, ProjCurve::Arc { .. })).collect();
        assert_eq!(rounds.len(), 1);
        let ProjCurve::Arc { radius, start, end, full, .. } = rounds[0].curve else { unreachable!() };
        assert!(!full && (radius - 8.0).abs() < 1e-9);
        assert!((start - Point2::new(60.0, 32.0)).norm() < 1e-9 && (end - Point2::new(52.0, 40.0)).norm() < 1e-9, "{start} {end}");
        // 4 straight outline edges and the arc.
        assert_eq!(hlr.edges.iter().filter(|e| kind(e) == Some(FlatEdgeKind::Outline)).count(), 5);
        assert_eq!(hlr.edges.iter().filter(|e| kind(e) == Some(FlatEdgeKind::Form)).count(), 2);
        let mark = hlr.edges.iter().find(|e| kind(e) == Some(FlatEdgeKind::HoleMark)).unwrap();
        assert!(matches!(mark.curve, ProjCurve::Arc { radius, full: true, .. } if (radius - 4.5).abs() < 1e-9));
        assert_eq!(edges.len(), hlr.edges.len());
        // Centermarks: the hole (and its counterbore, one mark) and the form.
        assert_eq!(data.centers.len(), 2);
        let v = view();
        let marks = centermarks(&DrawingStyle::default(), &v, &data);
        // The form's cross; the hole's cross and its arms out past the Ø9 counterbore's rim.
        assert_eq!(marks.len(), 8);
        let c = v.to_sheet([15.0, 20.0]);
        let reach = marks.iter().flat_map(|m| m.iter()).map(|p| (p[0] - c[0]).hypot(p[1] - c[1])).filter(|d| *d < 20.0).fold(0.0, f64::max);
        assert!(reach > 4.5 * v.scale.factor(), "{reach}");
        assert!(marks.iter().any(|m| (m[0][1] - c[1]).abs() < 1e-9 && (m[0][0] + m[1][0]) / 2.0 - c[0] < 1e-9));
        // The DXF has the round as an ARC and the hole and mark as CIRCLEs.
        let mut d = crate::Drawing::from_template(&crate::template::builtin("ANSI_A_MM.dwt").unwrap(), None);
        let id = v.id;
        d.sheets[0].views.push(v);
        let m = FlatModel(hlr, edges, data);
        let r = crate::ReferenceProps::default();
        let f = crate::rich::DrawingContext::default();
        let mut views = HashMap::new();
        views.insert(id, crate::export::ViewInput { model: &m, shaded: Vec::new(), sketches: Vec::new() });
        let page = crate::export::sheet_page(&d, 0, &crate::export::PageContext { reference: &r, fields: &f, views });
        let dxf = crate::dxf::write_dxf(&page);
        let back = crate::dxf::read_dxf(&dxf).unwrap();
        let (_, arcs, circles) = crate::dxf::counts(&back);
        assert!(arcs >= 1 && circles >= 3, "{arcs} arcs, {circles} circles");
    }

    #[test]
    fn flat_views_start_with_their_tangent_lines_hidden() {
        let (hlr, _, _) = flat_projection(uuid::Uuid::nil(), &rect_input(), &NamedView::Top.frame());
        // A new flat view hides tangent edges: the bend lines' chain lines stay clean
        // (`ex3-drawings/goal.png`).
        let mut v = View::flat_pattern(view().reference, NamedView::Top, Scale::new(1, 2), [100.0, 100.0]);
        assert_eq!(v.tangent_edges, crate::style::TangentEdges::Hidden);
        let tangents = |v: &View| crate::view::view_lines(v, &hlr).iter().filter(|l| l.kind == crate::view::LineKind::Tangent).count();
        assert_eq!(tangents(&v), 0);
        // Tangent edges → Solid shows them.
        v.tangent_edges = crate::style::TangentEdges::Solid;
        assert_eq!(tangents(&v), 2);
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

    struct FlatModel(Hlr, HashMap<EdgeName, ModelEdge>, FlatData);

    impl crate::annotation::ViewModel for FlatModel {
        fn projection(&self) -> &Hlr {
            &self.0
        }
        fn model_edge(&self, name: &EdgeName) -> Option<&ModelEdge> {
            self.1.get(name)
        }
        fn hole(&self, _: &uuid::Uuid) -> Option<&crate::annotation::HoleInfo> {
            None
        }
        fn flat(&self) -> Option<&FlatData> {
            Some(&self.2)
        }
    }

    #[test]
    fn the_dxf_export_has_the_bend_lines_and_notes() {
        use crate::export::{Item, Layer, PageContext, ViewInput, sheet_page};
        let mut d = crate::Drawing::from_template(&crate::template::builtin("ANSI_A_MM.dwt").unwrap(), None);
        let mut v = view();
        v.flat.as_mut().unwrap().up.color = [0xd0, 0x30, 0x20];
        let id = v.id;
        d.sheets[0].views.push(v);
        let (hlr, edges, data) = flat_projection(uuid::Uuid::nil(), &rect_input(), &NamedView::Top.frame());
        let m = FlatModel(hlr, edges, data);
        let r = crate::ReferenceProps::default();
        let f = crate::rich::DrawingContext::default();
        let mut views = HashMap::new();
        views.insert(id, ViewInput { model: &m, shaded: Vec::new(), sketches: Vec::new() });
        let page = sheet_page(&d, 0, &PageContext { reference: &r, fields: &f, views });
        let bends: Vec<&Item> = page.items.iter().filter(|i| matches!(i, Item::Stroke(_, p) if p.layer == Layer::BendUp)).collect();
        assert_eq!(bends.len(), 1);
        assert!(page.strings().contains(&"UP 90.0° R1.5"), "{:?}", page.strings());
        let dxf = crate::dxf::write_dxf(&page);
        assert!(dxf.contains("BEND_UP"));
        assert!(dxf.contains("CENTER"));
        // The bend line from (120, 100) to (120, 125) on the sheet, in its red.
        let at = dxf.find("\n  8\nBEND_UP\n").expect("an entity on BEND_UP");
        let entity = &dxf[at..at + 400];
        assert!(entity.contains("420\n13643808"), "{entity}");
        assert!(entity.contains("120"), "{entity}");
    }
}
