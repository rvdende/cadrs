//! Short ways to write the kernel's inputs: planes and axes from plain vectors, sketch curves
//! and outlines, profiles, paths, a helix, the motions of a swept solid, and the feature specs
//! in their common forms. Each is a constructor on the type it makes, so a model reads as what it
//! is (a rectangle on the Top plane, extruded; a screw of a turn and a half) and not as its
//! fields. Lengths are mm and angles radians, as everywhere in the kernel.

use crate::native::nurbs::NurbsSurface;
use crate::native::surface::Surface;
use crate::*;
use nalgebra::{Point2, Point3, Unit, Vector3, Vector4};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

type P2 = Point2<f64>;
type P3 = Point3<f64>;
type V3 = Vector3<f64>;

impl Plane {
    /// The plane through `origin` square to `normal`, its sketch x axis along `x_dir` (made
    /// square to the normal; it must not lie along it). The sketch y axis is `normal × x_dir`.
    pub fn new(origin: P3, x_dir: V3, normal: V3) -> Self {
        let n = Unit::new_normalize(normal);
        let x = Unit::new_normalize(x_dir - n.into_inner() * x_dir.dot(&n));
        Plane {
            origin,
            x_dir: x,
            normal: n,
        }
    }

    /// The plane z = `z`: sketch (x, y) the model's (x, y), normal +z (the Top plane, raised).
    pub fn xy(z: f64) -> Self {
        Plane::new(P3::new(0.0, 0.0, z), V3::x(), V3::z())
    }

    /// The plane y = `y`: sketch (x, y) the model's (x, z), normal −y (the Front plane, moved).
    pub fn xz(y: f64) -> Self {
        Plane::new(P3::new(0.0, y, 0.0), V3::x(), -V3::y())
    }

    /// The plane x = `x`: sketch (x, y) the model's (y, z), normal +x (the Right plane, moved).
    pub fn yz(x: f64) -> Self {
        Plane::new(P3::new(x, 0.0, 0.0), V3::y(), V3::x())
    }

    /// A plane through `origin` square to `normal`, with a sketch x axis of the kernel's
    /// choosing (the model's x made square to the normal, or its y where the normal is nearly
    /// along x).
    pub fn square_to(origin: P3, normal: V3) -> Self {
        let n = normal.normalize();
        let other = if n.x.abs() < 0.9 { V3::x() } else { V3::y() };
        Plane::new(origin, other, n)
    }

    /// A plane holding `axis`: its origin the axis's, its sketch y axis along the axis and its
    /// sketch x axis square to it (as [`Plane::square_to`] chooses one). A half section drawn
    /// on it with x ≥ 0 revolves about the axis into a solid.
    pub fn through_axis(axis: &Axis) -> Self {
        let d = axis.dir.into_inner();
        let other = if d.x.abs() < 0.9 { V3::x() } else { V3::y() };
        let x = (other - d * other.dot(&d)).normalize();
        // (y = normal × x is the axis, so the normal is x × axis.)
        Plane::new(axis.origin, x, x.cross(&d))
    }
}

impl Axis {
    /// The axis through `origin` along `dir` (of any length but nothing).
    pub fn new(origin: P3, dir: V3) -> Self {
        Axis {
            origin,
            dir: Unit::new_normalize(dir),
        }
    }

    /// The model's x axis.
    pub fn x() -> Self {
        Axis::new(P3::origin(), V3::x())
    }

    /// The model's y axis.
    pub fn y() -> Self {
        Axis::new(P3::origin(), V3::y())
    }

    /// The model's z axis.
    pub fn z() -> Self {
        Axis::new(P3::origin(), V3::z())
    }
}

/// The clamped knots of a Bézier curve of this degree on [0, 1].
pub fn bezier_knots(degree: usize) -> Vec<f64> {
    let mut knots = vec![0.0; degree + 1];
    knots.extend(vec![1.0; degree + 1]);
    knots
}

impl Curve2 {
    /// The segment from `a` to `b`.
    pub fn line(a: impl Into<P2>, b: impl Into<P2>) -> Self {
        Curve2::Line {
            a: a.into(),
            b: b.into(),
            source: None,
        }
    }

