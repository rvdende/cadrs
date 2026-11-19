//! Ported from springrep's tests/distance.rs (MIT, https://github.com/springcraft-ai/springrep):
//! closest distances against closed forms: between bodies, faces, edges and vertices.
//!
//! springrep's `distance::closest` between whole bodies measures the solids (a ball inside a box
//! is no distance from it); cadrs's `Kernel::min_distance` with `DistanceTarget::Body` measures
//! their boundaries, the only body distance it has, so the cases where one solid holds part of
//! the other show that gap. A vertex selection is its point (`DistanceTarget::Point`).

mod springrep_port;

use std::f64::consts::TAU;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

fn bodies(k: &NativeKernel, a: BodyId, b: BodyId) -> DistanceResult {
    k.min_distance(&DistanceTarget::Body(a), &DistanceTarget::Body(b))
        .unwrap()
}

/// The distance, to rounding, and that both points lie that far apart.
fn assert_distance(c: DistanceResult, want: f64) {
    assert!(
        (c.distance - want).abs() <= 1e-9 * (1.0 + want),
        "{c:?} vs {want}"
    );
    assert!(
        ((c.a - c.b).norm() - c.distance).abs() <= 1e-12 * (1.0 + want),
        "{c:?}"
    );
}

fn face_at(k: &NativeKernel, b: BodyId, p: P3) -> FaceId {
    k.faces(b)
        .unwrap()
        .into_iter()
        .min_by(|x, y| (x.center - p).norm().total_cmp(&(y.center - p).norm()))
        .unwrap()
        .id
}

fn vertex_at(k: &NativeKernel, b: BodyId, p: P3) -> P3 {
    k.vertices(b)
        .unwrap()
        .into_iter()
        .min_by(|x, y| (x.point - p).norm().total_cmp(&(y.point - p).norm()))
        .unwrap()
        .point
}

fn ball_at(k: &mut NativeKernel, c: P3, r: f64) -> BodyId {
    ball(k, [c.x, c.y, c.z], r)
}

#[test]
fn overlapping_balls_micrometres_across_are_no_distance_apart() {
    let mut k = NativeKernel::new();
    let a = ball_at(&mut k, P3::origin(), 0.004054250852155359);
    let b = ball_at(
        &mut k,
        P3::new(0.00338806114604008, 0.0, 0.0),
        0.0026689525243786373,
    );
    let d = bodies(&k, a, b).distance;
    assert!(d <= 1e-15, "{d}");
}

#[test]
fn two_balls_are_their_centres_apart_less_both_radii() {
    let mut k = NativeKernel::new();
    let (c1, c2) = (P3::new(1.0, -2.0, 0.5), P3::new(9.0, 3.0, -1.5));
    let a = ball_at(&mut k, c1, 3.0);
    let b = ball_at(&mut k, c2, 2.0);
    let c = bodies(&k, a, b);
    assert_distance(c, (c1 - c2).norm() - 5.0);
    assert!(
        ((c.a - c1).norm() - 3.0).abs() < 1e-9 && ((c.b - c2).norm() - 2.0).abs() < 1e-9,
        "{c:?}"
    );
}

#[test]
fn a_ball_by_a_box_meets_its_face_edge_or_corner() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, [0.0; 3], [10.0, 6.0, 3.0]);
    let s = ball_at(&mut k, P3::new(13.0, 3.0, 1.5), 1.0);
    assert_distance(bodies(&k, s, b), 2.0);
    let edge = (2.0f64 * 2.0 + 2.0 * 2.0).sqrt() - 1.0;
    let s = ball_at(&mut k, P3::new(12.0, 8.0, 1.5), 1.0);
    assert_distance(bodies(&k, s, b), edge);
    let corner = 12.0f64.sqrt() - 1.0;
    let s = ball_at(&mut k, P3::new(12.0, 8.0, 5.0), 1.0);
    assert_distance(bodies(&k, s, b), corner);
}

#[test]
fn boxes_set_corner_to_corner_meet_edge_to_edge() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0; 3], [1.0; 3]);
    let b = block(&mut k, [2.0, 2.0, 0.0], [3.0, 3.0, 1.0]);
    assert_distance(bodies(&k, a, b), 2.0f64.sqrt());
}

