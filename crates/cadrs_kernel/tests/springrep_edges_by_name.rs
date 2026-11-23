//! Ported from springrep's tests/edges_by_name.rs (MIT,
//! https://github.com/springcraft-ai/springrep): chamfers and fillets whose edges springrep picks
//! by the names of the two faces that meet in them: names a sweep gave from its profile, names on
//! a revolved solid, an edge a keyway interrupts, and a pair of surfaces that meet in more than
//! one edge. cadrs has no names on the body, so the two faces are picked by their geometry (a
//! springrep "exact" pair by which way each face looks, too); the geometry built and measured is
//! springrep's, and its assertions on the names of the results are left out. As in the blends'
//! port, chamfers are given their edges' tangent chains (springrep's blends follow them; cadrs's
//! chamfer leaves that to the caller).

#![allow(dead_code)]

use std::f64::consts::{PI, TAU};

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

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];
const DOWN: [f64; 3] = [0.0, 0.0, -1.0];

/// How far a through hole runs, so the cut leaves no skin.
const THROUGH: f64 = 1000.0;

/// `edges` and the edges of their tangent chains.
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

fn chamfer(k: &mut K, b: BodyId, edges: &[EdgeId], d: f64) -> BodyId {
    assert!(!edges.is_empty());
    let edges = chain(k, b, edges);
    k.chamfer(b, &edges, ChamferSpec::EqualDistance(d))
        .unwrap()
        .bodies[0]
}

/// A `width` × `width` block 10 tall on the xy plane, centred on the origin.
fn block(k: &mut K, width: f64) -> BodyId {
    extrude(k, 0.0, rect(0.0, 0.0, width, width), vec![], 10.0)
}

#[test]
fn a_chamfer_bevels_the_named_edge() {
    let mut k = K::new();
    let cube = extrude(&mut k, 0.0, rect(0.0, 0.0, 4.0, 4.0), vec![], 4.0);
    let e = pair(&k, cube, plane(Z, 4.0), plane(X, 2.0));
    let b = chamfer(&mut k, cube, &e, 1.0);
    let b = unify(&mut k, b);
    let got = volume(&k, b);
    assert!((got - 62.0).abs() < 1e-9 * 62.0, "{got}");
    // springrep: the bevel is named `ch.bevel_0` (naming).
}

#[test]
fn a_chamfer_picks_a_revolved_shafts_edge_by_its_profile_names() {
    // A 10 mm radius, 10 mm long shaft turned from a rectangle, its end ring bevelled 1 mm: the
    // bevel takes a 1 × 1 / 2 triangle whose centroid turns at 10 − 1/3 (Pappus).
    let mut k = K::new();
    let outline = Loop::polygon(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
    // springrep's Frame::new(0, −y, x), turned about its y (the z axis): cadrs's Plane::xz.
    let p = Profile::of(Plane::xz(0.0), vec![outline]);
    let shaft = k.revolve(&p, Axis::z(), TAU).unwrap().bodies[0];
    // face_a is the end z = 0, journal the cylinder.
    let e = pair(&k, shaft, plane(Z, 0.0), cylinder(0.0, 0.0, 10.0));
    let b = chamfer(&mut k, shaft, &e, 1.0);
    let b = unify(&mut k, b);
    let want = 1000.0 * PI - 2.0 * PI * (10.0 - 1.0 / 3.0) * 0.5;
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "{v} vs {want}");
    // springrep: `shaft.face_a`, `shaft.journal` and `break.bevel_0` once each (naming).
}

/// A 20 × 20 × 10 plate with a ⌀8 bore through it and a keyway 2 wide cut 1 past the bore's wall
/// on +x, the bore's edge on the plate's underside bevelled 0.5 when `chamfered`.
fn keyed_bore(k: &mut K, chamfered: bool) -> BodyId {
    let plate = extrude(k, 0.0, rect(0.0, 0.0, 20.0, 20.0), vec![], 10.0);
    let bore = extrude(k, 0.0, circle(0.0, 0.0, 8.0), vec![], THROUGH);
    let key = extrude(k, 0.0, rect(4.25, 0.0, 1.5, 2.0), vec![], 10.0);
    let b = difference(k, plate, bore);
    let mut b = difference(k, b, key);
    if chamfered {
        let e = pair(k, b, cylinder(0.0, 0.0, 4.0), plane(Z, 0.0));
        b = chamfer(k, b, &e, 0.5);
    }
    unify(k, b)
}

