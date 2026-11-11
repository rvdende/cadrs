//! The short ways to write the kernel's inputs (`construct`) and to pick faces and edges
//! (`Pick`): each makes what the long way does.

use cadrs_kernel::*;
use nalgebra::{Point3, Vector3};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn p3(x: f64, y: f64, z: f64) -> Point3<f64> {
    Point3::new(x, y, z)
}

fn volume(k: &DefaultKernel, body: BodyId) -> f64 {
    k.mass_properties(body).unwrap().volume
}

#[test]
fn planes_are_the_named_ones() {
    let (xy, xz, yz) = (Plane::xy(2.0), Plane::xz(3.0), Plane::yz(4.0));
    assert_eq!(xy.to_model([1.0, 5.0].into()), p3(1.0, 5.0, 2.0));
    assert_eq!(xz.to_model([1.0, 5.0].into()), p3(1.0, 3.0, 5.0));
    assert_eq!(yz.to_model([1.0, 5.0].into()), p3(4.0, 1.0, 5.0));
    // A plane's x axis is made square to its normal.
    let leaning = Plane::new(Point3::origin(), Vector3::new(1.0, 0.0, 1.0), Vector3::z());
    assert!((leaning.x_dir.into_inner() - Vector3::x()).norm() < 1e-15);
    // A plane through an axis has the axis as its sketch y.
    for dir in [Vector3::z(), Vector3::x(), Vector3::new(1.0, 2.0, -3.0)] {
        let axis = Axis::new(p3(1.0, 2.0, 3.0), dir);
        let on = Plane::through_axis(&axis);
        assert!((on.y_dir().into_inner() - axis.dir.into_inner()).norm() < 1e-12);
        assert!(on.x_dir.dot(&axis.dir).abs() < 1e-12);
        let square = Plane::square_to(axis.origin, dir);
        assert!((square.normal.into_inner() - axis.dir.into_inner()).norm() < 1e-12);
    }
}

#[test]
fn a_rectangle_extruded_is_its_block() {
    let mut k = DefaultKernel::new();
    let sketch = Profile::of(Plane::xy(1.0), vec![Loop::rectangle(0.0, 0.0, 2.0, 3.0)]);
    let up = k
        .extrude_with(&sketch, &ExtrudeSpec::normal_to(&sketch, 4.0))
        .unwrap()
        .first()
        .unwrap();
    assert!((volume(&k, up) - 24.0).abs() < 1e-9);
    let down = k
        .extrude_with(&sketch, &ExtrudeSpec::normal_to(&sketch, -4.0))
        .unwrap()
        .first()
        .unwrap();
    let bb = k.bounding_box(down).unwrap();
    assert!((bb.min.z + 3.0).abs() < 1e-9 && (bb.max.z - 1.0).abs() < 1e-9);
    // Half each side of the plane.
    let both = k
        .extrude_with(&sketch, &ExtrudeSpec::normal_to(&sketch, 4.0).both_ways())
        .unwrap()
        .first()
        .unwrap();
    let bb = k.bounding_box(both).unwrap();
    assert!((bb.min.z + 1.0).abs() < 1e-9 && (bb.max.z - 3.0).abs() < 1e-9);
    // A hole through it.
    let holed = Profile::with_holes(
        Plane::xy(0.0),
        Loop::rectangle(0.0, 0.0, 4.0, 4.0),
        vec![Loop::circle([2.0, 2.0], 1.0)],
    );
    let plate = k
        .extrude_with(&holed, &ExtrudeSpec::normal_to(&holed, 2.0))
        .unwrap()
        .first()
        .unwrap();
    assert!((volume(&k, plate) - (32.0 - TAU)).abs() < 1e-9);
}