    /// The arc about `center` of `radius` from the angle `start`, sweeping `sweep`
    /// (counter-clockwise when positive).
    pub fn arc(center: impl Into<P2>, radius: f64, start: f64, sweep: f64) -> Self {
        Curve2::Arc {
            center: center.into(),
            radius,
            start_angle: start,
            sweep,
            source: None,
        }
    }

    /// The whole circle about `center`.
    pub fn circle(center: impl Into<P2>, radius: f64) -> Self {
        Curve2::Circle {
            center: center.into(),
            radius,
            source: None,
        }
    }

    /// The whole ellipse about `center` with semi-axes `a` along the sketch's x and `b` along
    /// its y.
    pub fn ellipse(center: impl Into<P2>, a: f64, b: f64) -> Self {
        Curve2::Ellipse {
            center: center.into(),
            major_radius: a,
            minor_radius: b,
            rotation: 0.0,
            source: None,
        }
    }

    /// The arc of that ellipse from the parameter `start` through `sweep`.
    pub fn ellipse_arc(center: impl Into<P2>, a: f64, b: f64, start: f64, sweep: f64) -> Self {
        Curve2::EllipseArc {
            center: center.into(),
            major_radius: a,
            minor_radius: b,
            rotation: 0.0,
            start,
            sweep,
            source: None,
        }
    }

    /// The B-spline of this degree on these knots and poles, with a weight a pole (or none: a
    /// polynomial curve).
    pub fn spline<P: Into<P2> + Copy>(
        degree: usize,
        knots: &[f64],
        poles: &[P],
        weights: &[f64],
    ) -> Self {
        Curve2::Spline {
            degree,
            knots: knots.to_vec(),
            poles: poles.iter().map(|&p| p.into()).collect(),
            weights: weights.to_vec(),
            source: None,
        }
    }

    /// The Bézier curve on these control points (of the degree they make): a spline of one
    /// span.
    pub fn bezier<P: Into<P2> + Copy>(poles: &[P]) -> Self {
        Curve2::rational_bezier(poles, &[])
    }

    /// The rational Bézier curve on these control points and weights.
    pub fn rational_bezier<P: Into<P2> + Copy>(poles: &[P], weights: &[f64]) -> Self {
        let degree = poles.len().saturating_sub(1);
        Curve2::spline(degree, &bezier_knots(degree), poles, weights)
    }

    /// The conic arc from `a` to `b` whose end tangents meet at `m`, weighted `w` there (a
    /// rational quadratic: a parabola when `w` is 1, a hyperbola above it, an ellipse below; a
    /// circle's arc when `w` is the cosine of half its turn).
    pub fn conic(a: impl Into<P2>, m: impl Into<P2>, w: f64, b: impl Into<P2>) -> Self {
        Curve2::rational_bezier(&[a.into(), m.into(), b.into()], &[1.0, w, 1.0])
    }

    /// A whole closed curve: the periodic B-spline of this degree on these poles (taken in
    /// order, closing back on the first), its knots evenly spaced.
    pub fn periodic<P: Into<P2> + Copy>(degree: usize, poles: &[P]) -> Self {
        let mut ps: Vec<P2> = poles.iter().map(|&p| p.into()).collect();
        ps.extend_from_within(..degree.min(ps.len()));
        let knots: Vec<f64> = (0..ps.len() + degree + 1).map(|i| i as f64).collect();
        Curve2::Spline {
            degree,
            knots,
            poles: ps,
            weights: vec![],
            source: None,
        }
    }

    /// The open polyline through `points`: a segment between each two in turn.
    pub fn polyline<P: Into<P2> + Copy>(points: &[P]) -> Vec<Curve2> {
        points
            .windows(2)
            .map(|w| Curve2::line(w[0], w[1]))
            .collect()
    }
}

impl Loop {
    /// The loop of these curves, each starting where the one before ends.
    pub fn new(curves: Vec<Curve2>) -> Self {
        Loop { curves }
    }

    /// The closed polygon through `points`.
    pub fn polygon<P: Into<P2> + Copy>(points: &[P]) -> Self {
        let n = points.len();
        Loop {
            curves: (0..n)
                .map(|i| Curve2::line(points[i], points[(i + 1) % n]))
                .collect(),
        }
    }

    /// The rectangle [x0, x1] × [y0, y1].
    pub fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Loop::polygon(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
    }

