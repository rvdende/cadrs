//! What the cadrs implementations of the thicken cases share: sheets made as cadrs's Part Studio
//! makes them (a surface extrude or a surface revolve of sketch curves, a sketch region, faces
//! picked on a part) and cadrs's Thicken of them, to the side the case names.
use crate::Refusal;
use crate::bench::{P3, V3, arc, line, p2, p3, unit, v3, xz};
use cadrs_kernel::{
    Axis, BodyId, BodyKind, Chain, Curve2, DefaultKernel, ExtrudeSpec, FaceId, FaceInput, Kernel,
    Plane, Profile, RevolveSpec, ThickenSpec,
};

/// The first body of an operation's result.
fn first(r: cadrs_kernel::OpResult) -> Result<BodyId, Refusal> {
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the operation made no body"))
}

/// A sketch on `plane` of the open chain of `curves` (no region).
pub fn open_chain(plane: Plane, curves: Vec<Curve2>) -> Profile {
    Profile {
        plane,
        regions: vec![],
        chains: vec![Chain {
            curves,
            source: None,
        }],
    }
}

/// The open polyline through `points` (in the plane's coordinates).
pub fn polyline(points: &[(f64, f64)]) -> Vec<Curve2> {
    points
        .windows(2)
        .map(|w| line(p2(w[0].0, w[0].1), p2(w[1].0, w[1].1)))
        .collect()
}

/// The sheet `profile`'s curves sweep as they are extruded `depth` along `dir` (a surface
/// extrude).
pub fn surface_extrude(
    k: &mut DefaultKernel,
    profile: &Profile,
    dir: V3,
    depth: f64,
) -> Result<BodyId, Refusal> {
    let spec = ExtrudeSpec {
        body: BodyKind::Surface,
        ..ExtrudeSpec::blind(unit(dir), depth)
    };
    first(k.extrude_with(profile, &spec)?)
}

/// The sheet `profile`'s curves sweep as they are revolved about `axis` by `angle` degrees, or
/// a whole turn when `None` (a surface revolve).
pub fn surface_revolve(
    k: &mut DefaultKernel,
    profile: &Profile,
    axis: Axis,
    angle: Option<f64>,
) -> Result<BodyId, Refusal> {
    let spec = RevolveSpec {
        body: BodyKind::Surface,
        ..RevolveSpec::solid(axis, angle.map(f64::to_radians))
    };
    first(k.revolve_with(profile, &spec)?)
}

/// The angle sheet of the angle-bracket cases: the floor (the rectangle in the xy plane from the
/// origin to (10, 8, 0)) and the wall (the rectangle in the yz plane from the origin to
/// (0, 8, 10)), the open polyline (10, 0), (0, 0), (0, 10) on the Front plane (its (x, z))
/// surface-extruded 8 along +y.
pub fn angle_sheet(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = open_chain(xz(0.0), polyline(&[(10.0, 0.0), (0.0, 0.0), (0.0, 10.0)]));
    surface_extrude(k, &s, v3(0.0, 1.0, 0.0), 8.0)
}

/// The bent sheet of the bent-flange cases: on the Front plane (its (x, z)), the line from
/// (−10, 0) to (0, 0), the quarter circle of radius 2 about (0, 2) to (2, 2) and the line up to
/// (2, 12), surface-extruded 8 along +y.
pub fn flange_sheet(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let profile = vec![
        line(p2(-10.0, 0.0), p2(0.0, 0.0)),
        arc(p2(0.0, 2.0), 2.0, -90.0, 90.0),
        line(p2(2.0, 2.0), p2(2.0, 12.0)),
    ];
    surface_extrude(k, &open_chain(xz(0.0), profile), v3(0.0, 1.0, 0.0), 8.0)
}

