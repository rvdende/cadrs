//! Cases of brep-bench (https://brepbench.org, https://github.com/bmander/brep-bench) the
//! kernel once got wrong, built as its cadrs runner builds them (sketches extruded and revolved,
//! through the `Kernel` trait) and measured against the cases' exact answers.
use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Unit, Vector3};

type P3 = Point3<f64>;
type V3 = Vector3<f64>;

fn p2(x: f64, y: f64) -> Point2<f64> {
    Point2::new(x, y)
}

fn p3(x: f64, y: f64, z: f64) -> P3 {
    P3::new(x, y, z)
}

fn plane(origin: P3, x_dir: V3, normal: V3) -> Plane {
    let n = Unit::new_normalize(normal);
    let x = Unit::new_normalize(x_dir - n.into_inner() * x_dir.dot(&n));
    Plane {
        origin,
        x_dir: x,
        normal: n,
    }
}

/// The plane z = `z` (the Top plane).
fn xy(z: f64) -> Plane {
    plane(p3(0.0, 0.0, z), V3::x(), V3::z())
}

/// The plane y = 0 with sketch (x, y) the model's (x, z) (the Front plane).
fn xz() -> Plane {
    plane(p3(0.0, 0.0, 0.0), V3::x(), -V3::y())
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve2 {
    Curve2::Line {
        a: p2(a.0, a.1),
        b: p2(b.0, b.1),
        source: None,
    }
}

/// An arc about `c`, from `start` sweeping `sweep` (degrees).
fn arc(c: (f64, f64), r: f64, start: f64, sweep: f64) -> Curve2 {
    Curve2::Arc {
        center: p2(c.0, c.1),
        radius: r,
        start_angle: start.to_radians(),
        sweep: sweep.to_radians(),
        source: None,
    }
}

fn polygon(pts: &[(f64, f64)]) -> Loop {
    Loop {
        curves: (0..pts.len())
            .map(|i| line(pts[i], pts[(i + 1) % pts.len()]))
            .collect(),
    }
}

fn sketch(plane: Plane, outer: Loop, holes: Vec<Loop>) -> Profile {
    Profile::new(
        plane,
        vec![Region {
            outer,
            holes,
            source: None,
        }],
    )
}

fn extrude(k: &mut NativeKernel, p: &Profile, depth: f64) -> BodyId {
    k.extrude_with(p, &ExtrudeSpec::blind(p.plane.normal, depth))
        .unwrap()
        .bodies[0]
}

/// `p` revolved a whole turn about the z axis.
fn revolve_z(k: &mut NativeKernel, p: &Profile) -> BodyId {
    let axis = Axis {
        origin: p3(0.0, 0.0, 0.0),
        dir: Unit::new_normalize(V3::z()),
    };
    k.revolve_with(p, &RevolveSpec::solid(axis, None))
        .unwrap()
        .bodies[0]
}

fn block(k: &mut NativeKernel, min: P3, max: P3) -> BodyId {
    let s = sketch(
        xy(min.z),
        polygon(&[
            (min.x, min.y),
            (max.x, min.y),
            (max.x, max.y),
            (min.x, max.y),
        ]),
        vec![],
    );
    extrude(k, &s, max.z - min.z)
}

/// The ball of radius `r` about the origin: a half disc revolved.
fn ball(k: &mut NativeKernel, r: f64) -> BodyId {
    let half = Loop {
        curves: vec![arc((0.0, 0.0), r, -90.0, 180.0), line((0.0, r), (0.0, -r))],
    };
    revolve_z(k, &sketch(xz(), half, vec![]))
}

/// The face of `body` through `p`.
fn face_at(k: &NativeKernel, body: BodyId, p: P3) -> FaceId {
    let b = k.get(body).unwrap();
    (0..b.faces.len())
        .map(|f| FaceId(f as u64))
        .min_by(|a, c| {
            let d = |f: &FaceId| b.faces[f.0 as usize].surf.distance(&p);
            d(a).total_cmp(&d(c))
        })
        .unwrap()
}

/// That the body is one valid solid with this volume and area (to 1e-6), and that its mesh is
/// closed.
fn measures(k: &NativeKernel, body: BodyId, volume: f64, area: f64) {
    let m = k.mass_properties(body).unwrap();
    assert!(
        (m.volume - volume).abs() < 1e-6 * volume && (m.surface_area - area).abs() < 1e-6 * area,
        "volume {} against {volume}, area {} against {area}",
        m.volume,
        m.surface_area
    );
    let b = k.get(body).unwrap();
    let issues = b.check_with(cadrs_kernel::native::CheckLevel::Full);
    assert!(issues.is_empty(), "{issues:?}");
    let size = k.bounding_box(body).unwrap().diagonal();
    let defl = (1e-4 * size).min(5e-3 * volume / area);
    let mesh = k.tessellate(
        body,
        Tessellation {
            deflection: defl,
            angle: 5f64.to_radians(),
        },
    );
    let check = cadrs_kernel::native::tess::check_tri_mesh(&mesh.unwrap());
    assert!(check.is_watertight(), "{check:?}");
}

/// The regular tetrahedron with corners (3, 3, 3), (3, −3, −3), (−3, 3, −3), (−3, −3, 3): its
/// base triangle lofted to its apex.
fn tetrahedron(k: &mut NativeKernel) -> BodyId {
    let apex = p3(3.0, 3.0, 3.0);
    let base = [
        p3(3.0, -3.0, -3.0),
        p3(-3.0, 3.0, -3.0),
        p3(-3.0, -3.0, 3.0),
    ];
    let normal = (base[1] - base[0]).cross(&(base[2] - base[0]));
    let normal = if normal.dot(&(apex - base[0])) < 0.0 {
        -normal
    } else {
        normal
    };
    let on = plane(base[0], base[1] - base[0], normal);
    let y = on.normal.cross(&on.x_dir);
    let at = |p: P3| {
        let d = p - on.origin;
        (d.dot(&on.x_dir), d.dot(&y))
    };
    let tri = sketch(on, polygon(&base.map(at)), vec![]);
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections: vec![LoftSection::Profile(tri), LoftSection::Point(apex)],
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 1,
    };
    k.loft_with(&spec).unwrap().bodies[0]
}

