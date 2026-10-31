//! Onshape's surfacing features (`reference/onshape/surfacing.md`): **Thicken** (surfaces and
//! faces made solid with a thickness), **Helix** (a helical curve, usable as a sweep path) and
//! **Fill** (a surface bounded by edges or curves). Their rebuild is in
//! `rebuild/kernel_ops/surfacing.rs`.

use cadrs_sketch::CurveId;
use serde::{Deserialize, Serialize};

use crate::document::{AxisRef, BooleanOp, EdgeRef, FaceRef, RegionRef};
use crate::ids::{FeatureId, PartId};

// ---------------------------------------------------------------------------------------------
// Thicken

/// A Thicken: surfaces, part faces and sketch regions made solid (Onshape's dialog: New / Add /
/// Remove / Intersect, the selections, Mid plane or Thickness 1 and 2 with an opposite
/// direction arrow, Keep tools, Merge scope).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThickenFeature {
    /// Part faces (of solids or surfaces).
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    /// Whole surface parts.
    #[serde(default)]
    pub parts: Vec<PartId>,
    /// Sketch regions...
    #[serde(default)]
    pub regions: Vec<RegionRef>,
    /// ...and whole sketches (their regions).
    #[serde(default)]
    pub sketches: Vec<FeatureId>,
    /// Thickness 1 split evenly across the surface.
    #[serde(default)]
    pub mid_plane: bool,
    /// mm along the surface's normal (the other way when `flip`).
    pub thickness1: f64,
    #[serde(default)]
    pub thickness1_expr: String,
    /// mm on the other side.
    #[serde(default)]
    pub thickness2: f64,
    #[serde(default)]
    pub thickness2_expr: String,
    /// The opposite direction arrow.
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub op: BooleanOp,
    #[serde(default)]
    pub merge_all: bool,
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
    /// Keep the surfaces it thickened (else they are consumed, as in Onshape).
    #[serde(default)]
    pub keep_tools: bool,
}

impl Default for ThickenFeature {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            parts: Vec::new(),
            regions: Vec::new(),
            sketches: Vec::new(),
            mid_plane: false,
            thickness1: 5.0,
            thickness1_expr: "5 mm".into(),
            thickness2: 0.0,
            thickness2_expr: "0 mm".into(),
            flip: false,
            op: BooleanOp::New,
            merge_all: false,
            merge_scope: Vec::new(),
            keep_tools: false,
        }
    }
}

impl ThickenFeature {
    /// How far the solid reaches along the surface's normal and against it (mm).
    pub fn sides(&self) -> (f64, f64) {
        if self.mid_plane {
            (self.thickness1 / 2.0, self.thickness1 / 2.0)
        } else if self.flip {
            (self.thickness2, self.thickness1)
        } else {
            (self.thickness1, self.thickness2)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
            && self.parts.is_empty()
            && self.regions.is_empty()
            && self.sketches.is_empty()
    }

    /// The sketches it thickens regions of (or whole).
    pub fn sketch_ids(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self.regions.iter().map(|r| r.sketch).collect();
        for s in &self.sketches {
            if !v.contains(s) {
                v.push(*s);
            }
        }
        v.dedup();
        v
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.is_empty() {
            return Some("Select surfaces or faces to thicken");
        }
        let (a, b) = self.sides();
        if !(a >= 0.0 && b >= 0.0 && a + b > 1e-6) {
            return Some("The thickness must be greater than zero");
        }
        None
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out = self.sketch_ids();
        out.extend(self.faces.iter().map(|f| FeatureId(f.face.op)));
        out.extend(
            self.parts
                .iter()
                .chain(&self.merge_scope)
                .map(|p| p.feature),
        );
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Face blend

/// How a Face blend's ball is sized (Onshape's blend control).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendControl {
    /// A constant radius.
    #[default]
    Radius,
    /// A constant width: the distance between the blend's edges on the two walls.
    Width,
}

/// A Face blend (Onshape's, rolling ball, walls trimmed): a blend between faces of two surfaces
/// (or one) that meet along an edge; the walls are trimmed to it and joined with it into one
/// surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceBlendFeature {
    /// The first wall's faces.
    pub side1: Vec<FaceRef>,
    /// The second wall's faces.
    pub side2: Vec<FaceRef>,
    #[serde(default)]
    pub control: BlendControl,
    /// mm.
    pub radius: f64,
    #[serde(default)]
    pub radius_expr: String,
    /// mm.
    pub width: f64,
    #[serde(default)]
    pub width_expr: String,
}

impl Default for FaceBlendFeature {
    fn default() -> Self {
        Self {
            side1: Vec::new(),
            side2: Vec::new(),
            control: BlendControl::Radius,
            radius: 5.0,
            radius_expr: "5 mm".into(),
            width: 10.0,
            width_expr: "10 mm".into(),
        }
    }
}

impl FaceBlendFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.side1.is_empty() || self.side2.is_empty() {
            return Some("Select a face on each side");
        }
        let size = match self.control {
            BlendControl::Radius => self.radius,
            BlendControl::Width => self.width,
        };
        if !(size > 0.0 && size.is_finite()) {
            return Some("The blend's size must be greater than zero");
        }
        None
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        self.side1
            .iter()
            .chain(&self.side2)
            .map(|f| FeatureId(f.face.op))
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Projected curve

/// A Projected curve (Onshape's, Curve to face type): curves projected along a direction onto
/// faces. A curve feature, as a Helix: a sweep path, a loft guide, a face split's tool.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectCurveFeature {
    /// The curves to project (sketch curves, part edges).
    pub curves: Vec<crate::advanced::PathRef>,
    /// The faces they land on.
    pub faces: Vec<FaceRef>,
    /// The way they go: a picked direction, else the normal of the first curve's sketch.
    #[serde(default)]
    pub direction: Option<crate::document::DirectionRef>,
    /// Against it.
    #[serde(default)]
    pub flip: bool,
}

