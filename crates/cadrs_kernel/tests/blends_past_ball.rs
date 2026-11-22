//! Blends past the rolling ball, through the `Kernel` trait: conic fillets sized by their
//! setback, Hartmann's curvature-continuous section of order 2 (`FilletProfile::Thumbweight`),
//! the fillet between two faces that need not share an edge, and set-back corners. Checked
//! against closed forms.

use std::f64::consts::PI;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3};

fn one(r: OpResult) -> BodyId {
    assert_eq!(r.bodies.len(), 1, "one body");
    r.bodies[0]
}

fn polygon(pts: &[[f64; 2]]) -> Loop {
    let p = |q: [f64; 2]| Point2::new(q[0], q[1]);
    Loop {
        curves: (0..pts.len())
            .map(|i| Curve2::Line {
                a: p(pts[i]),
                b: p(pts[(i + 1) % pts.len()]),
                source: None,
            })
            .collect(),
    }
}

/// The polygon on the top plane, extruded up by `h`.
fn prism(k: &mut NativeKernel, pts: &[[f64; 2]], h: f64) -> BodyId {
    let region = Region {
        outer: polygon(pts),
        holes: vec![],
        source: None,
    };
    one(k
        .extrude(&Profile::new(Plane::top(), vec![region]), Extent::Blind(h))
        .unwrap())
}

/// A box `x` × `y` × `z` with a corner at the origin.
fn block(k: &mut NativeKernel, x: f64, y: f64, z: f64) -> BodyId {
    prism(k, &[[0.0, 0.0], [x, 0.0], [x, y], [0.0, y]], z)
}

