//! Drawing annotations (P3C.3, D5, D6, X8): centerlines, centermarks, virtual sharps, driven
//! dimensions and hole callouts, each belonging to one view and attached to the model's topology.
//!
//! # References
//! An annotation refers to geometry through an [`EdgeRef`]: the **persistent name** of the model
//! edge a projected line came from (`cadrs_kernel::ProjSource::edge_name`), or the name of the
//! face whose outline it is (a cylinder's silhouette, [`ProjSource::face_name`]), plus the 2D
//! shape it had when it was picked (to tell the two outlines of a cylinder apart, and as the
//! last known place of a reference that no longer resolves; P3C.6 draws those red). A point
//! (an end, a midpoint, a circle's centre) is a [`PointRef`]: an edge and which of its points.
//!
//! # Measurement (D6.1)
//! Dimensions are driven: they read the model. [`resolve`] finds a reference's geometry in this
//! order:
//! 1. **Model topology**: the named edge's exact 3D geometry ([`ModelEdge`]: a line's end
//!    vertices, a circle's centre, normal and radius from the B-rep), projected onto the view
//!    plane. Drawing views are orthographic, so a projected length is the model length of its
//!    component parallel to the view plane: a dimension of a feature parallel to the view plane
//!    reads its true size, whatever the view's scale.
//! 2. **The projected 2D geometry** (the kernel's hidden-line removal, model mm in the view's
//!    frame, i.e. the sheet geometry ÷ the view's scale): for silhouettes, which have no model
//!    edge (a hole's outline seen from the side), and for edges whose name no longer resolves.
//! 3. The shape stored when the reference was made (a dangling reference).
//!
//! [`Resolved::from_model`] says which one a measurement used; the unit tests check the Ex1
//! values against the model's parameters to 1e−6 in.
//!
//! # Placement
//! Annotation text positions are stored in the view's 2D frame (model mm), so they move, turn
//! and scale with their view. Everything is laid out on the sheet in paper millimetres with the
//! drawing properties' sizes (text height, arrow length, extension-line gap and overshoot,
//! centermark size, centerline extension), see [`annotation_graphics`].
//!
//! # Text
//! Values follow the drawing properties' units and precision ([`crate::DrawingStyle`]), with a
//! per-dimension override ([`DimFormat`]: prefix and suffix, tolerance, precision, dual units;
//! D2.6, D6.6). Hole callouts are generated from the Hole feature's spec ([`HoleInfo`], PS15.9).
//! The counterbore ⌴, countersink ⌵ and depth ↧ symbols are not in Inter, so text is split into
//! [`Run`]s and those symbols are drawn as vector strokes ([`symbol_strokes`]); Ø, ± and ° are
//! Inter glyphs.

use std::collections::HashMap;

use cadrs_kernel::naming::{EdgeName, FaceName};
use cadrs_kernel::{ProjCurve, ProjEdge, Projection as Hlr};
use nalgebra::Point3;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::style::{DrawingStyle, VirtualSharp as SharpStyle};
use crate::view::{View, dashes, rotate};

/// Identifies an annotation within its view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AnnotationId(pub Uuid);

impl AnnotationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for AnnotationId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------
// The model side

/// A model edge's exact geometry (model mm), from the part's B-rep.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelEdge {
    /// A straight edge between its two vertices.
    Line { a: [f64; 3], b: [f64; 3] },
    /// A circle or circular arc; `points` runs along it (closed for a whole circle).
    Circle {
        center: [f64; 3],
        normal: [f64; 3],
        radius: f64,
        points: Vec<[f64; 3]>,
    },
    /// Anything else, as points on it.
    Curve { points: Vec<[f64; 3]> },
}

/// How a hole ends (from the Hole feature).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoleEnd {
    Through,
    Blind,
    UpToNext,
    /// Up to a picked face or plane (P3.10).
    UpToEntity,
}

/// A Hole feature's spec, as a callout shows it (lengths in mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleInfo {
    pub diameter: f64,
    pub end: HoleEnd,
    pub depth: f64,
    /// Counterbore diameter and depth.
    pub cbore: Option<(f64, f64)>,
    /// Countersink diameter and angle (degrees).
    pub csink: Option<(f64, f64)>,
    /// A tapped hole's thread ("1/4-20 UNC") and thread depth.
    pub thread: Option<(String, f64)>,
}

/// A tapped hole's thread as views draw it (P3C.8, Show threads): lengths in mm, model
/// coordinates; `axis` is a unit vector from the entry into the material.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub center: [f64; 3],
    pub axis: [f64; 3],
    /// Major (nominal) and minor (tap drill) diameters.
    pub major: f64,
    pub minor: f64,
    /// Thread length from the entry.
    pub length: f64,
    /// The hole goes through the part.
    pub through: bool,
}

/// A Chamfer feature's spec as a chamfer dimension reads it (P3C.8, D6.4): lengths in mm, the
/// angle in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChamferInfo {
    pub distance: f64,
    /// The second distance of a two-distance chamfer.
    pub distance2: Option<f64>,
    pub angle: f64,
}

/// What an annotation needs from a view's model: the projection, the named edges' exact
/// geometry and the hole features. `cadrs_core`'s view geometry implements it.
pub trait ViewModel {
    fn projection(&self) -> &Hlr;
    fn model_edge(&self, name: &EdgeName) -> Option<&ModelEdge>;
    /// The Hole feature with this id (a face name's `op`).
    fn hole(&self, feature: &Uuid) -> Option<&HoleInfo>;
    /// The cut faces of a section or broken-out view (P3C.8): closed loops in the view's 2D
    /// frame, outer loops counter-clockwise and holes clockwise.
    fn hatch(&self) -> &[Vec<[f64; 2]>] {
        &[]
    }
    /// The tapped holes' threads (P3C.8).
    fn threads(&self) -> &[ThreadInfo] {
        &[]
    }
    /// The Chamfer feature with this id (P3C.8).
    fn chamfer(&self, _feature: &Uuid) -> Option<&ChamferInfo> {
        None
    }
    /// An assembly view's occurrence (P3C.5: what callouts read).
    fn occurrence(&self, _id: &Uuid) -> Option<&crate::assembly::OccurrenceInfo> {
        None
    }
    /// The view shows an assembly (its callouts dangle when their occurrence is gone).
    fn is_assembly(&self) -> bool {
        false
    }
    /// The BOM tables on the view's sheet (callouts' `Table:` fields).
    fn boms(&self) -> Vec<&crate::assembly::BomData> {
        Vec::new()
    }
}

/// A plain [`ViewModel`] (tests, and what `cadrs_core` stores).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelData {
    pub projection: Hlr,
    pub edges: HashMap<EdgeName, ModelEdge>,
    pub holes: HashMap<Uuid, HoleInfo>,
    pub hatch: Vec<Vec<[f64; 2]>>,
    pub threads: Vec<ThreadInfo>,
    pub chamfers: HashMap<Uuid, ChamferInfo>,
}

impl ViewModel for ModelData {
    fn projection(&self) -> &Hlr {
        &self.projection
    }
    fn model_edge(&self, name: &EdgeName) -> Option<&ModelEdge> {
        self.edges.get(name)
    }
    fn hole(&self, feature: &Uuid) -> Option<&HoleInfo> {
        self.holes.get(feature)
    }
    fn hatch(&self) -> &[Vec<[f64; 2]>] {
        &self.hatch
    }
    fn threads(&self) -> &[ThreadInfo] {
        &self.threads
    }
    fn chamfer(&self, feature: &Uuid) -> Option<&ChamferInfo> {
        self.chamfers.get(feature)
    }
}

// ---------------------------------------------------------------------------------------------
// 2D geometry

type P2 = [f64; 2];

fn add(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: P2, k: f64) -> P2 {
    [a[0] * k, a[1] * k]
}
fn dot(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn len(a: P2) -> f64 {
    a[0].hypot(a[1])
}
fn dist(a: P2, b: P2) -> f64 {
    len(sub(a, b))
}
fn unit(a: P2) -> P2 {
    let l = len(a);
    if l < 1e-300 { [1.0, 0.0] } else { mul(a, 1.0 / l) }
}
fn perp(a: P2) -> P2 {
    [-a[1], a[0]]
}
fn lerp(a: P2, b: P2, t: f64) -> P2 {
    add(a, mul(sub(b, a), t))
}

/// A projected edge's 2D shape (view frame, model mm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// An edge seen end-on.
    Point(P2),
    Line { a: P2, b: P2 },
    /// A circle, or an arc from `arc[0]` through `arc[1]` to `arc[2]`.
    Circle { center: P2, radius: f64, arc: Option<[P2; 3]> },
    /// Anything else (an ellipse, a spline): its ends.
    Curve { a: P2, b: P2 },
}

impl Shape {
    /// The points the dimension and centerline tools snap to: ends and middle of a line, the
    /// centre and ends of an arc, the centre of a circle.
    pub fn snap_points(&self) -> Vec<(PointOf, P2)> {
        match *self {
            Shape::Point(p) => vec![(PointOf::End, p)],
            Shape::Line { a, b } => vec![(PointOf::End, a), (PointOf::End, b), (PointOf::Mid, lerp(a, b, 0.5))],
            Shape::Circle { center, arc: None, .. } => vec![(PointOf::Center, center)],
            Shape::Circle { center, arc: Some(a), .. } => {
                vec![(PointOf::Center, center), (PointOf::End, a[0]), (PointOf::End, a[2]), (PointOf::Mid, a[1])]
            }
            Shape::Curve { a, b } => vec![(PointOf::End, a), (PointOf::End, b)],
        }
    }

    /// Distance from `p` to the shape.
    pub fn distance(&self, p: P2) -> f64 {
        match *self {
            Shape::Point(q) => dist(p, q),
            Shape::Line { a, b } | Shape::Curve { a, b } => seg_distance(p, a, b),
            Shape::Circle { center, radius, arc } => {
                let d = (dist(p, center) - radius).abs();
                match arc {
                    None => d,
                    Some(a) => {
                        if on_arc(center, a, sub(p, center)) {
                            d
                        } else {
                            dist(p, a[0]).min(dist(p, a[2]))
                        }
                    }
                }
            }
        }
    }

    /// The point of the shape nearest `p`.
    pub fn nearest(&self, p: P2) -> P2 {
        match *self {
            Shape::Point(q) => q,
            Shape::Line { a, b } | Shape::Curve { a, b } => {
                let d = sub(b, a);
                let l2 = dot(d, d);
                if l2 < 1e-300 {
                    return a;
                }
                add(a, mul(d, (dot(sub(p, a), d) / l2).clamp(0.0, 1.0)))
            }
            Shape::Circle { center, radius, arc } => {
                let on = add(center, mul(unit(sub(p, center)), radius));
                match arc {
                    Some(a) if !on_arc(center, a, sub(p, center)) => {
                        if dist(p, a[0]) < dist(p, a[2]) { a[0] } else { a[2] }
                    }
                    _ => on,
                }
            }
        }
    }

    /// A representative point (to tell pieces apart).
    fn mid(&self) -> P2 {
        match *self {
            Shape::Point(p) => p,
            Shape::Line { a, b } | Shape::Curve { a, b } => lerp(a, b, 0.5),
            Shape::Circle { center, arc: None, radius } => add(center, [radius, 0.0]),
            Shape::Circle { arc: Some(a), .. } => a[1],
        }
    }

