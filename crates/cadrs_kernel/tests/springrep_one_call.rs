//! Ported from springrep's tests/one_call.rs (MIT, https://github.com/springcraft-ai/springrep):
//! operations a caller once assembled from others, each as one call, against closed forms.
//! springrep's sweep "legs" (a line along the profile's normal, an arc about an axis, a turn on
//! the spot) are written out as cadrs's path curves; its sharp turn filled by the profile turned
//! about the corner is cadrs's round sweep corner. Assertions on springrep APIs cadrs has none
//! for are left out where they come, each with a comment.

#![allow(dead_code)]

use std::f64::consts::{FRAC_PI_2, PI};

use cadrs_kernel::native::{NativeKernel, tess};
use cadrs_kernel::*;
use nalgebra::{Matrix3, Point3, Vector3};

type K = NativeKernel;
type P3 = Point3<f64>;
type V3 = Vector3<f64>;

fn p3(x: f64, y: f64, z: f64) -> P3 {
    P3::new(x, y, z)
}

/// springrep's `Profile::rect(center, w, h)`: the `w` × `h` rectangle centred on (`cx`, `cy`).
fn rect(cx: f64, cy: f64, w: f64, h: f64) -> Loop {
    Loop::rectangle(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0)
}

/// springrep's `Profile::circle(center, diameter)`: it takes a diameter.
fn circle(cx: f64, cy: f64, diameter: f64) -> Loop {
    Loop::circle([cx, cy], diameter / 2.0)
}

/// `outer` less `holes` on the plane z = `z` (springrep's `Frame::new((0, 0, z), Z, X)`),
/// extruded `h` up z.
fn extrude(k: &mut K, z: f64, outer: Loop, holes: Vec<Loop>, h: f64) -> BodyId {
    let p = Profile::with_holes(Plane::xy(z), outer, holes);
    k.extrude(&p, Extent::Blind(h)).unwrap().bodies[0]
}

fn boolean(k: &mut K, op: BoolOp, a: BodyId, b: BodyId) -> BodyId {
    let r = k.boolean(op, a, &[b]).unwrap();
    assert_eq!(r.bodies.len(), 1, "{op:?} made {} bodies", r.bodies.len());
    r.bodies[0]
}

fn union(k: &mut K, a: BodyId, b: BodyId) -> BodyId {
    boolean(k, BoolOp::Union, a, b)
}

fn difference(k: &mut K, a: BodyId, b: BodyId) -> BodyId {
    boolean(k, BoolOp::Subtract, a, b)
}

/// springrep's `unify`: faces and edges on one surface or curve merged.
fn unify(k: &mut K, b: BodyId) -> BodyId {
    k.simplify(b).unwrap().bodies[0]
}

fn valid(k: &K, b: BodyId) {
    let issues = k.get(b).unwrap().check();
    assert!(issues.is_empty(), "check: {issues:?}");
}

