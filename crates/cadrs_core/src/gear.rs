//! Gear: involute gear teeth, as a new part or cut into a face of one (issue #179).
//!
//! - **Spur gear** (external teeth on a cylinder), **Ring gear** (internal teeth in a hole),
//!   **Bevel gear** (teeth on a cone) and **Rack** (teeth on a flat face), each with
//!   **Straight**, **Helical** or **Herringbone** teeth (a bevel's helical teeth are spiral
//!   ones; a bevel has no herringbone).
//! - With a **face** picked the teeth are cut into it: a cylinder becomes a spur gear whose tip
//!   circle is the cylinder, a hole a ring gear whose teeth's tips are the hole's wall, a cone a
//!   bevel gear whose face cone it is, a flat face a rack whose teeth's tips it is. The number
//!   of teeth is given and the module follows from the face (a rack takes its module).
//! - With none the gear is made whole from its **module** and **number of teeth**: its face
//!   width, bore and the rest, placed at a point with its axis along X, Y or Z.
//!
//! The module is the transverse one (the pitch diameter is `module × teeth` whatever the helix
//! angle), the teeth the standard full-depth ones: addendum `m`, dedendum `1.25 m`.
//!
//! The geometry here is plain numbers ([`plan`]): a blank and the regions to sweep, in frames
//! of their own. `rebuild/kernel_ops/gear.rs` makes them with the kernel.
//!
//! - A cylindrical gear's section is its transverse outline: involute flanks (one cubic
//!   B-spline each, through points of the involute with its end tangents), tip and root arcs,
//!   a radial line and a fillet below the base circle. Straight teeth extrude it; helical ones
//!   sweep it along the axis with a twist (an involute helicoid), herringbone ones in two
//!   halves.
//! - A bevel gear's teeth are spherical involutes: every flank a cone of lines through the
//!   apex. Its tooth spaces are drawn where they cross a plane square to the axis (the sphere's
//!   points seen from the apex) and swept toward the apex, shrinking to it; spiral teeth twist
//!   as they go. They are cut from the blank.
//! - A rack's tooth spaces are trapezoids, extruded across it (at the helix angle).

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

use crate::document::FaceRef;
use crate::ids::FeatureId;
use crate::solid::Solid;
use cadrs_sketch::Vec3;

/// What the teeth are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GearType {
    /// External teeth on a cylinder.
    #[default]
    Spur,
    /// Internal teeth in a ring.
    Ring,
    /// Teeth on a cone.
    Bevel,
    /// Teeth on a flat bar.
    Rack,
}

impl GearType {
    pub const ALL: [GearType; 4] = [
        GearType::Spur,
        GearType::Ring,
        GearType::Bevel,
        GearType::Rack,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GearType::Spur => "Spur gear",
            GearType::Ring => "Ring gear",
            GearType::Bevel => "Bevel gear",
            GearType::Rack => "Rack",
        }
    }
}

/// How the teeth run across the face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ToothStyle {
    #[default]
    Straight,
    Helical,
    Herringbone,
}

impl ToothStyle {
    pub const ALL: [ToothStyle; 3] = [
        ToothStyle::Straight,
        ToothStyle::Helical,
        ToothStyle::Herringbone,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToothStyle::Straight => "Straight",
            ToothStyle::Helical => "Helical",
            ToothStyle::Herringbone => "Herringbone",
        }
    }
}

/// How a bevel gear's helical teeth run along its face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SpiralKind {
    /// As a face-mill cutter leaves them (Gleason's spiral bevel): each tooth space the
    /// envelope of the cutter's two cones as the gear rolls on the cutter's crown gear, its
    /// length a circular arc of the cutter's radius.
    #[default]
    FaceMilled,
    /// At the same angle to the pitch cone's lines everywhere (a logarithmic spiral).
    Equiangular,
}

impl SpiralKind {
    pub const ALL: [SpiralKind; 2] = [SpiralKind::FaceMilled, SpiralKind::Equiangular];

    pub fn label(self) -> &'static str {
        match self {
            SpiralKind::FaceMilled => "Face-milled (Gleason)",
            SpiralKind::Equiangular => "Equiangular",
        }
    }
}

/// The axis of a gear made whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GearAxis {
    X,
    Y,
    #[default]
    Z,
}

impl GearAxis {
    pub const ALL: [GearAxis; 3] = [GearAxis::X, GearAxis::Y, GearAxis::Z];

    pub fn label(self) -> &'static str {
        match self {
            GearAxis::X => "X",
            GearAxis::Y => "Y",
            GearAxis::Z => "Z",
        }
    }

    /// The frame's x, y and z (the axis), right-handed.
    fn frame(self) -> [Vec3; 3] {
        let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        match self {
            GearAxis::X => [y, z, x],
            GearAxis::Y => [z, x, y],
            GearAxis::Z => [x, y, z],
        }
    }
}

/// A Gear feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GearFeature {
    /// The face the teeth are cut into: a cylinder, a hole, a cone or a flat face. None: the
    /// gear is made whole.
    #[serde(default)]
    pub face: Option<FaceRef>,
    #[serde(default)]
    pub gear_type: GearType,
    pub teeth: u32,
    #[serde(default)]
    pub teeth_expr: String,
    /// The transverse module (mm): the pitch diameter over the number of teeth. A gear on a
    /// cylinder, hole or cone takes its module from the face.
    pub module: f64,
    #[serde(default)]
    pub module_expr: String,
    /// Degrees.
    pub pressure_angle: f64,
    #[serde(default)]
    pub pressure_angle_expr: String,
    #[serde(default)]
    pub style: ToothStyle,
    /// Degrees, at the pitch circle (a bevel's spiral angle, the same all along its teeth).
    pub helix_angle: f64,
    #[serde(default)]
    pub helix_angle_expr: String,
    /// A left-hand helix (else right-hand, as a screw's thread).
    #[serde(default)]
    pub left_hand: bool,
    /// The face width (mm): along the axis, along a bevel's pitch cone, across a rack.
    pub width: f64,
    #[serde(default)]
    pub width_expr: String,
    /// The bore's diameter (mm), 0 for none.
    pub bore: f64,
    #[serde(default)]
    pub bore_expr: String,
    /// A ring gear's wall outside its teeth's roots (mm).
    pub rim: f64,
    #[serde(default)]
    pub rim_expr: String,
    /// A bevel gear's pitch angle (degrees): between its axis and its pitch cone.
    pub pitch_angle: f64,
    #[serde(default)]
    pub pitch_angle_expr: String,
    /// A rack's length (mm).
    pub length: f64,
    #[serde(default)]
    pub length_expr: String,
    /// The body under the teeth's roots (mm): a rack's bar, a bevel gear's back.
    pub height: f64,
    #[serde(default)]
    pub height_expr: String,
    /// The teeth are this much thinner at the pitch circle (mm).
    #[serde(default)]
    pub backlash: f64,
    #[serde(default)]
    pub backlash_expr: String,
    /// Where a gear made whole is (mm).
    #[serde(default)]
    pub position: [f64; 3],
    #[serde(default)]
    pub position_expr: [String; 3],
    #[serde(default)]
    pub axis: GearAxis,
    /// The gear turned about its axis (degrees): a tooth is centred on its frame's x axis at 0.
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub rotation_expr: String,
    /// A rack on a face: its teeth are spaced along the face's short side instead of its long
    /// one.
    #[serde(default)]
    pub flip: bool,
    /// How a bevel gear's helical teeth run.
    #[serde(default)]
    pub spiral: SpiralKind,
    /// A face-milled bevel gear's cutter radius (mm): 0 for the cone distance at the middle of
    /// its face.
    #[serde(default)]
    pub cutter_radius: f64,
    #[serde(default)]
    pub cutter_radius_expr: String,
}

impl Default for GearFeature {
    fn default() -> Self {
        let mm = |v: f64| format!("{} mm", crate::hole::fmt(v));
        let deg = |v: f64| format!("{} deg", crate::hole::fmt(v));
        Self {
            face: None,
            gear_type: GearType::Spur,
            teeth: 20,
            teeth_expr: "20".into(),
            module: 2.0,
            module_expr: mm(2.0),
            pressure_angle: 20.0,
            pressure_angle_expr: deg(20.0),
            style: ToothStyle::Straight,
            helix_angle: 20.0,
            helix_angle_expr: deg(20.0),
            left_hand: false,
            width: 10.0,
            width_expr: mm(10.0),
            bore: 8.0,
            bore_expr: mm(8.0),
            rim: 5.0,
            rim_expr: mm(5.0),
            pitch_angle: 45.0,
            pitch_angle_expr: deg(45.0),
            length: 60.0,
            length_expr: mm(60.0),
            height: 5.0,
            height_expr: mm(5.0),
            backlash: 0.0,
            backlash_expr: mm(0.0),
            position: [0.0; 3],
            position_expr: [mm(0.0), mm(0.0), mm(0.0)],
            axis: GearAxis::Z,
            rotation: 0.0,
            rotation_expr: deg(0.0),
            flip: false,
            spiral: SpiralKind::FaceMilled,
            cutter_radius: 0.0,
            cutter_radius_expr: mm(0.0),
        }
    }
}

/// Greater than zero (a number that isn't one is not).
fn positive(v: f64) -> bool {
    v > 0.0
}

/// Under zero, or not a number.
fn negative(v: f64) -> bool {
    v.is_nan() || v < 0.0
}

/// The fewest teeth a gear may have.
pub const MIN_TEETH: u32 = 6;
/// The most.
pub const MAX_TEETH: u32 = 400;