/// A cone on the z axis, base radius 3 at z = 0, apex (0, 0, 4): its section revolved.
fn cone(k: &mut NativeKernel) -> BodyId {
    revolve_z(
        k,
        &sketch(xz(), polygon(&[(0.0, 0.0), (3.0, 0.0), (0.0, 4.0)]), vec![]),
    )
}

fn sheet_of(k: &mut NativeKernel, p: &Profile, ext: bool) -> BodyId {
    if ext {
        let spec = ExtrudeSpec {
            body: BodyKind::Surface,
            ..ExtrudeSpec::blind(Unit::new_normalize(V3::y()), 8.0)
        };
        k.extrude_with(p, &spec).unwrap().bodies[0]
    } else {
        let axis = Axis {
            origin: p3(0.0, 0.0, 0.0),
            dir: Unit::new_normalize(V3::z()),
        };
        let spec = RevolveSpec {
            body: BodyKind::Surface,
            ..RevolveSpec::solid(axis, None)
        };
        k.revolve_with(p, &spec).unwrap().bodies[0]
    }
}

fn open_chain(curves: Vec<Curve2>) -> Profile {
    Profile {
        plane: xz(),
        regions: vec![],
        chains: vec![Chain {
            curves,
            source: None,
        }],
    }
}

/// `sheet` thickened by `t` toward `toward` from its face at `at`.
fn thickened(k: &mut NativeKernel, sheet: BodyId, at: P3, toward: V3, t: f64) -> BodyId {
    let face = face_at(k, sheet, at);
    let n = k.face_normals_at(sheet, face, &[at]).unwrap()[0];
    let (along, against) = if n.dot(&toward) > 0.0 {
        (t, 0.0)
    } else {
        (0.0, t)
    };
    let spec = ThickenSpec {
        bodies: vec![sheet],
        faces: vec![],
        profiles: vec![],
        along,
        against,
        source: 1,
    };
    k.thicken_surfaces(&spec).unwrap().bodies[0]
}

/// `body` shelled inward `t`, open at its faces through `openings`, its cavity rounded.
fn shelled(k: &mut NativeKernel, body: BodyId, openings: &[P3], t: f64) -> BodyId {
    let remove = openings.iter().map(|&p| face_at(k, body, p)).collect();
    let spec = ShellSpec {
        remove,
        thickness: t,
        outward: false,
        hollow: false,
        sharp: false,
    };
    k.shell_with(body, &spec).unwrap().bodies[0]
}

fn grown(k: &mut NativeKernel, body: BodyId, d: f64) -> BodyId {
    k.offset(
        body,
        &OffsetSpec {
            distance: d,
            faces: vec![],
            sharp: false,
        },
    )
    .unwrap()
    .bodies[0]
}

/// tangent/thin-lens: two balls of radius 5 overlapping 10⁻⁶ deep, their common lens, modelled
/// a hundred times finer than the default merge distance (as thick as the lens).
#[test]
fn a_lens_a_millionth_thick() {
    let mut k = NativeKernel::new();
    k.set_tolerances(cadrs_kernel::native::tol::Tolerances {
        confusion: 1e-9,
        point: 1e-8,
        retries: [1e-7, 1e-6],
    })
    .unwrap();
    let a = ball(&mut k, 5.0);
    let b = ball(&mut k, 5.0);
    let b = k
        .transform(b, &Transform::translation(9.999999, 0.0, 0.0))
        .unwrap()
        .bodies[0];
    let lens = k.boolean(BoolOp::Intersect, a, &[b]).unwrap().bodies[0];
    let m = k.mass_properties(lens).unwrap();
    let (v, a) = (7.853981372175095e-12, 3.1415926535897935e-05);
    assert!((m.volume - v).abs() < 1e-6 * v, "{}", m.volume);
    assert!((m.surface_area - a).abs() < 1e-6 * a, "{}", m.surface_area);
}

/// thicken/hemisphere-to-its-center: a hemisphere sheet thickened by its radius toward its
/// centre, the moved face closing to the centre: the half ball.
#[test]
fn a_hemisphere_thickened_to_its_centre() {
    let mut k = NativeKernel::new();
    let sheet = sheet_of(
        &mut k,
        &open_chain(vec![arc((0.0, 0.0), 5.0, 0.0, 90.0)]),
        false,
    );
    let at = p3(0.0, 1.0, 1.0) * (5.0 / 2f64.sqrt());
    let half = thickened(&mut k, sheet, at, -at.coords, 5.0);
    measures(&k, half, 261.79938779914943, 235.61944901923448);
}

/// thicken/bent-flange-past-its-bend: a bent sheet thickened by 3 inside its bend of radius 2,
/// the bend's face moved through its axis.
#[test]
fn a_bent_sheet_thickened_past_its_bend() {
    let mut k = NativeKernel::new();
    let profile = vec![
        line((-10.0, 0.0), (0.0, 0.0)),
        arc((0.0, 2.0), 2.0, -90.0, 90.0),
        line((2.0, 2.0), (2.0, 12.0)),
    ];
    let sheet = sheet_of(&mut k, &open_chain(profile), true);
    let flange = thickened(&mut k, sheet, p3(-5.0, 4.0, 0.0), V3::z(), 3.0);
    measures(&k, flange, 497.1327412287184, 501.41592653589794);
}