fn volume(k: &K, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

fn area(k: &K, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().surface_area
}

/// The body's mesh (springrep's `MeshOptions { linear, angular }`) is closed.
fn watertight(k: &K, b: BodyId, linear: f64, angular: f64) -> bool {
    let q = Tessellation {
        deflection: linear,
        angle: angular,
    };
    tess::check_tri_mesh(&k.tessellate(b, q).unwrap()).is_watertight()
}

/// Each face with its outward normal at (the point of it nearest) its centre.
fn face_list(k: &K, b: BodyId) -> Vec<(FaceInfo, V3)> {
    k.faces(b)
        .unwrap()
        .into_iter()
        .map(|f| {
            let n = k.face_normals_at(b, f.id, &[f.center]).unwrap()[0];
            (f, n)
        })
        .collect()
}

/// The faces `pick` takes (given each face and its outward normal).
fn faces(k: &K, b: BodyId, pick: impl Fn(&FaceInfo, &V3) -> bool) -> Vec<FaceId> {
    face_list(k, b)
        .iter()
        .filter(|(f, n)| pick(f, n))
        .map(|(f, _)| f.id)
        .collect()
}

fn kinds(k: &K, b: BodyId, kind: SurfaceKind) -> usize {
    k.faces(b)
        .unwrap()
        .iter()
        .filter(|f| f.kind == kind)
        .count()
}

/// Planar faces on the plane `n`·x = `d`, facing either way.
fn plane(n: [f64; 3], d: f64) -> impl Fn(&FaceInfo, &V3) -> bool {
    let n = V3::from(n).normalize();
    move |f, m| {
        f.kind == SurfaceKind::Plane
            && m.cross(&n).norm() < 1e-9
            && (f.center.coords.dot(&n) - d).abs() < 1e-7
    }
}

/// Planar faces on the plane `n`·x = `d` whose outward normal is `n`.
fn facing(n: [f64; 3], d: f64) -> impl Fn(&FaceInfo, &V3) -> bool {
    let on = plane(n, d);
    let n = V3::from(n).normalize();
    move |f, m| on(f, m) && m.dot(&n) > 0.0
}

/// Cylindrical faces of radius `r` about an axis through (`x`, `y`) along z.
fn cylinder(x: f64, y: f64, r: f64) -> impl Fn(&FaceInfo, &V3) -> bool {
    move |f, _| {
        f.kind == SurfaceKind::Cylinder
            && f.radius.is_some_and(|q| (q - r).abs() < 1e-7)
            && f.axis.is_some_and(|a| {
                let d = a.dir.into_inner();
                let o = a.origin - p3(x, y, 0.0);
                d.cross(&V3::z()).norm() < 1e-9 && (o - d * o.dot(&d)).norm() < 1e-7
            })
    }
}

/// The edges between a face `a` takes and one `c` takes (springrep's pair of face names).
fn pair(
    k: &K,
    b: BodyId,
    a: impl Fn(&FaceInfo, &V3) -> bool,
    c: impl Fn(&FaceInfo, &V3) -> bool,
) -> Vec<EdgeId> {
    let (fa, fc) = (faces(k, b, a), faces(k, b, c));
    k.edges(b)
        .unwrap()
        .iter()
        .filter(|e| match e.faces {
            [Some(x), Some(y)] => {
                (fa.contains(&x) && fc.contains(&y)) || (fa.contains(&y) && fc.contains(&x))
            }
            _ => false,
        })
        .map(|e| e.id)
        .collect()
}

/// The edge nearest `p` (springrep's `measure::edge_at`).
fn edge_at(k: &K, b: BodyId, p: P3) -> EdgeId {
    k.edges(b)
        .unwrap()
        .iter()
        .map(|e| {
            let d = k
                .min_distance(&DistanceTarget::Point(p), &DistanceTarget::Edge(b, e.id))
                .unwrap()
                .distance;
            (d, e.id)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap()
        .1
}

/// The block [0, 10] × [0, 8] × [0, 6] (springrep's `primitive::block(WORLD, (10, 8, 6))`) and
/// its top face.
fn block_and_top(k: &mut K) -> (BodyId, FaceId) {
    let b = k
        .primitive(&Primitive::block(Point3::origin(), p3(10.0, 8.0, 6.0)))
        .unwrap()
        .bodies[0];
    let top = faces(k, b, plane([0.0, 0.0, 1.0], 6.0));
    assert_eq!(top.len(), 1);
    (b, top[0])
}

fn sweep(
    k: &mut K,
    profile: Loop,
    path: Vec<PathCurve>,
    corners: SweepCorners,
) -> Result<OpResult> {
    let p = Profile::of(Plane::xy(0.0), vec![profile]);
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path,
        control: SweepControl::None,
        faces: vec![],
        corners,
    };
    k.sweep_with(&p, &spec)
}

#[test]
fn a_bend_turns_about_the_axis_it_is_given() {
    // A 2 × 2 square 5 mm up z, then a quarter turn of radius 4 about x: the path turns toward
    // x × z = −y and ends at y = −4, z = 9, heading along −y, so the solid reaches y = −4 and
    // z = 5 + 4 + 1.
    let mut k = K::new();
    // On the plane x = 0, sketch (x, y) is the model's (y, z).
    let yz = Plane::yz(0.0);
    let path = vec![
        PathCurve::sketch(yz, Curve2::line([0.0, 0.0], [0.0, 5.0])),
        PathCurve::sketch(yz, Curve2::arc([-4.0, 5.0], 4.0, 0.0, FRAC_PI_2)),
    ];
    let b = sweep(&mut k, rect(0.0, 0.0, 2.0, 2.0), path, SweepCorners::Mitre)
        .unwrap()
        .bodies[0];
    valid(&k, b);
    let want = 4.0 * (5.0 + 4.0 * FRAC_PI_2);
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "volume {v} vs {want}");
    let bb = k.bounding_box(b).unwrap();
    assert!(
        (bb.min.y + 4.0).abs() < 1e-9 && (bb.max.y - 1.0).abs() < 1e-9,
        "{bb:?}"
    );
    assert!(
        (bb.max.z - 10.0).abs() < 1e-9 && (bb.max.x - 1.0).abs() < 1e-9,
        "{bb:?}"
    );
    // springrep: an arc leg about an axis along the path is refused; cadrs's path is curves,
    // which have no such axis to give (no counterpart).
}

#[test]
fn a_sharp_turn_is_filled_by_the_outside_of_the_profile_turned_about_the_corner() {
    // A 2 × 2 square 5 mm up z, a quarter turn on the spot about y, then 5 mm along x. Each leg
    // is 20 mm³ and they share the 1 × 1 × 2 where they cross inside the turn. Outside it the
    // half of the square beyond the corner, 1 × 2, turns a quarter about the corner's axis: a
    // quarter cylinder of radius 1, 2 long.
    let mut k = K::new();
    // On the plane y = 0, sketch (x, y) is the model's (x, z).
    let xz = Plane::xz(0.0);
    let path = vec![
        PathCurve::sketch(xz, Curve2::line([0.0, 0.0], [0.0, 5.0])),
        PathCurve::sketch(xz, Curve2::line([0.0, 5.0], [5.0, 5.0])),
    ];
    let b = sweep(&mut k, rect(0.0, 0.0, 2.0, 2.0), path, SweepCorners::Round)
        .unwrap()
        .bodies[0];
    valid(&k, b);
    let want = 20.0 + 20.0 - 2.0 + FRAC_PI_2;
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "volume {v} vs {want}");
    let bb = k.bounding_box(b).unwrap();
    assert!(
        (bb.min.x + 1.0).abs() < 1e-9 && (bb.max.x - 5.0).abs() < 1e-9,
        "{bb:?}"
    );
    assert!((bb.max.z - 6.0).abs() < 1e-9, "{bb:?}");
}

