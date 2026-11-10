use cadrs_kernel::native::*;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3};

fn p(x: f64, y: f64) -> Point2<f64> {
    Point2::new(x, y)
}
fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
    let l = |a, b| Curve2::Line { a, b, source: None };
    Loop {
        curves: vec![
            l(p(x0, y0), p(x1, y0)),
            l(p(x1, y0), p(x1, y1)),
            l(p(x1, y1), p(x0, y1)),
            l(p(x0, y1), p(x0, y0)),
        ],
    }
}
fn reg(l: Loop) -> Region {
    Region {
        outer: l,
        holes: vec![],
        source: None,
    }
}

#[test]
fn debug_box_minus_cylinder() {
    let mut k = NativeKernel::new();
    let a = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 10., 20.))]),
            Extent::Blind(30.0),
        )
        .unwrap()
        .bodies[0];
    let c = Loop {
        curves: vec![Curve2::Circle {
            center: p(5., 5.),
            radius: 2.0,
            source: None,
        }],
    };
    let t = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(c)]),
            Extent::Blind(30.0),
        )
        .unwrap()
        .bodies[0];
    let ba = (**k.get(a).unwrap()).clone();
    let bt = (**k.get(t).unwrap()).clone();
    match boolean::boolean_bodies(&ba, &bt, BoolOp::Subtract) {
        Ok(r) => println!("ok faces {}", r.body.faces.len()),
        Err(e) => println!("err {e}"),
    }
}

#[test]
fn debug_mirror_union() {
    let mut k = NativeKernel::new();
    let half = Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc {
                    center: p(0., 0.),
                    radius: 10.0,
                    start_angle: 0.0,
                    sweep: std::f64::consts::PI,
                    source: None,
                },
                Curve2::Line {
                    a: p(-10., 0.),
                    b: p(10., 0.),
                    source: None,
                },
            ],
        },
        holes: vec![],
        source: None,
    };
    let hb = k
        .extrude(&Profile::new(Plane::top(), vec![half]), Extent::Blind(5.0))
        .unwrap()
        .bodies[0];
    let hm = k
        .transform_motion(hb, &Motion::reflection(Point3::origin(), Vector3::y_axis()))
        .unwrap()
        .bodies[0];
    println!(
        "mirror check {:?} vol {}",
        k.get(hm).unwrap().check(),
        k.mass_properties(hm).unwrap().volume
    );
    let (a, b) = (
        (**k.get(hb).unwrap()).clone(),
        (**k.get(hm).unwrap()).clone(),
    );
    match boolean::boolean_bodies(&a, &b, BoolOp::Union) {
        Ok(r) => println!(
            "ok faces {} solids {} shells {:?} check {:?}",
            r.body.faces.len(),
            r.body.solids.len(),
            r.body.shells,
            r.body.check()
        ),
        Err(e) => println!("err {e}"),
    }
}

#[test]
fn debug_revolve_upto() {
    let mut k = NativeKernel::new();
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let prof = Profile::new(front, vec![reg(rect(5., 0., 10., 20.))]);
    let plate = k
        .extrude(
            &Profile::new(
                Plane {
                    origin: Point3::new(0., 0., -5.),
                    ..Plane::top()
                },
                vec![reg(rect(-30., 5., 0., 40.))],
            ),
            Extent::Blind(35.0),
        )
        .unwrap()
        .bodies[0];
    let axis = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    let spec = RevolveSpec {
        full: false,
        end: RevolveEnd::UpToPart {
            body: plate,
            offset: 0.0,
        },
        ..RevolveSpec::solid(axis, None)
    };
    // The almost-full sweep minus the plate.
    let full = k
        .revolve_with(
            &prof,
            &RevolveSpec {
                full: false,
                end: RevolveEnd::Angle(std::f64::consts::TAU * (1.0 - 1e-4)),
                ..RevolveSpec::solid(axis, None)
            },
        )
        .unwrap()
        .bodies[0];
    let (a, b) = (
        (**k.get(full).unwrap()).clone(),
        (**k.get(plate).unwrap()).clone(),
    );
    for op in [BoolOp::Subtract, BoolOp::Intersect] {
        match boolean::boolean_bodies(&a, &b, op) {
            Ok(r) => println!(
                "{op:?} ok faces {} solids {}",
                r.body.faces.len(),
                r.body.solids.len()
            ),
            Err(e) => println!("{op:?} err {e}"),
        }
    }
    println!("{:?}", k.revolve_with(&prof, &spec).map(|r| r.bodies.len()));
}

#[test]
fn debug_thin() {
    let prof = Profile::new(Plane::top(), vec![reg(rect(0., 0., 20., 20.))]);
    let bands = cadrs_kernel::thin::thin_profile(&prof, 2.0, 0.0).unwrap();
    for r in &bands.regions {
        let pts: Vec<_> = r
            .outer
            .curves
            .iter()
            .map(|c| {
                let s = c.start();
                (s.x, s.y)
            })
            .collect();
        println!("region {:?} holes {} : {:?}", r.source, r.holes.len(), pts);
    }
    let regions = native::profile::regions(&bands).unwrap();
    let m = native::profile::merge_regions(&regions).unwrap();
    println!(
        "merged {} faces: {:?}",
        m.len(),
        m.iter()
            .map(|f| (
                f.outer.len(),
                f.holes.len(),
                native::profile::loop_area(&f.outer)
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn debug_thin2() {
    let mut k = NativeKernel::new();
    let a = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 20., 2.))]),
            Extent::Blind(10.0),
        )
        .unwrap()
        .bodies[0];
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(18., 0., 20., 20.))]),
            Extent::Blind(10.0),
        )
        .unwrap()
        .bodies[0];
    let u = k.boolean(BoolOp::Union, a, &[b]).unwrap().bodies[0];
    println!(
        "union vol {} faces {} solids {}",
        k.mass_properties(u).unwrap().volume,
        k.faces(u).unwrap().len(),
        k.solid_count(u).unwrap()
    );
}

#[test]
fn debug_thin3() {
    let mut k = NativeKernel::new();
    let mut spec = ExtrudeSpec::blind(Vector3::z_axis(), 10.0);
    spec.body = BodyKind::Thin {
        left: 2.0,
        right: 0.0,
    };
    let sq = Profile::new(Plane::top(), vec![reg(rect(0., 0., 20., 20.))]);
    let r = k.extrude_with(&sq, &spec);
    match r {
        Ok(r) => {
            let b = r.bodies[0];
            println!(
                "thin vol {} solids {}",
                k.mass_properties(b).unwrap().volume,
                k.solid_count(b).unwrap()
            );
        }
        Err(e) => println!("thin err {e}"),
    }
}

#[test]
fn debug_split() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 10., 20.))]),
            Extent::Blind(30.0),
        )
        .unwrap()
        .bodies[0];
    let plane = Plane {
        origin: Point3::new(0.0, 0.0, 10.0),
        ..Plane::top()
    };
    let r = k.split(b, &SplitTool::Plane(plane), 42).unwrap();
    let s = r.bodies[0];
    let body = k.get(s).unwrap();
    println!(
        "solids {} shells {:?} faces {}",
        body.solids.len(),
        body.shells.len(),
        body.faces.len()
    );
}

#[test]
fn debug_cube_fillet() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 100., 100.))]),
            Extent::Blind(100.0),
        )
        .unwrap()
        .bodies[0];
    let all: Vec<EdgeId> = k.edges(b).unwrap().iter().map(|e| e.id).collect();
    for n in 4..=12usize {
        let r = k.fillet_with(b, &all[..n], &FilletSpec::radius(10.0));
        match r {
            Ok(r) => println!(
                "{n} edges: vol {} faces {}",
                k.mass_properties(r.bodies[0]).unwrap().volume,
                k.faces(r.bodies[0]).unwrap().len()
            ),
            Err(e) => println!("{n} edges: err {e}"),
        }
    }
    let infos = k.edges(b).unwrap();
    for e in &infos[..3] {
        println!("edge {:?} {:?} -> {:?}", e.id, e.start, e.end);
    }
}

#[test]
fn debug_miter() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 100., 100.))]),
            Extent::Blind(100.0),
        )
        .unwrap()
        .bodies[0];
    let infos = k.edges(b).unwrap();
    let v = infos
        .iter()
        .find(|e| e.start.coords.norm() < 1e-9 && e.end.z > 50.0)
        .unwrap()
        .id;
    let h = infos
        .iter()
        .find(|e| {
            (e.start.coords.norm() < 1e-9 || e.end.coords.norm() < 1e-9)
                && (e.start.x - e.end.x).abs() > 50.0
        })
        .unwrap()
        .id;
    println!("v {v:?} h {h:?}");
    let r = k.fillet_with(b, &[v, h], &FilletSpec::radius(10.0));
    match r {
        Ok(r) => println!(
            "miter: vol {} faces {}",
            k.mass_properties(r.bodies[0]).unwrap().volume,
            k.faces(r.bodies[0]).unwrap().len()
        ),
        Err(e) => println!("miter: err {e}"),
    }
}

#[test]
fn debug_corner() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 30., 30.))]),
            Extent::Blind(30.0),
        )
        .unwrap()
        .bodies[0];
    let v = k
        .vertices(b)
        .unwrap()
        .into_iter()
        .find(|x| (x.point - Point3::new(30., 30., 30.)).norm() < 1e-9)
        .unwrap();
    let r = k.fillet_with(b, &v.edges, &FilletSpec::radius(3.0));
    match r {
        Ok(r) => println!(
            "corner: vol {} faces {}",
            k.mass_properties(r.bodies[0]).unwrap().volume,
            k.faces(r.bodies[0]).unwrap().len()
        ),
        Err(e) => println!("corner: err {e}"),
    }
}

#[test]
fn debug_block_ball() {
    let c = Point3::new(27., 27., 27.);
    let (a, b) = (27.0, 31.5);
    let block = native::blend::hexahedron(&[
        Point3::new(a, a, a),
        Point3::new(b, a, a),
        Point3::new(b, b, a),
        Point3::new(a, b, a),
        Point3::new(a, a, b),
        Point3::new(b, a, b),
        Point3::new(b, b, b),
        Point3::new(a, b, b),
    ])
    .unwrap();
    let ball = native::blend::ball_body(c, 3.0).unwrap();
    println!(
        "ball verts {:?}",
        ball.verts.iter().map(|v| v.p).collect::<Vec<_>>()
    );
    match native::boolean::boolean_bodies(&block, &ball, BoolOp::Subtract) {
        Ok(r) => {
            println!(
                "ok faces {} solids {}",
                r.body.faces.len(),
                r.body.solids.len()
            );
            for (i, v) in r.body.verts.iter().enumerate() {
                println!("v{i} {:?}", v.p);
            }
        }
        Err(e) => println!("err {e}"),
    }
}

#[test]
fn debug_cube7() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 100., 100.))]),
            Extent::Blind(100.0),
        )
        .unwrap()
        .bodies[0];
    let all: Vec<EdgeId> = k.edges(b).unwrap().iter().map(|e| e.id).collect();
    for e in &k.edges(b).unwrap()[..7] {
        eprintln!("edge {:?} {:?} -> {:?}", e.id, e.start, e.end);
    }
    // Apply one at a time through the blend to see which step opens.
    for n in [5usize, 6, 7] {
        let r = k.fillet_with(b, &all[4..n], &FilletSpec::radius(10.0));
        match r {
            Ok(r) => eprintln!(
                "bottom edges 4..{n}: vol {} solids {}",
                k.mass_properties(r.bodies[0]).unwrap().volume,
                k.solid_count(r.bodies[0]).unwrap()
            ),
            Err(e) => eprintln!("bottom edges 4..{n}: err {e}"),
        }
    }
    let mut set = vec![all[1], all[4], all[5]];
    let r = k.fillet_with(b, &set, &FilletSpec::radius(10.0)).unwrap();
    eprintln!(
        "1,4,5: vol {} solids {}",
        k.mass_properties(r.bodies[0]).unwrap().volume,
        k.solid_count(r.bodies[0]).unwrap()
    );
    set.push(all[2]);
    let r = k.fillet_with(b, &set, &FilletSpec::radius(10.0)).unwrap();
    eprintln!(
        "1,2,4,5: vol {} solids {}",
        k.mass_properties(r.bodies[0]).unwrap().volume,
        k.solid_count(r.bodies[0]).unwrap()
    );
}

#[test]
fn debug_cube_combos() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 100., 100.))]),
            Extent::Blind(100.0),
        )
        .unwrap()
        .bodies[0];
    let all: Vec<EdgeId> = k.edges(b).unwrap().iter().map(|e| e.id).collect();
    for extra in [None, Some(0usize), Some(2), Some(3), Some(5)] {
        let mut set = vec![all[1], all[4], all[6]];
        if let Some(x) = extra {
            set.push(all[x]);
        }
        match k.fillet_with(b, &set, &FilletSpec::radius(10.0)) {
            Ok(r) => eprintln!(
                "1,4,6 + {extra:?}: vol {} solids {}",
                k.mass_properties(r.bodies[0]).unwrap().volume,
                k.solid_count(r.bodies[0]).unwrap()
            ),
            Err(e) => eprintln!("1,4,6 + {extra:?}: err {e}"),
        }
    }
}

#[test]
fn debug_corners_all() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 30., 30.))]),
            Extent::Blind(30.0),
        )
        .unwrap()
        .bodies[0];
    for v in k.vertices(b).unwrap() {
        let r = k.fillet_with(b, &v.edges, &FilletSpec::radius(3.0));
        match r {
            Ok(r) => eprintln!(
                "corner {:?}: vol {:.4} solids {}",
                v.point,
                k.mass_properties(r.bodies[0]).unwrap().volume,
                k.solid_count(r.bodies[0]).unwrap()
            ),
            Err(e) => eprintln!("corner {:?}: err {e}", v.point),
        }
    }
}

#[test]
fn debug_corner_tools() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 30., 30.))]),
            Extent::Blind(30.0),
        )
        .unwrap()
        .bodies[0];
    let body = (**k.get(b).unwrap()).clone();
    for (vi, v) in body.verts.iter().enumerate() {
        if std::env::var("ONLY0").is_ok() && vi != 0 {
            continue;
        }
        match native::blend::corner_tool(&body, vi, 3.0, Origin::StartCap { region: 0 }) {
            Ok(Some((t, _))) => {
                let m = native::mass::body_mass(&t);
                // block 4.5³ minus a ball octant
                eprintln!(
                    "tool at {:?}: vol {:.9} (want {:.9}) faces {} solids {}",
                    v.p,
                    m.volume,
                    4.501f64.powi(3) - std::f64::consts::PI * 27.0 / 6.0,
                    t.faces.len(),
                    t.solids.len()
                );
                for (i, f) in t.faces.iter().enumerate() {
                    if matches!(f.surf, native::surface::Surface::Sphere { .. }) {
                        let q = native::mass::face_integrals(&t, f, &Vector3::zeros());
                        eprintln!(
                            "   sphere face {i} area {:.12} want {:.12}",
                            q[0],
                            std::f64::consts::PI * 9.0 / 2.0
                        );
                        for l in &f.loops {
                            for c in l {
                                let e = &t.edges[c.edge];
                                eprintln!(
                                    "      edge {:?} deg {} t {:?} pc {:?}",
                                    std::mem::discriminant(&e.curve),
                                    e.degenerate,
                                    e.t,
                                    c.pc
                                );
                            }
                        }
                    }
                }
            }
            Ok(None) => eprintln!("tool at {:?}: none", v.p),
            Err(e) => eprintln!("tool at {:?}: err {e}", v.p),
        }
    }
}