    /// The whole circle about `center`.
    pub fn circle(center: impl Into<P2>, radius: f64) -> Self {
        Loop::new(vec![Curve2::circle(center, radius)])
    }

    /// The whole ellipse about `center` with semi-axes `a` along the sketch's x and `b` along
    /// its y.
    pub fn ellipse(center: impl Into<P2>, a: f64, b: f64) -> Self {
        Loop::new(vec![Curve2::ellipse(center, a, b)])
    }

    /// An outline begun at `start`, drawn a piece at a time, each from where the last ended
    /// ([`Outline`]).
    pub fn from_point(start: impl Into<P2>) -> Outline {
        let start = start.into();
        Outline {
            start,
            at: start,
            curves: Vec::new(),
        }
    }
}

/// An outline being drawn: each piece runs from where the last ended ([`Loop::from_point`]), and
/// [`Outline::close`] makes the loop of them.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    start: P2,
    at: P2,
    curves: Vec<Curve2>,
}

impl Outline {
    /// A segment to `to`.
    pub fn line_to(mut self, to: impl Into<P2>) -> Self {
        let to = to.into();
        self.curves.push(Curve2::line(self.at, to));
        self.at = to;
        self
    }

    /// A conic arc to `to`, its end tangents meeting at `corner`, weighted `w` there
    /// ([`Curve2::conic`]).
    pub fn conic_to(mut self, corner: impl Into<P2>, w: f64, to: impl Into<P2>) -> Self {
        let to = to.into();
        self.curves.push(Curve2::conic(self.at, corner, w, to));
        self.at = to;
        self
    }

    /// A circle's arc about `center` through the angle `sweep` (counter-clockwise when
    /// positive), from where the outline is.
    pub fn arc_about(mut self, center: impl Into<P2>, sweep: f64) -> Self {
        let center = center.into();
        let from = self.at - center;
        let (radius, start) = (from.norm(), from.y.atan2(from.x));
        self.curves.push(Curve2::arc(center, radius, start, sweep));
        let end = start + sweep;
        self.at = P2::new(center.x + radius * end.cos(), center.y + radius * end.sin());
        self
    }

    /// A Bézier curve through the control points `poles` after the outline's own point (the
    /// last of them where it ends), with a weight for each pole and that point first (or none: a
    /// polynomial curve).
    pub fn bezier_to<P: Into<P2> + Copy>(mut self, poles: &[P], weights: &[f64]) -> Self {
        let mut all = vec![self.at];
        all.extend(poles.iter().map(|&p| p.into()));
        self.at = all[all.len() - 1];
        self.curves.push(Curve2::rational_bezier(&all, weights));
        self
    }

    /// Any curve that starts where the outline is.
    pub fn then(mut self, curve: Curve2) -> Self {
        self.at = curve.end();
        self.curves.push(curve);
        self
    }

    /// The loop: closed with a segment back to the start, where the outline hasn't come back
    /// to it.
    pub fn close(mut self) -> Loop {
        if (self.at - self.start).norm() > crate::native::tol::POINT {
            self.curves.push(Curve2::line(self.at, self.start));
        }
        Loop {
            curves: self.curves,
        }
    }

    /// The pieces as they are, not closed: an open chain.
    pub fn open(self) -> Vec<Curve2> {
        self.curves
    }
}

impl Region {
    /// The region inside `outer`.
    pub fn new(outer: Loop) -> Self {
        Region {
            outer,
            holes: Vec::new(),
            source: None,
        }
    }

    /// The region inside `outer` and outside each of `holes`.
    pub fn with_holes(outer: Loop, holes: Vec<Loop>) -> Self {
        Region {
            outer,
            holes,
            source: None,
        }
    }
}

impl Profile {
    /// A sketch on `plane` of the regions inside each of `outers`.
    pub fn of(plane: Plane, outers: Vec<Loop>) -> Self {
        Profile::new(plane, outers.into_iter().map(Region::new).collect())
    }

    /// A sketch on `plane` of one region: inside `outer`, outside each of `holes`.
    pub fn with_holes(plane: Plane, outer: Loop, holes: Vec<Loop>) -> Self {
        Profile::new(plane, vec![Region::with_holes(outer, holes)])
    }