#[test]
fn an_outline_is_drawn_from_where_it_is() {
    // A bull-nose mill's half section, its corner a conic and then the same arc about its
    // centre: the revolved solids are one.
    let corner = 0.5;
    let conic = Loop::from_point([0.0, 0.0])
        .line_to([1.5, 0.0])
        .conic_to([2.0, 0.0], (PI / 4.0).cos(), [2.0, corner])
        .line_to([2.0, 3.0])
        .line_to([0.0, 3.0])
        .close();
    let arc = Loop::from_point([0.0, 0.0])
        .line_to([1.5, 0.0])
        .arc_about([1.5, corner], FRAC_PI_2)
        .line_to([2.0, 3.0])
        .line_to([0.0, 3.0])
        .close();
    assert_eq!(conic.curves.len(), 5, "closed back to its start");
    assert_eq!(arc.curves.len(), 5);
    assert!((arc.curves[1].end() - nalgebra::Point2::new(2.0, corner)).norm() < 1e-12);
    // (Pappus: the cylinder less what the corner's rounding takes off.)
    let (square, quarter) = (corner * corner, PI * corner * corner / 4.0);
    let off = TAU
        * ((2.0 - corner / 2.0) * square - (2.0 - corner + 4.0 * corner / (3.0 * PI)) * quarter);
    let want = PI * 4.0 * 3.0 - off;
    let mut k = DefaultKernel::new();
    let axis = Axis::new(p3(1.0, 2.0, 3.0), Vector3::new(0.5, 0.0, 0.75f64.sqrt()));
    for section in [conic, arc] {
        let sketch = Profile::of(Plane::through_axis(&axis), vec![section]);
        let mill = k
            .revolve_with(&sketch, &RevolveSpec::solid(axis, None))
            .unwrap()
            .first()
            .unwrap();
        assert_eq!(k.faces(mill).unwrap().len(), 4);
        assert!(
            (volume(&k, mill) - want).abs() < 1e-8,
            "{}",
            volume(&k, mill)
        );
    }
}

#[test]
fn motions_are_their_steps() {
    let axis = Axis::new(p3(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 2.0));
    assert_eq!(axis.dir.into_inner(), Vector3::z());
    let screw = SolidMotion::helical(axis, 4.0, -PI, 3.0 * PI);
    let SolidMotion::Rigid { steps } = &screw else {
        panic!("a screw is a turn and a slide");
    };
    assert_eq!(
        steps[..],
        [
            RigidStep::Turn {
                axis,
                from: -PI,
                to: 3.0 * PI
            },
            RigidStep::Slide {
                from: Vector3::z() * -2.0,
                to: Vector3::z() * 6.0
            },
        ]
    );
    // A box slid along its own edge is the longer box.
    let mut k = DefaultKernel::new();
    let cube = k
        .primitive(&Primitive::block(Point3::origin(), p3(1.0, 1.0, 1.0)))
        .unwrap()
        .first()
        .unwrap();
    let slid = k
        .sweep_solid(cube, &SolidMotion::slide(Vector3::x() * 3.0))
        .unwrap()
        .first()
        .unwrap();
    assert!((volume(&k, slid) - 4.0).abs() < 1e-9);
    // And turned a quarter about an upright line a cube away from its corner: the cube, and
    // the quarter of the ring between its nearest and farthest corners (√2 and √8 away).
    let about = Axis::new(p3(-1.0, -1.0, 0.0), Vector3::z());
    let swung = k
        .sweep_solid(cube, &SolidMotion::swing(about, 0.0, FRAC_PI_2))
        .unwrap()
        .first()
        .unwrap();
    let want = 1.0 + PI / 4.0 * (8.0 - 2.0);
    assert!(
        (volume(&k, swung) - want).abs() < 1e-8,
        "{}",
        volume(&k, swung)
    );
}

#[test]
fn a_helix_lies_on_its_cylinder() {
    let axis = Axis::new(p3(1.0, 2.0, 3.0), Vector3::new(1.0, 1.0, 1.0));
    let helix = Helix::new(axis, 2.0, 0.5, 0.3, 2.25);
    let path = helix.path();
    assert_eq!(path.len(), 36, "sixteen pieces a turn");
    let off = |p: &Point3<f64>| {
        let w = p - axis.origin;
        let along = w.dot(&axis.dir);
        ((w - axis.dir.into_inner() * along).norm(), along)
    };
    for (i, piece) in path.iter().enumerate() {
        let PathCurve::Bezier3 { poles, weights } = piece else {
            panic!("a helix's pieces are Bézier curves");
        };
        assert_eq!((poles.len(), weights.len()), (3, 3));
        let (r, z) = off(&poles[0]);
        assert!((r - 2.0).abs() < 1e-12);
        assert!((z - 0.5 * 2.25 * i as f64 / 36.0).abs() < 1e-12);
    }
    let (end, tangent) = helix.at(1.0);
    let (r, z) = off(&end);
    assert!((r - 2.0).abs() < 1e-12 && (z - 1.125).abs() < 1e-12);
    // (Right-handed about the axis: the tangent rises along it.)
    assert!(tangent.dot(&axis.dir) > 0.0);
    // On a cone the pieces are quintics from one radius to the other.
    let spiral = Helix::conical(Axis::z(), 3.0, 1.0, 4.0, 0.0, 1.0);
    let path = spiral.path();
    assert_eq!(path.len(), 16);
    let PathCurve::Bezier3 { poles, weights } = &path[15] else {
        panic!("a spiral's pieces are Bézier curves");
    };
    assert_eq!((poles.len(), weights.len()), (6, 0));
    assert!((poles[5] - p3(1.0, 0.0, 4.0)).norm() < 1e-12);
}