/// draft/box-top-drafted-away: a box's four sides drafted 30° until they meet below its top.
#[test]
fn a_box_drafted_until_its_top_is_gone() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, p3(0.0, 0.0, 0.0), p3(30.0, 10.0, 10.0));
    let faces = [
        p3(0.0, 5.0, 5.0),
        p3(30.0, 5.0, 5.0),
        p3(15.0, 0.0, 5.0),
        p3(15.0, 10.0, 5.0),
    ]
    .map(|p| face_at(&k, b, p))
    .to_vec();
    let spec = DraftSpec {
        faces,
        angle: 30f64.to_radians(),
        pull: Unit::new_normalize(V3::z()),
        neutral: p3(0.0, 0.0, 0.0),
        neutral_normal: None,
        tangent_propagation: false,
    };
    let roof = k.draft(b, &spec).unwrap().bodies[0];
    measures(&k, roof, 1154.7005383792516, 900.0);
    assert_eq!(k.get(roof).unwrap().faces.len(), 5);
}

/// shell/box-walls-meeting: walls half the box's width thick meet everywhere: the box itself.
#[test]
fn a_box_shelled_to_half_its_width_is_the_box() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0));
    let s = shelled(&mut k, b, &[p3(5.0, 4.0, 6.0)], 4.0);
    measures(&k, s, 480.0, 376.0);
    assert_eq!(k.get(s).unwrap().faces.len(), 6);
}

/// shell/capsule-open-at-its-dome: the side moved in runs on up into the dome it opens at.
#[test]
fn a_capsule_shelled_open_at_its_dome() {
    let mut k = NativeKernel::new();
    let section = Loop {
        curves: vec![
            line((0.0, 0.0), (5.0, 0.0)),
            line((5.0, 0.0), (5.0, 10.0)),
            arc((0.0, 10.0), 5.0, 0.0, 90.0),
            line((0.0, 15.0), (0.0, 0.0)),
        ],
    };
    let c = revolve_z(&mut k, &sketch(xz(), section, vec![]));
    let s = shelled(&mut k, c, &[p3(0.0, 0.0, 15.0)], 1.0);
    measures(&k, s, 389.55748904513433, 838.8052385084748);
}

/// shell/hole-skin-merging-wall: the skin round a bore near a side runs into that side's wall.
#[test]
fn a_bore_skin_merging_with_a_wall() {
    let mut k = NativeKernel::new();
    let hole = Loop {
        curves: vec![Curve2::Circle {
            center: p2(15.5, 10.0),
            radius: 3.0,
            source: None,
        }],
    };
    let s = sketch(
        xy(0.0),
        polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]),
        vec![hole],
    );
    let b = extrude(&mut k, &s, 10.0);
    let s = shelled(&mut k, b, &[p3(2.0, 2.0, 10.0)], 1.0);
    measures(&k, s, 1241.873577515697, 2534.8987556529105);
}

/// shell/sphere-opening-sealed: the sphere moved in never reaches the opening: a closed void.
#[test]
fn a_ball_shelled_short_of_its_opening() {
    let mut k = NativeKernel::new();
    let b = ball(&mut k, 5.0);
    let above = block(&mut k, p3(-6.0, -6.0, 4.5), p3(6.0, 6.0, 6.0));
    let capped = k.boolean(BoolOp::Subtract, b, &[above]).unwrap().bodies[0];
    let s = shelled(&mut k, capped, &[p3(0.5, 0.5, 4.5)], 1.0);
    measures(&k, s, 251.7201113688822, 514.4357970253286);
}

/// offset/cone-grown: a cone grown by 1, its apex a ball's cap.
#[test]
fn a_cone_grown() {
    let mut k = NativeKernel::new();
    let c = cone(&mut k);
    let g = grown(&mut k, c, 1.0);
    measures(&k, g, 150.72175801789774, 154.83585886827183);
}

/// offset/torus-grown-past-its-hole: a torus's tube grown wider than its hole: an apple.
#[test]
fn a_torus_grown_past_its_hole() {
    let mut k = NativeKernel::new();
    let tube = Loop {
        curves: vec![Curve2::Circle {
            center: p2(4.0, 0.0),
            radius: 2.0,
            source: None,
        }],
    };
    let t = revolve_z(&mut k, &sketch(xz(), tube, vec![]));
    let g = grown(&mut k, t, 3.0);
    measures(&k, g, 1984.2874392993506, 816.3344428255867);
}

/// offset/tetrahedron-grown: a lofted tetrahedron (its edges straight B-splines) grown by 1.
#[test]
fn a_tetrahedron_grown() {
    let mut k = NativeKernel::new();
    let t = tetrahedron(&mut k);
    let g = grown(&mut k, t, 1.0);
    measures(&k, g, 249.53323018738044, 234.54759243458807);
}

/// singular/sectors-sharing-an-apex: two spherical sectors with one apex, their cones' sides
/// meeting in two rulings through it, common to both. The rulings are lines (a fit of them
/// crowded its knots toward the apex, where the faces either side merged their samples
/// differently and the mesh came apart).
#[test]
fn sectors_sharing_an_apex() {
    let mut k = NativeKernel::new();
    let rim = (
        10.0 * 45f64.to_radians().sin(),
        10.0 * 45f64.to_radians().cos(),
    );
    let section = Loop {
        curves: vec![
            line((0.0, 0.0), rim),
            arc((0.0, 0.0), 10.0, 45.0, 45.0),
            line((0.0, 10.0), (0.0, 0.0)),
        ],
    };
    let a = revolve_z(&mut k, &sketch(xz(), section, vec![]));
    let axis = Axis {
        origin: p3(0.0, 0.0, 0.0),
        dir: Unit::new_normalize(V3::y()),
    };
    let b = k
        .transform_motion(a, &Motion::rotation(&axis, 60f64.to_radians()))
        .unwrap()
        .bodies[0];
    let c = k.boolean(BoolOp::Intersect, a, &[b]).unwrap().bodies[0];
    let body = k.get(c).unwrap();
    let lines = body
        .edges
        .iter()
        .filter(|e| !e.degenerate && matches!(e.curve, cadrs_kernel::native::Curve::Line { .. }))
        .count();
    assert!(lines >= 2, "{lines} straight edges");
    measures(&k, c, 146.51640605521118, 179.05709358777133);
}

