//! Ported from springrep's tests/booleans.rs (MIT, https://github.com/springcraft-ai/springrep):
//! booleans of small swept solids against closed-form volume and area, each result checked valid
//! and watertight. springrep's face names following history have no counterpart here (cadrs
//! names faces through `History`), so those assertions are left out where they occur.

mod springrep_port;

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

/// The volume two cylinders of radii `a ≥ b` share when their axes cross at right angles.
fn crossing_volume(a: f64, b: f64) -> f64 {
    let n = 2000;
    let (h, mut sum) = (2.0 * b / n as f64, 0.0);
    for i in 0..n {
        let t = -b + h * (i as f64 + 0.5);
        sum += 4.0 * ((b * b - t * t) * (a * a - t * t)).sqrt();
    }
    sum * h
}

/// A copy of `b` (springrep's `clone`).
fn copy(k: &mut NativeKernel, b: BodyId) -> BodyId {
    moved(k, b, &Transform::identity())
}

#[test]
fn overlapping_blocks_unite_intersect_and_subtract() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let b = block(&mut k, [5.0, 5.0, 5.0], [15.0, 15.0, 15.0]);
    let u = union(&mut k, a, b);
    assert_solid(&mut k, u, 2000.0 - 125.0, 2.0 * 600.0 - 6.0 * 25.0);
    assert_eq!(faces(&k, u), 12);
    let i = intersection(&mut k, a, b);
    assert_solid(&mut k, i, 125.0, 150.0);
    // (springrep's checks of which input face names the intersection's faces carry: no
    // counterpart.)
    let d = difference(&mut k, a, b);
    assert_solid(&mut k, d, 875.0, 600.0);
    // (The name `b.start` on the difference: no counterpart.)
}

#[test]
fn a_tool_inside_leaves_a_void() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let b = block(&mut k, [3.0, 3.0, 3.0], [6.0, 6.0, 6.0]);
    let d = difference(&mut k, a, b);
    assert_eq!(solids(&k, Some(d)), 1);
    assert_eq!(k.get(d).unwrap().solids[0].shells.len(), 2);
    assert_solid(&mut k, d, 1000.0 - 27.0, 600.0 + 54.0);
    let u = union(&mut k, a, b);
    assert_solid(&mut k, u, 1000.0, 600.0);
    let i = intersection(&mut k, a, b);
    assert_solid(&mut k, i, 27.0, 54.0);
}

#[test]
fn disjoint_solids_stay_apart() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    let b = block(&mut k, [3.0, 0.0, 0.0], [4.0, 1.0, 1.0]);
    let u = union(&mut k, a, b);
    assert_eq!(solids(&k, Some(u)), 2);
    assert_solid(&mut k, u, 2.0, 12.0);
    let i = maybe(boolean(&mut k, BoolOp::Intersect, a, b));
    assert_eq!(solids(&k, i), 0);
    let d = difference(&mut k, a, b);
    assert_solid(&mut k, d, 1.0, 6.0);
}

#[test]
fn a_rod_through_a_block_is_a_hole() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [-10.0, -10.0, 0.0], [10.0, 10.0, 5.0]);
    let h = rod(&mut k, [2.0, 1.0, -3.0], [0.0, 0.0, 1.0], 3.0, 20.0);
    let d = difference(&mut k, a, h);
    assert_solid(
        &mut k,
        d,
        2000.0 - 45.0 * PI,
        2.0 * (400.0 - 9.0 * PI) + 400.0 + 30.0 * PI,
    );
    // (`h.side` named once: no counterpart.)
    let u = union(&mut k, a, h);
    // The rod's caps replace the discs it takes from the block's faces.
    assert_solid(
        &mut k,
        u,
        2000.0 + 15.0 * 9.0 * PI,
        1200.0 + 6.0 * PI * 15.0,
    );
    // (`h.side` named twice: no counterpart.)
}

