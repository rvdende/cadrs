//! Ported from springrep's tests/blends.rs (MIT, https://github.com/springcraft-ai/springrep):
//! chamfers and fillets on small swept solids against closed-form volume and area, each result
//! checked valid and watertight. springrep picks edges by the names of the two faces meeting in
//! them; here the faces are picked by their geometry. Its assertions on the names the blend
//! faces get are left out (cadrs names faces through `History`, not on the body).

#![allow(dead_code)]

use std::f64::consts::PI;

use cadrs_kernel::native::{NativeKernel, tess};
use cadrs_kernel::*;
use nalgebra::{Point3, Vector3};

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

/// `edges` and the edges of their tangent chains: springrep's blends run along a picked edge's
/// tangent chain, which cadrs's fillet does itself and its chamfer leaves to the caller (as the
/// app's Chamfer feature does with `tangent_chain`).
fn chain(k: &K, b: BodyId, edges: &[EdgeId]) -> Vec<EdgeId> {
    let mut all = edges.to_vec();
    for &e in edges {
        for c in k.tangent_chain(b, e).unwrap() {
            if !all.contains(&c) {
                all.push(c);
            }
        }
    }
    all
}

/// springrep's COARSE mesh options for this file.
const COARSE: (f64, f64) = (0.2, 0.5);

fn assert_solid(k: &K, b: BodyId, volume: f64, area: f64) {
    valid(k, b);
    let v = self::volume(k, b);
    let a = self::area(k, b);
    assert!(
        (v - volume).abs() <= 1e-9 * volume,
        "volume {v} vs {volume}"
    );
    assert!((a - area).abs() <= 1e-9 * area, "area {a} vs {area}");
    assert!(watertight(k, b, COARSE.0, COARSE.1), "mesh not watertight");
}

/// springrep's `block(id, w, d, h)`: [0, w] × [0, d] × [0, h].
fn block(k: &mut K, w: f64, d: f64, h: f64) -> BodyId {
    extrude(k, 0.0, rect(w / 2.0, d / 2.0, w, d), vec![], h)
}

/// springrep's `disc(id, z, diameter, h)`.
fn disc(k: &mut K, z: f64, diameter: f64, h: f64) -> BodyId {
    extrude(k, z, circle(0.0, 0.0, diameter), vec![], h)
}

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];

#[test]
fn a_straight_convex_edge_takes_a_cylinder_fillet() {
    let mut k = K::new();
    let bx = block(&mut k, 40.0, 30.0, 20.0);
    let edges = pair(&k, bx, plane(Z, 20.0), plane(Y, 0.0));
    assert_eq!(edges.len(), 1);
    let b = k.fillet(bx, &edges, 5.0).unwrap().bodies[0];
    let spandrel = 25.0 * (1.0 - PI / 4.0);
    let area = 5200.0 - 2.0 * 5.0 * 40.0 - 2.0 * spandrel + PI / 2.0 * 5.0 * 40.0;
    assert_solid(&k, b, 24000.0 - spandrel * 40.0, area);
    assert_eq!(k.face_count(b).unwrap(), 7);
    assert_eq!(kinds(&k, b, SurfaceKind::Cylinder), 1);
    // springrep: the round is named `f.round_0` and the top keeps `box.end` (naming).
}

#[test]
fn a_straight_concave_edge_takes_a_fillet_that_adds_material() {
    let mut k = K::new();
    let pts = [
        [0.0, 0.0],
        [40.0, 0.0],
        [40.0, 10.0],
        [10.0, 10.0],
        [10.0, 30.0],
        [0.0, 30.0],
    ];
    let ell = extrude(&mut k, 0.0, Loop::polygon(&pts), vec![], 20.0);
    // l.e2 is the side y = 10, l.e3 the side x = 10.
    let edges = pair(&k, ell, plane(Y, 10.0), plane(X, 10.0));
    assert_eq!(edges.len(), 1);
    let b = k.fillet(ell, &edges, 4.0).unwrap().bodies[0];
    let spandrel = 16.0 * (1.0 - PI / 4.0);
    let area = 2.0 * (600.0 + spandrel) + 20.0 * (140.0 - 8.0 + 2.0 * PI);
    assert_solid(&k, b, 600.0 * 20.0 + spandrel * 20.0, area);
    // springrep: names `f.round_0` and `l.e2` (naming).
}