/// geometry/ellipsoid: a ball of radius 1 stretched by 6, 4 and 3 along x, y and z.
#[test]
fn a_ball_stretched_into_an_ellipsoid() {
    let mut k = NativeKernel::new();
    let b = ball(&mut k, 1.0);
    let e = k
        .scale_axes(b, p3(0.0, 0.0, 0.0), V3::new(6.0, 4.0, 3.0))
        .unwrap()
        .bodies[0];
    measures(&k, e, 96.0 * std::f64::consts::PI, 231.24821501597344);
}

/// A solid sweep of `profile` along `path` with a law.
fn law_sweep(
    k: &mut NativeKernel,
    profile: &Profile,
    path: Vec<PathCurve>,
    twist: f64,
    scale: f64,
) -> BodyId {
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path,
        control: SweepControl::None,
        faces: vec![],
    };
    k.sweep_with_law(profile, &spec, &SweepLaw { twist, scale })
        .unwrap()
        .bodies[0]
}

/// loft/twisted-tapering-square: a square of side 30 swept 40 up the z axis, turning half a turn
/// and scaled to a third.
#[test]
fn a_square_tapering_as_it_twists() {
    let mut k = NativeKernel::new();
    let foot = sketch(
        xy(0.0),
        polygon(&[(-15.0, -15.0), (15.0, -15.0), (15.0, 15.0), (-15.0, 15.0)]),
        vec![],
    );
    let path = vec![PathCurve::Sketch {
        plane: xz(),
        curve: line((0.0, 0.0), (0.0, 40.0)),
    }];
    let c = law_sweep(&mut k, &foot, path, std::f64::consts::PI, 1.0 / 3.0);
    measures(&k, c, 17333.333333333332, 4643.764402962483);
}

/// The square of half-width 9 − 3θ/90° − `inset` about the arc of radius 30 round the z axis,
/// swept along it from `start` to `end` degrees.
fn narrowing(k: &mut NativeKernel, start: f64, end: f64, inset: f64) -> BodyId {
    let h = |deg: f64| 9.0 - 3.0 * deg / 90.0 - inset;
    let (s, c) = start.to_radians().sin_cos();
    let radial = V3::new(c, s, 0.0);
    let on = plane(p3(0.0, 0.0, 0.0), radial, radial.cross(&V3::z()));
    let hs = h(start);
    let profile = sketch(
        on,
        polygon(&[
            (30.0 - hs, -hs),
            (30.0 + hs, -hs),
            (30.0 + hs, hs),
            (30.0 - hs, hs),
        ]),
        vec![],
    );
    let path = vec![PathCurve::Sketch {
        plane: xy(0.0),
        curve: arc((0.0, 0.0), 30.0, start, end - start),
    }];
    law_sweep(k, &profile, path, 0.0, h(end) / hs)
}

/// sweep/narrowing-bend: a hollow square duct narrowing round a quarter turn, its wall 2 thick.
#[test]
fn a_duct_narrowing_round_a_bend() {
    let mut k = NativeKernel::new();
    let outside = narrowing(&mut k, 0.0, 90.0, 0.0);
    // ∫ (2h)²·30 dθ over the quarter turn, h from 9 to 6.
    let v = 4.0 * 30.0 * 57.0 * std::f64::consts::FRAC_PI_2;
    let m = k.mass_properties(outside).unwrap();
    assert!(
        (m.volume - v).abs() < 1e-6 * v,
        "outside {} against {v}",
        m.volume
    );
    let hole = narrowing(&mut k, -1.0, 91.0, 2.0);
    let duct = k
        .boolean(BoolOp::Subtract, outside, &[hole])
        .unwrap()
        .bodies[0];
    measures(&k, duct, 4900.88453960008, 5119.172836922541);
}

