//! Camera math for the Part Studio viewport, kept free of ECS so it can be unit-tested.
//!
//! The view is orthographic by default. It is described by an azimuth and an elevation (the
//! direction from the focus point toward the camera), a roll about the view direction, the focus
//! point (the world point shown at the center of the viewport) and a zoom (`scale`, world mm per
//! logical pixel).
//!
//! P3E.3a (TD6.5): **Perspective view** keeps the same description. The eye sits
//! [`FOCAL_PX`]` × scale` mm in front of the focus, so the focus plane still shows `scale` mm per
//! pixel: unprojecting onto it, panning and zooming about the cursor work as in orthographic,
//! and nearer points are drawn larger ([`ViewState::project`]). Pick rays start at the eye and
//! spread ([`ViewState::ray`]). A view also carries its tab's [`RenderMode`]. Standard views have no roll (Z up); a right-drag rotates freely about the screen
//! axes, as Onshape does by default, and Alt+right-drag turns without roll.
//!
//! - azimuth 0, elevation 0 looks at the Front plane (from -Y), azimuth 90° from +X (Right),
//!   elevation 90° straight down at Top.
//! - Screen offsets are in logical pixels from the viewport center, x to the right, y down.

use bevy::math::{Mat3, Quat, Vec2, Vec3};

/// How far the camera sits from the focus point along the view direction (mm). The projection is
/// orthographic, so this only has to keep the scene between the near and far planes.
pub const CAMERA_DISTANCE: f32 = 20_000.0;
/// The far plane of the main camera.
pub const CAMERA_FAR: f32 = 40_000.0;
/// Onshape's default zoom: a 150 mm default plane measures about 592 px on screen (so a
/// normal-to sketch view shows about 3.95 px per mm: `screens/11`, 49.12 mm across 194 px).
pub const DEFAULT_SCALE: f32 = 150.0 / 592.0;
/// Degrees of rotation per pixel of right-drag.
pub const ORBIT_DEG_PER_PX: f32 = 0.4;
/// Zoom factor per wheel line.
pub const ZOOM_PER_LINE: f32 = 1.15;
pub const MIN_SCALE: f32 = 1e-4;
pub const MAX_SCALE: f32 = 1e3;
/// Perspective view: the focal length in logical pixels (the eye is this many pixels' worth of
/// the focus plane's scale in front of it; about a 30° vertical field of view in an 900 px
/// viewport, a mild perspective like Onshape's).
pub const FOCAL_PX: f32 = 1700.0;

/// How the parts are drawn (P3E.3a, TD6.5, PS2.9, X14): the view cube menu's render modes, per
/// tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum RenderMode {
    /// Shaded with edges (the default).
    #[default]
    Shaded,
    ShadedWithoutEdges,
    /// Shaded, and the edges hidden behind faces drawn faintly through them.
    ShadedWithHiddenEdges,
    /// White faces with their visible edges: a hidden-line drawing.
    HiddenEdgesRemoved,
    /// White faces, the visible edges black and the hidden ones grey.
    HiddenEdgesVisible,
    /// See-through faces with every edge.
    Translucent,
}

impl RenderMode {
    pub const ALL: [RenderMode; 6] = [
        RenderMode::Shaded,
        RenderMode::ShadedWithoutEdges,
        RenderMode::ShadedWithHiddenEdges,
        RenderMode::HiddenEdgesRemoved,
        RenderMode::HiddenEdgesVisible,
        RenderMode::Translucent,
    ];

    /// The menu's label.
    pub fn label(self) -> &'static str {
        match self {
            RenderMode::Shaded => "Shaded with edges",
            RenderMode::ShadedWithoutEdges => "Shaded without edges",
            RenderMode::ShadedWithHiddenEdges => "Shaded with hidden edges",
            RenderMode::HiddenEdgesRemoved => "Hidden edges removed",
            RenderMode::HiddenEdgesVisible => "Hidden edges visible",
            RenderMode::Translucent => "Translucent",
        }
    }

    /// The menu item's name: `view-render-<slug>`.
    pub fn slug(self) -> &'static str {
        match self {
            RenderMode::Shaded => "shaded",
            RenderMode::ShadedWithoutEdges => "shaded-no-edges",
            RenderMode::ShadedWithHiddenEdges => "shaded-hidden-edges",
            RenderMode::HiddenEdgesRemoved => "hidden-removed",
            RenderMode::HiddenEdgesVisible => "hidden-visible",
            RenderMode::Translucent => "translucent",
        }
    }

    /// The faces are shaded (lit by the head light) rather than white.
    pub fn shaded(self) -> bool {
        !matches!(self, RenderMode::HiddenEdgesRemoved | RenderMode::HiddenEdgesVisible)
    }

    /// The visible edges are drawn.
    pub fn edges(self) -> bool {
        self != RenderMode::ShadedWithoutEdges
    }

    /// The edges behind faces are drawn too (faintly).
    pub fn hidden_edges(self) -> bool {
        matches!(self, RenderMode::ShadedWithHiddenEdges | RenderMode::HiddenEdgesVisible)
    }

    pub fn translucent(self) -> bool {
        self == RenderMode::Translucent
    }

    /// The menu group: the shaded modes (first ▸) or the hidden-line modes (second ▸).
    pub fn line_drawing(self) -> bool {
        !self.shaded()
    }
}