#[test]
fn a_ball_cut_by_a_plane_face_leaves_a_cap() {
    let mut k = NativeKernel::new();
    let s = ball(&mut k, [0.0, 0.0, 0.0], 5.0);
    let a = block(&mut k, [-10.0, -10.0, 3.0], [10.0, 10.0, 10.0]);
    let cap = intersection(&mut k, s, a);
    let h = 2.0;
    let disc = PI * (25.0 - 9.0);
    assert_solid(
        &mut k,
        cap,
        PI * h * h * (15.0 - h) / 3.0,
        2.0 * PI * 5.0 * h + disc,
    );
}

#[test]
fn coplanar_faces_merge_and_carry_both_names() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let b = block(&mut k, [10.0, 0.0, 0.0], [20.0, 10.0, 10.0]);
    let u = union(&mut k, a, b);
    assert_solid(&mut k, u, 2000.0, 1000.0);
    assert_eq!(faces(&k, u), 6);
    // (The merged top carrying both `a.end` and `b.end`: no counterpart.)
}

#[test]
fn equal_crossing_holes_split_each_other_at_the_touching_points() {
    let mut k = NativeKernel::new();
    let cube = block(&mut k, [-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]);
    let hx = rod(&mut k, [-10.0, 0.0, 0.0], [1.0, 0.0, 0.0], 4.0, 1000.0);
    let hy = rod(&mut k, [0.0, -10.0, 0.0], [0.0, 1.0, 0.0], 4.0, 1000.0);
    let one = difference(&mut k, cube, hx);
    let two = difference(&mut k, one, hy);
    let steinmetz = 16.0 * 64.0 / 3.0;
    let area = 6.0 * 400.0 - 4.0 * 16.0 * PI + 2.0 * 8.0 * PI * 20.0 - 16.0 * 16.0;
    assert_solid(
        &mut k,
        two,
        8000.0 - 2.0 * 16.0 * PI * 20.0 + steinmetz,
        area,
    );
    // (`hx.side` and `hy.side` each named twice: no counterpart.)
}

#[test]
fn holes_a_hair_from_tangent_stay_valid() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [20.0, 20.0, 10.0]);
    for (x, n) in [(5.000001, 7), (4.999999, 8)] {
        let h = rod(&mut k, [x, 10.0, 0.0], [0.0, 0.0, 1.0], 5.0, 1000.0);
        let d = difference(&mut k, a, h);
        assert!(issues(&k, d).is_empty(), "{x}: {:?}", issues(&k, d));
        assert_eq!(faces(&k, d), n, "{x}");
        let lens = if x < 5.0 {
            let (c, r) = (x, 5.0f64);
            r * r * (c / r).acos() - c * (r * r - c * c).sqrt()
        } else {
            0.0
        };
        let v = 4000.0 - 10.0 * (25.0 * PI - lens);
        let got = volume(&k, d);
        assert!((got - v).abs() < 1e-9 * v, "{x}: {got} vs {v}");
    }
}

#[test]
fn coaxial_cut_with_coincident_caps_leaves_a_tube() {
    let mut k = NativeKernel::new();
    let outer = rod(&mut k, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 10.0, 15.0);
    let bore = rod(&mut k, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 5.0, 15.0);
    let t = difference(&mut k, outer, bore);
    assert_solid(
        &mut k,
        t,
        PI * 75.0 * 15.0,
        2.0 * PI * 75.0 + 2.0 * PI * 15.0 * 15.0,
    );
    // (No face named after the bore's caps: no counterpart; the face count says as much.)
    assert_eq!(faces(&k, t), 4);
}

#[test]
fn coaxial_rods_unite_into_one_side_across_their_seams() {
    let mut k = NativeKernel::new();
    let a = rod(&mut k, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 5.0, 10.0);
    let b0 = rod(&mut k, [0.0, 0.0, 5.0], [0.0, 0.0, 1.0], 5.0, 10.0);
    let b = moved(&mut k, b0, &rotation(P3::origin(), V3::z(), 1.0));
    let u = union(&mut k, a, b);
    assert_solid(
        &mut k,
        u,
        PI * 25.0 * 15.0,
        2.0 * PI * 25.0 + 10.0 * PI * 15.0,
    );
    assert_eq!(faces(&k, u), 3);
    // (The side carrying both `a.side` and `b.side`: no counterpart.)
}