#[test]
fn debug_cube_precision() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let (l, r) = (30.0f64, 3.0f64);
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., l, l))]),
            Extent::Blind(l),
        )
        .unwrap()
        .bodies[0];
    let edge = (1.0 - PI / 4.0) * r * r * l;
    let corner = 3.0 * (1.0 - PI / 4.0) * r.powi(3) - (1.0 - PI / 6.0) * r.powi(3);
    let v = k
        .vertices(b)
        .unwrap()
        .into_iter()
        .find(|x| (x.point - Point3::new(l, l, l)).norm() < 1e-9)
        .unwrap();
    let one = k
        .fillet_with(b, &v.edges, &FilletSpec::radius(r))
        .unwrap()
        .bodies[0];
    eprintln!(
        "corner: {:.9} want {:.9}",
        k.mass_properties(one).unwrap().volume,
        l.powi(3) - 3.0 * edge + corner
    );
    let all: Vec<EdgeId> = k.edges(b).unwrap().iter().map(|e| e.id).collect();
    let f = k
        .fillet_with(b, &all, &FilletSpec::radius(r))
        .unwrap()
        .bodies[0];
    let m = k.mass_properties(f).unwrap();
    eprintln!(
        "all: {:.9} want {:.9}, faces {}",
        m.volume,
        l.powi(3) - 12.0 * edge + 8.0 * corner,
        k.faces(f).unwrap().len()
    );
    // Area: 6 faces (L−2r)², 12 quarter cylinders (π r/2)(L−2r), 8 octants 4πr²/8.
    eprintln!(
        "area: {:.9} want {:.9}",
        m.surface_area,
        6.0 * (l - 2.0 * r).powi(2) + 12.0 * PI * r / 2.0 * (l - 2.0 * r) + 4.0 * PI * r * r
    );
    for (i, fi) in k.faces(f).unwrap().iter().enumerate() {
        if i < 30 {
            eprintln!("  face {i} {:?} area {:.9}", fi.kind, fi.area);
        }
    }
}

#[test]
fn debug_octant() {
    use std::f64::consts::PI;
    let r = 3.0f64;
    let c = Point3::new(0., 0., 0.);
    for (name, ball) in [
        (
            "aligned",
            native::blend::ball_frame(c, r, Vector3::z(), -Vector3::new(1.0, 1.0, 0.0).normalize())
                .unwrap(),
        ),
        ("default", native::blend::ball_body(c, r).unwrap()),
    ] {
        let m = native::mass::body_mass(&ball);
        eprintln!(
            "{name} ball: vol err {:.3e} area err {:.3e}",
            m.volume - 4.0 / 3.0 * PI * r.powi(3),
            m.surface_area - 4.0 * PI * r * r
        );
        let bx = native::blend::hexahedron(&[
            Point3::new(0., 0., 0.),
            Point3::new(5., 0., 0.),
            Point3::new(5., 5., 0.),
            Point3::new(0., 5., 0.),
            Point3::new(0., 0., 5.),
            Point3::new(5., 0., 5.),
            Point3::new(5., 5., 5.),
            Point3::new(0., 5., 5.),
        ])
        .unwrap();
        let o = native::boolean::boolean_bodies(&ball, &bx, BoolOp::Intersect)
            .unwrap()
            .body;
        let m = native::mass::body_mass(&o);
        eprintln!(
            "{name} octant: vol err {:.3e} area err {:.3e} faces {}",
            m.volume - PI * r.powi(3) / 6.0,
            m.surface_area - (PI * r * r / 2.0 + 3.0 * PI * r * r / 4.0),
            o.faces.len()
        );
        for (i, f) in o.faces.iter().enumerate() {
            let q = native::mass::face_integrals(&o, f, &Vector3::zeros());
            eprintln!(
                "   face {i} {:?} area {:.12} loops {:?}",
                f.surf.kind(),
                q[0],
                f.loops
                    .iter()
                    .map(|l| l
                        .iter()
                        .map(|c| (
                            o.edges[c.edge].degenerate,
                            format!("{:?}", std::mem::discriminant(&c.pc))
                        ))
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn debug_corner_face() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 30., 30.))]),
            Extent::Blind(30.0),
        )
        .unwrap()
        .bodies[0];
    let v = k
        .vertices(b)
        .unwrap()
        .into_iter()
        .find(|x| (x.point - Point3::new(30., 30., 30.)).norm() < 1e-9)
        .unwrap();
    let r = k
        .fillet_with(b, &v.edges, &FilletSpec::radius(3.0))
        .unwrap()
        .bodies[0];
    let body = k.get(r).unwrap();
    for (i, v) in body.verts.iter().enumerate() {
        let off = |x: f64| (x - (x * 2.0).round() / 2.0).abs();
        if off(v.p.x).max(off(v.p.y)).max(off(v.p.z)) > 1e-12 {
            eprintln!("inexact v{i} {:?}", v.p);
        }
    }
    for (i, f) in body.faces.iter().enumerate() {
        let q = native::mass::face_integrals(body, f, &Vector3::zeros());
        eprintln!("face {i} {:?} area {:.12}", f.surf.kind(), q[0]);
        if matches!(
            f.surf,
            native::surface::Surface::Sphere { .. } | native::surface::Surface::Cylinder { .. }
        ) {
            for l in &f.loops {
                for c in l {
                    let e = &body.edges[c.edge];
                    eprintln!(
                        "      edge {:?} deg {} t {:?} pc {:?}",
                        e.curve, e.degenerate, e.t, c.pc
                    );
                }
            }
        }
    }
}

#[test]
fn debug_concave_conic() {
    let mut k = NativeKernel::new();
    let l = Loop {
        curves: {
            let pts = [
                p(0., 0.),
                p(20., 0.),
                p(20., 5.),
                p(5., 5.),
                p(5., 20.),
                p(0., 20.),
            ];
            (0..6)
                .map(|i| Curve2::Line {
                    a: pts[i],
                    b: pts[(i + 1) % 6],
                    source: None,
                })
                .collect()
        },
    };
    let lb = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(l)]),
            Extent::Blind(10.0),
        )
        .unwrap()
        .bodies[0];
    let inner = k
        .edges(lb)
        .unwrap()
        .into_iter()
        .find(|e| (e.mid - Point3::new(5.0, 5.0, 5.0)).norm() < 1e-9)
        .unwrap()
        .id;
    for prof in [FilletProfile::Circular, FilletProfile::Conic { rho: 0.5 }] {
        let spec = FilletSpec {
            profile: prof,
            ..FilletSpec::radius(2.0)
        };
        let f = k.fillet_with(lb, &[inner], &spec).unwrap().bodies[0];
        eprintln!(
            "{prof:?}: added {:.9} per mm (circle {:.9}) solids {} faces {}",
            (k.mass_properties(f).unwrap().volume - 1750.0) / 10.0,
            1.0 - std::f64::consts::PI / 4.0 * 4.0 / 4.0,
            k.solid_count(f).unwrap(),
            k.faces(f).unwrap().len()
        );
    }
}

#[test]
fn debug_sweep() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let prof = Profile::new(
        Plane {
            origin: Point3::origin(),
            x_dir: Vector3::y_axis(),
            normal: Vector3::x_axis(),
        },
        vec![Region {
            outer: Loop {
                curves: vec![Curve2::Circle {
                    center: p(0., 0.),
                    radius: 5.0,
                    source: None,
                }],
            },
            holes: vec![],
            source: Some(7),
        }],
    );
    let plane = Plane::top();
    let l = |a, b| PathCurve::Sketch {
        plane,
        curve: Curve2::Line { a, b, source: None },
    };
    let paths = vec![
        ("line", vec![l(p(0., 0.), p(100., 0.))]),
        (
            "arc",
            vec![PathCurve::Sketch {
                plane,
                curve: Curve2::Arc {
                    center: p(0., 50.),
                    radius: 50.0,
                    start_angle: -PI / 2.0,
                    sweep: PI / 2.0,
                    source: None,
                },
            }],
        ),
        (
            "L",
            vec![
                l(p(0., 0.), p(100., 0.)),
                PathCurve::Sketch {
                    plane,
                    curve: Curve2::Arc {
                        center: p(100., 50.),
                        radius: 50.0,
                        start_angle: -PI / 2.0,
                        sweep: PI / 2.0,
                        source: None,
                    },
                },
                l(p(150., 50.), p(150., 130.)),
            ],
        ),
    ];
    for (name, path) in paths {
        let spec = SweepSpec {
            body: BodyKind::Solid,
            path,
            control: SweepControl::None,
            corners: Default::default(),
            faces: vec![],
        };
        match k.sweep_with(&prof, &spec) {
            Ok(r) => {
                let b = r.bodies[0];
                eprintln!(
                    "{name}: vol {:.6} solids {} faces {} check {:?}",
                    k.mass_properties(b).unwrap().volume,
                    k.solid_count(b).unwrap(),
                    k.faces(b).unwrap().len(),
                    k.get(b).unwrap().check()
                );
            }
            Err(e) => eprintln!("{name}: err {e}"),
        }
    }
}

#[test]
fn debug_loft_match() {
    let mut k = NativeKernel::new();
    let circle_at = |z: f64, r: f64| {
        Profile::new(
            Plane {
                origin: Point3::new(0.0, 0.0, z),
                ..Plane::top()
            },
            vec![Region {
                outer: Loop {
                    curves: vec![Curve2::Circle {
                        center: p(0., 0.),
                        radius: r,
                        source: None,
                    }],
                },
                holes: vec![],
                source: Some(4),
            }],
        )
    };
    let none = LoftEnd::default();
    let first = k
        .loft_with(&LoftSpec {
            body: BodyKind::Solid,
            sections: vec![
                LoftSection::Profile(circle_at(0.0, 10.0)),
                LoftSection::Profile(circle_at(20.0, 5.0)),
            ],
            start: none,
            end: none,
            source: 9,
        })
        .unwrap()
        .bodies[0];
    let cap = k
        .faces(first)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|pl| pl.normal.z > 0.9))
        .unwrap()
        .id;
    let m = k
        .loft_with(&LoftSpec {
            body: BodyKind::Solid,
            sections: vec![
                LoftSection::Face(FaceInput {
                    body: first,
                    face: cap,
                    source: 7,
                }),
                LoftSection::Profile(circle_at(40.0, 10.0)),
            ],
            start: LoftEnd {
                condition: LoftCondition::MatchTangent,
                magnitude: 1.0,
            },
            end: none,
            source: 9,
        })
        .unwrap()
        .bodies[0];
    let (a, b) = (
        (**k.get(first).unwrap()).clone(),
        (**k.get(m).unwrap()).clone(),
    );
    for (n, f) in a.faces.iter().enumerate() {
        eprintln!("A face {n} {:?}", f.surf.kind());
    }
    for (n, f) in b.faces.iter().enumerate() {
        eprintln!("B face {n} {:?}", f.surf.kind());
    }
    let t = std::time::Instant::now();
    let r = native::boolean::boolean_bodies(&a, &b, BoolOp::Union);
    eprintln!(
        "union: {:?} in {:?}",
        r.as_ref()
            .map(|r| (r.body.faces.len(), r.body.solids.len()))
            .map_err(|e| e.to_string()),
        t.elapsed()
    );
}

#[test]
fn debug_d_chamfer() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let d_loop = Loop {
        curves: vec![
            Curve2::Arc {
                center: p(0., 0.),
                radius: 10.0,
                start_angle: 0.0,
                sweep: PI,
                source: None,
            },
            Curve2::Line {
                a: p(-10., 0.),
                b: p(10., 0.),
                source: None,
            },
        ],
    };
    let d = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(d_loop)]),
            Extent::Blind(10.0),
        )
        .unwrap()
        .bodies[0];
    let straight = k
        .edges(d)
        .unwrap()
        .into_iter()
        .find(|e| e.curve == CurveKind::Line && (e.length - 10.0).abs() < 1e-9 && e.start.x > 0.0)
        .unwrap()
        .id;
    let r = k.chamfer_with(
        d,
        &[straight],
        &ChamferOpts::new(ChamferSpec::EqualDistance(2.0)),
    );
    match r {
        Ok(r) => eprintln!(
            "D chamfer vol {} (D {}) solids {}",
            k.mass_properties(r.bodies[0]).unwrap().volume,
            k.mass_properties(d).unwrap().volume,
            k.solid_count(r.bodies[0]).unwrap()
        ),
        Err(e) => eprintln!("err {e}"),
    }
}