#[test]
fn a_sweep_along_lines_and_an_arc_has_its_profile_s_area_times_the_path() {
    // A 2 × 1 rectangle centred on the path: 5 mm up z, a quarter turn of radius 4 toward x, 3 mm
    // on along x. By Pappus the volume is the area times the path's length.
    let mut k = K::new();
    let xz = Plane::xz(0.0);
    let path = vec![
        PathCurve::sketch(xz, Curve2::line([0.0, 0.0], [0.0, 5.0])),
        PathCurve::sketch(xz, Curve2::arc([4.0, 5.0], 4.0, PI, -FRAC_PI_2)),
        PathCurve::sketch(xz, Curve2::line([4.0, 9.0], [7.0, 9.0])),
    ];
    let b = sweep(&mut k, rect(0.0, 0.0, 2.0, 1.0), path, SweepCorners::Mitre)
        .unwrap()
        .bodies[0];
    valid(&k, b);
    let length = 5.0 + 4.0 * FRAC_PI_2 + 3.0;
    let v = volume(&k, b);
    assert!((v - 2.0 * length).abs() < 1e-9, "{v}");
    // The bend ends 4 out and 4 up from its start, the profile 1 to either side; 3 more along x.
    let hi = k.bounding_box(b).unwrap().max;
    assert!(
        (hi.x - 7.0).abs() < 1e-9 && (hi.z - 10.0).abs() < 1e-9,
        "{hi:?}"
    );
}