    /// The shape as a polyline (for highlighting).
    pub fn polyline(&self) -> Vec<P2> {
        match *self {
            Shape::Point(p) => vec![p, p],
            Shape::Line { a, b } | Shape::Curve { a, b } => vec![a, b],
            Shape::Circle { center, radius, arc } => {
                let (a0, sweep) = match arc {
                    None => (0.0, std::f64::consts::TAU),
                    Some(a) => arc_angles(center, a),
                };
                let n = 64;
                (0..=n)
                    .map(|i| {
                        let t = a0 + sweep * i as f64 / n as f64;
                        add(center, [radius * t.cos(), radius * t.sin()])
                    })
                    .collect()
            }
        }
    }

    /// A line's ends (a circle seen edge-on is a line too).
    pub fn line(&self) -> Option<(P2, P2)> {
        match *self {
            Shape::Line { a, b } => Some((a, b)),
            _ => None,
        }
    }

    pub fn circle(&self) -> Option<(P2, f64)> {
        match *self {
            Shape::Circle { center, radius, .. } => Some((center, radius)),
            _ => None,
        }
    }
}

fn seg_distance(p: P2, a: P2, b: P2) -> f64 {
    let d = sub(b, a);
    let l2 = dot(d, d);
    if l2 < 1e-300 {
        return dist(p, a);
    }
    let t = (dot(sub(p, a), d) / l2).clamp(0.0, 1.0);
    dist(p, add(a, mul(d, t)))
}

/// The start angle and the signed sweep of an arc through three points about `center`.
fn arc_angles(center: P2, a: [P2; 3]) -> (f64, f64) {
    use std::f64::consts::TAU;
    let ang = |p: P2| (p[1] - center[1]).atan2(p[0] - center[0]);
    let (s, m, e) = (ang(a[0]), ang(a[1]), ang(a[2]));
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(TAU);
    let sweep = ccw(s, e);
    if ccw(s, m) <= sweep { (s, sweep) } else { (s, sweep - TAU) }
}

fn on_arc(center: P2, a: [P2; 3], d: P2) -> bool {
    use std::f64::consts::TAU;
    let (s, sweep) = arc_angles(center, a);
    let t = d[1].atan2(d[0]);
    if sweep >= 0.0 { (t - s).rem_euclid(TAU) <= sweep + 1e-9 } else { (s - t).rem_euclid(TAU) <= -sweep + 1e-9 }
}

/// The 2D shape of a projected (HLR) edge.
pub fn proj_shape(e: &ProjEdge) -> Shape {
    let p = |q: &nalgebra::Point2<f64>| [q.x, q.y];
    match &e.curve {
        ProjCurve::Line { start, end } => Shape::Line { a: p(start), b: p(end) },
        ProjCurve::Arc { center, radius, start, mid, end, full } => Shape::Circle {
            center: p(center),
            radius: *radius,
            arc: (!*full).then(|| [p(start), p(mid), p(end)]),
        },
        ProjCurve::Polyline => {
            let pts: Vec<P2> = e.points.iter().map(p).collect();
            polyline_shape(&pts)
        }
    }
}

/// Straight polylines (a curve seen edge-on) become lines between their extreme points.
fn polyline_shape(pts: &[P2]) -> Shape {
    let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else {
        return Shape::Point([0.0, 0.0]);
    };
    // The two points farthest apart along the polyline's main direction.
    let far = pts.iter().copied().max_by(|p, q| dist(*p, a).total_cmp(&dist(*q, a))).unwrap_or(b);
    let l = dist(a, far);
    if l < 1e-9 {
        return Shape::Point(a);
    }
    let d = unit(sub(far, a));
    let n = perp(d);
    let straight = pts.iter().all(|p| dot(sub(*p, a), n).abs() <= 1e-6 * l.max(1.0));
    if straight {
        let ts: Vec<f64> = pts.iter().map(|p| dot(sub(*p, a), d)).collect();
        let lo = ts.iter().copied().fold(f64::MAX, f64::min);
        let hi = ts.iter().copied().fold(f64::MIN, f64::max);
        Shape::Line { a: add(a, mul(d, lo)), b: add(a, mul(d, hi)) }
    } else {
        Shape::Curve { a, b }
    }
}

/// A model edge seen in `view`: a line (or a point, end-on), a circle or arc seen face-on, a
/// circle seen edge-on as a line, or another curve.
pub fn model_shape(view: &View, e: &ModelEdge) -> Shape {
    let f = view.frame.view_frame();
    let to2 = |p: &[f64; 3]| {
        let q = f.to_2d(&Point3::new(p[0], p[1], p[2]));
        [q.x, q.y]
    };
    match e {
        ModelEdge::Line { a, b } => {
            let (a, b) = (to2(a), to2(b));
            if dist(a, b) < 1e-9 { Shape::Point(a) } else { Shape::Line { a, b } }
        }
        ModelEdge::Circle { center, normal, radius, points } => {
            let n = nalgebra::Vector3::new(normal[0], normal[1], normal[2]);
            let c = n.normalize().dot(&f.dir).abs();
            if c > 1.0 - 1e-9 {
                let closed = match (points.first(), points.last()) {
                    (Some(a), Some(b)) => points.len() < 3 || (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6),
                    _ => true,
                };
                let arc = (!closed).then(|| [to2(&points[0]), to2(&points[points.len() / 2]), to2(&points[points.len() - 1])]);
                Shape::Circle { center: to2(center), radius: *radius, arc }
            } else {
                let pts: Vec<P2> = points.iter().map(to2).collect();
                polyline_shape(&pts)
            }
        }
        ModelEdge::Curve { points } => {
            let pts: Vec<P2> = points.iter().map(to2).collect();
            polyline_shape(&pts)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// References

/// A reference to a model edge through a view (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    /// The model edge's persistent name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<EdgeName>,
    /// The face whose outline it is (silhouettes have no model edge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceName>,
    /// The 2D shape when it was picked.
    pub shape: Shape,
}

impl EdgeRef {
    /// The reference to projected edge `e` of a view.
    pub fn of(e: &ProjEdge) -> Self {
        let src = e.source.as_ref();
        Self {
            edge: src.and_then(|s| s.edge_name),
            face: if src.and_then(|s| s.edge_name).is_none() { src.and_then(|s| s.face_name) } else { None },
            shape: proj_shape(e),
        }
    }
}

/// Which point of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointOf {
    /// A circle's or arc's centre.
    Center,
    /// A line's midpoint (an arc's middle).
    Mid,
    /// The end nearest the hint.
    End,
}

/// A point of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointRef {
    pub edge: EdgeRef,
    pub of: PointOf,
    /// Where it was when picked (view 2D): picks the end.
    pub hint: P2,
}

/// What a tool picked: a point, or a whole edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Pick {
    Point(PointRef),
    Edge(EdgeRef),
}

impl Pick {
    pub fn edge(&self) -> &EdgeRef {
        match self {
            Pick::Point(p) => &p.edge,
            Pick::Edge(e) => e,
        }
    }
}

/// A reference's current geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolved {
    pub shape: Shape,
    /// From the model edge's exact geometry (else the 2D projection or the stored shape).
    pub from_model: bool,
    /// Found at all (else the stored shape: dangling).
    pub found: bool,
}

/// The current geometry of a reference in `view` (see the module docs for the order).
pub fn resolve(view: &View, m: &dyn ViewModel, r: &EdgeRef) -> Resolved {
    if let Some(name) = &r.edge
        && let Some(e) = m.model_edge(name)
    {
        return Resolved { shape: model_shape(view, e), from_model: true, found: true };
    }
    let best = m
        .projection()
        .edges
        .iter()
        .filter(|e| {
            let s = e.source.as_ref();
            match (&r.edge, &r.face) {
                (Some(n), _) => s.and_then(|s| s.edge_name.as_ref()) == Some(n),
                (None, Some(f)) => s.is_some_and(|s| s.edge_name.is_none() && s.face_name.as_ref() == Some(f)),
                (None, None) => false,
            }
        })
        .map(proj_shape)
        .min_by(|a, b| dist(a.mid(), r.shape.mid()).total_cmp(&dist(b.mid(), r.shape.mid())));
    match best {
        Some(shape) => Resolved { shape, from_model: false, found: true },
        None => Resolved { shape: r.shape, from_model: false, found: false },
    }
}

/// A point reference's current position (view 2D).
pub fn resolve_point(view: &View, m: &dyn ViewModel, p: &PointRef) -> Option<(P2, bool)> {
    let r = resolve(view, m, &p.edge);
    let at = match (p.of, r.shape) {
        (PointOf::Center, Shape::Circle { center, .. }) => center,
        (PointOf::Mid, Shape::Line { a, b }) => lerp(a, b, 0.5),
        (PointOf::Mid, Shape::Circle { arc: Some(a), .. }) => a[1],
        (PointOf::End, s) => {
            let pts: Vec<P2> = match s {
                Shape::Point(q) => vec![q],
                Shape::Line { a, b } | Shape::Curve { a, b } => vec![a, b],
                Shape::Circle { arc: Some(a), .. } => vec![a[0], a[2]],
                Shape::Circle { center, .. } => vec![center],
            };
            pts.into_iter().min_by(|a, b| dist(*a, p.hint).total_cmp(&dist(*b, p.hint)))?
        }
        (_, Shape::Point(q)) => q,
        _ => return None,
    };
    Some((at, r.from_model))
}

// ---------------------------------------------------------------------------------------------
// Annotations

/// A centerline (D5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CenterlineKind {
    /// Through two points.
    Points { a: PointRef, b: PointRef },
    /// Halfway between two edges (a hole's two silhouettes).
    Lines { a: EdgeRef, b: EdgeRef },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Centerline {
    pub kind: CenterlineKind,
    /// How far each end was dragged past its default length (view mm, may be negative).
    #[serde(default)]
    pub extend: [f64; 2],
}

/// A circle centerline (D5.2): through three points, or a centre and a point on the circle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum CircleCenterline {
    ThreePoints([PointRef; 3]),
    CenterPoint { center: PointRef, on: PointRef },
}

/// How a distance between two points is measured on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orient {
    /// Along the sheet's x.
    Horizontal,
    /// Along the sheet's y.
    Vertical,
    /// Point to point.
    Aligned,
}

/// What a dimension measures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DimKind {
    Diameter(EdgeRef),
    Radius(EdgeRef),
    /// Point to point (oriented), point to line or line to line (perpendicular).
    Distance { a: Pick, b: Pick, orient: Orient },
    /// Between two lines; the text's quadrant picks which of the angles.
    Angle { a: EdgeRef, b: EdgeRef },
}

/// A tolerance (D6.6); lengths in mm, angles in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Tolerance {
    #[default]
    None,
    /// ± value.
    Symmetric(f64),
    /// +upper / lower (lower is usually negative).
    Deviation { upper: f64, lower: f64 },
    /// The limits: value + upper over value + lower.
    Limits { upper: f64, lower: f64 },
}

/// A dimension's own format, overriding the drawing properties (D2.6, D6.6).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DimFormat {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    #[serde(default)]
    pub tolerance: Tolerance,
    /// Decimals (the drawing's when `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance_precision: Option<u8>,
    /// Dual units on or off (the drawing's setting when `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dual: Option<bool>,
}

/// A driven dimension (D6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub kind: DimKind,
    /// The text's centre (view 2D, model mm).
    pub text: P2,
    #[serde(default)]
    pub format: DimFormat,
    /// The value it had when it last measured (P3C.6): what it shows while it dangles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<f64>,
}

