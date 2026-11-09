//! What the cadrs implementations share: sketches on planes, the solids cadrs's Part Studio
//! makes from them (an extrude, a revolve), the kernel's primitives, booleans, rigid motions,
//! and picking a face or an edge by a point on it, as a user picks one in the viewport.
//!
//! A block is a rectangle extruded and a cylinder a circle extruded, as a sketch-based modeller
//! such as Onshape makes them; a ball, a cone and a torus are the kernel's primitives
//! (`Kernel::primitive`), a torus whose tube is as wide as its ring or wider among them.
//!
//! Lengths are the cases' units (cadrs reads them as mm); angles here are in degrees, as the
//! cases give them, and turned into the kernel's radians.
use crate::Refusal;
use cadrs_kernel::{
    Axis, BodyId, BoolOp, Curve2, DefaultKernel, DistanceLocation, DistanceTarget, EdgeId,
    EdgeInfo, ExtrudeSpec, FaceId, FaceInfo, Kernel, Loop, Motion, Plane, Primitive, Profile,
    Region, RevolveSpec, Transform,
};
use nalgebra::{Point2, Point3, Unit, Vector3};

pub type P2 = Point2<f64>;
pub type P3 = Point3<f64>;
pub type V3 = Vector3<f64>;

pub fn p2(x: f64, y: f64) -> P2 {
    P2::new(x, y)
}

pub fn p3(x: f64, y: f64, z: f64) -> P3 {
    P3::new(x, y, z)
}

pub fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3::new(x, y, z)
}

pub fn unit(v: V3) -> Unit<V3> {
    Unit::new_normalize(v)
}

// Sketch planes.

/// The plane through `origin` with normal `normal` and sketch x axis `x_dir` (made square to the
/// normal); its sketch y axis is `normal × x_dir`.
pub fn plane(origin: P3, x_dir: V3, normal: V3) -> Plane {
    let n = unit(normal);
    let x = unit(x_dir - n.into_inner() * x_dir.dot(&n));
    Plane {
        origin,
        x_dir: x,
        normal: n,
    }
}

/// The plane z = `z`, sketch (x, y) the model's (x, y), normal +z (cadrs's Top plane).
pub fn xy(z: f64) -> Plane {
    plane(p3(0.0, 0.0, z), V3::x(), V3::z())
}

/// The plane y = `y` with sketch (x, y) the model's (x, z): normal −y (cadrs's Front plane).
pub fn xz(y: f64) -> Plane {
    plane(p3(0.0, y, 0.0), V3::x(), -V3::y())
}

/// The plane x = `x` with sketch (x, y) the model's (y, z): normal +x (cadrs's Right plane).
pub fn yz(x: f64) -> Plane {
    plane(p3(x, 0.0, 0.0), V3::y(), V3::x())
}

/// A plane square to `normal` through `origin`, with some sketch x axis.
pub fn square_to(origin: P3, normal: V3) -> Plane {
    let n = normal.normalize();
    let other = if n.x.abs() < 0.9 { V3::x() } else { V3::y() };
    plane(origin, other, n)
}

// Sketch curves (in the plane's coordinates).

pub fn line(a: P2, b: P2) -> Curve2 {
    Curve2::Line { a, b, source: None }
}

/// The arc about `center` of `radius` from `start` degrees, sweeping `sweep` degrees
/// (counter-clockwise when positive).
pub fn arc(center: P2, radius: f64, start: f64, sweep: f64) -> Curve2 {
    Curve2::Arc {
        center,
        radius,
        start_angle: start.to_radians(),
        sweep: sweep.to_radians(),
        source: None,
    }
}

pub fn circle(center: P2, radius: f64) -> Curve2 {
    Curve2::Circle {
        center,
        radius,
        source: None,
    }
}

/// The closed polygon through `points`.
pub fn polygon(points: &[(f64, f64)]) -> Loop {
    let n = points.len();
    let curves = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            line(p2(a.0, a.1), p2(b.0, b.1))
        })
        .collect();
    Loop { curves }
}

/// The rectangle [x0, x1] × [y0, y1].
pub fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
    polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

