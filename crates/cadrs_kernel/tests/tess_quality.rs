//! Tessellation quality (`native::tess`): watertight meshes of closed bodies (one discretisation
//! per edge shared by both faces, seams and poles included), the chord bound measured against the
//! exact surfaces, the maximum edge length, the controls' validation, determinism across thread
//! counts, and every SMLib model in `tests/data` meshed without a fold (timed: the largest are
//! reported on stderr).

mod smlib_common;

use std::f64::consts::{PI, TAU};
use std::path::Path;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::native::math::P3;
use cadrs_kernel::native::smb;
use cadrs_kernel::native::surface::Surface;
use cadrs_kernel::native::tess::{self, FaceMesh, TessParams};
use cadrs_kernel::native::topo::Body;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3};
use rayon::prelude::*;

fn p(x: f64, y: f64) -> Point2<f64> {
    Point2::new(x, y)
}

fn line(a: Point2<f64>, b: Point2<f64>) -> Curve2 {
    Curve2::Line { a, b, source: None }
}

fn region(curves: Vec<Curve2>) -> Region {
    Region {
        outer: Loop { curves },
        holes: vec![],
        source: None,
    }
}

/// The XZ plane with in-plane y along +Z, for revolve profiles.
fn front() -> Plane {
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

fn revolve(k: &mut NativeKernel, curves: Vec<Curve2>) -> BodyId {
    k.revolve(&Profile::new(front(), vec![region(curves)]), z_axis(), TAU)
        .unwrap()
        .bodies[0]
}

/// Bodies with seams, poles, an apex, a torus and blends: (name, body, its exact volume).
fn shapes(k: &mut NativeKernel) -> Vec<(&'static str, BodyId, f64)> {
    let sphere = revolve(
        k,
        vec![
            Curve2::Arc {
                center: p(0.0, 0.0),
                radius: 5.0,
                start_angle: -PI / 2.0,
                sweep: PI,
                source: None,
            },
            line(p(0.0, 5.0), p(0.0, -5.0)),
        ],
    );
    let cone = revolve(
        k,
        vec![
            line(p(0.0, 0.0), p(4.0, 0.0)),
            line(p(4.0, 0.0), p(0.0, 9.0)),
            line(p(0.0, 9.0), p(0.0, 0.0)),
        ],
    );
    let torus = k
        .revolve(
            &Profile::new(
                front(),
                vec![Region {
                    outer: Loop {
                        curves: vec![Curve2::Circle {
                            center: p(10.0, 0.0),
                            radius: 3.0,
                            source: None,
                        }],
                    },
                    holes: vec![],
                    source: None,
                }],
            ),
            z_axis(),
            TAU,
        )
        .unwrap()
        .bodies[0];
    let cyl = k
        .extrude(
            &Profile::new(
                Plane::top(),
                vec![Region {
                    outer: Loop {
                        curves: vec![Curve2::Circle {
                            center: p(0.0, 0.0),
                            radius: 6.0,
                            source: None,
                        }],
                    },
                    holes: vec![],
                    source: None,
                }],
            ),
            Extent::Blind(10.0),
        )
        .unwrap()
        .bodies[0];
    let bx = k
        .extrude(
            &Profile::new(
                Plane::top(),
                vec![region(vec![
                    line(p(0.0, 0.0), p(20.0, 0.0)),
                    line(p(20.0, 0.0), p(20.0, 10.0)),
                    line(p(20.0, 10.0), p(0.0, 10.0)),
                    line(p(0.0, 10.0), p(0.0, 0.0)),
                ])],
            ),
            Extent::Blind(8.0),
        )
        .unwrap()
        .bodies[0];
    let vertical: Vec<EdgeId> = k
        .edges(bx)
        .unwrap()
        .into_iter()
        .filter(|e| (e.end - e.start).z.abs() > 7.9)
        .map(|e| e.id)
        .collect();
    let filleted = k.fillet(bx, &vertical, 3.0).unwrap().bodies[0];
    let fillet_volume = 20.0 * 10.0 * 8.0 - 4.0 * (9.0 - PI * 9.0 / 4.0) * 8.0;
    vec![
        ("sphere", sphere, 4.0 / 3.0 * PI * 125.0),
        ("cone", cone, PI * 16.0 * 9.0 / 3.0),
        ("torus", torus, 2.0 * PI * PI * 10.0 * 9.0),
        ("cylinder", cyl, PI * 36.0 * 10.0),
        ("filleted box", filleted, fillet_volume),
    ]
}

/// The largest distance from a triangle's centroid or side middle to its face's surface, by
/// projection from each corner's (u, v) (the mesher's), the nearest; exact where the surface has
/// an implicit form.
fn deviation(body: &Body, faces: &[FaceMesh]) -> f64 {
    fn dist(s: &Surface, q: &P3, hint: (f64, f64)) -> f64 {
        if let Some(d) = s.signed_distance(q) {
            return d.abs();
        }
        let ((u0, u1), (v0, v1)) = s.param_box();
        let mut starts = vec![hint];
        let mut best = f64::INFINITY;
        let mut k = 0;
        while k < starts.len() && k < 8 {
            let (u, v) = s.project(q, Some(starts[k]));
            best = best.min((s.eval(u, v) - q).norm());
            if u <= u0 || u >= u1 {
                starts.push((if u <= u0 { u1 } else { u0 }, v));
            }
            if v <= v0 || v >= v1 {
                starts.push((u, if v <= v0 { v1 } else { v0 }));
            }
            k += 1;
        }
        best
    }
    faces
        .par_iter()
        .enumerate()
        .map(|(fi, m)| {
            let s = &body.faces[fi].surf;
            let mut worst = 0.0f64;
            for t in &m.indices {
                let [a, b, c] = t.map(|i| m.positions[i as usize]);
                for q in [
                    P3::from((a.coords + b.coords + c.coords) / 3.0),
                    nalgebra::center(&a, &b),
                    nalgebra::center(&b, &c),
                    nalgebra::center(&c, &a),
                ] {
                    worst = worst.max(
                        t.map(|i| dist(s, &q, (m.uvs[i as usize].x, m.uvs[i as usize].y)))
                            .into_iter()
                            .fold(f64::INFINITY, f64::min),
                    );
                }
            }
            worst
        })
        .reduce(|| 0.0, f64::max)
}

fn volume(faces: &[FaceMesh]) -> (f64, f64) {
    let (mut v, mut a) = (0.0, 0.0);
    for m in faces {
        for t in &m.indices {
            let [p0, p1, p2] = t.map(|i| m.positions[i as usize]);
            v += p0.coords.dot(&p1.coords.cross(&p2.coords)) / 6.0;
            a += (p1 - p0).cross(&(p2 - p0)).norm() / 2.0;
        }
    }
    (v, a)
}

fn longest_side(faces: &[FaceMesh]) -> f64 {
    faces
        .iter()
        .flat_map(|m| m.indices.iter().map(|t| t.map(|i| m.positions[i as usize])))
        .map(|[a, b, c]| (b - a).norm().max((c - b).norm()).max((a - c).norm()))
        .fold(0.0, f64::max)
}

#[test]
fn shapes_are_watertight_and_within_the_chord() {
    let mut k = NativeKernel::new();
    for (name, id, exact) in shapes(&mut k) {
        let body = k.get(id).unwrap().clone();
        for params in [
            TessParams::new(0.01, 0.3).unwrap(),
            TessParams::new(0.2, 1.0).unwrap(),
            TessParams::new(0.05, 0.5)
                .unwrap()
                .with_limits(1.5, 4.0)
                .unwrap(),
        ] {
            let (faces, _) = tess::mesh(&body, &params).unwrap();
            assert!(
                faces.iter().all(|m| !m.indices.is_empty()),
                "{name}: a face without triangles"
            );
            let check = tess::check_faces(&faces);
            assert!(check.is_watertight(), "{name} {params:?}: {check:?}");
            let dev = deviation(&body, &faces);
            assert!(
                dev <= params.chord * (1.0 + 1e-6),
                "{name} {params:?}: {dev} off the surfaces"
            );
            if params.max_edge > 0.0 {
                let l = longest_side(&faces);
                assert!(
                    l <= params.max_edge * (1.0 + 1e-9),
                    "{name} {params:?}: a side of {l}"
                );
            }
            // A closed mesh within the chord of the surfaces: its volume within chord × area.
            let (v, a) = volume(&faces);
            assert!(
                (v - exact).abs() <= params.chord * a,
                "{name} {params:?}: volume {v}, exact {exact}"
            );
            // Through the kernel too.
            let m = k.tessellate_with(id, &params).unwrap();
            assert!(
                tess::check_tri_mesh(&m).is_watertight(),
                "{name}: kernel mesh"
            );
        }
    }
}

/// A solid loft from a square to a diamond turned an eighth of a turn (brep-bench's
/// twisted-duct): each side a twisted ruled face between straight edges, which their curves alone
/// sample at their ends. The faces' triangles then needed points along the edges, and put them
/// a hair off them each on its own (closed in f64, not in an STL file's single precision): edges
/// are now also sampled by how far the faces' normals turn along them.
#[test]
fn straight_edges_of_twisted_faces_mesh_closed() {
    let on = |y: f64| Plane {
        origin: Point3::new(0.0, y, 0.0),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let quad = |c: [(f64, f64); 4]| Region {
        outer: Loop {
            curves: (0..4)
                .map(|i| line(p(c[i].0, c[i].1), p(c[(i + 1) % 4].0, c[(i + 1) % 4].1)))
                .collect(),
        },
        holes: vec![],
        source: None,
    };
    let square = Profile::new(
        on(0.0),
        vec![quad([
            (10.0, -10.0),
            (-10.0, -10.0),
            (-10.0, 10.0),
            (10.0, 10.0),
        ])],
    );
    let diamond = Profile::new(
        on(40.0),
        vec![quad([(0.0, -12.0), (-12.0, 0.0), (0.0, 12.0), (12.0, 0.0)])],
    );
    let mut k = NativeKernel::new();
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections: vec![LoftSection::Profile(square), LoftSection::Profile(diamond)],
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 1,
    };
    let duct = k.loft_with(&spec).unwrap().bodies[0];
    for deflection in [5.1e-3, 0.05] {
        let quality = Tessellation {
            deflection,
            angle: 5f64.to_radians(),
        };
        // As an STL file keeps it (single precision): the points the faces put a hair off the
        // edges on their own, closed in f64, fall onto each other there.
        let m = k.tessellate(duct, quality).unwrap();
        let f32 =
            |q: &Point3<f64>| Point3::new(q.x as f32 as f64, q.y as f32 as f64, q.z as f32 as f64);
        let check = tess::check_triangles(
            m.indices
                .iter()
                .map(|t| t.map(|i| f32(&m.positions[i as usize])))
                .filter(|[a, b, c]| a != b && b != c && c != a),
        );
        assert!(check.is_watertight(), "deflection {deflection}: {check:?}");
    }
}

/// A ball of radius 10 less a cylinder of radius 5 whose wall runs through its centre
/// (brep-bench's viviani-cut): the sphere and the wall are tangent at (10, 0, 0), where Viviani's
/// curve crosses itself and both faces pinch to a point. Each face joined the same two points
/// near it by a side of its triangles, which four triangles then shared; such sides are now split.
#[test]
fn faces_pinched_to_a_point_share_no_side() {
    let mut k = NativeKernel::new();
    // Poles on the y axis, the seam through (−10, 0, 0): the half disc on the −x side of the y
    // axis on the plane z = 0, turned about y (as the case asks, both off Viviani's curve).
    let on = Plane {
        origin: Point3::origin(),
        x_dir: -Vector3::x_axis(),
        normal: -Vector3::z_axis(),
    };
    let half = Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc {
                    center: p(0.0, 0.0),
                    radius: 10.0,
                    start_angle: -PI / 2.0,
                    sweep: PI,
                    source: None,
                },
                line(p(0.0, 10.0), p(0.0, -10.0)),
            ],
        },
        holes: vec![],
        source: None,
    };
    let y = Axis {
        origin: Point3::origin(),
        dir: Vector3::y_axis(),
    };
    let ball = k
        .revolve(&Profile::new(on, vec![half]), y, TAU)
        .unwrap()
        .bodies[0];
    let base = Plane {
        origin: Point3::new(5.0, 0.0, -12.0),
        x_dir: -Vector3::x_axis(),
        normal: Vector3::z_axis(),
    };
    let circle = Region {
        outer: Loop {
            curves: vec![Curve2::Circle {
                center: p(0.0, 0.0),
                radius: 5.0,
                source: None,
            }],
        },
        holes: vec![],
        source: None,
    };
    let rod = k
        .extrude_with(
            &Profile::new(base, vec![circle]),
            &ExtrudeSpec::blind(Vector3::z_axis(), 24.0),
        )
        .unwrap()
        .bodies[0];
    let cut = k.boolean(BoolOp::Subtract, ball, &[rod]).unwrap().bodies[0];
    for chord in [3.4e-3, 1e-3] {
        let params = TessParams::new(chord, 5f64.to_radians()).unwrap();
        let m = k.tessellate_with(cut, &params).unwrap();
        let check = tess::check_tri_mesh(&m);
        assert!(check.is_watertight(), "chord {chord}: {check:?}");
    }
}