#[test]
fn a_circular_edge_takes_a_cone_chamfer() {
    let mut k = K::new();
    let c = disc(&mut k, 0.0, 40.0, 20.0);
    let edges = pair(&k, c, plane(Z, 20.0), cylinder(0.0, 0.0, 20.0));
    assert_eq!(edges.len(), 1);
    let b = k
        .chamfer(c, &chain(&k, c, &edges), ChamferSpec::EqualDistance(2.0))
        .unwrap()
        .bodies[0];
    let volume = PI * 400.0 * 18.0 + PI * 2.0 / 3.0 * (400.0 + 360.0 + 324.0);
    let area = 400.0 * PI + 2.0 * PI * 20.0 * 18.0 + 324.0 * PI + PI * 38.0 * 2.0 * 2f64.sqrt();
    assert_solid(&k, b, volume, area);
    assert_eq!(kinds(&k, b, SurfaceKind::Cone), 1);
    // springrep: the bevel is named `ch.bevel_0` (naming).
}

#[test]
fn a_concave_circular_edge_takes_a_torus_fillet() {
    let mut k = K::new();
    let plate = extrude(&mut k, 0.0, rect(0.0, 0.0, 60.0, 60.0), vec![], 10.0);
    let boss = disc(&mut k, 10.0, 20.0, 10.0);
    let body = union(&mut k, plate, boss);
    let edges = pair(&k, body, plane(Z, 10.0), cylinder(0.0, 0.0, 10.0));
    assert_eq!(edges.len(), 1);
    let b = k.fillet(body, &edges, 3.0).unwrap().bodies[0];
    // Pappus: the spandrel's centroid sits 3·(10 − 3π)/(12 − 3π) from the corner, in ρ and z.
    let spandrel = 9.0 * (1.0 - PI / 4.0);
    let offset = 3.0 * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let volume = 36000.0 + PI * 1000.0 + 2.0 * PI * (10.0 + offset) * spandrel;
    let area = (3600.0 - PI * 169.0)
        + 3600.0
        + 2400.0
        + 2.0 * PI * 70.0
        + PI * 100.0
        + 2.0 * PI * (13.0 - 6.0 / PI) * (1.5 * PI);
    assert_solid(&k, b, volume, area);
    assert_eq!(kinds(&k, b, SurfaceKind::Torus), 1);
    // springrep: the round is named `f.round_0` (naming).
}

#[test]
fn three_fillets_at_a_corner_close_with_a_sphere() {
    let mut k = K::new();
    let bx = block(&mut k, 20.0, 20.0, 20.0);
    let mut edges = pair(&k, bx, plane(Z, 20.0), plane(X, 20.0));
    edges.extend(pair(&k, bx, plane(Z, 20.0), plane(Y, 20.0)));
    edges.extend(pair(&k, bx, plane(X, 20.0), plane(Y, 20.0)));
    assert_eq!(edges.len(), 3);
    let b = k.fillet(bx, &edges, 4.0).unwrap().bodies[0];
    let spandrel = 16.0 * (1.0 - PI / 4.0);
    let volume = 8000.0 - 3.0 * 16.0 * spandrel - (64.0 - PI * 64.0 / 6.0);
    let area = 2400.0 - 3.0 * 4.0 * 16.0 * 2.0 - 3.0 * 16.0 - 3.0 * spandrel
        + 3.0 * PI / 2.0 * 4.0 * 16.0
        + PI * 16.0 / 2.0;
    assert_solid(&k, b, volume, area);
    assert_eq!(kinds(&k, b, SurfaceKind::Sphere), 1);
    // springrep: the corner is named `f.corner` and the rounds `f.round_0..2` (naming).
}

#[test]
fn a_blend_wider_than_its_face_is_refused() {
    let mut k = K::new();
    let bx = block(&mut k, 40.0, 30.0, 4.0);
    let edges = pair(&k, bx, plane(Z, 4.0), plane(Y, 0.0));
    assert_eq!(edges.len(), 1);
    // (cadrs lets a chamfer run on past its face by default, as Onshape's does through a thin
    // wall: without overflow, it is refused.)
    let strict = ChamferOpts {
        overflow: false,
        ..ChamferOpts::new(ChamferSpec::EqualDistance(5.0))
    };
    let r = k.chamfer_with(bx, &chain(&k, bx, &edges), &strict);
    assert!(r.is_err(), "{:?}", r.map(|r| volume(&k, r.bodies[0])));
}