pub fn round(center: (f64, f64), radius: f64) -> Loop {
    Loop {
        curves: vec![circle(p2(center.0, center.1), radius)],
    }
}

/// A loop of curves, each starting where the one before ends.
pub fn chain(curves: Vec<Curve2>) -> Loop {
    Loop { curves }
}

/// The region inside `outer` and outside each of `holes`.
pub fn region(outer: Loop, holes: Vec<Loop>) -> Region {
    Region {
        outer,
        holes,
        source: None,
    }
}

/// A sketch of the regions inside each of `outers` on `plane`.
pub fn sketch(plane: Plane, outers: Vec<Loop>) -> Profile {
    Profile::new(
        plane,
        outers.into_iter().map(|l| region(l, vec![])).collect(),
    )
}

/// A sketch of one region, `outer` less `holes`, on `plane`.
pub fn sketch_with_holes(plane: Plane, outer: Loop, holes: Vec<Loop>) -> Profile {
    Profile::new(plane, vec![region(outer, holes)])
}

// Features.

/// The first body of an operation's result.
fn first(r: cadrs_kernel::OpResult) -> Result<BodyId, Refusal> {
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the operation made no body"))
}

/// `profile` extruded `depth` along its plane's normal (against it when negative).
pub fn extrude(k: &mut DefaultKernel, profile: &Profile, depth: f64) -> Result<BodyId, Refusal> {
    let dir = if depth < 0.0 {
        -profile.plane.normal
    } else {
        profile.plane.normal
    };
    first(k.extrude_with(profile, &ExtrudeSpec::blind(dir, depth.abs()))?)
}

/// `profile` extruded `depth` along the direction `dir` (not in its plane).
pub fn extrude_along(
    k: &mut DefaultKernel,
    profile: &Profile,
    dir: V3,
    depth: f64,
) -> Result<BodyId, Refusal> {
    first(k.extrude_with(profile, &ExtrudeSpec::blind(unit(dir), depth))?)
}

/// `profile` extruded symmetrically: `depth` in all, half each side of its plane.
pub fn extrude_symmetric(
    k: &mut DefaultKernel,
    profile: &Profile,
    depth: f64,
) -> Result<BodyId, Refusal> {
    let mut spec = ExtrudeSpec::blind(profile.plane.normal, depth);
    spec.symmetric = true;
    first(k.extrude_with(profile, &spec)?)
}

/// The axis through `origin` along `dir`.
pub fn axis(origin: P3, dir: V3) -> Axis {
    Axis {
        origin,
        dir: unit(dir),
    }
}

/// `profile` revolved about `axis` by `angle` degrees (right-handed), or a whole turn when
/// `None`.
pub fn revolve(
    k: &mut DefaultKernel,
    profile: &Profile,
    axis: Axis,
    angle: Option<f64>,
) -> Result<BodyId, Refusal> {
    first(k.revolve_with(
        profile,
        &RevolveSpec::solid(axis, angle.map(f64::to_radians)),
    )?)
}

// Bodies made from sketches.

/// The block with opposite corners `min` and `max`: the rectangle on the plane z = min.z,
/// extruded up to max.z.
pub fn block(k: &mut DefaultKernel, min: P3, max: P3) -> Result<BodyId, Refusal> {
    let s = sketch(xy(min.z), vec![rectangle(min.x, min.y, max.x, max.y)]);
    extrude(k, &s, max.z - min.z)
}

/// The cylinder of `radius` whose base is centred on `base` and whose axis runs `length` along
/// `dir`: a circle on the plane through `base` square to `dir`, extruded.
pub fn cylinder(
    k: &mut DefaultKernel,
    base: P3,
    dir: V3,
    radius: f64,
    length: f64,
) -> Result<BodyId, Refusal> {
    let s = sketch(square_to(base, dir), vec![round((0.0, 0.0), radius)]);
    extrude(k, &s, length)
}