/// The standard orientations (view cube faces, Shift+1…7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardView {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Isometric,
    /// The view a new Part Studio opens with (Onshape's default trimetric-like view).
    Default,
}

impl StandardView {
    /// (azimuth, elevation) in degrees.
    pub fn angles(self) -> (f32, f32) {
        match self {
            StandardView::Front => (0.0, 0.0),
            StandardView::Back => (180.0, 0.0),
            StandardView::Left => (-90.0, 0.0),
            StandardView::Right => (90.0, 0.0),
            StandardView::Top => (0.0, 90.0),
            StandardView::Bottom => (0.0, -90.0),
            // atan(1/√2) = 35.264°: the three axes are foreshortened equally.
            StandardView::Isometric => (45.0, 35.264_39),
            StandardView::Default => (30.0, 30.0),
        }
    }
}

/// The orientation, center and zoom of the 3D view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewState {
    /// Degrees, around +Z, measured from the Front view toward the Right view.
    pub azimuth: f32,
    /// Degrees above the XY plane, in -90..=90.
    pub elevation: f32,
    /// Degrees the view is turned about the view direction (counter-clockwise on screen).
    pub roll: f32,
    /// The world point at the center of the viewport.
    pub focus: Vec3,
    /// World millimeters per logical pixel (at the focus, in perspective).
    pub scale: f32,
    /// Perspective view (P3E.3a); orthographic when false (the default).
    pub perspective: bool,
    /// How the parts are drawn (P3E.3a).
    pub render: RenderMode,
}

impl Default for ViewState {
    fn default() -> Self {
        Self::standard(StandardView::Default)
    }
}

impl ViewState {
    pub fn standard(view: StandardView) -> Self {
        let (azimuth, elevation) = view.angles();
        Self {
            azimuth,
            elevation,
            roll: 0.0,
            focus: Vec3::ZERO,
            scale: DEFAULT_SCALE,
            perspective: false,
            render: RenderMode::Shaded,
        }
    }

    /// The same focus and zoom, oriented like `view`.
    pub fn oriented(self, view: StandardView) -> Self {
        let (azimuth, elevation) = view.angles();
        Self {
            azimuth,
            elevation,
            roll: 0.0,
            ..self
        }
    }

    /// Unit vector from the focus toward the camera.
    pub fn back(&self) -> Vec3 {
        let (az, el) = (self.azimuth.to_radians(), self.elevation.to_radians());
        Vec3::new(az.sin() * el.cos(), -az.cos() * el.cos(), el.sin())
    }

    /// Screen right with no roll: horizontal.
    fn level_right(&self) -> Vec3 {
        let az = self.azimuth.to_radians();
        Vec3::new(az.cos(), az.sin(), 0.0)
    }

    /// Screen right, as a world direction (horizontal when there is no roll, so Z stays up).
    pub fn right(&self) -> Vec3 {
        let r0 = self.level_right();
        if self.roll == 0.0 {
            return r0;
        }
        let u0 = self.back().cross(r0);
        let (s, c) = self.roll.to_radians().sin_cos();
        r0 * c + u0 * s
    }

    /// Screen up, as a world direction.
    pub fn up(&self) -> Vec3 {
        self.back().cross(self.right())
    }

    /// The view with the camera frame (right, up, back) = `q` applied to (X, Y, Z).
    fn with_rotation(self, q: Quat) -> Self {
        let back = (q * Vec3::Z).normalize();
        let right = (q * Vec3::X).normalize();
        let elevation = back.z.clamp(-1.0, 1.0).asin().to_degrees();
        let (azimuth, roll) = if back.z.abs() > 0.99999 {
            // Straight up or down: the azimuth takes the whole turn.
            let az = right.y.atan2(right.x).to_degrees();
            (az, 0.0)
        } else {
            let az = back.x.atan2(-back.y).to_degrees();
            let v = Self {
                azimuth: az,
                elevation,
                roll: 0.0,
                ..self
            };
            let (r0, u0) = (v.level_right(), v.up());
            (az, right.dot(u0).atan2(right.dot(r0)).to_degrees())
        };
        Self {
            azimuth: wrap_degrees(azimuth),
            elevation,
            roll: wrap_degrees(roll),
            ..self
        }
    }

    /// Rotates the view about its own screen axes: positive `yaw` turns the model to the
    /// right (about screen up), positive `pitch` tips its top toward the viewer (about screen
    /// right). Degrees.
    pub fn rotate_screen(&mut self, yaw: f32, pitch: f32) {
        let q = Quat::from_axis_angle(self.up(), -yaw.to_radians())
            * Quat::from_axis_angle(self.right(), -pitch.to_radians())
            * self.rotation();
        *self = self.with_rotation(q);
    }