    /// A sketch on `plane` of one open chain of curves and no region (what a surface extrude or
    /// revolve sweeps).
    pub fn open(plane: Plane, curves: Vec<Curve2>) -> Self {
        Profile {
            plane,
            regions: Vec::new(),
            chains: vec![Chain {
                curves,
                source: None,
            }],
        }
    }
}

impl PathCurve {
    /// A sketch curve on its plane.
    pub fn sketch(plane: Plane, curve: Curve2) -> Self {
        PathCurve::Sketch { plane, curve }
    }

    /// The Bézier curve in space on these control points.
    pub fn bezier(poles: &[P3]) -> Self {
        PathCurve::Bezier3 {
            poles: poles.to_vec(),
            weights: Vec::new(),
        }
    }

    /// The rational Bézier curve in space on these control points and weights.
    pub fn rational_bezier(poles: &[P3], weights: &[f64]) -> Self {
        PathCurve::Bezier3 {
            poles: poles.to_vec(),
            weights: weights.to_vec(),
        }
    }
}

/// A helix, or a spiral on a cone: it starts `r0` from the axis at the angle `start_angle` from
/// `x` and winds `turns` times round the axis (right-handed about its direction when positive)
/// while rising `height` along it, its radius going evenly to `r1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Helix {
    /// The axis, its origin level with the helix's start.
    pub axis: Axis,
    /// Where angles are measured from, square to the axis.
    pub x: Unit<V3>,
    /// The radius at the start.
    pub r0: f64,
    /// The radius at the end.
    pub r1: f64,
    /// How far it rises along the axis.
    pub height: f64,
    /// How many times it winds round: right-handed about the axis when positive.
    pub turns: f64,
    /// The angle it starts at, from `x`.
    pub start_angle: f64,
}

impl Helix {
    /// The helix of `radius` about `axis` rising `pitch` a turn, `turns` turns from the angle
    /// `start_angle`, angles measured from the direction [`Plane::through_axis`] gives the axis.
    pub fn new(axis: Axis, radius: f64, pitch: f64, start_angle: f64, turns: f64) -> Self {
        Helix {
            axis,
            x: Plane::through_axis(&axis).x_dir,
            r0: radius,
            r1: radius,
            height: pitch * turns.abs(),
            turns,
            start_angle,
        }
    }

    /// The spiral on a cone about `axis`: its radius from `r0` to `r1` as it rises `height` over
    /// `turns` turns from the angle `start_angle`.
    pub fn conical(
        axis: Axis,
        r0: f64,
        r1: f64,
        height: f64,
        start_angle: f64,
        turns: f64,
    ) -> Self {
        Helix {
            r0,
            r1,
            height,
            ..Helix::new(axis, r0, 0.0, start_angle, turns)
        }
    }

    fn frame(&self) -> (V3, V3, V3) {
        let a = self.axis.dir.into_inner();
        let x = (self.x.into_inner() - a * self.x.dot(&a)).normalize();
        (a, x, a.cross(&x))
    }

    /// The point at `s` (0 at the start, 1 at the end) with its first and second derivatives in
    /// `s`.
    pub fn at2(&self, s: f64) -> (P3, V3, V3) {
        let (a, x, y) = self.frame();
        let dth = TAU * self.turns;
        let th = self.start_angle + dth * s;
        let (r, dr) = (self.r0 + (self.r1 - self.r0) * s, self.r1 - self.r0);
        let (radial, tangential) = (x * th.cos() + y * th.sin(), y * th.cos() - x * th.sin());
        (
            self.axis.origin + a * (self.height * s) + radial * r,
            a * self.height + radial * dr + tangential * (r * dth),
            tangential * (2.0 * dr * dth) - radial * (r * dth * dth),
        )
    }

    /// The point at `s` (0 at the start, 1 at the end) and its derivative in `s` (along the
    /// tangent).
    pub fn at(&self, s: f64) -> (P3, V3) {
        let (p, d, _) = self.at2(s);
        (p, d)
    }

