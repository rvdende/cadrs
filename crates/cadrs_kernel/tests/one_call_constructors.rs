//! Constructors and operations made in one kernel call: the screw of a profile, the slab under a
//! B-spline height field, the faces of a STEP file sewn into a solid, and chamfers at several
//! distances meeting at a corner. Each against its closed form.

mod springrep_port;

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

/// The plane of x (out from the z axis) and z (along it): a meridian, drawn as for a revolve.
fn meridian() -> Plane {
    Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    }
}

fn z_axis() -> Axis {
    Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    }
}

use nalgebra::{Point3, Vector3};

fn screw(k: &mut NativeKernel, outer: Loop, rise: f64, angle: f64) -> Result<BodyId> {
    let profile = Profile::of(meridian(), vec![outer]);
    k.screw(
        &profile,
        &ScrewSpec {
            axis: z_axis(),
            rise,
            angle,
        },
    )?
    .first()
}

/// A square thread section 5 ≤ ρ ≤ 6, 0 ≤ w ≤ 1, screwed two turns rising 0.5 a radian: its
/// volume is the section's first moment about the axis times the angle, M·angle = 5.5·4π, and its
/// area the caps' and the four sides': the flat ones (ρ = 5 and 6) are cylinders' strips, the
/// others helicoids, ∫√(ρ² + rise²) dρ a radian.
#[test]
fn a_square_thread_holds_its_closed_form() {
    let mut k = NativeKernel::new();
    let (rise, angle) = (0.5, 2.0 * TAU);
    let s = screw(&mut k, rect(5.5, 0.5, 1.0, 1.0), rise, angle).unwrap();
    valid(&k, s);
    let v = volume(&k, s);
    let want = 5.5 * angle;
    assert!((v - want).abs() <= 1e-7 * want, "{v} vs {want}");
    let helicoid = |r: f64| {
        let q = (r * r + rise * rise).sqrt();
        (r * q + rise * rise * (r + q).ln()) / 2.0
    };
    let want_a = angle * (5.0 + 6.0 + 2.0 * (helicoid(6.0) - helicoid(5.0))) + 2.0;
    let a = area(&k, s);
    assert!((a - want_a).abs() <= 1e-6 * want_a, "{a} vs {want_a}");
}

/// A round thread falling as it turns (a negative rise): its volume is still M·angle, a circle's
/// area times its centre's distance from the axis.
#[test]
fn a_falling_round_thread_holds_its_volume() {
    let mut k = NativeKernel::new();
    let s = screw(&mut k, circle(4.0, 0.0, 0.5), -0.3, 1.5 * TAU).unwrap();
    valid(&k, s);
    let want = PI * 0.25 * 4.0 * 1.5 * TAU;
    let v = volume(&k, s);
    assert!((v - want).abs() <= 1e-7 * want, "{v} vs {want}");
    let bb = k.bounding_box(s).unwrap();
    assert!((bb.min.z - (-0.5 - 0.3 * 1.5 * TAU)).abs() < 1e-6, "{bb:?}");
}

/// A screw whose turns would run into each other (a turn rising less than the section is tall),
/// one turning no way, and one whose section crosses its axis are refused.
#[test]
fn screws_that_cannot_be_are_refused() {
    let mut k = NativeKernel::new();
    let collide = screw(&mut k, rect(5.5, 1.0, 1.0, 2.0), 2.0 * 0.5 / TAU, 2.0 * TAU);
    assert!(
        matches!(collide, Err(KernelError::InvalidParameter(_))),
        "{collide:?}"
    );
    let still = screw(&mut k, rect(5.5, 0.5, 1.0, 1.0), 0.5, 0.0);
    assert!(
        matches!(still, Err(KernelError::InvalidParameter(_))),
        "{still:?}"
    );
    let across = screw(&mut k, rect(0.0, 0.5, 2.0, 1.0), 0.5, TAU);
    assert!(
        matches!(across, Err(KernelError::InvalidProfile(_))),
        "{across:?}"
    );
}