/// A plane containing the axis through `origin` along `dir`, its sketch x axis square to the
/// axis and its sketch y axis along it.
pub fn through_axis(origin: P3, dir: V3) -> Plane {
    let d = dir.normalize();
    let other = if d.x.abs() < 0.9 { V3::x() } else { V3::y() };
    let x = (other - d * other.dot(&d)).normalize();
    // y = normal × x = d, so normal = x × d
    plane(origin, x, x.cross(&d))
}

/// The ball of `radius` about `center` (cadrs's sphere primitive), its poles on the z axis
/// through its centre and its seam toward +x.
pub fn ball(k: &mut DefaultKernel, center: P3, radius: f64) -> Result<BodyId, Refusal> {
    ball_oriented(k, center, radius, V3::z(), V3::x())
}

/// The ball of `radius` about `center` (cadrs's sphere primitive), its poles along `axis` from
/// its centre and its seam toward `seam` from that axis.
pub fn ball_oriented(
    k: &mut DefaultKernel,
    center: P3,
    radius: f64,
    axis: V3,
    seam: V3,
) -> Result<BodyId, Refusal> {
    let (axis, seam) = (unit(axis), unit(seam));
    first(k.primitive(&Primitive::Sphere {
        center,
        radius,
        axis,
        seam,
    })?)
}

/// The cone frustum on the axis from `base` along `dir`, `length` long, of radius `r0` at its
/// base and `r1` at its top (either may be 0; cadrs's cone primitive).
pub fn cone(
    k: &mut DefaultKernel,
    base: P3,
    dir: V3,
    r0: f64,
    r1: f64,
    length: f64,
) -> Result<BodyId, Refusal> {
    let axis = unit(dir);
    first(k.primitive(&Primitive::Cone {
        base,
        axis,
        base_radius: r0,
        top_radius: r1,
        height: length,
    })?)
}

/// The torus about the axis through `center` along `dir`, the circle through its tube's middle
/// of radius `major` and its tube of radius `minor` (cadrs's torus primitive: a ring, horn or
/// spindle torus).
pub fn torus(
    k: &mut DefaultKernel,
    center: P3,
    dir: V3,
    major: f64,
    minor: f64,
) -> Result<BodyId, Refusal> {
    first(k.primitive(&Primitive::Torus {
        center,
        axis: unit(dir),
        major,
        minor,
    })?)
}

// Booleans.

pub fn boolean(
    k: &mut DefaultKernel,
    op: BoolOp,
    target: BodyId,
    tools: &[BodyId],
) -> Result<BodyId, Refusal> {
    first(k.boolean(op, target, tools)?)
}

pub fn union(k: &mut DefaultKernel, a: BodyId, b: BodyId) -> Result<BodyId, Refusal> {
    boolean(k, BoolOp::Union, a, &[b])
}

pub fn cut(k: &mut DefaultKernel, a: BodyId, b: BodyId) -> Result<BodyId, Refusal> {
    boolean(k, BoolOp::Subtract, a, &[b])
}

pub fn common(k: &mut DefaultKernel, a: BodyId, b: BodyId) -> Result<BodyId, Refusal> {
    boolean(k, BoolOp::Intersect, a, &[b])
}

/// `body` less each of `tools`, one cut after another.
pub fn cut_each(k: &mut DefaultKernel, body: BodyId, tools: &[BodyId]) -> Result<BodyId, Refusal> {
    let mut b = body;
    for &t in tools {
        b = cut(k, b, t)?;
    }
    Ok(b)
}

/// `body` united with each of `tools`, one after another.
pub fn union_each(
    k: &mut DefaultKernel,
    body: BodyId,
    tools: &[BodyId],
) -> Result<BodyId, Refusal> {
    let mut b = body;
    for &t in tools {
        b = union(k, b, t)?;
    }
    Ok(b)
}

// Rigid motions.

/// A copy of `body` moved by `v`.
pub fn moved(k: &mut DefaultKernel, body: BodyId, v: V3) -> Result<BodyId, Refusal> {
    first(k.transform(body, &Transform::translation(v.x, v.y, v.z))?)
}