impl GearFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if !(MIN_TEETH..=MAX_TEETH).contains(&self.teeth) {
            return Some("The number of teeth must be between 6 and 400");
        }
        if !(self.pressure_angle >= 10.0 && self.pressure_angle <= 35.0) {
            return Some("The pressure angle must be between 10° and 35°");
        }
        if self.style != ToothStyle::Straight
            && !(self.helix_angle > 0.0 && self.helix_angle <= 60.0)
        {
            return Some("The helix angle must be above 0° and at most 60°");
        }
        if negative(self.backlash) {
            return Some("The backlash can't be negative");
        }
        let needs_module = self.face.is_none() || self.gear_type == GearType::Rack;
        if needs_module && !positive(self.module) {
            return Some("The module must be greater than zero");
        }
        if self.face.is_some() {
            return None;
        }
        if !positive(self.width) {
            return Some("The face width must be greater than zero");
        }
        if negative(self.bore) {
            return Some("The bore can't be negative");
        }
        match self.gear_type {
            GearType::Ring if !positive(self.rim) => {
                Some("The rim thickness must be greater than zero")
            }
            GearType::Bevel if !(self.pitch_angle >= 5.0 && self.pitch_angle <= 80.0) => {
                Some("The pitch angle must be between 5° and 80°")
            }
            GearType::Bevel | GearType::Rack if !positive(self.height) => {
                Some("The body thickness must be greater than zero")
            }
            GearType::Rack if !positive(self.length) => {
                Some("The rack's length must be greater than zero")
            }
            _ => None,
        }
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        match &self.face {
            Some(f) => vec![FeatureId(f.face.op)],
            None => Vec::new(),
        }
    }

    /// The name the feature list gives a new gear.
    pub fn base_name(&self) -> &'static str {
        "Gear"
    }

    /// The tooth form's numbers (the pressure angle in radians, the backlash).
    fn tooth(&self) -> Tooth {
        Tooth {
            alpha: self.pressure_angle.to_radians(),
            backlash: self.backlash,
        }
    }

    /// The helix angle (radians), signed: positive right-hand, 0 for straight teeth.
    fn beta(&self) -> f64 {
        if self.style == ToothStyle::Straight {
            0.0
        } else if self.left_hand {
            -self.helix_angle.to_radians()
        } else {
            self.helix_angle.to_radians()
        }
    }

    /// Takes a picked face: the gear type it makes, and the number of teeth that brings the
    /// module nearest the one set.
    pub fn fit(&mut self, target: &Target) {
        let m = if self.module > 0.0 { self.module } else { 2.0 };
        let teeth = |z: f64| (z.round() as i64).clamp(MIN_TEETH as i64, MAX_TEETH as i64) as u32;
        match *target {
            Target::Cylinder {
                radius, external, ..
            } => {
                if external {
                    self.gear_type = GearType::Spur;
                    self.teeth = teeth(2.0 * radius / m - 2.0);
                } else {
                    self.gear_type = GearType::Ring;
                    self.teeth = teeth(2.0 * radius / m + 2.0).max(12);
                }
            }
            Target::Cone {
                half_angle, heel, ..
            } => {
                self.gear_type = GearType::Bevel;
                self.teeth = teeth(2.0 * heel * half_angle.sin() / m - 2.0 * half_angle.cos());
                if self.style == ToothStyle::Herringbone {
                    self.style = ToothStyle::Helical;
                }
            }
            Target::Plane { .. } => self.gear_type = GearType::Rack,
        }
        self.teeth_expr = self.teeth.to_string();
    }

    /// The module the gear has on `target` (a rack's is its own).
    pub fn module_on(&self, target: &Target) -> Option<f64> {
        let z = self.teeth as f64;
        match *target {
            Target::Cylinder {
                radius,
                external: true,
                ..
            } => Some(2.0 * radius / (z + 2.0)),
            Target::Cylinder { radius, .. } => {
                Some(radius / (z / 2.0 - internal_addendum(z, self.tooth().alpha)))
            }
            Target::Cone {
                half_angle, heel, ..
            } => {
                let delta = pitch_angle_of_face(half_angle, z)?;
                Some(heel * half_angle.sin() / (z / 2.0 + delta.cos()))
            }
            Target::Plane { .. } => Some(self.module),
        }
    }

    /// A gear made whole: its pitch diameter (mm).
    pub fn pitch_diameter(&self) -> f64 {
        self.module * self.teeth as f64
    }

    /// A bevel gear made whole: how far its apex is above its back face, along its axis.
    pub fn apex_height(&self) -> Option<f64> {
        bevel_dims(self).map(|d| d.back)
    }
}

/// An internal gear's addendum over its module: 1, less where that would bring the teeth's tips
/// inside the base circle (fewer than 34 teeth at 20°).
fn internal_addendum(z: f64, alpha: f64) -> f64 {
    (0.95 * z * (1.0 - alpha.cos()) / 2.0).min(1.0)
}

/// The pitch angle of a bevel gear of `z` teeth whose face cone's half angle is `face`: the
/// face cone is the pitch cone raised by the addendum, `face = δ + atan(2 sin δ / z)`.
fn pitch_angle_of_face(face: f64, z: f64) -> Option<f64> {
    let f = |d: f64| d + (2.0 * d.sin() / z).atan() - face;
    let (mut lo, mut hi) = (1e-6, face);
    if f(lo) > 0.0 || f(hi) < 0.0 {
        return None;
    }
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if f(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

// ---------------------------------------------------------------------------------------------
// The face teeth are cut into

/// A face as a gear sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// A cylinder (a shaft's) or a hole.
    Cylinder {
        /// A point of the axis at one end of the face.
        start: Vec3,
        /// Unit, along the axis from `start` to the face's other end.
        dir: Vec3,
        radius: f64,
        /// The face's length along the axis.
        height: f64,
        /// Material inside it (else a hole).
        external: bool,
    },
    /// A cone with material inside it.
    Cone {
        apex: Vec3,
        /// Unit, along the axis from the apex toward the face.
        dir: Vec3,
        /// Between the axis and the cone's lines (radians).
        half_angle: f64,
        /// The face's near and far ends from the apex, along the cone's lines.
        toe: f64,
        heel: f64,
    },
    /// A flat face.
    Plane {
        /// A corner of the face's bounding rectangle.
        origin: Vec3,
        /// Unit: the face's outward normal.
        normal: Vec3,
        /// Unit, in the face: along its longest straight side.
        along: Vec3,
        /// The rectangle's sides: along `along`, and across (along `normal × along`).
        length: f64,
        width: f64,
    },
}

/// What a gear can do with face `face` of `solid` (an index into `solid.faces`).
pub fn target(solid: &Solid, face: usize) -> Option<Target> {
    use cadrs_kernel::SurfaceKind;
    let f = solid.faces.get(face)?;
    let pts: Vec<Vec3> = f.loops.iter().flatten().copied().collect();
    if pts.len() < 2 {
        return None;
    }
    match f.kind? {
        SurfaceKind::Plane => {
            let n = unit(f.plane?.normal());
            // The longest straight side.
            let mut best = (0.0, None);
            for l in &f.loops {
                for i in 0..l.len() {
                    let d = sub(l[(i + 1) % l.len()], l[i]);
                    if norm(d) > best.0 {
                        best = (norm(d), Some(unit(d)));
                    }
                }
            }
            let along = best.1?;
            let across = cross(n, along);
            let range = |d: Vec3| {
                pts.iter()
                    .map(|p| dot(*p, d))
                    .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)))
            };
            let ((a0, a1), (c0, c1)) = (range(along), range(across));
            let h = dot(pts[0], n);
            let origin = add(add(scale(along, a0), scale(across, c0)), scale(n, h));
            Some(Target::Plane {
                origin,
                normal: n,
                along,
                length: a1 - a0,
                width: c1 - c0,
            })
        }
        kind @ (SurfaceKind::Cylinder | SurfaceKind::Cone) => {
            let (o, d) = f.axis?;
            let d = unit(d);
            // Each boundary point's place along the axis and its distance from it.
            let tr: Vec<(f64, f64)> = pts
                .iter()
                .map(|p| {
                    let q = sub(*p, o);
                    let t = dot(q, d);
                    (t, norm(sub(q, scale(d, t))))
                })
                .collect();
            let (lo, hi) = tr
                .iter()
                .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v.0), b.max(v.0)));
            if hi - lo < 1e-6 {
                return None;
            }
            let (n, p) = (solid.face_normal(face)?, solid.face_point(face)?);
            let q = sub(p, o);
            let radial = sub(q, scale(d, dot(q, d)));
            let external = dot(n, radial) > 0.0;
            if kind == SurfaceKind::Cylinder {
                return Some(Target::Cylinder {
                    start: add(o, scale(d, lo)),
                    dir: d,
                    radius: f.radius?,
                    height: hi - lo,
                    external,
                });
            }
            // The cone's line through the points: r = r0 + k t (least squares).
            let nn = tr.len() as f64;
            let (st, sr) = tr.iter().fold((0.0, 0.0), |(a, b), v| (a + v.0, b + v.1));
            let (mt, mr) = (st / nn, sr / nn);
            let (stt, str_) = tr.iter().fold((0.0, 0.0), |(a, b), v| {
                (a + (v.0 - mt) * (v.0 - mt), b + (v.0 - mt) * (v.1 - mr))
            });
            let k = str_ / stt;
            if !external || k.abs() < 1e-6 {
                return None;
            }
            // From the apex (r = 0) toward the face.
            let t_apex = mt - mr / k;
            let (dir, s) = if k > 0.0 {
                (d, 1.0)
            } else {
                (scale(d, -1.0), -1.0)
            };
            let half_angle = k.abs().atan();
            let along = |t: f64| (s * (t - t_apex)).max(0.0) / half_angle.cos();
            let (a, b) = (along(lo), along(hi));
            Some(Target::Cone {
                apex: add(o, scale(d, t_apex)),
                dir,
                half_angle,
                toe: a.min(b),
                heel: a.max(b),
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Outlines

/// A curve of a section, in its plane.
#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Line {
        a: [f64; 2],
        b: [f64; 2],
    },
    /// From `start` (radians from x) through `sweep` (counter-clockwise when positive).
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// A clamped cubic B-spline.
    Spline {
        knots: Vec<f64>,
        poles: Vec<[f64; 2]>,
    },
    /// A whole circle about `center`.
    Circle {
        center: [f64; 2],
        radius: f64,
    },
}

impl Seg {
    /// Where it starts and ends (a circle: its point at angle 0, twice).
    pub fn ends(&self) -> ([f64; 2], [f64; 2]) {
        match self {
            Seg::Line { a, b } => (*a, *b),
            Seg::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let at = |t: f64| [center[0] + radius * t.cos(), center[1] + radius * t.sin()];
                (at(*start), at(start + sweep))
            }
            Seg::Spline { poles, .. } => (poles[0], poles[poles.len() - 1]),
            Seg::Circle { center, radius } => {
                let p = [center[0] + radius, center[1]];
                (p, p)
            }
        }
    }
}

/// A region of a section: its outer loop and its holes, each a closed run of curves.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Region2 {
    pub outer: Vec<Seg>,
    pub holes: Vec<Vec<Seg>>,
}

/// The tooth form's numbers.
#[derive(Debug, Clone, Copy)]
struct Tooth {
    /// The pressure angle (radians).
    alpha: f64,
    /// mm, at the pitch circle.
    backlash: f64,
}

/// A tooth's flank in polar coordinates about the gear's centre: at radius `rho` the flank is
/// the angle `psi(rho)` from the tooth's centre line.
#[derive(Debug, Clone, Copy)]
enum Flank {
    /// An involute of the circle of radius `base`; `half` is its angle at the base circle.
    Involute { base: f64, half: f64 },
    /// A spherical involute seen from the apex on the plane `h` from it: the base cone's half
    /// angle `base` (radians), `half` its angle at the base cone. `lead`: a spiral tooth's
    /// turn about the axis, `lead × ln(distance from the apex / cone)`: the same for every
    /// point of the tooth at one distance from the apex, none at the heel's pitch circle.
    Spherical {
        h: f64,
        base: f64,
        half: f64,
        lead: f64,
        cone: f64,
    },
}

fn inv(a: f64) -> f64 {
    a.tan() - a
}

/// The azimuth a spherical involute of the base cone `base` has turned through where it is
/// `delta` from the axis.
fn sph_inv(delta: f64, base: f64) -> f64 {
    if delta <= base {
        return 0.0;
    }
    let c = (delta.cos() / base.cos()).clamp(-1.0, 1.0);
    let t = (base.tan() / delta.tan()).clamp(-1.0, 1.0);
    c.acos() / base.sin() - t.acos()
}

