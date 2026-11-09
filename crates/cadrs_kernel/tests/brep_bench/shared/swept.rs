//! What the cadrs implementations of the swept cases share: the rod and the ring sector several of
//! them groove, a slide, a swing or a screw of a solid (cadrs's sweep of a solid,
//! `Kernel::sweep_solid`), and the solid a torus fills as it slides along the z axis.
use crate::Refusal;
use crate::bench::{
    P3, V3, arc, axis, chain, cylinder, extrude, line, p2, p3, plane, sketch, torus, v3, xy,
};
use crate::shared::sweep::first;
use cadrs_kernel::{Axis, BodyId, CylindricalBox, DefaultKernel, Kernel, RigidStep, SolidMotion};

/// The rod: the cylinder of radius 5 about the z axis from z = 0 to z = 20.
pub fn rod(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 20.0)
}

/// The block of a ring sector about the z axis: everything between the radii `radii`, the
/// heights `heights` and the polar angles `angles` (degrees, from +x toward +y), cadrs's
/// cylindrical box (an annular sector extruded).
pub fn ring_sector(
    k: &mut DefaultKernel,
    radii: (f64, f64),
    heights: (f64, f64),
    angles: (f64, f64),
) -> Result<BodyId, Refusal> {
    let start = angles.0.rem_euclid(360.0);
    let spec = CylindricalBox {
        frame: plane(
            p3(0.0, 0.0, heights.0),
            v3(1.0, 0.0, 0.0),
            v3(0.0, 0.0, 1.0),
        ),
        length: heights.1 - heights.0,
        inner_radius: radii.0,
        outer_radius: radii.1,
        start_angle: start.to_radians(),
        end_angle: (start + angles.1 - angles.0).to_radians(),
    };
    first(k.cylindrical_box(&spec)?)
}

/// The z axis.
pub fn z_axis() -> Axis {
    axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))
}

/// Everything `tool` passes through as it slides without turning along `v` (cadrs's sweep of a
/// solid, one slide).
pub fn slid(k: &mut DefaultKernel, tool: BodyId, v: V3) -> Result<BodyId, Refusal> {
    let steps = vec![RigidStep::Slide {
        from: V3::zeros(),
        to: v,
    }];
    first(k.sweep_solid(tool, &SolidMotion::Rigid { steps })?)
}

/// Everything `tool` (drawn where it is at a turn of 0) passes through as it turns rigidly about
/// `about`, right-handed, from `from` to `to` degrees (cadrs's sweep of a solid, one turn).
pub fn swung(
    k: &mut DefaultKernel,
    tool: BodyId,
    about: Axis,
    from: f64,
    to: f64,
) -> Result<BodyId, Refusal> {
    let steps = vec![RigidStep::Turn {
        axis: about,
        from: from.to_radians(),
        to: to.to_radians(),
    }];
    first(k.sweep_solid(tool, &SolidMotion::Rigid { steps })?)
}

/// Everything `tool` (drawn where it is at a turn of 0) passes through as it is screwed about
/// `about`: turned right-handed from `from` to `to` degrees while rising `lead` along the axis a
/// whole turn, rigidly (cadrs's sweep of a solid: a turn and a slide along its axis at once).
pub fn screwed(
    k: &mut DefaultKernel,
    tool: BodyId,
    about: Axis,
    lead: f64,
    from: f64,
    to: f64,
) -> Result<BodyId, Refusal> {
    let z = about.dir.into_inner();
    let rise = |deg: f64| z * (lead * deg / 360.0);
    let steps = vec![
        RigidStep::Turn {
            axis: about,
            from: from.to_radians(),
            to: to.to_radians(),
        },
        RigidStep::Slide {
            from: rise(from),
            to: rise(to),
        },
    ];
    first(k.sweep_solid(tool, &SolidMotion::Rigid { steps })?)
}

/// Everything `tool` (drawn in the frame where `path` starts) passes through as a frame rides
/// `path` upright: its origin the path's point, its x axis the path's tangent, its z axis up
/// (cadrs's sweep of a solid along a path, meshed).
pub fn carried(
    k: &mut DefaultKernel,
    tool: BodyId,
    path: Vec<cadrs_kernel::PathCurve>,
) -> Result<BodyId, Refusal> {
    let motion = SolidMotion::Path {
        path,
        up: crate::bench::unit(v3(0.0, 0.0, 1.0)),
    };
    first(k.sweep_solid(tool, &motion)?)
}

/// The cubic Bézier curve in the plane z = 0 on these control points, as a path.
pub fn bezier_path(poles: [(f64, f64); 4]) -> Vec<cadrs_kernel::PathCurve> {
    vec![cadrs_kernel::PathCurve::Bezier3 {
        poles: poles.iter().map(|&(x, y)| p3(x, y, 0.0)).collect(),
        weights: vec![],
    }]
}

/// `target` less everything `tool` passes through as it moves by `motion` (cadrs's cut by a swept
/// solid: the exact sweep cut from it, else the region inside it and in none of the tool's
/// places, meshed).
pub fn cut_swept(
    k: &mut DefaultKernel,
    target: BodyId,
    tool: BodyId,
    motion: &SolidMotion,
) -> Result<BodyId, Refusal> {
    first(k.cut_swept(target, tool, motion)?)
}

/// The motion of [`slid`].
pub fn slide(v: V3) -> SolidMotion {
    SolidMotion::Rigid {
        steps: vec![RigidStep::Slide {
            from: V3::zeros(),
            to: v,
        }],
    }
}

/// The motion of [`swung`].
pub fn swing(about: Axis, from: f64, to: f64) -> SolidMotion {
    SolidMotion::Rigid {
        steps: vec![RigidStep::Turn {
            axis: about,
            from: from.to_radians(),
            to: to.to_radians(),
        }],
    }
}