/// A round thread round a rod that bites into its section: the union, intersection and
/// difference are valid and their volumes add up (the conformance suite's helical sweeps).
#[test]
fn a_thread_and_a_rod_biting_into_it_unite_and_subtract() {
    let mut k = NativeKernel::new();
    let (cx, cy, r) = (6.531227707413701, 0.9926175765746613, 0.7424098516969266);
    let thread = screw(
        &mut k,
        circle(cx, cy, r),
        0.4000147107301036,
        9.47827177551747,
    )
    .unwrap();
    let rod = rod(
        &mut k,
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        6.4432188256335605,
        7.776683295654249,
    );
    let (va, vb) = (volume(&k, thread), volume(&k, rod));
    let i = intersection(&mut k, thread, rod);
    let u = boolean(&mut k, BoolOp::Union, thread, rod).unwrap_or_else(|e| panic!("{e}"));
    let d = boolean(&mut k, BoolOp::Subtract, thread, rod).unwrap_or_else(|e| panic!("{e}"));
    for x in [i, u, d] {
        valid(&k, x);
    }
    let vi = volume(&k, i);
    let bound = 1e-6 * (area(&k, thread) + area(&k, rod));
    assert!((volume(&k, u) - (va + vb - vi)).abs() <= bound);
    assert!((volume(&k, d) - (va - vi)).abs() <= bound);
}

fn slab_spec(size: [f64; 2], knots_u: Vec<f64>, knots_v: Vec<f64>, h: Vec<Vec<f64>>) -> SlabSpec {
    SlabSpec {
        origin: Point3::new(1.0, 2.0, 3.0),
        x: Vector3::y_axis(),
        y: -Vector3::x_axis(),
        size,
        knots_u,
        knots_v,
        heights: h,
    }
}

/// A biquadratic slab, flat at 5 but for a bump to 7 in the middle coefficient: its volume is the
/// integral of the height field, 100·(5 + 2·(1/3)²) (each quadratic Bernstein's mean is a third).
#[test]
fn a_slab_holds_the_integral_of_its_heights() {
    let mut k = NativeKernel::new();
    let q = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let spec = slab_spec(
        [10.0, 10.0],
        q.clone(),
        q,
        vec![vec![5.0; 3], vec![5.0, 7.0, 5.0], vec![5.0; 3]],
    );
    let s = k.slab(&spec).unwrap().first().unwrap();
    valid(&k, s);
    let want = 100.0 * (5.0 + 2.0 / 9.0);
    let v = volume(&k, s);
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    // Placed on its frame: x along y, y along −x, up along z.
    let bb = k.bounding_box(s).unwrap();
    assert!(
        (bb.min - Point3::new(-9.0, 2.0, 3.0)).norm() < 1e-9,
        "{bb:?}"
    );
    assert!((bb.max.y - 12.0).abs() < 1e-9 && bb.max.z > 8.0, "{bb:?}");
}

/// A bicubic slab with an inner knot each way: its volume is the coefficients weighted by their
/// B-splines' integrals, (k_{i+4} − k_i)/4 each way.
#[test]
fn a_bicubic_slab_with_inner_knots_holds_its_volume() {
    let mut k = NativeKernel::new();
    let ku = vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0];
    let kv = vec![0.0, 0.0, 0.0, 0.0, 0.7, 1.0, 1.0, 1.0, 1.0];
    let h: Vec<Vec<f64>> = (0..5)
        .map(|i| {
            (0..5)
                .map(|j| 2.0 + ((i * 7 + j * 3) % 5) as f64 * 0.4)
                .collect()
        })
        .collect();
    let mean = |kn: &[f64], i: usize| (kn[i + 4] - kn[i]) / 4.0;
    let want: f64 = (0..5)
        .flat_map(|i| (0..5).map(move |j| (i, j)))
        .map(|(i, j)| h[i][j] * mean(&ku, i) * mean(&kv, j))
        .sum::<f64>()
        * 6.0
        * 8.0;
    let s = k
        .slab(&slab_spec([6.0, 8.0], ku, kv, h))
        .unwrap()
        .first()
        .unwrap();
    valid(&k, s);
    let v = volume(&k, s);
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
}