#[test]
fn a_sweep_along_no_legs_is_invalid() {
    let mut k = K::new();
    let r = sweep(
        &mut k,
        rect(0.0, 0.0, 2.0, 1.0),
        vec![],
        SweepCorners::Mitre,
    );
    assert!(r.is_err(), "{:?}", r.map(|r| r.bodies));
}

#[test]
fn a_path_sweep_along_a_straight_path_is_a_prism_or_a_twisted_one() {
    // A 2 × 1 rectangle 6 mm along z: a line, and a cubic whose control points lie on that line.
    let line = PathCurve::sketch(Plane::xz(0.0), Curve2::line([0.0, 0.0], [0.0, 6.0]));
    let cubic = PathCurve::bezier(&[0.0, 1.0, 4.0, 6.0].map(|z| p3(0.0, 0.0, z)));
    for (name, path) in [("line", line), ("cubic", cubic)] {
        let mut k = K::new();
        let flat = sweep(
            &mut k,
            rect(0.0, 0.0, 2.0, 1.0),
            vec![path.clone()],
            SweepCorners::Mitre,
        )
        .unwrap()
        .bodies[0];
        valid(&k, flat);
        assert_eq!(k.face_count(flat).unwrap(), 6, "{name}");
        let (v, a) = (volume(&k, flat), area(&k, flat));
        assert!(
            (v - 12.0).abs() < 1e-9 && (a - 40.0).abs() < 1e-9,
            "{name}: {v} {a}"
        );
        // Twisted a quarter turn, each section is still the rectangle: the volume is unchanged.
        let p = Profile::of(Plane::xy(0.0), vec![rect(0.0, 0.0, 2.0, 1.0)]);
        let spec = SweepSpec {
            body: BodyKind::Solid,
            path: vec![path],
            control: SweepControl::None,
            faces: vec![],
            corners: SweepCorners::Mitre,
        };
        let law = SweepLaw {
            twist: FRAC_PI_2,
            ..SweepLaw::default()
        };
        let twisted = k.sweep_with_law(&p, &spec, &law).unwrap().bodies[0];
        valid(&k, twisted);
        let v = volume(&k, twisted);
        assert!((v - 12.0).abs() < 1e-7, "{name}: {v}");
        // Its top is the rectangle turned: 1 wide along x and 2 along y.
        let on = |p: P3| k.classify(twisted, p, 1e-9).unwrap() == PointClass::OnBoundary;
        assert!(on(p3(0.0, 0.9, 6.0)), "{name}");
        assert!(!on(p3(0.9, 0.0, 6.0)), "{name}");
    }
}

#[test]
fn a_shell_open_at_the_top_keeps_walls_of_its_thickness() {
    // A 10 × 8 × 6 block hollowed to 1 mm walls, open at the top: the block less a 8 × 6 × 5
    // pocket.
    let mut k = K::new();
    let (block, top) = block_and_top(&mut k);
    let spec = |t: f64| ShellSpec {
        remove: vec![top],
        thickness: t,
        outward: false,
        hollow: false,
        sharp: true,
        // springrep's: a wall that leaves no cavity is refused.
        nothing_hollowed: NothingHollowed::Refuse,
    };
    let b = k.shell_with(block, &spec(1.0)).unwrap().bodies[0];
    valid(&k, b);
    let v = volume(&k, b);
    assert!((v - (480.0 - 240.0)).abs() < 1e-9, "{v}");
    // A wall as thick as half the part leaves no hollow, and is refused.
    let r = k.shell_with(block, &spec(4.0));
    assert!(r.is_err(), "{:?}", r.map(|r| volume(&k, r.bodies[0])));
}