/// A ball's cap 10⁻⁴ high round its pole (brep-bench's pole-cap-a-ten-thousandth-high): a disc
/// 0.063 across under a dome, meshed at a chord of 2·10⁻⁷. Spaced by the sphere's radius each
/// way in (u, v), its triangles ran long round the pole and the mesh held 5 % too little.
#[test]
fn a_cap_round_a_pole_meshes_within_the_chord() {
    let (r, h) = (5.0f64, 1e-4f64);
    let z0 = r - h;
    let rim = (r * r - z0 * z0).sqrt();
    let mut k = NativeKernel::new();
    let id = revolve(
        &mut k,
        vec![
            Curve2::Arc {
                center: p(0.0, 0.0),
                radius: r,
                start_angle: PI / 2.0,
                sweep: z0.atan2(rim) - PI / 2.0,
                source: None,
            },
            line(p(rim, z0), p(0.0, z0)),
            line(p(0.0, z0), p(0.0, r)),
        ],
    );
    let body = k.get(id).unwrap().clone();
    let params = TessParams::new(2e-7, 5f64.to_radians()).unwrap();
    let (faces, _) = tess::mesh(&body, &params).unwrap();
    assert!(tess::check_faces(&faces).is_watertight());
    let dev = deviation(&body, &faces);
    assert!(dev <= params.chord * (1.0 + 1e-6), "{dev} off the surfaces");
    // A closed mesh within the chord of the surfaces: its volume within chord × area (0.8 %).
    let exact = PI * h * h * (3.0 * r - h) / 3.0;
    let (v, a) = volume(&faces);
    assert!(
        (v - exact).abs() <= params.chord * a,
        "volume {v}, exact {exact}"
    );
}