impl Flank {
    /// The radius its involute starts at; below it the flank is a radial line.
    fn base(&self) -> f64 {
        match *self {
            Flank::Involute { base, .. } => base,
            Flank::Spherical { h, base, .. } => h * base.tan(),
        }
    }

    fn psi(&self, rho: f64) -> f64 {
        match *self {
            Flank::Involute { base, half } => {
                if rho <= base {
                    half
                } else {
                    half - inv((base / rho).acos())
                }
            }
            Flank::Spherical { h, base, half, .. } => half - sph_inv((rho / h).atan(), base),
        }
    }

    /// How far the tooth is turned about the axis at the radius `rho` (a spiral bevel's).
    fn lead(&self, rho: f64) -> f64 {
        match *self {
            Flank::Spherical { h, lead, cone, .. } if lead != 0.0 => {
                lead * ((h * h + rho * rho).sqrt() / cone).ln()
            }
            _ => 0.0,
        }
    }

    /// The flank's point at `rho` of the tooth centred at `centre`, on its side `side` (−1 the
    /// side before the centre going counter-clockwise, +1 the side after).
    fn point(&self, rho: f64, centre: f64, side: f64) -> [f64; 2] {
        let a = centre + side * self.psi(rho) + self.lead(rho);
        [rho * a.cos(), rho * a.sin()]
    }

    /// The flank from `from` to `to` (radii, `from` not below the base) as one spline.
    fn spline(&self, from: f64, to: f64, centre: f64, side: f64) -> Seg {
        const N: usize = 16;
        // Closer together toward the base, where the involute curls.
        let (lo, hi) = (from.min(to), from.max(to));
        let rho = |i: usize| {
            let u = i as f64 / N as f64;
            let u = if from <= to { u } else { 1.0 - u };
            lo + (hi - lo) * u * u
        };
        let pts: Vec<[f64; 2]> = (0..=N).map(|i| self.point(rho(i), centre, side)).collect();
        // The tangent at an end: along growing radius, by a one-sided difference of psi.
        let tangent = |r: f64| {
            let h = 1e-6 * hi;
            let (r0, r1) = if r + h <= hi { (r, r + h) } else { (r - h, r) };
            let (p0, p1) = (self.point(r0, centre, side), self.point(r1, centre, side));
            let d = [p1[0] - p0[0], p1[1] - p0[1]];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
            [d[0] / l, d[1] / l]
        };
        let s = if from <= to { 1.0 } else { -1.0 };
        let (t0, t1) = (tangent(from), tangent(to));
        interpolate(&pts, [s * t0[0], s * t0[1]], [s * t1[0], s * t1[1]])
    }
}

/// The clamped cubic B-spline through `pts` (chord-length parameters) leaving along the unit
/// tangent `t0` and arriving along `t1`.
fn interpolate(pts: &[[f64; 2]], t0: [f64; 2], t1: [f64; 2]) -> Seg {
    let n = pts.len() - 1;
    let mut u = vec![0.0; n + 1];
    for i in 1..=n {
        let d = [pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]];
        u[i] = u[i - 1] + (d[0] * d[0] + d[1] * d[1]).sqrt();
    }
    let total = u[n];
    u.iter_mut().for_each(|v| *v /= total);
    // Knots: clamped, the interior ones at the points' parameters.
    let mut knots = vec![0.0; 4];
    knots.extend_from_slice(&u[1..n]);
    knots.extend_from_slice(&[1.0; 4]);
    let m = n + 3;
    // Rows: the first point, the start derivative, the interior points, the end derivative,
    // the last point.
    let mut a = vec![vec![0.0; m]; m];
    let mut b = vec![[0.0; 2]; m];
    a[0][0] = 1.0;
    b[0] = pts[0];
    let k0 = 3.0 / knots[4];
    a[1][0] = -k0;
    a[1][1] = k0;
    for k in 1..n {
        let (span, basis) = basis(&knots, m, u[k]);
        for (j, v) in basis.iter().enumerate() {
            a[k + 1][span - 3 + j] = *v;
        }
        b[k + 1] = pts[k];
    }
    let k1 = 3.0 / (1.0 - knots[m - 1]);
    a[m - 2][m - 2] = -k1;
    a[m - 2][m - 1] = k1;
    a[m - 1][m - 1] = 1.0;
    b[m - 1] = pts[n];
    // The derivative rows are per unit of the parameter: a unit tangent over the whole chord
    // length is the curve's speed.
    b[1] = [t0[0] * total, t0[1] * total];
    b[m - 2] = [t1[0] * total, t1[1] * total];
    let poles = solve(a, b);
    Seg::Spline { knots, poles }
}

/// The four cubic basis functions that aren't zero at `u`, and the span they belong to.
fn basis(knots: &[f64], poles: usize, u: f64) -> (usize, [f64; 4]) {
    let mut span = 3;
    while span + 1 < poles && u >= knots[span + 1] {
        span += 1;
    }
    let mut n = [1.0, 0.0, 0.0, 0.0];
    let (mut left, mut right) = ([0.0; 4], [0.0; 4]);
    for j in 1..=3 {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let t = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * t;
            saved = left[j - r] * t;
        }
        n[j] = saved;
    }
    (span, n)
}

/// Solves `a x = b` (Gaussian elimination with partial pivoting; the systems are small).
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n)
            .max_by(|i, j| a[*i][c].abs().total_cmp(&a[*j][c].abs()))
            .expect("a row");
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..n {
            let f = a[r][c] / a[c][c];
            if f == 0.0 {
                continue;
            }
            for k in c..n {
                a[r][k] -= f * a[c][k];
            }
            b[r] = [b[r][0] - f * b[c][0], b[r][1] - f * b[c][1]];
        }
    }
    let mut x = vec![[0.0; 2]; n];
    for r in (0..n).rev() {
        let mut v = b[r];
        for k in r + 1..n {
            v = [v[0] - a[r][k] * x[k][0], v[1] - a[r][k] * x[k][1]];
        }
        x[r] = [v[0] / a[r][r], v[1] / a[r][r]];
    }
    x
}

/// A gear's section: its flank and the radii of its root and tip circles.
#[derive(Debug, Clone, Copy)]
struct Section {
    flank: Flank,
    teeth: u32,
    root: f64,
    tip: f64,
    /// The root fillet's radius wanted (0 for none).
    fillet: f64,
    /// The angle of the first tooth's centre (radians).
    phase: f64,
}

/// A flank's foot: where it leaves the root circle.
#[derive(Debug, Clone, Copy)]
struct Foot {
    /// The radius the involute starts at.
    start: f64,
    /// The fillet's radius (0: none) and where it meets the radial line.
    fillet: f64,
    line: f64,
    /// The angle from the tooth's centre of the flank at `start` and of the fillet's end on the
    /// root circle.
    psi_start: f64,
    psi_root: f64,
}

impl Section {
    fn pitch(&self) -> f64 {
        2.0 * PI / self.teeth as f64
    }

    /// Half the least top land a tooth is left.
    fn land(&self) -> f64 {
        0.05 * (self.tip - self.root)
    }