#[test]
fn debug_width_chain() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let l = |a, b| Curve2::Line { a, b, source: None };
    let ar = |c, r, s, w| Curve2::Arc {
        center: c,
        radius: r,
        start_angle: s,
        sweep: w,
        source: None,
    };
    let rounded = Loop {
        curves: vec![
            l(p(10., 0.), p(50., 0.)),
            ar(p(50., 10.), 10.0, -PI / 2.0, PI / 2.0),
            l(p(60., 10.), p(60., 30.)),
            ar(p(50., 30.), 10.0, 0.0, PI / 2.0),
            l(p(50., 40.), p(10., 40.)),
            ar(p(10., 30.), 10.0, PI / 2.0, PI / 2.0),
            l(p(0., 30.), p(0., 10.)),
            ar(p(10., 10.), 10.0, PI, PI / 2.0),
        ],
    };
    let plate = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rounded)]),
            Extent::Blind(20.0),
        )
        .unwrap()
        .bodies[0];
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let poly = |pts: &[Point2<f64>]| Loop {
        curves: (0..pts.len())
            .map(|i| l(pts[i], pts[(i + 1) % pts.len()]))
            .collect(),
    };
    let wedge = k
        .extrude(
            &Profile::new(
                front,
                vec![reg(poly(&[
                    p(-10., 7.5),
                    p(70., 27.5),
                    p(70., 40.),
                    p(-10., 40.),
                ]))],
            ),
            Extent::Symmetric(200.0),
        )
        .unwrap()
        .bodies[0];
    let cut = k.boolean(BoolOp::Subtract, plate, &[wedge]).unwrap().bodies[0];
    let v_cut = k.mass_properties(cut).unwrap().volume;
    {
        let b = k.get(cut).unwrap();
        eprintln!(
            "cut: vol {v_cut} solids {} faces {} check {:?}",
            b.solids.len(),
            b.faces.len(),
            b.check()
        );
        let small = k
            .extrude(
                &Profile::new(Plane::top(), vec![reg(rect(20., 15., 30., 25.))]),
                Extent::Blind(30.0),
            )
            .unwrap()
            .bodies[0];
        let r = k.boolean(BoolOp::Subtract, cut, &[small]);
        eprintln!(
            "cut − small box: {:?}",
            r.map(|r| k.mass_properties(r.bodies[0]).unwrap().volume)
        );
    }
    let slope = k
        .faces(cut)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.plane
                .is_some_and(|pl| pl.normal.x < -0.1 && pl.normal.z > 0.5)
        })
        .unwrap()
        .id;
    let e0 = k
        .edges(cut)
        .unwrap()
        .into_iter()
        .find(|e| e.faces.contains(&Some(slope)))
        .unwrap()
        .id;
    let chain = k.tangent_chain(cut, e0).unwrap();
    let infos = k.edges(cut).unwrap();
    let spec = FilletSpec {
        size: FilletSize::Width(3.0),
        ..FilletSpec::radius(1.0)
    };
    for e in &chain {
        let i = &infos[e.0 as usize];
        let body = k.get(cut).unwrap().clone();
        let tool = native::blend::edge_tool(
            &body,
            cut,
            e.0 as usize,
            &|phi: f64| native::blend::SectionKind::Circle {
                r: 3.0 / (2.0 * (phi / 2.0).sin()),
            },
            false,
        );
        let tv = tool
            .as_ref()
            .map(|t| native::mass::body_mass(&t.body).volume)
            .map_err(|e| e.to_string());
        eprintln!(
            "edge {:?} {:?} len {:.3}: tool vol {:?}",
            e, i.curve, i.length, tv
        );
        if let Ok(t) = tool {
            if std::env::var("ONLY_FIRST").is_ok() && e.0 != 3 {
                continue;
            }
            if std::env::var("SKIP_SINGLE").is_ok() {
                continue;
            }
            eprintln!(
                "   tool check {:?} solids {}",
                t.body.check(),
                t.body.solids.len()
            );
            match native::boolean::boolean_bodies(&body, &t.body, BoolOp::Subtract) {
                Ok(r) => eprintln!(
                    "   removed {:.4} solids {}",
                    v_cut - native::mass::body_mass(&r.body).volume,
                    r.body.solids.len()
                ),
                Err(er) => eprintln!("   err {er}"),
            }
        }
    }
    let _ = (v_cut, spec);
    {
        let body = k.get(cut).unwrap().clone();
        // The chain as (edge, forward) in order: walk it.
        let ch: Vec<(usize, bool)> = {
            let mut v: Vec<(usize, bool)> = Vec::new();
            let es: Vec<usize> = chain.iter().map(|e| e.0 as usize).collect();
            // start: an edge whose start vertex touches no other chain edge
            let touches = |vx: usize, skip: usize| {
                es.iter()
                    .any(|&o| o != skip && body.edges[o].v.contains(&vx))
            };
            let mut cur = es
                .iter()
                .copied()
                .find(|&e| !touches(body.edges[e].v[0], e))
                .map(|e| (e, true))
                .or_else(|| {
                    es.iter()
                        .copied()
                        .find(|&e| !touches(body.edges[e].v[1], e))
                        .map(|e| (e, false))
                })
                .unwrap();
            v.push(cur);
            loop {
                let end = if cur.1 {
                    body.edges[cur.0].v[1]
                } else {
                    body.edges[cur.0].v[0]
                };
                let Some(n) = es
                    .iter()
                    .copied()
                    .find(|&o| !v.iter().any(|x| x.0 == o) && body.edges[o].v.contains(&end))
                else {
                    break;
                };
                cur = (n, body.edges[n].v[0] == end);
                v.push(cur);
            }
            v
        };
        let t = native::blend::chain_tool(
            &body,
            cut,
            &ch,
            &|phi: f64| native::blend::SectionKind::Circle {
                r: 3.0 / (2.0 * (phi / 2.0).sin()),
            },
            false,
            (0.0, 1.0),
            (true, true),
        )
        .unwrap();
        eprintln!(
            "chain tool: vol {} solids {} check {:?}",
            native::mass::body_mass(&t.body).volume,
            t.body.solids.len(),
            t.body.check().len()
        );
        for (ei, e) in t.body.edges.iter().enumerate() {
            if e.degenerate {
                continue;
            }
            let mut worst: f64 = f64::INFINITY;
            let mut best_face = 0;
            for (fi, f) in body.faces.iter().enumerate() {
                let w = (0..=20)
                    .map(|i| {
                        f.surf
                            .distance(&e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 20.0))
                    })
                    .fold(0.0f64, f64::max);
                if w < worst {
                    worst = w;
                    best_face = fi;
                }
            }
            eprintln!("  tool edge {ei}: nearest body surface {best_face} max dev {worst:.3e}");
        }
    }
    let mk = || -> Box<dyn Fn(f64) -> native::blend::SectionKind> {
        Box::new(|phi: f64| native::blend::SectionKind::Circle {
            r: 3.0 / (2.0 * (phi / 2.0).sin()),
        })
    };
    for n in 1..=chain.len() {
        type Section = Box<dyn Fn(f64) -> native::blend::SectionKind>;
        let list: Vec<(EdgeId, Section, bool)> =
            chain[..n].iter().map(|e| (*e, mk(), false)).collect();
        match native::blend::blend(&mut k, cut, &list, false) {
            Ok(r) => eprintln!(
                "chain[..{n}]: removed {:.4}",
                v_cut - k.mass_properties(r.bodies[0]).unwrap().volume
            ),
            Err(er) => eprintln!("chain[..{n}]: err {er}"),
        }
    }
}

/// A Ø2 disc swept along a 3D cubic Bézier (rotation-minimizing frames): Pappus, V = A·length.
#[test]
fn debug_sweep_bezier() {
    let mut k = NativeKernel::new();
    let poles = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 20.0),
        Point3::new(20.0, 10.0, 20.0),
        Point3::new(30.0, 30.0, 40.0),
    ];
    let path = vec![PathCurve::Bezier3 {
        poles: poles.clone(),
        weights: vec![],
    }];
    let prof = Profile::new(
        Plane::top(),
        vec![reg(Loop {
            curves: vec![Curve2::Circle {
                center: p(0.0, 0.0),
                radius: 1.0,
                source: None,
            }],
        })],
    );
    for control in [SweepControl::None, SweepControl::KeepOrientation] {
        let spec = SweepSpec {
            body: BodyKind::Solid,
            path: path.clone(),
            control,
            corners: Default::default(),
            faces: vec![],
        };
        let t = std::time::Instant::now();
        let r = k.sweep_with(&prof, &spec).unwrap();
        let b = r.bodies[0];
        let ts = t.elapsed();
        let m = k.mass_properties(b).unwrap();
        eprintln!("sweep {ts:?} mass {:?}", t.elapsed() - ts);
        // Arc length of the Bézier.
        let bez = |t: f64| {
            let u = 1.0 - t;
            poles[0].coords * u * u * u
                + poles[1].coords * 3.0 * u * u * t
                + poles[2].coords * 3.0 * u * t * t
                + poles[3].coords * t * t * t
        };
        let mut len = 0.0;
        for i in 0..20000 {
            len += (bez((i + 1) as f64 / 20000.0) - bez(i as f64 / 20000.0)).norm();
        }
        eprintln!(
            "{control:?}: volume {} want {} ({:?}) faces {}",
            m.volume,
            std::f64::consts::PI * len,
            t.elapsed(),
            k.faces(b).unwrap().len()
        );
        if control == SweepControl::None {
            assert!(
                (m.volume - std::f64::consts::PI * len).abs() < 1e-3 * std::f64::consts::PI * len,
                "{} vs {}",
                m.volume,
                std::f64::consts::PI * len
            );
        }
    }
}

/// A Ø40 cylinder minus a full revolve of a 4 × 4 square straddling its wall (the revolve's
/// seams lie in the cylinder's seam plane): V = π·20²·40 − π(22² − 18²)·4.
#[test]
fn debug_groove_ring() {
    let mut k = NativeKernel::new();
    let c = Loop {
        curves: vec![Curve2::Circle {
            center: p(0.0, 0.0),
            radius: 20.0,
            source: None,
        }],
    };
    let cyl = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(c)]),
            Extent::Blind(40.0),
        )
        .unwrap()
        .bodies[0];
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let axis = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    let ring = k
        .revolve(
            &Profile::new(front, vec![reg(rect(18., 18., 22., 22.))]),
            axis,
            std::f64::consts::TAU,
        )
        .unwrap()
        .bodies[0];
    eprintln!("ring volume {}", k.mass_properties(ring).unwrap().volume);
    let r = k.boolean(BoolOp::Subtract, cyl, &[ring]).unwrap();
    let v = k.mass_properties(r.bodies[0]).unwrap().volume;
    let want = std::f64::consts::PI * (16000.0 - 304.0);
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
}

/// A thin full revolve of a 4 × 4 square's loop, 1 mm inside: V = 12·2π·20.
#[test]
fn debug_thin_ring() {
    let mut k = NativeKernel::new();
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let axis = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    for prof in [
        Profile::new(front, vec![reg(rect(18., 18., 22., 22.))]),
        Profile {
            plane: front,
            regions: vec![],
            chains: vec![Chain {
                curves: rect(18., 18., 22., 22.).curves,
                source: None,
            }],
        },
    ] {
        let spec = RevolveSpec {
            body: BodyKind::Thin {
                left: 1.0,
                right: 0.0,
            },
            ..RevolveSpec::solid(axis, None)
        };
        match k.revolve_with(&prof, &spec) {
            Ok(r) => {
                let v = k.mass_properties(r.bodies[0]).unwrap().volume;
                let want = 12.0 * std::f64::consts::TAU * 20.0;
                eprintln!("thin ring {v} vs {want}");
            }
            Err(e) => eprintln!("thin ring: {e}"),
        }
    }
}

/// Two full revolves of overlapping rectangles united (thin walls' bands).
#[test]
fn debug_ring_union() {
    let mut k = NativeKernel::new();
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let axis = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    let ring = |k: &mut NativeKernel, r: Loop| {
        k.revolve(
            &Profile::new(front, vec![reg(r)]),
            axis,
            std::f64::consts::TAU,
        )
        .unwrap()
        .bodies[0]
    };
    let a = ring(&mut k, rect(18., 18., 19., 22.));
    let b = ring(&mut k, rect(18., 21., 22., 22.));
    let r = k.boolean(BoolOp::Union, a, &[b]);
    let tau = std::f64::consts::TAU;
    // Areas 4 + 4 − 1 = 7 at centroids: A·r̄ = 4·18.5 + 4·20 − 1·18.5.
    let want = tau * (4.0 * 18.5 + 4.0 * 20.0 - 18.5);
    match r {
        Ok(r) => {
            let v = k.mass_properties(r.bodies[0]).unwrap().volume;
            assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
        }
        Err(e) => panic!("{e}"),
    }
}

/// A slotted pan head screw (#4-40 × 1/2): a revolved profile with lines and cones, minus a slot
/// across the head down to 0.6 of its height. One solid.
#[test]
fn debug_pan_head_slot() {
    let mut k = NativeKernel::new();
    let u = 25.4;
    let (d, hh, a, length, j) = (
        0.112 * u,
        0.068 * u,
        0.219 * u / 2.0,
        0.5 * u,
        0.039 * u / 2.0,
    );
    let w = a * 1.2;
    let pts = [
        (0.0, -length),
        (d / 2.0, -length),
        (d / 2.0, 0.0),
        (a, 0.0),
        (a, 0.45 * hh),
        (0.88 * a, 0.82 * hh),
        (0.68 * a, hh),
        (0.0, hh),
    ];
    let n = pts.len();
    let lp = Loop {
        curves: (0..n)
            .map(|i| Curve2::Line {
                a: p(pts[i].0, pts[i].1),
                b: p(pts[(i + 1) % n].0, pts[(i + 1) % n].1),
                source: None,
            })
            .collect(),
    };
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let axis = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    let body = k
        .revolve(
            &Profile::new(front, vec![reg(lp)]),
            axis,
            std::f64::consts::TAU,
        )
        .unwrap()
        .bodies[0];
    let v0 = k.mass_properties(body).unwrap().volume;
    let slot = k
        .extrude(
            &Profile::new(
                Plane {
                    origin: Point3::new(0.0, 0.0, 0.6 * hh),
                    ..Plane::top()
                },
                vec![reg(rect(-w, -j, w, j))],
            ),
            Extent::Blind(0.7 * hh),
        )
        .unwrap()
        .bodies[0];
    let r = k.boolean(BoolOp::Subtract, body, &[slot]).unwrap();
    let m = k.mass_properties(r.bodies[0]).unwrap();
    eprintln!(
        "solids {} volume {} (before {v0})",
        k.solid_count(r.bodies[0]).unwrap(),
        m.volume
    );
    assert_eq!(k.solid_count(r.bodies[0]).unwrap(), 1);
}

/// A rounded rectangle (x −55..55, y −200..0, R20 corners) 40 high, shelled 4 mm from the
/// bottom: V = A·40 − A'·36 with A' the outline offset 4 inwards (R16 corners).
#[test]
fn debug_rounded_box_shell() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let rr = |x0: f64, y0: f64, x1: f64, y1: f64, r: f64| -> Loop {
        let l = |a, b| Curve2::Line { a, b, source: None };
        let arc = |cx: f64, cy: f64, a0: f64| Curve2::Arc {
            center: p(cx, cy),
            radius: r,
            start_angle: a0,
            sweep: PI / 2.0,
            source: None,
        };
        Loop {
            curves: vec![
                l(p(x0 + r, y0), p(x1 - r, y0)),
                arc(x1 - r, y0 + r, -PI / 2.0),
                l(p(x1, y0 + r), p(x1, y1 - r)),
                arc(x1 - r, y1 - r, 0.0),
                l(p(x1 - r, y1), p(x0 + r, y1)),
                arc(x0 + r, y1 - r, PI / 2.0),
                l(p(x0, y1 - r), p(x0, y0 + r)),
                arc(x0 + r, y0 + r, PI),
            ],
        }
    };
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rr(-55., -200., 55., 0., 20.))]),
            Extent::Blind(40.0),
        )
        .unwrap()
        .bodies[0];
    let faces = k.faces(b).unwrap();
    let bottom = faces
        .iter()
        .find(|f| f.plane.is_some_and(|p| p.normal.z < -0.9))
        .unwrap()
        .id;
    let area = |w: f64, h: f64, r: f64| w * h - (4.0 - PI) * r * r;
    let want = area(110.0, 200.0, 20.0) * 40.0 - area(102.0, 192.0, 16.0) * 36.0;
    let r = k.shell(b, &[bottom], 4.0).unwrap();
    let v = k.mass_properties(r.bodies[0]).unwrap().volume;
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
}

/// Replays a dumped shell (CADRS_DUMP_DIR): CADRS_REPRO=<file.body>, CADRS_REPRO_REMOVE=<face>,
/// CADRS_REPRO_T=<thickness>.
#[test]
fn debug_repro_shell() {
    let Ok(path) = std::env::var("CADRS_REPRO") else {
        return;
    };
    let mut k = NativeKernel::new();
    let body = cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap();
    let b = k.insert(body);
    let remove: Vec<FaceId> = std::env::var("CADRS_REPRO_REMOVE")
        .unwrap()
        .split(',')
        .map(|s| FaceId(s.parse().unwrap()))
        .collect();
    let t: f64 = std::env::var("CADRS_REPRO_T").unwrap().parse().unwrap();
    eprintln!("body volume {}", k.mass_properties(b).unwrap().volume);
    let r = k.shell(b, &remove, t);
    match r {
        Ok(r) => eprintln!(
            "shell volume {}",
            k.mass_properties(r.bodies[0]).unwrap().volume
        ),
        Err(e) => panic!("{e}"),
    }
}

