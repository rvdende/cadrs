//! Fillet corners the rolling ball settles exactly (brep-conformance's definitions of `fillet`
//! and `fillet_radii`): three radii at a box corner, a narrow face the ball bridges over, a top
//! loop past its corner rounds, convex fillets round a concave edge filleted too, and every edge
//! of a prism whose sides' strips would cross past a short side.

use std::f64::consts::PI;

use cadrs_kernel::native::kernel::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3};

/// The prism on the closed outline `segs` (lines and arcs in the top plane) `h` high.
fn prism(k: &mut NativeKernel, segs: Vec<Curve2>, h: f64) -> BodyId {
    k.extrude(
        &Profile::new(
            Plane::top(),
            vec![Region {
                outer: Loop { curves: segs },
                holes: vec![],
                source: None,
            }],
        ),
        Extent::Blind(h),
    )
    .unwrap()
    .bodies[0]
}

fn polygon(pts: &[(f64, f64)]) -> Vec<Curve2> {
    (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            Curve2::Line {
                a: Point2::new(a.0, a.1),
                b: Point2::new(b.0, b.1),
                source: None,
            }
        })
        .collect()
}

/// The edge whose middle is nearest `p`.
fn edge_at(k: &NativeKernel, b: BodyId, p: [f64; 3]) -> EdgeId {
    let p = Point3::new(p[0], p[1], p[2]);
    k.edges(b)
        .unwrap()
        .into_iter()
        .min_by(|x, y| (x.mid - p).norm().total_cmp(&(y.mid - p).norm()))
        .unwrap()
        .id
}

fn valid(k: &NativeKernel, b: BodyId) {
    let issues = k.get(b).unwrap().check();
    assert!(issues.is_empty(), "{issues:?}");
}

fn volume(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

/// A 12 cube's three edges at its corner (12, 12, 12) rounded 2, 1.5 and 1: the ball of radius
/// 2 touching the three faces closes the corner, the radius-2 strip runs on to it, and from it a
/// cone shrinks to each smaller strip, which a cap of that strip's ball ends. The corner is all
/// spheres and cones, and lies between the cube rounded at 2 and at 1 (a ball closing each).
#[test]
fn three_radii_at_a_corner_close_with_a_ball_and_cones() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[(0.0, 0.0), (12.0, 0.0), (12.0, 12.0), (0.0, 12.0)]),
        12.0,
    );
    let radii = [2.0, 1.5, 1.0];
    let laws: Vec<FilletLaw> = [[6.0, 12.0, 12.0], [12.0, 6.0, 12.0], [12.0, 12.0, 6.0]]
        .iter()
        .zip(radii)
        .map(|(p, r)| FilletLaw {
            edge: edge_at(&k, b, *p),
            radii: vec![(0.0, r), (1.0, r)],
        })
        .collect();
    let out = k.fillet_variable(b, &laws, true).unwrap().bodies[0];
    valid(&k, out);
    let faces = k.faces(out).unwrap();
    let count = |kind: SurfaceKind| faces.iter().filter(|f| f.kind == kind).count();
    // The corner ball and the two smaller strips' caps; their two cones.
    assert_eq!(count(SurfaceKind::Sphere), 3);
    assert_eq!(count(SurfaceKind::Cone), 2);
    let kk = 1.0 - PI / 4.0;
    let at = |r: f64| 1728.0 - 3.0 * kk * r * r * (12.0 - r) - r.powi(3) * (1.0 - PI / 6.0);
    let v = volume(&k, out);
    assert!(
        v > at(2.0) && v < at(1.0),
        "{v} not in {} to {}",
        at(2.0),
        at(1.0)
    );
}

/// A 10 × 10 × 5 block whose upright corner (10, 10) is chamfered by a hair, both edges of the
/// chamfer filleted 2: the ball can't touch the chamfer, rests on the two sides either side and
/// bridges over it. The result is the sharp corner filleted 2, the chamfer gone.
#[test]
fn a_round_swallows_a_face_too_narrow_for_its_ball() {
    let mut k = NativeKernel::new();
    let cut = 0.01;
    let b = prism(
        &mut k,
        polygon(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0 - cut),
            (10.0 - cut, 10.0),
            (0.0, 10.0),
        ]),
        5.0,
    );
    let edges = [
        edge_at(&k, b, [10.0, 10.0 - cut, 2.5]),
        edge_at(&k, b, [10.0 - cut, 10.0, 2.5]),
    ];
    let out = k.fillet(b, &edges, 2.0).unwrap().bodies[0];
    valid(&k, out);
    let want = 500.0 - (1.0 - PI / 4.0) * 4.0 * 5.0;
    let v = volume(&k, out);
    assert!((v - want).abs() < 1e-9 * want, "volume {v}, want {want}");
    let faces = k.faces(out).unwrap();
    assert_eq!(
        faces.len(),
        7,
        "the chamfer is gone, one round in its place"
    );
    assert!(faces.iter().any(
        |f| f.kind == SurfaceKind::Cylinder && f.radius.is_some_and(|r| (r - 2.0).abs() < 1e-9)
    ));
}