/// A hole callout (D6.7): the hole's spec, with a prefix ("4x") typed in Edit….
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleCallout {
    /// An edge of the hole (its circle).
    pub edge: EdgeRef,
    /// The text's left end, at the middle of its first line (view 2D).
    pub text: P2,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    /// The hole's spec as of the last update (P3C.6): what a dangling callout still shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<HoleInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnnotationKind {
    /// A cross at a circle's or arc's centre (D5.3).
    Centermark(EdgeRef),
    Centerline(Centerline),
    CircleCenterline(CircleCenterline),
    /// Where two lines would meet (D5.4).
    VirtualSharp { a: EdgeRef, b: EdgeRef },
    Dimension(Dimension),
    HoleCallout(HoleCallout),
    /// Baseline dimensions (P3C.8, D6.4).
    Baseline(crate::annotation_more::Baseline),
    /// Ordinate dimensions (P3C.8, D6.4).
    Ordinate(crate::annotation_more::Ordinate),
    /// A chamfer dimension (P3C.8, D6.4).
    ChamferDim(crate::annotation_more::ChamferDim),
    /// An arc length dimension (P3C.8, D6.4).
    ArcLength(crate::annotation_more::ArcLength),
    /// A GD&T feature control frame (P3C.8, X14).
    FeatureControl(crate::annotation_more::FeatureControl),
    /// A datum feature symbol (P3C.8, X14).
    Datum(crate::annotation_more::Datum),
    /// A surface finish symbol (P3C.8, X14).
    SurfaceFinish(crate::annotation_more::SurfaceFinish),
    /// A weld symbol (P3C.8, X14).
    Weld(crate::annotation_more::Weld),
    /// A callout (balloon) of an assembly view (P3C.5, D11.4).
    Callout(crate::assembly::Callout),
}

impl AnnotationKind {
    /// Where its text (or symbol) sits, view 2D, for dragging.
    pub fn text_mut(&mut self) -> Option<&mut P2> {
        match self {
            AnnotationKind::Dimension(d) => Some(&mut d.text),
            AnnotationKind::HoleCallout(c) => Some(&mut c.text),
            AnnotationKind::Baseline(b) => Some(&mut b.text),
            AnnotationKind::ChamferDim(c) => Some(&mut c.text),
            AnnotationKind::ArcLength(a) => Some(&mut a.text),
            AnnotationKind::FeatureControl(f) => Some(&mut f.text),
            AnnotationKind::Datum(d) => Some(&mut d.text),
            AnnotationKind::SurfaceFinish(f) => Some(&mut f.text),
            AnnotationKind::Weld(w) => Some(&mut w.text),
            AnnotationKind::Callout(c) => Some(&mut c.text),
            _ => None,
        }
    }
}

/// An annotation of a view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    pub kind: AnnotationKind,
}

impl Annotation {
    pub fn new(kind: AnnotationKind) -> Self {
        Self { id: AnnotationId::new(), kind }
    }

    /// What it is, for undo labels ("Insert dimension").
    pub fn noun(&self) -> &'static str {
        match &self.kind {
            AnnotationKind::Centermark(_) => "centermark",
            AnnotationKind::Centerline(_) => "centerline",
            AnnotationKind::CircleCenterline(_) => "circle centerline",
            AnnotationKind::VirtualSharp { .. } => "virtual sharp",
            AnnotationKind::Dimension(_) => "dimension",
            AnnotationKind::HoleCallout(_) => "hole callout",
            AnnotationKind::Baseline(_) => "baseline dimension",
            AnnotationKind::Ordinate(_) => "ordinate dimension",
            AnnotationKind::ChamferDim(_) => "chamfer dimension",
            AnnotationKind::ArcLength(_) => "arc length dimension",
            AnnotationKind::FeatureControl(_) => "feature control frame",
            AnnotationKind::Datum(_) => "datum feature",
            AnnotationKind::SurfaceFinish(_) => "surface finish symbol",
            AnnotationKind::Weld(_) => "weld symbol",
            AnnotationKind::Callout(_) => "callout",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Measuring

/// What a dimension's value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Length,
    Diameter,
    Radius,
    Angle,
}

/// A linear dimension's geometry in the view's frame: the two points it spans and the
/// direction it measures along.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Linear {
    p1: P2,
    p2: P2,
    u: P2,
    /// The lines the points lie on (their extension lines start at the segment's nearest end).
    seg1: Option<(P2, P2)>,
    seg2: Option<(P2, P2)>,
}

/// A measured value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measure {
    pub kind: ValueKind,
    /// mm or degrees.
    pub value: f64,
    /// Every reference came from the model's exact geometry.
    pub from_model: bool,
}

/// The sheet's x and y directions in a view's frame.
fn sheet_axes(view: &View) -> (P2, P2) {
    (rotate([1.0, 0.0], -view.rotation), rotate([0.0, 1.0], -view.rotation))
}

enum Geo {
    Point(P2),
    Line(P2, P2),
}

fn pick_geo(view: &View, m: &dyn ViewModel, p: &Pick) -> Option<(Geo, bool)> {
    match p {
        Pick::Point(pr) => resolve_point(view, m, pr).map(|(q, e)| (Geo::Point(q), e)),
        Pick::Edge(e) => {
            let r = resolve(view, m, e);
            let g = match r.shape {
                Shape::Point(q) => Geo::Point(q),
                Shape::Line { a, b } => Geo::Line(a, b),
                Shape::Circle { center, .. } => Geo::Point(center),
                Shape::Curve { a, b } => Geo::Line(a, b),
            };
            Some((g, r.from_model))
        }
    }
}

fn foot(p: P2, a: P2, b: P2) -> P2 {
    let d = unit(sub(b, a));
    add(a, mul(d, dot(sub(p, a), d)))
}

/// The linear layout of a distance, given where its text is.
fn linear(view: &View, m: &dyn ViewModel, a: &Pick, b: &Pick, orient: Orient, text: P2) -> Option<(Linear, bool)> {
    let (ga, ea) = pick_geo(view, m, a)?;
    let (gb, eb) = pick_geo(view, m, b)?;
    let (sx, sy) = sheet_axes(view);
    let exact = ea && eb;
    let out = match (ga, gb) {
        (Geo::Point(p1), Geo::Point(p2)) => {
            let u = match orient {
                Orient::Horizontal => sx,
                Orient::Vertical => sy,
                Orient::Aligned => unit(sub(p2, p1)),
            };
            Linear { p1, p2, u, seg1: None, seg2: None }
        }
        (Geo::Point(p), Geo::Line(l0, l1)) => {
            let u = perp(unit(sub(l1, l0)));
            Linear { p1: p, p2: near_on_segment(l0, l1, text), u, seg1: None, seg2: Some((l0, l1)) }
        }
        (Geo::Line(l0, l1), Geo::Point(p)) => {
            let u = perp(unit(sub(l1, l0)));
            Linear { p1: near_on_segment(l0, l1, text), p2: p, u, seg1: Some((l0, l1)), seg2: None }
        }
        (Geo::Line(a0, a1), Geo::Line(b0, b1)) => {
            let da = unit(sub(a1, a0));
            let db = unit(sub(b1, b0));
            // Not parallel: nothing to measure between them.
            if cross(da, db).abs() > 1e-3 {
                return None;
            }
            let u = perp(da);
            let p1 = near_on_segment(a0, a1, text);
            let p2 = near_on_segment(b0, b1, text);
            Linear { p1, p2, u, seg1: Some((a0, a1)), seg2: Some((b0, b1)) }
        }
    };
    Some((out, exact))
}

/// The point of segment `a`–`b` nearest the line through `text` perpendicular to it.
fn near_on_segment(a: P2, b: P2, text: P2) -> P2 {
    let d = sub(b, a);
    let l2 = dot(d, d).max(1e-300);
    add(a, mul(d, (dot(sub(text, a), d) / l2).clamp(0.0, 1.0)))
}

/// Two lines' crossing `x`, the rays `ua`, `ub` along them that put the text between them, the
/// lines' segments, and whether both came from the model.
struct AngleRays {
    x: P2,
    ua: P2,
    ub: P2,
    sa: (P2, P2),
    sb: (P2, P2),
    exact: bool,
}

fn angle_rays(view: &View, m: &dyn ViewModel, a: &EdgeRef, b: &EdgeRef, text: P2) -> Option<AngleRays> {
    let ra = resolve(view, m, a);
    let rb = resolve(view, m, b);
    let line = |s: Shape| match s {
        Shape::Line { a, b } | Shape::Curve { a, b } => Some((a, b)),
        _ => None,
    };
    let (a0, a1) = line(ra.shape)?;
    let (b0, b1) = line(rb.shape)?;
    let da = unit(sub(a1, a0));
    let db = unit(sub(b1, b0));
    let den = cross(da, db);
    if den.abs() < 1e-9 {
        return None;
    }
    // a0 + s·da = b0 + t·db.
    let s = cross(sub(b0, a0), db) / den;
    let x = add(a0, mul(da, s));
    let c = sub(text, x);
    let alpha = cross(c, db) / den;
    let beta = cross(da, c) / den;
    let ua = if alpha < 0.0 { mul(da, -1.0) } else { da };
    let ub = if beta < 0.0 { mul(db, -1.0) } else { db };
    Some(AngleRays { x, ua, ub, sa: (a0, a1), sb: (b0, b1), exact: ra.from_model && rb.from_model })
}

/// A dimension's value (mm or degrees) and kind, from the model where it can (D6.1).
pub fn measure(view: &View, m: &dyn ViewModel, d: &Dimension) -> Option<Measure> {
    match &d.kind {
        DimKind::Diameter(e) | DimKind::Radius(e) => {
            let r = resolve(view, m, e);
            let (_, radius) = r.shape.circle()?;
            let dia = matches!(d.kind, DimKind::Diameter(_));
            Some(Measure {
                kind: if dia { ValueKind::Diameter } else { ValueKind::Radius },
                value: if dia { 2.0 * radius } else { radius },
                from_model: r.from_model,
            })
        }
        DimKind::Distance { a, b, orient } => {
            let (l, exact) = linear(view, m, a, b, *orient, d.text)?;
            Some(Measure { kind: ValueKind::Length, value: dot(sub(l.p2, l.p1), l.u).abs(), from_model: exact })
        }
        DimKind::Angle { a, b } => {
            let AngleRays { ua, ub, exact, .. } = angle_rays(view, m, a, b, d.text)?;
            Some(Measure {
                kind: ValueKind::Angle,
                value: dot(ua, ub).clamp(-1.0, 1.0).acos().to_degrees(),
                from_model: exact,
            })
        }
    }
}

/// Centres a distance's text in its span (along the measured direction), keeping its offset
/// from the geometry (after a re-attach, D6.5: the text goes between the new ends).
pub fn centre_text_in_span(view: &View, m: &dyn ViewModel, d: &mut Dimension) {
    let DimKind::Distance { a, b, orient } = &d.kind else { return };
    let Some((l, _)) = linear(view, m, a, b, *orient, d.text) else { return };
    let t = dot(sub(d.text, l.p1), l.u);
    let span = dot(sub(l.p2, l.p1), l.u);
    d.text = add(d.text, mul(l.u, span / 2.0 - t));
}

// ---------------------------------------------------------------------------------------------
// Text

/// A vector symbol (not in Inter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    /// ⌴
    Counterbore,
    /// ⌵
    Countersink,
    /// ↧
    Depth,
    /// ⌒ (arc length, P3C.8)
    Arc,
}

impl Symbol {
    pub fn of(c: char) -> Option<Symbol> {
        match c {
            '⌴' => Some(Symbol::Counterbore),
            '⌵' => Some(Symbol::Countersink),
            '↧' => Some(Symbol::Depth),
            '⌒' => Some(Symbol::Arc),
            _ => None,
        }
    }

