//! Ported from springrep's tests/touching.rs (MIT, https://github.com/springcraft-ai/springrep):
//! results whose pieces touch, or that touch themselves, at points or along curves: the
//! regularized set operation, which need not be a manifold. Touching pieces are one solid, its
//! shells joined through the edges and vertices the contact makes, an edge there used by four
//! faces and a vertex by two fans. Each result is checked valid and measured against closed forms.
//!
//! springrep's Betti numbers (`measure::betti`) have no counterpart in cadrs and are left out;
//! its edges four faces use (radial edges) are counted here by geometry: edges along one curve,
//! their face uses added up.

mod springrep_port;

use std::f64::consts::PI;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

/// springrep's test `cone`: base radius `r`, height `h`, standing on its apex at `apex`, opening
/// along `up` (±1): the meridian (0,0), (r, up·h), (0, up·h) on (apex, z = −Y, x = X) about Z.
fn cone_at(k: &mut NativeKernel, apex: [f64; 3], up: f64, r: f64, h: f64) -> BodyId {
    let l = Loop::new(vec![
        Curve2::line([0.0, 0.0], [r, up * h]),
        Curve2::line([r, up * h], [0.0, up * h]),
        Curve2::line([0.0, up * h], [0.0, 0.0]),
    ]);
    let c = p3(apex);
    let f = frame(c, -V3::y(), V3::x());
    revolve(k, f, l, c, V3::z())
}

fn cone_measures(r: f64, h: f64) -> (f64, f64) {
    (
        PI * r * r * h / 3.0,
        PI * r * r + PI * r * (r * r + h * h).sqrt(),
    )
}

/// One valid solid of the volume and area given, meshed watertight. (springrep's Betti numbers:
/// no counterpart.)
fn assert_touching(k: &mut NativeKernel, b: BodyId, vol: f64, ar: f64) {
    valid(k, b);
    let (v, a) = (volume(k, b), area(k, b));
    assert!((v - vol).abs() <= 1e-9 * vol, "volume {v} vs {vol}");
    assert!((a - ar).abs() <= 1e-9 * ar, "area {a} vs {ar}");
    watertight(k, b);
    // Last, so that the measures above are reported first.
    assert_eq!(solids(k, Some(b)), 1, "touching pieces are one solid");
}

fn vertices_at(k: &NativeKernel, b: BodyId, at: P3) -> usize {
    k.vertices(b)
        .unwrap()
        .iter()
        .filter(|v| (v.point - at).norm() < 1e-9)
        .count()
}

/// The two 10 mm cubes sharing the vertical edge x = y = 10.
fn edge_pair(k: &mut NativeKernel) -> BodyId {
    let a = block(k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let b = block(k, [10.0, 10.0, 0.0], [20.0, 20.0, 10.0]);
    union(k, a, b)
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn blocks_touching_along_an_edge_unite_round_it() {
    let mut k = NativeKernel::new();
    let u = edge_pair(&mut k);
    assert_touching(&mut k, u, 2000.0, 1200.0);
    assert_eq!(four_face_edges(&k, u), 1);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn blocks_touching_at_a_corner_unite_there() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let b = block(&mut k, [10.0, 10.0, 10.0], [20.0, 20.0, 20.0]);
    let u = union(&mut k, a, b);
    assert_touching(&mut k, u, 2000.0, 1200.0);
    assert_eq!(vertices_at(&k, u, P3::new(10.0, 10.0, 10.0)), 1);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_rod_lying_on_a_box_face_unites_along_the_line() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [20.0, 10.0, 10.0]);
    let c = rod(&mut k, [5.0, 5.0, 12.0], [1.0, 0.0, 0.0], 2.0, 10.0);
    let u = union(&mut k, a, c);
    let rod_area = 2.0 * PI * 4.0 + 2.0 * PI * 2.0 * 10.0;
    assert_touching(&mut k, u, 2000.0 + PI * 4.0 * 10.0, 1000.0 + rod_area);
    assert_eq!(four_face_edges(&k, u), 1);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_box_less_an_inscribed_cylinder_is_four_corners_in_a_ring() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 8.0]);
    let c = rod(&mut k, [5.0, 5.0, -1.0], [0.0, 0.0, 1.0], 5.0, 10.0);
    let d = difference(&mut k, a, c);
    let ar = 2.0 * (100.0 - PI * 25.0) + 4.0 * 80.0 + 2.0 * PI * 5.0 * 8.0;
    assert_touching(&mut k, d, 800.0 - PI * 25.0 * 8.0, ar);
    assert_eq!(four_face_edges(&k, d), 4);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_block_less_a_bore_flush_to_its_top_touches_itself_along_a_line() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [12.0, 10.0, 8.0]);
    let c = rod(&mut k, [-1.0, 5.0, 5.0], [1.0, 0.0, 0.0], 3.0, 14.0);
    let d = difference(&mut k, a, c);
    let ar = 2.0 * (120.0 + 96.0 + 80.0) - 2.0 * PI * 9.0 + 2.0 * PI * 3.0 * 12.0;
    assert_touching(&mut k, d, 960.0 - PI * 9.0 * 12.0, ar);
    assert_eq!(four_face_edges(&k, d), 1);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_rod_inside_a_box_against_a_wall_leaves_a_void_touching_it() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let c = rod(&mut k, [2.0, 5.0, 2.0], [0.0, 0.0, 1.0], 2.0, 6.0);
    let d = difference(&mut k, a, c);
    let ar = 600.0 + 2.0 * PI * 4.0 + 2.0 * PI * 2.0 * 6.0;
    assert_touching(&mut k, d, 1000.0 - PI * 4.0 * 6.0, ar);
    assert_eq!(four_face_edges(&k, d), 1);
}