#[test]
fn slabs_that_cannot_be_are_refused() {
    let mut k = NativeKernel::new();
    let q = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let low = slab_spec(
        [4.0, 4.0],
        q.clone(),
        q.clone(),
        vec![vec![1.0, 0.0, 1.0]; 3],
    );
    assert!(k.slab(&low).is_err());
    let unclamped = slab_spec(
        [4.0, 4.0],
        vec![0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        q,
        vec![vec![1.0; 3]; 3],
    );
    assert!(k.slab(&unclamped).is_err());
}

/// Two bicubic slabs crossed square, their common part cut by a slanted plane
/// (brep-conformance `slabs-8e6323004994e64d`): the section closes. A pair of a patch's face
/// and the plane's sheet looked to leave a curve's end loose and was traced again over wider
/// boxes, which found no curve at all; the first trace is kept where the second finds fewer.
#[test]
fn two_slabs_common_part_has_a_closed_section() {
    let mut k = NativeKernel::new();
    let cubic = |inner: &[f64]| {
        let mut v = vec![0.0; 4];
        v.extend_from_slice(inner);
        v.extend([1.0; 4]);
        v
    };
    let a = k
        .slab(&SlabSpec {
            origin: Point3::origin(),
            x: Vector3::x_axis(),
            y: Vector3::y_axis(),
            size: [4.652457352639631, 6.418761585288305],
            knots_u: cubic(&[0.5]),
            knots_v: cubic(&[0.25, 0.5, 0.75]),
            heights: vec![
                vec![
                    7.004578093367903,
                    6.904709222925296,
                    4.30819696886233,
                    7.6663160065357925,
                    5.008723928289747,
                    7.149428218101325,
                    7.905001828965269,
                ],
                vec![
                    7.873128900224459,
                    4.914211599587008,
                    6.646265100105766,
                    6.059267069843136,
                    3.7170574649858907,
                    4.096066826482392,
                    6.53327873003202,
                ],
                vec![
                    7.809843109919876,
                    4.530921038262217,
                    4.644750727348984,
                    4.808933089455844,
                    5.0335408732583495,
                    7.116112857333141,
                    5.127948157379857,
                ],
                vec![
                    5.832559904237259,
                    7.468372400286761,
                    6.594964381384109,
                    6.951358516989805,
                    5.100078442877011,
                    3.7682507733655557,
                    7.713084847551488,
                ],
                vec![
                    5.1293571374300395,
                    4.628558858998163,
                    7.1343913945028214,
                    6.746111868689292,
                    8.010299512096488,
                    5.482153986385592,
                    5.807827285929516,
                ],
            ],
        })
        .unwrap()
        .bodies[0];
    let b = k
        .slab(&SlabSpec {
            origin: Point3::new(4.273167858120837, -1.1325017095145813, -0.2211947378099397),
            x: Vector3::y_axis(),
            y: -Vector3::x_axis(),
            size: [7.067046426130112, 5.9990238729918275],
            knots_u: cubic(&[]),
            knots_v: cubic(&[0.25, 0.5, 0.75]),
            heights: vec![
                vec![
                    7.157894677002286,
                    7.830176785600108,
                    4.457826015539567,
                    4.817447695310303,
                    7.491275306486914,
                    6.07857584932224,
                    6.40772856931952,
                ],
                vec![
                    4.834787231299248,
                    7.208477014675593,
                    8.057791251875717,
                    5.580055523531385,
                    8.041471692334502,
                    4.786893789542063,
                    6.186948689110716,
                ],
                vec![
                    7.615508086662337,
                    4.896025520337414,
                    6.779312415841248,
                    7.912705676388149,
                    5.152184511737124,
                    7.869899629632066,
                    7.722873214333752,
                ],
                vec![
                    7.095690134565823,
                    5.895901388792211,
                    3.9195995999465003,
                    8.305282190490061,
                    6.053581015969796,
                    3.876923308672928,
                    4.269817732507928,
                ],
            ],
        })
        .unwrap()
        .bodies[0];
    let common = k.boolean(BoolOp::Intersect, a, &[b]).unwrap().bodies[0];
    let area = k
        .section_area(
            common,
            Point3::new(2.1365839290604187, 2.967272358307765, 4.005149756048244),
            Vector3::new(
                -0.11593503738995992,
                -0.7253496783540048,
                0.6785476484500779,
            ),
        )
        .unwrap();
    assert!((area - 26.941682).abs() < 1e-5, "{area}");
}

/// A box's faces written loose to STEP (each its own surface, edges and corners; one shifted
/// off by 1e-5, one inside out), sewn back into the box in one call.
#[test]
fn a_boxs_loose_faces_sew_into_the_box() {
    let mut k = NativeKernel::new();
    let (x, y, z) = (10.0, 20.0, 30.0);
    let p = |a: f64, b: f64, c: f64| Point3::new(a, b, c);
    // Each face by its corners, counter-clockwise seen from outside (the last inside out).
    let quads = [
        [
            p(0.0, 0.0, 0.0),
            p(0.0, y, 0.0),
            p(x, y, 0.0),
            p(x, 0.0, 0.0),
        ],
        [p(0.0, 0.0, z), p(x, 0.0, z), p(x, y, z), p(0.0, y, z)],
        [
            p(0.0, 0.0, 0.0),
            p(x, 0.0, 0.0),
            p(x, 0.0, z),
            p(0.0, 0.0, z),
        ],
        [p(0.0, y, 0.0), p(0.0, y, z), p(x, y, z), p(x, y, 0.0)],
        [
            p(0.0, 0.0, 0.0),
            p(0.0, 0.0, z),
            p(0.0, y, z),
            p(0.0, y, 0.0),
        ],
        [p(x, 0.0, 0.0), p(x, 0.0, z), p(x, y, z), p(x, y, 0.0)],
    ];
    let sheets: Vec<BodyId> = quads
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let shift = Vector3::new(0.0, 0.0, if i == 1 { 1e-5 } else { 0.0 });
            let surface = BsplineSurface {
                degree_u: 1,
                degree_v: 1,
                knots_u: vec![0.0, 0.0, 1.0, 1.0],
                knots_v: vec![0.0, 0.0, 1.0, 1.0],
                poles: vec![
                    vec![q[0] + shift, q[3] + shift],
                    vec![q[1] + shift, q[2] + shift],
                ],
                weights: Vec::new(),
            };
            k.bspline_face(&surface, None).unwrap().first().unwrap()
        })
        .collect();
    let text = k.export_step(&sheets).unwrap();
    let s = k.sew_step(&text, 1e-3).unwrap().first().unwrap();
    valid(&k, s);
    let v = volume(&k, s);
    assert!((v - 6000.0).abs() < 1e-5 * 200.0 + 1e-6, "{v}");
    assert!(k.sew_step(&text, 0.0).is_err());
}