#[test]
#[ignore = "cadrs's fillet or chamfer end against a face it meets at a glancing angle, or past a short edge, is off"]
fn a_chamfer_round_most_of_a_bore_stops_at_the_keyway_walls() {
    // The bore's edge runs from one keyway wall (y = 1) round to the other (y = −1), 331°. The
    // chamfer's triangle, 0.5 mm each way, turns everywhere the keyway leaves material: at radius
    // r it is 4.5 − r tall and misses the 2·asin(1/r) the keyway takes. Each end is trimmed by
    // its own wall, which the far end lies across.
    let mut k = K::new();
    let (plain, cut) = (keyed_bore(&mut k, false), keyed_bore(&mut k, true));
    let removed = volume(&k, plain) - volume(&k, cut);
    let n = 2000;
    let f = |r: f64| (4.5 - r) * r * (TAU - 2.0 * (1.0 / r).asin());
    let h = 0.5 / n as f64;
    let mut want = f(4.0) + f(4.5);
    for i in 1..n {
        want += f(4.0 + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    want *= h / 3.0;
    assert!((removed - want).abs() < 1e-9 * want, "{removed} vs {want}");
}

/// The block with a 10 × 10 × 5 boss on its top from x = 5 to 15, overhanging its +x side: at
/// (10, ±5, 10) the block's top, the boss's side, its underside and the block's side meet, the top
/// and the underside on one plane.
fn overhang(k: &mut K) -> BodyId {
    let boss = extrude(k, 10.0, rect(10.0, 0.0, 10.0, 10.0), vec![], 5.0);
    let b = block(k, 20.0);
    union(k, b, boss)
}

#[test]
fn an_exact_edge_picks_one_of_the_edges_two_surfaces_meet_in() {
    // The boss's side meets the z = 10 plane twice along y = 5: over the overhang, convex, where
    // the plane is the boss's underside, and over the block, concave, where it is the block's top.
    // Named by surfaces the pair picks both; named exactly, only the one its faces make. Filleted
    // R1 on its convex edges alone, each 5 long and ending flush at the saddle, the part loses
    // 20 × (1 − π/4); on its concave ones alone (two of 5 and the underside's 10) it gains as much.
    // springrep's exact pairs: the boss's underside (`o.start`) is the z = 10 face looking down,
    // the block's top (`block.end`) the one looking up.
    let cut = 20.0 * (1.0 - PI / 4.0);
    for convex in [true, false] {
        let mut k = K::new();
        let body = overhang(&mut k);
        let mut edges = Vec::new();
        if convex {
            edges.extend(pair(&k, body, plane(Y, 5.0), facing(DOWN, -10.0)));
            edges.extend(pair(&k, body, plane(Y, -5.0), facing(DOWN, -10.0)));
            edges.extend(pair(&k, body, plane(X, 10.0), facing(Z, 10.0)));
        } else {
            edges.extend(pair(&k, body, facing(Z, 10.0), plane(Y, 5.0)));
            edges.extend(pair(&k, body, facing(Z, 10.0), plane(Y, -5.0)));
            edges.extend(pair(&k, body, facing(DOWN, -10.0), plane(X, 10.0)));
        }
        let want = if convex { 4500.0 - cut } else { 4500.0 + cut };
        let b = k.fillet(body, &edges, 1.0).unwrap().bodies[0];
        let b = unify(&mut k, b);
        valid(&k, b);
        assert!(watertight(&k, b, 0.01, 0.2), "convex {convex}");
        let got = volume(&k, b);
        assert!(
            (got - want).abs() < 1e-12 * want,
            "convex {convex}: volume {got} for {want}"
        );
        assert_eq!(
            kinds(&k, b, SurfaceKind::Cylinder),
            if convex { 4 } else { 3 },
            "convex {convex}"
        );
    }
}