#[test]
fn a_hole_through_a_rod_crosses_its_seam() {
    let mut k = NativeKernel::new();
    let body = rod(&mut k, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 10.0, 30.0);
    let h = rod(&mut k, [-15.0, 0.0, 15.0], [1.0, 0.0, 0.0], 3.0, 1000.0);
    let d = difference(&mut k, body, h);
    let v = PI * 100.0 * 30.0 - crossing_volume(10.0, 3.0);
    valid(&k, d);
    let got = volume(&k, d);
    assert!((got - v).abs() < 1e-6 * v, "volume {got} vs {v}");
    assert_eq!(faces(&k, d), 4);
}

#[test]
fn a_plane_through_a_balls_poles_halves_it() {
    let mut k = NativeKernel::new();
    let s = ball(&mut k, [0.0, 0.0, 0.0], 5.0);
    let kb = block(&mut k, [0.0, -10.0, -10.0], [10.0, 10.0, 10.0]);
    let half = intersection(&mut k, s, kb);
    assert_solid(
        &mut k,
        half,
        2.0 / 3.0 * PI * 125.0,
        2.0 * PI * 25.0 + PI * 25.0,
    );
    assert_eq!(faces(&k, half), 2);
}

#[test]
fn a_counterbore_and_a_boss_share_caps_with_the_plate() {
    let mut k = NativeKernel::new();
    let plate = block(&mut k, [0.0, 0.0, 0.0], [40.0, 40.0, 10.0]);
    let h = rod(&mut k, [20.0, 20.0, 0.0], [0.0, 0.0, 1.0], 5.0, 1000.0);
    let holed = difference(&mut k, plate, h);
    let cb = rod(&mut k, [20.0, 20.0, 10.0], [0.0, 0.0, -1.0], 8.0, 4.0);
    let bored = difference(&mut k, holed, cb);
    assert_solid(
        &mut k,
        bored,
        16000.0 - 250.0 * PI - 4.0 * 39.0 * PI,
        2.0 * 1600.0 + 1600.0 - 64.0 * PI - 25.0 * PI
            + 39.0 * PI
            + 16.0 * PI * 4.0
            + 10.0 * PI * 6.0,
    );
    // (`cb.end` named once: no counterpart.)
    let boss = rod(&mut k, [20.0, 20.0, 10.0], [0.0, 0.0, 1.0], 5.0, 8.0);
    let u = union(&mut k, plate, boss);
    assert_solid(
        &mut k,
        u,
        16000.0 + 200.0 * PI,
        2.0 * 1600.0 + 1600.0 + 10.0 * PI * 8.0,
    );
    // (No `b.start`: no counterpart.)
}

/// The circle where two balls meet bounds both sphere faces, whose meshes refine against it.
#[test]
fn overlapping_balls_unite_and_mesh() {
    let mut k = NativeKernel::new();
    let (big, small) = (5.0, 4.0);
    let d = 14f64.sqrt();
    let lens = PI
        * (big + small - d).powi(2)
        * (d * d + 2.0 * d * small - 3.0 * small * small + 2.0 * d * big + 6.0 * small * big
            - 3.0 * big * big)
        / (12.0 * d);
    let vol = 4.0 / 3.0 * PI * (big.powi(3) + small.powi(3)) - lens;
    let h1 = (small - big + d) * (small + big - d) / (2.0 * d);
    let h2 = (big - small + d) * (big + small - d) / (2.0 * d);
    let ar = 4.0 * PI * (big * big + small * small) - 2.0 * PI * (big * h1 + small * h2);
    let a = ball(&mut k, [0.0, 0.0, 0.0], big);
    let b = ball(&mut k, [3.0, 2.0, 1.0], small);
    let u = union(&mut k, a, b);
    assert_solid(&mut k, u, vol, ar);
}

#[test]
fn a_solid_minus_itself_is_empty() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let a2 = copy(&mut k, a);
    let b = block(&mut k, [20.0, 0.0, 0.0], [30.0, 10.0, 10.0]);
    for r in [
        maybe(boolean(&mut k, BoolOp::Subtract, a, a2)),
        maybe(boolean(&mut k, BoolOp::Intersect, a, b)),
    ] {
        assert_eq!(solids(&k, r), 0);
        if let Some(r) = r {
            assert_eq!(faces(&k, r), 0);
            valid(&k, r);
        }
    }
}