#[test]
fn crossed_rods_are_their_axes_apart_less_both_radii() {
    let mut k = NativeKernel::new();
    let a = extrude(
        &mut k,
        frame(P3::new(0.0, 0.0, -5.0), V3::z(), V3::x()),
        circle(0.0, 0.0, 1.0),
        10.0,
    );
    let b = extrude(
        &mut k,
        frame(P3::new(-5.0, 4.0, 0.0), V3::x(), V3::y()),
        circle(0.0, 0.0, 1.5),
        10.0,
    );
    assert_distance(bodies(&k, a, b), 4.0 - 1.0 - 1.5);
}

#[test]
fn a_ball_on_a_torus_axis_is_nearest_along_a_whole_ring() {
    let mut k = NativeKernel::new();
    let xz = frame(P3::origin(), -V3::y(), V3::x());
    let t = revolve(&mut k, xz, circle(10.0, 0.0, 2.0), P3::origin(), V3::z());
    let s = ball_at(&mut k, P3::new(0.0, 0.0, 6.0), 1.0);
    assert_distance(bodies(&k, s, t), (100.0f64 + 36.0).sqrt() - 3.0);
}

#[test]
#[ignore = "cadrs measures distance between boundaries, and has no distance between solids"]
fn a_ball_inside_a_box_or_through_it_is_no_distance_away() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, [0.0; 3], [10.0, 6.0, 3.0]);
    // Inside, touching nothing: springrep's solids share points, so no distance (cadrs's
    // boundaries are a unit apart).
    let s = ball_at(&mut k, P3::new(5.0, 3.0, 1.5), 1.0);
    assert_eq!(bodies(&k, s, b).distance, 0.0);
    // Through the face x = 10.
    let s = ball_at(&mut k, P3::new(10.5, 3.0, 1.5), 1.0);
    assert_distance(bodies(&k, s, b), 0.0);
}

#[test]
#[ignore = "cadrs measures distance between boundaries, and has no distance between solids"]
fn a_rod_inside_one_a_hair_wider_is_no_distance_away() {
    let mut k = NativeKernel::new();
    let f = |z: f64| frame(P3::new(0.0, 0.0, z), V3::z(), V3::x());
    let outer = extrude(&mut k, f(1.28), circle(0.0, 0.0, 6.6391 / 2.0), 9.17);
    let inner = extrude(&mut k, f(1.38), circle(0.0, 0.0, 6.6390934 / 2.0), 5.64);
    assert_distance(bodies(&k, outer, inner), 0.0);
    assert_distance(bodies(&k, inner, outer), 0.0);
}

#[test]
fn faces_edges_and_vertices_measure_as_themselves() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, [0.0; 3], [10.0, 6.0, 3.0]);
    let (bottom, top) = (
        face_at(&k, b, P3::new(5.0, 3.0, 0.0)),
        face_at(&k, b, P3::new(5.0, 3.0, 3.0)),
    );
    let c = k
        .min_distance(
            &DistanceTarget::Face(b, bottom),
            &DistanceTarget::Face(b, top),
        )
        .unwrap();
    assert_distance(c, 3.0);
    let other = block(&mut k, [12.0, 7.0, 4.0], [14.0, 9.0, 5.0]);
    let v = vertex_at(&k, other, P3::new(12.0, 7.0, 4.0));
    let c = k
        .min_distance(&DistanceTarget::Point(v), &DistanceTarget::Face(b, top))
        .unwrap();
    assert_distance(c, (4.0f64 + 1.0 + 1.0).sqrt());
}

