//! Shared construction for the tests ported from springrep's (MIT,
//! https://github.com/springcraft-ai/springrep): its frames, profiles and primitives built with
//! cadrs's kernel, placed and seamed as springrep places them, and its measures read back.
#![allow(dead_code)]

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point3, Vector3};

pub type P3 = Point3<f64>;
pub type V3 = Vector3<f64>;

pub fn p3(a: [f64; 3]) -> P3 {
    P3::new(a[0], a[1], a[2])
}

pub fn v3(a: [f64; 3]) -> V3 {
    V3::new(a[0], a[1], a[2])
}

/// springrep's `Frame::new(origin, z, x_hint)`: z made unit, x the hint made square to it, y = z × x.
pub fn frame(origin: P3, z: V3, x: V3) -> Plane {
    Plane::new(origin, x, z)
}

/// springrep's `Frame::from_axis`: x along `z × (X if |z.x| < 0.6 else Y)`.
pub fn frame_axis(origin: P3, z: V3) -> Plane {
    let a = if z.x.abs() < 0.6 { V3::x() } else { V3::y() };
    frame(origin, z, z.cross(&a).normalize())
}

/// springrep's `Frame::WORLD`.
pub fn world() -> Plane {
    Plane::xy(0.0)
}

pub fn y_of(f: &Plane) -> V3 {
    f.y_dir().into_inner()
}

/// The frame's local coordinates of `p`.
pub fn to_local(f: &Plane, p: P3) -> V3 {
    let d = p - f.origin;
    V3::new(
        d.dot(&f.x_dir),
        d.dot(&f.y_dir()),
        d.dot(&f.normal.into_inner()),
    )
}

/// springrep's `Profile::rect(center, w, h)`.
pub fn rect(cx: f64, cy: f64, w: f64, h: f64) -> Loop {
    Loop::rectangle(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0)
}

/// springrep's `Profile::circle(center, DIAMETER)`, as a radius here.
pub fn circle(cx: f64, cy: f64, r: f64) -> Loop {
    Loop::circle([cx, cy], r)
}

/// springrep's `extrude(profile, frame, distance)`: along the frame's z (against it when negative).
pub fn extrude(k: &mut NativeKernel, plane: Plane, outer: Loop, distance: f64) -> BodyId {
    extrude_try(k, plane, vec![outer], distance).unwrap()
}

pub fn extrude_try(
    k: &mut NativeKernel,
    plane: Plane,
    loops: Vec<Loop>,
    distance: f64,
) -> Result<BodyId> {
    k.extrude(&Profile::of(plane, loops), Extent::Blind(distance))?
        .first()
}

/// springrep's `revolve(profile, frame, origin, dir, angle)`: about the line through `origin`
/// along `dir`, which lies in the profile's plane.
pub fn revolve(k: &mut NativeKernel, plane: Plane, outer: Loop, origin: P3, dir: V3) -> BodyId {
    k.revolve(
        &Profile::of(plane, vec![outer]),
        Axis::new(origin, dir),
        TAU,
    )
    .unwrap()
    .first()
    .unwrap()
}

/// The half disc of radius `r` on the right of the plane's y axis, closed along it: springrep's
/// ball meridian (an arc from −90° through 180°, then the axis line).
pub fn half_disc(r: f64) -> Loop {
    Loop::new(vec![
        Curve2::arc([0.0, 0.0], r, -PI / 2.0, PI),
        Curve2::line([0.0, r], [0.0, -r]),
    ])
}

/// springrep's test `block`: the box `lo`..`hi`, extruded from z = lo.z on the world's axes.
pub fn block(k: &mut NativeKernel, lo: [f64; 3], hi: [f64; 3]) -> BodyId {
    let c = (0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]));
    let f = frame(P3::new(0.0, 0.0, lo[2]), V3::z(), V3::x());
    extrude(
        k,
        f,
        rect(c.0, c.1, hi[0] - lo[0], hi[1] - lo[1]),
        hi[2] - lo[2],
    )
}

/// springrep's test `rod`: a cylinder of radius `r` along `axis` from `base`, `len` long.
pub fn rod(k: &mut NativeKernel, base: [f64; 3], axis: [f64; 3], r: f64, len: f64) -> BodyId {
    let f = frame_axis(p3(base), v3(axis));
    extrude(k, f, circle(0.0, 0.0, r), len)
}

/// springrep's test `ball`: the half disc on the frame (c, z = −Y, x = X) revolved about Z.
pub fn ball(k: &mut NativeKernel, c: [f64; 3], r: f64) -> BodyId {
    let c = p3(c);
    let f = frame(c, -V3::y(), V3::x());
    revolve(k, f, half_disc(r), c, V3::z())
}