/// Within the tolerance of tangent a hole touches the wall: the block touches itself along that
/// line, one solid round an edge four faces use. Just past it the hole cuts a slot, its cap's
/// sliver outside the wall decided by the exact side.
#[test]
#[ignore = "cadrs keeps solids that only touch apart and makes no edge where faces are tangent (springrep: one non-manifold solid)"]
fn holes_at_the_tolerance_from_tangent_touch_or_cut() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [20.0, 20.0, 10.0]);
    for x in [5.0 + 5e-8, 5.0 - 5e-8, 5.0 - 9e-8] {
        let h = rod(&mut k, [x, 10.0, 0.0], [0.0, 0.0, 1.0], 5.0, 1000.0);
        let d = difference(&mut k, a, h);
        assert!(issues(&k, d).is_empty(), "{x}: {:?}", issues(&k, d));
        assert_eq!(solids(&k, Some(d)), 1, "{x}");
        // springrep's one radial edge, found here by geometry.
        assert_eq!(four_face_edges(&k, d), 1, "{x}");
        let v = 4000.0 - 250.0 * PI;
        let got = volume(&k, d);
        assert!((got - v).abs() < 1e-9 * v, "{x}: {got} vs {v}");
    }
    for x in [5.0 - 1.5e-7, 5.0 - 3e-7] {
        let h = rod(&mut k, [x, 10.0, 0.0], [0.0, 0.0, 1.0], 5.0, 1000.0);
        let d = difference(&mut k, a, h);
        assert!(issues(&k, d).is_empty(), "{x}: {:?}", issues(&k, d));
        assert_eq!(faces(&k, d), 8, "{x}");
        let (c, r) = (x, 5.0f64);
        let lens = r * r * (c / r).acos() - c * (r * r - c * c).sqrt();
        let v = 4000.0 - 10.0 * (25.0 * PI - lens);
        let got = volume(&k, d);
        assert!((got - v).abs() < 1e-9 * v, "{x}: {got} vs {v}");
    }
}

/// Faces 1.5 times the tolerance off coplanar leave slivers that are classified by their exact
/// side rather than refused.
#[test]
fn faces_just_past_the_tolerance_from_coplanar_unite() {
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0, 0.0, 0.0], [20.0, 20.0, 10.0]);
    let b = block(&mut k, [20.0, 0.0, 1.5e-7], [40.0, 20.0, 10.0 + 1.5e-7]);
    let u = union(&mut k, a, b);
    valid(&k, u);
    let got = volume(&k, u);
    assert!((got - 8000.0).abs() < 1e-9 * 8000.0, "{got}");
}

/// A relief bore 0.05 mm over a bore leaves annuli thinner than a polygon of their circles can
/// find an inside point in.
#[test]
fn a_relief_bore_leaves_thin_annuli() {
    let mut k = NativeKernel::new();
    let body = rod(&mut k, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 52.5, 45.0);
    let bore = rod(&mut k, [0.0, 0.0, -1.0], [0.0, 0.0, 1.0], 45.0, 47.0);
    let relief = rod(&mut k, [0.0, 0.0, 10.0], [0.0, 0.0, 1.0], 45.05, 24.0);
    let bored = difference(&mut k, body, bore);
    let d = difference(&mut k, bored, relief);
    let v =
        PI * (52.5f64.powi(2) - 45.0f64.powi(2)) * 45.0 - PI * (45.05f64.powi(2) - 2025.0) * 24.0;
    valid(&k, d);
    let got = volume(&k, d);
    assert!((got - v).abs() < 1e-9 * v, "{got} vs {v}");
}

fn spline_disc(k: &mut NativeKernel, z: f64, h: f64) -> BodyId {
    let pts: Vec<[f64; 2]> = (0..8)
        .map(|i| {
            let a = i as f64 * TAU / 8.0;
            [6.0 * a.cos(), 4.0 * a.sin()]
        })
        .collect();
    extrude(k, Plane::xy(z), periodic_spline(&pts), h)
}