    /// The helix as path curves, sixteen Bézier pieces a turn. At one radius each is a rational
    /// quadratic lying on the helix's cylinder (its arc of the circle, the height rising with
    /// it), which a sweep recognises and follows by the helix's exact screw; on a cone, quintics
    /// matching the spiral's points and first and second derivatives at their ends.
    pub fn path(&self) -> Vec<PathCurve> {
        let n = ((self.turns.abs() * 16.0).ceil() as usize).max(1);
        let (a, x, y) = self.frame();
        let level = (self.r1 - self.r0).abs() <= 1e-12 * self.r0.abs().max(1.0);
        (0..n)
            .map(|k| {
                let (s0, s1) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
                if level {
                    let th = |s: f64| self.start_angle + TAU * self.turns * s;
                    let (t0, t1) = (th(s0), th(s1));
                    let half = (t1 - t0) / 2.0;
                    let at = |t: f64, r: f64, z: f64| {
                        self.axis.origin + a * z + (x * t.cos() + y * t.sin()) * r
                    };
                    let (z0, z1) = (self.height * s0, self.height * s1);
                    return PathCurve::Bezier3 {
                        poles: vec![
                            at(t0, self.r0, z0),
                            at(t0 + half, self.r0 / half.cos(), (z0 + z1) / 2.0),
                            at(t1, self.r0, z1),
                        ],
                        weights: vec![1.0, half.cos(), 1.0],
                    };
                }
                let h = s1 - s0;
                let (p0, d0, a0) = self.at2(s0);
                let (p5, d5, a5) = self.at2(s1);
                PathCurve::bezier(&[
                    p0,
                    p0 + d0 * (h / 5.0),
                    p0 + d0 * (2.0 * h / 5.0) + a0 * (h * h / 20.0),
                    p5 - d5 * (2.0 * h / 5.0) + a5 * (h * h / 20.0),
                    p5 - d5 * (h / 5.0),
                    p5,
                ])
            })
            .collect()
    }
}

impl RigidStep {
    /// A turn about `axis` from the angle `from` to `to` (right-handed about its direction).
    pub fn turn(axis: Axis, from: f64, to: f64) -> Self {
        RigidStep::Turn { axis, from, to }
    }

    /// A slide from `from` to `to`.
    pub fn slide(from: V3, to: V3) -> Self {
        RigidStep::Slide { from, to }
    }
}

impl SolidMotion {
    /// Slid without turning by `v`, from where the body is.
    pub fn slide(v: V3) -> Self {
        SolidMotion::rigid(vec![RigidStep::slide(V3::zeros(), v)])
    }

    /// Turned about `axis` from the angle `from` to `to` (right-handed about its direction),
    /// the body drawn where it is at the angle 0.
    pub fn swing(axis: Axis, from: f64, to: f64) -> Self {
        SolidMotion::rigid(vec![RigidStep::turn(axis, from, to)])
    }

    /// Screwed about `axis` from the angle `from` to `to` while rising `lead` along it a whole
    /// turn, the body drawn where it is at the angle 0: a turn and a slide along its axis at
    /// once ([`SolidMotion::Screw`] is the screw without end, between two planes).
    pub fn helical(axis: Axis, lead: f64, from: f64, to: f64) -> Self {
        let rise = |angle: f64| axis.dir.into_inner() * (lead * angle / TAU);
        SolidMotion::rigid(vec![
            RigidStep::turn(axis, from, to),
            RigidStep::slide(rise(from), rise(to)),
        ])
    }

    /// Moved by `steps` applied in order at each instant ([`SolidMotion::Rigid`]).
    pub fn rigid(steps: Vec<RigidStep>) -> Self {
        SolidMotion::Rigid { steps }
    }

    /// Carried along `path` by the frame riding it upright: its z axis `up`
    /// ([`SolidMotion::Path`]).
    pub fn along(path: Vec<PathCurve>, up: V3) -> Self {
        SolidMotion::Path {
            path,
            up: Unit::new_normalize(up),
        }
    }
}

impl ExtrudeSpec {
    /// A solid extrude of `profile` by `depth` along its plane's normal, or against it when
    /// `depth` is negative.
    pub fn normal_to(profile: &Profile, depth: f64) -> Self {
        let n = profile.plane.normal;
        ExtrudeSpec::blind(if depth < 0.0 { -n } else { n }, depth.abs())
    }

    /// A surface extrude of `depth` along `direction`: the sheet a profile's curves sweep.
    pub fn surface(direction: Unit<V3>, depth: f64) -> Self {
        ExtrudeSpec {
            body: BodyKind::Surface,
            ..ExtrudeSpec::blind(direction, depth)
        }
    }