/// The primitives (brep-bench's primitives/*): each one valid solid with its exact measures.
#[test]
fn primitives_measure_exactly() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let z = Unit::new_normalize(V3::z());
    let one = |k: &mut NativeKernel, p: Primitive| k.primitive(&p).unwrap().bodies[0];
    let b = one(
        &mut k,
        Primitive::Box {
            min: p3(0.0, 0.0, 0.0),
            max: p3(10.0, 20.0, 30.0),
        },
    );
    measures(&k, b, 6000.0, 2200.0);
    let c = one(
        &mut k,
        Primitive::Cylinder {
            base: p3(0.0, 0.0, 0.0),
            axis: z,
            radius: 5.0,
            height: 10.0,
        },
    );
    measures(&k, c, PI * 250.0, 2.0 * PI * 25.0 + 2.0 * PI * 50.0);
    // a frustum of radii 5 and 2, 6 high: π h (R² + R r + r²) / 3, its slant √(3² + 6²)
    let f = one(
        &mut k,
        Primitive::Cone {
            base: p3(0.0, 0.0, 0.0),
            axis: z,
            base_radius: 5.0,
            top_radius: 2.0,
            height: 6.0,
        },
    );
    measures(
        &k,
        f,
        PI * 6.0 * 39.0 / 3.0,
        PI * (25.0 + 4.0) + PI * 7.0 * 45f64.sqrt(),
    );
    let x = Unit::new_normalize(V3::x());
    let s = one(
        &mut k,
        Primitive::Sphere {
            center: p3(1.0, 2.0, 3.0),
            radius: 5.0,
            axis: x,
            seam: z,
        },
    );
    measures(&k, s, 4.0 / 3.0 * PI * 125.0, 4.0 * PI * 25.0);
    let t = one(
        &mut k,
        Primitive::Torus {
            center: p3(0.0, 0.0, 0.0),
            axis: z,
            major: 10.0,
            minor: 3.0,
        },
    );
    measures(
        &k,
        t,
        2.0 * PI * PI * 10.0 * 9.0,
        4.0 * PI * PI * 10.0 * 3.0,
    );
    // A horn torus, its tube as wide as its ring: 2π²Rr², 4π²Rr.
    let h = one(
        &mut k,
        Primitive::Torus {
            center: p3(0.0, 0.0, 0.0),
            axis: z,
            major: 3.0,
            minor: 3.0,
        },
    );
    measures(&k, h, 2.0 * PI * PI * 27.0, 4.0 * PI * PI * 9.0);
    // A spindle torus (R = 3, r = 5), the solid counted once: 2π(75(π − θ) + 236/3) and
    // 20π(3(π − θ) + 4), θ = acos(3/5).
    let sp = one(
        &mut k,
        Primitive::Torus {
            center: p3(0.0, 0.0, 0.0),
            axis: z,
            major: 3.0,
            minor: 5.0,
        },
    );
    let th = (3.0f64 / 5.0).acos();
    measures(
        &k,
        sp,
        2.0 * PI * (75.0 * (PI - th) + 236.0 / 3.0),
        20.0 * PI * (3.0 * (PI - th) + 4.0),
    );
}

/// The analytic surfaces' patch boxes (their grid's points, each row's and column's sines and
/// cosines taken once, grown by the bilinear error bound) hold every point of the patch and are
/// not much bigger than its points' box.
#[test]
fn analytic_patch_boxes_hold_their_patches() {
    use cadrs_kernel::native::{Frame, Surface};
    let f = Frame::from_xz(
        p3(1.0, -2.0, 0.5),
        V3::new(0.3, 1.0, -0.2),
        V3::new(1.0, 0.2, 0.7),
    );
    let surfs = [
        Surface::Cylinder { f, r: 3.0 },
        Surface::Cone { f, r: 2.0, a: 0.4 },
        Surface::Cone { f, r: 2.0, a: -0.7 },
        Surface::Sphere { f, r: 4.0 },
        Surface::Torus {
            f,
            rmaj: 5.0,
            rmin: 1.5,
        },
    ];
    let mut seed = 7u64;
    let mut rnd = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for s in &surfs {
        for _ in 0..200 {
            let u0 = rnd() * 8.0 - 2.0;
            let u1 = u0 + rnd() * 3.0;
            let (v0, v1) = match s {
                Surface::Sphere { .. } => {
                    let a = rnd() * 3.0 - 1.5;
                    (a, (a + rnd()).min(1.57))
                }
                Surface::Torus { .. } => {
                    let a = rnd() * 8.0 - 4.0;
                    (a, a + rnd() * 2.0)
                }
                _ => {
                    let a = rnd() * 6.0 - 3.0;
                    (a, a + rnd() * 4.0)
                }
            };
            let bb = s.patch_bbox((u0, u1), (v0, v1));
            let (mut lo, mut hi) = (V3::repeat(f64::INFINITY), V3::repeat(f64::NEG_INFINITY));
            let n = 40;
            for i in 0..=n {
                for j in 0..=n {
                    let q = s.eval(
                        u0 + (u1 - u0) * i as f64 / n as f64,
                        v0 + (v1 - v0) * j as f64 / n as f64,
                    );
                    assert!(
                        (0..3).all(|k| q[k] >= bb.min[k] - 1e-12 && q[k] <= bb.max[k] + 1e-12),
                        "{s:?} {u0} {u1} {v0} {v1}: {q:?} outside {bb:?}"
                    );
                    lo = lo.inf(&q.coords);
                    hi = hi.sup(&q.coords);
                }
            }
            let size = (hi - lo).norm().max(1e-9);
            let slack = (bb.max.coords - hi)
                .abs()
                .max()
                .max((lo - bb.min.coords).abs().max());
            assert!(
                slack < 0.35 * size + 1e-9,
                "{s:?}: box {slack} looser than its points' {size}"
            );
        }
    }
}

/// A sketch spline (`Curve2::Spline`) of these knots, poles and weights.
fn spline(degree: usize, knots: Vec<f64>, poles: &[(f64, f64)], weights: Vec<f64>) -> Curve2 {
    Curve2::Spline {
        degree,
        knots,
        poles: poles.iter().map(|&(x, y)| p2(x, y)).collect(),
        weights,
        source: None,
    }
}

/// geometry/periodic-bspline-prism: a periodic quadratic B-spline sketched (its poles and the
/// first two again on even knots) and extruded.
#[test]
fn a_periodic_spline_extruded() {
    let mut k = NativeKernel::new();
    let mut poles = vec![(10.0, 10.0), (-10.0, 10.0), (-10.0, -10.0), (10.0, -10.0)];
    poles.extend_from_within(..2);
    let knots = (0..poles.len() + 3).map(|i| i as f64).collect();
    let c = spline(2, knots, &poles, vec![]);
    assert!(c.is_closed());
    let s = sketch(xy(0.0), Loop { curves: vec![c] }, vec![]);
    let b = extrude(&mut k, &s, 10.0);
    measures(&k, b, 3333.3333333333335, 1315.9567627227589);
}