    pub fn char(self) -> char {
        match self {
            Symbol::Counterbore => '⌴',
            Symbol::Countersink => '⌵',
            Symbol::Depth => '↧',
            Symbol::Arc => '⌒',
        }
    }

    /// Advance, in cap heights.
    pub fn width(self) -> f64 {
        match self {
            Symbol::Counterbore | Symbol::Countersink => 0.95,
            Symbol::Depth => 0.75,
            Symbol::Arc => 1.1,
        }
    }
}

/// A piece of a line of text.
#[derive(Debug, Clone, PartialEq)]
pub enum Run {
    Text(String),
    Sym(Symbol),
    /// Two small lines stacked (a deviation tolerance).
    Stack(String, String),
}

/// Lines of runs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextBlock {
    pub lines: Vec<Vec<Run>>,
}

impl TextBlock {
    /// The text as a string: lines joined by spaces, symbols as their characters, stacks as
    /// "upper/lower".
    pub fn plain(&self) -> String {
        self.lines
            .iter()
            .map(|l| {
                l.iter()
                    .map(|r| match r {
                        Run::Text(s) => s.clone(),
                        Run::Sym(s) => s.char().to_string(),
                        Run::Stack(a, b) => format!("{a}/{b}"),
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Splits a string into text and symbol runs.
pub fn runs(s: &str) -> Vec<Run> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        match Symbol::of(c) {
            Some(sym) => {
                if !cur.is_empty() {
                    out.push(Run::Text(std::mem::take(&mut cur)));
                }
                out.push(Run::Sym(sym));
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(Run::Text(cur));
    }
    out
}

fn format_decimals(v: f64, decimals: u8, leading: bool, style: &DrawingStyle) -> String {
    crate::style::format_number(v, decimals, leading, style.length_trailing_zeros, style.decimal_separator)
}

/// A dimension's text: its value by the drawing's units and precision, with the dimension's own
/// prefix, suffix, tolerance, precision and dual units.
pub fn dimension_text(style: &DrawingStyle, kind: ValueKind, value: f64, f: &DimFormat) -> TextBlock {
    let mut st = style.clone();
    if let Some(p) = f.precision {
        match kind {
            ValueKind::Angle => st.angular_precision = p,
            _ => st.precision = p,
        }
    }
    let tp = f.tolerance_precision.unwrap_or(match kind {
        ValueKind::Angle => st.angular_precision,
        _ => st.tolerance_precision,
    });
    let fmt = |v: f64| match kind {
        ValueKind::Angle => st.format_angle(v),
        _ => st.format_length(v),
    };
    let tol = |v: f64| match kind {
        ValueKind::Angle => format!("{}°", format_decimals(v.abs(), tp, st.angle_leading_zeros, &st)),
        _ => format_decimals(v.abs() / st.units.mm(), tp, st.tolerance_leading_zeros, &st),
    };
    let sign = |v: f64| if v < 0.0 { "-" } else { "+" };
    let symbol = match kind {
        ValueKind::Diameter => "Ø",
        ValueKind::Radius => "R",
        _ => "",
    };
    // A count prefix ("3x") stacks over the value, as Onshape draws it (`ex3-step9.png`).
    let count = {
        let p = f.prefix.trim();
        p.len() >= 2 && p.ends_with(['x', 'X']) && p[..p.len() - 1].chars().all(|c| c.is_ascii_digit())
    };
    let mut lines = Vec::new();
    if count {
        lines.push(runs(f.prefix.trim()));
    }
    let mut first = if count { Vec::new() } else { runs(&f.prefix) };
    match f.tolerance {
        Tolerance::Limits { upper, lower } => {
            let mut l1 = first.clone();
            l1.extend(runs(&format!("{symbol}{}", fmt(value + upper))));
            l1.extend(runs(&f.suffix));
            let mut l2 = first;
            l2.extend(runs(&format!("{symbol}{}", fmt(value + lower))));
            l2.extend(runs(&f.suffix));
            lines.push(l1);
            lines.push(l2);
        }
        t => {
            first.extend(runs(&format!("{symbol}{}", fmt(value))));
            match t {
                Tolerance::Symmetric(v) => first.push(Run::Text(format!("±{}", tol(v)))),
                Tolerance::Deviation { upper, lower } => first.push(Run::Stack(
                    format!("{}{}", sign(upper), tol(upper)),
                    format!("{}{}", sign(lower), tol(lower)),
                )),
                _ => {}
            }
            first.extend(runs(&f.suffix));
            lines.push(first);
        }
    }
    let dual = f.dual.unwrap_or(style.show_dual);
    if dual && kind != ValueKind::Angle {
        let d = crate::style::format_number(
            value / style.dual_units.mm(),
            style.dual_precision,
            true,
            style.length_trailing_zeros,
            style.decimal_separator,
        );
        let unit = if style.show_dual_unit { format!(" {}", style.dual_units.symbol()) } else { String::new() };
        let text = format!("[{symbol}{d}{unit}]");
        match style.dual_location {
            crate::style::DualLocation::Top => lines.insert(0, vec![Run::Text(text)]),
            _ => lines.push(vec![Run::Text(text)]),
        }
    }
    TextBlock { lines: merge_lines(lines) }
}

fn merge_lines(lines: Vec<Vec<Run>>) -> Vec<Vec<Run>> {
    lines
        .into_iter()
        .map(|l| {
            let mut out: Vec<Run> = Vec::new();
            for r in l {
                match (out.last_mut(), r) {
                    (Some(Run::Text(a)), Run::Text(b)) => a.push_str(&b),
                    (_, r) => out.push(r),
                }
            }
            out
        })
        .collect()
}

/// A hole callout's text from the hole's spec, e.g. `4x Ø.266 THRU` / `⌴Ø.438 ↧.250`.
pub fn hole_text(style: &DrawingStyle, h: &HoleInfo, prefix: &str) -> TextBlock {
    let l = |v: f64| style.format_length(v);
    let main = match (&h.thread, h.end) {
        (Some((t, _)), HoleEnd::Through) => format!("{t} THRU"),
        (Some((t, d)), _) => format!("{t} ↧{}", l(*d)),
        (None, HoleEnd::Through) => format!("Ø{} THRU", l(h.diameter)),
        (None, HoleEnd::Blind) => format!("Ø{} ↧{}", l(h.diameter), l(h.depth)),
        (None, HoleEnd::UpToNext) => format!("Ø{} UP TO NEXT", l(h.diameter)),
        (None, HoleEnd::UpToEntity) => format!("Ø{} UP TO ENTITY", l(h.diameter)),
    };
    let first = if prefix.trim().is_empty() { main } else { format!("{} {main}", prefix.trim()) };
    let mut lines = vec![runs(&first)];
    if let Some((d, depth)) = h.cbore {
        lines.push(runs(&format!("⌴Ø{} ↧{}", l(d), l(depth))));
    }
    if let Some((d, angle)) = h.csink {
        lines.push(runs(&format!("⌵Ø{} X {}°", l(d), crate::style::format_number(angle, 0, true, false, style.decimal_separator))));
    }
    TextBlock { lines }
}

/// The hole feature an edge belongs to: the first of its faces made by a Hole feature.
pub fn hole_of<'a>(m: &'a dyn ViewModel, e: &EdgeRef) -> Option<&'a HoleInfo> {
    let faces: Vec<&FaceName> = match (&e.edge, &e.face) {
        (Some(n), _) => n.faces.iter().collect(),
        (None, Some(f)) => vec![f],
        _ => Vec::new(),
    };
    faces.into_iter().find_map(|f| m.hole(&f.op))
}

/// The text a dimension shows now, with its value.
pub fn dimension_display(style: &DrawingStyle, view: &View, m: &dyn ViewModel, d: &Dimension) -> Option<(TextBlock, Measure)> {
    let v = measure(view, m, d)?;
    Some((dimension_text(style, v.kind, v.value, &d.format), v))
}

// ---------------------------------------------------------------------------------------------
// Text layout

/// Width of `s` in cap heights, in the face annotation text is drawn with (Inter Medium, see
/// [`crate::rich::FACE_REGULAR`]).
pub fn text_width(s: &str) -> f64 {
    crate::rich::width(s, &crate::rich::CharStyle::default())
}

/// A piece of text placed on the sheet: its left end at the middle of the capitals.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedText {
    pub pos: P2,
    /// Cap height (mm).
    pub height: f64,
    pub text: String,
}

/// A laid-out text block.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Label {
    pub texts: Vec<PlacedText>,
    /// The symbols' strokes.
    pub strokes: Vec<Vec<P2>>,
    /// Where each symbol is (its left end at the capitals' middle), its height and character:
    /// exports put the character there as text (a PDF's ActualText, P3C.8).
    pub symbols: Vec<(P2, f64, char)>,
    /// The block's box (min, max).
    pub min: P2,
    pub max: P2,
}

fn run_width(r: &Run, h: f64) -> f64 {
    match r {
        Run::Text(s) => text_width(s) * h,
        Run::Sym(s) => s.width() * h,
        Run::Stack(a, b) => text_width(a).max(text_width(b)) * h * 0.7 + 0.15 * h,
    }
}

/// Line spacing, in cap heights.
const LINE: f64 = 1.7;

/// Lays out `block` with cap height `h`: centred on `anchor`, or left-aligned with its first
/// line's middle at `anchor`.
pub fn layout_label(block: &TextBlock, anchor: P2, h: f64, centered: bool) -> Label {
    let widths: Vec<f64> = block.lines.iter().map(|l| l.iter().map(|r| run_width(r, h)).sum()).collect();
    let wmax = widths.iter().copied().fold(0.0, f64::max);
    let n = block.lines.len().max(1) as f64;
    let total_h = h * (1.0 + LINE * (n - 1.0));
    let (x0, y_top) = if centered {
        (anchor[0] - wmax / 2.0, anchor[1] + total_h / 2.0 - h / 2.0)
    } else {
        (anchor[0], anchor[1])
    };
    let mut out = Label::default();
    for (i, (line, w)) in block.lines.iter().zip(&widths).enumerate() {
        let y = y_top - LINE * h * i as f64;
        let mut x = if centered { anchor[0] - w / 2.0 } else { x0 };
        for r in line {
            let rw = run_width(r, h);
            match r {
                Run::Text(s) => out.texts.push(PlacedText { pos: [x, y], height: h, text: s.clone() }),
                Run::Sym(s) => {
                    out.strokes.extend(symbol_strokes(*s, [x, y - h / 2.0], h));
                    out.symbols.push(([x, y], h, s.char()));
                }
                Run::Stack(a, b) => {
                    let sh = h * 0.7;
                    out.texts.push(PlacedText { pos: [x + 0.15 * h, y + 0.45 * h], height: sh, text: a.clone() });
                    out.texts.push(PlacedText { pos: [x + 0.15 * h, y - 0.45 * h], height: sh, text: b.clone() });
                }
            }
            x += rw;
        }
    }
    let pad = 0.35 * h;
    let top = y_top + h / 2.0;
    out.min = [x0 - pad, top - total_h - pad];
    out.max = [x0 + wmax + pad, top + pad];
    out
}

/// The strokes of a symbol whose baseline starts at `origin`, `h` high (cap height).
pub fn symbol_strokes(s: Symbol, origin: P2, h: f64) -> Vec<Vec<P2>> {
    let p = |x: f64, y: f64| [origin[0] + x * h, origin[1] + y * h];
    match s {
        Symbol::Counterbore => vec![vec![p(0.12, 0.85), p(0.12, 0.0), p(0.82, 0.0), p(0.82, 0.85)]],
        Symbol::Countersink => vec![vec![p(0.1, 0.85), p(0.47, 0.0), p(0.84, 0.85)]],
        Symbol::Depth => vec![
            vec![p(0.37, 1.0), p(0.37, 0.12)],
            vec![p(0.17, 0.38), p(0.37, 0.1), p(0.57, 0.38)],
            vec![p(0.1, 0.0), p(0.64, 0.0)],
        ],
        // As tall as the capitals and wider than a digit (P3C.8's delta: it read as a speck).
        Symbol::Arc => vec![(0..=20)
            .map(|i| {
                let t = std::f64::consts::PI * i as f64 / 20.0;
                p(0.5 + 0.42 * t.cos(), 0.3 + 0.62 * t.sin())
            })
            .collect()],
    }
}

// ---------------------------------------------------------------------------------------------
// Graphics

/// A grip of a selected annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GripKind {
    /// Drag the text.
    Text,
    /// Drag attachment `i` onto other geometry (D6.5).
    Attach(usize),
    /// Drag a centerline's end to extend it.
    End(usize),
}

/// An annotation on the sheet (paper mm): thin strokes, filled arrowheads, text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnGraphics {
    pub strokes: Vec<Vec<P2>>,
    /// Filled triangles.
    pub fills: Vec<[P2; 3]>,
    pub texts: Vec<PlacedText>,
    /// Text boxes (for picking).
    pub boxes: Vec<(P2, P2)>,
    pub grips: Vec<(P2, GripKind)>,
    /// The geometry it is attached to, on the sheet (highlighted blue while selected).
    pub attached: Vec<Vec<P2>>,
    /// Which of `attached` no longer exist (shown red while selected, P3C.6).
    pub attached_dead: Vec<bool>,
    /// A reference no longer resolves (P3C.6, D13.3): the annotation is drawn red, as it last
    /// was (see [`is_dangling`]).
    pub dangling: bool,
    /// The vector symbols' places and characters (see [`Label::symbols`]).
    pub symbols: Vec<(P2, f64, char)>,
}