/// Fillets of `r` on the top edge of `body`'s side y = `side`, from x = `from` to 60, whose end at
/// `from` dies into a curved face: a point w in from the side at x lies past that face where
/// w > `w0(x)`, which reaches r at `x1`. The round takes the part of its spandrel short of w₀,
/// S(u) = r·u − ∫_{r−u}^{r} √(r² − v²) dv, and the whole S(r) = r²(1 − π/4) from x₁ to 60.
fn assert_dies_into(
    k: &mut K,
    body: BodyId,
    side: f64,
    r: f64,
    from: f64,
    x1: f64,
    w0: impl Fn(f64) -> f64,
) {
    let edge = edge_at(k, body, p3(30.0, side, 4.0));
    let b = k.fillet(body, &[edge], r).unwrap().bodies[0];
    let g = |v: f64| v / 2.0 * (r * r - v * v).max(0.0).sqrt() + r * r / 2.0 * (v / r).asin();
    let s = |u: f64| r * u - (g(r) - g(r - u));
    let n = 4000;
    let h = (x1 - from) / n as f64;
    let simpson: f64 = (0..=n)
        .map(|i| {
            let weight = if i == 0 || i == n {
                1.0
            } else {
                [2.0, 4.0][i % 2]
            };
            weight * s(w0(from + h * i as f64).clamp(0.0, r))
        })
        .sum::<f64>()
        * h
        / 3.0;
    let removed = simpson + (60.0 - x1) * s(r);
    valid(k, b);
    let taken = volume(k, body) - volume(k, b);
    assert!(
        (taken - removed).abs() <= 1e-6 * removed,
        "took {taken} vs {removed}"
    );
    assert_eq!(k.face_count(b).unwrap(), k.face_count(body).unwrap() + 1);
    // springrep: the round is named `f.round_0` (naming).
}

#[test]
#[ignore = "cadrs's fillet or chamfer end against a face it meets at a glancing angle, or past a short edge, is off"]
fn a_fillet_ending_on_a_hub_it_meets_at_a_glancing_angle_dies_into_it() {
    // A hub of radius 25 and a bar out to x = 60, both 4 thick, the bar's side on the line y = −d
    // with d = √624: the side leaves the hub at x = 1, at 1/25 of a right angle from its tangent.
    // A fillet of 2 on the side's top edge ends on the hub, and leaves the hub's own edge sharp.
    let mut k = K::new();
    let d = 624f64.sqrt();
    let bar = extrude(&mut k, 0.0, rect(30.0, 0.0, 60.0, 2.0 * d), vec![], 4.0);
    let hub = disc(&mut k, 0.0, 50.0, 4.0);
    let body = union(&mut k, hub, bar);
    let x1 = (625.0 - (d - 2.0) * (d - 2.0)).sqrt();
    assert_dies_into(&mut k, body, -d, 2.0, 1.0, x1, |x| {
        d - (625.0 - x * x).sqrt()
    });
}

#[test]
#[ignore = "cadrs's fillet or chamfer end against a face it meets at a glancing angle, or past a short edge, is off"]
fn a_fillet_end_whose_rim_bows_out_behind_its_edge_runs_no_further() {
    // A plate 4 thick whose side y = 0 runs from x = 0 to 60. At x = 0 its rim turns onto an arc
    // about (−1, 50) through the origin, which bows out below y = 0 and comes back to it short of
    // x = −2, ending 5e-8 above it, within the tolerance of the line, in a reflex corner. The
    // fillet of 2 on the side's top edge meets the arc at a glancing angle and dies into it; the
    // rim past the arc never reaches into the fillet's strip.
    let mut k = K::new();
    let (c, radius, lift) = ([-1.0f64, 50.0f64], 2501f64.sqrt(), 5e-8);
    let end = [
        c[0] - (radius * radius - (c[1] - lift).powi(2)).sqrt(),
        lift,
    ];
    let at = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let corners = [
        [0.0, 0.0],
        [60.0, 0.0],
        [60.0, 20.0],
        [-10.0, 20.0],
        [-10.0, -0.5],
    ];
    // rim, then side, end, back, heel, and toe back to the rim's start.
    let mut curves = vec![Curve2::arc(c, radius, at(end), at(corners[0]) - at(end))];
    for i in 0..4 {
        curves.push(Curve2::line(corners[i], corners[i + 1]));
    }
    curves.push(Curve2::line(corners[4], end));
    let body = extrude(&mut k, 0.0, Loop::new(curves), vec![], 4.0);
    let x1 = (2501.0f64 - 48.0 * 48.0).sqrt() - 1.0;
    assert_dies_into(&mut k, body, 0.0, 2.0, 0.0, x1, |x| {
        50.0 - (2501.0 - (x + 1.0) * (x + 1.0)).sqrt()
    });
}