/// Three edges of a box meeting at a corner chamfered at three distances in one call: away from
/// the corner each takes d²/2 from each unit of its length; at the corner the result lies
/// between the corner chamfered at the smallest distance and at the largest.
#[test]
fn three_chamfers_of_three_distances_meet_at_a_corner() {
    let mut k = NativeKernel::new();
    let w = [10.0, 12.0, 14.0];
    let b = block(&mut k, [0.0; 3], w);
    let d = [1.0, 1.5, 2.2];
    // The edges at the corner w, along x, y and z.
    let edges: Vec<EdgeId> = (0..3)
        .map(|axis| {
            let mut p = w;
            p[axis] = w[axis] / 2.0;
            k.edges(b)
                .unwrap()
                .into_iter()
                .find(|e| {
                    let m = nalgebra::center(&e.start, &e.end);
                    (m - Point3::from(p)).norm() < 1e-9
                })
                .unwrap()
                .id
        })
        .collect();
    let s = k
        .chamfer_distances(b, &edges, &d, false)
        .unwrap()
        .first()
        .unwrap();
    valid(&k, s);
    let full = w[0] * w[1] * w[2];
    let at =
        |r: f64| full - w.iter().map(|l| 0.5 * r * r * (l - r)).sum::<f64>() - 0.75 * r.powi(3);
    let v = volume(&k, s);
    assert!(
        v <= at(1.0) + 1e-9 && v >= at(2.2) - 1e-9,
        "{v}: {} – {}",
        at(2.2),
        at(1.0)
    );
    // Each edge's own distance far from the corner: its faces cut back that far.
    let on_face = |axis: usize, back: f64| {
        let mut p = w;
        p[axis] = 1.0;
        p[(axis + 1) % 3] -= back;
        k.classify(s, Point3::from(p), 1e-9).unwrap()
    };
    for (axis, &dist) in d.iter().enumerate() {
        assert_eq!(
            on_face(axis, dist - 1e-4),
            PointClass::Outside,
            "edge {axis}"
        );
        assert_eq!(
            on_face(axis, dist + 1e-4),
            PointClass::OnBoundary,
            "edge {axis}"
        );
    }
    // One distance too wide for its faces is refused, the others notwithstanding.
    let too = k.chamfer_distances(b, &edges, &[1.0, 11.0, 1.0], false);
    assert!(too.is_err());
}

