//! Ported from springrep's tests/sweeps.rs (MIT, https://github.com/springcraft-ai/springrep):
//! swept solids against closed-form volume, area and box, each checked valid and watertight.
//! springrep's interpolating splines are built with the native kernel's interpolation (the same
//! cubics: chord-length parameters, periodic or not) and given to cadrs as sketch splines. Its
//! assertions on the names the sweeps give are left out; where they imply a face count, that is
//! checked.

#![allow(dead_code)]

use std::f64::consts::PI;

use cadrs_kernel::native::{NativeKernel, NurbsCurve, tess};
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

/// springrep's COARSE mesh options for this file.
const COARSE: Tessellation = Tessellation {
    deflection: 0.05,
    angle: 0.3,
};

/// The volume a closed mesh holds.
fn mesh_volume(m: &TriMesh) -> f64 {
    m.indices
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| m.positions[i as usize].coords);
            a.dot(&b.cross(&c)) / 6.0
        })
        .sum()
}

fn assert_solid(k: &K, b: BodyId, volume: f64, area: f64, lo: [f64; 3], hi: [f64; 3], rel: f64) {
    valid(k, b);
    let v = self::volume(k, b);
    let a = self::area(k, b);
    assert!((v - volume).abs() <= rel * volume, "volume {v} vs {volume}");
    assert!((a - area).abs() <= rel * area, "area {a} vs {area}");
    let bb = k.bounding_box(b).unwrap();
    let d = (bb.min - P3::from(lo))
        .abs()
        .max()
        .max((bb.max - P3::from(hi)).abs().max());
    assert!(d < 1e-6, "bbox {bb:?}");
    let m = k.tessellate(b, COARSE).unwrap();
    let c = tess::check_tri_mesh(&m);
    assert!(c.is_watertight(), "{c:?}");
    let mv = mesh_volume(&m);
    assert!((mv - volume).abs() < 0.02 * volume, "mesh volume {mv}");
}

/// springrep's `xz_frame`, `Frame::new(0, −y, x)`: sketch x along x, sketch y along z.
fn xz() -> Plane {
    Plane::xz(0.0)
}

fn revolve(k: &mut K, outline: Loop, angle: f64) -> BodyId {
    let p = Profile::of(xz(), vec![outline]);
    k.revolve(&p, Axis::z(), angle).unwrap().bodies[0]
}

#[test]
fn box_extrusion_is_exact_and_named() {
    let mut k = K::new();
    let b = extrude(&mut k, 0.0, rect(5.0, 2.0, 10.0, 4.0), vec![], 3.0);
    assert_solid(
        &k,
        b,
        120.0,
        2.0 * (40.0 + 30.0 + 12.0),
        [0.0, 0.0, 0.0],
        [10.0, 4.0, 3.0],
        1e-12,
    );
    // springrep: the faces are named start, end, v_min, u_max, v_max and u_min, once each
    // (naming); six faces.
    assert_eq!(k.face_count(b).unwrap(), 6);
}

#[test]
fn plate_with_a_hole_either_way_along_its_normal() {
    let mut k = K::new();
    let (outer, hole) = (rect(0.0, 0.0, 10.0, 6.0), circle(2.0, 0.0, 2.0));
    let down = Plane::new(Point3::origin(), Vector3::x(), -Vector3::z());
    let p = Profile::with_holes(down, outer.clone(), vec![hole.clone()]);
    let b = k.extrude(&p, Extent::Blind(2.0)).unwrap().bodies[0];
    let v = 2.0 * (60.0 - PI);
    let a = 2.0 * (60.0 - PI) + 2.0 * 32.0 + 2.0 * PI * 2.0;
    assert_solid(&k, b, v, a, [-5.0, -3.0, -2.0], [5.0, 3.0, 0.0], 1e-12);
    let p = Profile::with_holes(Plane::xy(0.0), outer, vec![hole]);
    let negative = k.extrude(&p, Extent::Blind(-2.0)).unwrap().bodies[0];
    assert_solid(
        &k,
        negative,
        v,
        a,
        [-5.0, -3.0, -2.0],
        [5.0, 3.0, 0.0],
        1e-12,
    );
    // springrep: the cap on the sketch plane is named `p.start` (naming).
}