impl ProjectCurveFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.curves.is_empty() {
            return Some("Select curves to project");
        }
        if self.faces.is_empty() {
            return Some("Select faces to project onto");
        }
        None
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out: Vec<FeatureId> = Vec::new();
        for c in &self.curves {
            match c {
                crate::advanced::PathRef::Edge(e) => {
                    out.extend([FeatureId(e.edge.faces[0].op), FeatureId(e.edge.faces[1].op)])
                }
                crate::advanced::PathRef::Curve(f) => out.push(*f),
                c => out.extend(c.sketch()),
            }
        }
        out.extend(self.faces.iter().map(|f| FeatureId(f.face.op)));
        out.extend(
            self.direction
                .as_ref()
                .and_then(crate::document::direction_parent),
        );
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Helix

/// What a helix is built on (the dialog's type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HelixType {
    /// A cylindrical or conical face: its axis, radius (radii) and height.
    #[default]
    CylinderCone,
    /// An axis, with a radius and a height.
    Axis,
    /// A circle (a sketch circle or arc, a circular edge): its axis and radius, with a height.
    Circle,
}

impl HelixType {
    pub const ALL: [HelixType; 3] = [HelixType::CylinderCone, HelixType::Axis, HelixType::Circle];

    pub fn label(self) -> &'static str {
        match self {
            HelixType::CylinderCone => "Cylinder/Cone",
            HelixType::Axis => "Axis",
            HelixType::Circle => "Circle",
        }
    }
}

/// How the helix's length is given (the dialog's input type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HelixPath {
    /// Revolutions over the height.
    #[default]
    Turns,
    /// Pitch over the height.
    Pitch,
    /// Revolutions and pitch (the height follows).
    TurnsAndPitch,
}

impl HelixPath {
    pub const ALL: [HelixPath; 3] = [HelixPath::Turns, HelixPath::Pitch, HelixPath::TurnsAndPitch];

    pub fn label(self) -> &'static str {
        match self {
            HelixPath::Turns => "Turns",
            HelixPath::Pitch => "Pitch",
            HelixPath::TurnsAndPitch => "Turns and pitch",
        }
    }
}

/// A Helix: a helical curve, shown in the Part Studio and usable as a sweep path
/// ([`crate::advanced::PathRef::Curve`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelixFeature {
    #[serde(default)]
    pub helix_type: HelixType,
    /// The cylindrical or conical face (Cylinder/Cone).
    #[serde(default)]
    pub face: Option<FaceRef>,
    /// The axis (Axis) or the circle (Circle).
    #[serde(default)]
    pub axis: Option<AxisRef>,
    #[serde(default)]
    pub path: HelixPath,
    pub revolutions: f64,
    #[serde(default)]
    pub revolutions_expr: String,
    pub pitch: f64,
    #[serde(default)]
    pub pitch_expr: String,
    /// The height for Axis and Circle (a face gives its own).
    pub height: f64,
    #[serde(default)]
    pub height_expr: String,
    /// The radius for Axis.
    pub radius: f64,
    #[serde(default)]
    pub radius_expr: String,
    /// Where it starts round the axis (degrees from the reference direction).
    #[serde(default)]
    pub start_angle: f64,
    #[serde(default)]
    pub start_angle_expr: String,
    /// Turns clockwise seen from its start looking along the axis (a right-handed helix).
    #[serde(default)]
    pub clockwise: bool,
    /// The opposite direction arrow: starts at the other end.
    #[serde(default)]
    pub flip: bool,
}