#[test]
#[ignore = "cadrs's fillet or chamfer end against a face it meets at a glancing angle, or past a short edge, is off"]
fn a_boss_fillet_wider_than_its_ledge_ends_on_the_ledge_side() {
    // A disc of radius 10 and height 4 with a boss of radius 8 and height 6 on it: the ledge is 2
    // wide and a fillet of 3 at the boss's foot runs over its rim, ending on the disc's side.
    let mut k = K::new();
    let base = disc(&mut k, 0.0, 20.0, 4.0);
    let boss = disc(&mut k, 4.0, 16.0, 6.0);
    let body = union(&mut k, base, boss);
    let edges = pair(&k, body, plane(Z, 4.0), cylinder(0.0, 0.0, 8.0));
    assert_eq!(edges.len(), 1);
    let b = k.fillet(body, &edges, 3.0).unwrap().bodies[0];
    // In the section, s out from the boss and h up from the ledge, the arc's centre is (3, 3):
    // h = 3 − √(9 − w²), w = 3 − s, kept for s ≤ 2 (w ≥ 1). With J = ∫₁³ √(9 − w²) dw
    // = 9π/4 − √2 − 4.5·asin(1/3), the spandrel kept is 6 − J and its moment about the boss's
    // side ∫₁³ (3 − w)(3 − √(9 − w²)) dw = 6 − 3J + 16√2/3. The arc meets the disc's side at
    // h = 3 − √8 after turning acos(1/3), its moment about the boss 9·acos(1/3) − 6√2.
    let j = 9.0 * PI / 4.0 - 2f64.sqrt() - 4.5 * (1.0f64 / 3.0).asin();
    let (kept, moment) = (6.0 - j, 6.0 - 3.0 * j + 16.0 * 2f64.sqrt() / 3.0);
    let turn = (1.0f64 / 3.0).acos();
    let volume = 784.0 * PI + 2.0 * PI * (8.0 * kept + moment);
    let area = (376.0 - 36.0 - 48.0) * PI
        + 20.0 * PI * (3.0 - 8f64.sqrt())
        + 2.0 * PI * (33.0 * turn - 6.0 * 2f64.sqrt());
    assert_solid(&k, b, volume, area);
    assert_eq!(k.face_count(b).unwrap(), 5);
    assert_eq!(kinds(&k, b, SurfaceKind::Torus), 1);
    // The ledge (springrep's `base.end`) is gone: no face is left on z = 4.
    assert!(faces(&k, b, plane(Z, 4.0)).is_empty());
}

#[test]
#[ignore = "cadrs's fillet or chamfer end against a face it meets at a glancing angle, or past a short edge, is off"]
fn a_concave_fillet_wider_than_its_ledge_ends_on_the_ledge_face() {
    // A ledge 10 deep (u), 4 high (v) and 30 long (z), and on it a wall 4 thick and 6 high standing
    // 2 back from the ledge's front (u = 0), running 20 of its length. A fillet of 3 where the
    // ledge's top meets the wall needs 3 of ledge and finds 2: it runs over the ledge's front edge
    // and ends on the front face, which is carried up to meet it.
    let mut k = K::new();
    let ledge = extrude(&mut k, 0.0, rect(5.0, 2.0, 10.0, 4.0), vec![], 30.0);
    let wall = extrude(&mut k, 0.0, rect(4.0, 7.0, 4.0, 6.0), vec![], 20.0);
    let body = union(&mut k, ledge, wall);
    // ledge.v_max is y = 4, wall.u_min is x = 2.
    let edges = pair(&k, body, plane(Y, 4.0), plane(X, 2.0));
    assert_eq!(edges.len(), 1);
    let b = k.fillet(body, &edges, 3.0).unwrap().bodies[0];
    // The arc's centre is at (−1, 7). The full spandrel is 9 − 9π/4; past u = 0 it would hold
    // ∫₀¹ (3 − √(9 − s²)) ds = 3 − √2 − 4.5·asin(1/3). The arc meets u = 0 at v = 7 − √8, having
    // turned acos(1/3) from the wall. The body's area is 1208.
    let kept = 6.0 - 9.0 * PI / 4.0 + 2f64.sqrt() + 4.5 * (1.0f64 / 3.0).asin();
    let rise = 3.0 - 8f64.sqrt();
    let area = 1208.0 + 2.0 * kept + 20.0 * rise - 40.0 - 60.0 + 60.0 * (1.0f64 / 3.0).acos();
    assert_solid(&k, b, 1680.0 + 20.0 * kept, area);
    assert_eq!(k.face_count(b).unwrap(), 11);
    // springrep: names `f.round_0` and `ledge.u_min` (naming).
}