    /// The tip radius, lowered where the teeth would come to a point: the top land is kept a
    /// tenth of the tooth's depth wide.
    fn tip(&self) -> f64 {
        let land = self.land();
        if self.flank.psi(self.tip) * self.tip >= land {
            return self.tip;
        }
        let (mut lo, mut hi) = (self.root.max(self.flank.base()), self.tip);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if self.flank.psi(mid) * mid >= land {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    fn foot(&self) -> Foot {
        // (A spiral tooth's flank under the base cone is no straight line: the flank's curve
        // runs from the root.)
        let start = if self.flank.lead(self.root) != 0.0 || self.flank.lead(self.tip) != 0.0 {
            self.root
        } else {
            self.root.max(self.flank.base())
        };
        let psi_start = self.flank.psi(start);
        let none = Foot {
            start,
            fillet: 0.0,
            line: start,
            psi_start,
            psi_root: psi_start,
        };
        if start - self.root < 1e-9 * start {
            return none;
        }
        // The fillet's circle touches the root circle and the radial line: it must fit in the
        // line's length and in half the space between two teeth.
        let room = (self.pitch() / 2.0 - psi_start).max(0.0);
        let by_line = (start * start - self.root * self.root) / (2.0 * self.root);
        let by_room = self.root * room.sin() / (1.0 - room.sin()).max(1e-9);
        let f = self.fillet.min(0.98 * by_line).min(0.98 * by_room);
        if f < 1e-4 * (self.tip - self.root) {
            return Foot {
                line: self.root,
                ..none
            };
        }
        Foot {
            start,
            fillet: f,
            line: ((self.root + f) * (self.root + f) - f * f).sqrt(),
            psi_start,
            psi_root: psi_start + (f / (self.root + f)).asin(),
        }
    }

    /// One flank from the root circle up to `top`, or from `top` down: the fillet, the radial
    /// line and the involute, of the tooth centred at `centre` on its side `side`.
    fn flank(&self, foot: &Foot, top: f64, centre: f64, side: f64, up: bool) -> Vec<Seg> {
        let polar = |r: f64, a: f64| [r * a.cos(), r * a.sin()];
        let a_line = centre + side * foot.psi_start;
        let mut out = Vec::new();
        if foot.fillet > 0.0 {
            let a_c = centre + side * foot.psi_root;
            // From the root circle (toward the gear's centre from the fillet's) to the line.
            // On the side before the centre it turns clockwise onto the line; on the side
            // after, the other way.
            out.push(Seg::Arc {
                center: polar(self.root + foot.fillet, a_c),
                radius: foot.fillet,
                start: a_c + PI,
                sweep: side * (PI / 2.0 - (foot.psi_root - foot.psi_start)),
            });
        }
        if foot.start - foot.line > 1e-9 * foot.start {
            out.push(Seg::Line {
                a: polar(foot.line, a_line),
                b: polar(foot.start, a_line),
            });
        }
        if top - foot.start > 1e-9 * top {
            out.push(self.flank.spline(foot.start, top, centre, side));
        }
        if !up {
            out.reverse();
            out.iter_mut().for_each(reverse);
        }
        out
    }

    /// The gear's whole outline, counter-clockwise.
    fn outline(&self) -> Vec<Seg> {
        let (foot, tip) = (self.foot(), self.tip());
        let psi_tip = self.flank.psi(tip);
        let gap = self.pitch() - 2.0 * foot.psi_root;
        let mut out = Vec::new();
        for i in 0..self.teeth {
            let c = self.phase + i as f64 * self.pitch();
            if gap > 1e-9 {
                out.push(Seg::Arc {
                    center: [0.0, 0.0],
                    radius: self.root,
                    start: c - self.pitch() + foot.psi_root + self.flank.lead(self.root),
                    sweep: gap,
                });
            }
            out.extend(self.flank(&foot, tip, c, -1.0, true));
            out.push(Seg::Arc {
                center: [0.0, 0.0],
                radius: tip,
                start: c - psi_tip + self.flank.lead(tip),
                sweep: 2.0 * psi_tip,
            });
            out.extend(self.flank(&foot, tip, c, 1.0, false));
        }
        out
    }

    /// The spaces between the teeth as regions, each closed outside the gear by an arc of
    /// radius `outer` (past the tips: the flanks run on to it). Where the teeth come to a point
    /// before `outer` the spaces are one region: the ring between the gear's outline, its teeth
    /// cut short there, and the circle.
    ///
    /// `whole`: a space's wall is one curve ([`Section::wall`]); else each of its pieces is a
    /// curve of its own (flank, line, fillet, root arc).
    fn spaces(&self, outer: f64, whole: bool) -> Vec<Region2> {
        let foot = self.foot();
        let psi_out = self.flank.psi(outer);
        if psi_out * outer < self.land() {
            let cut = Section {
                tip: outer,
                ..*self
            };
            return vec![Region2 {
                outer: circle(outer),
                holes: vec![cut.outline()],
            }];
        }
        (0..self.teeth)
            .map(|i| {
                let c = self.phase + i as f64 * self.pitch();
                let next = c + self.pitch();
                let mut wall = if whole {
                    vec![self.wall(&foot, outer, c)]
                } else {
                    // Down the flank after this tooth, round the root, up the next tooth's.
                    let mut out = self.flank(&foot, outer, c, 1.0, false);
                    let gap = self.pitch() - 2.0 * foot.psi_root;
                    if gap > 1e-9 {
                        out.push(Seg::Arc {
                            center: [0.0, 0.0],
                            radius: self.root,
                            start: c + foot.psi_root + self.flank.lead(self.root),
                            sweep: gap,
                        });
                    }
                    out.extend(self.flank(&foot, outer, next, -1.0, true));
                    out
                };
                wall.push(Seg::Arc {
                    center: [0.0, 0.0],
                    radius: outer,
                    start: next - psi_out + self.flank.lead(outer),
                    sweep: -(self.pitch() - 2.0 * psi_out),
                });
                Region2 {
                    outer: wall,
                    holes: Vec::new(),
                }
            })
            .collect()
    }

    /// One side of a space from the root circle up to `top`, as points: along the fillet, the
    /// radial line and the flank of the tooth centred at `centre`, on its side `side`. Closer
    /// together where the curve bends or changes: a cut's tool has one face a space, and the
    /// curve through these points is its wall.
    fn side_points(&self, foot: &Foot, top: f64, centre: f64, side: f64) -> Vec<[f64; 2]> {
        let polar = |r: f64, a: f64| [r * a.cos(), r * a.sin()];
        let a_line = centre + side * foot.psi_start;
        let mut out = Vec::new();
        if foot.fillet > 0.0 {
            let a_c = centre + side * foot.psi_root;
            let c = polar(self.root + foot.fillet, a_c);
            let sweep = side * (PI / 2.0 - (foot.psi_root - foot.psi_start));
            const N: usize = 8;
            for k in 0..N {
                let a = a_c + PI + sweep * k as f64 / N as f64;
                out.push([c[0] + foot.fillet * a.cos(), c[1] + foot.fillet * a.sin()]);
            }
        } else {
            out.push(polar(self.root, a_line));
        }
        if foot.start - foot.line > 1e-9 * foot.start {
            // (The fillet's points stop short of the line's start; the root circle's point is
            // it where there is no fillet.)
            let from = if foot.fillet > 0.0 { 0 } else { 1 };
            const N: usize = 4;
            for k in from..N {
                let r = foot.line + (foot.start - foot.line) * k as f64 / N as f64;
                out.push(polar(r, a_line));
            }
        } else if foot.fillet == 0.0 {
            out.pop();
        }
        // The flank, closer together toward the base circle, where the involute curls.
        const N: usize = 20;
        if top - foot.start < 1e-9 * top {
            out.push(self.flank.point(foot.start, centre, side));
            return out;
        }
        for k in 0..=N {
            let u = k as f64 / N as f64;
            out.push(
                self.flank
                    .point(foot.start + (top - foot.start) * u * u, centre, side),
            );
        }
        out
    }

    /// A space's wall as one curve: from `outer` down the flank after the tooth centred at
    /// `c`, round the root and up the next tooth's flank to `outer` again. Its pieces meet
    /// tangent, so one cubic B-spline through points of them follows them closely.
    fn wall(&self, foot: &Foot, outer: f64, c: f64) -> Seg {
        let next = c + self.pitch();
        let mut pts = self.side_points(foot, outer, c, 1.0);
        pts.reverse();
        let up = self.side_points(foot, outer, next, -1.0);
        // Along the root circle between the two sides.
        let (a0, a1) = (c + foot.psi_root, next - foot.psi_root);
        if a1 - a0 > 1e-9 {
            const N: usize = 6;
            for k in 1..N {
                let a = a0 + (a1 - a0) * k as f64 / N as f64 + self.flank.lead(self.root);
                pts.push([self.root * a.cos(), self.root * a.sin()]);
            }
            pts.extend(up);
        } else {
            // The two fillets meet: one point there.
            pts.extend(up.into_iter().skip(1));
        }
        // The ends' tangents: along the flank, down it and up it.
        let tangent = |centre: f64, side: f64| {
            // (At the base circle the involute leaves along the radius, as the line below it.)
            let h = 1e-6 * outer;
            let (p0, p1) = (
                self.flank.point(outer, centre, side),
                self.flank.point(outer + h, centre, side),
            );
            let d = [p1[0] - p0[0], p1[1] - p0[1]];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
            [d[0] / l, d[1] / l]
        };
        let (t0, t1) = (tangent(c, 1.0), tangent(next, -1.0));
        interpolate(&pts, [-t0[0], -t0[1]], t1)
    }
}

/// Turns a curve end for end.
fn reverse(s: &mut Seg) {
    match s {
        Seg::Line { a, b } => std::mem::swap(a, b),
        Seg::Arc { start, sweep, .. } => {
            *start += *sweep;
            *sweep = -*sweep;
        }
        Seg::Spline { knots, poles } => {
            poles.reverse();
            let (lo, hi) = (knots[0], knots[knots.len() - 1]);
            knots.reverse();
            knots.iter_mut().for_each(|k| *k = lo + hi - *k);
        }
        Seg::Circle { .. } => {}
    }
}

fn circle(radius: f64) -> Vec<Seg> {
    vec![Seg::Circle {
        center: [0.0, 0.0],
        radius,
    }]
}

// ---------------------------------------------------------------------------------------------
// The plan

/// A plane a section is drawn on: its origin, its x axis and its normal (unit, square).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: Vec3,
    pub x: Vec3,
    pub normal: Vec3,
}

impl Frame {
    pub fn y(&self) -> Vec3 {
        cross(self.normal, self.x)
    }

    /// The frame moved `d` along its normal.
    fn raised(&self, d: f64) -> Frame {
        Frame {
            origin: add(self.origin, scale(self.normal, d)),
            ..*self
        }
    }
}

/// Regions swept from their plane: straight along `travel`, twisting about it and shrinking
/// toward (or growing from) its line as they go.
#[derive(Debug, Clone, PartialEq)]
pub struct Sweep {
    pub frame: Frame,
    pub regions: Vec<Region2>,
    /// From the plane's origin to the sweep's end: along the normal, or (a rack's helical
    /// teeth) leaning from it, the section keeping its orientation.
    pub travel: Vec3,
    /// The turn about `travel` over its length (radians, right-handed about it).
    pub twist: f64,
    /// The section's size at the end over its size at the start.
    pub scale: f64,
    /// The twist runs in proportion to the logarithm of the size (a spiral bevel's teeth), not
    /// to the distance travelled.
    pub spiral: bool,
}

/// A solid the teeth are cut from.
#[derive(Debug, Clone, PartialEq)]
pub enum Blank {
    /// The polygon (x, y) in `frame` turned about the frame's y axis through its origin.
    Revolve {
        frame: Frame,
        polygon: Vec<[f64; 2]>,
    },
    /// The polygon in `frame` extruded `depth` along its normal.
    Extrude {
        frame: Frame,
        polygon: Vec<[f64; 2]>,
        depth: f64,
    },
}

/// How a gear is made.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// A gear made whole by cutting: the solid its tooth spaces are cut from.
    pub blank: Option<Blank>,
    /// With a blank, or a face picked: the tooth spaces to cut. Else the gear itself, in pieces
    /// that join end to end.
    pub sweeps: Vec<Sweep>,
    /// More tooth spaces to cut, each lofted through its sections (a face-milled bevel's).
    pub lofts: Vec<Loft>,
    /// The module the gear came out with (mm).
    pub module: f64,
}

/// A solid through sections: each a region on its plane, the regions' curves matching one for
/// one from section to section.
#[derive(Debug, Clone, PartialEq)]
pub struct Loft {
    pub sections: Vec<(Frame, Region2)>,
}

/// The plan of `x`: on `target` (the face its teeth are cut into), or made whole.
pub fn plan(x: &GearFeature, target: Option<&Target>) -> Result<Plan, &'static str> {
    if let Some(p) = x.problem() {
        return Err(p);
    }
    match (target, x.gear_type) {
        (None, GearType::Spur) => spur(x),
        (None, GearType::Ring) => ring(x),
        (None, GearType::Bevel) => bevel(x),
        (None, GearType::Rack) => rack(x),
        (Some(t @ Target::Cylinder { external: true, .. }), GearType::Spur)
        | (
            Some(
                t @ Target::Cylinder {
                    external: false, ..
                },
            ),
            GearType::Ring,
        ) => on_cylinder(x, t),
        (Some(t @ Target::Cone { .. }), GearType::Bevel) => on_cone(x, t),
        (Some(t @ Target::Plane { .. }), GearType::Rack) => on_plane(x, t),
        (Some(_), GearType::Spur) => Err("A spur gear's teeth go on a cylinder"),
        (Some(_), GearType::Ring) => Err("A ring gear's teeth go in a hole"),
        (Some(_), GearType::Bevel) => Err("A bevel gear's teeth go on a cone"),
        (Some(_), GearType::Rack) => Err("A rack's teeth go on a flat face"),
    }
}

/// An external gear's section: module `m`, about the origin.
fn spur_section(x: &GearFeature, m: f64) -> Section {
    let (z, t) = (x.teeth as f64, x.tooth());
    let r = m * z / 2.0;
    Section {
        flank: Flank::Involute {
            base: r * t.alpha.cos(),
            half: (PI * m / 2.0 - t.backlash) / (2.0 * r) + inv(t.alpha),
        },
        teeth: x.teeth,
        root: r - 1.25 * m,
        tip: r + m,
        fillet: 0.38 * m,
        phase: x.rotation.to_radians(),
    }
}

/// An internal gear's spaces as a section: an external gear's teeth with the internal one's
/// root circle as their tips, and `inner` (the internal teeth's tip circle, or inside it) as
/// their root.
fn ring_section(x: &GearFeature, m: f64, inner: f64) -> Section {
    let (z, t) = (x.teeth as f64, x.tooth());
    let r = m * z / 2.0;
    Section {
        flank: Flank::Involute {
            base: r * t.alpha.cos(),
            half: (PI * m / 2.0 + t.backlash) / (2.0 * r) + inv(t.alpha),
        },
        teeth: x.teeth,
        root: inner,
        tip: r + 1.25 * m,
        fillet: 0.0,
        // A space, not a tooth, on the x axis: it meshes with a spur gear's tooth there.
        phase: x.rotation.to_radians(),
    }
}