#[test]
fn arcs_in_a_profile_sweep_cylinders_both_ways() {
    // A 10 × 10 square with a half-disc bump on the right and a half-disc bite on the left.
    let mut k = K::new();
    let lp = Loop::new(vec![
        Curve2::line([0.0, 0.0], [10.0, 0.0]),
        Curve2::arc([10.0, 5.0], 5.0, -PI / 2.0, PI),
        Curve2::line([10.0, 10.0], [0.0, 10.0]),
        Curve2::line([0.0, 10.0], [0.0, 8.0]),
        Curve2::arc([0.0, 5.0], 3.0, PI / 2.0, -PI),
        Curve2::line([0.0, 2.0], [0.0, 0.0]),
    ]);
    let b = extrude(&mut k, 0.0, lp, vec![], 4.0);
    let area2d = 100.0 + PI * 12.5 - PI * 4.5;
    let perim = 10.0 + PI * 5.0 + 10.0 + 2.0 + PI * 3.0 + 2.0;
    assert_solid(
        &k,
        b,
        4.0 * area2d,
        2.0 * area2d + 4.0 * perim,
        [0.0, 0.0, 0.0],
        [15.0, 10.0, 4.0],
        1e-12,
    );
}

/// The closed cubic through `pts` at chord-length parameters (springrep's
/// `BSpline::interpolate_periodic`), as a sketch curve.
fn periodic_spline(pts: &[[f64; 2]]) -> Curve2 {
    let p3: Vec<P3> = pts.iter().map(|p| p3(p[0], p[1], 0.0)).collect();
    let n = p3.len();
    let mut params = vec![0.0];
    for i in 0..n {
        params.push(params[i] + (p3[(i + 1) % n] - p3[i]).norm());
    }
    let c = NurbsCurve::interpolate_periodic(&p3, &params, 3).unwrap();
    let poles: Vec<[f64; 2]> = c.cps.iter().map(|h| [h[0] / h[3], h[1] / h[3]]).collect();
    Curve2::spline(c.degree, &c.knots, &poles, &[])
}

/// The open cubic through `pts` at chord-length parameters (springrep's `BSpline::interpolate`).
fn open_spline(pts: &[[f64; 2]]) -> Curve2 {
    let p3: Vec<P3> = pts.iter().map(|p| p3(p[0], p[1], 0.0)).collect();
    let mut params = vec![0.0];
    for i in 1..p3.len() {
        params.push(params[i - 1] + (p3[i] - p3[i - 1]).norm());
    }
    let c = NurbsCurve::interpolate(&p3, &params, 3, None, None);
    let poles: Vec<[f64; 2]> = c.cps.iter().map(|h| [h[0] / h[3], h[1] / h[3]]).collect();
    Curve2::spline(c.degree, &c.knots, &poles, &[])
}

#[test]
fn periodic_spline_disc_matches_its_ellipse() {
    let mut k = K::new();
    let pts: Vec<[f64; 2]> = (0..72)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / 72.0;
            [30.0 * a.cos(), 20.0 * a.sin()]
        })
        .collect();
    let b = extrude(
        &mut k,
        0.0,
        Loop::new(vec![periodic_spline(&pts)]),
        vec![],
        10.0,
    );
    valid(&k, b);
    let v = volume(&k, b);
    assert!((v / (PI * 6000.0) - 1.0).abs() < 2e-5, "{v}");
    assert_eq!(k.face_count(b).unwrap(), 3);
    let bb = k.bounding_box(b).unwrap();
    assert!(
        (bb.max.x - 30.0).abs() < 1e-3 && (bb.min.y + 20.0).abs() < 1e-3,
        "{bb:?}"
    );
}

#[test]
fn partial_revolve_of_a_square() {
    let mut k = K::new();
    let p = Loop::polygon(&[[10.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0]]);
    let b = revolve(&mut k, p, PI / 2.0);
    let v = PI * 300.0 * 10.0 / 4.0;
    let a = 200.0 + 2.0 * PI * 300.0 / 4.0 + 2.0 * PI * 200.0 / 4.0 + 2.0 * PI * 100.0 / 4.0;
    assert_solid(&k, b, v, a, [0.0, 0.0, 0.0], [20.0, 20.0, 10.0], 1e-12);
    // springrep: six distinct face names (naming); six faces.
    assert_eq!(k.face_count(b).unwrap(), 6);
}