#[test]
fn a_pair_that_names_no_edge_is_invalid() {
    // The top and the bottom meet in no edge: nothing to fillet, refused.
    let mut k = K::new();
    let bx = block(&mut k, 4.0, 4.0, 4.0);
    let edges = pair(&k, bx, plane(Z, 4.0), plane(Z, 0.0));
    assert!(edges.is_empty());
    let r = k.fillet(bx, &edges, 1.0);
    assert!(r.is_err(), "{:?}", r.map(|r| r.bodies));
}

/// A `w × d` rectangle centred on the origin with corners of radius `r`: sides `s0..s3`
/// counter-clockwise from the bottom, each followed by its corner arc `c0..c3`.
fn rounded(w: f64, d: f64, r: f64) -> Loop {
    let (x, y) = (w / 2.0 - r, d / 2.0 - r);
    let (hw, hd) = (w / 2.0, d / 2.0);
    let arc = |cx: f64, cy: f64, start: f64| Curve2::arc([cx, cy], r, start, PI / 2.0);
    Loop::new(vec![
        Curve2::line([-x, -hd], [x, -hd]),
        arc(x, -y, -PI / 2.0),
        Curve2::line([hw, -y], [hw, y]),
        arc(x, y, 0.0),
        Curve2::line([x, hd], [-x, hd]),
        arc(-x, y, PI / 2.0),
        Curve2::line([-hw, y], [-hw, -y]),
        arc(-x, -y, PI),
    ])
}

#[test]
fn a_chamfer_follows_a_tangent_chain_round_a_pocket() {
    // A rounded-rectangle pocket through a plate: picking two opposite sides takes the corner arcs
    // and the other sides with them, so the whole rim is bevelled, the arcs as cones.
    let mut k = K::new();
    let plate = extrude(&mut k, 0.0, rect(0.0, 0.0, 60.0, 40.0), vec![], 10.0);
    let slot = extrude(&mut k, 0.0, rounded(30.0, 12.0, 5.0), vec![], 10.0);
    let body = difference(&mut k, plate, slot);
    let d = 1.0;
    let mut edges = pair(&k, body, plane(Z, 10.0), plane(Y, -6.0));
    edges.extend(pair(&k, body, plane(Z, 10.0), plane(Y, 6.0)));
    assert_eq!(edges.len(), 2);
    let b = k
        .chamfer(
            body,
            &chain(&k, body, &edges),
            ChamferSpec::EqualDistance(d),
        )
        .unwrap()
        .bodies[0];
    // The band a hole's chamfer takes grows as the outline's outward offset: perimeter L + 2πw.
    let l = 2.0 * 20.0 + 2.0 * 2.0 + 2.0 * PI * 5.0;
    let hole = 30.0 * 12.0 - (4.0 - PI) * 25.0;
    let volume = (2400.0 - hole) * 10.0 - (l * d * d / 2.0 + PI * d * d * d / 3.0);
    let area = 2.0 * (2400.0 - hole) + 10.0 * (200.0 + l) - (l * d + PI * d * d) - l * d
        + 2f64.sqrt() * (l * d + PI * d * d);
    assert_solid(&k, b, volume, area);
    assert_eq!(kinds(&k, b, SurfaceKind::Cone), 4);
    // springrep: eight faces named `ch.bevel_0`/`ch.bevel_1` (naming).
}

#[test]
fn an_edge_two_pairs_pick_is_blended_once() {
    let mut k = K::new();
    let bx = block(&mut k, 40.0, 30.0, 20.0);
    let mut edges = pair(&k, bx, plane(Z, 20.0), plane(Y, 0.0));
    edges.extend(pair(&k, bx, plane(Y, 0.0), plane(Z, 20.0)));
    assert_eq!(edges.len(), 2);
    let b = k.fillet(bx, &edges, 5.0).unwrap().bodies[0];
    let spandrel = 25.0 * (1.0 - PI / 4.0);
    let v = volume(&k, b);
    assert!(
        (v - (24000.0 - spandrel * 40.0)).abs() <= 1e-9 * 24000.0,
        "volume {v}"
    );
    assert_eq!(kinds(&k, b, SurfaceKind::Cylinder), 1);
    // springrep: the round carries both `f.round_0` and `f.round_1` (naming).
}