#[test]
fn faces_moved_by_a_motion_go_where_it_takes_them() {
    // The top of a 10 × 8 × 6 block lifted 2 mm: a 10 × 8 × 8 block.
    let mut k = K::new();
    let (block, top) = block_and_top(&mut k);
    let lift = Motion::translation(Vector3::new(0.0, 0.0, 2.0));
    let b = k.move_faces(block, &[top], &lift).unwrap().bodies[0];
    valid(&k, b);
    let v = volume(&k, b);
    assert!((v - 640.0).abs() < 1e-9, "{v}");
    // A map that stretches is no motion.
    let stretch = Motion {
        linear: Matrix3::from_diagonal(&Vector3::new(2.0, 1.0, 1.0)),
        translation: Vector3::zeros(),
    };
    let r = k.move_faces(block, &[top], &stretch);
    assert!(r.is_err(), "{:?}", r.map(|r| volume(&k, r.bodies[0])));
}

#[test]
fn a_conic_segment_is_the_ellipse_arc_it_lies_on() {
    // The quarter of the unit circle from (1, 0) to (0, 1): apex (1, 1), weight cos 45°.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let s = Curve2::conic([1.0, 0.0], [1.0, 1.0], w, [0.0, 1.0]);
    for k in 0..=8 {
        let p = s.point_at(f64::from(k) / 8.0);
        assert!((p.coords.norm() - 1.0).abs() < 1e-12, "{p:?}");
    }
    assert!((s.start() - nalgebra::Point2::new(1.0, 0.0)).norm() < 1e-12);
    assert!((s.end() - nalgebra::Point2::new(0.0, 1.0)).norm() < 1e-12);
    // A slanted one: the arc starts and ends where it does, and its points are the rational
    // quadratic's.
    let (a, apex, b, w) = (
        nalgebra::Point2::new(3.0, -1.0),
        nalgebra::Point2::new(4.5, 2.0),
        nalgebra::Point2::new(1.0, 3.5),
        0.83,
    );
    let s = Curve2::conic(a, apex, w, b);
    assert!((s.start() - a).norm() < 1e-12 && (s.end() - b).norm() < 1e-12);
    for i in 0..=20 {
        let t = f64::from(i) / 20.0;
        let (b0, b1, b2) = ((1.0 - t).powi(2), 2.0 * w * t * (1.0 - t), t * t);
        let want = (a.coords * b0 + apex.coords * b1 + b.coords * b2) / (b0 + b1 + b2);
        let got = s.point_at(t);
        assert!(
            (got.coords - want).norm() < 1e-12,
            "t {t}: {got:?} vs {want:?}"
        );
    }
    // springrep: the slanted conic comes back as an `Ellipse` segment (centre, axes, rotation)
    // whose equation its points satisfy, and weights of 1 and above (a parabola, a hyperbola)
    // and 0 are refused. cadrs keeps a conic as a rational Bézier of any weight (no counterpart).
}