    /// A solid extrude along `direction` up to `end`, the bodies of `scene` the ones Up to next
    /// and Through all consider.
    pub fn up_to(direction: Unit<V3>, end: ExtrudeEnd, scene: &[BodyId]) -> Self {
        ExtrudeSpec {
            end,
            scene: scene.to_vec(),
            ..ExtrudeSpec::blind(direction, 0.0)
        }
    }

    /// Its depth split evenly either side of the sketch plane.
    pub fn both_ways(mut self) -> Self {
        self.symmetric = true;
        self
    }

    /// With planar faces extruded along with the profile's regions.
    pub fn with_faces(mut self, faces: Vec<FaceInput>) -> Self {
        self.faces = faces;
        self
    }
}

impl RevolveSpec {
    /// A surface revolve about `axis`: a whole turn when `angle` is `None`.
    pub fn surface(axis: Axis, angle: Option<f64>) -> Self {
        RevolveSpec {
            body: BodyKind::Surface,
            ..RevolveSpec::solid(axis, angle)
        }
    }

    /// A solid revolve about `axis` up to `end`, the bodies of `scene` the ones Up to next
    /// considers.
    pub fn up_to(axis: Axis, end: RevolveEnd, scene: &[BodyId]) -> Self {
        RevolveSpec {
            end,
            scene: scene.to_vec(),
            ..RevolveSpec::solid(axis, Some(0.0))
        }
    }

    /// With planar faces revolved along with the profile's regions.
    pub fn with_faces(mut self, faces: Vec<FaceInput>) -> Self {
        self.faces = faces;
        self
    }
}

impl SweepSpec {
    /// A solid sweep along `path`: the profile keeps its angle to the path, sharp corners of
    /// the path mitred.
    pub fn solid(path: Vec<PathCurve>) -> Self {
        SweepSpec {
            body: BodyKind::Solid,
            path,
            control: SweepControl::None,
            faces: Vec::new(),
            corners: SweepCorners::Mitre,
        }
    }

    /// Round the path's sharp corners: the profile turned about each.
    pub fn round_corners(mut self) -> Self {
        self.corners = SweepCorners::Round;
        self
    }
}

impl LoftSpec {
    /// A solid loft through `sections` in order, with no end conditions.
    pub fn solid(sections: Vec<LoftSection>) -> Self {
        LoftSpec {
            body: BodyKind::Solid,
            sections,
            start: LoftEnd::default(),
            end: LoftEnd::default(),
            source: 0,
        }
    }

    /// A solid loft through sketched sections in order.
    pub fn through(sections: Vec<Profile>) -> Self {
        LoftSpec::solid(sections.into_iter().map(LoftSection::Profile).collect())
    }
}

impl CurveLoftSpec {
    /// A sheet lofted through `sections` in order (each a wire, or a point at either end) along
    /// `guides`, with no end conditions.
    pub fn new(sections: Vec<CurveSection>, guides: Vec<Vec<PathCurve>>) -> Self {
        CurveLoftSpec {
            sections,
            guides,
            start: CurveLoftEnd::default(),
            end: CurveLoftEnd::default(),
            source: 0,
        }
    }
}

impl FillSpec {
    /// The fill of a closed chain of sketch curves, each on its own plane.
    pub fn sketched(curves: &[(Plane, Curve2)]) -> Self {
        FillSpec {
            curves: curves
                .iter()
                .map(|(plane, curve)| FillCurve::Sketch {
                    plane: *plane,
                    curve: curve.clone(),
                })
                .collect(),
            source: 0,
        }
    }

    /// The flat fill of the polygon through `points` on `plane`.
    pub fn polygon<P: Into<P2> + Copy>(plane: Plane, points: &[P]) -> Self {
        let sides: Vec<(Plane, Curve2)> = Loop::polygon(points)
            .curves
            .into_iter()
            .map(|c| (plane, c))
            .collect();
        FillSpec::sketched(&sides)
    }

    /// The fill of a closed chain of a body's edges.
    pub fn edges(body: BodyId, edges: &[EdgeId]) -> Self {
        FillSpec {
            curves: edges
                .iter()
                .map(|&edge| FillCurve::Edge { body, edge })
                .collect(),
            source: 0,
        }
    }
}