#[test]
fn faces_and_edges_are_picked_by_a_point_on_them() {
    let mut k = DefaultKernel::new();
    let block = k
        .primitive(&Primitive::block(Point3::origin(), p3(2.0, 3.0, 4.0)))
        .unwrap()
        .first()
        .unwrap();
    let (top, normal) = k.normal_at(block, p3(1.0, 1.5, 4.0)).unwrap();
    assert_eq!(top, k.face_at(block, p3(1.0, 1.5, 4.0)).unwrap());
    assert!((normal - Vector3::z()).norm() < 1e-12);
    assert!(matches!(
        k.location_at(block, p3(1.0, 1.5, 4.0)).unwrap(),
        DistanceLocation::Face(f, ..) if f == top
    ));
    let edge = k.edge_at(block, p3(1.0, 0.0, 4.0)).unwrap();
    let info = k
        .edges(block)
        .unwrap()
        .into_iter()
        .find(|e| e.id == edge)
        .unwrap();
    assert!((info.length - 2.0).abs() < 1e-12 && info.faces.contains(&Some(top)));
    assert_eq!(k.edges_but_seams(block).unwrap().len(), 12);
    let sides = k
        .faces_where(block, &|f| f.is_side_for(&Vector3::z()))
        .unwrap();
    assert_eq!(sides.len(), 4);
    assert!(!sides.contains(&top));
    let long = k
        .edges_where(block, &|e| (e.length - 4.0).abs() < 1e-9)
        .unwrap();
    assert_eq!(long.len(), 4);
    // A cylinder's seam is no edge to pick.
    let rod = k
        .primitive(&Primitive::cylinder(
            Point3::origin(),
            Vector3::z(),
            1.0,
            5.0,
        ))
        .unwrap()
        .first()
        .unwrap();
    let all = k.edges(rod).unwrap();
    let picked = k.edges_but_seams(rod).unwrap();
    assert_eq!(picked.len(), all.iter().filter(|e| !e.is_seam()).count());
    assert_eq!(picked.len(), 2, "its two rims");
    assert_eq!(
        k.faces_where(rod, &|f| f.is_side_for(&Vector3::z()))
            .unwrap()
            .len(),
        1
    );
    // Shelled open at its top, by the face picked there.
    let open = k
        .shell_with(block, &ShellSpec::inward(vec![top], 0.5).mitred())
        .unwrap()
        .first()
        .unwrap();
    assert!((volume(&k, open) - (24.0 - 1.0 * 2.0 * 3.5)).abs() < 1e-9);
}

#[test]
fn a_face_between_curves_in_space_is_extruded() {
    let mut k = DefaultKernel::new();
    let (a, b, c) = (p3(0.0, 0.0, 1.0), p3(4.0, 0.0, 1.0), p3(0.0, 3.0, 1.0));
    let triangle = [
        PathCurve::bezier(&[a, b]),
        PathCurve::bezier(&[b, c]),
        PathCurve::bezier(&[c, a]),
    ];
    let face = k.planar_face(&triangle).unwrap();
    let spec = ExtrudeSpec::blind(Vector3::z_axis(), 2.0).with_faces(vec![face]);
    let prism = k
        .extrude_with(&Profile::new(Plane::xy(1.0), vec![]), &spec)
        .unwrap()
        .first()
        .unwrap();
    assert!((volume(&k, prism) - 12.0).abs() < 1e-9);
}

#[test]
fn an_operation_with_no_body_has_no_first() {
    let none = OpResult {
        bodies: vec![],
        history: History::default(),
    };
    assert!(none.first().is_err());
}