#[test]
fn partial_revolve_touching_the_axis_has_a_pole() {
    // A right triangle on the axis turned 120°: a cone sector closed by two caps.
    let mut k = K::new();
    let p = Loop::polygon(&[[0.0, 0.0], [6.0, 0.0], [0.0, 8.0]]);
    let b = revolve(&mut k, p, 2.0 * PI / 3.0);
    let v = PI * 36.0 * 8.0 / 3.0 / 3.0;
    let a = (PI * 36.0 + PI * 6.0 * 10.0) / 3.0 + 2.0 * 24.0;
    assert_solid(&k, b, v, a, [-3.0, 0.0, 0.0], [6.0, 6.0, 8.0], 1e-12);
}

#[test]
fn full_revolve_of_a_half_disc_is_a_sphere() {
    let mut k = K::new();
    let p = Loop::new(vec![
        Curve2::arc([0.0, 0.0], 5.0, -PI / 2.0, PI),
        Curve2::line([0.0, 5.0], [0.0, -5.0]),
    ]);
    let b = revolve(&mut k, p, 2.0 * PI);
    assert_solid(
        &k,
        b,
        4.0 / 3.0 * PI * 125.0,
        4.0 * PI * 25.0,
        [-5.0; 3],
        [5.0; 3],
        1e-11,
    );
}

#[test]
fn full_revolve_of_an_off_axis_circle_is_a_torus() {
    let mut k = K::new();
    let b = revolve(&mut k, circle(10.0, 0.0, 4.0), 2.0 * PI);
    let v = 2.0 * PI * PI * 10.0 * 4.0;
    let a = 4.0 * PI * PI * 10.0 * 2.0;
    assert_solid(&k, b, v, a, [-12.0, -12.0, -2.0], [12.0, 12.0, 2.0], 1e-11);
}

#[test]
fn turned_stack_with_a_cone() {
    let mut k = K::new();
    let pts = [
        [0.0, 0.0],
        [20.0, 0.0],
        [20.0, 10.0],
        [10.0, 10.0],
        [10.0, 28.0],
        [8.0, 30.0],
        [0.0, 30.0],
    ];
    let b = revolve(&mut k, Loop::polygon(&pts), 2.0 * PI);
    let frustum = PI * 2.0 / 3.0 * (100.0 + 80.0 + 64.0);
    let v = PI * 400.0 * 10.0 + PI * 100.0 * 18.0 + frustum;
    let a = 400.0 * PI
        + 400.0 * PI
        + 300.0 * PI
        + 360.0 * PI
        + 64.0 * PI
        + PI * 18.0 * 2.0 * 2f64.sqrt();
    assert_solid(&k, b, v, a, [-20.0, -20.0, 0.0], [20.0, 20.0, 30.0], 1e-12);
    assert_eq!(k.face_count(b).unwrap(), 6);
}

#[test]
fn revolving_a_spline_makes_a_surface_of_revolution() {
    // A spline bulge on the outside of a tube section: the volume by Pappus would need the
    // spline's centroid, so check against the tessellation instead, and validity exactly.
    let mut k = K::new();
    let spline = open_spline(&[[10.0, 0.0], [12.0, 3.0], [11.0, 7.0], [10.0, 10.0]]);
    let p = Loop::new(vec![
        Curve2::line([5.0, 0.0], [10.0, 0.0]),
        spline,
        Curve2::line([10.0, 10.0], [5.0, 10.0]),
        Curve2::line([5.0, 10.0], [5.0, 0.0]),
    ]);
    let b = revolve(&mut k, p, 2.0 * PI);
    valid(&k, b);
    let v = volume(&k, b);
    let m = k.tessellate(b, COARSE).unwrap();
    let c = tess::check_tri_mesh(&m);
    assert!(c.is_watertight(), "{c:?}");
    let mv = mesh_volume(&m);
    assert!((mv - v).abs() < 5e-3 * v, "{mv} vs {v}");
    let bb = k.bounding_box(b).unwrap();
    assert!(bb.max.x > 12.0 && bb.max.x < 12.5, "{bb:?}");
}

#[test]
fn bad_dimensions_are_invalid_input() {
    let mut k = K::new();
    let mut try_extrude = |w: f64, h: f64| {
        let p = Profile::of(Plane::xy(0.0), vec![rect(0.0, 0.0, w, 1.0)]);
        k.extrude(&p, Extent::Blind(h)).map(|r| r.bodies)
    };
    let bad = [
        try_extrude(f64::NAN, 1.0),
        try_extrude(1e-9, 1.0),
        try_extrude(1.0, f64::INFINITY),
        try_extrude(1.0, f64::NAN),
    ];
    let accepted: Vec<_> = bad
        .into_iter()
        .enumerate()
        .filter(|(_, r)| r.is_ok())
        .collect();
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
}