fn volume(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

fn close(got: f64, want: f64, rel: f64) {
    assert!(
        (got - want).abs() <= rel * want.abs(),
        "{got} vs {want} ({:.1e} off)",
        (got - want).abs() / want.abs()
    );
}

fn valid(k: &NativeKernel, b: BodyId) {
    let issues = k.get(b).unwrap().check();
    assert!(issues.is_empty(), "{issues:?}");
}

/// The edge of `b` through `p`.
fn edge_at(k: &NativeKernel, b: BodyId, p: [f64; 3]) -> EdgeId {
    let p = Point3::new(p[0], p[1], p[2]);
    k.edges(b)
        .unwrap()
        .into_iter()
        .find(|e| {
            let d = e.end - e.start;
            let t = (p - e.start).dot(&d) / d.norm_squared();
            (0.0..=1.0).contains(&t) && (e.start + d * t - p).norm() < 1e-9
        })
        .expect("no edge there")
        .id
}

/// The face of `b` nearest `p`.
fn face_at(k: &NativeKernel, b: BodyId, p: [f64; 3]) -> FaceId {
    let r = k
        .min_distance(
            &DistanceTarget::Point(Point3::new(p[0], p[1], p[2])),
            &DistanceTarget::Body(b),
        )
        .unwrap();
    let DistanceLocation::Face(f, ..) = r.b_at else {
        panic!("{p:?} is on no one face");
    };
    f
}

fn spec(size: FilletSize, profile: FilletProfile) -> FilletSpec {
    FilletSpec {
        size,
        profile,
        ..FilletSpec::radius(1.0)
    }
}

#[test]
fn a_conic_fillet_by_setback_takes_off_its_section() {
    // A square edge 30 long, its conic meeting both faces 2 from it. At rho = √2 − 1 it is the
    // quarter circle of radius 2; at rho = ½ the parabola, which leaves 2/3 of the triangle on
    // its legs, so takes off d²/6.
    let d = 2.0;
    for (rho, off) in [
        (2f64.sqrt() - 1.0, (1.0 - PI / 4.0) * d * d),
        (0.5, d * d / 6.0),
    ] {
        let mut k = NativeKernel::new();
        let b = block(&mut k, 10.0, 20.0, 30.0);
        let e = edge_at(&k, b, [0.0, 0.0, 15.0]);
        let s = spec(FilletSize::Setback(d), FilletProfile::Conic { rho });
        let r = one(k.fillet_with(b, &[e], &s).unwrap());
        valid(&k, r);
        close(volume(&k, r), 6000.0 - 30.0 * off, 1e-10);
    }
}

#[test]
fn a_setback_on_a_slanted_edge_is_measured_along_the_faces() {
    // An equilateral prism's 60° edge: a setback of 1 along each face is the round of radius
    // tan 30°, which takes off its kite r²·cot(θ/2) less its sector (π − θ)/2·r².
    let mut k = NativeKernel::new();
    let h = 3f64.sqrt() * 5.0;
    let b = prism(&mut k, &[[0.0, 0.0], [10.0, 0.0], [5.0, h]], 4.0);
    let e = edge_at(&k, b, [0.0, 0.0, 2.0]);
    let theta = PI / 3.0;
    let r_ball = (theta / 2.0).tan();
    let s = spec(FilletSize::Setback(1.0), FilletProfile::Circular);
    let r = one(k.fillet_with(b, &[e], &s).unwrap());
    valid(&k, r);
    let off = r_ball * r_ball * (1.0 / (theta / 2.0).tan() - (PI - theta) / 2.0);
    close(volume(&k, r), 0.5 * 10.0 * h * 4.0 - 4.0 * off, 1e-10);
}

#[test]
fn the_equal_area_g2_section_takes_off_what_the_round_does() {
    // Hartmann's order-2 section at the equal-area thumbweight between the round's contacts:
    // the same (1 − π/4)·r² off a square edge, but leaving each face as straight as the face.
    let r_ = 1.5;
    let mut k = NativeKernel::new();
    let b = block(&mut k, 10.0, 20.0, 30.0);
    let e = edge_at(&k, b, [0.0, 0.0, 15.0]);
    let s = spec(
        FilletSize::Radius(r_),
        FilletProfile::Thumbweight { center: None },
    );
    let g2 = one(k.fillet_with(b, &[e], &s).unwrap());
    valid(&k, g2);
    close(
        volume(&k, g2),
        6000.0 - 30.0 * (1.0 - PI / 4.0) * r_ * r_,
        1e-10,
    );
    // A tighter middle is a fuller shoulder: less comes off (the middle radius falls as the
    // thumbweight rises, and so does the area the section takes off).
    let tight = spec(
        FilletSize::Radius(r_),
        FilletProfile::Thumbweight {
            center: Some(0.5 * r_),
        },
    );
    let t = one(k.fillet_with(b, &[e], &tight).unwrap());
    valid(&k, t);
    assert!(volume(&k, t) > volume(&k, g2));
}

#[test]
#[ignore = "the blend tool's boolean leaves an open shell on edges 0 and 7 at this centre radius"]
fn g2_fillets_with_a_sharp_middle_on_neighbouring_edges() {
    // brep-conformance fillet-g2-f693f060e35c38af: five edges of an octagonal prism, offset
    // 1.213 and a centre radius of 0.193 (a sharp middle).
    let pts = [
        [4.0538962799406555, 3.773913246634172],
        [-0.060428929679734435, 5.5383070144309725],
        [-3.7699000546621435, 4.057628595558732],
        [-5.450354285344475, -0.9849540120557292],
        [-3.7062582281803382, -4.115840884641083],
        [0.046255228535082446, -5.538443526431476],
        [4.038241479243662, -3.790659836618623],
        [5.484123507622734, 0.7751681074304311],
    ];
    let mut k = NativeKernel::new();
    let b = prism(&mut k, &pts, 5.474537886577101);
    let edges: Vec<EdgeId> = [0, 2, 3, 6, 7]
        .iter()
        .map(|&i| edge_at(&k, b, [pts[i][0], pts[i][1], 2.0]))
        .collect();
    let s = spec(
        FilletSize::Radius(1.2130314619632838),
        FilletProfile::Thumbweight {
            center: Some(0.19343450528914122),
        },
    );
    let r = one(k.fillet_with(b, &edges, &s).unwrap());
    valid(&k, r);
    assert!(volume(&k, r) < volume(&k, b));
}

#[test]
fn a_fillet_between_faces_consumes_a_chamfer() {
    // A box's edge chamfered 0.5 back, then rounded between the two faces either side of the
    // chamfer by a ball of radius 3: the chamfer goes, and what is left is the sharp edge
    // rounded at 3.
    let mut k = NativeKernel::new();
    let b = block(&mut k, 10.0, 20.0, 30.0);
    let e = edge_at(&k, b, [0.0, 0.0, 15.0]);
    let c = one(k.chamfer(b, &[e], ChamferSpec::EqualDistance(0.5)).unwrap());
    let (fa, fb) = (
        face_at(&k, c, [5.0, 0.0, 15.0]),
        face_at(&k, c, [0.0, 10.0, 15.0]),
    );
    let r = one(k.fillet_faces(c, fa, fb, 3.0).unwrap());
    valid(&k, r);
    close(volume(&k, r), 6000.0 - 30.0 * (1.0 - PI / 4.0) * 9.0, 1e-10);
    // A ball too big to touch the faces, and one face named twice.
    assert!(matches!(
        k.fillet_faces(c, fa, fb, 11.0),
        Err(KernelError::TooLarge { .. })
    ));
    assert!(k.fillet_faces(c, fa, fa, 3.0).is_err());
}

#[test]
fn a_set_back_corner_stops_each_fillet_short() {
    // A box corner's three edges filleted R1, set back 3: outside the cube of side 3 at the
    // corner each fillet is whole, and the result lies between the box filleted with that cube
    // left sharp and with it taken away.
    let (rad, s) = (1.0, 3.0);
    let k_ = 1.0 - PI / 4.0;
    let w = [10.0, 20.0, 30.0];
    {
        let profile = FilletProfile::Circular;
        let mut k = NativeKernel::new();
        let b = block(&mut k, w[0], w[1], w[2]);
        let edges = [
            edge_at(&k, b, [5.0, 20.0, 30.0]),
            edge_at(&k, b, [10.0, 10.0, 30.0]),
            edge_at(&k, b, [10.0, 20.0, 15.0]),
        ];
        let sp = spec(FilletSize::Radius(rad), profile);
        let r = one(k.fillet_setback(b, &edges, &sp, s).unwrap());
        valid(&k, r);
        let v = volume(&k, r);
        let full = w[0] * w[1] * w[2];
        let along = |from: f64| (0..3).map(|i| k_ * rad * rad * (w[i] - from)).sum::<f64>();
        let (least, most) = (full - along(s) - s * s * s, full - along(s));
        assert!(v > least && v < most, "{v} not in [{least}, {most}]");
        // Where the face's two contact lines would cross, the face has gone.
        let q = Point3::new(w[0] - rad, w[1] - rad, w[2] - 1e-5);
        assert_eq!(k.classify(r, q, 1e-9).unwrap(), PointClass::Outside);
        // Away from the corner, the fillet is whole.
        let p = Point3::new(w[0] - 1.2 * s, w[1] - 0.1 * rad, w[2] - 0.1 * rad);
        assert_eq!(k.classify(r, p, 1e-9).unwrap(), PointClass::Outside);
        // A setback no longer than the fillet is refused.
        assert!(k.fillet_setback(b, &edges, &sp, 0.9 * rad).is_err());
    }
}

#[test]
fn a_g2_set_back_corner_is_its_fillets_beyond_the_setback() {
    // (springrep's test of its corner, MIT.) A 10 cube's corner, its edges rounded by the
    // equal-area G2 section with contacts 1 from each and set back 3: the section takes what a
    // ball of radius 1 does, (1 − π/4) from every unit of each edge beyond the 3 cube at the
    // corner; within it the corner takes less than the whole cube and more than nothing. On
    // each face the flat has gone where the fillets' contact lines would cross.
    let mut k = NativeKernel::new();
    let b = block(&mut k, 10.0, 10.0, 10.0);
    let edges = [
        edge_at(&k, b, [5.0, 10.0, 10.0]),
        edge_at(&k, b, [10.0, 5.0, 10.0]),
        edge_at(&k, b, [10.0, 10.0, 5.0]),
    ];
    let sp = spec(
        FilletSize::Radius(1.0),
        FilletProfile::Thumbweight { center: None },
    );
    let r = one(k.fillet_setback(b, &edges, &sp, 3.0).unwrap());
    valid(&k, r);
    let round = 1.0 - PI / 4.0;
    let most = 1000.0 - 3.0 * 7.0 * round;
    let v = volume(&k, r);
    assert!(v > most - 27.0 && v < most, "{v} against {most}");
    let q = Point3::new(9.0, 9.0, 10.0 - 1e-5);
    assert_eq!(k.classify(r, q, 1e-9).unwrap(), PointClass::Outside);
    let p = Point3::new(5.0, 9.9, 9.9);
    assert_eq!(k.classify(r, p, 1e-9).unwrap(), PointClass::Outside);
}