impl Default for HelixFeature {
    fn default() -> Self {
        Self {
            helix_type: HelixType::CylinderCone,
            face: None,
            axis: None,
            path: HelixPath::Turns,
            revolutions: 4.0,
            revolutions_expr: "4".into(),
            pitch: 25.0,
            pitch_expr: "25 mm".into(),
            height: 25.0,
            height_expr: "25 mm".into(),
            radius: 25.0,
            radius_expr: "25 mm".into(),
            start_angle: 0.0,
            start_angle_expr: "0 deg".into(),
            clockwise: true,
            flip: false,
        }
    }
}

impl HelixFeature {
    pub fn problem(&self) -> Option<&'static str> {
        match self.helix_type {
            HelixType::CylinderCone if self.face.is_none() => {
                return Some("Select a cylindrical or conical face");
            }
            HelixType::Axis if self.axis.is_none() => return Some("Select an axis"),
            HelixType::Circle if self.axis.is_none() => return Some("Select a circle"),
            _ => {}
        }
        let turns_needed = matches!(self.path, HelixPath::Turns | HelixPath::TurnsAndPitch);
        let pitch_needed = matches!(self.path, HelixPath::Pitch | HelixPath::TurnsAndPitch);
        if turns_needed && self.revolutions.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Some("The number of revolutions must be greater than zero");
        }
        if pitch_needed && self.pitch.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Some("The pitch must be greater than zero");
        }
        if self.helix_type == HelixType::Axis
            && self.radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        {
            return Some("The radius must be greater than zero");
        }
        None
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out = Vec::new();
        if let Some(f) = &self.face {
            out.push(FeatureId(f.face.op));
        }
        match &self.axis {
            Some(AxisRef::SketchCurve { sketch, .. }) => out.push(*sketch),
            Some(AxisRef::Edge(e)) => {
                out.extend([FeatureId(e.edge.faces[0].op), FeatureId(e.edge.faces[1].op)])
            }
            Some(AxisRef::Face(f)) => out.push(FeatureId(f.face.op)),
            Some(AxisRef::Connector(c)) => out.extend(c.parent()),
            None => {}
        }
        out
    }

    /// The turns and the height, from the input type: `face_height` is the face's (or the
    /// dialog's) height.
    pub fn turns_and_height(&self, face_height: f64) -> (f64, f64) {
        match self.path {
            HelixPath::Turns => (self.revolutions, face_height),
            HelixPath::Pitch => (face_height / self.pitch.max(1e-9), face_height),
            HelixPath::TurnsAndPitch => (self.revolutions, self.revolutions * self.pitch),
        }
    }
}

/// A curve feature's curve, where it built: a Helix's, or B-spline pieces (a Projected curve's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CurveGeom {
    Helix(HelixGeom),
    Bspline(Vec<cadrs_kernel::Bspline3>),
}

impl CurveGeom {
    /// As the kernel's path curves (a Helix's in rational Bézier pieces).
    pub fn path(&self) -> Vec<cadrs_kernel::PathCurve> {
        match self {
            CurveGeom::Helix(h) => h
                .beziers()
                .into_iter()
                .map(|(poles, weights)| cadrs_kernel::PathCurve::Bezier3 {
                    poles: poles.iter().map(|p| nalgebra::Point3::from(*p)).collect(),
                    weights,
                })
                .collect(),
            CurveGeom::Bspline(pieces) => pieces
                .iter()
                .cloned()
                .map(cadrs_kernel::PathCurve::Bspline)
                .collect(),
        }
    }

    /// Points along each piece, for display.
    pub fn polylines(&self) -> Vec<Vec<[f64; 3]>> {
        match self {
            CurveGeom::Helix(h) => vec![h.polyline()],
            CurveGeom::Bspline(pieces) => pieces
                .iter()
                .map(|b| {
                    let (lo, hi) = (b.knots[b.degree], b.knots[b.knots.len() - b.degree - 1]);
                    let n = 8 * b.poles.len().max(8);
                    (0..=n)
                        .map(|i| bspline_point(b, lo + (hi - lo) * i as f64 / n as f64))
                        .collect()
                })
                .collect(),
        }
    }