impl AnnGraphics {
    /// Distance from a sheet point to the drawn annotation (0 inside a text box).
    pub fn distance(&self, p: P2) -> f64 {
        let mut d = f64::MAX;
        for (lo, hi) in &self.boxes {
            if p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1] {
                return 0.0;
            }
        }
        for s in &self.strokes {
            for w in s.windows(2) {
                d = d.min(seg_distance(p, w[0], w[1]));
            }
        }
        for t in &self.fills {
            d = d.min(dist(p, t[0]));
        }
        d
    }

    fn label(&mut self, l: Label) {
        self.texts.extend(l.texts);
        self.strokes.extend(l.strokes);
        self.symbols.extend(l.symbols);
        self.boxes.push((l.min, l.max));
    }

    fn arrow(&mut self, tip: P2, dir: P2, length: f64) {
        let d = unit(dir);
        let base = sub(tip, mul(d, length));
        let n = mul(perp(d), length * 0.18);
        self.fills.push([tip, add(base, n), sub(base, n)]);
    }
}

/// The chain line pattern of centre lines (paper mm): long, gap, short, gap.
pub const CHAIN: [f64; 4] = [6.0, 1.2, 1.5, 1.2];

fn chain(g: &mut AnnGraphics, pts: &[P2]) {
    g.strokes.extend(dashes(pts, &CHAIN));
}

/// Sheet sizes (paper mm) from the drawing properties.
struct Sizes {
    text: f64,
    arrow: f64,
    gap: f64,
    beyond: f64,
    mark: f64,
    extension: f64,
}

impl Sizes {
    fn of(style: &DrawingStyle) -> Self {
        Self {
            text: style.dim_text_height,
            arrow: style.dim_arrow_length,
            gap: style.extension_gap,
            beyond: style.extension_beyond,
            mark: style.centermark_size,
            extension: style.centerline_extension,
        }
    }
}

/// The annotation's graphics on the sheet, or `None` when its references don't resolve to
/// anything it can draw.
pub fn annotation_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, a: &Annotation) -> Option<AnnGraphics> {
    // A dangling annotation's dead references are drawn from their stored shapes (never
    // re-attached to a guess, D13.3); its live ones follow the model.
    if let AnnotationKind::Callout(c) = &a.kind {
        return Some(crate::assembly::callout_graphics(style, view, m, c));
    }
    let dangling = is_dangling(view, m, a);
    let sz = Sizes::of(style);
    let s = |p: P2| view.to_sheet(p);
    let k = view.scale.factor();
    let mut g = AnnGraphics { dangling, ..AnnGraphics::default() };
    let attach = |g: &mut AnnGraphics, e: &EdgeRef| {
        let r = resolve(view, m, e);
        g.attached.push(r.shape.polyline().into_iter().map(s).collect());
        g.attached_dead.push(ref_dangles(view, m, e));
    };
    match &a.kind {
        AnnotationKind::Centermark(e) if dangling => {
            // A dangling centermark is a filled dot where its circle's centre was
            // (`ex3-step9.png`, `ex3-step10.png`).
            let (c, _) = resolve(view, m, e).shape.circle()?;
            let c = s(c);
            let rad = sz.mark * 0.3;
            let n = 16;
            for i in 0..n {
                let (a0, a1) = (std::f64::consts::TAU * i as f64 / n as f64, std::f64::consts::TAU * (i + 1) as f64 / n as f64);
                g.fills.push([c, add(c, [rad * a0.cos(), rad * a0.sin()]), add(c, [rad * a1.cos(), rad * a1.sin()])]);
            }
            g.strokes.push(vec![c, c]);
            attach(&mut g, e);
        }
        AnnotationKind::Centermark(e) => {
            let r = resolve(view, m, e);
            let (c, radius) = r.shape.circle()?;
            let (c, radius) = (s(c), radius * k);
            let (ax, ay) = (rotate([1.0, 0.0], view.rotation), rotate([0.0, 1.0], view.rotation));
            let half = sz.mark / 2.0;
            for d in [ax, ay] {
                g.strokes.push(vec![sub(c, mul(d, half)), add(c, mul(d, half))]);
                // Small circles (holes) get the cross's arms out past the rim; large ones only
                // the cross (`ex1-step7.png`).
                let from = half + 1.2;
                let to = radius + sz.extension;
                if to > from + 0.5 && radius <= 4.0 * sz.mark {
                    for sgn in [1.0, -1.0] {
                        g.strokes.push(vec![add(c, mul(d, sgn * from)), add(c, mul(d, sgn * to))]);
                    }
                }
            }
            attach(&mut g, e);
        }
        AnnotationKind::Centerline(cl) => {
            let (a0, a1) = centerline_ends(view, m, cl)?;
            let (a0, a1) = (s(a0), s(a1));
            let d = unit(sub(a1, a0));
            let e0 = sub(a0, mul(d, sz.extension + cl.extend[0] * k));
            let e1 = add(a1, mul(d, sz.extension + cl.extend[1] * k));
            chain(&mut g, &[e0, e1]);
            g.grips.push((e0, GripKind::End(0)));
            g.grips.push((e1, GripKind::End(1)));
            match &cl.kind {
                CenterlineKind::Points { a, b } => {
                    attach(&mut g, &a.edge);
                    attach(&mut g, &b.edge);
                }
                CenterlineKind::Lines { a, b } => {
                    attach(&mut g, a);
                    attach(&mut g, b);
                }
            }
        }
        AnnotationKind::CircleCenterline(cc) => {
            let (c, r) = circle_centerline(view, m, cc)?;
            let (c, r) = (s(c), r * k);
            let n = 128;
            let pts: Vec<P2> = (0..=n)
                .map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / n as f64 + std::f64::consts::FRAC_PI_4;
                    add(c, [r * t.cos(), r * t.sin()])
                })
                .collect();
            chain(&mut g, &pts);
        }
        AnnotationKind::VirtualSharp { a, b } => {
            let ra = resolve(view, m, a).shape.line()?;
            let rb = resolve(view, m, b).shape.line()?;
            let x = intersect(ra, rb)?;
            let xs = s(x);
            match style.virtual_sharp {
                SharpStyle::Mark => {
                    let h = sz.mark / 2.0;
                    let r = view.rotation + std::f64::consts::FRAC_PI_4;
                    for d in [rotate([1.0, 0.0], r), rotate([0.0, 1.0], r)] {
                        g.strokes.push(vec![sub(xs, mul(d, h)), add(xs, mul(d, h))]);
                    }
                }
                SharpStyle::Extension => {
                    for (l0, l1) in [ra, rb] {
                        let near = if dist(l0, x) < dist(l1, x) { l0 } else { l1 };
                        let (n, xs2) = (s(near), xs);
                        let d = unit(sub(xs2, n));
                        if dist(n, xs2) > sz.gap {
                            g.strokes.push(vec![add(n, mul(d, sz.gap)), add(xs2, mul(d, sz.beyond * 0.5))]);
                        }
                    }
                }
            }
            attach(&mut g, a);
            attach(&mut g, b);
        }
        AnnotationKind::Dimension(d) => {
            let (mut block, v) = dimension_display(style, view, m, d)?;
            // A dangling dimension keeps its last value (D13.3, `ex3-step9.png`'s red 75.00).
            if dangling && let Some(last) = d.last {
                block = dimension_text(style, v.kind, last, &d.format);
            }
            let t = s(d.text);
            let label = layout_label(&block, t, sz.text, true);
            let half = [(label.max[0] - label.min[0]) / 2.0, (label.max[1] - label.min[1]) / 2.0];
            match &d.kind {
                DimKind::Diameter(e) | DimKind::Radius(e) => {
                    let r = resolve(view, m, e);
                    let (c, radius) = r.shape.circle()?;
                    // An arc's leader lands on the arc itself (its nearer end when the text is
                    // off to one side of it).
                    let arc = match r.shape {
                        Shape::Circle { arc: Some(a), .. } => Some(a.map(s)),
                        _ => None,
                    };
                    let (c, radius) = (s(c), radius * k);
                    radial(&mut g, &sz, c, radius, arc, t, half, matches!(d.kind, DimKind::Diameter(_)));
                    attach(&mut g, e);
                }
                DimKind::Distance { a, b, orient } => {
                    let (l, _) = linear(view, m, a, b, *orient, d.text)?;
                    let seg = |s0: Option<(P2, P2)>| s0.map(|(p, q)| (s(p), s(q)));
                    let u = unit(sub(s(add(l.p1, l.u)), s(l.p1)));
                    linear_graphics(&mut g, &sz, s(l.p1), s(l.p2), u, t, half, seg(l.seg1), seg(l.seg2));
                    attach(&mut g, a.edge());
                    attach(&mut g, b.edge());
                }
                DimKind::Angle { a, b } => {
                    let AngleRays { x, ua, ub, sa, sb, .. } = angle_rays(view, m, a, b, d.text)?;
                    let to_sheet_dir = |v: P2| unit(sub(s(add(x, v)), s(x)));
                    let seg = |(p, q): (P2, P2)| (s(p), s(q));
                    angle_graphics(&mut g, &sz, s(x), to_sheet_dir(ua), to_sheet_dir(ub), t, half, seg(sa), seg(sb));
                    attach(&mut g, a);
                    attach(&mut g, b);
                }
            }
            g.label(label);
            g.grips.push((t, GripKind::Text));
        }
        AnnotationKind::HoleCallout(hc) => {
            let info = hole_of(m, &hc.edge).or(hc.last.as_ref())?;
            let block = hole_text(style, info, &hc.prefix);
            let r = resolve(view, m, &hc.edge);
            let (c, radius) = r.shape.circle()?;
            let (c, radius) = (s(c), radius * k);
            let t = s(hc.text);
            // The text on the side away from the hole, left-aligned when it is to the right.
            let right = t[0] >= c[0];
            let w = {
                let l = layout_label(&block, [0.0, 0.0], sz.text, false);
                l.max[0] - l.min[0] - 0.7 * sz.text
            };
            let left_x = if right { t[0] } else { t[0] - w };
            let label = layout_label(&block, [left_x, t[1]], sz.text, false);
            let pad = 0.35 * sz.text;
            let land_end = if right { [left_x - pad, t[1]] } else { [left_x + w + pad, t[1]] };
            let land_start = add(land_end, [if right { -sz.arrow } else { sz.arrow }, 0.0]);
            let dir = unit(sub(land_start, c));
            let tip = add(c, mul(dir, radius));
            g.strokes.push(vec![tip, land_start, land_end]);
            g.arrow(tip, sub(tip, land_start), sz.arrow);
            g.label(label);
            g.grips.push((t, GripKind::Text));
            g.grips.push((tip, GripKind::Attach(0)));
            attach(&mut g, &hc.edge);
        }
        other => {
            use crate::annotation_more as more;
            let mut x = match other {
                AnnotationKind::Baseline(b) => more::baseline_graphics(style, view, m, b)?,
                AnnotationKind::Ordinate(o) => more::ordinate_graphics(style, view, m, o)?,
                AnnotationKind::ChamferDim(c) => more::chamfer_graphics(style, view, m, c)?,
                AnnotationKind::ArcLength(a) => more::arc_length_graphics(style, view, m, a)?,
                AnnotationKind::FeatureControl(f) => more::fcf_graphics(style, view, m, f)?,
                AnnotationKind::Datum(d) => more::datum_graphics(style, view, m, d)?,
                AnnotationKind::SurfaceFinish(f) => more::finish_graphics(style, view, m, f)?,
                AnnotationKind::Weld(w) => more::weld_graphics(style, view, m, w)?,
                _ => return None,
            };
            x.dangling = dangling;
            return Some(x);
        }
    }
    Some(g)
}