#[test]
fn a_cube_less_a_ball_touching_its_six_faces_keeps_the_void() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let s = ball(&mut k, [5.0, 5.0, 5.0], 5.0);
    let d = difference(&mut k, a, s);
    assert_touching(
        &mut k,
        d,
        1000.0 - 4.0 / 3.0 * PI * 125.0,
        600.0 + 4.0 * PI * 25.0,
    );
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_touching_result_combined_with_itself_keeps_its_contact() {
    let mut k = NativeKernel::new();
    let u = edge_pair(&mut k);
    for op in [BoolOp::Intersect, BoolOp::Union] {
        let u2 = moved(&mut k, u, &Transform::identity());
        let x = boolean(&mut k, op, u, u2).unwrap();
        assert_touching(&mut k, x, 2000.0, 1200.0);
        assert_eq!(four_face_edges(&k, x), 1, "{op:?}");
    }
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn balls_touching_at_a_point_unite_there() {
    let mut k = NativeKernel::new();
    let a = ball(&mut k, [5.0, 0.0, 0.0], 5.0);
    let b = ball(&mut k, [15.0, 0.0, 0.0], 5.0);
    let u = union(&mut k, a, b);
    let (v, s) = (4.0 / 3.0 * PI * 125.0, 4.0 * PI * 25.0);
    assert_touching(&mut k, u, 2.0 * v, 2.0 * s);
    assert_eq!(vertices_at(&k, u, P3::new(10.0, 0.0, 0.0)), 1);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_ball_on_a_box_face_unites_at_a_point() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let s = ball(&mut k, [5.0, 5.0, 13.0], 3.0);
    let u = union(&mut k, a, s);
    let (v, ar) = (4.0 / 3.0 * PI * 27.0, 4.0 * PI * 9.0);
    assert_touching(&mut k, u, 1000.0 + v, 600.0 + ar);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn balls_touching_at_a_point_combine_as_inputs() {
    let mut k = NativeKernel::new();
    let a = ball(&mut k, [5.0, 0.0, 0.0], 5.0);
    let b = ball(&mut k, [15.0, 0.0, 0.0], 5.0);
    let u = union(&mut k, a, b);
    let (v, s) = (4.0 / 3.0 * PI * 125.0, 4.0 * PI * 25.0);
    // Combined with itself, the contact is kept.
    for op in [BoolOp::Intersect, BoolOp::Union] {
        let u2 = moved(&mut k, u, &Transform::identity());
        let x = boolean(&mut k, op, u, u2).unwrap();
        assert_touching(&mut k, x, 2.0 * v, 2.0 * s);
    }
    // A third ball touching the second at (20, 0, 0) joins the chain at a second point.
    let c = ball(&mut k, [25.0, 0.0, 0.0], 5.0);
    let chain = union(&mut k, u, c);
    assert_touching(&mut k, chain, 3.0 * v, 3.0 * s);
    // A slab through the contact takes a cap of height 1 off each ball, parting them.
    let slab = block(&mut k, [9.0, -6.0, -6.0], [11.0, 6.0, 6.0]);
    let d = difference(&mut k, u, slab);
    valid(&k, d);
    assert_eq!(solids(&k, Some(d)), 2);
    // (Betti numbers [2, 0, 0]: no counterpart.)
    let cap = PI * (3.0 * 5.0 - 1.0) / 3.0;
    let got = volume(&k, d);
    assert!(
        (got - 2.0 * (v - cap)).abs() < 1e-9 * v,
        "{got} vs {}",
        2.0 * (v - cap)
    );
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_cone_standing_on_its_apex_unites_with_the_face_under_it() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [-10.0, -10.0, -10.0], [10.0, 10.0, 0.0]);
    let c = cone_at(&mut k, [0.0, 0.0, 0.0], 1.0, 3.0, 6.0);
    let u = union(&mut k, a, c);
    let (v, s) = cone_measures(3.0, 6.0);
    assert_touching(&mut k, u, 4000.0 + v, 1600.0 + s);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn cones_touching_apex_to_apex_unite_there() {
    let mut k = NativeKernel::new();
    let a = cone_at(&mut k, [0.0, 0.0, 0.0], 1.0, 3.0, 6.0);
    let b = cone_at(&mut k, [0.0, 0.0, 0.0], -1.0, 2.0, 5.0);
    let u = union(&mut k, a, b);
    let (va, sa) = cone_measures(3.0, 6.0);
    let (vb, sb) = cone_measures(2.0, 5.0);
    assert_touching(&mut k, u, va + vb, sa + sb);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_cone_on_a_ball_unites_at_its_apex() {
    let mut k = NativeKernel::new();
    let a = ball(&mut k, [0.0, 0.0, -4.0], 4.0);
    let c = cone_at(&mut k, [0.0, 0.0, 0.0], 1.0, 3.0, 6.0);
    let u = union(&mut k, a, c);
    let (v, s) = cone_measures(3.0, 6.0);
    assert_touching(&mut k, u, 4.0 / 3.0 * PI * 64.0 + v, 4.0 * PI * 16.0 + s);
}

#[test]
fn filling_the_corner_between_touching_blocks_makes_one_manifold_piece() {
    let mut k = NativeKernel::new();
    let u = edge_pair(&mut k);
    let c = block(&mut k, [5.0, 5.0, 0.0], [15.0, 15.0, 10.0]);
    let filled = union(&mut k, u, c);
    let ar = 2.0 * 250.0 + 10.0 * 80.0;
    assert_touching(&mut k, filled, 2500.0, ar);
    assert_eq!(four_face_edges(&k, filled), 0);
}

#[test]
fn cutting_the_contact_away_parts_the_pieces() {
    let mut k = NativeKernel::new();
    let u = edge_pair(&mut k);
    let c = block(&mut k, [8.0, 8.0, -1.0], [12.0, 12.0, 11.0]);
    let d = difference(&mut k, u, c);
    valid(&k, d);
    assert_eq!(solids(&k, Some(d)), 2);
    let got = volume(&k, d);
    assert!((got - 2.0 * 960.0).abs() < 1e-9, "{got}");
    assert_eq!(four_face_edges(&k, d), 0);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn cutting_across_the_contact_keeps_it_where_it_is_left() {
    let mut k = NativeKernel::new();
    let u = edge_pair(&mut k);
    let c = block(&mut k, [-1.0, -1.0, 5.0], [21.0, 21.0, 11.0]);
    let d = difference(&mut k, u, c);
    assert_touching(&mut k, d, 1000.0, 2.0 * (200.0 + 200.0));
    assert_eq!(four_face_edges(&k, d), 1);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_touching_result_with_its_contact_cut_into() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 8.0]);
    let c = rod(&mut k, [5.0, 5.0, -1.0], [0.0, 0.0, 1.0], 5.0, 10.0);
    let ring = difference(&mut k, a, c);
    let slot = block(&mut k, [9.0, 4.0, -1.0], [11.0, 6.0, 9.0]);
    let d = difference(&mut k, ring, slot);
    valid(&k, d);
    assert_eq!(solids(&k, Some(d)), 1);
    // (Betti numbers [1, 0, 0]: no counterpart.)
    assert_eq!(four_face_edges(&k, d), 3);
}

/// The edge of `b` whose middle is at `mid`.
fn edge_at(k: &NativeKernel, b: BodyId, mid: P3) -> EdgeId {
    k.edges(b)
        .unwrap()
        .into_iter()
        .find(|e| (e.mid - mid).norm() < 1e-9)
        .expect("no edge there")
        .id
}

/// springrep refuses with `Unsupported("… touch …")`; the message is springrep's own, so any
/// refusal counts here.
fn refused(r: Result<OpResult>, k: &NativeKernel) {
    if let Ok(r) = r {
        panic!(
            "expected a refusal, got volume {:?}",
            r.bodies.first().map(|&b| volume(k, b))
        );
    }
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_blend_along_a_contact_is_refused() {
    // The edge the cubes share is the contact: a round there would cut into the other cube.
    let mut k = NativeKernel::new();
    let u = edge_pair(&mut k);
    let contact = edge_at(&k, u, P3::new(10.0, 10.0, 5.0));
    let r = k.fillet(u, &[contact], 1.0);
    refused(r, &k);
    let r = k.chamfer(u, &[contact], ChamferSpec::EqualDistance(1.0));
    refused(r, &k);
    // An edge away from the contact still blends: a's edge x = y = 0.
    let away = edge_at(&k, u, P3::new(0.0, 0.0, 5.0));
    let f = k.fillet(u, &[away], 1.0).unwrap().first().unwrap();
    valid(&k, f);
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn moving_a_face_at_a_contact_is_refused() {
    // a's face x = 10 (outward +x) ends at the contact: moving it opens or overlaps it.
    let mut k = NativeKernel::new();
    let u = edge_pair(&mut k);
    let face = |k: &NativeKernel, n: V3, x: f64| {
        k.faces(u)
            .unwrap()
            .into_iter()
            .find(|f| {
                f.plane.is_some_and(|p| p.normal.dot(&n) > 1.0 - 1e-9)
                    && (f.center.x - x).abs() < 1e-9
                    && f.center.y < 10.0
            })
            .expect("no such face")
            .id
    };
    let u_max = face(&k, V3::x(), 10.0);
    let spec = |f: FaceId| OffsetSpec {
        distance: 0.0,
        faces: vec![(f, 1.0)],
        sharp: true,
    };
    let r = k.offset(u, &spec(u_max));
    refused(r, &k);
    let u_min = face(&k, -V3::x(), 0.0);
    let m = k.offset(u, &spec(u_min)).unwrap().first().unwrap();
    valid(&k, m);
    let got = volume(&k, m);
    assert!((got - 2100.0).abs() < 1e-9, "{got}");
}

#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn a_face_a_rod_touches_through_a_hole_splits_round_the_hole() {
    let mut k = NativeKernel::new();
    let blk = block(&mut k, [0.0, 0.0, 0.0], [40.0, 20.0, 20.0]);
    let hole = rod(&mut k, [20.0, -1.0, 10.0], [0.0, 1.0, 0.0], 2.0, 22.0);
    let holed = difference(&mut k, blk, hole);
    let standing = rod(&mut k, [20.0, -3.0, 5.0], [0.0, 0.0, 1.0], 3.0, 15.0);
    let touched = union(&mut k, holed, standing);
    valid(&k, touched);
    let left = block(&mut k, [-1.0, -10.0, -1.0], [30.0, 30.0, 21.0]);
    let kept = intersection(&mut k, touched, left);
    valid(&k, kept);
    assert_eq!(solids(&k, Some(kept)), 1);
    let want = 12_000.0 - 80.0 * PI + 135.0 * PI;
    let got = volume(&k, kept);
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");
}