/// A 10 mm box shelled open at its front and top with 1 mm walls, its back face chamfered 1 mm
/// (issue 156): the chamfer eats the back wall's rim whole, so the top rim is left one face
/// pinched to no width along the chamfer, with two boundary edges along one line (the inner
/// back wall's, and the chamfer's running on past its ends). The triangulation splits the
/// chamfer's edge at the other's ends; the face's triangles still cover it, no more.
#[test]
fn a_face_pinched_to_no_width_along_a_line() {
    let mut k = NativeKernel::new();
    let square = region(vec![
        line(p(-5.0, -5.0), p(5.0, -5.0)),
        line(p(5.0, -5.0), p(5.0, 5.0)),
        line(p(5.0, 5.0), p(-5.0, 5.0)),
        line(p(-5.0, 5.0), p(-5.0, -5.0)),
    ]);
    let block = k
        .extrude(
            &Profile::new(Plane::top(), vec![square]),
            Extent::Blind(10.0),
        )
        .unwrap()
        .bodies[0];
    let open: Vec<FaceId> = k
        .faces(block)
        .unwrap()
        .into_iter()
        .filter(|f| (f.center.y + 5.0).abs() < 1e-9 || (f.center.z - 10.0).abs() < 1e-9)
        .map(|f| f.id)
        .collect();
    let spec = ShellSpec {
        remove: open,
        thickness: 1.0,
        outward: false,
        hollow: false,
        sharp: true,
    };
    let cup = k.shell_with(block, &spec).unwrap().bodies[0];
    let back = k
        .faces(cup)
        .unwrap()
        .into_iter()
        .find(|f| (f.center.y - 5.0).abs() < 1e-9)
        .unwrap()
        .id;
    let rim: Vec<EdgeId> = k
        .edges(cup)
        .unwrap()
        .into_iter()
        .filter(|e| e.faces.contains(&Some(back)))
        .map(|e| e.id)
        .collect();
    let opts = ChamferOpts::new(ChamferSpec::EqualDistance(1.0));
    let out = k.chamfer_with(cup, &rim, &opts).unwrap().bodies[0];
    let (meshes, _) = tess::mesh_body(k.get(out).unwrap(), 0.01, 0.2);
    for (f, m) in k.faces(out).unwrap().iter().zip(&meshes) {
        let area: f64 = m
            .indices
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| m.positions[i as usize]);
                0.5 * (b - a).cross(&(c - a)).norm()
            })
            .sum();
        assert!(
            (area - f.area).abs() < 1e-9 * f.area,
            "face {:?} at {:?}: triangles of {area} mm² for {} mm²",
            f.id,
            f.center,
            f.area
        );
    }
    let check = tess::check_faces(&meshes);
    assert!(check.is_watertight(), "{check:?}");
}