/// The motion of [`screwed`].
pub fn screw(about: Axis, lead: f64, from: f64, to: f64) -> SolidMotion {
    let z = about.dir.into_inner();
    let rise = |deg: f64| z * (lead * deg / 360.0);
    SolidMotion::Rigid {
        steps: vec![
            RigidStep::Turn {
                axis: about,
                from: from.to_radians(),
                to: to.to_radians(),
            },
            RigidStep::Slide {
                from: rise(from),
                to: rise(to),
            },
        ],
    }
}

/// The motion of [`carried`].
pub fn carry(path: Vec<cadrs_kernel::PathCurve>) -> SolidMotion {
    SolidMotion::Path {
        path,
        up: crate::bench::unit(v3(0.0, 0.0, 1.0)),
    }
}

/// The part of `tool` within `radius` of the z axis (cadrs's boolean common with a cylinder about
/// it). A turn or screw about the z axis maps the cylinder of any radius about it onto itself, so
/// what the tool passes through within the rod (radius 5) is what this part of it passes through
/// there, for any radius past 5.
pub fn near_axis(k: &mut DefaultKernel, tool: BodyId, radius: f64) -> Result<BodyId, Refusal> {
    let bb = k.bounding_box(tool)?;
    let (z0, z1) = (bb.min.z - 1.0, bb.max.z + 1.0);
    let c = cylinder(k, p3(0.0, 0.0, z0), v3(0.0, 0.0, 1.0), radius, z1 - z0)?;
    crate::bench::common(k, tool, c)
}

/// What the torus about the axis through `center` parallel to the y axis, of radii `major` and
/// `minor`, fills as it slides without turning `rise` up the z axis: the torus where it starts
/// and where it ends, and between them the region it covers seen along the slide (its shadow on
/// the plane z = `center.z`, the stadium of half-width `minor` about the segment from
/// `center.x − major` to `center.x + major` along x) extruded `rise` up. Every point the torus
/// passes lies in one of them.
pub fn torus_slid(
    k: &mut DefaultKernel,
    center: P3,
    major: f64,
    minor: f64,
    rise: f64,
) -> Result<Vec<BodyId>, Refusal> {
    let start = torus(k, center, v3(0.0, 1.0, 0.0), major, minor)?;
    let end = torus(
        k,
        center + v3(0.0, 0.0, rise),
        v3(0.0, 1.0, 0.0),
        major,
        minor,
    )?;
    let (x0, x1, y, r) = (center.x - major, center.x + major, center.y, minor);
    let stadium = chain(vec![
        line(p2(x0, y - r), p2(x1, y - r)),
        arc(p2(x1, y), r, -90.0, 180.0),
        line(p2(x1, y + r), p2(x0, y + r)),
        arc(p2(x0, y), r, 90.0, 180.0),
    ]);
    let shadow = extrude(k, &sketch(xy(center.z), vec![stadium]), rise)?;
    Ok(vec![start, shadow, end])
}

/// A tumbling torus (`swept/tumbling-torus-*`): what the torus about `center` (axis along z, major
/// radius 1, tube radius `tube`) fills as, at each roll t from `rolls.0` to `rolls.1` degrees, it is
/// turned by `rate`·t about the upright line through (2, 0, 0), then by `sign`·t about the line
/// through q = (0, h cos ℓ, h sin ℓ) along u = (cos α, sin α cos ℓ, sin α sin ℓ), `(ℓ, α, h)` the
/// second axis's lean, tilt and offset (degrees, degrees, length). Cadrs's sweep of a solid, with
/// the rigid motion of its two turns, the second axis fixed in the world.
pub fn tumbling_torus(
    k: &mut DefaultKernel,
    center: P3,
    tube: f64,
    (lean, tilt, h): (f64, f64, f64),
    rate: f64,
    sign: f64,
    rolls: (f64, f64),
) -> Result<BodyId, Refusal> {
    let t = crate::bench::torus(k, center, v3(0.0, 0.0, 1.0), 1.0, tube)?;
    let (l, a) = (lean.to_radians(), tilt.to_radians());
    let q = p3(0.0, h * l.cos(), h * l.sin());
    let u = v3(a.cos(), a.sin() * l.cos(), a.sin() * l.sin());
    let (t0, t1) = (rolls.0.to_radians(), rolls.1.to_radians());
    let motion = SolidMotion::Rigid {
        steps: vec![
            RigidStep::Turn {
                axis: axis(p3(2.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)),
                from: rate * t0,
                to: rate * t1,
            },
            RigidStep::Turn {
                axis: axis(q, u),
                from: sign * t0,
                to: sign * t1,
            },
        ],
    };
    first(k.sweep_solid(t, &motion)?)
}

/// The two operands of `swept/tumbling-torus-crossed-*`: the torus (center (3, 0, 2), tube 0.22)
/// rolled from −100° to 100° at 0.7 about the upright axis, the second axis leaning 45° and
/// tilted 20° at 0.7071, turned against the roll (A) and with it (B).
pub fn crossed_tori(k: &mut DefaultKernel) -> Result<(BodyId, BodyId), Refusal> {
    let a = tumbling_torus(
        k,
        p3(3.0, 0.0, 2.0),
        0.22,
        (45.0, 20.0, 0.7071),
        0.7,
        -1.0,
        (-100.0, 100.0),
    )?;
    let b = tumbling_torus(
        k,
        p3(3.0, 0.0, 2.0),
        0.22,
        (45.0, 20.0, 0.7071),
        0.7,
        1.0,
        (-100.0, 100.0),
    )?;
    Ok((a, b))
}