impl ShellSpec {
    /// Shelled inward to walls `thickness` thick, open at the faces `remove`, its corners
    /// rounded.
    pub fn inward(remove: Vec<FaceId>, thickness: f64) -> Self {
        ShellSpec {
            remove,
            thickness,
            outward: false,
            hollow: false,
            sharp: false,
        }
    }

    /// Shelled outward: the walls grown outside the body.
    pub fn outward(remove: Vec<FaceId>, thickness: f64) -> Self {
        ShellSpec {
            outward: true,
            ..ShellSpec::inward(remove, thickness)
        }
    }

    /// Hollowed to walls `thickness` thick with no opening: the inside a closed void.
    pub fn hollow(thickness: f64) -> Self {
        ShellSpec {
            hollow: true,
            ..ShellSpec::inward(Vec::new(), thickness)
        }
    }

    /// Its corners sharp: the moved faces extended until they meet.
    pub fn mitred(mut self) -> Self {
        self.sharp = true;
        self
    }
}

impl ThickenSpec {
    /// The sheets `bodies` thickened `along` their faces' normals and `against` them, their
    /// bends rounded.
    pub fn of_bodies(bodies: Vec<BodyId>, along: f64, against: f64) -> Self {
        ThickenSpec {
            bodies,
            faces: Vec::new(),
            profiles: Vec::new(),
            along,
            against,
            sharp: false,
            source: 0,
        }
    }

    /// The faces `faces` of the part `body` thickened.
    pub fn of_faces(body: BodyId, faces: &[FaceId], along: f64, against: f64) -> Self {
        ThickenSpec {
            faces: faces
                .iter()
                .map(|&face| FaceInput::new(body, face))
                .collect(),
            ..ThickenSpec::of_bodies(Vec::new(), along, against)
        }
    }

    /// The regions of the sketches `profiles` thickened (their normals along their planes').
    pub fn of_profiles(profiles: Vec<Profile>, along: f64, against: f64) -> Self {
        ThickenSpec {
            profiles,
            ..ThickenSpec::of_bodies(Vec::new(), along, against)
        }
    }

    /// Its bends mitred: the moved faces extended until they meet.
    pub fn mitred(mut self) -> Self {
        self.sharp = true;
        self
    }
}

impl OffsetSpec {
    /// Every face moved `distance` outward (inward when negative), edges rounded where the
    /// distance makes them round: the solid of the points within `distance` of the body.
    pub fn by(distance: f64) -> Self {
        OffsetSpec {
            distance,
            faces: Vec::new(),
            sharp: false,
        }
    }

    /// Only these faces moved, each its own distance along its outward normal, the others kept
    /// and trimmed to meet them (Move face, Offset).
    pub fn of_faces(faces: &[(FaceId, f64)]) -> Self {
        OffsetSpec {
            distance: 0.0,
            faces: faces.to_vec(),
            sharp: true,
        }
    }

    /// Its edges kept sharp: the moved faces extended until they meet.
    pub fn mitred(mut self) -> Self {
        self.sharp = true;
        self
    }
}

impl DraftSpec {
    /// `faces` drafted by `angle` for the pull direction `pull`, about the neutral plane
    /// through `neutral` square to the pull, the faces tangent to them drafted too.
    pub fn new(faces: Vec<FaceId>, angle: f64, pull: V3, neutral: P3) -> Self {
        DraftSpec {
            faces,
            angle,
            pull: Unit::new_normalize(pull),
            neutral,
            neutral_normal: None,
            tangent_propagation: true,
        }
    }
}

impl FaceInput {
    /// A face of a body as a feature's input.
    pub fn new(body: BodyId, face: FaceId) -> Self {
        FaceInput {
            body,
            face,
            source: 0,
        }
    }
}

impl FaceReplacement {
    /// `face` given `surface`, its edges the crossings nearest the old ones.
    pub fn new(face: FaceId, surface: Surface) -> Self {
        FaceReplacement {
            face,
            surface,
            domain: None,
        }
    }

    /// The part of the surface the face lies on, in the surface's own parameters.
    pub fn over(mut self, domain: ((f64, f64), (f64, f64))) -> Self {
        self.domain = Some(domain);
        self
    }
}

impl Primitive {
    /// The block with opposite corners `min` and `max`.
    pub fn block(min: P3, max: P3) -> Self {
        Primitive::Box { min, max }
    }