    /// Points along it, its pieces one after another.
    pub fn polyline(&self) -> Vec<[f64; 3]> {
        self.polylines().into_iter().flatten().collect()
    }
}

/// A point of a B-spline (de Boor's algorithm).
fn bspline_point(b: &cadrs_kernel::Bspline3, t: f64) -> [f64; 3] {
    let p = b.degree;
    let n = b.poles.len();
    let k = &b.knots;
    let t = t.clamp(k[p], k[n]);
    let mut span = p;
    while span + 1 < n && k[span + 1] <= t {
        span += 1;
    }
    let w = |i: usize| b.weights.get(i).copied().unwrap_or(1.0);
    let mut d: Vec<[f64; 4]> = (0..=p)
        .map(|j| {
            let i = span - p + j;
            let q = b.poles[i];
            [q.x * w(i), q.y * w(i), q.z * w(i), w(i)]
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = span - p + j;
            let den = k[i + p + 1 - r] - k[i];
            let a = if den.abs() > 0.0 {
                (t - k[i]) / den
            } else {
                0.0
            };
            for c in 0..4 {
                d[j][c] = (1.0 - a) * d[j - 1][c] + a * d[j][c];
            }
        }
    }
    let q = d[p];
    [q[0] / q[3], q[1] / q[3], q[2] / q[3]]
}

/// A helix in space: it starts at `origin + r0·x` (turned `start_angle` about the axis) and
/// runs `turns` times round `axis` over `height`, its radius going from `r0` to `r1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HelixGeom {
    /// On the axis, level with the start.
    pub origin: [f64; 3],
    /// Unit, the way it rises.
    pub axis: [f64; 3],
    /// Unit, square to the axis: where angle 0 is.
    pub x: [f64; 3],
    pub r0: f64,
    pub r1: f64,
    pub height: f64,
    pub turns: f64,
    /// Radians.
    pub start_angle: f64,
    pub clockwise: bool,
}

impl HelixGeom {
    fn frame(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let a = self.axis;
        let x = self.x;
        // y = axis × x: counter-clockwise seen looking down the axis from its tip.
        let y = [
            a[1] * x[2] - a[2] * x[1],
            a[2] * x[0] - a[0] * x[2],
            a[0] * x[1] - a[1] * x[0],
        ];
        (a, x, y)
    }

    /// The point at `s` in 0..=1 along it, and its derivative in `s`.
    pub fn at(&self, s: f64) -> ([f64; 3], [f64; 3]) {
        let (p, d, _) = self.at2(s);
        (p, d)
    }