    /// The camera's rotation (its local -Z looks along `-back`, local +Y is `up`).
    pub fn rotation(&self) -> Quat {
        Quat::from_mat3(&Mat3::from_cols(self.right(), self.up(), self.back()))
    }

    /// In perspective, how far the eye is in front of the focus (mm).
    pub fn eye_distance(&self) -> f32 {
        FOCAL_PX * self.scale
    }

    pub fn camera_position(&self) -> Vec3 {
        if self.perspective {
            return self.focus + self.back() * self.eye_distance();
        }
        self.focus + self.back() * CAMERA_DISTANCE
    }

    /// In perspective, how much larger than on the focus plane a point `p` is drawn (1 on the
    /// focus plane, more nearer the eye); 1 in orthographic.
    pub fn magnification(&self, p: Vec3) -> f32 {
        if !self.perspective {
            return 1.0;
        }
        let eye = self.eye_distance();
        let depth = eye - (p - self.focus).dot(self.back());
        eye / depth.max(eye * 1e-3)
    }

    /// Screen offset (px from the viewport center, y down) of a world point.
    pub fn project(&self, p: Vec3) -> Vec2 {
        let d = p - self.focus;
        Vec2::new(d.dot(self.right()), -d.dot(self.up())) / self.scale * self.magnification(p)
    }

    /// Projected screen-space vector (px, y down) of a world direction or offset (as seen at the
    /// focus, in perspective).
    pub fn project_vector(&self, v: Vec3) -> Vec2 {
        Vec2::new(v.dot(self.right()), -v.dot(self.up())) / self.scale
    }

    /// The world point on the plane through the focus (facing the camera) under a screen offset
    /// (in perspective too: the focus plane shows `scale` mm per pixel).
    pub fn unproject(&self, offset: Vec2) -> Vec3 {
        self.focus + (self.right() * offset.x - self.up() * offset.y) * self.scale
    }

    /// The pick ray through a screen offset: origin (on the camera plane, or the eye in
    /// perspective) and unit direction.
    pub fn ray(&self, offset: Vec2) -> (Vec3, Vec3) {
        if self.perspective {
            let eye = self.camera_position();
            return (eye, (self.unproject(offset) - eye).normalize());
        }
        (
            self.unproject(offset) + self.back() * CAMERA_DISTANCE,
            -self.back(),
        )
    }

    /// Right-drag orbit (Onshape's default): free rotation about the screen axes. Dragging
    /// right turns the model to the right; dragging down tips its top toward the viewer. The
    /// focus stays fixed.
    pub fn orbit(&mut self, delta_px: Vec2) {
        self.rotate_screen(delta_px.x * ORBIT_DEG_PER_PX, delta_px.y * ORBIT_DEG_PER_PX);
    }

    /// Alt+right-drag: turntable rotation without roll: horizontal motion turns about world Z,
    /// vertical motion tilts, and Z stays up (any roll is removed).
    pub fn orbit_turntable(&mut self, delta_px: Vec2) {
        self.azimuth = wrap_degrees(self.azimuth - delta_px.x * ORBIT_DEG_PER_PX);
        self.elevation = (self.elevation + delta_px.y * ORBIT_DEG_PER_PX).clamp(-90.0, 90.0);
        self.roll = 0.0;
    }

    /// Turns the view with `turn` (one of the orbits above) about the world point `pivot`
    /// instead of the focus: the pivot stays where it is on screen and the view swings
    /// around it.
    pub fn orbit_about(&mut self, pivot: Vec3, turn: impl FnOnce(&mut Self)) {
        let before = self.rotation();
        turn(self);
        let q = self.rotation() * before.inverse();
        self.focus = pivot + q * (self.focus - pivot);
    }

    /// Rotates by fixed angles about the screen axes (view cube arrows, arrow keys): positive
    /// `yaw` turns the model to the right, positive `pitch` tips its top toward the viewer.
    pub fn rotate_by(&mut self, yaw: f32, pitch: f32) {
        self.rotate_screen(yaw, pitch);
    }

    /// Middle-drag pan: the scene follows the pointer.
    pub fn pan(&mut self, delta_px: Vec2) {
        self.focus -= (self.right() * delta_px.x - self.up() * delta_px.y) * self.scale;
    }