/// spline/rational-spheroid: a rational quadratic B-spline of two spans and the axis, revolved.
#[test]
fn a_rational_spline_revolved() {
    let mut k = NativeKernel::new();
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let c = spline(
        2,
        vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        &[
            (0.0, -10.0),
            (6.0, -10.0),
            (6.0, 0.0),
            (6.0, 10.0),
            (0.0, 10.0),
        ],
        vec![1.0, w, 1.0, w, 1.0],
    );
    let s = sketch(
        xz(),
        Loop {
            curves: vec![c, line((0.0, 10.0), (0.0, -10.0))],
        },
        vec![],
    );
    let b = revolve_z(&mut k, &s);
    measures(&k, b, 1507.9644737231006, 663.1722477458867);
    // Reversed, the same curve run the other way.
    let c = spline(
        2,
        vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        &[
            (0.0, -10.0),
            (6.0, -10.0),
            (6.0, 0.0),
            (6.0, 10.0),
            (0.0, 10.0),
        ],
        vec![1.0, w, 1.0, w, 1.0],
    );
    let r = c.reversed();
    for s in [0.0, 0.3, 0.5, 0.8, 1.0] {
        assert!(
            (r.point_at(s) - c.point_at(1.0 - s)).norm() < 1e-12,
            "at {s}"
        );
    }
}

/// boolean/steinmetz-common: two cylinders of radius 5 whose axes cross square, in common: their
/// walls meet in two ellipses, exactly (16r³/3 and 16r²).
#[test]
fn a_steinmetz_solid_is_bounded_by_ellipses() {
    let mut k = NativeKernel::new();
    let z = Unit::new_normalize(V3::z());
    let x = Unit::new_normalize(V3::x());
    let a = k
        .primitive(&Primitive::Cylinder {
            base: p3(0.0, 0.0, -10.0),
            axis: z,
            radius: 5.0,
            height: 20.0,
        })
        .unwrap()
        .bodies[0];
    let b = k
        .primitive(&Primitive::Cylinder {
            base: p3(-10.0, 0.0, 0.0),
            axis: x,
            radius: 5.0,
            height: 20.0,
        })
        .unwrap()
        .bodies[0];
    let c = k.boolean(BoolOp::Intersect, a, &[b]).unwrap().bodies[0];
    measures(&k, c, 16.0 * 125.0 / 3.0, 16.0 * 25.0);
    let body = k.get(c).unwrap();
    assert!(
        body.edges
            .iter()
            .filter(|e| !e.degenerate)
            .all(|e| matches!(e.curve, cadrs_kernel::native::Curve::Ellipse { .. })),
        "{:?}",
        body.edges
            .iter()
            .map(|e| format!("{:?}", e.curve)
                .chars()
                .take(12)
                .collect::<String>())
            .collect::<Vec<_>>()
    );
}

/// A cone's frustum far from its apex (tangent/cone-tip-on-face's, its cone drawn from the apex):
/// meshed in a few hundred triangles, not tens of thousands (its metric was the radius at the
/// apex, 0).
#[test]
fn a_frustum_far_from_its_apex_meshes_coarsely() {
    let mut k = NativeKernel::new();
    let z = Unit::new_normalize(V3::z());
    let cone = k
        .primitive(&Primitive::Cone {
            base: p3(0.0, 0.0, 0.0),
            axis: z,
            base_radius: 0.0,
            top_radius: 5.0,
            height: 15.0,
        })
        .unwrap()
        .bodies[0];
    let below = block(&mut k, p3(-6.0, -6.0, -1.0), p3(6.0, 6.0, 10.0));
    let frustum = k.boolean(BoolOp::Subtract, cone, &[below]).unwrap().bodies[0];
    let m = k
        .tessellate(
            frustum,
            Tessellation {
                deflection: 0.032,
                angle: 15f64.to_radians(),
            },
        )
        .unwrap();
    assert!(m.indices.len() < 2000, "{} triangles", m.indices.len());
}

/// A solid swept without end about an axis (`Kernel::sweep_solid`): a cylinder of radius a,
/// height H, standing d from the axis. Turned, it fills the ring d − a ≤ r ≤ d + a (4π a d H);
/// screwed with rise c a turn, its outline at each height is the disk swept round an arc of
/// 2πH/c, of area πa² + 2a·d·2πH/c, and the solid that between the planes.
#[test]
fn a_cylinder_turned_and_screwed() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let z = Unit::new_normalize(V3::z());
    let (a, d, h) = (2.0, 10.0, 3.0);
    let c = k
        .primitive(&Primitive::Cylinder {
            base: p3(d, 0.0, 0.0),
            axis: z,
            radius: a,
            height: h,
        })
        .unwrap()
        .bodies[0];
    let axis = Axis {
        origin: p3(0.0, 0.0, 0.0),
        dir: z,
    };
    let turned = k
        .sweep_solid(c, &SolidMotion::Turn { axis })
        .unwrap()
        .bodies[0];
    let v = k.mass_properties(turned).unwrap().volume;
    let want = 4.0 * PI * a * d * h;
    assert!((v - want).abs() < 1e-6 * want, "turned {v} against {want}");
    let rise = 60.0;
    let screwed = k
        .sweep_solid(
            c,
            &SolidMotion::Screw {
                axis,
                rise,
                from: 1.0,
                to: 6.0,
            },
        )
        .unwrap()
        .bodies[0];
    let v = k.mass_properties(screwed).unwrap().volume;
    let want = (PI * a * a + 2.0 * a * d * (2.0 * PI * h / rise)) * 5.0;
    assert!((v - want).abs() < 1e-5 * want, "screwed {v} against {want}");
}