/// A cylindrical gear's sweeps: `regions` on `frame`, `width` along its normal, straight,
/// helical or in herringbone halves.
fn cylindrical(
    x: &GearFeature,
    frame: Frame,
    regions: Vec<Region2>,
    width: f64,
    pitch_radius: f64,
) -> Vec<Sweep> {
    let twist = width * x.beta().tan() / pitch_radius;
    let piece = |frame: Frame, regions: Vec<Region2>, length: f64, twist: f64| Sweep {
        frame,
        regions,
        travel: scale(frame.normal, length),
        twist,
        scale: 1.0,
        spiral: false,
    };
    if x.style != ToothStyle::Herringbone {
        return vec![piece(frame, regions, width, twist)];
    }
    // Two halves: the second starts turned by the first's twist and turns back. A whole circle
    // starts where it is drawn from, turned or not, so circles go as two arcs: the second
    // half's then start where the first half's end, and the kernel joins the halves edge to
    // edge.
    let half = twist / 2.0;
    let regions: Vec<Region2> = regions.iter().map(halved_circles).collect();
    let turned: Vec<Region2> = regions.iter().map(|r| rotate_region(r, half)).collect();
    vec![
        piece(frame, regions, width / 2.0, half),
        piece(frame.raised(width / 2.0), turned, width / 2.0, -half),
    ]
}

/// The region with each whole circle as two half arcs.
fn halved_circles(r: &Region2) -> Region2 {
    let halve = |l: &Vec<Seg>| {
        l.iter()
            .flat_map(|s| match s {
                Seg::Circle { center, radius } => (0..2)
                    .map(|k| Seg::Arc {
                        center: *center,
                        radius: *radius,
                        start: k as f64 * PI,
                        sweep: PI,
                    })
                    .collect(),
                s => vec![s.clone()],
            })
            .collect()
    };
    Region2 {
        outer: halve(&r.outer),
        holes: r.holes.iter().map(halve).collect(),
    }
}

fn rotate_region(r: &Region2, a: f64) -> Region2 {
    let turn = |l: &Vec<Seg>| l.iter().map(|s| rotate(s, a)).collect();
    Region2 {
        outer: turn(&r.outer),
        holes: r.holes.iter().map(turn).collect(),
    }
}

fn rotate(s: &Seg, a: f64) -> Seg {
    let (sn, cs) = a.sin_cos();
    let p = |q: [f64; 2]| [cs * q[0] - sn * q[1], sn * q[0] + cs * q[1]];
    match s {
        Seg::Line { a, b } => Seg::Line { a: p(*a), b: p(*b) },
        Seg::Arc {
            center,
            radius,
            start,
            sweep,
        } => Seg::Arc {
            center: p(*center),
            radius: *radius,
            start: start + a,
            sweep: *sweep,
        },
        Seg::Spline { knots, poles } => Seg::Spline {
            knots: knots.clone(),
            poles: poles.iter().map(|q| p(*q)).collect(),
        },
        Seg::Circle { center, radius } => Seg::Circle {
            center: p(*center),
            radius: *radius,
        },
    }
}

/// The frame of a gear made whole: at its position, its normal along its axis.
fn own_frame(x: &GearFeature) -> Frame {
    let [fx, _, fz] = x.axis.frame();
    Frame {
        origin: x.position,
        x: fx,
        normal: fz,
    }
}

fn spur(x: &GearFeature) -> Result<Plan, &'static str> {
    let m = x.module;
    let s = spur_section(x, m);
    if s.root <= 0.0 {
        return Err("The gear has too few teeth for its tooth form");
    }
    let mut region = Region2 {
        outer: s.outline(),
        holes: Vec::new(),
    };
    if x.bore > 0.0 {
        if x.bore / 2.0 >= s.root - 0.25 * m {
            return Err("The bore is too large for the gear");
        }
        region.holes.push(circle(x.bore / 2.0));
    }
    Ok(Plan {
        blank: None,
        sweeps: cylindrical(
            x,
            own_frame(x),
            vec![region],
            x.width,
            m * x.teeth as f64 / 2.0,
        ),
        lofts: Vec::new(),
        module: m,
    })
}

fn ring(x: &GearFeature) -> Result<Plan, &'static str> {
    let (m, z) = (x.module, x.teeth as f64);
    let r = m * z / 2.0;
    let inner = r - internal_addendum(z, x.tooth().alpha) * m;
    let s = ring_section(x, m, inner);
    let region = Region2 {
        outer: circle(s.tip + x.rim),
        holes: vec![s.outline()],
    };
    Ok(Plan {
        blank: None,
        sweeps: cylindrical(x, own_frame(x), vec![region], x.width, r),
        lofts: Vec::new(),
        module: m,
    })
}

/// A frame at `origin` square to `dir`, its x axis a direction that depends on `dir` alone.
fn frame_about(origin: Vec3, dir: Vec3) -> Frame {
    // The world axis least along `dir`, made square to it.
    let a = [dir[0].abs(), dir[1].abs(), dir[2].abs()];
    let e = if a[0] <= a[1] && a[0] <= a[2] {
        [1.0, 0.0, 0.0]
    } else if a[1] <= a[2] {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let x = unit(sub(e, scale(dir, dot(e, dir))));
    Frame {
        origin,
        x,
        normal: dir,
    }
}

fn on_cylinder(x: &GearFeature, t: &Target) -> Result<Plan, &'static str> {
    let Target::Cylinder {
        start,
        dir,
        radius,
        height,
        external,
    } = *t
    else {
        return Err("Select a cylinder");
    };
    let m = x.module_on(t).ok_or("The gear doesn't fit the face")?;
    let frame = frame_about(start, dir);
    let r = m * x.teeth as f64 / 2.0;
    let regions = if external {
        let s = spur_section(x, m);
        if s.root <= 0.0 {
            return Err("The gear has too few teeth for its tooth form");
        }
        // Past the cylinder, so the cut leaves none of its surface between the teeth.
        s.spaces(radius + 0.25 * m, true)
    } else {
        // Its "roots" inside the hole, in the air.
        let s = ring_section(x, m, radius - 0.25 * m);
        vec![Region2 {
            outer: s.outline(),
            holes: Vec::new(),
        }]
    };
    Ok(Plan {
        blank: None,
        sweeps: cylindrical(x, frame, regions, height, r),
        lofts: Vec::new(),
        module: m,
    })
}

/// A bevel gear's numbers.
struct BevelDims {
    /// The module at the heel.
    m: f64,
    /// The pitch angle (radians).
    delta: f64,
    /// The cone distance: from the apex to the pitch circle along the pitch cone.
    cone: f64,
    /// The toe's cone distance over the heel's.
    kappa: f64,
    /// The apex's height over the back face (a gear made whole).
    back: f64,
    /// The body's depth under the roots at the heel.
    body: f64,
}

fn bevel_dims(x: &GearFeature) -> Option<BevelDims> {
    let (m, z) = (x.module, x.teeth as f64);
    let delta = x.pitch_angle.to_radians();
    let cone = m * z / (2.0 * delta.sin());
    if !(x.width > 0.0 && x.width < 0.9 * cone) {
        return None;
    }
    // The back cone runs down from the heel no nearer the axis than the hub.
    let hub = (x.bore / 2.0 + m).max(0.25 * m * z / 2.0);
    let reach = (cone * delta.sin() - hub) / delta.cos() - 1.25 * m;
    let body = x.height.min(reach);
    if body < 0.25 * m {
        return None;
    }
    Some(BevelDims {
        m,
        delta,
        cone,
        kappa: 1.0 - x.width / cone,
        back: cone * delta.cos() + (1.25 * m + body) * delta.sin(),
        body,
    })
}

/// A bevel gear's tooth spaces: swept toward the apex from the plane just behind the heel's
/// roots. `frame` is at the apex, its normal along the axis away from the gear (so the gear is
/// at negative heights).
fn bevel_spaces(x: &GearFeature, d: &BevelDims, apex: Frame) -> Result<Sweep, &'static str> {
    let (z, t) = (x.teeth as f64, x.tooth());
    let (m, delta, cone) = (d.m, d.delta, d.cone);
    let face = delta + (m / cone).atan();
    let root = delta - (1.25 * m / cone).atan();
    if face > 84f64.to_radians() {
        return Err("The gear's face cone is too flat: lower the pitch angle or add teeth");
    }
    if root <= 0.5f64.to_radians() {
        return Err("The gear has too few teeth for its pitch angle");
    }
    let base = (delta.sin() * t.alpha.cos()).asin();
    // The plane the section is drawn on: just past where the root cone leaves the back cone.
    let h1 = cone * delta.cos() + 1.25 * m * delta.sin() + 0.5 * m;
    // The plane it is swept to: just past the toe's tips.
    let h2 = (d.kappa * (cone * delta.cos() - m * delta.sin()) - 0.5 * m).max(0.08 * h1);
    if h2 >= h1 {
        return Err("The gear's face is too narrow");
    }
    // A spiral tooth turns about the axis as it runs to the apex, by an amount that depends on
    // the distance from the apex alone: `lead × ln(distance / cone)`, an equiangular spiral,
    // which crosses the pitch cone's lines at the helix angle everywhere and is none at the
    // heel's pitch circle (where the rotation sets two gears in mesh). The section holds the
    // turn each of its points has where it is drawn; the sweep adds the rest, in proportion to
    // the logarithm of its scale.
    let lead = -x.beta().tan() / delta.sin();
    let section = Section {
        flank: Flank::Spherical {
            h: h1,
            base,
            half: PI / (2.0 * z) - t.backlash / (m * z) + sph_inv(delta, base),
            lead,
            cone,
        },
        teeth: x.teeth,
        root: h1 * root.tan(),
        tip: h1 * face.tan(),
        // No root fillet: swept to the apex it is a face the blank's cones are slow to cut (a
        // third of the gear's time), where the root's own cone and the flanks' planes are not.
        fillet: 0.0,
        phase: x.rotation.to_radians(),
    };
    // A quarter of the addendum past the tips.
    let outer = h1 * (face + 0.25 * (m / cone).atan()).tan();
    let frame = apex.raised(-h1);
    Ok(Sweep {
        frame,
        // (One curve for a whole wall makes one large face a space, which the cones of a bevel
        // gear's blank are slow to cut: its pieces stay faces of their own, the lines planes
        // and the root a cone.)
        regions: section.spaces(outer, false),
        travel: scale(frame.normal, h1 - h2),
        twist: lead * (h2 / h1).ln(),
        scale: h2 / h1,
        spiral: lead != 0.0,
    })
}

/// A bevel gear's tooth spaces: swept, or (face-milled spiral teeth) lofted.
fn bevel_teeth(
    x: &GearFeature,
    d: &BevelDims,
    apex: Frame,
) -> Result<(Vec<Sweep>, Vec<Loft>), &'static str> {
    if x.style == ToothStyle::Helical && x.spiral == SpiralKind::FaceMilled {
        Ok((Vec::new(), face_milled_spaces(x, d, apex)?))
    } else {
        Ok((vec![bevel_spaces(x, d, apex)?], Vec::new()))
    }
}