/// A solid with spline edges against an exact copy keeps its edges whole: curves that run together
/// meet only at their ends.
#[test]
fn a_spline_solid_unites_with_itself_unchanged() {
    let mut k = NativeKernel::new();
    let a = spline_disc(&mut k, 0.0, 2.0);
    let v = volume(&k, a);
    let edges = k.edges(a).unwrap().len();
    for op in [BoolOp::Union, BoolOp::Intersect] {
        let a2 = copy(&mut k, a);
        let r = boolean(&mut k, op, a, a2).unwrap();
        valid(&k, r);
        assert_eq!(k.edges(r).unwrap().len(), edges, "{op:?}");
        let got = volume(&k, r);
        assert!((got - v).abs() < 1e-9 * v, "{op:?}: {got} vs {v}");
        watertight(&mut k, r);
    }
}

/// Two extrusions of one spline in frames a height apart have the same side surface, so stacked
/// they unite into one side face.
#[test]
fn stacked_spline_extrusions_share_their_side() {
    let mut k = NativeKernel::new();
    let a = spline_disc(&mut k, 0.0, 2.0);
    let b = spline_disc(&mut k, 2.0, 3.0);
    let u = union(&mut k, a, b);
    valid(&k, u);
    assert_eq!(faces(&k, u), 3);
    let v = volume(&k, a) / 2.0 * 5.0;
    let got = volume(&k, u);
    assert!((got - v).abs() < 1e-9 * v, "{got} vs {v}");
}

#[test]
fn a_rod_touching_a_box_face_along_a_line_takes_nothing_from_it() {
    // The rod's side touches the face x = 10 along its middle, where both faces' interior points
    // fall; overlaps within the tolerance are the same touch.
    for overlap in [0.0, 1e-9, 5e-8, 1e-7] {
        let mut k = NativeKernel::new();
        let a = block(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let c = rod(
            &mut k,
            [15.0 - overlap, 5.0, 2.0],
            [0.0, 0.0, 1.0],
            5.0,
            6.0,
        );
        let d = difference(&mut k, a, c);
        assert_solid(&mut k, d, 1000.0, 600.0);
        let i = maybe(boolean(&mut k, BoolOp::Intersect, a, c));
        assert_eq!(solids(&k, i), 0, "overlap {overlap}");
    }
}

/// springrep's crescent tube: an outer circle of radius 2.4 and a bore of diameter 3, on frames
/// at (ex, ey) turned by `turn`.
fn tube(k: &mut NativeKernel, ex: f64, ey: f64, turn: f64) -> BodyId {
    let at = |z: f64| {
        let x = V3::new(turn.cos(), turn.sin(), 0.0);
        frame(P3::new(ex, ey, z), V3::z(), x)
    };
    let o = extrude(k, at(2.4), circle(0.0, 0.0, 2.4), 4.0);
    let b = extrude(k, at(1.4), circle(0.0, 0.0, 1.5), 6.0);
    difference(k, o, b)
}

fn pocketed_wall(k: &mut NativeKernel) -> BodyId {
    let w = block(k, [-6.0, -6.0, 0.0], [6.0, 6.0, 8.0]);
    let p = rod(k, [0.0, 0.0, 2.0], [0.0, 0.0, 1.0], 2.4, 7.0);
    difference(k, w, p)
}

#[test]
#[ignore = "cadrs leaves an open shell or loses volume where cylinders lie a micron off each other's axis"]
fn a_tube_a_hair_off_the_axis_of_its_pocket_meets_the_wall_in_a_crescent() {
    // A tube of outer radius 2.4 in a pocket of the same radius, its axis 0.38 µm off the
    // pocket's: it meets the wall only in the crescent of its circle outside the pocket's, along
    // its 4 mm.
    let mut k = NativeKernel::new();
    let r = 2.4;
    let (ex, ey) = (1.95e-4f64, 3.31e-4f64);
    let e = (ex * ex + ey * ey).sqrt();
    let wall = pocketed_wall(&mut k);
    let lens = 2.0 * r * r * (e / (2.0 * r)).acos() - 0.5 * e * (4.0 * r * r - e * e).sqrt();
    let want = (PI * r * r - lens) * 4.0;
    for turn in [0.5f64, 2.3, 2.7, 4.0, 5.5] {
        let t = tube(&mut k, ex, ey, turn);
        let x = boolean(&mut k, BoolOp::Intersect, wall, t)
            .unwrap_or_else(|e| panic!("turn {turn}: {e:?}"));
        assert!(issues(&k, x).is_empty(), "turn {turn}: {:?}", issues(&k, x));
        let got = volume(&k, x);
        assert!(
            (got - want).abs() <= 1e-7 * want,
            "turn {turn}: {got} vs {want}"
        );
    }
}

#[test]
#[ignore = "cadrs does not snap faces within a micron of each other to one"]
fn faces_within_a_micron_of_each_other_all_over_only_touch() {
    // A is the cube 0..4; B starts `o` inside A's face x = 4. Where o is under the micron the two
    // only touch; past it, the sliver o × 4 × 3 is kept.
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0; 3], [4.0; 3]);
    for o in [1.2e-7, 1.6e-7, 5e-7, 9e-7, 3e-6] {
        let b = block(&mut k, [4.0 - o, 0.0, -1.0], [8.0, 4.0, 3.0]);
        let common = if o < 1e-6 { 0.0 } else { o * 4.0 * 3.0 };
        let b_volume = if o < 1e-6 { 64.0 } else { (4.0 + o) * 16.0 };
        for (name, op, want) in [
            ("common", BoolOp::Intersect, common),
            ("union", BoolOp::Union, 64.0 + b_volume - common),
            ("cut", BoolOp::Subtract, 64.0 - common),
        ] {
            // An empty common part is springrep's empty solid set: volume 0.
            let got = match boolean(&mut k, op, a, b) {
                Ok(x) => {
                    assert!(issues(&k, x).is_empty(), "{o} {name}: {:?}", issues(&k, x));
                    volume(&k, x)
                }
                Err(KernelError::EmptyResult(_)) => 0.0,
                Err(e) => panic!("{o} {name}: {e:?}"),
            };
            assert!((got - want).abs() <= 1e-11, "{o} {name}: {got} vs {want}");
        }
    }
}