/// The tray of the open-box cases: the cube from the origin to (10, 10, 10), a square sketched
/// on the Top plane and extruded 10, and its floor and four walls picked (not its top), their
/// normals pointing out of the cube.
pub fn tray(k: &mut DefaultKernel) -> Result<(BodyId, Vec<FaceId>), Refusal> {
    let cube = crate::bench::block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let faces = crate::bench::faces_at(
        k,
        cube,
        &[
            p3(5.0, 5.0, 0.0),
            p3(0.0, 5.0, 5.0),
            p3(10.0, 5.0, 5.0),
            p3(5.0, 0.0, 5.0),
            p3(5.0, 10.0, 5.0),
        ],
    )?;
    Ok((cube, faces))
}

/// Thickens the sheet `sheet`: `along` along its faces' normals and `against` against them,
/// its bends rounded.
pub fn thicken(
    k: &mut DefaultKernel,
    sheet: BodyId,
    along: f64,
    against: f64,
) -> Result<BodyId, Refusal> {
    thicken_with(k, sheet, along, against, false)
}

/// [`thicken`], its bends rounded, or mitred with `sharp` (the moved faces extended until they
/// meet).
pub fn thicken_with(
    k: &mut DefaultKernel,
    sheet: BodyId,
    along: f64,
    against: f64,
    sharp: bool,
) -> Result<BodyId, Refusal> {
    let spec = ThickenSpec {
        bodies: vec![sheet],
        faces: vec![],
        profiles: vec![],
        along,
        against,
        sharp,
        source: 1,
    };
    first(k.thicken_surfaces(&spec)?)
}

/// Thickens the faces `faces` of the part `body`: `along` along their normals and `against`
/// against them.
pub fn thicken_faces(
    k: &mut DefaultKernel,
    body: BodyId,
    faces: &[FaceId],
    along: f64,
    against: f64,
) -> Result<BodyId, Refusal> {
    thicken_faces_with(k, body, faces, along, against, false)
}

/// [`thicken_faces`], their corners rounded, or mitred with `sharp` (the moved faces extended
/// until they meet).
pub fn thicken_faces_with(
    k: &mut DefaultKernel,
    body: BodyId,
    faces: &[FaceId],
    along: f64,
    against: f64,
    sharp: bool,
) -> Result<BodyId, Refusal> {
    let faces = faces
        .iter()
        .map(|&face| FaceInput {
            body,
            face,
            source: 0,
        })
        .collect();
    let spec = ThickenSpec {
        bodies: vec![],
        faces,
        profiles: vec![],
        along,
        against,
        sharp,
        source: 1,
    };
    first(k.thicken_surfaces(&spec)?)
}

/// Thickens the regions of the sketch `profile` (their faces' normals along the plane's):
/// `along` along the plane's normal and `against` against it.
pub fn thicken_sketch(
    k: &mut DefaultKernel,
    profile: Profile,
    along: f64,
    against: f64,
) -> Result<BodyId, Refusal> {
    let spec = ThickenSpec {
        bodies: vec![],
        faces: vec![],
        profiles: vec![profile],
        along,
        against,
        sharp: false,
        source: 1,
    };
    first(k.thicken_surfaces(&spec)?)
}

/// Thickens the sheet `sheet` by `t` to the side `toward` points to at `at`, a point on it, as a
/// user flips Thicken's direction arrow until it points that way: along its normal there when
/// the normal points that way, else against it.
pub fn thicken_toward(
    k: &mut DefaultKernel,
    sheet: BodyId,
    at: P3,
    toward: V3,
    t: f64,
) -> Result<BodyId, Refusal> {
    thicken_toward_with(k, sheet, at, toward, t, false)
}

/// [`thicken_toward`], its bends rounded, or mitred with `sharp` (the moved faces extended until
/// they meet).
pub fn thicken_toward_with(
    k: &mut DefaultKernel,
    sheet: BodyId,
    at: P3,
    toward: V3,
    t: f64,
    sharp: bool,
) -> Result<BodyId, Refusal> {
    let face = crate::bench::face_at(k, sheet, at)?;
    let n = k.face_normals_at(sheet, face, &[at])?;
    let n = n
        .first()
        .ok_or_else(|| Refusal::from("the sheet has no normal there"))?;
    if n.dot(&toward) > 0.0 {
        thicken_with(k, sheet, t, 0.0, sharp)
    } else {
        thicken_with(k, sheet, 0.0, t, sharp)
    }
}