    /// The point at `s`, and its first and second derivatives in `s`.
    pub fn at2(&self, s: f64) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let (a, x, y) = self.frame();
        // A right-handed (clockwise) screw turns counter-clockwise about its own direction:
        // +angle about `axis`.
        let sign = if self.clockwise { 1.0 } else { -1.0 };
        let tau = std::f64::consts::TAU;
        let th = self.start_angle + sign * tau * self.turns * s;
        let dth = sign * tau * self.turns;
        let r = self.r0 + (self.r1 - self.r0) * s;
        let dr = self.r1 - self.r0;
        let h = self.height * s;
        let (c, sn) = (th.cos(), th.sin());
        let radial = [0, 1, 2].map(|i| x[i] * c + y[i] * sn);
        let tangential = [0, 1, 2].map(|i| -x[i] * sn + y[i] * c);
        let p = [0, 1, 2].map(|i| self.origin[i] + a[i] * h + radial[i] * r);
        let d = [0, 1, 2].map(|i| a[i] * self.height + radial[i] * dr + tangential[i] * r * dth);
        let dd = [0, 1, 2].map(|i| tangential[i] * 2.0 * dr * dth - radial[i] * r * dth * dth);
        (p, d, dd)
    }

    /// Bézier pieces (poles and weights) through it, 16 per turn. At a constant radius each is
    /// a rational quadratic on the cylinder exactly (its arc of the circle, the height rising
    /// with it to within about 1e-3 of the pitch), so a sweep along it meets the cylinder it
    /// lies on; on a cone, quintics matching the helix's points and first and second
    /// derivatives at their ends.
    pub fn beziers(&self) -> Vec<(Vec<[f64; 3]>, Vec<f64>)> {
        let n = ((self.turns.abs() * 16.0).ceil() as usize).max(1);
        let (a, x, y) = self.frame();
        let sign = if self.clockwise { 1.0 } else { -1.0 };
        let tau = std::f64::consts::TAU;
        let exact = (self.r1 - self.r0).abs() <= 1e-12 * self.r0.abs().max(1.0);
        (0..n)
            .map(|k| {
                let (s0, s1) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
                if exact {
                    let th = |s: f64| self.start_angle + sign * tau * self.turns * s;
                    let (t0, t1) = (th(s0), th(s1));
                    let half = (t1 - t0) / 2.0;
                    let at = |t: f64, r: f64, z: f64| {
                        [0, 1, 2].map(|i| {
                            self.origin[i] + a[i] * z + (x[i] * t.cos() + y[i] * t.sin()) * r
                        })
                    };
                    let (z0, z1) = (self.height * s0, self.height * s1);
                    let poles = vec![
                        at(t0, self.r0, z0),
                        at(t0 + half, self.r0 / half.cos(), (z0 + z1) / 2.0),
                        at(t1, self.r0, z1),
                    ];
                    return (poles, vec![1.0, half.cos(), 1.0]);
                }
                let h = s1 - s0;
                let (p0, d0, a0) = self.at2(s0);
                let (p5, d5, a5) = self.at2(s1);
                let b = |f: &dyn Fn(usize) -> f64| [0, 1, 2].map(f);
                let poles = vec![
                    p0,
                    b(&|i| p0[i] + d0[i] * h / 5.0),
                    b(&|i| p0[i] + d0[i] * 2.0 * h / 5.0 + a0[i] * h * h / 20.0),
                    b(&|i| p5[i] - d5[i] * 2.0 * h / 5.0 + a5[i] * h * h / 20.0),
                    b(&|i| p5[i] - d5[i] * h / 5.0),
                    p5,
                ];
                (poles, vec![1.0; 6])
            })
            .collect()
    }

    /// Points along it for display (every 5°).
    pub fn polyline(&self) -> Vec<[f64; 3]> {
        let n = ((self.turns.abs() * 72.0).ceil() as usize).max(2);
        (0..=n).map(|k| self.at(k as f64 / n as f64).0).collect()
    }

    /// Its length (numerically).
    pub fn length(&self) -> f64 {
        let pts = {
            let n = ((self.turns.abs() * 720.0).ceil() as usize).max(2);
            (0..=n)
                .map(|k| self.at(k as f64 / n as f64).0)
                .collect::<Vec<_>>()
        };
        pts.windows(2)
            .map(|w| {
                ((w[1][0] - w[0][0]).powi(2)
                    + (w[1][1] - w[0][1]).powi(2)
                    + (w[1][2] - w[0][2]).powi(2))
                .sqrt()
            })
            .sum()
    }
}

// ---------------------------------------------------------------------------------------------
// Fill

/// A boundary curve's continuity with the faces next to it (Onshape's per-edge option; cadrs
/// builds Position only for now).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Continuity {
    #[default]
    Position,
    Tangency,
    Curvature,
}

impl Continuity {
    pub const ALL: [Continuity; 3] = [
        Continuity::Position,
        Continuity::Tangency,
        Continuity::Curvature,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Continuity::Position => "Position (G0)",
            Continuity::Tangency => "Tangency (G1)",
            Continuity::Curvature => "Curvature (G2)",
        }
    }
}

/// One boundary curve of a Fill.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FillEdge {
    Edge(EdgeRef),
    SketchCurve { sketch: FeatureId, curve: CurveId },
}

/// A Fill: a surface through a closed boundary of edges and sketch curves (New, or Add: sewn
/// with the surfaces it meets, which makes a solid when they close).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FillFeature {
    pub edges: Vec<FillEdge>,
    /// The continuity asked for at each edge (parallel to `edges`).
    #[serde(default)]
    pub continuity: Vec<Continuity>,
    /// Add: merged with the surfaces in the merge scope (all it touches when empty).
    #[serde(default)]
    pub add: bool,
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
}