#[test]
fn a_file_of_two_touching_solids_reads_as_the_one_part() {
    // Two blocks face to face, written as a file of two solids, read back as one part of their
    // joint volume in one piece.
    let mut k = K::new();
    let a = k
        .primitive(&Primitive::block(Point3::origin(), p3(4.0, 4.0, 4.0)))
        .unwrap()
        .bodies[0];
    let b = k
        .primitive(&Primitive::block(p3(4.0, 0.0, 0.0), p3(6.0, 4.0, 4.0)))
        .unwrap()
        .bodies[0];
    let both = k.compound(&[a, b]).unwrap().bodies[0];
    assert_eq!(k.solid_count(both).unwrap(), 2);
    let text = k.export_step(&[both]).unwrap();
    // springrep's `step::read_part` reads a file's solids as one part, fused; cadrs reads one body
    // per solid (no counterpart), so the solids read are united here.
    let read = k.import_step(&text).unwrap();
    assert!(!read.is_empty());
    let mut part = read[0];
    for &other in &read[1..] {
        part = union(&mut k, part, other);
    }
    valid(&k, part);
    assert_eq!(k.solid_count(part).unwrap(), 1);
    let v = volume(&k, part);
    assert!((v - 96.0).abs() < 1e-9, "{v}");
}

#[test]
fn a_mesh_within_a_tolerance_lies_that_close() {
    // A ball of radius 5 meshed within 0.01: the mesh's volume falls short of the ball's by no
    // more than the tolerance times its area.
    let mut k = K::new();
    let ball = k
        .primitive(&Primitive::ball(Point3::origin(), 5.0))
        .unwrap()
        .bodies[0];
    let q = Tessellation {
        deflection: 0.01,
        angle: FRAC_PI_2,
    };
    let m = k.tessellate(ball, q).unwrap();
    let c = tess::check_tri_mesh(&m);
    assert!(c.is_watertight(), "{c:?}");
    let mv: f64 = m
        .indices
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| m.positions[i as usize].coords);
            a.dot(&b.cross(&c)) / 6.0
        })
        .sum();
    let exact = 4.0 / 3.0 * PI * 125.0;
    assert!((mv - exact).abs() <= 0.01 * 4.0 * PI * 25.0, "{mv}");
}

#[test]
fn the_step_writer_says_which_order_it_wrote_the_faces_in() {
    let mut k = K::new();
    let b = extrude(&mut k, 0.0, rect(0.0, 0.0, 4.0, 2.0), vec![], 3.0);
    let text = String::from_utf8(k.export_step(&[b]).unwrap()).unwrap();
    // springrep's `step::write_faces` gives the order it wrote the faces in, by name; cadrs has
    // no counterpart. What is left: six faces written, and the same text each time.
    assert_eq!(text.matches("ADVANCED_FACE(").count(), 6);
    assert_eq!(
        text,
        String::from_utf8(k.export_step(&[b]).unwrap()).unwrap()
    );
}

#[test]
fn a_section_is_given_as_the_curves_it_runs_along() {
    // A Ø10 rod cut square to its axis: one outline, every piece of it a circle of radius 5
    // about the axis at the cut's height. springrep's `measure::section_curves` (a body cut by a
    // plane) is the native section of the rod with a box whose bottom face is that plane.
    let mut k = K::new();
    let rod = k
        .primitive(&Primitive::cylinder(
            Point3::origin(),
            Vector3::z(),
            5.0,
            8.0,
        ))
        .unwrap()
        .bodies[0];
    let slab = k
        .primitive(&Primitive::block(
            p3(-50.0, -50.0, 3.0),
            p3(50.0, 50.0, 50.0),
        ))
        .unwrap()
        .bodies[0];
    let cut =
        cadrs_kernel::native::boolean::section(k.get(rod).unwrap(), k.get(slab).unwrap()).unwrap();
    let cut = k.insert(cut);
    let edges = k.edges(cut).unwrap();
    assert!(!edges.is_empty());
    let mut turned = 0.0;
    for e in &edges {
        let Some(c) = e.circle else {
            panic!("a rod's section is circles, not {:?}", e.curve);
        };
        assert!(
            (c.radius - 5.0).abs() < 1e-9 && (c.center.z - 3.0).abs() < 1e-9,
            "{c:?}"
        );
        turned += e.length / c.radius;
    }
    assert!((turned - std::f64::consts::TAU).abs() < 1e-9, "{turned}");
}