/// A face-mill cutter generating a spiral bevel gear's tooth space (Gleason's method, as
/// Litvin describes it: NASA CR-4259, 1989), without the changes to the pinion's machine
/// settings that localise a pair's bearing contact.
///
/// The cutter's blades turn about its axis, their edges sweeping two cones: the outside
/// blades' and the inside blades', the pressure angle from the axis. The cutter sits on the
/// cradle, whose axis is square to the plane the blades' tips sweep; that plane lies along the
/// gear's root cone. The cradle turns as the gear turns on its own axis, at the ratio that
/// makes the gear's pitch cone roll on the cradle's (its crown gear). The gear's flanks are the
/// envelopes of the two cones: at each instant a cone touches its flank along the line where
/// its normals pass through the pitch line the two roll about.
///
/// Fixed axes: x along that pitch line from the apex, z up from the gear's side of the pitch
/// plane. Lengths in mm, angles in radians.
struct FaceMill {
    /// The gear's axis, from the apex toward the gear's back.
    g: Vec3,
    /// The cradle's axis, and the plane square to it the blades' tips sweep: `u` along the
    /// root cone's line under the pitch line, `v` across (u × v = c).
    c: Vec3,
    u: Vec3,
    v: Vec3,
    /// The gear's own x (at 0 turn): from its axis to the pitch line.
    x_g: Vec3,
    /// The cutter's centre, in the tips' plane.
    centre: Vec3,
    /// The cutter's radius: to the middle of the slot, at the height of the pitch line's
    /// middle point over the tips' plane (`level`).
    radius: f64,
    /// The slot's width there.
    slot: f64,
    level: f64,
    tan_alpha: f64,
    /// The gear's turn over the cradle's.
    ratio: f64,
    /// The dedendum angle: between the pitch line and the tips' plane.
    dedendum: f64,
    /// Where the middle of the slot is on the cutter's circle (radians from u).
    tau_mid: f64,
    /// The root cone's half angle.
    root: f64,
}

/// `p` turned by `a` about the unit axis `k`.
fn rot(p: Vec3, k: Vec3, a: f64) -> Vec3 {
    let (s, c) = a.sin_cos();
    add(
        add(scale(p, c), scale(cross(k, p), s)),
        scale(k, dot(k, p) * (1.0 - c)),
    )
}

impl FaceMill {
    /// `delta` the pitch angle, `dedendum` the dedendum angle, `mean` the cone distance at the
    /// middle of the face, `beta` the spiral angle there (its sign the hand), `radius` the
    /// cutter's, `slot` the slot's width at the pitch line's middle point.
    fn new(
        delta: f64,
        dedendum: f64,
        mean: f64,
        beta: f64,
        radius: f64,
        slot: f64,
        alpha: f64,
    ) -> Self {
        let (sf, cf) = dedendum.sin_cos();
        let (u, v, c) = ([cf, 0.0, -sf], [0.0, 1.0, 0.0], [sf, 0.0, cf]);
        // The middle point M of the pitch line, seen in the tips' plane, and the cutter's
        // centre: the slot's middle circle passes through M at the spiral angle to u.
        // (The side the cutter's centre is on is the hand: a right-hand tooth turns toward the
        // toe as the equiangular spiral's does.)
        let hand = if beta < 0.0 { 1.0 } else { -1.0 };
        let b = beta.abs();
        let m = [mean * cf, 0.0];
        let cc = [m[0] - radius * b.sin(), hand * radius * b.cos()];
        Self {
            g: [delta.cos(), 0.0, -delta.sin()],
            c,
            u,
            v,
            x_g: [delta.sin(), 0.0, delta.cos()],
            centre: add(scale(u, cc[0]), scale(v, cc[1])),
            radius,
            slot,
            level: mean * sf,
            tan_alpha: alpha.tan(),
            ratio: cf / delta.sin(),
            dedendum,
            tau_mid: (-hand * b.cos()).atan2(b.sin()),
            root: delta - dedendum,
        }
    }

    /// The flank's point the blade `side` (+1 the outside blades', −1 the inside blades') makes
    /// with its edge's point at `tau` round the cutter and `w` over the tips' plane: its
    /// distance from the apex along the gear's axis, its distance from the axis and its angle
    /// about it from the gear's x.
    fn point(&self, side: f64, tau: f64, w: f64) -> [f64; 3] {
        let (st, ct) = tau.sin_cos();
        let radial = add(scale(self.u, ct), scale(self.v, st));
        let rho = self.radius + side * (self.slot / 2.0 + (w - self.level) * self.tan_alpha);
        let r0 = add(add(self.centre, scale(radial, rho)), scale(self.c, w));
        let n0 = sub(radial, scale(self.c, side * self.tan_alpha));
        // The cradle's turn at which the cone touches the flank there: the normal through the
        // point meets the pitch line, (r × n) · x = 0 with both turned about the cradle's axis.
        let q0 = cross(r0, n0);
        let cf = self.dedendum.cos();
        let (a, b) = (cf * dot(q0, self.u), -cf * dot(q0, self.v));
        let d = -dot(q0, self.c) * self.dedendum.sin();
        let (base, off) = (b.atan2(a), (d / a.hypot(b)).clamp(-1.0, 1.0).acos());
        let wrap = |x: f64| (x + PI).rem_euclid(2.0 * PI) - PI;
        let (p1, p2) = (wrap(base + off), wrap(base - off));
        let phi = if p1.abs() <= p2.abs() { p1 } else { p2 };
        // On the cradle at that turn, then as the gear holds it: the gear has turned the other
        // way by the ratio.
        let p = rot(rot(r0, self.c, phi), self.g, self.ratio * phi);
        let zd = dot(p, self.g);
        let out = sub(p, scale(self.g, zd));
        [zd, norm(out), dot(p, self.v).atan2(dot(p, self.x_g))]
    }

    /// The edge's point (`tau`, `w`) whose flank point is `h` from the apex along the axis and
    /// `rho` from the axis, from `guess` (else from where the cutter's circle crosses there).
    fn solve(&self, side: f64, h: f64, rho: f64, guess: Option<[f64; 2]>) -> Option<[f64; 2]> {
        let s = h.hypot(rho);
        let mut x = match guess {
            Some(g) => g,
            None => {
                // Over the tips' plane by its angle over the root cone; round the cutter where
                // its circle is that far from the apex, on the side of the slot's middle.
                let over = rho.atan2(h) - self.root;
                let (w, d) = (s * over.sin(), s * over.cos());
                let r = self.radius + side * (self.slot / 2.0 + (w - self.level) * self.tan_alpha);
                let (cu, cv) = (dot(self.centre, self.u), dot(self.centre, self.v));
                let far = cu.hypot(cv);
                let cos = (far * far + r * r - d * d) / (2.0 * far * r);
                if cos.abs() > 1.0 {
                    return None;
                }
                let toward = cv.atan2(cu) + PI;
                let wrap = |x: f64| (x + PI).rem_euclid(2.0 * PI) - PI;
                let (t1, t2) = (toward + cos.acos(), toward - cos.acos());
                let tau = if wrap(t1 - self.tau_mid).abs() <= wrap(t2 - self.tau_mid).abs() {
                    t1
                } else {
                    t2
                };
                [tau, w]
            }
        };
        let miss = |x: [f64; 2]| {
            let p = self.point(side, x[0], x[1]);
            [p[0] - h, p[1] - rho]
        };
        for _ in 0..40 {
            let f = miss(x);
            if f[0].hypot(f[1]) < 1e-11 * s {
                return Some(x);
            }
            let (dt, dw) = (1e-7, 1e-7 * s);
            let (ft, fw) = (miss([x[0] + dt, x[1]]), miss([x[0], x[1] + dw]));
            let j = [
                [(ft[0] - f[0]) / dt, (fw[0] - f[0]) / dw],
                [(ft[1] - f[1]) / dt, (fw[1] - f[1]) / dw],
            ];
            let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
            if det.abs() < 1e-300 {
                return None;
            }
            x[0] -= (f[0] * j[1][1] - f[1] * j[0][1]) / det;
            x[1] -= (f[1] * j[0][0] - f[0] * j[1][0]) / det;
        }
        None
    }

    /// Where round the cutter the edge's point `w` over the tips' plane makes a flank point `h`
    /// from the apex along the axis, from `guess`.
    fn tau_at(&self, side: f64, h: f64, w: f64, guess: f64) -> Option<f64> {
        let mut tau = guess;
        for _ in 0..40 {
            let f = self.point(side, tau, w)[0] - h;
            if f.abs() < 1e-11 * h {
                return Some(tau);
            }
            let dt = 1e-7;
            let slope = (self.point(side, tau + dt, w)[0] - h - f) / dt;
            if slope.abs() < 1e-300 {
                return None;
            }
            tau -= f / slope;
        }
        None
    }

    /// The flank the blade `side` makes where the plane `h` from the apex crosses it: from
    /// where the blade's tip leaves it (the flank's foot: below it the tip cuts the fillet) up
    /// to the radius `top`, a curve through its points, turned by `turn` about the axis. Also
    /// its unit tangent at its foot (up the flank).
    fn flank(&self, side: f64, h: f64, pitch: f64, top: f64, turn: f64) -> Option<(Seg, [f64; 2])> {
        const N: usize = 12;
        // From the pitch cone, where the flank is surely the envelope: down the blade to its
        // tip, and up to the top.
        let at_pitch = self.solve(side, h, pitch, None)?;
        let at_top = self.solve(side, h, top, Some(at_pitch))?;
        let mut tau = at_pitch[0];
        const DOWN: usize = 6;
        for k in 1..=DOWN {
            let w = at_pitch[1] * (1.0 - k as f64 / DOWN as f64);
            tau = self.tau_at(side, h, w, tau)?;
        }
        let place = |tau: f64, w: f64| {
            let p = self.point(side, tau, w);
            let a = p[2] + turn;
            [p[1] * a.cos(), p[1] * a.sin()]
        };
        let step = at_top[1] / N as f64;
        let mut pts = Vec::with_capacity(N + 1);
        let foot = tau;
        for k in 0..N {
            tau = self.tau_at(side, h, step * k as f64, tau)?;
            pts.push(place(tau, step * k as f64));
        }
        pts.push(place(at_top[0], at_top[1]));
        // The ends' tangents, from points just inside them.
        let near = 1e-4 * step;
        let b = place(self.tau_at(side, h, near, foot)?, near);
        let a = place(
            self.tau_at(side, h, at_top[1] - near, at_top[0])?,
            at_top[1] - near,
        );
        let dir = |p: [f64; 2], q: [f64; 2]| {
            let d = [q[0] - p[0], q[1] - p[1]];
            let l = d[0].hypot(d[1]);
            [d[0] / l, d[1] / l]
        };
        let up = dir(pts[0], b);
        Some((interpolate(&pts, up, dir(a, pts[N])), up))
    }
}