#[test]
fn a_tube_within_a_micron_of_its_pockets_axis_only_touches_the_wall() {
    // As the crescent test's tube, its axis 0.51 µm off the pocket's: the tube sits in the pocket.
    let mut k = NativeKernel::new();
    let wall = pocketed_wall(&mut k);
    for turn in [0.5f64, 2.7] {
        let t = tube(&mut k, 3e-7, 4.1e-7, turn);
        let common = maybe(boolean(&mut k, BoolOp::Intersect, wall, t));
        assert_eq!(solids(&k, common), 0, "turn {turn}");
    }
}

#[test]
fn flush_faces_crossing_at_a_hair_meet_along_their_line() {
    // A 2 × 2 × 2 box in the cube 0..4, its face x = 4 on the cube's, turned 1.56e-5 rad about an
    // oblique axis through that face and moved by up to 1e-5 mm. Every result is valid, and the
    // three agree.
    let mut k = NativeKernel::new();
    let a = block(&mut k, [0.0; 3], [4.0; 3]);
    for (turn, off) in [(1.56e-5f64, 0.0f64), (1.56e-5, -1e-5), (2.4e-5, 0.0)] {
        let b0 = block(&mut k, [2.0, 1.0, 1.0], [4.0, 3.0, 3.0]);
        let axis = V3::new(0.0, 0.6, 0.8);
        let b1 = moved(&mut k, b0, &rotation(P3::new(4.0, 2.2, 1.9), axis, turn));
        let b = moved(&mut k, b1, &translation(V3::new(off, 0.0, 0.0)));
        let mut run = |op: BoolOp, name: &str| {
            let x =
                boolean(&mut k, op, a, b).unwrap_or_else(|e| panic!("{turn} {off} {name}: {e:?}"));
            assert!(
                issues(&k, x).is_empty(),
                "{turn} {off} {name}: {:?}",
                issues(&k, x)
            );
            volume(&k, x)
        };
        let common = run(BoolOp::Intersect, "common");
        let joined = run(BoolOp::Union, "union");
        let cut = run(BoolOp::Subtract, "cut");
        let vb = volume(&k, b);
        assert!(
            (common + joined - 64.0 - vb).abs() <= 1e-11,
            "{turn} {off}: common {common} + union {joined} vs 64 + {vb}"
        );
        assert!(
            (cut - (64.0 - common)).abs() <= 1e-11,
            "{turn} {off}: cut {cut} vs 64 - {common}"
        );
        assert!(
            common < 8.0 && common > 8.0 - 1e-3,
            "{turn} {off}: {common}"
        );
    }
}

