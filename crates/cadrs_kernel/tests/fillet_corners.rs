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

#[test]
fn every_edge_of_a_box_rounded_is_its_inset_grown() {
    // A box a × b × c with every edge rounded by r is the box moved in by r grown by a ball:
    // V = a'b'c' + 2(a'b' + b'c' + c'a') r + π r² (a' + b' + c') + 4πr³/3 and
    // A = 2(a'b' + b'c' + c'a') + 2π r (a' + b' + c') + 4πr², with a' = a − 2r and so on.
    use cadrs_kernel::*;
    use std::f64::consts::PI;
    let mut k = DefaultKernel::default();
    let (a, b, c, r) = (10.0, 20.0, 30.0, 2.0);
    let cube = k
        .primitive(&Primitive::Box {
            min: nalgebra::Point3::new(1.0, 2.0, 3.0),
            max: nalgebra::Point3::new(1.0 + a, 2.0 + b, 3.0 + c),
        })
        .unwrap()
        .first()
        .unwrap();
    let edges: Vec<EdgeId> = k.edges(cube).unwrap().iter().map(|e| e.id).collect();
    let round = k.fillet(cube, &edges, r).unwrap();
    let id = round.first().unwrap();
    let (x, y, z) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
    let m = k.mass_properties(id).unwrap();
    let v = x * y * z
        + 2.0 * (x * y + y * z + z * x) * r
        + PI * r * r * (x + y + z)
        + 4.0 / 3.0 * PI * r.powi(3);
    let s = 2.0 * (x * y + y * z + z * x) + 2.0 * PI * r * (x + y + z) + 4.0 * PI * r * r;
    assert!(
        (m.volume - v).abs() <= 1e-10 * v,
        "volume {} vs {v}",
        m.volume
    );
    assert!(
        (m.surface_area - s).abs() <= 1e-10 * s,
        "area {} vs {s}",
        m.surface_area
    );
    assert_eq!(k.faces(id).unwrap().len(), 6 + 12 + 8);
    assert!(k.get(id).unwrap().check().is_empty());
}

#[test]
fn a_rounded_prism_s_top_loop_rounds_in_closed_form() {
    // A box W × L × H with its upright edges rounded by ρ, then its top loop by r: the rim
    // takes r²(1 − π/4) along each line and, about each corner's axis, that area along the
    // circle its centroid follows, of radius ρ − r(10 − 3π)/(3(4 − π)) (Pappus).
    use cadrs_kernel::*;
    use std::f64::consts::PI;
    let (w, l, h, rho, r) = (6.8, 10.25, 3.67, 2.6, 0.5);
    let mut k = DefaultKernel::default();
    let bx = k
        .primitive(&Primitive::Box {
            min: nalgebra::Point3::new(0.0, 0.0, 0.0),
            max: nalgebra::Point3::new(w, l, h),
        })
        .unwrap()
        .first()
        .unwrap();
    let up: Vec<EdgeId> = k
        .edges(bx)
        .unwrap()
        .iter()
        .filter(|e| (e.end - e.start).normalize().z.abs() > 0.99)
        .map(|e| e.id)
        .collect();
    let corners = k.fillet(bx, &up, rho).unwrap().first().unwrap();
    let top: Vec<EdgeId> = k
        .edges(corners)
        .unwrap()
        .iter()
        .filter(|e| e.start.z > h - 1e-9 && e.end.z > h - 1e-9)
        .map(|e| e.id)
        .collect();
    assert_eq!(top.len(), 8);
    let rim = k.fillet(corners, &top, r).unwrap().first().unwrap();
    let section = r * r * (1.0 - PI / 4.0);
    let centroid = r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let lines = 2.0 * (w - 2.0 * rho) + 2.0 * (l - 2.0 * rho);
    let base = (w * l - (4.0 - PI) * rho * rho) * h;
    let want = base - section * (lines + 2.0 * PI * (rho - centroid));
    let got = k.mass_properties(rim).unwrap().volume;
    assert!((got - want).abs() <= 1e-9 * want, "volume {got} vs {want}");
    assert_eq!(k.faces(rim).unwrap().len(), 10 + 8);
    assert!(k.get(rim).unwrap().check().is_empty());
}

#[test]
fn a_prism_s_upright_edges_round_in_closed_form() {
    // A pentagonal prism of height h with its five upright edges rounded by r: each corner of
    // interior angle φ takes r²(cot(φ/2) − (π − φ)/2) of the section.
    let pts = [[0.0, 0.0], [6.0, -1.0], [8.0, 4.0], [3.0, 7.0], [-1.0, 4.0]];
    let (h, r) = (3.5, 0.7);
    let mut k = NativeKernel::new();
    let tuples: Vec<(f64, f64)> = pts.iter().map(|p| (p[0], p[1])).collect();
    let prism = prism(&mut k, polygon(&tuples), h);
    let up: Vec<EdgeId> = k
        .edges(prism)
        .unwrap()
        .iter()
        .filter(|e| (e.end - e.start).normalize().z.abs() > 0.99)
        .map(|e| e.id)
        .collect();
    assert_eq!(up.len(), 5);
    let round = k.fillet(prism, &up, r).unwrap().first().unwrap();
    let n = pts.len();
    let mut area = 0.0;
    let mut cut = 0.0;
    for i in 0..n {
        let (p, q, s) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        area += 0.5 * (q[0] * s[1] - s[0] * q[1]);
        let (u, v) = ([p[0] - q[0], p[1] - q[1]], [s[0] - q[0], s[1] - q[1]]);
        let phi = ((u[0] * v[0] + u[1] * v[1]) / (u[0].hypot(u[1]) * v[0].hypot(v[1]))).acos();
        cut += r * r * (1.0 / (phi / 2.0).tan() - (PI - phi) / 2.0);
    }
    let want = (area - cut) * h;
    let got = k.mass_properties(round).unwrap().volume;
    assert!((got - want).abs() <= 1e-9 * want, "volume {got} vs {want}");
    assert_eq!(k.faces(round).unwrap().len(), 7 + 5);
    assert!(k.get(round).unwrap().check().is_empty());
}