/// springrep's `primitive::turned`: the meridian on the plane (origin, z = −y, x = x) revolved
/// about the frame's z.
pub fn turned(k: &mut NativeKernel, f: &Plane, meridian: Loop) -> BodyId {
    let plane = frame(f.origin, -y_of(f), f.x_dir.into_inner());
    revolve(k, plane, meridian, f.origin, f.normal.into_inner())
}

/// springrep's `primitive::block(frame, size)`: from the frame's origin along its axes.
pub fn pblock(k: &mut NativeKernel, f: &Plane, size: [f64; 3]) -> BodyId {
    extrude(
        k,
        *f,
        rect(size[0] / 2.0, size[1] / 2.0, size[0], size[1]),
        size[2],
    )
}

/// springrep's `primitive::cylinder(frame, radius, height)`.
pub fn cylinder(k: &mut NativeKernel, f: &Plane, r: f64, h: f64) -> BodyId {
    extrude(k, *f, circle(0.0, 0.0, r), h)
}

/// springrep's `primitive::sphere(frame, radius)`.
pub fn sphere(k: &mut NativeKernel, f: &Plane, r: f64) -> BodyId {
    turned(k, f, half_disc(r))
}

/// springrep's `primitive::torus(frame, major, minor)`.
pub fn torus(k: &mut NativeKernel, f: &Plane, major: f64, minor: f64) -> BodyId {
    turned(k, f, circle(major, 0.0, minor))
}

/// springrep's `primitive::cone(frame, bottom, top, height)`: the meridian (0,0), (bottom, 0),
/// (top, h), (0, h), a zero radius an apex.
pub fn cone(k: &mut NativeKernel, f: &Plane, bottom: f64, top: f64, h: f64) -> BodyId {
    let pts = [[0.0, 0.0], [bottom, 0.0], [top, h], [0.0, h]];
    let mut curves = Vec::new();
    for i in 0..4 {
        let (a, b) = (pts[i], pts[(i + 1) % 4]);
        if a != b {
            curves.push(Curve2::line(a, b));
        }
    }
    turned(k, f, Loop::new(curves))
}

/// springrep's `BSpline::interpolate_periodic`: the closed C² cubic through the points (in the
/// plane z = 0) at chord-length parameters, clamped at its first point.
pub fn periodic_curve(pts: &[[f64; 2]]) -> cadrs_kernel::testkit::nurbs::NurbsCurve {
    use cadrs_kernel::testkit::nurbs::NurbsCurve;
    let p3s: Vec<P3> = pts.iter().map(|p| P3::new(p[0], p[1], 0.0)).collect();
    let n = p3s.len();
    let mut params = vec![0.0];
    for i in 0..n {
        let d = (p3s[(i + 1) % n] - p3s[i]).norm();
        params.push(params[i] + d);
    }
    NurbsCurve::interpolate_periodic(&p3s, &params, 3).unwrap()
}

/// springrep's `Profile::points(.., PointsInterp::PeriodicSpline)`: [`periodic_curve`] as a loop.
pub fn periodic_spline(pts: &[[f64; 2]]) -> Loop {
    let c = periodic_curve(pts);
    let poles: Vec<[f64; 2]> = c.cps.iter().map(|h| [h.x / h.w, h.y / h.w]).collect();
    Loop::new(vec![Curve2::spline(3, &c.knots, &poles, &[])])
}

/// A boolean of two bodies, as springrep's `union`/`intersection`/`difference` take two.
pub fn boolean(k: &mut NativeKernel, op: BoolOp, a: BodyId, b: BodyId) -> Result<BodyId> {
    let r = k.boolean(op, a, &[b])?;
    match r.bodies.len() {
        1 => Ok(r.bodies[0]),
        0 => Err(KernelError::EmptyResult("no body".into())),
        _ => k.compound(&r.bodies)?.first(),
    }
}

pub fn union(k: &mut NativeKernel, a: BodyId, b: BodyId) -> BodyId {
    boolean(k, BoolOp::Union, a, b).unwrap()
}

pub fn intersection(k: &mut NativeKernel, a: BodyId, b: BodyId) -> BodyId {
    boolean(k, BoolOp::Intersect, a, b).unwrap()
}

pub fn difference(k: &mut NativeKernel, a: BodyId, b: BodyId) -> BodyId {
    boolean(k, BoolOp::Subtract, a, b).unwrap()
}