/// A face-milled bevel gear's tooth spaces, each a loft through its sections on planes square
/// to the axis. `apex` as [`bevel_spaces`] takes it.
fn face_milled_spaces(
    x: &GearFeature,
    d: &BevelDims,
    apex: Frame,
) -> Result<Vec<Loft>, &'static str> {
    let (z, t) = (x.teeth as f64, x.tooth());
    let (m, delta, cone) = (d.m, d.delta, d.cone);
    let dedendum = (1.25 * m / cone).atan();
    let face = delta + (m / cone).atan();
    let root = delta - dedendum;
    if face > 84f64.to_radians() {
        return Err("The gear's face cone is too flat: lower the pitch angle or add teeth");
    }
    if root <= 0.5f64.to_radians() {
        return Err("The gear has too few teeth for its pitch angle");
    }
    let h1 = cone * delta.cos() + 1.25 * m * delta.sin() + 0.5 * m;
    let h2 = (d.kappa * (cone * delta.cos() - m * delta.sin()) - 0.5 * m).max(0.08 * h1);
    if h2 >= h1 {
        return Err("The gear's face is too narrow");
    }
    // The cutter: its radius the cone distance at the middle of the face unless one is given;
    // its slot half the normal pitch there.
    let mean = cone * (1.0 + d.kappa) / 2.0;
    let beta = x.beta();
    let radius = if x.cutter_radius > 0.0 {
        x.cutter_radius
    } else {
        mean
    };
    if radius < 0.55 * (cone - d.kappa * cone) / beta.abs().cos().max(0.2) {
        return Err("The cutter is too small to cross the gear's face");
    }
    let slot = PI * m * (mean / cone) * beta.cos() / 2.0 + t.backlash;
    let mill = FaceMill::new(delta, dedendum, mean, beta, radius, slot, t.alpha);
    // The slot's middle is on the gear's x at the middle of the face: turned half a pitch, a
    // tooth is there, and by the rotation.
    let turn = x.rotation.to_radians() + PI / z;
    let outer = (face + 0.25 * (m / cone).atan()).tan();
    const SECTIONS: usize = 7;
    let mut first: Vec<(Frame, Region2)> = Vec::with_capacity(SECTIONS);
    let unfit = "The cutter doesn't reach across the gear's face: change its radius";
    for k in 0..SECTIONS {
        let h = h1 + (h2 - h1) * k as f64 / (SECTIONS - 1) as f64;
        let (lo, hi) = (h * root.tan(), h * outer);
        // Up the outside blades' flank, round outside the tips, down the inside blades', and
        // the bottom: a curve from one flank's foot to the other's that leaves them along
        // their lines and touches the root cone between (the fillet the blades' tips cut, near
        // enough).
        let (up, t_up) = mill.flank(1.0, h, h * delta.tan(), hi, turn).ok_or(unfit)?;
        let (mut down, t_down) = mill
            .flank(-1.0, h, h * delta.tan(), hi, turn)
            .ok_or(unfit)?;
        reverse(&mut down);
        let angle = |p: [f64; 2]| p[1].atan2(p[0]);
        let wrap = |a: f64| (a + PI).rem_euclid(2.0 * PI) - PI;
        let ((a0, a1), (b0, b1)) = (up.ends(), down.ends());
        let across = Seg::Arc {
            center: [0.0, 0.0],
            radius: hi,
            start: angle(a1),
            sweep: wrap(angle(b0) - angle(a1)),
        };
        let mid = angle(b1) + wrap(angle(a0) - angle(b1)) / 2.0;
        let floor = interpolate(
            &[b1, [lo * mid.cos(), lo * mid.sin()], a0],
            [-t_down[0], -t_down[1]],
            t_up,
        );
        first.push((
            apex.raised(-h),
            Region2 {
                outer: vec![up, across, down, floor],
                holes: Vec::new(),
            },
        ));
    }
    Ok((0..x.teeth)
        .map(|i| Loft {
            sections: first
                .iter()
                .map(|(f, r)| (*f, rotate_region(r, i as f64 * 2.0 * PI / z)))
                .collect(),
        })
        .collect())
}

fn bevel(x: &GearFeature) -> Result<Plan, &'static str> {
    if x.style == ToothStyle::Herringbone {
        return Err("A bevel gear's teeth are straight or helical");
    }
    let d = bevel_dims(x).ok_or("The bevel gear's face width or bore is too large for it")?;
    let own = own_frame(x);
    let (m, delta, cone, kappa) = (d.m, d.delta, d.cone, d.kappa);
    // In (radius, height over the back face): along the pitch cone from the apex, and square
    // to it away from the axis.
    let g = [delta.sin(), -delta.cos()];
    let n = [delta.cos(), delta.sin()];
    let at = |s: f64, u: f64| [s * g[0] + u * n[0], d.back + s * g[1] + u * n[1]];
    let heel_tip = at(cone, m);
    let toe_tip = at(kappa * cone, kappa * m);
    let heel_back = at(cone, -(1.25 * m + d.body));
    // The front runs down from the toe's tips as the back does, no nearer the axis than the
    // bore.
    let inner = x.bore / 2.0 + if x.bore > 0.0 { 0.5 * m } else { 0.0 };
    let drop = (kappa * (2.25 * m + d.body)).min((toe_tip[0] - inner) / delta.cos());
    if drop <= 0.0 {
        return Err("The bevel gear's face width or bore is too large for it");
    }
    let toe_front = [toe_tip[0] - drop * n[0], toe_tip[1] - drop * n[1]];
    let bore = x.bore / 2.0;
    let mut polygon = vec![heel_tip, toe_tip, toe_front];
    if toe_front[0] - bore > 1e-9 {
        polygon.push([bore, toe_front[1]]);
    }
    polygon.push([bore, 0.0]);
    polygon.push([heel_back[0], 0.0]);
    // The section's plane holds the axis: its x the gear's x, its y the axis.
    let revolve = Frame {
        origin: own.origin,
        x: own.x,
        normal: scale(own.y(), -1.0),
    };
    let apex = own.raised(d.back);
    let (sweeps, lofts) = bevel_teeth(x, &d, apex)?;
    Ok(Plan {
        blank: Some(Blank::Revolve {
            frame: revolve,
            polygon,
        }),
        sweeps,
        lofts,
        module: m,
    })
}

fn on_cone(x: &GearFeature, t: &Target) -> Result<Plan, &'static str> {
    let Target::Cone {
        apex,
        dir,
        half_angle,
        toe,
        heel,
    } = *t
    else {
        return Err("Select a cone");
    };
    if x.style == ToothStyle::Herringbone {
        return Err("A bevel gear's teeth are straight or helical");
    }
    let z = x.teeth as f64;
    let delta = pitch_angle_of_face(half_angle, z).ok_or("The gear doesn't fit the cone")?;
    let m = x.module_on(t).ok_or("The gear doesn't fit the cone")?;
    let d = BevelDims {
        m,
        delta,
        cone: m * z / (2.0 * delta.sin()),
        kappa: (toe / heel).clamp(0.0, 0.999),
        back: 0.0,
        body: 0.0,
    };
    // Its normal from the gear toward the apex and on.
    let frame = frame_about(apex, scale(dir, -1.0));
    let (sweeps, lofts) = bevel_teeth(x, &d, frame)?;
    Ok(Plan {
        blank: None,
        sweeps,
        lofts,
        module: m,
    })
}

/// A rack's tooth spaces in the plane across it: x along the rack, y out of its teeth, the
/// pitch line y = 0. Those whose roots reach into `from..to`, drawn up to `top`.
fn rack_spaces(x: &GearFeature, from: f64, to: f64, top: f64, centre: f64) -> Vec<Region2> {
    let (m, t) = (x.module, x.tooth());
    let pitch = PI * m;
    let half = |y: f64| pitch / 4.0 + t.backlash / 2.0 + y * t.alpha.tan();
    let bottom = -1.25 * m;
    let (wb, wt) = (half(bottom).max(0.02 * m), half(top));
    let first = ((from - wt - centre) / pitch - 0.5).ceil() as i64;
    let last = ((to + wt - centre) / pitch - 0.5).floor() as i64;
    (first..=last)
        .map(|k| {
            let c = centre + (k as f64 + 0.5) * pitch;
            let p = [
                [c - wb, bottom],
                [c + wb, bottom],
                [c + wt, top],
                [c - wt, top],
            ];
            Region2 {
                outer: (0..4)
                    .map(|i| Seg::Line {
                        a: p[i],
                        b: p[(i + 1) % 4],
                    })
                    .collect(),
                holes: Vec::new(),
            }
        })
        .collect()
}

/// A rack's sweeps: its spaces across `width` from `frame` (x along the rack, its normal across
/// it), over `from..to` along it, the tips' line at `top` over the pitch line.
fn rack_sweeps(
    x: &GearFeature,
    frame: Frame,
    from: f64,
    to: f64,
    width: f64,
    top: f64,
) -> Vec<Sweep> {
    let lean = x.beta().tan();
    let centre = (from + to) / 2.0;
    let piece = |at: f64, across: f64, lean: f64, shift: f64| {
        // The teeth lean along the rack as they cross it: spaces enough for both sides.
        let reach = (across * lean).abs();
        let f = frame.raised(at);
        Sweep {
            frame: f,
            regions: rack_spaces(x, from - reach, to + reach, top, centre + shift),
            travel: add(scale(f.normal, across), scale(f.x, across * lean)),
            twist: 0.0,
            scale: 1.0,
            spiral: false,
        }
    };
    if x.style != ToothStyle::Herringbone {
        return vec![piece(0.0, width, lean, 0.0)];
    }
    let h = width / 2.0;
    vec![piece(0.0, h, lean, 0.0), piece(h, h, -lean, h * lean)]
}

fn rack(x: &GearFeature) -> Result<Plan, &'static str> {
    let m = x.module;
    let own = own_frame(x);
    let (l, top, bottom) = (x.length / 2.0, m, -(1.25 * m + x.height));
    Ok(Plan {
        blank: Some(Blank::Extrude {
            frame: own,
            polygon: vec![[-l, bottom], [l, bottom], [l, top], [-l, top]],
            depth: x.width,
        }),
        // Drawn past the tips, so the cut leaves none of the bar's top between the teeth.
        sweeps: rack_sweeps(x, own, -l, l, x.width, top + 0.25 * m),
        lofts: Vec::new(),
        module: m,
    })
}

fn on_plane(x: &GearFeature, t: &Target) -> Result<Plan, &'static str> {
    let Target::Plane {
        origin,
        normal,
        along,
        length,
        width,
    } = *t
    else {
        return Err("Select a flat face");
    };
    let m = x.module;
    let across = cross(normal, along);
    // The rack runs along the face's long side, or (flipped) its short one. Its frame: x
    // along the rack, y the face's normal, so its normal (across the rack) is x × y: it starts
    // from the corner that has the face ahead of it both ways.
    let (fx, origin, length, width) = if x.flip {
        (across, origin, width, length)
    } else {
        (along, add(origin, scale(across, width)), length, width)
    };
    if length < PI * m {
        return Err("The face is shorter than one tooth");
    }
    // The pitch line is a module under the face (its teeth's tips).
    let frame = Frame {
        origin: sub(origin, scale(normal, m)),
        x: fx,
        normal: cross(fx, normal),
    };
    Ok(Plan {
        blank: None,
        sweeps: rack_sweeps(x, frame, 0.0, length, width, 1.25 * m),
        lofts: Vec::new(),
        module: m,
    })
}