/// A thread profile swept along a helix (cubic Bézier pieces, 16 a turn): per-face mass times.
#[test]
fn debug_thread_sweep_mass() {
    use std::f64::consts::TAU;
    let mut k = NativeKernel::new();
    let (r, pitch, turns, per) = (18.375, 4.125, 2.0, 16usize);
    let n = (turns * per as f64) as usize;
    let th = TAU / per as f64;
    let kk = 4.0 / 3.0 * (th / 4.0).tan();
    let pt = |a: f64| Point3::new(r * a.cos(), r * a.sin(), pitch * a / TAU);
    let tan = |a: f64| Vector3::new(-r * a.sin(), r * a.cos(), pitch / TAU);
    let path: Vec<PathCurve> = (0..n)
        .map(|i| {
            let (a0, a1) = (th * i as f64, th * (i + 1) as f64);
            let poles = vec![pt(a0), pt(a0) + tan(a0) * kk, pt(a1) - tan(a1) * kk, pt(a1)];
            PathCurve::Bezier3 {
                poles,
                weights: vec![],
            }
        })
        .collect();
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let poly = |pts: &[(f64, f64)]| Loop {
        curves: (0..pts.len())
            .map(|i| Curve2::Line {
                a: p(pts[i].0, pts[i].1),
                b: p(pts[(i + 1) % pts.len()].0, pts[(i + 1) % pts.len()].1),
                source: None,
            })
            .collect(),
    };
    let prof = Profile::new(
        front,
        vec![reg(poly(&[
            (18.375, -1.2),
            (19.8, -0.7),
            (19.8, 0.7),
            (18.375, 1.2),
        ]))],
    );
    let t = std::time::Instant::now();
    let r = k
        .sweep_with(
            &prof,
            &SweepSpec {
                body: BodyKind::Solid,
                path,
                control: SweepControl::None,
                corners: Default::default(),
                faces: vec![],
            },
        )
        .unwrap();
    eprintln!("sweep {:?}", t.elapsed());
    let b = k.get(r.bodies[0]).unwrap().clone();
    for (fi, f) in b.faces.iter().enumerate() {
        let t = std::time::Instant::now();
        let q = native::mass::face_integrals(&b, f, &Vector3::zeros());
        let pcs: Vec<String> = f
            .loops
            .iter()
            .flatten()
            .map(|c| match &c.pc {
                native::curve::Curve::Line { d, .. } => format!("L({:.1e},{:.1e})", d.x, d.y),
                native::curve::Curve::Nurbs(n) => format!("N{}", n.cps.len()),
                o => format!("{:?}", std::mem::discriminant(o)),
            })
            .collect();
        eprintln!(
            "face {fi} {:?}: {:?} area {:.4} pcs {pcs:?}",
            f.surf.kind(),
            t.elapsed(),
            q[0]
        );
    }
}

/// Per-face mass times of a dumped body: CADRS_REPRO=<file.body>.
#[test]
fn debug_repro_mass() {
    let Ok(path) = std::env::var("CADRS_REPRO") else {
        return;
    };
    let b = cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap();
    for (fi, f) in b.faces.iter().enumerate() {
        let t = std::time::Instant::now();
        let q = native::mass::face_integrals(&b, f, &Vector3::zeros());
        let pcs: Vec<String> = f
            .loops
            .iter()
            .flatten()
            .map(|c| {
                let e3 = match &b.edges[c.edge].curve {
                    native::curve::Curve::Nurbs(n) => {
                        format!("/3D N{} deg{}", n.cps.len(), n.degree)
                    }
                    _ => String::new(),
                };
                match &c.pc {
                    native::curve::Curve::Line { d, .. } => {
                        format!("L({:.1e},{:.1e}){e3}", d.x, d.y)
                    }
                    native::curve::Curve::Nurbs(n) => format!("N{}{e3}", n.cps.len()),
                    o => format!("{:?}", std::mem::discriminant(o)),
                }
            })
            .collect();
        let surf = match &f.surf {
            native::surface::Surface::Nurbs(n) => format!(
                "NURBS {}x{} deg {}x{}",
                n.cps.len(),
                n.cps[0].len(),
                n.du,
                n.dv
            ),
            s => format!("{:?}", s.kind()),
        };
        eprintln!(
            "face {fi} {surf}: {:?} area {:.4} pcs {pcs:?}",
            t.elapsed(),
            q[0]
        );
    }
}

/// The 10 × 20 × 30 block's vertical edge with a linear R2 → R4 law sampled at nine points.
#[test]
fn debug_variable_nine() {
    let mut k = NativeKernel::new();
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![reg(rect(0., 0., 10., 20.))]),
            Extent::Blind(30.0),
        )
        .unwrap()
        .bodies[0];
    let edge = k
        .edges(b)
        .unwrap()
        .into_iter()
        .find(|e| {
            (e.start.x - 10.0).abs() < 1e-9
                && (e.start.y - 20.0).abs() < 1e-9
                && (e.end.x - 10.0).abs() < 1e-9
                && (e.end.y - 20.0).abs() < 1e-9
        })
        .unwrap();
    let n = std::env::var("LAW_N")
        .map(|s| s.parse().unwrap())
        .unwrap_or(9usize);
    let radii: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let f = i as f64 / (n - 1) as f64;
            (f, 2.0 + 2.0 * f)
        })
        .collect();
    let r = k.fillet_variable(
        b,
        &[FilletLaw {
            edge: edge.id,
            radii,
        }],
        true,
    );
    match r {
        Ok(r) => eprintln!("volume {}", k.mass_properties(r.bodies[0]).unwrap().volume),
        Err(e) => panic!("{e}"),
    }
}

/// A thread profile swept along a helix (TURNS, default ¼, 16 Bézier pieces a turn), its inner
/// side at RIN (default 18: inside the Ø36.75 cylinder), united with the cylinder.
#[test]
fn debug_thread_union() {
    use std::f64::consts::TAU;
    let env = |k: &str, d: f64| {
        std::env::var(k)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(d)
    };
    let (r, pitch, turns, per) = (18.375, 4.125, env("TURNS", 0.25), 16usize);
    let rin = env("RIN", 18.0);
    let n = (turns * per as f64).round() as usize;
    let th = TAU / per as f64;
    let kk = 4.0 / 3.0 * (th / 4.0).tan();
    let pt = |a: f64| Point3::new(r * a.cos(), r * a.sin(), pitch * a / TAU);
    let tan = |a: f64| Vector3::new(-r * a.sin(), r * a.cos(), pitch / TAU);
    let path: Vec<PathCurve> = (0..n)
        .map(|i| {
            let (a0, a1) = (th * i as f64, th * (i + 1) as f64);
            PathCurve::Bezier3 {
                poles: vec![pt(a0), pt(a0) + tan(a0) * kk, pt(a1) - tan(a1) * kk, pt(a1)],
                weights: vec![],
            }
        })
        .collect();
    let mut k = NativeKernel::new();
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let pts = [(rin, -1.2), (19.8, -0.7), (19.8, 0.7), (rin, 1.2)];
    let lp = Loop {
        curves: (0..4)
            .map(|i| Curve2::Line {
                a: p(pts[i].0, pts[i].1),
                b: p(pts[(i + 1) % 4].0, pts[(i + 1) % 4].1),
                source: None,
            })
            .collect(),
    };
    let thread = k
        .sweep_with(
            &Profile::new(front, vec![reg(lp)]),
            &SweepSpec {
                body: BodyKind::Solid,
                path,
                control: SweepControl::None,
                corners: Default::default(),
                faces: vec![],
            },
        )
        .unwrap()
        .bodies[0];
    let disc = Loop {
        curves: vec![Curve2::Circle {
            center: p(0.0, 0.0),
            radius: r,
            source: None,
        }],
    };
    let cyl = k
        .extrude(
            &Profile::new(
                Plane {
                    origin: Point3::new(0.0, 0.0, -2.0),
                    ..Plane::top()
                },
                vec![reg(disc)],
            ),
            Extent::Blind(14.0),
        )
        .unwrap()
        .bodies[0];
    let (vt, vc) = (
        k.mass_properties(thread).unwrap().volume,
        k.mass_properties(cyl).unwrap().volume,
    );
    let kinds: Vec<SurfaceKind> = k.faces(thread).unwrap().iter().map(|f| f.kind).collect();
    eprintln!("thread faces: {kinds:?}");
    match k.boolean(BoolOp::Union, cyl, &[thread]) {
        Ok(u) => {
            let v = k.mass_properties(u.bodies[0]).unwrap().volume;
            eprintln!(
                "union {v} (thread {vt}, cylinder {vc}, outside part ≤ {})",
                vt
            );
            assert!(v > vc && v <= vc + vt + 1e-6 * vc);
        }
        Err(e) => panic!("{e}"),
    }
}

/// A Ø1 circle swept along a helix (R10, pitch 5, three turns; Bézier pieces, 16 a turn), the
/// circle in the plane through the axis at the start: V = π/4 · cos(slope) · length.
#[test]
fn debug_helix_tube() {
    use std::f64::consts::{PI, TAU};
    let (r, pitch, turns, per) = (10.0, 5.0, 3.0, 16usize);
    let n = (turns * per as f64) as usize;
    let th = TAU / per as f64;
    let kk = 4.0 / 3.0 * (th / 4.0).tan();
    let pt = |a: f64| Point3::new(r * a.cos(), r * a.sin(), pitch * a / TAU);
    let tan = |a: f64| Vector3::new(-r * a.sin(), r * a.cos(), pitch / TAU);
    let quad = std::env::var("QUAD").is_ok();
    let path: Vec<PathCurve> = (0..n)
        .map(|i| {
            let (a0, a1) = (th * i as f64, th * (i + 1) as f64);
            if quad {
                // Rational quadratic arcs (as cadrs_core's helix): the middle pole where the end
                // tangents meet, weight cos(θ/2), z through the middle.
                let w = (th / 2.0).cos();
                let m = (a0 + a1) / 2.0;
                let mid = Point3::new(r * m.cos() / w, r * m.sin() / w, pitch * m / TAU);
                PathCurve::Bezier3 {
                    poles: vec![pt(a0), mid, pt(a1)],
                    weights: vec![1.0, w, 1.0],
                }
            } else {
                PathCurve::Bezier3 {
                    poles: vec![pt(a0), pt(a0) + tan(a0) * kk, pt(a1) - tan(a1) * kk, pt(a1)],
                    weights: vec![],
                }
            }
        })
        .collect();
    let mut k = NativeKernel::new();
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let c = if std::env::var("ARC").is_ok() {
        Loop {
            curves: vec![Curve2::Arc {
                center: p(10.0, 0.0),
                radius: 0.5,
                start_angle: 0.0,
                sweep: TAU,
                source: None,
            }],
        }
    } else {
        Loop {
            curves: vec![Curve2::Circle {
                center: p(10.0, 0.0),
                radius: 0.5,
                source: None,
            }],
        }
    };
    let b = k
        .sweep_with(
            &Profile::new(front, vec![reg(c)]),
            &SweepSpec {
                body: BodyKind::Solid,
                path,
                control: SweepControl::None,
                corners: Default::default(),
                faces: vec![],
            },
        )
        .unwrap()
        .bodies[0];
    let v = k.mass_properties(b).unwrap().volume;
    let len = turns * ((TAU * r).powi(2) + pitch * pitch).sqrt();
    let slope = (pitch / (TAU * r)).atan();
    let want = PI * 0.25 * slope.cos() * len;
    let faces: Vec<SurfaceKind> = k.faces(b).unwrap().iter().map(|f| f.kind).collect();
    eprintln!("tube {v} want {want} ({} faces: {faces:?})", faces.len());
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
}

/// Replays a dumped boolean: CADRS_REPRO_A=<target.body>, CADRS_REPRO_B=<tool.body>,
/// CADRS_REPRO_OP=Subtract|Union|Intersect.
#[test]
fn debug_repro_boolean() {
    let (Ok(a), Ok(b)) = (
        std::env::var("CADRS_REPRO_A"),
        std::env::var("CADRS_REPRO_B"),
    ) else {
        return;
    };
    let op = match std::env::var("CADRS_REPRO_OP").as_deref() {
        Ok("Union") => BoolOp::Union,
        Ok("Intersect") => BoolOp::Intersect,
        _ => BoolOp::Subtract,
    };
    let mut k = NativeKernel::new();
    let a = k.insert(cadrs_kernel::native::bodyio::read(&std::fs::read(a).unwrap()).unwrap());
    let b = k.insert(cadrs_kernel::native::bodyio::read(&std::fs::read(b).unwrap()).unwrap());
    eprintln!(
        "volumes {} and {}",
        k.mass_properties(a).unwrap().volume,
        k.mass_properties(b).unwrap().volume
    );
    match k.boolean(op, a, &[b]) {
        Ok(r) => eprintln!(
            "result volume {}",
            k.mass_properties(r.bodies[0]).unwrap().volume
        ),
        Err(e) => panic!("{e}"),
    }
}

/// A Ø25 bar along X minus a counterbored hole across it along Y (Ø5.5 through, ⌴Ø9.75 from
/// below): the hand brake's grip without its slot.
#[test]
fn debug_cbore_across_bar() {
    use std::f64::consts::{PI, TAU};
    let mut k = NativeKernel::new();
    let right = Plane {
        origin: Point3::new(-10.0, 0.0, 0.0),
        x_dir: Vector3::y_axis(),
        normal: Vector3::x_axis(),
    };
    let c = |cx: f64, cy: f64, r: f64| Loop {
        curves: vec![Curve2::Circle {
            center: p(cx, cy),
            radius: r,
            source: None,
        }],
    };
    let bar = k
        .extrude(
            &Profile::new(right, vec![reg(c(-4.0, -12.5, 12.5))]),
            Extent::Blind(170.0),
        )
        .unwrap()
        .bodies[0];
    // The hole: Ø5.5 from y −17 to 9.5, the counterbore Ø9.75 from y −17 to −12.
    let bottom = Plane {
        origin: Point3::new(0.0, -17.0, 0.0),
        x_dir: Vector3::x_axis(),
        normal: Vector3::y_axis(),
    };
    let hole = k
        .extrude(
            &Profile::new(bottom, vec![reg(c(25.0, 12.5, 2.75))]),
            Extent::Blind(26.5),
        )
        .unwrap()
        .bodies[0];
    let cb = k
        .extrude(
            &Profile::new(bottom, vec![reg(c(25.0, 12.5, 4.875))]),
            Extent::Blind(5.0),
        )
        .unwrap()
        .bodies[0];
    let tool = match std::env::var("TOOL").as_deref() {
        Ok("hole") => hole,
        Ok("cb") => cb,
        _ => k.boolean(BoolOp::Union, hole, &[cb]).unwrap().bodies[0],
    };
    let v0 = k.mass_properties(bar).unwrap().volume;
    let vt = k.mass_properties(tool).unwrap().volume;
    eprintln!("bar {v0} (πr²L {}), tool {vt}", PI * 12.5 * 12.5 * 170.0);
    let _ = TAU;
    match k.boolean(BoolOp::Subtract, bar, &[tool]) {
        Ok(r) => eprintln!(
            "bar − hole {}",
            k.mass_properties(r.bodies[0]).unwrap().volume
        ),
        Err(e) => panic!("{e}"),
    }
}