    /// Zooms by `factor` (> 1 zooms in) keeping the world point under `cursor` (offset from the
    /// viewport center) where it is on screen.
    pub fn zoom_at(&mut self, factor: f32, cursor: Vec2) {
        let before = self.unproject(cursor);
        self.scale = (self.scale / factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.unproject(cursor);
        self.focus += before - after;
    }

    /// Mouse wheel: `lines` > 0 (away from the user) zooms in, toward the cursor.
    pub fn wheel(&mut self, lines: f32, cursor: Vec2) {
        self.zoom_at(ZOOM_PER_LINE.powf(lines), cursor);
    }

    /// The orientation looking straight at a plane with world normal `normal` from whichever
    /// side currently faces the viewer (Onshape's "Normal to").
    pub fn normal_to(self, normal: Vec3) -> Self {
        let n = if self.back().dot(normal) < 0.0 {
            -normal
        } else {
            normal
        }
        .normalize();
        let elevation = n.z.clamp(-1.0, 1.0).asin().to_degrees();
        let azimuth = if n.z.abs() > 0.9999 {
            // Straight up or down: keep X to the right, like the Top view.
            0.0
        } else {
            n.x.atan2(-n.y).to_degrees()
        };
        Self {
            azimuth,
            elevation,
            roll: 0.0,
            ..self
        }
    }

    /// Zoom to fit (F): the same orientation, centered on `points` and zoomed so they fill
    /// `fill` (0..1) of `viewport` (logical px). Leaves the view as it is with no points.
    pub fn fitted(self, points: &[Vec3], viewport: Vec2, fill: f32) -> Self {
        let Some(first) = points.first() else {
            return self;
        };
        // Bounds in the screen plane, in mm (right, up) relative to the current focus.
        let to_plane = |p: Vec3| {
            let d = p - self.focus;
            Vec2::new(d.dot(self.right()), d.dot(self.up()))
        };
        let (mut lo, mut hi) = (to_plane(*first), to_plane(*first));
        for p in points {
            let q = to_plane(*p);
            lo = lo.min(q);
            hi = hi.max(q);
        }
        let center = (lo + hi) / 2.0;
        let extent = (hi - lo).max(Vec2::splat(1e-3));
        let avail = (viewport * fill.clamp(0.05, 1.0)).max(Vec2::ONE);
        let scale = (extent.x / avail.x).max(extent.y / avail.y).clamp(MIN_SCALE, MAX_SCALE);
        let mut out = Self {
            focus: self.focus + self.right() * center.x + self.up() * center.y,
            scale,
            ..self
        };
        // The focus on the points' middle in depth too: the view orbits about the focus, so
        // it turns about the middle of what was fitted, not a point in front of or behind it.
        let depths = points.iter().map(|p| (*p - out.focus).dot(out.back()));
        let (dlo, dhi) = depths.fold((f32::MAX, f32::MIN), |(a, b), d| (a.min(d), b.max(d)));
        out.focus += out.back() * (dlo + dhi) / 2.0;
        if self.perspective {
            // A few rounds of measuring the drawn (magnified) box and correcting the zoom and
            // centre.
            for _ in 0..6 {
                let (mut plo, mut phi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                for p in points {
                    let q = out.project(*p);
                    plo = plo.min(q);
                    phi = phi.max(q);
                }
                let size = (phi - plo).max(Vec2::splat(1e-3));
                let k = (size.x / avail.x).max(size.y / avail.y);
                let mid = (plo + phi) / 2.0;
                out.focus += (out.right() * mid.x - out.up() * mid.y) * out.scale;
                out.scale = (out.scale * k).clamp(MIN_SCALE, MAX_SCALE);
            }
        }
        out
    }

    /// [`Self::fitted`] into the part of the viewport right of `left_inset` px (a feature
    /// dialog floating over its left side): the points are centred in what stays visible.
    pub fn fitted_beside(self, points: &[Vec3], viewport: Vec2, fill: f32, left_inset: f32) -> Self {
        let inset = left_inset.clamp(0.0, viewport.x * 0.6);
        let mut to = self.fitted(points, Vec2::new(viewport.x - inset, viewport.y), fill);
        if !points.is_empty() {
            to.pan(Vec2::new(inset / 2.0, 0.0));
        }
        to
    }

    /// Z / Shift+Z: zooms out (`factor` < 1) or in about the viewport center.
    pub fn zoomed(self, factor: f32) -> Self {
        Self {
            scale: (self.scale / factor).clamp(MIN_SCALE, MAX_SCALE),
            ..self
        }
    }

    /// Interpolates toward `to` (t in 0..=1) along the shortest azimuth path.
    pub fn lerp(&self, to: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let daz = wrap_degrees(to.azimuth - self.azimuth);
        // Zoom interpolates geometrically so it feels uniform.
        let scale = self.scale * (to.scale / self.scale).powf(t);
        let droll = wrap_degrees(to.roll - self.roll);
        Self {
            azimuth: wrap_degrees(self.azimuth + daz * t),
            elevation: self.elevation + (to.elevation - self.elevation) * t,
            roll: wrap_degrees(self.roll + droll * t),
            focus: self.focus.lerp(to.focus, t),
            scale,
            perspective: to.perspective,
            render: to.render,
        }
    }

    /// True if both views look the same (angles within 0.01°).
    pub fn approx_eq(&self, other: &Self) -> bool {
        wrap_degrees(self.azimuth - other.azimuth).abs() < 0.01
            && (self.elevation - other.elevation).abs() < 0.01
            && wrap_degrees(self.roll - other.roll).abs() < 0.01
            && self.focus.distance(other.focus) < 1e-3
            && (self.scale / other.scale - 1.0).abs() < 1e-4
            && self.perspective == other.perspective
            && self.render == other.render
    }

    /// In perspective, the same picture with the focus moved along the view axis to the depth
    /// of `p` (the eye stays; the focus plane, and so zooming about the cursor, then passes
    /// through `p`). Unchanged in orthographic, or for a point behind the eye.
    pub fn refocused(self, p: Vec3) -> Self {
        if !self.perspective {
            return self;
        }
        let eye = self.camera_position();
        let d = (eye - p).dot(self.back());
        if d <= self.eye_distance() * 1e-3 {
            return self;
        }
        Self {
            focus: eye - self.back() * d,
            scale: (d / FOCAL_PX).clamp(MIN_SCALE, MAX_SCALE),
            ..self
        }
    }

    /// Zoom to window (P3E.3a): the view zoomed so the screen box from `a` to `b` (offsets from
    /// the viewport center) fills the viewport, keeping the orientation. A box smaller than a few
    /// pixels leaves the view as it is.
    pub fn zoomed_to_box(self, a: Vec2, b: Vec2, viewport: Vec2) -> Self {
        let size = (b - a).abs();
        if size.x < 4.0 || size.y < 4.0 {
            return self;
        }
        let center = (a + b) / 2.0;
        let k = (size.x / viewport.x.max(1.0)).max(size.y / viewport.y.max(1.0));
        Self {
            focus: self.unproject(center),
            scale: (self.scale * k).clamp(MIN_SCALE, MAX_SCALE),
            ..self
        }
    }
}

/// Wraps an angle in degrees into -180..180.
pub fn wrap_degrees(a: f32) -> f32 {
    let mut a = (a + 180.0).rem_euclid(360.0) - 180.0;
    if a <= -180.0 {
        a += 360.0;
    }
    a
}

/// Smooth ease for view animations.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Intersects a ray with a square of half-size `half` centered at `center`, spanned by the unit
/// vectors `u` and `v`. Returns the distance along the ray.
pub fn ray_square(
    origin: Vec3,
    dir: Vec3,
    center: Vec3,
    u: Vec3,
    v: Vec3,
    half: f32,
) -> Option<f32> {
    let n = u.cross(v);
    let denom = dir.dot(n);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (center - origin).dot(n) / denom;
    if t < 0.0 {
        return None;
    }
    let p = origin + dir * t - center;
    (p.dot(u).abs() <= half && p.dot(v).abs() <= half).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    /// A3.5: the view cube's named views are the drawings' named views, so an assembly's (or a
    /// Part Studio's) orientation on the cube is the orientation its drawing views get.
    #[test]
    fn view_cube_views_are_the_drawing_views() {
        use cadrs_drawing::NamedView;
        for (cube, drawing) in [
            (StandardView::Front, NamedView::Front),
            (StandardView::Back, NamedView::Back),
            (StandardView::Left, NamedView::Left),
            (StandardView::Right, NamedView::Right),
            (StandardView::Top, NamedView::Top),
            (StandardView::Bottom, NamedView::Bottom),
            (StandardView::Isometric, NamedView::Isometric),
        ] {
            let v = ViewState::standard(cube);
            let f = drawing.frame();
            let sight = Vec3::new(f.dir[0] as f32, f.dir[1] as f32, f.dir[2] as f32);
            let right = Vec3::new(f.x[0] as f32, f.x[1] as f32, f.x[2] as f32);
            assert!(close(-v.back(), sight.normalize()), "{cube:?}: sight {:?} vs {sight:?}", -v.back());
            assert!(close(v.right(), right.normalize()), "{cube:?}: right {:?} vs {right:?}", v.right());
        }
    }

    #[test]
    fn basis_is_orthonormal_and_z_up() {
        for (az, el) in [(30.0, 30.0), (-120.0, 80.0), (45.0, -35.0), (0.0, 90.0)] {
            let v = ViewState {
                azimuth: az,
                elevation: el,
                ..ViewState::default()
            };
            // (Z up holds without roll.)
            let (r, u, b) = (v.right(), v.up(), v.back());
            assert!((r.length() - 1.0).abs() < 1e-5);
            assert!((u.length() - 1.0).abs() < 1e-5);
            assert!(r.dot(u).abs() < 1e-5 && r.dot(b).abs() < 1e-5 && u.dot(b).abs() < 1e-5);
            assert!(r.z.abs() < 1e-6, "right must stay horizontal");
            assert!(u.z >= -1e-6, "Z must never point down on screen");
            let q = v.rotation();
            assert!(close(q * Vec3::Z, b));
            assert!(close(q * Vec3::Y, u));
        }
    }

    #[test]
    fn default_view_matches_onshape() {
        let v = ViewState::default();
        // Z points straight up on screen, X down to the right, Y up to the right.
        let z = v.project_vector(Vec3::Z);
        assert!(z.x.abs() < 1e-3 && z.y < 0.0);
        let x = v.project_vector(Vec3::X);
        assert!(x.x > 0.0 && x.y > 0.0);
        let y = v.project_vector(Vec3::Y);
        assert!(y.x > 0.0 && y.y < 0.0);
        // A 150 mm plane edge along Z is about 512 px tall, as in the reference capture.
        assert!((v.project_vector(Vec3::Z * 150.0).length() - 512.0).abs() < 2.0);
    }

    #[test]
    fn turntable_orbit_keeps_z_up_and_focus() {
        let mut v = ViewState {
            focus: Vec3::new(5.0, -3.0, 2.0),
            ..ViewState::default()
        };
        v.orbit(Vec2::new(40.0, 25.0));
        let focus = v.focus;
        for d in [
            Vec2::new(120.0, 0.0),
            Vec2::new(0.0, 500.0),
            Vec2::new(-300.0, -900.0),
            Vec2::new(40.0, 30.0),
        ] {
            v.orbit_turntable(d);
            assert!(v.right().z.abs() < 1e-6);
            assert!(v.up().z >= -1e-6);
            assert!((-90.0..=90.0).contains(&v.elevation));
            assert_eq!(v.focus, focus);
            assert!(v.project(focus).length() < 1e-3);
        }
        let mut w = ViewState::default();
        w.orbit_turntable(Vec2::new(50.0, 0.0));
        assert!(w.azimuth < 30.0);
    }

    #[test]
    fn zoom_to_fit_centres_the_focus_in_depth() {
        // A board far from the origin: after F the view orbits about its middle, not a point
        // on the line of sight in front of or behind it.
        let mid = Vec3::new(100.0, -80.0, 0.8);
        let pts: Vec<Vec3> = (0..8).map(|i| mid + Vec3::new(15.0, 20.0, 0.8) * Vec3::new([-1.0, 1.0][i & 1], [-1.0, 1.0][(i >> 1) & 1], [-1.0, 1.0][i >> 2])).collect();
        // (In perspective the drawn box is centred, which is a few mm off its middle.)
        for (perspective, tol) in [(false, 0.01), (true, 5.0)] {
            let v = ViewState { perspective, ..ViewState::default() }.fitted(&pts, Vec2::new(1200.0, 800.0), 0.8);
            assert!((v.focus - mid).length() < tol, "{perspective}: {:?}", v.focus);
        }
    }

    #[test]
    fn orbit_about_a_pivot_keeps_it_in_place() {
        let mut v = ViewState { focus: Vec3::new(100.0, -80.0, 0.0), ..ViewState::default() };
        let pivot = Vec3::new(110.0, -72.0, 3.0);
        let on_screen = v.project(pivot);
        let depth = (pivot - v.focus).dot(v.back());
        v.orbit_about(pivot, |v| v.orbit(Vec2::new(70.0, -40.0)));
        assert!((v.project(pivot) - on_screen).length() < 1e-2);
        assert!(((pivot - v.focus).dot(v.back()) - depth).abs() < 1e-3);
        v.orbit_about(pivot, |v| v.orbit_turntable(Vec2::new(-50.0, 30.0)));
        assert!((v.project(pivot) - on_screen).length() < 1e-2);
        // Without a pivot the focus stays put and the pivot moves on screen.
        let mut w = v;
        w.orbit(Vec2::new(70.0, 0.0));
        assert!((w.project(pivot) - on_screen).length() > 1.0);
    }

    #[test]
    fn free_orbit_rotates_about_the_screen_axes() {
        let v = ViewState::default();
        // A horizontal drag turns about screen up: that axis stays put on screen.
        let mut a = v;
        a.orbit(Vec2::new(60.0, 0.0));
        assert!(a.up().distance(v.up()) < 1e-4);
        assert!(a.right().distance(v.right()) > 0.1);
        // A vertical drag turns about screen right.
        let mut b = v;
        b.orbit(Vec2::new(0.0, 80.0));
        assert!(b.right().distance(v.right()) < 1e-4);
        // Drags compose into a rolled view whose basis stays orthonormal.
        let mut c = v;
        for d in [Vec2::new(200.0, 0.0), Vec2::new(0.0, 150.0), Vec2::new(-90.0, 40.0)] {
            c.orbit(d);
        }
        assert!(c.roll.abs() > 0.5, "{}", c.roll);
        let (r, u, k) = (c.right(), c.up(), c.back());
        assert!(r.dot(u).abs() < 1e-4 && r.dot(k).abs() < 1e-4 && (r.length() - 1.0).abs() < 1e-4);
        assert!(c.project(c.focus).length() < 1e-3);
        // Rotating 90° up four times comes back (no clamping at the poles).
        let mut e = v;
        for _ in 0..4 {
            e.rotate_by(0.0, 90.0);
        }
        assert!(e.back().distance(v.back()) < 1e-3 && e.right().distance(v.right()) < 1e-3);
        // 90° up from the front view looks straight down... at the model's top.
        let mut f = ViewState::standard(StandardView::Front);
        f.rotate_by(0.0, 90.0);
        assert!(f.back().distance(Vec3::Z) < 1e-4, "{:?}", f.back());
        // Turntable removes the roll again.
        c.orbit_turntable(Vec2::ZERO);
        assert!(c.right().z.abs() < 1e-6);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut v = ViewState::default();
        v.pan(Vec2::new(33.0, -12.0));
        let cursor = Vec2::new(210.0, -145.0);
        let p = v.unproject(cursor);
        for lines in [3.0, -1.5, 7.0] {
            v.wheel(lines, cursor);
            assert!((v.project(p) - cursor).length() < 1e-2, "{:?}", v.project(p));
        }
        // Wheel away from the user zooms in (fewer mm per pixel).
        let s = v.scale;
        v.wheel(1.0, cursor);
        assert!(v.scale < s);
    }

    #[test]
    fn pan_moves_the_scene_with_the_pointer() {
        let mut v = ViewState::default();
        let before = v.project(Vec3::ZERO);
        v.pan(Vec2::new(40.0, 25.0));
        assert!((v.project(Vec3::ZERO) - before - Vec2::new(40.0, 25.0)).length() < 1e-3);
    }

    #[test]
    fn standard_orientations() {
        let top = ViewState::standard(StandardView::Top);
        assert!(close(top.back(), Vec3::Z));
        assert!(close(top.right(), Vec3::X));
        assert!(close(top.up(), Vec3::Y));
        let front = ViewState::standard(StandardView::Front);
        assert!(close(front.back(), -Vec3::Y));
        assert!(close(front.up(), Vec3::Z));
        let right = ViewState::standard(StandardView::Right);
        assert!(close(right.back(), Vec3::X));
        assert!(close(right.right(), Vec3::Y));
        let iso = ViewState::standard(StandardView::Isometric);
        let b = iso.back();
        assert!((b.x - b.z).abs() < 1e-4 && (b.x + b.y).abs() < 1e-4 && b.z > 0.0);
        // All three axes are foreshortened equally.
        let l = |a: Vec3| iso.project_vector(a).length();
        assert!((l(Vec3::X) - l(Vec3::Y)).abs() < 1e-3 && (l(Vec3::X) - l(Vec3::Z)).abs() < 1e-3);
    }

    #[test]
    fn normal_to_faces_the_viewer() {
        let v = ViewState::default();
        let top = v.normal_to(Vec3::Z);
        assert!(close(top.back(), Vec3::Z) && close(top.up(), Vec3::Y));
        let front = v.normal_to(-Vec3::Y);
        assert!(close(front.back(), -Vec3::Y));
        let right = v.normal_to(Vec3::X);
        assert!(close(right.back(), Vec3::X));
        // From below, normal-to-Top looks up at the plane's other side.
        let below = ViewState {
            elevation: -20.0,
            ..v
        };
        assert!(close(below.normal_to(Vec3::Z).back(), -Vec3::Z));
        // Given either side of the normal, the result is the same.
        assert!(v.normal_to(Vec3::Y).approx_eq(&front));
        // Zoom and focus are kept.
        assert_eq!(top.scale, v.scale);
    }

    #[test]
    fn fitting_centers_and_fills() {
        let v = ViewState::standard(StandardView::Top);
        let pts = [Vec3::new(100.0, 50.0, 0.0), Vec3::new(140.0, 70.0, 0.0)];
        let f = v.fitted(&pts, Vec2::new(800.0, 600.0), 0.5);
        // Centered on the points' middle...
        assert!(f.project(Vec3::new(120.0, 60.0, 0.0)).length() < 1e-3);
        // ...and the 40 mm width fills half of the 800 px viewport.
        let w = f.project_vector(Vec3::X * 40.0).length();
        assert!((w - 400.0).abs() < 0.5, "{w}");
        // The orientation is kept, and no points leaves the view alone.
        assert_eq!((f.azimuth, f.elevation), (v.azimuth, v.elevation));
        assert!(v.fitted(&[], Vec2::splat(100.0), 0.5).approx_eq(&v));
        // Zooming in shows more pixels per mm.
        assert!(v.zoomed(1.25).scale < v.scale);
    }

    #[test]
    fn lerp_takes_the_short_way() {
        let a = ViewState {
            azimuth: 170.0,
            ..ViewState::default()
        };
        let b = ViewState {
            azimuth: -170.0,
            ..ViewState::default()
        };
        let m = a.lerp(&b, 0.5);
        assert!((m.azimuth.abs() - 180.0).abs() < 1e-3);
        assert!(a.lerp(&b, 1.0).approx_eq(&b));
    }

    /// P3E.3a (TD6.5, Risk "perspective breaks picking and zoom"): in perspective the focus
    /// plane keeps its scale, a point's pick ray passes through it, nearer points are drawn
    /// larger, zoom about the cursor keeps the point under it and a fit fills the view.
    #[test]
    fn perspective_projection_rays_zoom_and_fit() {
        let mut v = ViewState { perspective: true, ..ViewState::default() };
        v.pan(Vec2::new(20.0, -10.0));
        // On the focus plane, as in orthographic.
        let q = v.unproject(Vec2::new(120.0, -80.0));
        assert!((v.project(q) - Vec2::new(120.0, -80.0)).length() < 1e-2);
        // Every point lies on the ray through its projection.
        for p in [Vec3::new(30.0, -20.0, 45.0), Vec3::new(-60.0, 80.0, -5.0), Vec3::ZERO] {
            let (o, d) = v.ray(v.project(p));
            let along = (p - o).dot(d);
            assert!((o + d * along).distance(p) < 1e-2 * p.length().max(1.0), "{p:?}");
            assert!(along > 0.0);
        }
        // The eye is at the camera position; nearer is larger.
        assert!(v.ray(Vec2::ZERO).0.distance(v.camera_position()) < 1e-3);
        let near = v.focus + v.back() * 50.0;
        let far = v.focus - v.back() * 50.0;
        assert!(v.magnification(near) > 1.0 && v.magnification(far) < 1.0);
        let l = |c: Vec3| (v.project(c + v.right() * 10.0) - v.project(c)).length();
        assert!(l(near) > l(v.focus) && l(v.focus) > l(far));
        // Zoom about the cursor keeps the focus-plane point under it.
        let cursor = Vec2::new(-150.0, 90.0);
        let p = v.unproject(cursor);
        for lines in [2.0, -3.0, 5.0] {
            v.wheel(lines, cursor);
            assert!((v.project(p) - cursor).length() < 1e-2, "{:?}", v.project(p));
        }
        // A fit of a 100 mm cube fills 80% of the viewport, measured on the drawn corners.
        let pts: Vec<Vec3> = (0..8)
            .map(|i| Vec3::new((i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32) * 100.0)
            .collect();
        let size = Vec2::new(1000.0, 700.0);
        let f = v.fitted(&pts, size, 0.8);
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in &pts {
            lo = lo.min(f.project(*p));
            hi = hi.max(f.project(*p));
        }
        let fill = ((hi - lo) / size).max_element();
        assert!((fill - 0.8).abs() < 0.01, "{fill}");
        assert!(((hi + lo) / 2.0).length() < 1.0, "centred");
        // The eye stays outside the cube.
        assert!(pts.iter().all(|p| (f.camera_position() - *p).dot(f.back()) > 0.0));
        // Orthographic is unchanged by the flag's default.
        assert!(!ViewState::default().perspective);
        assert_eq!(ViewState::default().render, RenderMode::Shaded);
    }

    /// Refocusing in perspective keeps the picture (every point projects where it did) and
    /// puts the focus plane through the point; zooming about the cursor then keeps that point
    /// under the cursor.
    #[test]
    fn refocus_keeps_the_picture() {
        let mut v = ViewState { perspective: true, ..ViewState::default() };
        v.pan(Vec2::new(-40.0, 25.0));
        let p = Vec3::new(30.0, -20.0, 45.0);
        let r = v.refocused(p);
        for q in [Vec3::ZERO, p, Vec3::new(-50.0, 60.0, 10.0)] {
            assert!((r.project(q) - v.project(q)).length() < 1e-2, "{q:?}");
        }
        assert!((p - r.focus).dot(r.back()).abs() < 1e-3);
        let cursor = r.project(p);
        let mut z = r;
        z.wheel(4.0, cursor);
        assert!((z.project(p) - cursor).length() < 1e-2);
        // Orthographic: unchanged.
        assert!(ViewState::default().refocused(p).approx_eq(&ViewState::default()));
    }

    #[test]
    fn zoom_to_window_frames_the_box() {
        let v = ViewState::standard(StandardView::Top);
        let (a, b) = (Vec2::new(100.0, -50.0), Vec2::new(300.0, 50.0));
        let z = v.zoomed_to_box(a, b, Vec2::new(800.0, 600.0));
        // The box's centre is now the view's centre, and its 200 px width fills 800 px.
        assert!(z.project(v.unproject(Vec2::new(200.0, 0.0))).length() < 1e-2);
        let w = (z.project(v.unproject(b)) - z.project(v.unproject(a))).x;
        assert!((w - 800.0).abs() < 0.5, "{w}");
        // A click (no box) leaves it alone.
        assert!(v.zoomed_to_box(a, a + Vec2::ONE, Vec2::new(800.0, 600.0)).approx_eq(&v));
    }

    #[test]
    fn pick_ray_hits_squares() {
        let v = ViewState::standard(StandardView::Top);
        let (o, d) = v.ray(Vec2::new(10.0, -10.0));
        let t = ray_square(o, d, Vec3::ZERO, Vec3::X, Vec3::Y, 50.0).unwrap();
        let p = o + d * t;
        assert!(p.z.abs() < 1e-2 && p.x > 0.0 && p.y > 0.0);
        // Seen edge-on, the Front plane is not hit.
        assert!(ray_square(o, d, Vec3::ZERO, Vec3::X, Vec3::Z, 50.0).is_none());
        // Outside the square.
        let (o, d) = v.ray(Vec2::new(1000.0, 0.0));
        assert!(ray_square(o, d, Vec3::ZERO, Vec3::X, Vec3::Y, 50.0).is_none());
    }
}