impl FillFeature {
    pub fn problem(&self) -> Option<&'static str> {
        self.edges
            .is_empty()
            .then_some("Select edges or curves to fill")
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out = Vec::new();
        for e in &self.edges {
            match e {
                FillEdge::Edge(r) => {
                    out.extend([FeatureId(r.edge.faces[0].op), FeatureId(r.edge.faces[1].op)])
                }
                FillEdge::SketchCurve { sketch, .. } => out.push(*sketch),
            }
        }
        out.extend(self.merge_scope.iter().map(|p| p.feature));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn helix(turns: f64, clockwise: bool) -> HelixGeom {
        HelixGeom {
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            x: [1.0, 0.0, 0.0],
            r0: 10.0,
            r1: 10.0,
            height: 20.0,
            turns,
            start_angle: 0.0,
            clockwise,
        }
    }

    #[test]
    fn a_helix_has_its_length_and_ends() {
        let h = helix(4.0, true);
        let want = 4.0 * ((std::f64::consts::TAU * 10.0).powi(2) + 5.0f64.powi(2)).sqrt();
        assert!(
            (h.length() - want).abs() / want < 1e-5,
            "{} vs {want}",
            h.length()
        );
        let (p0, _) = h.at(0.0);
        let (p1, _) = h.at(1.0);
        assert!((p0[0] - 10.0).abs() < 1e-12 && p0[2].abs() < 1e-12);
        assert!((p1[0] - 10.0).abs() < 1e-9 && (p1[2] - 20.0).abs() < 1e-12);
        // A right-handed (clockwise) helix turns counter-clockwise about +Z as it rises.
        let (q, _) = h.at(1.0 / 16.0);
        assert!(q[1] > 0.0);
        assert!(helix(4.0, false).at(1.0 / 16.0).0[1] < 0.0);
    }

    /// A rational Bézier's point (homogeneous de Casteljau).
    fn rbez(poles: &[[f64; 3]], w: &[f64], t: f64) -> [f64; 3] {
        let mut p: Vec<[f64; 4]> = poles
            .iter()
            .zip(w)
            .map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w])
            .collect();
        for k in 1..p.len() {
            for i in 0..p.len() - k {
                p[i] = [0, 1, 2, 3].map(|c| p[i][c] + (p[i + 1][c] - p[i][c]) * t);
            }
        }
        [p[0][0] / p[0][3], p[0][1] / p[0][3], p[0][2] / p[0][3]]
    }

    #[test]
    fn its_bezier_pieces_stay_on_it() {
        for (h, cylinder) in [
            (helix(2.5, true), true),
            (
                HelixGeom {
                    r1: 6.0,
                    ..helix(2.5, false)
                },
                false,
            ),
        ] {
            let pieces = h.beziers();
            assert_eq!(pieces.len(), 40);
            let (mut off, mut turn): (f64, f64) = (0.0, 0.0);
            for (poles, w) in &pieces {
                for j in 0..=16 {
                    let p = rbez(poles, w, j as f64 / 16.0);
                    // Off the cylinder (or cone), and off the helix's angle at its height.
                    let s_of_z = p[2] / h.height;
                    let r_want = h.r0 + (h.r1 - h.r0) * s_of_z;
                    off = off.max(((p[0] * p[0] + p[1] * p[1]).sqrt() - r_want).abs());
                    let q = h.at(s_of_z).0;
                    turn = turn.max(((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt() / h.r0);
                }
            }
            assert!(off < if cylinder { 1e-10 } else { 1e-4 }, "{off}");
            assert!(turn < 1e-3, "{turn}");
        }
    }

    #[test]
    fn thicken_sides() {
        let t = ThickenFeature {
            thickness1: 2.0,
            thickness2: 0.5,
            ..Default::default()
        };
        assert_eq!(t.sides(), (2.0, 0.5));
        assert_eq!(
            ThickenFeature {
                flip: true,
                ..t.clone()
            }
            .sides(),
            (0.5, 2.0)
        );
        assert_eq!(
            ThickenFeature {
                mid_plane: true,
                ..t.clone()
            }
            .sides(),
            (1.0, 1.0)
        );
        assert_eq!(
            ThickenFeature::default().problem(),
            Some("Select surfaces or faces to thicken")
        );
    }

    #[test]
    fn helix_turns_and_height() {
        let h = HelixFeature {
            path: HelixPath::TurnsAndPitch,
            revolutions: 20.0,
            pitch: 2.0,
            ..Default::default()
        };
        assert_eq!(h.turns_and_height(10.0), (20.0, 40.0));
        let h = HelixFeature {
            path: HelixPath::Pitch,
            pitch: 2.0,
            ..Default::default()
        };
        assert_eq!(h.turns_and_height(10.0), (5.0, 10.0));
    }
}