/// A square thread rising less a turn than it is tall, cut by a cylinder through it
/// (brep-conformance `helices-905391a1d3e41797`), meshed to 0.01 with no angle bound: in
/// milliseconds, and the triangles bound the solid's volume. Its helicoids are fitted patches
/// straight along their rulings to rounding (S_vv·n 10⁻¹²), whose step that way, 10⁵ across a
/// face 1 wide, stood for a row's length in the twist's step: 10⁻⁶ along, a grid at its limit,
/// and minutes of refinement.
#[test]
fn a_thread_cut_by_a_cylinder_meshes_in_milliseconds() {
    let mut k = NativeKernel::new();
    let (r0, r1, h) = (7.08559283118267, 8.422332008494148, 1.4750553374306028);
    let s = screw(
        &mut k,
        rect((r0 + r1) / 2.0, h / 2.0, r1 - r0, h),
        0.6309272329891976,
        5.958131805117148,
    )
    .unwrap();
    let c = k
        .primitive(&Primitive::Cylinder {
            base: Point3::new(0.0, 0.0, -1.0),
            axis: Vector3::z_axis(),
            radius: 7.500693513908281,
            height: 7.234202951018098,
        })
        .unwrap()
        .bodies[0];
    let i = k.boolean(BoolOp::Intersect, s, &[c]).unwrap().bodies[0];
    let t = std::time::Instant::now();
    let m = k
        .tessellate(
            i,
            Tessellation {
                deflection: 0.01,
                angle: PI,
            },
        )
        .unwrap();
    let took = t.elapsed().as_secs_f64();
    assert!(took < 2.0, "{took} s");
    let p = |j: u32| m.positions[j as usize].coords;
    let meshed: f64 = m
        .indices
        .iter()
        .map(|t| p(t[0]).dot(&p(t[1]).cross(&p(t[2]))) / 6.0)
        .sum();
    let v = volume(&k, i);
    assert!((meshed - v).abs() < 0.01 * v, "{meshed} vs {v}");
}