/// An a-square block with its upright corners rounded ρ, its top loop filleted r > ρ: the ball
/// can't follow a round; at each corner both strips end at the section through the one centre
/// r in from the top and both sides, and the corner is the family of quarter ellipses from the
/// base r under the top (along the sides and the round) up to the axis's foot on the top. In
/// the half plane at φ round the axis, its base d(φ) from it, the corner takes
/// ∫₀ᵈ r(1 − √(1 − ρ²/d²))ρ dρ = r·d²/6, and ∫d² dφ over the corner is twice the area between the
/// axis and its base, 2(r² − (1 − π/4)ρ²). Each side's strip, a − 2r long, takes (1 − π/4)r² a
/// unit of length.
#[test]
fn a_top_loop_bigger_than_its_corner_rounds() {
    let (a, rho, h, r) = (6.0, 0.5, 4.0, 1.0);
    let mut k = NativeKernel::new();
    let mut segs = Vec::new();
    // Counter-clockwise from (ρ, 0): each side, then the round turning to the next.
    let corners = [
        (a - rho, rho),
        (a - rho, a - rho),
        (rho, a - rho),
        (rho, rho),
    ];
    let starts = [(rho, 0.0), (a, rho), (a - rho, a), (0.0, a - rho)];
    for i in 0..4 {
        let (s, c) = (starts[i], corners[i]);
        let angle = -PI / 2.0 + i as f64 * PI / 2.0;
        let e = (c.0 + rho * angle.cos(), c.1 + rho * angle.sin());
        segs.push(Curve2::Line {
            a: Point2::new(s.0, s.1),
            b: Point2::new(e.0, e.1),
            source: None,
        });
        segs.push(Curve2::Arc {
            center: Point2::new(c.0, c.1),
            radius: rho,
            start_angle: angle,
            sweep: PI / 2.0,
            source: None,
        });
    }
    let b = prism(&mut k, segs, h);
    let top: Vec<EdgeId> = k
        .edges(b)
        .unwrap()
        .into_iter()
        .filter(|e| (e.start.z - h).abs() < 1e-9 && (e.end.z - h).abs() < 1e-9)
        .map(|e| e.id)
        .collect();
    assert_eq!(top.len(), 8);
    let out = k.fillet(b, &top, r).unwrap().bodies[0];
    valid(&k, out);
    let kk = 1.0 - PI / 4.0;
    let want = (a * a - (4.0 - PI) * rho * rho) * h
        - 4.0 * kk * r * r * (a - 2.0 * r)
        - 4.0 * r / 3.0 * (r * r - kk * rho * rho);
    let v = volume(&k, out);
    assert!((v - want).abs() < 1e-6 * want, "volume {v}, want {want}");
}

/// The two top edges of an L's notch and the notch's upright edge, all filleted 1: at the top of
/// the upright edge a concave fillet meets two convex ones. One face closes the corner, meeting
/// each fillet with its tangent plane; the corner is its own mirror in the plane between the
/// arms, and so is the result.
#[test]
fn convex_fillets_meeting_a_concave_one() {
    let mut k = NativeKernel::new();
    let b = prism(
        &mut k,
        polygon(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 5.0),
            (5.0, 5.0),
            (5.0, 10.0),
            (0.0, 10.0),
        ]),
        5.0,
    );
    let edges = [
        edge_at(&k, b, [7.5, 5.0, 5.0]),
        edge_at(&k, b, [5.0, 7.5, 5.0]),
        edge_at(&k, b, [5.0, 5.0, 2.5]),
    ];
    let out = k.fillet(b, &edges, 1.0).unwrap().bodies[0];
    valid(&k, out);
    let m = k.mass_properties(out).unwrap();
    let c = m.center_of_mass;
    assert!((c.x - c.y).abs() < 1e-5, "not its own mirror: {c:?}");
}

/// Every edge of a prism on a convex heptagon, at a radius under which two sides' strips on the
/// top would cross past the short side between them: they end first at their corners with it,
/// so no weld. The result is the heptagon inset by r between r and h − r, swelled by r
/// (Steiner: with the inset's area A and perimeter L and H = h − 2r,
/// V = AH + (2A + LH)r + (πH + πL/2)r² + 4πr³/3).
#[test]
fn a_prism_rounded_all_over_past_a_short_side() {
    let pts = [
        (3.558304498792547, 4.536634498215077),
        (-0.33761755039428737, 5.755744770764962),
        (-5.274511547884164, 2.3285427648551007),
        (-5.558972316933715, -1.5298399445978157),
        (-2.225225473791464, -5.318924239666513),
        (3.5920736096239256, -4.509943531749125),
        (5.724639619273676, -0.686356107253791),
    ];
    let (h, r) = (9.424739101496737, 2.606285687091499);
    let mut k = NativeKernel::new();
    let b = prism(&mut k, polygon(&pts), h);
    let all: Vec<EdgeId> = k.edges(b).unwrap().into_iter().map(|e| e.id).collect();
    let out = k.fillet(b, &all, r).unwrap().bodies[0];
    valid(&k, out);
    // The inset: each corner moved along the sum of its sides' inward normals.
    let n = pts.len();
    let inward = |i: usize| {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        let l = (q.0 - p.0).hypot(q.1 - p.1);
        (-(q.1 - p.1) / l, (q.0 - p.0) / l)
    };
    let q: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let (a, c) = (inward((i + n - 1) % n), inward(i));
            let s = r / (1.0 + a.0 * c.0 + a.1 * c.1);
            (pts[i].0 + (a.0 + c.0) * s, pts[i].1 + (a.1 + c.1) * s)
        })
        .collect();
    let area = (0..n)
        .map(|i| q[i].0 * q[(i + 1) % n].1 - q[(i + 1) % n].0 * q[i].1)
        .sum::<f64>()
        / 2.0;
    let len: f64 = (0..n)
        .map(|i| (q[(i + 1) % n].0 - q[i].0).hypot(q[(i + 1) % n].1 - q[i].1))
        .sum();
    let big_h = h - 2.0 * r;
    let want = area * big_h
        + (2.0 * area + len * big_h) * r
        + (PI * big_h + PI * len / 2.0) * r * r
        + 4.0 * PI * r.powi(3) / 3.0;
    let v = volume(&k, out);
    assert!((v - want).abs() < 1e-6 * want, "volume {v}, want {want}");
}