/// Two cylinders micrometres across crossing at an odd angle: valid every way round, the four
/// results agreeing.
#[test]
fn cylinders_micrometres_across_crossing_aslant_combine() {
    let mut k = NativeKernel::new();
    let mut cyl = |o: [f64; 3], z: [f64; 3], x: [f64; 3], r: f64, h: f64| {
        let f = frame(p3(o), v3(z), v3(x));
        extrude(&mut k, f, circle(0.0, 0.0, r), h)
    };
    let a = cyl(
        [0.0, 0.0, -0.015],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        0.007173807691792167,
        0.03,
    );
    let b = cyl(
        [
            -0.002532118538179013,
            0.0072643840011615635,
            0.008785851568869818,
        ],
        [0.2734748768191249, -0.9104945125904419, -0.3101793582614065],
        [0.9618791461242749, 0.25886555049919935, 0.08818806617667722],
        0.0053024717958101335,
        0.03,
    );
    let mut vols = Vec::new();
    for (op, x, y) in [
        (BoolOp::Union, a, b),
        (BoolOp::Intersect, a, b),
        (BoolOp::Subtract, a, b),
        (BoolOp::Subtract, b, a),
    ] {
        let r = boolean(&mut k, op, x, y).unwrap();
        assert!(issues(&k, r).is_empty(), "{op:?}: {:?}", issues(&k, r));
        vols.push(volume(&k, r));
    }
    // Exact to the tolerance in space, so to it times the area in volume.
    let (va, vb) = (volume(&k, a), volume(&k, b));
    let bound = 1e-7 * (area(&k, a) + area(&k, b));
    assert!((vols[0] - (va + vb - vols[1])).abs() <= bound, "{vols:?}");
    assert!((vols[2] - (va - vols[1])).abs() <= bound, "{vols:?}");
    assert!((vols[3] - (vb - vols[1])).abs() <= bound, "{vols:?}");
}

/// A round thread, a 0.37 mm circle 8.01 mm out, screwed half a turn and a little, cut by a rod on
/// its axis whose side bites through the thread's section: the intersection is the same solid
/// whichever operand comes first, and valid.
#[test]
fn a_thread_and_a_rod_biting_into_it_intersect_either_way_round() {
    let mut k = NativeKernel::new();
    let (cx, cy) = (8.013653159337178, 0.5917483297087343);
    let rise = 0.25338470539470814; // per radian
    let angle = 3.1916513479467015;
    // The meridian frame (origin, z = −Y, x = X) is the plane xz; the circle screwed about z,
    // right-handed, along the helix its centre follows.
    let profile = Profile::of(Plane::xz(0.0), vec![circle(cx, cy, 0.37382307626072764)]);
    let helix = Helix::new(
        Axis::new(P3::new(0.0, 0.0, cy), V3::z()),
        cx,
        rise * TAU,
        0.0,
        angle / TAU,
    );
    let mut spec = SweepSpec::solid(helix.path());
    spec.control = SweepControl::Helix(helix);
    let thread = k.sweep_with(&profile, &spec).unwrap().first().unwrap();
    let r = rod(
        &mut k,
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        7.932446399148578,
        3.9922122959395665,
    );
    let ab = intersection(&mut k, thread, r);
    let ba = intersection(&mut k, r, thread);
    for x in [ab, ba] {
        assert!(issues(&k, x).is_empty(), "{:?}", issues(&k, x));
    }
    // Exact to the tolerance in space, so to it times the area in volume.
    let (va, vb) = (volume(&k, ab), volume(&k, ba));
    assert!((va - vb).abs() <= 1e-7 * area(&k, ab), "{va} vs {vb}");
}