#[test]
fn controls_are_validated() {
    let mut k = NativeKernel::new();
    let (_, id, _) = shapes(&mut k)[0];
    for (d, a) in [
        (0.0, 0.1),
        (-0.1, 0.1),
        (f64::NAN, 0.1),
        (0.1, 0.0),
        (0.1, f64::INFINITY),
    ] {
        assert!(
            matches!(
                k.tessellate(
                    id,
                    Tessellation {
                        deflection: d,
                        angle: a
                    }
                ),
                Err(KernelError::InvalidInput(_))
            ),
            "{d} {a}"
        );
    }
    assert!(k.tessellate(id, Tessellation::default()).is_ok());
    let p = TessParams::new(0.1, 0.5).unwrap();
    assert!(p.with_limits(-1.0, 0.0).is_err() && p.with_limits(0.0, 1.0).is_err());
    assert!(matches!(
        k.tessellate_with(id, &TessParams { chord: 0.0, ..p }),
        Err(KernelError::InvalidInput(_))
    ));
}

fn read(path: &Path) -> Vec<Body> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    if path.extension().is_some_and(|e| e == "smp") {
        smb::read_part(&bytes)
            .map(|p| p.breps.into_iter().map(|b| b.body).collect())
            .unwrap_or_default()
    } else {
        smb::read_brep(&bytes)
            .map(|b| vec![b.body])
            .unwrap_or_default()
    }
}