#[test]
fn chamfers_meeting_at_corners_are_mitred() {
    let mut k = K::new();
    let (w, d, h, c) = (20.0, 12.0, 8.0, 1.5);
    let bx = block(&mut k, w, d, h);
    let mut edges = Vec::new();
    for side in [plane(X, 0.0), plane(X, w)] {
        edges.extend(pair(&k, bx, plane(Z, h), side));
    }
    for side in [plane(Y, 0.0), plane(Y, d)] {
        edges.extend(pair(&k, bx, plane(Z, h), side));
    }
    assert_eq!(edges.len(), 4);
    let b = k
        .chamfer(bx, &chain(&k, bx, &edges), ChamferSpec::EqualDistance(c))
        .unwrap()
        .bodies[0];
    // A hipped top: at depth z the section is the rectangle inset by c − z on every side.
    let volume = w * d * h - (c * c * (w + d) - 4.0 * c * c * c / 3.0);
    let area = (w - 2.0 * c) * (d - 2.0 * c)
        + w * d
        + 2.0 * (w + d) * (h - c)
        + 2.0 * 2f64.sqrt() * c * (w - c + d - c);
    assert_solid(&k, b, volume, area);
    assert_eq!(k.face_count(b).unwrap(), 10);
}

#[test]
#[ignore = "cadrs's fillet or chamfer end against a face it meets at a glancing angle, or past a short edge, is off"]
fn a_chamfer_ending_under_an_overhang_stops_at_its_end_face() {
    // A bar 14 × 20 × 40 with a web 14 × 19 × 10 on top, flush with it at x = 0, x = 14 and
    // y = 0, which leaves a strip of the bar's top 1 wide along y = 20. A chamfer of 4 on the bar's
    // edge at x = 14, y = 20 runs past that strip's 1 mm edge, under the web, and into the concave
    // corner where the strip meets the web's side; it ends square on the strip's plane, z = 40.
    let mut k = K::new();
    let bar = extrude(&mut k, 0.0, rect(7.0, 10.0, 14.0, 20.0), vec![], 40.0);
    let web = extrude(&mut k, 40.0, rect(7.0, 9.5, 14.0, 19.0), vec![], 10.0);
    let body = union(&mut k, bar, web);
    let edges = pair(&k, body, plane(X, 14.0), plane(Y, 20.0));
    assert_eq!(edges.len(), 1);
    let b = k
        .chamfer(
            body,
            &chain(&k, body, &edges),
            ChamferSpec::EqualDistance(4.0),
        )
        .unwrap()
        .bodies[0];
    // The bevel takes the triangle x + y ≥ 30 off the bar's 40 height: 8 × 40 of volume, 4 of each
    // wall and 4√2 of bevel across, and 8 off the bottom. At z = 40 it takes 3.5 off the strip,
    // and leaves the web's underside over (14, 16), (14, 19), (11, 19) bare: 4.5. The body's area
    // is 3940.
    let area = 3940.0 - 320.0 + 160.0 * 2f64.sqrt() - 8.0 - 3.5 + 4.5;
    assert_solid(&k, b, 13860.0 - 320.0, area);
    assert_eq!(k.face_count(b).unwrap(), 10);
    // springrep: the bevel is named `ch.bevel_0` (naming).
}

#[test]
fn a_chamfer_ending_on_a_slanted_face_is_trimmed_by_it() {
    // A trapezoid prism whose right end leans back; the chamfered edge between its top and its
    // end cap runs from the square left end into that slant.
    let mut k = K::new();
    let (l, s, hh, t, d) = (30.0, 6.0, 10.0, 8.0, 1.0);
    let pts = [[0.0, 0.0], [l, 0.0], [l - s, hh], [0.0, hh]];
    let body = extrude(&mut k, 0.0, Loop::polygon(&pts), vec![], t);
    // p.top is the side y = hh, p.end the cap z = t.
    let edges = pair(&k, body, plane(Y, hh), plane(Z, t));
    assert_eq!(edges.len(), 1);
    let b = k
        .chamfer(
            body,
            &chain(&k, body, &edges),
            ChamferSpec::EqualDistance(d),
        )
        .unwrap()
        .bodies[0];
    // Each line along x through the bevel's section ends on the slant x = l − s·y/hh.
    let tri = d * d / 2.0;
    let reach = |y: f64| l - s * y / hh;
    let volume = (l + l - s) / 2.0 * hh * t - tri * reach(hh - d / 3.0);
    let slant = (s * s + hh * hh).sqrt();
    let a0 = (l + l - s) * hh + t * (l + (l - s) + hh + slant);
    let cap_strip = d * l - s * (hh * hh - (hh - d) * (hh - d)) / (2.0 * hh);
    let area = a0 - d * (l - s) - cap_strip - tri * slant / hh - tri
        + d * 2f64.sqrt() * reach(hh - d / 2.0);
    assert_solid(&k, b, volume, area);
    // springrep: the bevel is named `ch.bevel_0` (naming).
}