/// A dumped fillet's input (`CADRS_REPRO_FILLET=<body>`, edges `CADRS_REPRO_EDGES=9,…`, width
/// `CADRS_REPRO_WIDTH` or radius `CADRS_REPRO_RADIUS`): the volume it removes and its new faces.
#[test]
fn debug_repro_fillet() {
    let Ok(path) = std::env::var("CADRS_REPRO_FILLET") else {
        return;
    };
    let mut k = NativeKernel::new();
    let b = k.insert(cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap());
    let edges: Vec<EdgeId> = std::env::var("CADRS_REPRO_EDGES")
        .unwrap()
        .split(',')
        .map(|s| EdgeId(s.parse().unwrap()))
        .collect();
    let size = match std::env::var("CADRS_REPRO_WIDTH") {
        Ok(w) => FilletSize::Width(w.parse().unwrap()),
        Err(_) => FilletSize::Radius(
            std::env::var("CADRS_REPRO_RADIUS")
                .unwrap()
                .parse()
                .unwrap(),
        ),
    };
    let spec = FilletSpec {
        size,
        ..FilletSpec::radius(1.0)
    };
    let v0 = k.mass_properties(b).unwrap().volume;
    let r = k.fillet_with(b, &edges, &spec).unwrap();
    let f = r.bodies[0];
    let mesh_vol = |k: &NativeKernel, b: BodyId| {
        let m = k
            .tessellate(
                b,
                Tessellation {
                    deflection: 0.01,
                    angle: 0.1,
                },
            )
            .unwrap();
        m.indices
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| m.positions[i as usize].coords);
                a.dot(&b.cross(&c)) / 6.0
            })
            .sum::<f64>()
    };
    eprintln!(
        "removed {} (mesh {})",
        v0 - k.mass_properties(f).unwrap().volume,
        mesh_vol(&k, b) - mesh_vol(&k, f)
    );
    // Each face's volume term, exact and from the mesh (about the bounding box's centre).
    let body = k.get(f).unwrap().clone();
    let shift = body.bbox().center().coords;
    let m = k
        .tessellate(
            f,
            Tessellation {
                deflection: 0.01,
                angle: 0.1,
            },
        )
        .unwrap();
    let mut by_face = vec![0.0; body.faces.len()];
    for (t, fid) in m.indices.iter().zip(&m.triangle_faces) {
        let [a, b, c] = t.map(|i| m.positions[i as usize].coords - shift);
        by_face[fid.0 as usize] += a.dot(&b.cross(&c)) / 6.0;
    }
    for (i, face) in body.faces.iter().enumerate() {
        let q = cadrs_kernel::native::mass::face_integrals(&body, face, &shift);
        let mesh_area: f64 = m
            .indices
            .iter()
            .zip(&m.triangle_faces)
            .filter(|(_, x)| x.0 as usize == i)
            .map(|(t, _)| {
                let [a, b, c] = t.map(|j| m.positions[j as usize]);
                (b - a).cross(&(c - a)).norm() / 2.0
            })
            .sum();
        if (q[4] - by_face[i]).abs() > 1e-3 * q[4].abs().max(1.0) {
            eprintln!("  area exact {:.4} mesh {mesh_area:.4}", q[0]);
            eprintln!(
                "face {i} {:?}: exact {:.4} mesh {:.4}",
                face.surf.kind(),
                q[4],
                by_face[i]
            );
        }
    }
    // Mesh areas of the new faces at several deflections (they should converge on the exact).
    for d in [0.1, 0.01, 0.001] {
        let m = k
            .tessellate(
                f,
                Tessellation {
                    deflection: d,
                    angle: 0.1,
                },
            )
            .unwrap();
        let mut areas: Vec<(u64, f64)> = Vec::new();
        for (fid, _) in &r.history.generated {
            let a: f64 = m
                .indices
                .iter()
                .zip(&m.triangle_faces)
                .filter(|(_, x)| *x == fid)
                .map(|(t, _)| {
                    let [a, b, c] = t.map(|j| m.positions[j as usize]);
                    (b - a).cross(&(c - a)).norm() / 2.0
                })
                .sum();
            areas.push((fid.0, a));
        }
        eprintln!("deflection {d}: {areas:.4?}");
    }
    if let Ok(fi) = std::env::var("CADRS_REPRO_MESH_FACE") {
        let fi: usize = fi.parse().unwrap();
        let es = cadrs_kernel::native::tess::edge_samples(&body, 0.01, 0.1);
        // A single-threaded debug repro: nothing else reads the environment meanwhile.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("CADRS_TESS_PROFILE", "1")
        };
        let fm = cadrs_kernel::native::tess::mesh_face(&body, &body.faces[fi], &es, 0.01, 0.1);
        #[allow(unsafe_code)]
        unsafe {
            std::env::remove_var("CADRS_TESS_PROFILE")
        };
        eprintln!("face {fi}: {} triangles", fm.indices.len());
    }
    // The uv boundary of each new face as the mesher samples it: self-crossings and repeats.
    let es = cadrs_kernel::native::tess::edge_samples(&body, 0.01, 0.1);
    for (fid, _) in &r.history.generated {
        let face = &body.faces[fid.0 as usize];
        let mut pts: Vec<(f64, f64, usize)> = Vec::new();
        for c in face.loops.iter().flatten() {
            let (ts, _) = &es[c.edge];
            let n = ts.len();
            let idx: Vec<usize> = if c.fwd {
                (0..n - 1).collect()
            } else {
                (1..n).rev().collect()
            };
            for i in idx {
                let q = c.pc.point(ts[i]);
                pts.push((q.x, q.y, c.edge));
            }
        }
        let n = pts.len();
        let mut crossings = 0;
        let mut first = None;
        let cross = |a: (f64, f64, usize),
                     b: (f64, f64, usize),
                     c: (f64, f64, usize),
                     d: (f64, f64, usize)| {
            let o = |p: (f64, f64, usize), q: (f64, f64, usize), r: (f64, f64, usize)| {
                (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
            };
            (o(a, b, c) > 0.0) != (o(a, b, d) > 0.0) && (o(c, d, a) > 0.0) != (o(c, d, b) > 0.0)
        };
        for i in 0..n {
            for j in i + 2..n {
                if (j + 1) % n == i {
                    continue;
                }
                if cross(pts[i], pts[(i + 1) % n], pts[j], pts[(j + 1) % n]) {
                    crossings += 1;
                    first.get_or_insert((i, j, pts[i], pts[j]));
                }
            }
        }
        let mut dup = 0;
        for i in 0..n {
            for j in i + 1..n {
                if (pts[i].0 - pts[j].0).abs() < 1e-12 && (pts[i].1 - pts[j].1).abs() < 1e-12 {
                    dup += 1;
                }
            }
        }
        let area: f64 = (0..n)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                a.0 * b.1 - b.0 * a.1
            })
            .sum::<f64>()
            / 2.0;
        eprintln!(
            "uv loop of face {}: {n} points, uv area {area:.6}, {crossings} self-crossings {first:?}, {dup} repeats",
            fid.0
        );
    }
    // The p-curves of the new faces against their edges.
    for (fid, _) in &r.history.generated {
        let face = &body.faces[fid.0 as usize];
        for c in face.loops.iter().flatten() {
            let e = &body.edges[c.edge];
            let mut worst: f64 = 0.0;
            for i in 0..=64 {
                let t = e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 64.0;
                let uv = c.pc.point(t);
                worst = worst.max((face.surf.eval(uv.x, uv.y) - e.point(t)).norm());
            }
            let kind = match &e.curve {
                cadrs_kernel::native::curve::Curve::Nurbs(n) => format!("nurbs {}", n.cps.len()),
                x => format!("{:?}", std::mem::discriminant(x)),
            };
            let pk = match &c.pc {
                cadrs_kernel::native::curve::Curve::Nurbs(n) => format!("nurbs {}", n.cps.len()),
                _ => "line".into(),
            };
            eprintln!(
                "face {} edge {} ({kind}, pc {pk}, deg {}): off {worst:.2e}",
                fid.0, c.edge, e.degenerate
            );
        }
    }
    let faces = k.faces(f).unwrap();
    for (fid, o) in &r.history.generated {
        let fi = faces.iter().find(|x| x.id == *fid).unwrap();
        eprintln!(
            "new face {fid:?} {o:?}: area {:.4} at {:.2?}",
            fi.area, fi.center
        );
    }
}

/// OCCT blend/simple/J1: a cone's edges blended one at a time (which is OCCT's `s_1`?).
#[test]
fn debug_draw_cone_blend() -> cadrs_kernel::Result<()> {
    let mut k = NativeKernel::new();
    let s = k.pcone(10.0, 5.0, 20.0)?;
    let s = k.tscale(&s, 0.0, 0.0, 0.0, 10.0)?;
    if let cadrs_kernel::native::draw::Shape::Body(b) = &s {
        let body = k.get(*b)?;
        let ef = body.edge_faces();
        for (i, e) in body.edges.iter().enumerate() {
            eprintln!(
                "edge {i}: degenerate {} faces {:?} {:?}",
                e.degenerate,
                ef[i],
                std::mem::discriminant(&e.curve)
            );
        }
    }
    let es = k.explode_edges(&s)?;
    for (i, e) in es.iter().enumerate() {
        match k.blend(&s, 10.0, &[e]) {
            Ok(r) => eprintln!("s_{}: area {}", i + 1, k.sprops(&r)?),
            Err(x) => eprintln!("s_{}: {x}", i + 1),
        }
    }
    Ok(())
}

/// OCCT blend/simple/B7 and friends: each edge of a 270° cylinder sector blended alone.
#[test]
fn debug_draw_sector_blends() -> cadrs_kernel::Result<()> {
    let mut k = NativeKernel::new();
    let s = k.pcylinder_a(None, 50.0, 100.0, 270.0)?;
    let es = k.explode_edges(&s)?;
    for combo in [
        [0usize, 4, 99],
        [0, 5, 99],
        [0, 1, 99],
        [0, 1, 4],
        [0, 1, 5],
        [0, 2, 99],
        [0, 8, 99],
        [4, 8, 99],
    ] {
        let picked: Vec<&cadrs_kernel::native::draw::Shape> = combo
            .iter()
            .filter(|&&i| i < es.len())
            .map(|&i| &es[i])
            .collect();
        match k.blend(&s, 10.0, &picked) {
            Ok(r) => eprintln!("combo {combo:?}: area {:.2}", k.sprops(&r)?),
            Err(x) => eprintln!("combo {combo:?}: {x}"),
        }
    }
    for (i, e) in es.iter().enumerate() {
        let len = k.lprops(e)?;
        match k.blend(&s, 10.0, &[e]) {
            Ok(r) => eprintln!(
                "edge {} (length {len:.2}): area {:.2}",
                i + 1,
                k.sprops(&r)?
            ),
            Err(x) => eprintln!("edge {} (length {len:.2}): {x}", i + 1),
        }
    }
    Ok(())
}

/// A 270° cylinder sector: its top arc and a vertical side edge blended together (they meet at
/// a corner).
#[test]
fn debug_sector_arc_and_side() -> cadrs_kernel::Result<()> {
    let mut k = NativeKernel::new();
    let s = k.pcylinder_a(None, 50.0, 100.0, 270.0)?;
    let es = k.explode_edges(&s)?;
    // Fails today (the two tools' contact rails cross on the cylinder): printed, not asserted.
    match k.blend(&s, 10.0, &[&es[0], &es[4]]) {
        Ok(r) => eprintln!("area {}", k.sprops(&r)?),
        Err(e) => eprintln!("blend failed: {e}"),
    }
    Ok(())
}

/// Reads every `.brep` under `CADRS_OCCT_DATA` and reports which fail (and why, by kind).
#[test]
#[ignore]
fn debug_read_occt_dataset() {
    let Ok(dir) = std::env::var("CADRS_OCCT_DATA") else {
        return;
    };
    let one = std::path::Path::new(&dir);
    let mut paths: Vec<_> = if one.is_file() {
        vec![one.to_path_buf()]
    } else {
        walk(one)
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "brep" || e == "rle"))
            .collect()
    };
    paths.sort();
    let (mut ok, mut fail) = (0, 0);
    let mut reasons: std::collections::BTreeMap<String, usize> = Default::default();
    for p in &paths {
        let bytes = std::fs::read(p).unwrap();
        let t = std::time::Instant::now();
        let r = std::panic::catch_unwind(|| cadrs_kernel::native::brep_occt::read(&bytes));
        let dt = t.elapsed().as_secs_f64();
        match r {
            Ok(Ok((b, w))) => {
                ok += 1;
                let issues = b.check();
                let v = cadrs_kernel::native::mass::body_mass(&b).volume;
                println!(
                    "OK {} faces {} solids {} warn {} issues {} volume {v:.6e} {dt:.2}s",
                    p.display(),
                    b.faces.len(),
                    b.solids.len(),
                    w.len(),
                    issues.len()
                );
                if paths.len() == 1 {
                    for x in w.iter().chain(issues.iter()) {
                        println!("  {x}");
                    }
                }
            }
            Ok(Err(e)) => {
                fail += 1;
                let k: String = e.chars().take(40).collect();
                *reasons.entry(k).or_default() += 1;
                println!(
                    "ERR {}: {}",
                    p.display(),
                    e.chars().take(200).collect::<String>()
                );
            }
            Err(_) => {
                fail += 1;
                *reasons.entry("panic".into()).or_default() += 1;
                println!("PANIC {}", p.display());
            }
        }
    }
    println!("read {ok}, failed {fail}");
    for (k, n) in reasons {
        println!("{n:5} {k}");
    }
}

fn walk(d: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p))
        } else {
            out.push(p)
        }
    }
    out
}