/// Whether a reference no longer resolves: it names model topology (an edge or a face) and
/// neither the model nor the projection has it (P3C.6). A reference without a name (a 2D pick)
/// never dangles.
pub fn ref_dangles(view: &View, m: &dyn ViewModel, r: &EdgeRef) -> bool {
    (r.edge.is_some() || r.face.is_some()) && !resolve(view, m, r).found
}

/// Every edge reference of an annotation.
pub fn edge_refs(a: &AnnotationKind) -> Vec<&EdgeRef> {
    match a {
        AnnotationKind::Centermark(e) => vec![e],
        AnnotationKind::Centerline(cl) => match &cl.kind {
            CenterlineKind::Points { a, b } => vec![&a.edge, &b.edge],
            CenterlineKind::Lines { a, b } => vec![a, b],
        },
        AnnotationKind::CircleCenterline(CircleCenterline::ThreePoints(p)) => p.iter().map(|p| &p.edge).collect(),
        AnnotationKind::CircleCenterline(CircleCenterline::CenterPoint { center, on }) => vec![&center.edge, &on.edge],
        AnnotationKind::VirtualSharp { a, b } => vec![a, b],
        AnnotationKind::Dimension(d) => match &d.kind {
            DimKind::Diameter(e) | DimKind::Radius(e) => vec![e],
            DimKind::Distance { a, b, .. } => vec![a.edge(), b.edge()],
            DimKind::Angle { a, b } => vec![a, b],
        },
        AnnotationKind::HoleCallout(hc) => vec![&hc.edge],
        AnnotationKind::Baseline(b) => std::iter::once(b.base.edge()).chain(b.targets.iter().map(|t| t.edge())).collect(),
        AnnotationKind::Ordinate(o) => std::iter::once(&o.origin.edge).chain(o.points.iter().map(|p| &p.edge)).collect(),
        AnnotationKind::ChamferDim(c) => vec![&c.edge],
        AnnotationKind::ArcLength(a) => vec![&a.edge],
        AnnotationKind::FeatureControl(f) => f.edge.iter().collect(),
        AnnotationKind::Datum(d) => vec![&d.edge],
        AnnotationKind::SurfaceFinish(f) => vec![&f.edge],
        AnnotationKind::Weld(w) => vec![&w.edge],
        AnnotationKind::Callout(_) => Vec::new(),
    }
}

/// Whether the annotation dangles (D13.3): one of its references no longer resolves, or a hole
/// callout's hole feature is gone.
pub fn is_dangling(view: &View, m: &dyn ViewModel, a: &Annotation) -> bool {
    if edge_refs(&a.kind).into_iter().any(|r| ref_dangles(view, m, r)) {
        return true;
    }
    if let AnnotationKind::Callout(c) = &a.kind {
        return crate::assembly::callout_dangles(m, c);
    }
    matches!(&a.kind, AnnotationKind::HoleCallout(hc) if hole_of(m, &hc.edge).is_none() && (hc.edge.edge.is_some() || hc.edge.face.is_some()))
}

/// A diameter or radius: a leader onto the rim with a landing at the text (text outside), or a
/// line across the circle (text inside). A radius's leader stops at the arc (`ex3-step9.png`),
/// on the arc's span.
#[allow(clippy::too_many_arguments)]
fn radial(g: &mut AnnGraphics, sz: &Sizes, c: P2, r: f64, arc: Option<[P2; 3]>, t: P2, half: P2, diameter: bool) {
    if dist(t, c) > r + 1e-9 {
        let right = t[0] >= c[0];
        let land_end = [if right { t[0] - half[0] } else { t[0] + half[0] }, t[1]];
        let land_start = add(land_end, [if right { -sz.arrow } else { sz.arrow }, 0.0]);
        let dir = unit(sub(land_start, c));
        let tip = match arc {
            Some(a) if !on_arc(c, a, dir) => {
                if dist(a[0], land_start) < dist(a[2], land_start) { a[0] } else { a[2] }
            }
            _ => add(c, mul(dir, r)),
        };
        if dist(land_start, c) > r {
            g.strokes.push(vec![tip, land_start, land_end]);
            g.arrow(tip, sub(tip, land_start), sz.arrow);
        } else {
            g.strokes.push(vec![land_start, land_end]);
        }
        g.grips.push((tip, GripKind::Attach(0)));
    } else {
        let d = unit(sub(t, c));
        let d = if len(sub(t, c)) < 1e-9 { [std::f64::consts::FRAC_1_SQRT_2; 2] } else { d };
        let a = if diameter { sub(c, mul(d, r)) } else { c };
        let b = add(c, mul(d, r));
        g.strokes.push(vec![a, b]);
        g.arrow(b, d, sz.arrow);
        if diameter {
            g.arrow(a, mul(d, -1.0), sz.arrow);
        }
        g.grips.push((b, GripKind::Attach(0)));
    }
}

/// How far a line along `dir` from a label's centre stays from it: to the box's edge plus a gap.
fn label_extent(half: P2, dir: P2, gap: f64) -> f64 {
    let (dx, dy) = (dir[0].abs(), dir[1].abs());
    let to_edge = if dx < 1e-9 {
        half[1]
    } else if dy < 1e-9 {
        half[0]
    } else {
        (half[0] / dx).min(half[1] / dy)
    };
    to_edge + gap
}

/// The extension line from `p` (on the geometry, or the nearest end of the segment `seg`) to
/// `to` on the dimension line.
fn extension(g: &mut AnnGraphics, sz: &Sizes, p: P2, to: P2) {
    let l = dist(p, to);
    if l <= sz.gap {
        return;
    }
    let d = unit(sub(to, p));
    g.strokes.push(vec![add(p, mul(d, sz.gap)), add(to, mul(d, sz.beyond))]);
}

#[allow(clippy::too_many_arguments)]
fn linear_graphics(
    g: &mut AnnGraphics,
    sz: &Sizes,
    p1: P2,
    p2: P2,
    u: P2,
    t: P2,
    half: P2,
    _seg1: Option<(P2, P2)>,
    _seg2: Option<(P2, P2)>,
) {
    let tn = perp(u);
    // A vertical dimension whose text lies along its span keeps its (horizontal) text beside the
    // line, on the side away from the geometry (`ex1-drawing.png`'s 6.000, `ex3-step9.png`'s
    // 25.00); otherwise the line runs through the text.
    let along_t = dot(sub(t, p1), u);
    let span_u = dot(sub(p2, p1), u);
    let beside = u[1].abs() > 0.99 && along_t >= span_u.min(0.0) && along_t <= span_u.max(0.0);
    let line_at = if beside {
        let side = if dot(sub(p1, t), tn) >= 0.0 { 1.0 } else { -1.0 };
        add(t, mul(tn, side * (half[0] + 0.8)))
    } else {
        t
    };
    // The dimension line, along u.
    let d1 = add(p1, mul(tn, dot(sub(line_at, p1), tn)));
    let d2 = add(p2, mul(tn, dot(sub(line_at, p2), tn)));
    extension(g, sz, p1, d1);
    extension(g, sz, p2, d2);
    g.grips.push((p1, GripKind::Attach(0)));
    g.grips.push((p2, GripKind::Attach(1)));
    let l = dist(d1, d2);
    let dir = if l < 1e-9 { u } else { unit(sub(d2, d1)) };
    let tpos = dot(sub(t, d1), dir);
    let e = if beside { 0.0 } else { label_extent(half, dir, 0.8) };
    // Arrows inside when they and the text (if it sits between them) fit; else outside.
    let text_between = !beside && tpos > 0.0 && tpos < l;
    let inside = l >= 2.0 * sz.arrow + 1.0 && (!text_between || l >= 2.0 * e + 2.0 * sz.arrow);
    let (lo, hi) = if inside {
        ((tpos + e).min(0.0), (tpos - e).max(l))
    } else {
        ((tpos + e).min(-2.0 * sz.arrow), (tpos - e).max(l + 2.0 * sz.arrow))
    };
    let at = |s: f64| add(d1, mul(dir, s));
    // The line from lo to hi, less the label.
    let (b0, b1) = (tpos - e, tpos + e);
    for (s0, s1) in [(lo, b0.min(hi)), (b1.max(lo), hi)] {
        if s1 - s0 > 1e-6 {
            g.strokes.push(vec![at(s0), at(s1)]);
        }
    }
    if inside {
        g.arrow(d1, mul(dir, -1.0), sz.arrow);
        g.arrow(d2, dir, sz.arrow);
    } else {
        g.arrow(d1, dir, sz.arrow);
        g.arrow(d2, mul(dir, -1.0), sz.arrow);
    }
}

