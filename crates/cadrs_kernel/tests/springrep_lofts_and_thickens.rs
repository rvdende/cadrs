//! Ported from springrep's tests/lofts_and_thickens.rs (MIT,
//! https://github.com/springcraft-ai/springrep): ruled lofts through square sections against the
//! frustum and Cavalieri volumes, and faces thickened by moving them along their outward
//! normals, flat walls and a bore's cylinder. The `Kernel` trait's loft has no ruled option, so
//! springrep's ruled loft through three sections is the native kernel's `thrusections` (ruled);
//! a loft of two sections is ruled anyway. springrep's `offset_faces` is cadrs's offset with
//! per-face distances (sharp). Faces are picked by geometry where springrep picks them by name.

#![allow(dead_code)]

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::{NativeKernel, draw, tess};
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
const Z: [f64; 3] = [0.0, 0.0, 1.0];

/// The `w` × `w` square centred on (`cx`, 0) on the plane at `z`, as a closed polyline.
fn square_wire(k: &K, z: f64, cx: f64, w: f64) -> draw::Shape {
    let h = w / 2.0;
    k.polyline(&[
        [cx - h, -h, z],
        [cx + h, -h, z],
        [cx + h, h, z],
        [cx - h, h, z],
        [cx - h, -h, z],
    ])
    .unwrap()
}

/// The 20 × 20 × 10 block on the xy plane, centred on the origin.
fn block(k: &mut K) -> BodyId {
    extrude(k, 0.0, rect(0.0, 0.0, 20.0, 20.0), vec![], 10.0)
}

/// `solid` with each face in `faces` moved `d` along its outward normal.
fn thickened(k: &mut K, solid: BodyId, faces: Vec<FaceId>, d: f64) -> BodyId {
    assert!(!faces.is_empty());
    let spec = OffsetSpec {
        distance: 0.0,
        faces: faces.into_iter().map(|f| (f, d)).collect(),
        sharp: true,
    };
    let b = k.offset(solid, &spec).unwrap().bodies[0];
    unify(k, b)
}

/// The area of the one face `pick` takes.
fn area_of(k: &K, b: BodyId, pick: impl Fn(&FaceInfo, &V3) -> bool) -> f64 {
    let found: Vec<(FaceInfo, V3)> = face_list(k, b)
        .into_iter()
        .filter(|(f, n)| pick(f, n))
        .collect();
    assert_eq!(found.len(), 1);
    found[0].0.area
}

#[test]
fn a_ruled_loft_through_three_sections_is_two_frustums() {
    // Squares 20, 10 and 20 at z = 0, 10 and 20, ruled: two square frustums 10 high, each
    // h/3 · (A₁ + A₂ + √(A₁A₂)) = 10/3 · 700.
    let mut k = K::new();
    let w = [
        square_wire(&k, 0.0, 0.0, 20.0),
        square_wire(&k, 10.0, 0.0, 10.0),
        square_wire(&k, 20.0, 0.0, 20.0),
    ];
    let draw::Shape::Body(b) = k.thrusections(true, true, &[&w[0], &w[1], &w[2]]).unwrap() else {
        panic!("the loft is not a body")
    };
    let b = unify(&mut k, b);
    valid(&k, b);
    let want = 2.0 * 10.0 / 3.0 * 700.0;
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "{v}");
}

#[test]
fn a_ruled_loft_between_shifted_equal_sections_holds_their_area_all_the_way() {
    // Two 10 mm squares 10 apart, the top one 5 along x: a leaning prism, every level the same
    // square, so by Cavalieri 100 · 10.
    let mut k = K::new();
    let sections = [(0.0, 0.0), (10.0, 5.0)]
        .iter()
        .map(|&(z, cx)| {
            LoftSection::Profile(Profile::of(Plane::xy(z), vec![rect(cx, 0.0, 10.0, 10.0)]))
        })
        .collect();
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections,
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 0,
    };
    let b = k.loft_with(&spec).unwrap().bodies[0];
    let b = unify(&mut k, b);
    valid(&k, b);
    let v = volume(&k, b);
    assert!((v - 1000.0).abs() < 1e-9 * 1000.0, "{v}");
}

#[test]
fn thickening_a_wall_moves_it_out_and_stretches_the_faces_beside_it() {
    // The 20 × 20 × 10 block's +x wall moved out 2: a 22 × 20 × 10 block whose top is 22 × 20.
    let mut k = K::new();
    let bl = block(&mut k);
    let wall = faces(&k, bl, plane(X, 10.0));
    let b = thickened(&mut k, bl, wall, 2.0);
    let v = volume(&k, b);
    assert!((v - 4400.0).abs() < 1e-9 * 4400.0, "{v}");
    assert_eq!(k.face_count(b).unwrap(), 6);
    let top = area_of(&k, b, plane(Z, 10.0));
    assert!((top - 440.0).abs() < 1e-9, "top {top}");
    let wall = area_of(&k, b, plane(X, 12.0));
    assert!((wall - 200.0).abs() < 1e-9, "wall {wall}");
    // Negative takes material away: both side walls in 1 leaves 18 × 20 × 10.
    let mut walls = faces(&k, bl, plane(X, -10.0));
    walls.extend(faces(&k, bl, plane(X, 10.0)));
    let b = thickened(&mut k, bl, walls, -1.0);
    let v = volume(&k, b);
    assert!((v - 3600.0).abs() < 1e-9 * 3600.0, "{v}");
    let bottom = area_of(&k, b, plane(Z, 0.0));
    assert!((bottom - 360.0).abs() < 1e-9, "bottom {bottom}");
}

#[test]
fn thickening_a_bore_closes_it_and_thinning_it_opens_it() {
    // A Ø6 hole through the 10 mm block: its wall's outward normal points into the hole, so +1
    // leaves a Ø4 bore and −1 a Ø8 one.
    let mut k = K::new();
    let bl = block(&mut k);
    let hole = extrude(&mut k, 0.0, circle(0.0, 0.0, 6.0), vec![], 10.0);
    let drilled = difference(&mut k, bl, hole);
    for (d, r) in [(1.0, 2.0), (-1.0, 4.0)] {
        let wall = faces(&k, drilled, |f, _| f.kind == SurfaceKind::Cylinder);
        let b = thickened(&mut k, drilled, wall, d);
        let want = 4000.0 - PI * r * r * 10.0;
        let v = volume(&k, b);
        assert!((v - want).abs() < 1e-9 * want, "{d}: volume {v} vs {want}");
        let wall: f64 = faces(&k, b, |f, _| f.kind == SurfaceKind::Cylinder)
            .iter()
            .map(|&f| k.faces(b).unwrap()[f.0 as usize].area)
            .sum();
        assert!((wall - TAU * r * 10.0).abs() < 1e-8, "{d}: wall {wall}");
    }
}