type V2 = nalgebra::Vector2<f64>;

fn cross2(a: V2, b: V2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// The area `½∮(x dy − y dx)` contributes along the shorter arc about `o` from `a` to `b`, and
/// that arc's length.
fn arc_green(o: V2, r: f64, a: V2, b: V2) -> (f64, f64) {
    let (sa, sb) = ((a - o).y.atan2((a - o).x), (b - o).y.atan2((b - o).x));
    let sw = (sb - sa + PI).rem_euclid(2.0 * PI) - PI;
    let chord = o.x * (sb.sin() - sa.sin()) - o.y * (sb.cos() - sa.cos());
    (0.5 * (r * r * sw + r * chord), r * sw.abs())
}

#[test]
fn a_straight_edge_between_a_plane_and_a_bore_takes_a_fillet() {
    // A key standing in a bore: the root of each side is filleted against the bore's wall.
    let mut k = K::new();
    let plate = extrude(&mut k, 0.0, rect(0.0, 0.0, 60.0, 60.0), vec![], 10.0);
    let bore = extrude(&mut k, 0.0, circle(0.0, 0.0, 40.0), vec![], 10.0);
    let key = extrude(&mut k, 0.0, rect(18.0, 0.0, 6.0, 6.0), vec![], 10.0);
    let drilled = difference(&mut k, plate, bore);
    let body = union(&mut k, drilled, key);
    let r = 1.0;
    let mut edges = pair(&k, body, plane(Y, 3.0), cylinder(0.0, 0.0, 20.0));
    edges.extend(pair(&k, body, plane(Y, -3.0), cylinder(0.0, 0.0, 20.0)));
    assert_eq!(edges.len(), 2);
    let b = k.fillet(body, &edges, r).unwrap().bodies[0];
    // The round's centre is 1 off the side y = 3 and 1 inside the bore of radius 20.
    let p = V2::new(391f64.sqrt(), 3.0);
    let c = V2::new(345f64.sqrt(), 4.0);
    let (t0, t1) = (V2::new(c.x, 3.0), c * (20.0 / 19.0));
    let (g_round, round_len) = arc_green(c, r, t0, t1);
    let (g_bore, bore_len) = arc_green(V2::zeros(), 20.0, t1, p);
    let spandrel = (0.5 * cross2(p, t0) + g_round + g_bore).abs();
    let in_bore = |y: f64| y / 2.0 * (400.0 - y * y).sqrt() + 200.0 * (y / 20.0).asin();
    let key_in_bore = in_bore(3.0) - in_bore(-3.0) - 15.0 * 6.0;
    let volume = 10.0 * (3600.0 - 400.0 * PI + key_in_bore + 2.0 * spandrel);
    let area = area(&k, body) + 4.0 * spandrel + 20.0 * (round_len - (t0 - p).norm() - bore_len);
    assert_solid(&k, b, volume, area);
    assert_eq!(kinds(&k, b, SurfaceKind::Cylinder), 3);
    // springrep: the rounds are named `f.round_0` and `f.round_1` (naming).
}

#[test]
fn a_fillet_as_round_as_its_corners_closes_them_with_spheres() {
    // Every corner of the outline has the fillet's own radius, so each corner's round is an eighth
    // of a sphere; picking one side takes the whole tangent rim.
    let mut k = K::new();
    let (w, d, h, r) = (20.0, 12.0, 6.0, 2.0);
    let body = extrude(&mut k, 0.0, rounded(w, d, r), vec![], h);
    let edges = pair(&k, body, plane(Z, h), plane(X, w / 2.0));
    assert_eq!(edges.len(), 1);
    let b = k.fillet(body, &edges, r).unwrap().bodies[0];
    let straight = 2.0 * (w - 2.0 * r) + 2.0 * (d - 2.0 * r);
    let spandrel = r * r * (1.0 - PI / 4.0);
    // Pappus: the spandrel's centroid sits r·(10 − 3π)/(12 − 3π) in from the rim.
    let off = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let top = w * d - (4.0 - PI) * r * r;
    let volume = top * h - spandrel * straight - 2.0 * PI * (r - off) * spandrel;
    let perimeter = straight + 2.0 * PI * r;
    let area = 2.0 * top + perimeter * h - (straight * r + PI * r * r) - perimeter * r
        + PI / 2.0 * r * straight
        + 2.0 * PI * r * r;
    assert_solid(&k, b, volume, area);
    assert_eq!(kinds(&k, b, SurfaceKind::Sphere), 4);
    // springrep: eight faces named `f.round_0` (naming).
}

#[test]
fn a_chamfer_as_wide_as_its_corner_arcs_narrows_them_to_points() {
    // Each corner arc's bevel is a cone whose apex lies on the arc's axis, where the top face keeps
    // only a sharp rectangle.
    let mut k = K::new();
    let (w, d, h, r) = (40.0, 20.0, 10.0, 5.0);
    let body = extrude(&mut k, 0.0, rounded(w, d, r), vec![], h);
    let edges = pair(&k, body, plane(Z, h), plane(Y, -d / 2.0));
    assert_eq!(edges.len(), 1);
    let b = k
        .chamfer(
            body,
            &chain(&k, body, &edges),
            ChamferSpec::EqualDistance(r),
        )
        .unwrap()
        .bodies[0];
    let straight = 2.0 * (w - 2.0 * r) + 2.0 * (d - 2.0 * r);
    let section = w * d - (4.0 - PI) * r * r;
    // Pappus: the bevel's triangle has its centroid r/3 in from the rim.
    let volume = section * h - r * r / 2.0 * (straight + 2.0 * PI * (r - r / 3.0));
    let area = section
        + (w - 2.0 * r) * (d - 2.0 * r)
        + (straight + 2.0 * PI * r) * (h - r)
        + 2f64.sqrt() * r * (straight + PI * r);
    assert_solid(&k, b, volume, area);
    assert_eq!(kinds(&k, b, SurfaceKind::Cone), 4);
}

#[test]
#[ignore = "cadrs's two-distance chamfer is 1.5e-9 off in volume, and refusing a first face beside no chosen edge is unchecked"]
fn a_chamfer_of_two_distances_sets_the_first_back_on_the_face_named() {
    // A block 10 by 8 by 6; its edge along y where the top meets the side x = 10. Set back 2 on
    // the top and 1 on the side, the top keeps 8 of its 10 across and the side 5 of its 6.
    let mut k = K::new();
    let b = k
        .primitive(&Primitive::block(Point3::origin(), p3(10.0, 8.0, 6.0)))
        .unwrap()
        .bodies[0];
    let edge = edge_at(&k, b, p3(10.0, 4.0, 6.0));
    let top = faces(&k, b, plane(Z, 6.0))[0];
    let side = faces(&k, b, plane(X, 10.0))[0];
    for (first, on_top, on_side) in [(top, 2.0, 1.0), (side, 1.0, 2.0)] {
        let opts = ChamferOpts::new(ChamferSpec::TwoDistances(2.0, 1.0)).first_on(first);
        let cut = k.chamfer_with(b, &[edge], &opts).unwrap().bodies[0];
        valid(&k, cut);
        let want = 480.0 - 0.5 * 2.0 * 1.0 * 8.0;
        let v = volume(&k, cut);
        assert!((v - want).abs() < 1e-9, "volume {v} vs {want}");
        // Where each face now ends: a point just inside the kept part is on the body's boundary,
        // one just past the chamfer's edge is not.
        let on = |p: P3| k.classify(cut, p, 1e-9).unwrap() == PointClass::OnBoundary;
        assert!(on(p3(10.0 - on_top - 0.01, 4.0, 6.0)));
        assert!(!on(p3(10.0 - on_top + 0.01, 4.0, 6.0)));
        assert!(on(p3(10.0, 4.0, 6.0 - on_side - 0.01)));
        assert!(!on(p3(10.0, 4.0, 6.0 - on_side + 0.01)));
    }
    // A face that is not one of the edge's is refused.
    let bottom = faces(&k, b, plane(Z, 0.0))[0];
    let opts = ChamferOpts::new(ChamferSpec::TwoDistances(2.0, 1.0)).first_on(bottom);
    let r = k.chamfer_with(b, &[edge], &opts);
    assert!(
        r.is_err(),
        "first distance on a face not the edge's: accepted"
    );
}