#[allow(clippy::too_many_arguments)]
fn angle_graphics(g: &mut AnnGraphics, sz: &Sizes, x: P2, ua: P2, ub: P2, t: P2, half: P2, sa: (P2, P2), sb: (P2, P2)) {
    let a0 = ua[1].atan2(ua[0]);
    let mut sweep = ub[1].atan2(ub[0]) - a0;
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    while sweep < -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    // The arc runs through the text, broken around it; when it is too short to hold the text
    // and both arrowheads, it runs just inside the text instead (the text sits outside it).
    let d = dist(t, x);
    let text_along = 2.0 * half[0].max(half[1]);
    let rho = if d * sweep.abs() < text_along + 2.0 * sz.arrow {
        (d - label_extent(half, unit(sub(t, x)), 0.8)).max(sz.arrow * 2.0)
    } else {
        d.max(sz.arrow * 2.0)
    };
    let n = 48;
    let pts: Vec<P2> = (0..=n)
        .map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            add(x, [rho * a.cos(), rho * a.sin()])
        })
        .collect();
    // Break the arc around the label.
    let (lo, hi) = ([t[0] - half[0] - 0.6, t[1] - half[1] - 0.6], [t[0] + half[0] + 0.6, t[1] + half[1] + 0.6]);
    let inside = |p: &P2| p[0] > lo[0] && p[0] < hi[0] && p[1] > lo[1] && p[1] < hi[1];
    let mut cur: Vec<P2> = Vec::new();
    for p in &pts {
        if inside(p) {
            if cur.len() > 1 {
                g.strokes.push(std::mem::take(&mut cur));
            }
            cur.clear();
        } else {
            cur.push(*p);
        }
    }
    if cur.len() > 1 {
        g.strokes.push(cur);
    }
    // Arrowheads at the arc's ends, along the arc.
    let e0 = pts[0];
    let e1 = pts[n];
    let tan0 = mul(perp(ua), -sweep.signum());
    let tan1 = mul(perp(ub), sweep.signum());
    g.arrow(e0, tan0, sz.arrow);
    g.arrow(e1, tan1, sz.arrow);
    // Extension lines out along each line where the arc lies beyond it.
    for ((s0, s1), ray, end) in [(sa, ua, e0), (sb, ub, e1)] {
        let p0 = dot(sub(s0, x), ray);
        let p1 = dot(sub(s1, x), ray);
        let far = p0.max(p1);
        let near = p0.min(p1);
        if rho > far + 1e-6 {
            extension(g, sz, add(x, mul(ray, far.max(0.0))), end);
        } else if rho < near - 1e-6 {
            extension(g, sz, add(x, mul(ray, near)), end);
        }
    }
    g.grips.push((e0, GripKind::Attach(0)));
    g.grips.push((e1, GripKind::Attach(1)));
}

fn intersect((a0, a1): (P2, P2), (b0, b1): (P2, P2)) -> Option<P2> {
    let da = sub(a1, a0);
    let db = sub(b1, b0);
    let den = cross(da, db);
    if den.abs() < 1e-12 * len(da).max(1.0) * len(db).max(1.0) {
        return None;
    }
    let s = cross(sub(b0, a0), db) / den;
    Some(add(a0, mul(da, s)))
}

/// A centerline's two ends (view 2D) before its extension.
pub fn centerline_ends(view: &View, m: &dyn ViewModel, cl: &Centerline) -> Option<(P2, P2)> {
    match &cl.kind {
        CenterlineKind::Points { a, b } => {
            let (pa, _) = resolve_point(view, m, a)?;
            let (pb, _) = resolve_point(view, m, b)?;
            (dist(pa, pb) > 1e-9).then_some((pa, pb))
        }
        CenterlineKind::Lines { a, b } => {
            let (a0, a1) = line_of(resolve(view, m, a).shape)?;
            let (b0, b1) = line_of(resolve(view, m, b).shape)?;
            let da = unit(sub(a1, a0));
            let db0 = unit(sub(b1, b0));
            let db = if dot(da, db0) < 0.0 { mul(db0, -1.0) } else { db0 };
            let t = unit(add(da, db));
            // The midline: halfway between the lines, over both their extents.
            let o = mul(add(a0, foot(a0, b0, b1)), 0.5);
            let ts = [a0, a1, b0, b1].map(|p| dot(sub(p, o), t));
            let lo = ts.iter().copied().fold(f64::MAX, f64::min);
            let hi = ts.iter().copied().fold(f64::MIN, f64::max);
            Some((add(o, mul(t, lo)), add(o, mul(t, hi))))
        }
    }
}

fn line_of(s: Shape) -> Option<(P2, P2)> {
    match s {
        Shape::Line { a, b } | Shape::Curve { a, b } => Some((a, b)),
        _ => None,
    }
}

/// A circle centerline's centre and radius (view 2D).
pub fn circle_centerline(view: &View, m: &dyn ViewModel, cc: &CircleCenterline) -> Option<(P2, f64)> {
    match cc {
        CircleCenterline::ThreePoints(p) => {
            let a = resolve_point(view, m, &p[0])?.0;
            let b = resolve_point(view, m, &p[1])?.0;
            let c = resolve_point(view, m, &p[2])?.0;
            circumcircle(a, b, c)
        }
        CircleCenterline::CenterPoint { center, on } => {
            let c = resolve_point(view, m, center)?.0;
            let p = resolve_point(view, m, on)?.0;
            let r = dist(c, p);
            (r > 1e-9).then_some((c, r))
        }
    }
}

/// The circle through three points.
pub fn circumcircle(a: P2, b: P2, c: P2) -> Option<(P2, f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let sq = |p: P2| p[0] * p[0] + p[1] * p[1];
    let ux = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let uy = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    let center = [ux, uy];
    Some((center, dist(center, a)))
}

// ---------------------------------------------------------------------------------------------
// Proposing dimensions (the tools)

/// The dimension tools (D6.2, D6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DimTool {
    /// D: the type from the picks.
    Smart,
    /// Shift+R.
    Radial,
    /// Shift+D.
    Diameter,
    PointToPoint,
    LineToLine,
    Angular,
    /// Baseline, ordinate, chamfer and arc length (P3C.8): made by the app's own flows.
    Baseline,
    Ordinate,
    Chamfer,
    ArcLength,
}

/// What a pick is, for the tools.
fn pick_class(view: &View, m: &dyn ViewModel, p: &Pick) -> Option<char> {
    Some(match p {
        Pick::Point(_) => 'p',
        Pick::Edge(e) => match resolve(view, m, e).shape {
            Shape::Point(_) => 'p',
            Shape::Line { .. } | Shape::Curve { .. } => 'l',
            Shape::Circle { arc: None, .. } => 'c',
            Shape::Circle { arc: Some(_), .. } => 'a',
        },
    })
}

/// Whether `tool` accepts a pick of this kind as pick number `n` (0-based).
pub fn accepts(tool: DimTool, view: &View, m: &dyn ViewModel, picks: &[Pick], p: &Pick) -> bool {
    let Some(c) = pick_class(view, m, p) else {
        return false;
    };
    let n = picks.len();
    match tool {
        DimTool::Radial | DimTool::Diameter => n == 0 && (c == 'c' || c == 'a'),
        DimTool::PointToPoint => n < 2 && (c == 'p' || c == 'c' || c == 'a'),
        DimTool::LineToLine | DimTool::Angular => n < 2 && c == 'l',
        DimTool::Smart => n < 2,
        DimTool::Baseline | DimTool::Ordinate | DimTool::Chamfer | DimTool::ArcLength => false,
    }
}