/// Every non-degenerate edge used by two coedges, once each way (a seam by one face twice): a
/// closed, consistently oriented B-rep, whose mesh must be closed too.
fn closed(b: &Body) -> bool {
    let mut uses = vec![(0usize, 0usize); b.edges.len()];
    for c in b.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        if c.fwd {
            uses[c.edge].0 += 1;
        } else {
            uses[c.edge].1 += 1;
        }
    }
    !b.faces.is_empty()
        && b.edges
            .iter()
            .zip(&uses)
            .all(|(e, &n)| e.degenerate || n == (1, 1) || n == (0, 0))
}

#[test]
fn deterministic_across_thread_counts() {
    let body = smlib_common::model("tessellation", "nurb_fillet.smb")
        .unwrap()
        .body;
    let params = TessParams::new(0.05, 0.4)
        .unwrap()
        .with_limits(0.0, 4.0)
        .unwrap();
    let run = |n: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap()
            .install(|| tess::mesh(&body, &params).unwrap())
    };
    let one = run(1);
    assert_eq!(one, run(4));
    assert_eq!(one, run(4));
}

/// Closed bodies whose meshes may not be watertight, and why (a listed body that passes is
/// reported, not failed: the reader's fixes land separately).
const KNOWN: &[(&str, &str)] = &[
    // Not diagnosed yet: 10 sides of 38 600 open where two planes meet a cylinder strip at the
    // hub (HEAD: 27 open, 76 folded).
    ("fillets/blade_hub_3.smb#0", "open sides at the hub"),
    // Not diagnosed yet: one triangle's worth (3 sides of 897) open by a degenerate edge's
    // corner on a B-spline face (HEAD: 41 open, 21 folded).
    (
        "offset/OffsetBoxSphOnCorner.smb#0",
        "a triangle missing at a degenerate corner",
    ),
];