/// A torus turned about an axis it is coaxial with fills itself.
#[test]
fn a_torus_turned_about_its_own_axis_is_itself() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let z = Unit::new_normalize(V3::z());
    let t = k
        .primitive(&Primitive::Torus {
            center: p3(0.0, 0.0, 1.0),
            axis: z,
            major: 6.0,
            minor: 2.0,
        })
        .unwrap()
        .bodies[0];
    let axis = Axis {
        origin: p3(0.0, 0.0, 0.0),
        dir: z,
    };
    let s = k
        .sweep_solid(t, &SolidMotion::Turn { axis })
        .unwrap()
        .bodies[0];
    measures(&k, s, 2.0 * PI * PI * 6.0 * 4.0, 4.0 * PI * PI * 6.0 * 2.0);
}

/// A planar solid sweep (`SolidMotion::Planar`): a box slid by (vx, vy) fills its square's
/// Minkowski sum with the slide (a² + a(vx + vy)) by h; turned a whole turn about a far line
/// along z, the ring it covers.
#[test]
fn a_prism_slid_and_turned_in_its_plane() {
    use std::f64::consts::{PI, TAU};
    let mut k = NativeKernel::new();
    let (a, h, l) = (2.0, 3.0, 5.0);
    let b = k
        .primitive(&Primitive::Box {
            min: p3(0.0, 0.0, 0.0),
            max: p3(a, a, h),
        })
        .unwrap()
        .bodies[0];
    let normal = Unit::new_normalize(V3::z());
    let slid = k
        .sweep_solid(
            b,
            &SolidMotion::Planar {
                normal,
                steps: vec![PlanarStep::Slide {
                    from: V3::zeros(),
                    to: V3::new(l, 0.5 * l, 0.0),
                }],
            },
        )
        .unwrap()
        .bodies[0];
    let v = k.mass_properties(slid).unwrap().volume;
    let want = (a * a + a * 1.5 * l) * h;
    assert!((v - want).abs() < 1e-6 * want, "slid {v} against {want}");
    // Turned a whole turn about the line through (−10, 1, 0): the ring between its nearest and
    // farthest corners' circles.
    let turned = k
        .sweep_solid(
            b,
            &SolidMotion::Planar {
                normal,
                steps: vec![PlanarStep::Turn {
                    about: p3(-10.0, 1.0, 0.0),
                    from: 0.0,
                    to: TAU,
                }],
            },
        )
        .unwrap()
        .bodies[0];
    let (r0, r1) = (10.0f64, (12.0f64.powi(2) + 1.0).sqrt());
    let v = k.mass_properties(turned).unwrap().volume;
    let want = PI * (r1 * r1 - r0 * r0) * h;
    assert!((v - want).abs() < 1e-6 * want, "turned {v} against {want}");
}

/// A rigid-motion solid sweep (`SolidMotion::Rigid`, meshed): a torus turned a whole turn about
/// its own axis fills itself, to the mesh's sag.
#[test]
fn a_torus_turned_by_a_rigid_motion() {
    use std::f64::consts::{PI, TAU};
    let mut k = NativeKernel::new();
    let z = Unit::new_normalize(V3::z());
    let t = k
        .primitive(&Primitive::Torus {
            center: p3(0.0, 0.0, 0.0),
            axis: z,
            major: 3.0,
            minor: 1.0,
        })
        .unwrap()
        .bodies[0];
    let axis = Axis {
        origin: p3(0.0, 0.0, 0.0),
        dir: z,
    };
    let s = k
        .sweep_solid(
            t,
            &SolidMotion::Rigid {
                steps: vec![RigidStep::Turn {
                    axis,
                    from: 0.0,
                    to: TAU,
                }],
            },
        )
        .unwrap()
        .bodies[0];
    let v = k.mass_properties(s).unwrap().volume;
    let want = 2.0 * PI * PI * 3.0;
    assert!((v - want).abs() < 1e-2 * want, "{v} against {want}");
    assert!(k.get(s).unwrap().check().is_empty());
}

/// Balls cut from a ball at once (they are apart: united side by side, one cut) leave what they
/// leave cut one after another: the ball less six lenses, each face from an input.
#[test]
fn balls_apart_cut_at_once() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let z = Unit::new_normalize(V3::z());
    let x = Unit::new_normalize(V3::x());
    let ball_at = |k: &mut NativeKernel, c: P3, r: f64| {
        k.primitive(&Primitive::Sphere {
            center: c,
            radius: r,
            axis: z,
            seam: x,
        })
        .unwrap()
        .bodies[0]
    };
    let (big, small, d) = (10.0, 1.2, 10.6);
    let golf = ball_at(&mut k, p3(0.0, 0.0, 0.0), big);
    let dirs = [V3::x(), -V3::x(), V3::y(), -V3::y(), V3::z(), -V3::z()];
    let tools: Vec<BodyId> = dirs
        .iter()
        .map(|v| ball_at(&mut k, P3::from(v * d), small))
        .collect();
    let at_once = k.boolean(BoolOp::Subtract, golf, &tools).unwrap();
    let mut one_by_one = golf;
    for &t in &tools {
        one_by_one = k
            .boolean(BoolOp::Subtract, one_by_one, &[t])
            .unwrap()
            .bodies[0];
    }
    // Two balls' lens: π (R + r − d)² (d² + 2dr − 3r² + 2dR + 6rR − 3R²) / 12d.
    let lens = PI
        * (big + small - d).powi(2)
        * (d * d + 2.0 * d * small - 3.0 * small * small + 2.0 * d * big + 6.0 * small * big
            - 3.0 * big * big)
        / (12.0 * d);
    let want = 4.0 / 3.0 * PI * big.powi(3) - 6.0 * lens;
    let body = at_once.bodies[0];
    let (v, v1) = (
        k.mass_properties(body).unwrap().volume,
        k.mass_properties(one_by_one).unwrap().volume,
    );
    assert!((v - want).abs() < 1e-9 * want, "{v} against {want}");
    assert!((v - v1).abs() < 1e-9 * want, "{v} at once, {v1} one by one");
    let b = k.get(body).unwrap();
    assert_eq!(b.faces.len(), k.get(one_by_one).unwrap().faces.len());
    assert!(b.check().is_empty());
    let mut inputs = vec![golf];
    inputs.extend(&tools);
    for f in 0..b.faces.len() {
        let from: Vec<&InputFace> = at_once
            .history
            .modified
            .iter()
            .filter(|(o, _)| o.0 == f as u64)
            .map(|(_, i)| i)
            .collect();
        assert!(
            !from.is_empty() && from.iter().all(|i| inputs.contains(&i.body)),
            "face {f} from {from:?}"
        );
    }
}