/// Replays a dumped chamfer: CADRS_REPRO_CHAMFER=<file.body>, CADRS_REPRO_EDGES=<e,e,…>,
/// CADRS_REPRO_D=<distance> (equal distance; two with CADRS_REPRO_D2=<second>).
#[test]
#[ignore]
fn debug_repro_chamfer() {
    let Ok(path) = std::env::var("CADRS_REPRO_CHAMFER") else {
        return;
    };
    let mut k = NativeKernel::new();
    let b = k.insert(cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap());
    let edges: Vec<EdgeId> = std::env::var("CADRS_REPRO_EDGES")
        .unwrap()
        .split(',')
        .map(|s| EdgeId(s.parse().unwrap()))
        .collect();
    let d: f64 = std::env::var("CADRS_REPRO_D").unwrap().parse().unwrap();
    let body = k.get(b).unwrap().clone();
    for e in &edges {
        let ed = &body.edges[e.0 as usize];
        let faces: Vec<_> = body.edge_faces()[e.0 as usize]
            .iter()
            .map(|&f| format!("{f}:{:?}", body.faces[f].surf.kind()))
            .collect();
        eprintln!(
            "edge {} {:?} {:?}..{:?} faces {faces:?}",
            e.0,
            std::mem::discriminant(&ed.curve),
            ed.point(ed.t[0]),
            ed.point(ed.t[1])
        );
    }
    let v0 = k.mass_properties(b).unwrap().volume;
    // (Two distances with `CADRS_REPRO_D2`.)
    let spec = match std::env::var("CADRS_REPRO_D2") {
        Ok(d2) => ChamferSpec::TwoDistances(d, d2.parse().unwrap()),
        Err(_) => ChamferSpec::EqualDistance(d),
    };
    let r = k.chamfer_with(b, &edges, &ChamferOpts::new(spec));
    match r {
        Ok(r) => eprintln!(
            "ok: removed {}",
            v0 - k.mass_properties(r.bodies[0]).unwrap().volume
        ),
        Err(e) => eprintln!("failed: {e}"),
    }
}

/// Prints an edge boolean's pieces: CADRS_EBOP_A=<file>, CADRS_EBOP_B=<file> (object a, tool b),
/// CADRS_EBOP_OP=common|cut|fuse.
#[test]
#[ignore]
fn debug_edge_bop() {
    let (Ok(a), Ok(b)) = (std::env::var("CADRS_EBOP_A"), std::env::var("CADRS_EBOP_B")) else {
        return;
    };
    let mut k = NativeKernel::new();
    let (sa, sb) = (k.restore(&a).unwrap(), k.restore(&b).unwrap());
    let show = |s: &draw::Shape| match s {
        draw::Shape::Wire(w) => {
            for e in w {
                eprintln!("  {:?} -> {:?} len {:.4}", e.start(), e.end(), e.length());
            }
        }
        other => eprintln!("  {other:?}"),
    };
    eprintln!("a:");
    show(&sa);
    eprintln!("b:");
    if let draw::Shape::Body(id) = sb {
        let body = k.get(id).unwrap();
        eprintln!(
            "  body: {} faces {} edges {} solids, volume {}, check {:?}",
            body.faces.len(),
            body.edges.len(),
            body.solids.len(),
            k.vprops(&sb).unwrap(),
            body.check()
        );
        if std::env::var("CADRS_EBOP_EDGES").is_ok() {
            for e in &body.edges {
                eprintln!(
                    "    edge {:?} t {:?} {:?} -> {:?} deg {}",
                    e.curve,
                    e.t,
                    e.point(e.t[0]),
                    e.point(e.t[1]),
                    e.degenerate
                );
            }
        }
    } else {
        show(&sb);
    }
    let r = match std::env::var("CADRS_EBOP_OP").as_deref() {
        Ok("cut") => k.bcut(&sa, &sb),
        Ok("fuse") => k.bfuse(&sa, &sb),
        _ => k.bcommon(&sa, &sb),
    }
    .unwrap();
    eprintln!(
        "result: length {}, {:?}",
        k.lprops(&r).unwrap(),
        k.nbshapes(&r).unwrap()
    );
    show(&r);
}

/// The distance from edge set A's points to B's edges along A (CADRS_EBOP_A, CADRS_EBOP_B).
#[test]
#[ignore]
fn debug_edge_gap() {
    let (Ok(a), Ok(b)) = (std::env::var("CADRS_EBOP_A"), std::env::var("CADRS_EBOP_B")) else {
        return;
    };
    let mut k = NativeKernel::new();
    let (draw::Shape::Wire(wa), draw::Shape::Wire(wb)) =
        (k.restore(&a).unwrap(), k.restore(&b).unwrap())
    else {
        panic!()
    };
    for e in &wa {
        for i in 0..=100 {
            let t = e.t0 + (e.t1 - e.t0) * i as f64 / 100.0;
            let p = e.curve.point(t);
            let d = wb
                .iter()
                .map(|o| {
                    let tt = o.curve.closest(&p, o.t0.min(o.t1), o.t0.max(o.t1));
                    (o.curve.point(tt) - p).norm()
                })
                .fold(f64::INFINITY, f64::min);
            if d < 1e-3 {
                eprintln!("{i:3} {:?} {d:.3e}", p);
            }
        }
    }
}

/// A boolean of two OCCT data files: CADRS_RB_A, CADRS_RB_B, CADRS_RB_OP=common|cut|fuse.
#[test]
#[ignore]
fn debug_restore_bop() {
    let (Ok(a), Ok(b)) = (std::env::var("CADRS_RB_A"), std::env::var("CADRS_RB_B")) else {
        return;
    };
    let mut k = NativeKernel::new();
    let (sa, sb) = (k.restore(&a).unwrap(), k.restore(&b).unwrap());
    for (n, s) in [("a", &sa), ("b", &sb)] {
        if let draw::Shape::Body(id) = s {
            let (v, a) = (k.vprops(s).unwrap(), k.sprops(s).unwrap());
            let body = k.get(*id).unwrap();
            eprintln!(
                "{n}: {} faces {} solids volume {v:.6} area {a:.6} check {:?}",
                body.faces.len(),
                body.solids.len(),
                body.check()
            );
            for f in &body.faces {
                eprint!("{:?} ", f.surf.kind());
            }
            eprintln!();
            if std::env::var("CADRS_RB_FACES").is_ok() {
                for (i, f) in body.faces.iter().enumerate() {
                    let n = f.surf.normal(0.0, 0.0) * if f.sense { 1.0 } else { -1.0 };
                    let loops: Vec<Vec<(usize, bool, [f64; 3])>> = f
                        .loops
                        .iter()
                        .map(|l| {
                            l.iter()
                                .map(|c| {
                                    let p = body.verts[body.co_start(c)].p;
                                    (c.edge, c.fwd, [p.x, p.y, p.z])
                                })
                                .collect()
                        })
                        .collect();
                    eprintln!("  face {i} normal {:?} loops {loops:?}", [n.x, n.y, n.z]);
                }
            }
        }
    }
    let t = std::time::Instant::now();
    let r = match std::env::var("CADRS_RB_OP").as_deref() {
        Ok("cut") => k.bcut(&sa, &sb),
        Ok("fuse") => k.bfuse(&sa, &sb),
        _ => k.bcommon(&sa, &sb),
    };
    match r {
        Ok(r) => eprintln!(
            "result in {:?}: area {:?} volume {:?}",
            t.elapsed(),
            k.sprops(&r),
            k.vprops(&r)
        ),
        Err(e) => eprintln!("failed in {:?}: {e}", t.elapsed()),
    }
}

/// Lists a chamfer result's faces (kind, area, normal, centre): CADRS_REPRO_CHAMFER,
/// CADRS_REPRO_EDGES, CADRS_REPRO_D.
#[test]
#[ignore]
fn debug_chamfer_faces() {
    let Ok(path) = std::env::var("CADRS_REPRO_CHAMFER") else {
        return;
    };
    let mut k = NativeKernel::new();
    let b = k.insert(cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap());
    let edges: Vec<EdgeId> = std::env::var("CADRS_REPRO_EDGES")
        .unwrap()
        .split(',')
        .map(|s| EdgeId(s.parse().unwrap()))
        .collect();
    let d: f64 = std::env::var("CADRS_REPRO_D").unwrap().parse().unwrap();
    let r = k
        .chamfer_with(b, &edges, &ChamferOpts::new(ChamferSpec::EqualDistance(d)))
        .unwrap();
    let out = r.bodies[0];
    let faces = k.faces(out).unwrap();
    eprintln!(
        "{} faces (input {})",
        faces.len(),
        k.faces(b).unwrap().len()
    );
    let body = k.get(out).unwrap();
    let bb = body.bbox();
    eprintln!(
        "bbox {:?} .. {:?} (input {:?} .. {:?}) check {:?} volume {} (input {})",
        bb.min,
        bb.max,
        k.get(b).unwrap().bbox().min,
        k.get(b).unwrap().bbox().max,
        body.check(),
        k.mass_properties(out).unwrap().volume,
        k.mass_properties(b).unwrap().volume
    );
    for f in &faces {
        eprintln!(
            "  {:?} area {:10.4} normal {:?} centre {:?}",
            f.kind,
            f.area,
            f.plane.map(|p| [p.normal.x, p.normal.y, p.normal.z]),
            [f.center.x, f.center.y, f.center.z]
        );
    }
}

/// The area of lofts with known answers (a cone from a circle to an apex, a frustum).
#[test]
#[ignore]
fn debug_loft_area() {
    use cadrs_kernel::native::NativeKernel;
    let mut k = NativeKernel::new();
    let c = k.circle(0.0, 0.0, 0.0, 10.0);
    let e = k.mkedge_full(&c);
    let w = k.wire(&[&e]).unwrap();
    let v = k.vertex(0.0, 0.0, 40.0);
    let cone = k.thrusections(false, false, &[&v, &w]).unwrap();
    let a = k.sprops(&cone).unwrap();
    println!(
        "cone {a} exact {}",
        std::f64::consts::PI * 10.0 * 1700f64.sqrt()
    );
    let c2 = k.circle(0.0, 0.0, 40.0, 5.0);
    let e2 = k.mkedge_full(&c2);
    let w2 = k.wire(&[&e2]).unwrap();
    let fr = k.thrusections(false, false, &[&w, &w2]).unwrap();
    let a = k.sprops(&fr).unwrap();
    println!(
        "frustum {a} exact {}",
        std::f64::consts::PI * 15.0 * 1625f64.sqrt()
    );
}

#[test]
#[ignore]
fn debug_restore_shapes() {
    use cadrs_kernel::native::NativeKernel;
    let mut k = NativeKernel::new();
    let Ok(d) = std::env::var("CADRS_OCCT_DATA") else {
        eprintln!("debug_restore_shapes: set CADRS_OCCT_DATA to OCCT's dataset folder");
        return;
    };
    for f in std::env::var("CADRS_FILES")
        .unwrap_or("C13 C1".into())
        .split_whitespace()
    {
        let s = k.restore(&format!("{d}/brep/{f}")).unwrap();
        println!("{f}: {s:?}");
    }
}

/// Reads every .brep under CADRS_BREP_DIR and prints what it gives (validity, volume, area).
#[test]
#[ignore]
fn debug_read_brep_dir() {
    use cadrs_kernel::native::brep_occt;
    let Ok(dir) = std::env::var("CADRS_BREP_DIR") else {
        return;
    };
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "brep"))
        .collect();
    names.sort();
    for p in names {
        let bytes = std::fs::read(&p).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        match brep_occt::read_ordered(&bytes) {
            Ok(r) => match r.body {
                Some(b) => {
                    let m = cadrs_kernel::native::mass::body_mass(&b);
                    println!(
                        "{name}: faces {} solids {} check {:?} volume {:.6} area {:.6}",
                        b.faces.len(),
                        b.solids.len(),
                        b.check().first(),
                        m.volume,
                        m.surface_area
                    );
                }
                None => println!(
                    "{name}: no body ({} free edges, {} points)",
                    r.free_edges.len(),
                    r.points.len()
                ),
            },
            Err(e) => println!("{name}: ERROR {e}"),
        }
    }
}

/// Reads every .smb under CADRS_SMB_DIR (recursively) and prints what it gives.
#[test]
#[ignore]
fn debug_read_smb_dir() {
    use cadrs_kernel::native::{mass, smb};
    let Ok(dir) = std::env::var("CADRS_SMB_DIR") else {
        return;
    };
    let mut files = Vec::new();
    let root = std::path::PathBuf::from(&dir);
    if root.is_file() {
        files.push(root.clone());
    }
    let mut stack = if root.is_dir() {
        vec![root.clone()]
    } else {
        vec![]
    };
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "smb") {
                files.push(p);
            }
        }
    }
    files.sort();
    let (mut ok, mut valid) = (0, 0);
    for p in &files {
        let name = p
            .strip_prefix(&dir)
            .ok()
            .filter(|x| !x.as_os_str().is_empty())
            .unwrap_or(p)
            .display()
            .to_string();
        let t = std::time::Instant::now();
        match smb::read(&std::fs::read(p).unwrap()) {
            Ok((b, w)) => {
                ok += 1;
                let issues = b.check();
                if issues.is_empty() && !b.solids.is_empty() {
                    valid += 1;
                }
                let m = mass::body_mass(&b);
                println!(
                    "{name}: faces {} solids {} check {:?} volume {:.4} warnings {} ({:?})",
                    b.faces.len(),
                    b.solids.len(),
                    issues.first(),
                    m.volume,
                    w.len(),
                    t.elapsed()
                );
            }
            Err(e) => println!("{name}: ERROR {e}"),
        }
    }
    println!("{} files, {ok} read, {valid} valid solids", files.len());
}

/// Free edges and sizes of a restored file before and after sewing (CADRS_FILE, CADRS_TOL).
#[test]
#[ignore]
fn debug_sew_file() {
    use cadrs_kernel::native::NativeKernel;
    let Ok(f) = std::env::var("CADRS_FILE") else {
        return;
    };
    let tol: f64 = std::env::var("CADRS_TOL")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(1e-6);
    let mut k = NativeKernel::new();
    let a = k.restore(&f).unwrap();
    let n = k.nbshapes(&a).unwrap();
    println!("input {n:?}");
    let r = k.sewing(tol, &[&a]).unwrap();
    println!("sewn {:?}", k.nbshapes(&r).unwrap());
    for want in 0..60 {
        if k.checkfreebounds(&r, want).is_ok() {
            println!("free edges {want}");
        }
    }
}