/// A boolean whose result may be empty: springrep returns an empty solid set where cadrs says
/// `EmptyResult`; None for that.
pub fn maybe(r: Result<BodyId>) -> Option<BodyId> {
    match r {
        Ok(b) => Some(b),
        Err(KernelError::EmptyResult(_)) => None,
        Err(e) => panic!("{e:?}"),
    }
}

/// The number of solids, 0 for an empty result.
pub fn solids(k: &NativeKernel, b: Option<BodyId>) -> usize {
    b.map_or(0, |b| k.solid_count(b).unwrap())
}

pub fn issues(k: &NativeKernel, b: BodyId) -> Vec<String> {
    k.get(b).unwrap().check()
}

pub fn valid(k: &NativeKernel, b: BodyId) {
    let i = issues(k, b);
    assert!(i.is_empty(), "check: {i:?}");
}

pub fn volume(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

pub fn area(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().surface_area
}

pub fn faces(k: &NativeKernel, b: BodyId) -> usize {
    k.face_count(b).unwrap()
}

/// springrep's COARSE mesh (0.2 mm, 0.5 rad), checked watertight solid by solid (springrep's
/// `mesh::report` is per solid).
pub fn watertight(k: &mut NativeKernel, b: BodyId) {
    if k.solid_count(b).unwrap() > 1 {
        for r in k.split_solids(b).unwrap() {
            watertight_one(k, r.first().unwrap());
        }
    } else {
        watertight_one(k, b);
    }
}

fn watertight_one(k: &NativeKernel, b: BodyId) {
    let m = k
        .tessellate(
            b,
            Tessellation {
                deflection: 0.2,
                angle: 0.5,
            },
        )
        .unwrap();
    let c = cadrs_kernel::testkit::tess::check_tri_mesh(&m);
    assert!(c.is_watertight(), "mesh not watertight: {c:?}");
}

/// Valid, the volume and area to 1e-9 of themselves, and a watertight mesh.
pub fn assert_solid(k: &mut NativeKernel, b: BodyId, vol: f64, ar: f64) {
    assert_solid_rel(k, b, vol, ar, 1e-9);
    watertight(k, b);
}

pub fn assert_solid_rel(k: &NativeKernel, b: BodyId, vol: f64, ar: f64, rel: f64) {
    valid(k, b);
    let (v, a) = (volume(k, b), area(k, b));
    assert!((v - vol).abs() <= rel * vol, "volume {v} vs {vol}");
    assert!((a - ar).abs() <= rel * ar, "area {a} vs {ar}");
}

/// A copy of `b` moved by `t`.
pub fn moved(k: &mut NativeKernel, b: BodyId, t: &Transform) -> BodyId {
    k.transform(b, t).unwrap().first().unwrap()
}

/// springrep's `Transform::rotation(origin, axis, angle)`.
pub fn rotation(origin: P3, axis: V3, angle: f64) -> Transform {
    let r = nalgebra::UnitQuaternion::from_axis_angle(&nalgebra::Unit::new_normalize(axis), angle);
    let o = origin.coords;
    Transform::from_parts(nalgebra::Translation3::from(o - r * o), r)
}

pub fn translation(v: V3) -> Transform {
    Transform::translation(v.x, v.y, v.z)
}

/// A fixed sequence of numbers in [0, 1): springrep's test `Seq`.
pub struct Seq(pub u64);

impl Seq {
    pub fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn within(&mut self, lo: [f64; 3], hi: [f64; 3]) -> P3 {
        P3::new(
            lo[0] + (hi[0] - lo[0]) * self.next(),
            lo[1] + (hi[1] - lo[1]) * self.next(),
            lo[2] + (hi[2] - lo[2]) * self.next(),
        )
    }
}

/// The edges of a body four faces use: where pieces meet along a curve. cadrs has no radial
/// edges, so these are found by geometry: edges along the same curve (the same ends and middle),
/// their face uses counted together.
pub fn four_face_edges(k: &NativeKernel, b: BodyId) -> usize {
    let edges = k.edges(b).unwrap();
    let same = |a: &EdgeInfo, e: &EdgeInfo| {
        let close = |p: P3, q: P3| (p - q).norm() < 1e-6;
        close(a.mid, e.mid)
            && ((close(a.start, e.start) && close(a.end, e.end))
                || (close(a.start, e.end) && close(a.end, e.start)))
    };
    let mut groups: Vec<(EdgeInfo, usize)> = Vec::new();
    for e in edges {
        let uses = e.faces.iter().filter(|f| f.is_some()).count();
        match groups.iter_mut().find(|(g, _)| same(g, &e)) {
            Some(g) => g.1 += uses,
            None => groups.push((e, uses)),
        }
    }
    groups.iter().filter(|g| g.1 == 4).count()
}