/// A block whose top is a rational B-spline barrel vault (exactly a cylinder of radius 15
/// about y = 0, z = 1), cut by a box whose floor touches the vault along its crown: the
/// B-spline, an extrusion along x, meets the box's floor in one ruling it only touches, so the
/// cut takes nothing.
#[test]
fn a_vault_cut_by_the_plane_along_its_crown() {
    use cadrs_kernel::native::nurbs::{H, NurbsSurface};
    use cadrs_kernel::native::surface::Surface;
    let mut k = NativeKernel::new();
    let b = block(&mut k, p3(-12.0, -12.0, 0.0), p3(12.0, 12.0, 10.0));
    let top = face_at(&k, b, p3(0.0, 0.0, 10.0));
    let row = |x: f64| {
        [(-12.0, 10.0, 1.0), (0.0, 26.0, 0.6), (12.0, 10.0, 1.0)]
            .map(|(y, z, w)| H::new(x * w, y * w, z * w, w))
            .to_vec()
    };
    let vault = NurbsSurface::new(
        1,
        2,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![row(-12.0), row(12.0)],
    )
    .unwrap();
    let domed = k
        .replace_faces(
            b,
            &[FaceReplacement {
                face: top,
                surface: Surface::Nurbs(std::sync::Arc::new(vault)),
                domain: Some(((0.0, 1.0), (0.0, 1.0))),
            }],
        )
        .unwrap()
        .bodies[0];
    let lid = block(&mut k, p3(-20.0, -20.0, 16.0), p3(20.0, 20.0, 30.0));
    let cut = k.boolean(BoolOp::Subtract, domed, &[lid]).unwrap().bodies[0];
    // The block to z = 10 and the arch's segment above it: the circular segment of radius 15
    // whose chord, 24 long, lies 9 below its centre.
    let (r, h): (f64, f64) = (15.0, 9.0);
    let half = (h / r).acos();
    let segment = r * r * half - h * (r * r - h * h).sqrt();
    let want = 24.0 * 24.0 * 10.0 + 24.0 * segment;
    let (v0, v1) = (
        k.mass_properties(domed).unwrap().volume,
        k.mass_properties(cut).unwrap().volume,
    );
    assert!((v0 - want).abs() < 1e-9 * want, "{v0} against {want}");
    assert!((v1 - want).abs() < 1e-9 * want, "{v1} after the cut");
    assert!(k.get(cut).unwrap().check().is_empty());
}

/// A torus less a ball as wide as its tube, centred on its core circle: the ball lies in the
/// tube, touching its wall along the meridian circle through its centre only, so the cut takes
/// the ball's volume and leaves a cavity.
#[test]
fn a_ball_in_a_torus_tube() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let z = Unit::new_normalize(V3::z());
    let x = Unit::new_normalize(V3::x());
    let (major, minor) = (12.0, 3.0);
    let ring = k
        .primitive(&Primitive::Torus {
            center: p3(0.0, 0.0, 0.0),
            axis: z,
            major,
            minor,
        })
        .unwrap()
        .bodies[0];
    let ball = k
        .primitive(&Primitive::Sphere {
            center: p3(major, 0.0, 0.0),
            radius: minor - 1e-3,
            axis: z,
            seam: x,
        })
        .unwrap()
        .bodies[0];
    let cut = k.boolean(BoolOp::Subtract, ring, &[ball]).unwrap().bodies[0];
    // A ball the tube's width only touches it: no crossing, the meridian circle one of touch.
    {
        use cadrs_kernel::native::math::{Bbox, Frame};
        use cadrs_kernel::native::surface::Surface;
        let ball = Surface::Sphere {
            f: Frame::from_z(p3(major, 0.0, 0.0), V3::z()),
            r: minor,
        };
        let tube = Surface::Torus {
            f: Frame::world(),
            rmaj: major,
            rmin: minor,
        };
        let mut bb = Bbox::empty();
        bb.add(&p3(-16.0, -16.0, -4.0));
        bb.add(&p3(16.0, 16.0, 4.0));
        let full = ((0.0, 2.0 * PI), (-PI / 2.0, PI / 2.0));
        let ssi = cadrs_kernel::native::ssi::surface_surface_full(
            &ball,
            full,
            &tube,
            ((0.0, 2.0 * PI), (0.0, 2.0 * PI)),
            &bb,
        );
        assert!(ssi.curves.is_empty(), "{} crossings", ssi.curves.len());
        assert_eq!(ssi.tangent.len(), 1);
    }
    let r = minor - 1e-3;
    let want = 2.0 * PI * PI * major * minor * minor - 4.0 / 3.0 * PI * r * r * r;
    let v = k.mass_properties(cut).unwrap().volume;
    assert!((v - want).abs() < 1e-9 * want, "{v} against {want}");
    assert!(k.get(cut).unwrap().check().is_empty());
}