    /// The cylinder of `radius` on the axis from `base` along `dir`, `height` long.
    pub fn cylinder(base: P3, dir: V3, radius: f64, height: f64) -> Self {
        Primitive::Cylinder {
            base,
            axis: Unit::new_normalize(dir),
            radius,
            height,
        }
    }

    /// The cone, or its frustum, on the axis from `base` along `dir`, `height` long: of radius
    /// `r0` at its base and `r1` at its top (either may be 0).
    pub fn cone(base: P3, dir: V3, r0: f64, r1: f64, height: f64) -> Self {
        Primitive::Cone {
            base,
            axis: Unit::new_normalize(dir),
            base_radius: r0,
            top_radius: r1,
            height,
        }
    }

    /// The ball of `radius` about `center`, its poles on the z axis through its centre and its
    /// seam toward +x.
    pub fn ball(center: P3, radius: f64) -> Self {
        Primitive::ball_with(center, radius, V3::z(), V3::x())
    }

    /// The ball with its poles along `dir` from its centre, its seam where [`Plane::through_axis`]
    /// puts that axis's sketch x.
    pub fn ball_about(center: P3, dir: V3, radius: f64) -> Self {
        let seam = Plane::through_axis(&Axis::new(center, dir))
            .x_dir
            .into_inner();
        Primitive::ball_with(center, radius, dir, seam)
    }

    /// The ball with its poles along `axis` from its centre and its seam toward `seam` from
    /// that axis.
    pub fn ball_with(center: P3, radius: f64, axis: V3, seam: V3) -> Self {
        Primitive::Sphere {
            center,
            radius,
            axis: Unit::new_normalize(axis),
            seam: Unit::new_normalize(seam),
        }
    }

    /// The torus about the axis through `center` along `dir`: the circle through its tube's
    /// middle of radius `major`, its tube of radius `minor`.
    pub fn torus(center: P3, dir: V3, major: f64, minor: f64) -> Self {
        Primitive::Torus {
            center,
            axis: Unit::new_normalize(dir),
            major,
            minor,
        }
    }
}

impl OpResult {
    /// The operation's body: its first, or an error where it made none.
    pub fn first(&self) -> Result<BodyId> {
        self.bodies
            .first()
            .copied()
            .ok_or_else(|| KernelError::OperationFailed("the operation made no body".into()))
    }
}

impl EdgeInfo {
    /// Whether the edge is a seam: one face on both its sides (where a cylinder's face closes
    /// on itself), no edge a user picks.
    pub fn is_seam(&self) -> bool {
        self.faces[0].is_some() && self.faces[0] == self.faces[1]
    }
}

impl FaceInfo {
    /// Whether the face is a side for a pull along `pull`: a plane along the pull, or a
    /// cylinder or cone about an axis along it (the faces a Draft leans).
    pub fn is_side_for(&self, pull: &V3) -> bool {
        let pull = pull.normalize();
        match self.kind {
            SurfaceKind::Plane => self.plane.is_some_and(|p| p.normal.dot(&pull).abs() < 1e-9),
            SurfaceKind::Cylinder | SurfaceKind::Cone => {
                self.axis.is_some_and(|a| a.dir.cross(&pull).norm() < 1e-9)
            }
            _ => false,
        }
    }
}

impl NurbsSurface {
    /// The B-spline surface of degrees `du` × `dv` on the knots `ku` × `kv` through the control
    /// points `points[i][j]` (i along u), with the weights `weights[i][j]` (or none: a
    /// polynomial surface).
    pub fn from_points(
        du: usize,
        dv: usize,
        ku: Vec<f64>,
        kv: Vec<f64>,
        points: &[Vec<[f64; 3]>],
        weights: Option<&[Vec<f64>]>,
    ) -> Result<Self> {
        let cps = points
            .iter()
            .enumerate()
            .map(|(i, row)| {
                row.iter()
                    .enumerate()
                    .map(|(j, p)| {
                        let w = weights.map_or(1.0, |w| w[i][j]);
                        Vector4::new(p[0] * w, p[1] * w, p[2] * w, w)
                    })
                    .collect()
            })
            .collect();
        NurbsSurface::new(du, dv, ku, kv, cps)
    }
}

impl From<NurbsSurface> for Surface {
    fn from(s: NurbsSurface) -> Self {
        Surface::Nurbs(std::sync::Arc::new(s))
    }
}