/// A copy of `body` turned `angle` degrees (right-handed) about `axis`.
pub fn turned(
    k: &mut DefaultKernel,
    body: BodyId,
    axis: Axis,
    angle: f64,
) -> Result<BodyId, Refusal> {
    first(k.transform_motion(body, &Motion::rotation(&axis, angle.to_radians()))?)
}

/// A copy of `body` reflected in the plane through `point` with normal `normal`.
pub fn mirrored(
    k: &mut DefaultKernel,
    body: BodyId,
    point: P3,
    normal: V3,
) -> Result<BodyId, Refusal> {
    first(k.transform_motion(body, &Motion::reflection(point, unit(normal)))?)
}

// Picking faces and edges.

/// The face of `body` nearest `point` (a point on it, away from its edges).
pub fn face_at(k: &DefaultKernel, body: BodyId, point: P3) -> Result<FaceId, Refusal> {
    let faces = k.faces(body)?;
    let mut best: Option<(f64, FaceId)> = None;
    for f in &faces {
        let d = k.min_distance(
            &DistanceTarget::Point(point),
            &DistanceTarget::Face(body, f.id),
        )?;
        if best.is_none_or(|(b, _)| d.distance < b) {
            best = Some((d.distance, f.id));
        }
    }
    best.map(|(_, f)| f)
        .ok_or_else(|| Refusal::from("the body has no faces"))
}

/// The faces of `body` through each of `points`.
pub fn faces_at(k: &DefaultKernel, body: BodyId, points: &[P3]) -> Result<Vec<FaceId>, Refusal> {
    points.iter().map(|&p| face_at(k, body, p)).collect()
}

/// The edge of `body` nearest `point` (a point on it, away from its ends).
pub fn edge_at(k: &DefaultKernel, body: BodyId, point: P3) -> Result<EdgeId, Refusal> {
    let edges = k.edges(body)?;
    let mut best: Option<(f64, EdgeId)> = None;
    for e in &edges {
        if e.faces[0].is_some() && e.faces[0] == e.faces[1] {
            continue; // a seam is no edge to pick
        }
        let d = k.min_distance(
            &DistanceTarget::Point(point),
            &DistanceTarget::Edge(body, e.id),
        )?;
        if best.is_none_or(|(b, _)| d.distance < b) {
            best = Some((d.distance, e.id));
        }
    }
    best.map(|(_, e)| e)
        .ok_or_else(|| Refusal::from("the body has no edges"))
}

/// The edges of `body` through each of `points`.
pub fn edges_at(k: &DefaultKernel, body: BodyId, points: &[P3]) -> Result<Vec<EdgeId>, Refusal> {
    points.iter().map(|&p| edge_at(k, body, p)).collect()
}

/// The edges of `body` (seams left out) that `keep` keeps.
pub fn edges_where(
    k: &DefaultKernel,
    body: BodyId,
    keep: impl Fn(&EdgeInfo) -> bool,
) -> Result<Vec<EdgeId>, Refusal> {
    Ok(k.edges(body)?
        .into_iter()
        .filter(|e| !(e.faces[0].is_some() && e.faces[0] == e.faces[1]))
        .filter(|e| keep(e))
        .map(|e| e.id)
        .collect())
}

/// Every edge of `body` but its seams.
pub fn all_edges(k: &DefaultKernel, body: BodyId) -> Result<Vec<EdgeId>, Refusal> {
    edges_where(k, body, |_| true)
}

/// The faces of `body` that `keep` keeps.
pub fn faces_where(
    k: &DefaultKernel,
    body: BodyId,
    keep: impl Fn(&FaceInfo) -> bool,
) -> Result<Vec<FaceId>, Refusal> {
    Ok(k.faces(body)?
        .into_iter()
        .filter(|f| keep(f))
        .map(|f| f.id)
        .collect())
}

/// Where a point lies on `body`: the face, edge or vertex nearest it.
pub fn location(k: &DefaultKernel, body: BodyId, point: P3) -> Result<DistanceLocation, Refusal> {
    Ok(
        k.min_distance(&DistanceTarget::Point(point), &DistanceTarget::Body(body))?
            .b_at,
    )
}