#[test]
fn a_ball_beside_a_spline_extrusion_is_the_profile_curves_distance_away() {
    // The outline's own distance comes from sampling the profile's curve (the disc's bottom edge)
    // densely and refining the nearest sample by golden sections.
    let mut k = NativeKernel::new();
    let pts: Vec<[f64; 2]> = (0..24)
        .map(|i| {
            let a = TAU * i as f64 / 24.0;
            [30.0 * a.cos(), 20.0 * a.sin() + 3.0 * (3.0 * a).sin()]
        })
        .collect();
    let disc = extrude(&mut k, world(), periodic_spline(&pts), 10.0);
    let centre = P3::new(33.0, 9.0, 5.0);
    let c = periodic_curve(&pts);
    let (t0, t1) = c.range();
    let plan = |t: f64| {
        let q = c.point(t);
        (q.x - centre.x).hypot(q.y - centre.y)
    };
    let n = 200_000;
    let at = |i: usize| t0 + (t1 - t0) * i as f64 / n as f64;
    let i = (0..=n)
        .min_by(|&i, &j| plan(at(i)).total_cmp(&plan(at(j))))
        .unwrap();
    let (mut lo, mut hi) = (at(i.saturating_sub(1)), at((i + 1).min(n)));
    let g = 0.5 * (5.0f64.sqrt() - 1.0);
    for _ in 0..200 {
        let (x1, x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
        if plan(x1) < plan(x2) {
            hi = x2;
        } else {
            lo = x1;
        }
    }
    let want = plan(0.5 * (lo + hi)) - 1.0;
    let s = ball_at(&mut k, centre, 1.0);
    assert_distance(bodies(&k, s, disc), want);
}

/// A plate 30 square and 2 thick with a hole of radius 10 through its middle.
fn drilled_plate(k: &mut NativeKernel) -> BodyId {
    let plate = pblock(k, &world(), [30.0, 30.0, 2.0]);
    let at = frame(P3::new(15.0, 15.0, -1.0), V3::z(), V3::x());
    let hole = cylinder(k, &at, 10.0, 4.0);
    difference(k, plate, hole)
}

#[test]
fn a_point_just_inside_a_holes_rim_is_as_far_as_the_rim_not_on_the_face() {
    let mut k = NativeKernel::new();
    let plate = drilled_plate(&mut k);
    for (off, want) in [(-0.005, 0.005), (-0.001, 0.001), (0.003, 0.0)] {
        for turn in [0.1f64, 1.3, 2.9, 4.4] {
            let r = 10.0 + off;
            let p = P3::new(15.0 + r * turn.cos(), 15.0 + r * turn.sin(), 2.0);
            let got = k
                .min_distance(&DistanceTarget::Point(p), &DistanceTarget::Body(plate))
                .unwrap()
                .distance;
            assert!(
                (got - want).abs() < 1e-7,
                "{off} at {turn}: {got} against {want}"
            );
        }
    }
}

#[test]
fn a_dowel_in_a_hole_is_as_far_from_the_plate_as_its_clearance() {
    let mut k = NativeKernel::new();
    let plate = drilled_plate(&mut k);
    let at = frame(P3::new(15.0, 15.0, -3.0), V3::z(), V3::x());
    for clearance in [0.002, 0.02, 0.2] {
        let dowel = cylinder(&mut k, &at, 10.0 - clearance, 8.0);
        let c = bodies(&k, plate, dowel);
        assert!(
            (c.distance - clearance).abs() < 1e-7,
            "clearance {clearance}: {}",
            c.distance
        );
    }
}

/// springrep's `measure::face_at(b, p)`: the face whose own region holds the foot of `p` on its
/// surface, the nearest such, and how far. Here: each face's surface foot of `p`, kept where the
/// face holds it (its distance to the face is nil).
fn face_at_point(k: &NativeKernel, b: BodyId, p: P3) -> Option<(FaceId, f64)> {
    let body = k.get(b).unwrap();
    let mut best: Option<(FaceId, f64)> = None;
    for (i, f) in body.faces.iter().enumerate() {
        let (u, v) = f.surf.project(&p, None);
        let foot = f.surf.eval(u, v);
        let id = FaceId(i as u64);
        let on = k
            .min_distance(&DistanceTarget::Point(foot), &DistanceTarget::Face(b, id))
            .unwrap()
            .distance;
        if on > 1e-9 {
            continue;
        }
        let d = (foot - p).norm();
        if best.is_none_or(|(_, e)| d < e) {
            best = Some((id, d));
        }
    }
    best
}

#[test]
fn the_face_at_a_point_just_inside_a_holes_rim_is_the_holes_wall() {
    let mut k = NativeKernel::new();
    let plate = drilled_plate(&mut k);
    let kind = |k: &NativeKernel, f: FaceId| k.faces(plate).unwrap()[f.0 as usize].kind;
    for turn in [0.1f64, 1.3, 2.9, 4.4] {
        let r = 10.0 - 0.005;
        let p = P3::new(15.0 + r * turn.cos(), 15.0 + r * turn.sin(), 2.0);
        let (face, d) = face_at_point(&k, plate, p).unwrap();
        assert_eq!(kind(&k, face), SurfaceKind::Cylinder, "at {turn}");
        assert!((d - 0.005).abs() < 1e-9, "at {turn}: {d}");
        let r = 10.0 + 0.005;
        let p = P3::new(15.0 + r * turn.cos(), 15.0 + r * turn.sin(), 2.0);
        let (face, d) = face_at_point(&k, plate, p).unwrap();
        assert_eq!(kind(&k, face), SurfaceKind::Plane, "at {turn}");
        assert!(d < 1e-9, "at {turn}: {d}");
    }
}