#[test]
fn a_cylinder_s_top_rim_rounds_in_closed_form() {
    // A cylinder of radius R and height h with its top rim rounded by r: it loses r²(1 − π/4)
    // of section round the circle its centroid follows, of radius
    // R − r(10 − 3π)/(3(4 − π)) (Pappus).
    let (big, h, r) = (3.0, 4.0, 0.8);
    let mut k = NativeKernel::new();
    let c = k
        .primitive(&Primitive::Cylinder {
            base: Point3::new(1.0, -2.0, 0.5),
            axis: nalgebra::Unit::new_normalize(nalgebra::Vector3::z()),
            radius: big,
            height: h,
        })
        .unwrap()
        .bodies[0];
    let top: Vec<EdgeId> = k
        .edges(c)
        .unwrap()
        .iter()
        .filter(|e| e.start.z > 0.5 + h - 1e-9 && e.end.z > 0.5 + h - 1e-9)
        .map(|e| e.id)
        .collect();
    assert_eq!(top.len(), 1);
    let round = k.fillet(c, &top, r).unwrap().bodies[0];
    let centroid = r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let want = PI * big * big * h - r * r * (1.0 - PI / 4.0) * 2.0 * PI * (big - centroid);
    let got = k.mass_properties(round).unwrap().volume;
    assert!((got - want).abs() <= 1e-9 * want, "volume {got} vs {want}");
    assert_eq!(k.faces(round).unwrap().len(), 4);
    assert!(k.get(round).unwrap().check().is_empty());
}

#[test]
fn a_frustum_s_top_rim_rounds_in_closed_form() {
    // A frustum (radii 4 and 2, 3 high) with its top rim rounded by r: in the half plane
    // through the axis the round cuts off the corner between the top and the slanted side
    // (the kite of the tangent segments less the ball's sector), swept round the axis by its
    // centroid (Pappus).
    let (r0, r1, h, r) = (4.0f64, 2.0f64, 3.0f64, 0.5f64);
    let mut k = NativeKernel::new();
    let c = k
        .primitive(&Primitive::Cone {
            base: Point3::new(0.0, 0.0, 0.0),
            axis: nalgebra::Unit::new_normalize(nalgebra::Vector3::z()),
            base_radius: r0,
            top_radius: r1,
            height: h,
        })
        .unwrap()
        .bodies[0];
    let top: Vec<EdgeId> = k
        .edges(c)
        .unwrap()
        .iter()
        .filter(|e| e.start.z > h - 1e-9 && e.end.z > h - 1e-9)
        .map(|e| e.id)
        .collect();
    assert_eq!(top.len(), 1);
    let round = k.fillet(c, &top, r).unwrap().bodies[0];
    // The corner in (ρ, z).
    let l = (r0 - r1).hypot(h);
    let (ma, mb) = ((-1.0, 0.0), ((r0 - r1) / l, -h / l));
    let theta = (ma.0 * mb.0 + ma.1 * mb.1).acos();
    let s = r / (theta / 2.0).tan();
    let b = {
        let (x, y) = (ma.0 + mb.0, ma.1 + mb.1);
        let n = x.hypot(y);
        (x / n, y / n)
    };
    let p = (r1, h);
    let c = (
        p.0 + b.0 * r / (theta / 2.0).sin(),
        p.1 + b.1 * r / (theta / 2.0).sin(),
    );
    let (pa, pb) = ((p.0 + ma.0 * s, p.1), (p.0 + mb.0 * s, p.1 + mb.1 * s));
    let kite = r * s;
    let kite_rho = ((p.0 + pa.0 + c.0) / 3.0 + (p.0 + pb.0 + c.0) / 3.0) / 2.0;
    let alpha = PI - theta;
    let sector = alpha * r * r / 2.0;
    let sector_rho = c.0 - b.0 * 4.0 * r * (alpha / 2.0).sin() / (3.0 * alpha);
    let corner = kite - sector;
    let rho = (kite * kite_rho - sector * sector_rho) / corner;
    let frustum = PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
    let want = frustum - corner * 2.0 * PI * rho;
    let got = k.mass_properties(round).unwrap().volume;
    assert!((got - want).abs() <= 1e-9 * want, "volume {got} vs {want}");
    assert!(k.get(round).unwrap().check().is_empty());
}