/// One SMLib fillet case's edges (CADRS_SMB file, CADRS_EDGES "a b c" file indices): what they
/// map to and what they are.
#[test]
#[ignore]
fn debug_smb_edges() {
    use cadrs_kernel::native::smb;
    let Ok(f) = std::env::var("CADRS_SMB") else {
        return;
    };
    let (body, mids, _) = smb::read_with_edges(&std::fs::read(&f).unwrap()).unwrap();
    println!(
        "{} faces, {} edges; surfaces {:?}",
        body.faces.len(),
        body.edges.len(),
        body.faces
            .iter()
            .map(|f| format!("{:?}", f.surf).chars().take(12).collect::<String>())
            .collect::<Vec<_>>()
    );
    let ef = body.edge_faces();
    for w in std::env::var("CADRS_EDGES")
        .unwrap_or_default()
        .split_whitespace()
    {
        let i: usize = w.parse().unwrap();
        let m = mids[i];
        let (e, d) = (0..body.edges.len())
            .filter(|&e| !body.edges[e].degenerate)
            .map(|e| {
                let ed = &body.edges[e];
                let t = ed.curve.closest(&m, ed.t[0], ed.t[1]);
                (e, (ed.point(t) - m).norm())
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let ed = &body.edges[e];
        println!(
            "file edge {i} at {m:?} -> edge {e} (off {d:.2e}) {:?} faces {:?} v {:?} tangents {:?} {:?}",
            std::mem::discriminant(&ed.curve),
            ef[e]
                .iter()
                .map(|&f| body.faces[f].surf.kind())
                .collect::<Vec<_>>(),
            ed.v,
            ed.curve.tangent(ed.t[0]).normalize(),
            ed.curve.tangent(ed.t[1]).normalize()
        );
    }
}

/// One SMLib case's result (CADRS_SMB file, CADRS_EDGES file indices, CADRS_R radius): faces
/// with their kinds and loop sizes.
#[test]
#[ignore]
fn debug_smb_fillet() {
    use cadrs_kernel::native::{NativeKernel, smb};
    use cadrs_kernel::{EdgeId, Kernel};
    let Ok(f) = std::env::var("CADRS_SMB") else {
        return;
    };
    let r: f64 = std::env::var("CADRS_R").unwrap().parse().unwrap();
    let (body, mids, _) = smb::read_with_edges(&std::fs::read(&f).unwrap()).unwrap();
    let ids: Vec<EdgeId> = std::env::var("CADRS_EDGES")
        .unwrap()
        .split_whitespace()
        .map(|w| {
            let m = mids[w.parse::<usize>().unwrap()];
            EdgeId(
                (0..body.edges.len())
                    .filter(|&e| !body.edges[e].degenerate)
                    .min_by(|&a, &b| {
                        let d = |e: usize| {
                            let ed = &body.edges[e];
                            let t = ed.curve.closest(&m, ed.t[0], ed.t[1]);
                            (ed.point(t) - m).norm()
                        };
                        d(a).total_cmp(&d(b))
                    })
                    .unwrap() as u64,
            )
        })
        .collect();
    let mut k = NativeKernel::new();
    let b = k.insert(body);
    let res = k.fillet(b, &ids, r).unwrap();
    let out = k.get(res.bodies[0]).unwrap();
    println!("{} faces {} edges", out.faces.len(), out.edges.len());
    let mut again = (**out).clone();
    cadrs_kernel::native::unify::merge_edges(&mut again);
    println!("merge_edges again: {} edges", again.edges.len());
    for (i, f) in out.faces.iter().enumerate() {
        println!(
            "  face {i} {:?} loops {:?}",
            f.surf.kind(),
            f.loops.iter().map(|l| l.len()).collect::<Vec<_>>()
        );
        if f.loops.len() == 1 && f.loops[0].len() == 3 {
            for c in &f.loops[0] {
                let e = &out.edges[c.edge];
                println!(
                    "      {:?} -> {:?} {:?}",
                    out.verts[e.v[0]].p,
                    out.verts[e.v[1]].p,
                    std::mem::discriminant(&e.curve)
                );
            }
        }
    }
    let mut ve = vec![Vec::new(); out.verts.len()];
    for (i, e) in out.edges.iter().enumerate() {
        ve[e.v[0]].push(i);
        if e.v[1] != e.v[0] {
            ve[e.v[1]].push(i);
        }
    }
    let ef = out.edge_faces();
    for (v, es) in ve.iter().enumerate().filter(|(_, es)| es.len() == 2) {
        println!(
            "  vertex {v} at {:?}: edges {es:?} {:?} faces {:?} {:?}",
            out.verts[v].p,
            es.iter()
                .map(|&e| std::mem::discriminant(&out.edges[e].curve))
                .collect::<Vec<_>>(),
            ef[es[0]],
            ef[es[1]]
        );
        for &e in es {
            println!(
                "      edge {e}: {:?} t {:?} v {:?}",
                out.edges[e].curve, out.edges[e].t, out.edges[e].v
            );
        }
    }
}

/// What a dumped body is made of (CADRS_REPRO=<file.body>): each face's surface, box, B-spline
/// size and loops, and each edge's curve and ends.
#[test]
fn debug_describe_body() {
    let Ok(path) = std::env::var("CADRS_REPRO") else {
        return;
    };
    let b = cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap();
    eprintln!(
        "{} faces, {} edges, {} vertices, {} solids; check {:?}",
        b.faces.len(),
        b.edges.len(),
        b.verts.len(),
        b.solids.len(),
        b.check().into_iter().take(3).collect::<Vec<_>>()
    );
    for (fi, f) in b.faces.iter().enumerate() {
        let bb = b.face_bbox(fi);
        let kind = match &f.surf {
            Surface::Nurbs(n) => format!(
                "Nurbs {}x{} degrees, {}x{} poles",
                n.du,
                n.dv,
                n.cps.len(),
                n.cps.first().map_or(0, |r| r.len())
            ),
            s => format!("{s:?}").chars().take(60).collect(),
        };
        let loops: Vec<Vec<usize>> = f
            .loops
            .iter()
            .map(|l| l.iter().map(|c| c.edge).collect())
            .collect();
        eprintln!(
            "face {fi}: {kind}; box [{:.3}, {:.3}, {:.3}]..[{:.3}, {:.3}, {:.3}]; sense {}; edges {loops:?}",
            bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z, f.sense
        );
    }
    for (ei, e) in b.edges.iter().enumerate() {
        let (p, q) = (b.verts[e.v[0]].p, b.verts[e.v[1]].p);
        let kind: String = match &e.curve {
            Curve::Nurbs(n) => format!("Nurbs degree {}, {} poles", n.degree, n.cps.len()),
            c => format!("{c:?}").chars().take(30).collect(),
        };
        eprintln!(
            "edge {ei}: {kind}… from [{:.4}, {:.4}, {:.4}] to [{:.4}, {:.4}, {:.4}]",
            p.x, p.y, p.z, q.x, q.y, q.z
        );
    }
}

/// The swung cases of brep-bench (`CADRS_SWUNG=ball|cone`): the exact sweep's error, if any.
#[test]
fn debug_swung() {
    let Some(which) = std::env::var_os("CADRS_SWUNG") else {
        return;
    };
    let mut k = NativeKernel::new();
    let z = Axis {
        origin: Point3::origin(),
        dir: nalgebra::Unit::new_normalize(Vector3::z()),
    };
    let (tool, deg) = match which.to_str().unwrap() {
        "ball" => (
            k.primitive(&Primitive::Sphere {
                center: Point3::new(1.0, 0.0, 0.0),
                radius: 2.0,
                axis: nalgebra::Unit::new_normalize(Vector3::z()),
                seam: nalgebra::Unit::new_normalize(Vector3::x()),
            })
            .unwrap()
            .bodies[0],
            120.0f64,
        ),
        _ => {
            let w = Vector3::new(30f64.to_radians().cos(), 0.0, 30f64.to_radians().sin());
            (
                k.primitive(&Primitive::Cone {
                    base: Point3::from(w * -2.0),
                    axis: nalgebra::Unit::new_normalize(w),
                    base_radius: 0.0,
                    top_radius: 7.0 * 25f64.to_radians().tan(),
                    height: 7.0,
                })
                .unwrap()
                .bodies[0],
                150.0,
            )
        }
    };
    let b = (**k.get(tool).unwrap()).clone();
    let steps = vec![RigidStep::Turn {
        axis: z,
        from: 0.0,
        to: deg.to_radians(),
    }];
    let t = std::time::Instant::now();
    match cadrs_kernel::native::sweep_faces::by_faces(&mut k, &b, &steps) {
        Ok(r) => {
            let m = k.mass_properties(r.bodies[0]).unwrap();
            eprintln!(
                "exact: volume {} area {} faces {} in {:?}",
                m.volume,
                m.surface_area,
                k.get(r.bodies[0]).unwrap().faces.len(),
                t.elapsed()
            );
        }
        Err(e) => eprintln!("exact failed: {e} in {:?}", t.elapsed()),
    }
}

/// brep-bench's bar end turned into a floor (`CADRS_BAR_END=1`): the cut's faces and volume.
#[test]
fn debug_bar_end() {
    if std::env::var_os("CADRS_BAR_END").is_none() {
        return;
    }
    let mut k = NativeKernel::new();
    let block = k
        .cylindrical_box(&CylindricalBox {
            frame: Plane {
                origin: Point3::new(0.0, 0.0, -1.0),
                x_dir: Vector3::x_axis(),
                normal: Vector3::z_axis(),
            },
            length: 2.0,
            inner_radius: 8.0,
            outer_radius: 12.0,
            start_angle: 0.0,
            end_angle: 60f64.to_radians(),
        })
        .unwrap()
        .bodies[0];
    let t = 20f64.to_radians();
    let (u, n) = (
        Vector3::new(t.cos(), 0.0, -t.sin()),
        Vector3::new(t.sin(), 0.0, t.cos()),
    );
    let on = Plane {
        origin: Point3::new(10.0, 0.0, 0.0),
        x_dir: nalgebra::Unit::new_normalize(u),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let f = 5f64.to_radians();
    let c = |a: f64, b: f64| p(a * f.cos() - b * f.sin(), a * f.sin() + b * f.cos());
    let pts = [c(0.0, -2.0), c(3.0, -2.0), c(3.0, 2.0), c(0.0, 2.0)];
    let curves = (0..4)
        .map(|i| Curve2::Line {
            a: pts[i],
            b: pts[(i + 1) % 4],
            source: None,
        })
        .collect();
    let bar = k
        .extrude(
            &Profile::new(on, vec![reg(Loop { curves })]),
            Extent::Blind(3.0),
        )
        .unwrap()
        .bodies[0];
    let motion = SolidMotion::Rigid {
        steps: vec![RigidStep::Turn {
            axis: Axis {
                origin: Point3::origin(),
                dir: Vector3::z_axis(),
            },
            from: (-140f64).to_radians(),
            to: 140f64.to_radians(),
        }],
    };
    // The ends keep clear of the block: the whole turn's solid of revolution cuts the same.
    let rev = k.sweep_solid(
        bar,
        &SolidMotion::Turn {
            axis: Axis {
                origin: Point3::origin(),
                dir: Vector3::z_axis(),
            },
        },
    );
    if let Err(e) = &rev {
        eprintln!("revolved failed: {e}");
    }
    if let Ok(rev) = rev {
        match k.boolean(BoolOp::Subtract, block, &[rev.bodies[0]]) {
            Ok(c) => {
                let m = k.mass_properties(c.bodies[0]).unwrap();
                eprintln!("revolved: volume {} area {}", m.volume, m.surface_area);
            }
            Err(e) => eprintln!("revolved cut failed: {e}"),
        }
    }
    let t0 = std::time::Instant::now();
    match k.cut_swept(block, bar, &motion) {
        Ok(r) => {
            let b = k.get(r.bodies[0]).unwrap();
            let m = k.mass_properties(r.bodies[0]).unwrap();
            eprintln!(
                "cut: volume {} area {} faces {} in {:?}",
                m.volume,
                m.surface_area,
                b.faces.len(),
                t0.elapsed()
            );
        }
        Err(e) => eprintln!("cut failed: {e}"),
    }
}

/// brep-bench's creased bar turned through a ring sector (`CADRS_CREASED=1`): the swept solid
/// cut from the block, and the same by `cut_swept`.
#[test]
fn debug_creased_bar() {
    if std::env::var_os("CADRS_CREASED").is_none() {
        return;
    }
    let mut k = NativeKernel::new();
    let block = k
        .cylindrical_box(&CylindricalBox {
            frame: Plane {
                origin: Point3::new(0.0, 0.0, -1.0),
                x_dir: Vector3::x_axis(),
                normal: Vector3::z_axis(),
            },
            length: 2.0,
            inner_radius: 8.0,
            outer_radius: 12.0,
            start_angle: 0.0,
            end_angle: 60f64.to_radians(),
        })
        .unwrap()
        .bodies[0];
    let t = 20f64.to_radians();
    let (u, n) = (Vector3::x(), Vector3::new(0.0, t.sin(), t.cos()));
    let at = |h: f64| Plane {
        origin: Point3::from(n * h),
        x_dir: nalgebra::Unit::new_normalize(u),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let d = 4.0 * 2f64.to_radians().tan();
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let pts = [p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)];
        Loop {
            curves: (0..4)
                .map(|i| Curve2::Line {
                    a: pts[i],
                    b: pts[(i + 1) % 4],
                    source: None,
                })
                .collect(),
        }
    };
    let section = |h: f64, d: f64| {
        LoftSection::Profile(Profile::new(
            at(h),
            vec![reg(rect(10.0 + d, -2.0 + d, 13.0 - d, 2.0 - d))],
        ))
    };
    let loft = |k: &mut NativeKernel, a: LoftSection, b: LoftSection| {
        k.loft_with(&LoftSpec {
            body: BodyKind::Solid,
            sections: vec![a, b],
            start: LoftEnd::default(),
            end: LoftEnd::default(),
            source: 1,
        })
        .unwrap()
        .bodies[0]
    };
    let up = loft(&mut k, section(0.0, 0.0), section(4.0, d));
    let down = loft(&mut k, section(-4.0, d), section(0.0, 0.0));
    let bar = k.boolean(BoolOp::Union, up, &[down]).unwrap().bodies[0];
    let motion = SolidMotion::Rigid {
        steps: vec![RigidStep::Turn {
            axis: Axis {
                origin: Point3::origin(),
                dir: Vector3::z_axis(),
            },
            from: (-140f64).to_radians(),
            to: 140f64.to_radians(),
        }],
    };
    let report = |k: &NativeKernel, what: &str, id: BodyId, t0: std::time::Instant| {
        let b = k.get(id).unwrap();
        let m = k.mass_properties(id).unwrap();
        eprintln!(
            "{what}: volume {} area {} faces {} in {:?}",
            m.volume,
            m.surface_area,
            b.faces.len(),
            t0.elapsed()
        );
    };
    let t0 = std::time::Instant::now();
    match k.sweep_solid(bar, &motion) {
        Ok(s) => {
            report(&k, "swept", s.bodies[0], t0);
            match k.boolean(BoolOp::Subtract, block, &[s.bodies[0]]) {
                Ok(c) => report(&k, "swept, cut", c.bodies[0], t0),
                Err(e) => eprintln!("swept, cut failed: {e}"),
            }
        }
        Err(e) => eprintln!("swept failed: {e}"),
    }
    let t0 = std::time::Instant::now();
    match k.cut_swept(block, bar, &motion) {
        Ok(r) => report(&k, "cut_swept", r.bodies[0], t0),
        Err(e) => eprintln!("cut_swept failed: {e}"),
    }
}

/// brep-bench's tools carried along a spline (`CADRS_CARRIED=key|ring|nose`): the groove's cut.
#[test]
fn debug_carried() {
    let Some(which) = std::env::var_os("CADRS_CARRIED") else {
        return;
    };
    let mut k = NativeKernel::new();
    let boxed = |k: &mut NativeKernel, a: Point3<f64>, b: Point3<f64>| {
        let pts = [p(a.x, a.y), p(b.x, a.y), p(b.x, b.y), p(a.x, b.y)];
        let curves = (0..4)
            .map(|i| Curve2::Line {
                a: pts[i],
                b: pts[(i + 1) % 4],
                source: None,
            })
            .collect();
        let top = Plane {
            origin: Point3::new(0.0, 0.0, a.z),
            x_dir: Vector3::x_axis(),
            normal: Vector3::z_axis(),
        };
        k.extrude(
            &Profile::new(top, vec![reg(Loop { curves })]),
            Extent::Blind(b.z - a.z),
        )
        .unwrap()
        .bodies[0]
    };
    let block = boxed(
        &mut k,
        Point3::new(-6.0, -6.0, -8.0),
        Point3::new(36.0, 26.0, 0.0),
    );
    let tool = match which.to_str().unwrap() {
        "key" => boxed(
            &mut k,
            Point3::new(-1.5, -0.8, -1.5),
            Point3::new(1.5, 0.8, 2.5),
        ),
        _ => {
            k.primitive(&Primitive::Torus {
                center: Point3::new(0.0, 0.0, 0.5),
                axis: Vector3::y_axis(),
                major: 1.5,
                minor: 0.5,
            })
            .unwrap()
            .bodies[0]
        }
    };
    let path = vec![PathCurve::Bezier3 {
        poles: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(20.0, 20.0, 0.0),
            Point3::new(30.0, 20.0, 0.0),
        ],
        weights: vec![],
    }];
    let motion = SolidMotion::Path {
        path,
        up: Vector3::z_axis(),
    };
    let t0 = std::time::Instant::now();
    match k.cut_swept(block, tool, &motion) {
        Ok(r) => {
            let b = k.get(r.bodies[0]).unwrap();
            let m = k.mass_properties(r.bodies[0]).unwrap();
            eprintln!(
                "cut: volume {} area {} faces {} in {:?}",
                m.volume,
                m.surface_area,
                b.faces.len(),
                t0.elapsed()
            );
        }
        Err(e) => eprintln!("cut failed: {e}"),
    }
}

/// brep-bench's tumbling cube (`CADRS_TUMBLE=1`): the moving-contact sweep's result, checked in
/// full (of the cube in `CADRS_TUMBLE_BODY` when given, a dump; left in `CADRS_TUMBLE_OUT`), then
/// the kernel's sweep (unless `CADRS_TUMBLE_ONLY`).
#[test]
fn debug_tumbling_cube() {
    if std::env::var_os("CADRS_TUMBLE").is_none() {
        return;
    }
    let mut k = NativeKernel::new();
    let pts = [p(2.4, -0.6), p(3.6, -0.6), p(3.6, 0.6), p(2.4, 0.6)];
    let curves = (0..4)
        .map(|i| Curve2::Line {
            a: pts[i],
            b: pts[(i + 1) % 4],
            source: None,
        })
        .collect();
    let plane = Plane {
        origin: Point3::new(0.0, 0.0, 1.4),
        x_dir: Vector3::x_axis(),
        normal: Vector3::z_axis(),
    };
    let cube = k
        .extrude(
            &Profile::new(plane, vec![reg(Loop { curves })]),
            Extent::Blind(1.2),
        )
        .unwrap()
        .bodies[0];
    let t = std::env::var("CADRS_TUMBLE_DEG")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(75.0)
        .to_radians();
    let steps = vec![
        RigidStep::Turn {
            axis: Axis {
                origin: Point3::new(2.0, 0.0, 0.0),
                dir: Vector3::z_axis(),
            },
            from: -t / 4.0,
            to: t / 4.0,
        },
        RigidStep::Turn {
            axis: Axis {
                origin: Point3::new(0.0, 0.5, 0.5),
                dir: Vector3::x_axis(),
            },
            from: t,
            to: -t,
        },
    ];
    let t0 = std::time::Instant::now();
    let body = match std::env::var("CADRS_TUMBLE_BODY") {
        Ok(path) => cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap(),
        Err(_) => (**k.get(cube).unwrap()).clone(),
    };
    let place = |t: f64| {
        let (mut r, mut d) = (nalgebra::Rotation3::identity(), Vector3::zeros());
        for s in &steps {
            if let RigidStep::Turn { axis, from, to } = *s {
                let a = from + (to - from) * t;
                let rr = nalgebra::Rotation3::from_axis_angle(&axis.dir, a);
                let o = axis.origin.coords;
                d = rr * d + o - rr * o;
                r = rr * r;
            }
        }
        (r, d)
    };
    let direct = cadrs_kernel::native::sweep_moving::polyhedron(&mut k, &body, &place);
    eprintln!(
        "direct: {:?} in {:?}",
        direct
            .as_ref()
            .map(|r| r.as_ref().map(|_| "ok").map_err(|e| e.to_string())),
        t0.elapsed()
    );
    if let Some(Ok(r)) = &direct {
        let id = r.bodies[0];
        let b = k.get(id).unwrap();
        let m = k.mass_properties(id).unwrap();
        let t1 = std::time::Instant::now();
        let full = b.check_with(cadrs_kernel::native::topo::CheckLevel::Full);
        eprintln!(
            "direct: volume {} area {} faces {} class {:?}; full check ({:?}): {:?}",
            m.volume,
            m.surface_area,
            b.faces.len(),
            k.body_class(id),
            t1.elapsed(),
            full
        );
        // Points at random in its box, in or out by the kernel's classification and exactly
        // (some instant puts them in the cube), compared.
        if let Some(n) = std::env::var("CADRS_TUMBLE_MC")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            let cb = &body;
            let planes: Vec<(Vector3<f64>, f64)> = cb
                .faces
                .iter()
                .map(|f| {
                    let cadrs_kernel::native::surface::Surface::Plane { f: fr } = &f.surf else {
                        unreachable!()
                    };
                    let nn = fr.z * if f.sense { 1.0 } else { -1.0 };
                    (nn, nn.dot(&fr.o.coords))
                })
                .collect();
            let oracle = cadrs_kernel::native::sweep_moving::Oracle::new(planes, &place, 4096);
            let polys: Vec<Vec<Vec<Point2<f64>>>> = b
                .faces
                .iter()
                .map(|f| cadrs_kernel::native::classify::face_polygons(b, f))
                .collect();
            let classifier = cadrs_kernel::native::classify::Classifier::new(b, &polys);
            let bb = b.bbox();
            let mut seed: u64 = 0x9e3779b97f4a7c15;
            let mut rnd = || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 11) as f64 / (1u64 << 53) as f64
            };
            let (mut inside, mut differ, mut near) = (0usize, 0usize, 0usize);
            let span = bb.max - bb.min;
            for _ in 0..n {
                let x = Point3::new(
                    bb.min.x + span.x * rnd(),
                    bb.min.y + span.y * rnd(),
                    bb.min.z + span.z * rnd(),
                );
                let d = oracle.depth(&x).0;
                if d.abs() < 1e-6 {
                    near += 1;
                    continue;
                }
                let exact = d < 0.0;
                let kernel = classifier.parity_inside(&x);
                inside += usize::from(exact);
                if exact != kernel {
                    differ += 1;
                    if differ <= 10 {
                        eprintln!("differ at {x:?}: depth {d:e}, kernel says inside {kernel}");
                    }
                }
            }
            let bv = span.x * span.y * span.z;
            eprintln!(
                "{n} points: {differ} classified differently, {near} too near to tell; exact \
                 volume estimate {} ± {}",
                inside as f64 / n as f64 * bv,
                bv * ((inside as f64 / n as f64) * (1.0 - inside as f64 / n as f64) / n as f64)
                    .sqrt()
            );
        }
        if let Ok(dir) = std::env::var("CADRS_TUMBLE_OUT") {
            std::fs::write(
                std::path::Path::new(&dir).join("result.body"),
                cadrs_kernel::native::bodyio::write(b),
            )
            .unwrap();
        }
    }
    if std::env::var_os("CADRS_TUMBLE_ONLY").is_some() {
        return;
    }
    let t0 = std::time::Instant::now();
    match k.sweep_solid(cube, &SolidMotion::Rigid { steps }) {
        Ok(r) => {
            let b = k.get(r.bodies[0]).unwrap();
            let m = k.mass_properties(r.bodies[0]).unwrap();
            eprintln!(
                "swept: volume {} area {} faces {} in {:?}",
                m.volume,
                m.surface_area,
                b.faces.len(),
                t0.elapsed()
            );
        }
        Err(e) => eprintln!("failed: {e}"),
    }
}

