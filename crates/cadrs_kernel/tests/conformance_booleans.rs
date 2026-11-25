//! Booleans the brep-conformance suite found wrong, each reduced to what went wrong.

use cadrs_kernel::native::curve::Curve;
use cadrs_kernel::native::math::*;
use cadrs_kernel::native::nurbs::{NurbsCurve, hom};
use cadrs_kernel::native::ssi;
use cadrs_kernel::native::surface::Surface;

/// A closed uniform cubic B-spline through the given poles (the first three repeated at the end),
/// at height `z`, clamped as a sketch's spline becomes.
fn closed_spline(poles: &[[f64; 2]], z: f64) -> NurbsCurve {
    let mut ps: Vec<[f64; 2]> = poles.to_vec();
    ps.extend_from_slice(&poles[..3]);
    let knots: Vec<f64> = (0..ps.len() + 4).map(|i| i as f64).collect();
    let cps = ps
        .iter()
        .map(|p| hom(&P3::new(p[0], p[1], z), 1.0))
        .collect();
    NurbsCurve::new(3, knots, cps).unwrap().clamped()
}

const A: [[f64; 2]; 12] = [
    [2.8018606634132044, 0.0],
    [2.5679439377342153, 1.4826031237147164],
    [1.3797557534071818, 2.3898070669367133],
    [1.7075139842222685e-16, 2.7885819575262127],
    [-1.4780206476942437, 2.5600068564422913],
    [-1.682133032855744, 0.9711799593320253],
    [-2.7901858576617413, 3.416992179611664e-16],
    [-2.179280367485408, -1.258208106807366],
    [-1.1510099528952042, -1.9936077184319516],
    [-4.884311554764415e-16, -2.658895369214516],
    [1.2701435258423606, -2.1999531196636415],
    [2.157623321111984, -1.24570440525382],
];

const B: [[f64; 2]; 12] = [
    [2.787863030624791, 0.2797193229968888],
    [2.407101578874299, 1.7315629006118713],
    [1.1342801170874393, 2.515613716813649],
    [-0.2783936644195419, 2.7746506629577325],
    [-1.726210931940638, 2.399661634166511],
    [-1.7706855877622671, 0.7983950168454287],
    [-2.7762465502732736, -0.2785537872500318],
    [-2.0427818288012913, -1.469487311977332],
    [-0.946231027422643, -2.0985572399622465],
    [0.26544660921511426, -2.645611967406898],
    [1.4834269351129363, -2.062159749665019],
    [2.271207118515926, -1.0240781639492509],
];

/// A spline crossing the side of a prism of another spline crosses it at as many points as the
/// two profiles cross in the plane.
#[test]
fn spline_edge_crosses_spline_prism_side() {
    let b = B;
    let pa = closed_spline(&A, 0.0);
    let eb = closed_spline(&b, 1.75);
    let surf = Surface::Extrusion {
        c: Curve::Nurbs(pa.clone().into()),
        d: V3::new(0.0, 0.0, 1.0),
    };
    let (lo, hi) = eb.range();
    let ts = ssi::curve_surface(&Curve::Nurbs(eb.clone().into()), lo, hi, &surf);
    eprintln!("{ts:?}");
    for t in &ts {
        assert!(surf.distance(&eb.point(*t)) < 1e-7);
    }
    assert_eq!(ts.len(), 8, "{ts:?}");
}

/// The nearest point of a clamped closed spline to a point on it just before its end.
#[test]
fn closest_near_the_end_of_a_closed_spline() {
    let e: [[f64; 2]; 14] = [
        [2.345981289851668, 1.526211944530817],
        [2.052024345262844, 2.0285106797790062],
        [1.3763528769849425, 3.0137924114891335],
        [-0.4545393949609349, 3.161390932742787],
        [-2.338932594938957, 2.69927210693947],
        [-3.0122917155283857, 0.884488652137896],
        [-3.6998837590899942, -1.0863838924612148],
        [-2.7780441672349623, -3.2060338757470377],
        [-0.6175023034761249, -4.294822857598472],
        [1.581694135861133, -3.463427049607789],
        [2.662651351150616, -1.7111821191890315],
        [3.1400945445178894, 0.0],
        [2.6399382344404927, 1.023913209282628],
        [2.345981289851668, 1.526211944530817],
    ];
    let knots = vec![
        3.0, 3.0, 3.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 14.0, 14.0,
        14.0,
    ];
    let z = 3.438136172363456;
    let cps = e
        .iter()
        .map(|p| hom(&P3::new(p[0], p[1], z), 1.0))
        .collect();
    let c = Curve::Nurbs(NurbsCurve::new(3, knots, cps).unwrap().into());
    let p = P3::new(2.4306797909570745, 1.3787891119194988, z);
    let t = c.closest(&p, 3.0, 14.0);
    eprintln!("t {t} off {:e}", (c.point(t) - p).norm());
    for k in 0..=20 {
        let s = 13.0 + k as f64 * 0.05;
        eprintln!("  {s} {:e}", (c.point(s) - p).norm());
    }
    assert!((c.point(t) - p).norm() < 1e-6);
}