/// The SMLib models in `tests/data/smlib`, meshed at a thousandth of their size and 0.5 rad:
/// every face gets triangles, and every closed, consistently oriented body a watertight mesh.
#[test]
fn every_smlib_model() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/smlib");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
        .map(|f| f.path())
        .filter(|p| p.extension().is_some_and(|e| e == "smb" || e == "smp"))
        .collect();
    files.sort();
    let mut failures = Vec::new();
    let mut timings = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(&root).unwrap().display().to_string();
        // Read on the side: a reader stuck on a model is the reader's problem, not the mesher's.
        let (tx, rx) = std::sync::mpsc::channel();
        let file = path.clone();
        std::thread::spawn(move || tx.send(read(&file)));
        let Ok(bodies) = rx.recv_timeout(std::time::Duration::from_secs(60)) else {
            eprintln!("{rel}: not read in a minute, skipped");
            continue;
        };
        for (bi, body) in bodies.into_iter().enumerate() {
            // The size from the edges' samples (a body's box can be just its vertices').
            let pts: Vec<P3> = tess::edge_samples(&body, 1.0, 0.5)
                .into_iter()
                .flat_map(|(_, p)| p)
                .collect();
            let size = pts.iter().fold(
                nalgebra::Point3::from(nalgebra::Vector3::repeat(f64::INFINITY)),
                |a, q| a.inf(q),
            ) - pts.iter().fold(
                nalgebra::Point3::from(nalgebra::Vector3::repeat(f64::NEG_INFINITY)),
                |a, q| a.sup(q),
            );
            let size = size.norm();
            if body.faces.is_empty() || !(size > 0.0 && size.is_finite()) {
                continue;
            }
            let params = TessParams::new(1e-3 * size, 0.5).unwrap();
            let t = std::time::Instant::now();
            let (faces, _) = tess::mesh(&body, &params).unwrap();
            let elapsed = t.elapsed();
            let tris: usize = faces.iter().map(|m| m.indices.len()).sum();
            timings.push((
                elapsed,
                format!("{rel}#{bi}: {} faces, {tris} triangles", body.faces.len()),
            ));
            let check = tess::check_faces(&faces);
            let empty = faces.iter().filter(|m| m.indices.is_empty()).count();
            let name = format!("{rel}#{bi}");
            let failed = (closed(&body) && !check.is_watertight()) || empty > 0;
            let known = KNOWN.iter().any(|(k, _)| *k == name);
            if failed && !known {
                failures.push(format!(
                    "{name}: {check:?}, {empty} faces without triangles"
                ));
            } else if known && !failed {
                eprintln!(
                    "{name}: known failure passes now (take it off KNOWN once it does for good)"
                );
            }
        }
    }
    timings.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (t, what) in timings.iter().take(8) {
        eprintln!("{what} in {t:?}");
    }
    let total: std::time::Duration = timings.iter().map(|t| t.0).sum();
    eprintln!("{} bodies in {total:?}", timings.len());
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