/// Each face of the body in `CADRS_BODY` (a native body file): its surface, area, centroid,
/// outward normal there and edges.
/// `CADRS_STEP=<file>`: the solid in a STEP file imported and measured (volume, area, faces by
/// surface kind, soundness), to compare another kernel's result with ours.
#[test]
fn debug_step_measure() {
    let Ok(path) = std::env::var("CADRS_STEP") else {
        return;
    };
    let mut k = NativeKernel::new();
    let t0 = std::time::Instant::now();
    let ids = k.import_step(&std::fs::read(path).unwrap()).unwrap();
    eprintln!("imported {} bodies in {:?}", ids.len(), t0.elapsed());
    for id in ids {
        let m = k.mass_properties(id).unwrap();
        let b = k.get(id).unwrap();
        let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
        for f in &b.faces {
            let kind = match &f.surf {
                cadrs_kernel::native::surface::Surface::Plane { .. } => "plane",
                cadrs_kernel::native::surface::Surface::Torus { .. } => "torus",
                cadrs_kernel::native::surface::Surface::Nurbs(_) => "bspline",
                _ => "other",
            };
            *kinds.entry(kind).or_default() += 1;
        }
        eprintln!(
            "volume {} area {} faces {:?} edges {} class {:?} problems {:?}",
            m.volume,
            m.surface_area,
            kinds,
            b.edges.len(),
            k.body_class(id),
            b.check().first()
        );
    }
}

/// `CADRS_BODY=<file> CADRS_POINT=x,y,z`: the edges of the body nearest the point, with their
/// faces, lengths, and how their curves bend there.
#[test]
fn debug_edges_near() {
    let (Ok(path), Ok(pt)) = (std::env::var("CADRS_BODY"), std::env::var("CADRS_POINT")) else {
        return;
    };
    let b = cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap();
    let c: Vec<f64> = pt.split(',').map(|v| v.trim().parse().unwrap()).collect();
    let p = Point3::new(c[0], c[1], c[2]);
    let ef = b.edge_faces();
    let mut near: Vec<(f64, usize, f64)> = b
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.degenerate)
        .map(|(i, e)| {
            let t = e.curve.closest(&p, e.t[0], e.t[1]);
            ((e.curve.point(t) - p).norm(), i, t)
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d, i, t) in near.into_iter().take(6) {
        let e = &b.edges[i];
        let (_, d1, d2) = e.curve.d2(t);
        let knots = match &e.curve {
            cadrs_kernel::native::curve::Curve::Nurbs(n) => n.knots.len(),
            _ => 0,
        };
        eprintln!(
            "edge {i} of faces {:?}: {d:.3e} away at t {t:.6} of {:?} (tol {:.1e}, {knots} knots); speed {:.3e}, bend {:.3e}; ends {:?} {:?}",
            ef[i],
            e.t,
            e.tol,
            d1.norm(),
            d2.norm() / d1.norm_squared().max(1e-300),
            b.verts[e.v[0]].p,
            b.verts[e.v[1]].p
        );
    }
}

#[test]
fn debug_body_faces() {
    let Ok(path) = std::env::var("CADRS_BODY") else {
        return;
    };
    let b = cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap();
    eprintln!(
        "volume {} faces {} edges {} solids {} shells {:?}",
        cadrs_kernel::native::mass::body_volume(&b),
        b.faces.len(),
        b.edges.len(),
        b.solids.len(),
        b.shells.iter().map(|s| s.faces.len()).collect::<Vec<_>>()
    );
    for fi in 0..b.faces.len() {
        let (a, c) = cadrs_kernel::native::mass::face_area(&b, fi);
        let f = &b.faces[fi];
        let (u, v) = f.surf.project(&c, None);
        let n = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
        let es: Vec<(usize, bool)> = f.loops.iter().flatten().map(|c| (c.edge, c.fwd)).collect();
        eprintln!(
            "face {fi} {:?} area {a:.6} centroid {:.5?} normal {:.4?} edges {es:?}",
            f.surf.kind(),
            c,
            n
        );
    }
    if std::env::var_os("CADRS_BODY_EDGES").is_some() {
        for (ei, e) in b.edges.iter().enumerate() {
            eprintln!(
                "edge {ei} {} {:.6?} -> {:.6?} tol {:e}",
                if matches!(e.curve, cadrs_kernel::native::curve::Curve::Line { .. }) {
                    "line"
                } else {
                    "curve"
                },
                b.verts[e.v[0]].p,
                b.verts[e.v[1]].p,
                e.tol
            );
        }
    }
}

/// The body in `CADRS_BODY` meshed as brep-bench's runner meshes it: the triangles' edges not
/// matched by one the other way (points rounded to single precision).
#[test]
fn debug_mesh_closed() {
    let Ok(path) = std::env::var("CADRS_BODY") else {
        return;
    };
    let mut k = NativeKernel::new();
    let b = k.insert(cadrs_kernel::native::bodyio::read(&std::fs::read(path).unwrap()).unwrap());
    let t0 = std::time::Instant::now();
    let m = k.mass_properties(b).unwrap();
    eprintln!("mass properties in {:?}: {m:?}", t0.elapsed());
    let t0 = std::time::Instant::now();
    let class = k.body_class(b);
    eprintln!("class {class:?} in {:?}", t0.elapsed());
    let size = k.bounding_box(b).unwrap().diagonal();
    let thickness = m.volume / m.surface_area;
    let deflection = (1e-3 * size).min(5e-2 * thickness).max(1e-10);
    let t0 = std::time::Instant::now();
    let mesh = k
        .tessellate(
            b,
            Tessellation {
                deflection,
                angle: 15f64.to_radians(),
            },
        )
        .unwrap();
    eprintln!("{} triangles in {:?}", mesh.indices.len(), t0.elapsed());
    let key = |i: u32| {
        let p = mesh.positions[i as usize];
        [p.x as f32, p.y as f32, p.z as f32].map(f32::to_bits)
    };
    let mut edges: std::collections::HashMap<([u32; 3], [u32; 3]), i64> = Default::default();
    let mut degenerate = 0;
    for t in &mesh.indices {
        for (a, z) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, z) = (key(a), key(z));
            if a == z {
                degenerate += 1;
                continue;
            }
            *edges.entry((a, z)).or_default() += 1;
        }
    }
    let f = |k: [u32; 3]| k.map(f32::from_bits);
    for ((a, z), n) in &edges {
        if edges.get(&(*z, *a)).copied().unwrap_or(0) != *n {
            eprintln!("unmatched {:?} -> {:?} ({n})", f(*a), f(*z));
            for (ti, t) in mesh.indices.iter().enumerate() {
                let ks: Vec<[u32; 3]> = t.iter().map(|&i| key(i)).collect();
                if ks.contains(a) && ks.contains(z) {
                    eprintln!(
                        "  triangle of face {:?}: {:?}",
                        mesh.triangle_faces[ti],
                        t.iter()
                            .map(|&i| mesh.positions[i as usize])
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    eprintln!("{degenerate} triangle edges of no length (in single precision)");
}