// ---------------------------------------------------------------------------------------------

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn scale(a: Vec3, k: f64) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn norm(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: Vec3) -> Vec3 {
    let l = norm(a);
    if l < 1e-15 { a } else { scale(a, 1.0 / l) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
    }

    /// The loop closes: every curve starts where the one before ends.
    fn closed(l: &[Seg]) {
        for i in 0..l.len() {
            let (a, b) = (l[i].ends().1, l[(i + 1) % l.len()].ends().0);
            assert!(
                dist(a, b) < 1e-9,
                "curve {i} ends at {a:?}, the next starts at {b:?}"
            );
        }
    }

    /// A spline's point at `u`.
    fn spline_at(knots: &[f64], poles: &[[f64; 2]], u: f64) -> [f64; 2] {
        let (span, n) = basis(knots, poles.len(), u);
        let mut p = [0.0; 2];
        for j in 0..4 {
            p[0] += n[j] * poles[span - 3 + j][0];
            p[1] += n[j] * poles[span - 3 + j][1];
        }
        p
    }

    #[test]
    fn a_spur_gears_outline_closes_on_its_circles() {
        let x = GearFeature::default();
        let s = spur_section(&x, 2.0);
        let l = s.outline();
        closed(&l);
        // 20 teeth: a root arc, two fillets, two lines, two flanks and a tip arc each.
        assert_eq!(l.len(), 20 * 8);
        let far = l
            .iter()
            .map(|c| dist(c.ends().0, [0.0, 0.0]))
            .fold(0.0, f64::max);
        assert!((far - 22.0).abs() < 1e-9, "{far}");
    }

    #[test]
    fn a_flanks_spline_lies_on_the_involute() {
        let x = GearFeature::default();
        let s = spur_section(&x, 2.0);
        let Flank::Involute { base, half } = s.flank else {
            panic!()
        };
        let Seg::Spline { knots, poles } = s.flank.spline(base, s.tip, 0.0, 1.0) else {
            panic!()
        };
        let mut worst: f64 = 0.0;
        for i in 0..=400 {
            let p = spline_at(&knots, &poles, i as f64 / 400.0 * 0.999999);
            let rho = dist(p, [0.0, 0.0]);
            let want = half - inv((base / rho.max(base)).acos());
            worst = worst.max((p[1].atan2(p[0]) - want).abs() * rho);
        }
        assert!(worst < 2e-5, "{worst} mm off the involute");
    }

    #[test]
    fn the_tooth_is_half_the_pitch_thick_at_the_pitch_circle() {
        let x = GearFeature::default();
        let s = spur_section(&x, 2.0);
        assert!((s.flank.psi(20.0) * 20.0 * 2.0 - PI).abs() < 1e-12);
    }

    #[test]
    fn spaces_close_and_reach_past_the_tips() {
        let x = GearFeature::default();
        let s = spur_section(&x, 2.0);
        let spaces = s.spaces(22.5, true);
        assert_eq!(spaces.len(), 20);
        for r in &spaces {
            closed(&r.outer);
        }
        // A space's wall, one curve, follows the flank, fillet, root, fillet and flank it is
        // drawn through: every point of it within a micron of them.
        let foot = s.foot();
        let Seg::Spline { knots, poles } = s.wall(&foot, 22.5, 0.0) else {
            panic!()
        };
        let exact: Vec<Seg> = s
            .flank(&foot, 22.5, 0.0, 1.0, false)
            .into_iter()
            .chain([Seg::Arc {
                center: [0.0, 0.0],
                radius: s.root,
                start: foot.psi_root,
                sweep: s.pitch() - 2.0 * foot.psi_root,
            }])
            .chain(s.flank(&foot, 22.5, s.pitch(), -1.0, true))
            .collect();
        let mut dense = Vec::new();
        for c in &exact {
            for k in 0..2000 {
                let u = k as f64 / 2000.0;
                dense.push(match c {
                    Seg::Line { a, b } => [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u],
                    Seg::Arc {
                        center,
                        radius,
                        start,
                        sweep,
                    } => {
                        let t = start + sweep * u;
                        [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
                    }
                    Seg::Spline { knots, poles } => spline_at(knots, poles, u * 0.999999),
                    Seg::Circle { .. } => unreachable!(),
                });
            }
        }
        dense.push(exact[exact.len() - 1].ends().1);
        let (mut worst, mut at) = (0.0f64, (0.0, 0.0));
        for k in 0..=600 {
            let p = spline_at(&knots, &poles, k as f64 / 600.0 * 0.999999);
            // (To the polyline through the points, not the points: they are microns apart.)
            let near = dense
                .windows(2)
                .map(|w| {
                    let (d, e) = (
                        [w[1][0] - w[0][0], w[1][1] - w[0][1]],
                        [p[0] - w[0][0], p[1] - w[0][1]],
                    );
                    let l2 = d[0] * d[0] + d[1] * d[1];
                    let t = if l2 > 0.0 {
                        ((d[0] * e[0] + d[1] * e[1]) / l2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    dist(p, [w[0][0] + t * d[0], w[0][1] + t * d[1]])
                })
                .fold(f64::MAX, f64::min);
            if near > worst {
                worst = near;
                at = (dist(p, [0.0, 0.0]), p[1].atan2(p[0]));
            }
        }
        assert!(worst < 1e-3, "{worst} mm off the wall at {at:?}");
        // Far enough out the teeth have come to a point: one ring round the gear.
        let ring = s.spaces(26.0, true);
        assert_eq!(ring.len(), 1);
        closed(&ring[0].holes[0]);
    }

    #[test]
    fn a_bevel_gears_spaces_close() {
        let x = GearFeature {
            gear_type: GearType::Bevel,
            ..Default::default()
        };
        let p = plan(&x, None).unwrap();
        assert!(p.blank.is_some());
        let s = &p.sweeps[0];
        assert_eq!(s.regions.len(), 20);
        for r in &s.regions {
            closed(&r.outer);
        }
        assert!(s.scale > 0.0 && s.scale < 1.0);
    }

    #[test]
    fn a_spherical_involute_is_the_plane_one_on_a_narrow_cone() {
        // A cone of 0.01 rad: its sphere is nearly flat there.
        let (delta, alpha) = (0.01f64, 20f64.to_radians());
        let base = (delta.sin() * alpha.cos()).asin();
        let got = sph_inv(delta, base);
        assert!(
            (got - inv(alpha)).abs() < 1e-4 * inv(alpha),
            "{got} {}",
            inv(alpha)
        );
    }

    #[test]
    fn the_module_follows_from_the_face() {
        let mut x = GearFeature::default();
        let cyl = Target::Cylinder {
            start: [0.0; 3],
            dir: [0.0, 0.0, 1.0],
            radius: 22.0,
            height: 10.0,
            external: true,
        };
        x.fit(&cyl);
        assert_eq!((x.gear_type, x.teeth), (GearType::Spur, 20));
        assert!((x.module_on(&cyl).unwrap() - 2.0).abs() < 1e-12);
        let hole = Target::Cylinder {
            start: [0.0; 3],
            dir: [0.0, 0.0, 1.0],
            radius: 38.0,
            height: 10.0,
            external: false,
        };
        x.fit(&hole);
        assert_eq!((x.gear_type, x.teeth), (GearType::Ring, 40));
        assert!((x.module_on(&hole).unwrap() - 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_cones_pitch_angle_is_under_its_face_angle() {
        let d = pitch_angle_of_face(45f64.to_radians(), 20.0).unwrap();
        assert!((d + (2.0 * d.sin() / 20.0).atan() - 45f64.to_radians()).abs() < 1e-12);
        assert!(d < 45f64.to_radians());
    }

    #[test]
    fn a_face_mills_flank_touches_its_cone_on_the_pitch_line() {
        // 40 teeth of module 2 at 68.2°, 35° spiral, the cutter's radius the mean cone distance.
        let (delta, cone) = (68.2f64.to_radians(), 43.08);
        let (dedendum, mean) = ((2.5f64 / cone).atan(), cone - 6.0);
        let mill = FaceMill::new(delta, dedendum, mean, 35f64.to_radians(), mean, 2.2, 0.349);
        // The middle of the slot at the pitch line's middle point: either blade's flank is half
        // the slot from it there, a point on the gear's pitch cone near its x.
        for side in [1.0, -1.0] {
            let p = mill.point(side, mill.tau_mid, mill.level);
            let (s, polar) = (p[0].hypot(p[1]), p[1].atan2(p[0]));
            assert!((s - mean).abs() < 0.4 * 2.2, "{s} vs {mean}");
            assert!((polar - delta).abs() < 2e-2, "{polar} vs {delta}");
            assert!(
                (p[2] * p[1]).abs() < 0.75 * 2.2,
                "{} off the gear's x",
                p[2] * p[1]
            );
        }
        // A point of the flank found again from where it is.
        let p = mill.point(1.0, mill.tau_mid + 0.05, mill.level + 0.7);
        let x = mill
            .solve(1.0, p[0], p[1], None)
            .expect("the point is found");
        let q = mill.point(1.0, x[0], x[1]);
        assert!((q[0] - p[0]).abs() < 1e-8 && (q[1] - p[1]).abs() < 1e-8);
        assert!((q[2] - p[2]).abs() < 1e-8, "{} vs {}", q[2], p[2]);
    }

    #[test]
    fn a_face_milled_bevel_has_a_loft_a_space() {
        let x = GearFeature {
            gear_type: GearType::Bevel,
            style: ToothStyle::Helical,
            helix_angle: 35.0,
            ..Default::default()
        };
        let p = plan(&x, None).unwrap();
        assert!(p.sweeps.is_empty());
        assert_eq!(p.lofts.len(), 20);
        for (_, r) in &p.lofts[0].sections {
            closed(&r.outer);
            assert_eq!(r.outer.len(), 4);
        }
        // Both spirals lean the same way for the same hand: toward the toe the slot has
        // turned the way the equiangular spiral has.
        let slot = |s: &(Frame, Region2)| {
            let (a, b) = (s.1.outer[0].ends().0, s.1.outer[2].ends().1);
            ((a[1] + b[1]) / 2.0).atan2((a[0] + b[0]) / 2.0)
        };
        let l = &p.lofts[0].sections;
        let leaned = slot(&l[l.len() - 1]) - slot(&l[0]);
        let y = GearFeature {
            spiral: SpiralKind::Equiangular,
            ..x
        };
        let q = plan(&y, None).unwrap();
        assert!(
            q.sweeps[0].twist * leaned > 0.0,
            "{} {leaned}",
            q.sweeps[0].twist
        );
    }

    #[test]
    fn a_racks_spaces_cover_it() {
        let x = GearFeature {
            gear_type: GearType::Rack,
            ..Default::default()
        };
        let p = plan(&x, None).unwrap();
        // 60 mm over a pitch of 2π: nine or ten spaces.
        let n = p.sweeps[0].regions.len();
        assert!((9..=11).contains(&n), "{n}");
    }
}
