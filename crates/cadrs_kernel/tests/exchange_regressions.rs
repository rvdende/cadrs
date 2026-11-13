//! STEP and IGES reader fixes, each on a minimal fixture.

use cadrs_kernel::native::{NativeKernel, step};
use cadrs_kernel::*;
use nalgebra::Point2;

fn block(k: &mut NativeKernel) -> BodyId {
    let c = [
        Point2::new(0.0, 0.0),
        Point2::new(100.0, 0.0),
        Point2::new(100.0, 60.0),
        Point2::new(0.0, 60.0),
    ];
    let curves = (0..4)
        .map(|i| Curve2::Line {
            a: c[i],
            b: c[(i + 1) % 4],
            source: None,
        })
        .collect();
    let profile = Profile::new(
        Plane::top(),
        vec![Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        }],
    );
    k.extrude(&profile, Extent::Blind(40.0)).unwrap().bodies[0]
}

/// A STEP file whose vertex lies off its edges' ends (by 5e-3 on a 100 mm block, far past the
/// file's stated 1e-6): the vertex's tolerance grows to the gap (ShapeFix's same-parameter), so
/// the block reads clean.
#[test]
fn step_vertex_off_its_edges_reads_clean() {
    let mut k = NativeKernel::new();
    let b = block(&mut k);
    let body = k.get(b).unwrap().as_ref().clone();
    let text = String::from_utf8(step::write("block", &[(&body, "block")], &[])).unwrap();
    // The first vertex's point, moved 5e-3 along z.
    let vp = text
        .lines()
        .find(|l| l.contains("=VERTEX_POINT('',#"))
        .unwrap();
    let pid: String = vp
        .split("#")
        .nth(2)
        .unwrap()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let head = format!("#{pid}=CARTESIAN_POINT('',(");
    let line = text
        .lines()
        .find(|l| l.starts_with(&head))
        .unwrap()
        .to_string();
    let nums: Vec<f64> = line[head.len()..line.find("))").unwrap()]
        .split(',')
        .map(|x| x.trim().parse().unwrap())
        .collect();
    let moved = format!("{head}{},{},{}));", nums[0], nums[1], nums[2] + 5e-3);
    let text = text.replace(&line, &moved);
    let m = step::read(text.as_bytes()).unwrap();
    let errs = m.parts[0].body.check();
    assert!(errs.is_empty(), "{errs:?}");
}

/// A STEP sphere whose one face says it points in (`ADVANCED_FACE` same_sense .F., as
/// PRO7071.stp's spheres and cylinders): its loops alone (a seam) can't tell which side the
/// material is, so the solid would enclose a negative volume; it is turned round instead.
#[test]
fn step_solid_facing_in_is_turned_round() {
    let mut k = NativeKernel::new();
    let half = Loop {
        curves: vec![
            Curve2::Arc {
                center: Point2::origin(),
                radius: 10.0,
                start_angle: -std::f64::consts::FRAC_PI_2,
                sweep: std::f64::consts::PI,
                source: None,
            },
            Curve2::Line {
                a: Point2::new(0.0, 10.0),
                b: Point2::new(0.0, -10.0),
                source: None,
            },
        ],
    };
    let front = Plane {
        origin: nalgebra::Point3::origin(),
        x_dir: nalgebra::Vector3::x_axis(),
        normal: -nalgebra::Vector3::y_axis(),
    };
    let axis = Axis {
        origin: nalgebra::Point3::origin(),
        dir: nalgebra::Vector3::z_axis(),
    };
    let s = k
        .revolve_with(
            &Profile::new(
                front,
                vec![Region {
                    outer: half,
                    holes: vec![],
                    source: None,
                }],
            ),
            &RevolveSpec::solid(axis, None),
        )
        .unwrap()
        .bodies[0];
    let body = k.get(s).unwrap().as_ref().clone();
    let text = String::from_utf8(step::write("ball", &[(&body, "ball")], &[])).unwrap();
    let flipped = text
        .lines()
        .map(|l| {
            if l.contains("=ADVANCED_FACE(") {
                l.replace(",.T.);", ",.F.);")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(flipped, text);
    let m = step::read(flipped.as_bytes()).unwrap();
    let v = cadrs_kernel::native::mass::body_mass(&m.parts[0].body).volume;
    let want = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
    assert!((v - want).abs() < 1e-6 * want, "{v}");
}

/// A block with two crossing holes, the splines the holes meet along each made to run on past
/// its edge's end (as a kernel leaves an edge on part of a longer curve): the STEP file holds
/// each edge's part only, its ends the edge's vertices, and reads back as the same solid.
#[test]
fn step_edge_along_part_of_a_spline_is_written_as_that_part() {
    use cadrs_kernel::native::curve::Curve;
    let mut k = NativeKernel::new();
    let b = block(&mut k);
    let hole = |k: &mut NativeKernel, plane: Plane, at: [f64; 2], r: f64, len: f64| {
        let profile = Profile::new(
            plane,
            vec![Region {
                outer: Loop {
                    curves: vec![Curve2::Circle {
                        center: Point2::new(at[0], at[1]),
                        radius: r,
                        source: None,
                    }],
                },
                holes: vec![],
                source: None,
            }],
        );
        k.extrude(&profile, Extent::Blind(len)).unwrap().bodies[0]
    };
    let down = hole(&mut k, Plane::top(), [50.0, 30.0], 12.0, 40.0);
    let side = Plane {
        origin: nalgebra::Point3::origin(),
        x_dir: nalgebra::Vector3::y_axis(),
        normal: nalgebra::Vector3::x_axis(),
    };
    let along = hole(&mut k, side, [30.0, 22.0], 9.0, 100.0);
    let holed = k
        .boolean(BoolOp::Subtract, b, &[down, along])
        .unwrap()
        .bodies[0];
    let mut body = k.get(holed).unwrap().as_ref().clone();
    // How many edges run along only a part of a spline.
    let part = |body: &cadrs_kernel::native::topo::Body| {
        body.edges
            .iter()
            .filter(|e| match &e.curve {
                Curve::Nurbs(n) => {
                    let (a, z) = n.range();
                    !e.degenerate && (e.t[0] - a > 1e-6 * (z - a) || z - e.t[1] > 1e-6 * (z - a))
                }
                _ => false,
            })
            .count()
    };
    for e in &mut body.edges {
        if let Curve::Nurbs(n) = &e.curve
            && !e.degenerate
        {
            e.curve = Curve::Nurbs(std::sync::Arc::new(n.joined(&n.reversed())));
        }
    }
    assert!(
        part(&body) >= 4,
        "the holes meet along {} splines",
        part(&body)
    );
    let text = step::write("holed", &[(&body, "holed")], &[]);
    let m = step::read(&text).unwrap();
    let back = &m.parts[0].body;
    assert_eq!(part(back), 0);
    let errs = back.check();
    assert!(errs.is_empty(), "{errs:?}");
    let want = k.mass_properties(holed).unwrap();
    let got = {
        let id = k.insert(back.clone());
        k.mass_properties(id).unwrap()
    };
    assert!(
        (got.volume - want.volume).abs() < 1e-9 * want.volume,
        "{} of {}",
        got.volume,
        want.volume
    );
    assert!((got.surface_area - want.surface_area).abs() < 1e-9 * want.surface_area);
}