/// The dimension the picks make with the text at `cursor` (view 2D), if they make one.
pub fn propose(tool: DimTool, view: &View, m: &dyn ViewModel, picks: &[Pick], cursor: P2) -> Option<Dimension> {
    let classes: Vec<char> = picks.iter().map(|p| pick_class(view, m, p)).collect::<Option<_>>()?;
    let dim = |kind: DimKind| {
        Some(Dimension {
            kind,
            text: cursor,
            format: DimFormat::default(),
            last: None,
        })
    };
    let point_of = |p: &Pick| -> Pick {
        match p {
            Pick::Edge(e) if matches!(resolve(view, m, e).shape, Shape::Circle { .. }) => Pick::Point(PointRef {
                edge: *e,
                of: PointOf::Center,
                hint: cursor,
            }),
            other => *other,
        }
    };
    let oriented = |a: Pick, b: Pick| -> Option<Dimension> {
        let pa = match pick_geo(view, m, &a)?.0 {
            Geo::Point(p) => p,
            _ => return None,
        };
        let pb = match pick_geo(view, m, &b)?.0 {
            Geo::Point(p) => p,
            _ => return None,
        };
        let (sx, sy) = sheet_axes(view);
        // In sheet axes: horizontal when the text is above or below, vertical beside.
        let q = |p: P2| [dot(p, sx), dot(p, sy)];
        let (qa, qb, qc) = (q(pa), q(pb), q(cursor));
        let (lo, hi) = ([qa[0].min(qb[0]), qa[1].min(qb[1])], [qa[0].max(qb[0]), qa[1].max(qb[1])]);
        let in_x = qc[0] >= lo[0] && qc[0] <= hi[0];
        let in_y = qc[1] >= lo[1] && qc[1] <= hi[1];
        let tiny = 1e-6 * dist(pa, pb).max(1.0);
        // Points on a vertical (horizontal) line measure vertically (horizontally) wherever the
        // text is.
        let (dx, dy) = ((qb[0] - qa[0]).abs(), (qb[1] - qa[1]).abs());
        let orient = if (in_x && !in_y && dx > tiny) || dy <= tiny {
            Orient::Horizontal
        } else if (in_y && !in_x) || dx <= tiny {
            Orient::Vertical
        } else {
            Orient::Aligned
        };
        dim(DimKind::Distance { a, b, orient })
    };
    match (tool, classes.as_slice(), picks) {
        (DimTool::Diameter, ['c' | 'a'], [p]) => dim(DimKind::Diameter(*p.edge())),
        (DimTool::Radial, ['c' | 'a'], [p]) => dim(DimKind::Radius(*p.edge())),
        (DimTool::Smart, ['c'], [p]) => dim(DimKind::Diameter(*p.edge())),
        (DimTool::Smart, ['a'], [p]) => dim(DimKind::Radius(*p.edge())),
        (DimTool::Smart | DimTool::PointToPoint, ['l'], [Pick::Edge(e)]) if tool == DimTool::Smart => {
            let (a, b) = line_of(resolve(view, m, e).shape)?;
            oriented(
                Pick::Point(PointRef { edge: *e, of: PointOf::End, hint: a }),
                Pick::Point(PointRef { edge: *e, of: PointOf::End, hint: b }),
            )
        }
        (DimTool::Angular, ['l', 'l'], [Pick::Edge(a), Pick::Edge(b)]) => {
            angle_rays(view, m, a, b, cursor)?;
            dim(DimKind::Angle { a: *a, b: *b })
        }
        (DimTool::LineToLine, ['l', 'l'], [a, b]) => {
            let d = Dimension { kind: DimKind::Distance { a: *a, b: *b, orient: Orient::Aligned }, text: cursor, format: DimFormat::default(), last: None };
            measure(view, m, &d).map(|_| d)
        }
        (DimTool::Smart, ['l', 'l'], [Pick::Edge(a), Pick::Edge(b)]) => {
            let d = Dimension { kind: DimKind::Distance { a: picks[0], b: picks[1], orient: Orient::Aligned }, text: cursor, format: DimFormat::default(), last: None };
            if measure(view, m, &d).is_some() {
                Some(d)
            } else {
                angle_rays(view, m, a, b, cursor)?;
                dim(DimKind::Angle { a: *a, b: *b })
            }
        }
        (DimTool::Smart | DimTool::PointToPoint, [_, _], [a, b]) => {
            let (a, b) = (point_of(a), point_of(b));
            match (pick_class(view, m, &a)?, pick_class(view, m, &b)?) {
                ('p', 'p') => oriented(a, b),
                _ if tool == DimTool::Smart => dim(DimKind::Distance { a, b, orient: Orient::Aligned }),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standard::Scale;
    use crate::view::NamedView;
    use crate::ObjectRef;
    use cadrs_kernel::naming::{FaceOrigin, OpId};
    use cadrs_kernel::{ProjClass, ProjSource, ProjVisibility};

    fn face(op: u128, k: u64) -> FaceName {
        FaceName { op: OpId::from_u128(op), origin: FaceOrigin::Unnamed { index: k as u32 }, split: 0 }
    }

    fn edge_name(k: u64) -> EdgeName {
        EdgeName { faces: [face(1, k), face(1, k + 100)], index: 0 }
    }

    fn line_edge(k: u64, a: P2, b: P2) -> ProjEdge {
        ProjEdge {
            visibility: ProjVisibility::Visible,
            class: ProjClass::Sharp,
            curve: ProjCurve::Line { start: a.into(), end: b.into() },
            points: vec![a.into(), b.into()],
            source: Some(ProjSource { edge_name: Some(edge_name(k)), ..Default::default() }),
        }
    }

    fn front(scale: Scale) -> View {
        View::base(ObjectRef { element: Uuid::nil(), part: None }, NamedView::Front, scale, [100.0, 80.0])
    }

    /// A 40 × 30 rectangle's front view (x 0..40, z 0..30) with a circle Ø10 at (20, 15) seen
    /// face-on — exact model edges for all of them, and 2D lines a little off (to tell which
    /// was used).
    fn model() -> ModelData {
        let mut m = ModelData::default();
        let off = 0.01;
        let lines = [
            (1, [0.0, 0.0], [40.0, 0.0]),
            (2, [40.0, 0.0], [40.0, 30.0]),
            (3, [40.0, 30.0], [0.0, 30.0]),
            (4, [0.0, 30.0], [0.0, 0.0]),
        ];
        for (k, a, b) in lines {
            m.projection.edges.push(line_edge(k, add(a, [off, off]), add(b, [off, off])));
            m.edges.insert(edge_name(k), ModelEdge::Line { a: [a[0], -5.0, a[1]], b: [b[0], -5.0, b[1]] });
        }
        let pts: Vec<[f64; 3]> = (0..=16)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 16.0;
                [20.0 + 5.0 * t.cos(), -5.0, 15.0 + 5.0 * t.sin()]
            })
            .collect();
        m.projection.edges.push(ProjEdge {
            visibility: ProjVisibility::Visible,
            class: ProjClass::Sharp,
            curve: ProjCurve::Arc {
                center: [20.0, 15.0].into(),
                radius: 5.02,
                start: [25.0, 15.0].into(),
                mid: [15.0, 15.0].into(),
                end: [25.0, 15.0].into(),
                full: true,
            },
            points: vec![[25.0, 15.0].into(), [15.0, 15.0].into()],
            source: Some(ProjSource { edge_name: Some(edge_name(9)), ..Default::default() }),
        });
        m.edges.insert(
            edge_name(9),
            ModelEdge::Circle { center: [20.0, -5.0, 15.0], normal: [0.0, 1.0, 0.0], radius: 5.0, points: pts },
        );
        m
    }

    fn r(m: &ModelData, i: usize) -> EdgeRef {
        EdgeRef::of(&m.projection.edges[i])
    }

    #[test]
    fn dimensions_read_the_model_not_the_picture() {
        let m = model();
        let v = front(Scale::new(1, 2));
        // Ø10 from the model circle (the 2D arc says 10.04).
        let d = propose(DimTool::Smart, &v, &m, &[Pick::Edge(r(&m, 4))], [30.0, 25.0]).unwrap();
        assert!(matches!(d.kind, DimKind::Diameter(_)));
        let x = measure(&v, &m, &d).unwrap();
        assert_eq!(x.value, 10.0);
        assert!(x.from_model);
        // The two vertical sides: 40 apart (line to line).
        let d = propose(DimTool::Smart, &v, &m, &[Pick::Edge(r(&m, 1)), Pick::Edge(r(&m, 3))], [20.0, 40.0]).unwrap();
        assert_eq!(measure(&v, &m, &d).unwrap().value, 40.0);
        // The bottom and the right side: an angle (90°).
        let d = propose(DimTool::Smart, &v, &m, &[Pick::Edge(r(&m, 0)), Pick::Edge(r(&m, 1))], [35.0, 5.0]).unwrap();
        assert!(matches!(d.kind, DimKind::Angle { .. }));
        assert!((measure(&v, &m, &d).unwrap().value - 90.0).abs() < 1e-9);
        // The circle's centre to the top-right corner: vertical beside them, horizontal above.
        let centre = Pick::Point(PointRef { edge: r(&m, 4), of: PointOf::Center, hint: [20.0, 15.0] });
        let corner = Pick::Point(PointRef { edge: r(&m, 2), of: PointOf::End, hint: [40.0, 30.0] });
        let d = propose(DimTool::Smart, &v, &m, &[centre, corner], [50.0, 22.0]).unwrap();
        assert!(matches!(d.kind, DimKind::Distance { orient: Orient::Vertical, .. }));
        assert_eq!(measure(&v, &m, &d).unwrap().value, 15.0);
        let d = propose(DimTool::Smart, &v, &m, &[centre, corner], [30.0, 40.0]).unwrap();
        assert_eq!(measure(&v, &m, &d).unwrap().value, 20.0);
        // Without model edges, the 2D projection is used.
        let mut m2 = m.clone();
        m2.edges.clear();
        let d = propose(DimTool::Diameter, &v, &m2, &[Pick::Edge(r(&m2, 4))], [30.0, 25.0]).unwrap();
        let x = measure(&v, &m2, &d).unwrap();
        assert!((x.value - 10.04).abs() < 1e-12 && !x.from_model);
    }

    #[test]
    fn angles_follow_the_texts_quadrant() {
        let mut m = ModelData::default();
        // Two lines from the origin: along x, and at 43° above it.
        let a = 43f64.to_radians();
        m.projection.edges.push(line_edge(1, [0.0, 0.0], [10.0, 0.0]));
        m.projection.edges.push(line_edge(2, [0.0, 0.0], [10.0 * a.cos(), 10.0 * a.sin()]));
        let v = front(Scale::new(1, 1));
        let (e1, e2) = (EdgeRef::of(&m.projection.edges[0]), EdgeRef::of(&m.projection.edges[1]));
        let at = |t: P2| measure(&v, &m, &Dimension { kind: DimKind::Angle { a: e1, b: e2 }, text: t, format: DimFormat::default(), last: None }).unwrap().value;
        assert!((at([5.0, 1.0]) - 43.0).abs() < 1e-9);
        assert!((at([-5.0, 1.0]) - 137.0).abs() < 1e-9);
        assert!((at([-5.0, -1.0]) - 43.0).abs() < 1e-9);
    }

    #[test]
    fn text_follows_units_and_the_palette() {
        let inch = DrawingStyle::for_units(crate::DrawingUnits::Inch, crate::Standard::Ansi);
        let f = DimFormat::default();
        let t = |kind, v, f: &DimFormat| dimension_text(&inch, kind, v, f).plain();
        assert_eq!(t(ValueKind::Diameter, 4.75 * 25.4, &f), "Ø4.750");
        assert_eq!(t(ValueKind::Length, 3.282 * 25.4, &f), "3.282");
        assert_eq!(t(ValueKind::Angle, 43.0, &f), "43.0°");
        assert_eq!(t(ValueKind::Length, 0.266 * 25.4, &f), ".266");
        // ±.005 and dual millimetres (D6.6).
        let p = DimFormat { tolerance: Tolerance::Symmetric(0.005 * 25.4), dual: Some(true), ..DimFormat::default() };
        assert_eq!(t(ValueKind::Length, 3.282 * 25.4, &p), "[83.36] 3.282±.005");
        let p = DimFormat {
            prefix: "2x ".into(),
            tolerance: Tolerance::Deviation { upper: 0.005 * 25.4, lower: -0.002 * 25.4 },
            precision: Some(2),
            ..DimFormat::default()
        };
        assert_eq!(t(ValueKind::Length, 3.282 * 25.4, &p), "2x 3.28+.005/-.002");
        let p = DimFormat { tolerance: Tolerance::Limits { upper: 0.01 * 25.4, lower: -0.01 * 25.4 }, ..DimFormat::default() };
        assert_eq!(t(ValueKind::Length, 1.0 * 25.4, &p), "1.010 .990");
        // Symbols are runs of their own (drawn as vectors).
        assert_eq!(runs("⌴Ø.438 ↧.250"), vec![Run::Sym(Symbol::Counterbore), Run::Text("Ø.438 ".into()), Run::Sym(Symbol::Depth), Run::Text(".250".into())]);
    }

    #[test]
    fn hole_callouts_come_from_the_spec() {
        let inch = DrawingStyle::for_units(crate::DrawingUnits::Inch, crate::Standard::Ansi);
        let cb = HoleInfo {
            diameter: 0.266 * 25.4,
            end: HoleEnd::Through,
            depth: 0.0,
            cbore: Some((0.438 * 25.4, 0.25 * 25.4)),
            csink: None,
            thread: None,
        };
        assert_eq!(hole_text(&inch, &cb, "4x").plain(), "4x Ø.266 THRU ⌴Ø.438 ↧.250");
        let plain = HoleInfo { cbore: None, ..cb.clone() };
        assert_eq!(hole_text(&inch, &plain, "8x").plain(), "8x Ø.266 THRU");
        assert_eq!(hole_text(&inch, &plain, "").plain(), "Ø.266 THRU");
    }

    #[test]
    fn graphics_have_arrows_text_and_grips() {
        let m = model();
        let v = front(Scale::new(1, 2));
        let style = DrawingStyle::default();
        let d = propose(DimTool::Smart, &v, &m, &[Pick::Edge(r(&m, 1)), Pick::Edge(r(&m, 3))], [20.0, 40.0]).unwrap();
        let a = Annotation::new(AnnotationKind::Dimension(d));
        let g = annotation_graphics(&style, &v, &m, &a).unwrap();
        assert_eq!(g.fills.len(), 2);
        assert_eq!(g.texts.len(), 1);
        assert_eq!(g.texts[0].text, "40.00");
        assert!(g.grips.iter().any(|(_, k)| *k == GripKind::Text));
        // Extension lines up from the sides' tops to the dimension line, which sits at z 40
        // (sheet y 80 + 20).
        assert!(g.strokes.iter().any(|s| s.iter().any(|p| (p[1] - (100.0 + style.extension_beyond)).abs() < 1e-9)));
        // A centermark: a cross and four dashes.
        let cm = Annotation::new(AnnotationKind::Centermark(r(&m, 4)));
        assert_eq!(annotation_graphics(&style, &v, &m, &cm).unwrap().strokes.len(), 6);
        // A centerline between the sides: vertical at x 20, past both ends.
        let cl = Centerline { kind: CenterlineKind::Lines { a: r(&m, 1), b: r(&m, 3) }, extend: [0.0, 5.0] };
        let (p0, p1) = centerline_ends(&v, &m, &cl).unwrap();
        assert!((p0[0] - 20.0).abs() < 1e-9 && (p1[0] - 20.0).abs() < 1e-9);
        assert!((p0[1] - p1[1]).abs() > 29.9);
    }

    #[test]
    fn circle_through_three_points() {
        let (c, r) = circumcircle([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]).unwrap();
        assert!(dist(c, [0.0, 0.0]) < 1e-12 && (r - 1.0).abs() < 1e-12);
    }
}