/// A closed curve on the side of a prism on a closed spline outline, round it once from its
/// seam: its p-curve runs the outline's whole range, seam to seam, not back to its start (the
/// extrusion is closed in u without being periodic, like a closed B-spline surface).
#[test]
fn pcurve_round_a_closed_spline_prism() {
    let pa = closed_spline(&A, 0.0);
    let surf = Surface::Extrusion {
        c: Curve::Nurbs(pa.clone().into()),
        d: V3::new(0.0, 0.0, 1.0),
    };
    assert_eq!(
        cadrs_kernel::native::pcurve::closed_periods(&surf),
        [Some(12.0), None]
    );
    // The outline with its poles lifted unevenly: on the side, its height varying.
    let mut lifted = pa.clone();
    let n = lifted.cps.len();
    for (i, h) in lifted.cps.iter_mut().enumerate() {
        h.z = h.w * (1.0 + 0.5 * ((i * 7 % n) as f64 / n as f64));
    }
    // (Its ends one point.)
    let z0 = lifted.cps[0].z;
    lifted.cps[n - 1].z = z0;
    let c = Curve::Nurbs(lifted.into());
    let (lo, hi) = pa.range();
    for near in [None, Some(P2::new(lo, 1.0)), Some(P2::new(hi, 1.0))] {
        let pc = cadrs_kernel::native::pcurve::pcurve(&surf, &c, lo, hi, near);
        let (s, e) = (pc.point(lo), pc.point(hi));
        assert!(
            (s.x - lo).abs() < 1e-7 && (e.x - hi).abs() < 1e-7,
            "{s:?} {e:?}"
        );
        for k in 0..=40 {
            let t = lo + (hi - lo) * k as f64 / 40.0;
            let q = pc.point(t);
            assert!((surf.eval(q.x, q.y) - c.point(t)).norm() < 1e-8);
        }
    }
}

/// A ball shaved by a box whose face lies a hair below its top: the cap, 7.8 µm high and 22 µm
/// across, is a solid of its own volume, not dropped as empty by the ball's size.
#[test]
fn cap_shaved_off_a_ball() {
    use cadrs_kernel::{BoolOp, DefaultKernel, Kernel, Primitive};
    use nalgebra::{Point3, Unit, Vector3};
    let r = 7.758466922627455;
    let z0 = 7.7584591641605325;
    let mut k = DefaultKernel::new();
    let ball = k
        .primitive(&Primitive::Sphere {
            center: Point3::origin(),
            radius: r,
            axis: Unit::new_normalize(Vector3::z()),
            seam: Unit::new_normalize(Vector3::x()),
        })
        .unwrap()
        .bodies[0];
    let block = k
        .primitive(&Primitive::Box {
            min: Point3::new(-23.0, -23.0, z0),
            max: Point3::new(23.0, 23.0, z0 + 15.0),
        })
        .unwrap()
        .bodies[0];
    let cap = k.boolean(BoolOp::Intersect, ball, &[block]).unwrap().bodies[0];
    let h = r - z0;
    let exact = std::f64::consts::PI * h * h * (3.0 * r - h) / 3.0;
    let v = k.mass_properties(cap).unwrap().volume;
    assert!((v - exact).abs() < 1e-6 * exact, "{v:e} against {exact:e}");
}

#[test]
fn cylinder_crossing_through_a_node_is_fitted_between_corners() {
    // The suite's calibration: a cylinder of radius 1.5 across one of radius 2 whose side it
    // just reaches, so their curve crosses itself where they touch. Fitted through that node as
    // one loop it took 15 000 points (a cubic rings round a corner); each branch closing at the
    // node takes a few hundred. The volume is ∫ 2√(4 − y²) · 2√(2.25 − (y − 0.5)²) dy.
    use cadrs_kernel::*;
    use nalgebra::{Point3, Unit, Vector3};
    let mut k = DefaultKernel::default();
    let cyl = |k: &mut DefaultKernel, base: Point3<f64>, axis: Vector3<f64>, radius: f64| {
        k.primitive(&Primitive::Cylinder {
            base,
            axis: Unit::new_normalize(axis),
            radius,
            height: 10.0,
        })
        .unwrap()
        .first()
        .unwrap()
    };
    let a = cyl(&mut k, Point3::new(0.0, 0.0, -5.0), Vector3::z(), 2.0);
    let b = cyl(&mut k, Point3::new(-5.0, 0.5, 0.0), Vector3::x(), 1.5);
    let i = k
        .boolean(BoolOp::Intersect, a, &[b])
        .unwrap()
        .first()
        .unwrap();
    let v = k.mass_properties(i).unwrap().volume;
    let want = 24.835511089434053;
    assert!((v - want).abs() <= 1e-9 * want, "volume {v} vs {want}");
    let body = k.get(i).unwrap();
    assert!(body.check().is_empty());
    for e in &body.edges {
        if let Curve::Nurbs(n) = &e.curve {
            assert!(
                n.cps.len() < 2000,
                "an edge fitted with {} poles",
                n.cps.len()
            );
        }
    }
}
