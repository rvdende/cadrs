//! Ported from springrep's tests/edges_by_id.rs (MIT, https://github.com/springcraft-ai/springrep):
//! chamfers and fillets of edges picked by id. springrep checks that the result's faces keep
//! the names they had and that the blend faces are named under the feature; cadrs names faces
//! through `History`, not on the body, so those assertions are left out and what they imply
//! geometrically is kept: the volumes, and that each result has the block's six faces and one
//! blend face (the grown block, six faces, twelve rounds and eight balls' patches).

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

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];

/// A 20 × 20 block 10 tall, and its one edge between the `u_max` (x = 10) and `v_max` (y = 10)
/// sides.
fn block_and_edge(k: &mut K) -> (BodyId, Vec<EdgeId>) {
    let b = extrude(k, 0.0, rect(0.0, 0.0, 20.0, 20.0), vec![], 10.0);
    let edges = pair(k, b, plane(X, 10.0), plane(Y, 10.0));
    assert_eq!(edges.len(), 1);
    (b, edges)
}

#[test]
fn a_fillet_of_an_edge_by_id_keeps_every_face_name() {
    let mut k = K::new();
    let (b, edges) = block_and_edge(&mut k);
    let out = k.fillet(b, &edges, 2.0).unwrap().bodies[0];
    // The corner's 2 × 2 square less a quarter disc of radius 2 goes, 10 tall.
    let want = 4000.0 - 10.0 * (4.0 - PI);
    let v = volume(&k, out);
    assert!((v - want).abs() < 1e-9 * want, "volume {v} vs {want}");
    // springrep: the six faces keep their names and the round is `f.round_0` (naming).
    assert_eq!(k.face_count(out).unwrap(), 7);
}

#[test]
fn a_chamfer_of_an_edge_by_id_keeps_every_face_name() {
    let mut k = K::new();
    let (b, edges) = block_and_edge(&mut k);
    let out = k
        .chamfer(b, &edges, ChamferSpec::EqualDistance(2.0))
        .unwrap()
        .bodies[0];
    // The corner's 2 × 2 triangle goes, 10 tall.
    let want = 4000.0 - 10.0 * 2.0;
    let v = volume(&k, out);
    assert!((v - want).abs() < 1e-9 * want, "volume {v} vs {want}");
    // springrep: the six faces keep their names and the bevel is `c.bevel_0` (naming).
    assert_eq!(k.face_count(out).unwrap(), 7);
}

#[test]
fn a_shaped_blend_names_what_it_makes_under_its_feature() {
    let mut k = K::new();
    let (b, edges) = block_and_edge(&mut k);
    // springrep's `fillet_radii` (a radius per edge): cadrs's variable fillet, 2 all along.
    let law = FilletLaw {
        edge: edges[0],
        radii: vec![(0.0, 2.0), (1.0, 2.0)],
    };
    let radii = k.fillet_variable(b, &[law], true).unwrap().bodies[0];
    assert_eq!(k.face_count(radii).unwrap(), 7);
    // springrep's `fillet_conic(…, 2.0, 0.5)`: a conic section of size 2, ρ 0.5.
    let spec = FilletSpec {
        profile: FilletProfile::Conic { rho: 0.5 },
        ..FilletSpec::radius(2.0)
    };
    let conic = k.fillet_with(b, &edges, &spec).unwrap().bodies[0];
    assert_eq!(k.face_count(conic).unwrap(), 7);
    // springrep's `chamfer_distances` (a distance per edge).
    let bevels = k
        .chamfer(b, &edges, ChamferSpec::EqualDistance(2.0))
        .unwrap()
        .bodies[0];
    assert_eq!(k.face_count(bevels).unwrap(), 7);
    // A block grown by 1 mm: its six faces, a round on each of twelve edges, a ball's patch at
    // each of eight corners.
    let spec = OffsetSpec {
        distance: 1.0,
        faces: vec![],
        sharp: false,
    };
    let grown = k.offset(b, &spec).unwrap().bodies[0];
    assert_eq!(kinds(&k, grown, SurfaceKind::Plane), 6);
    assert_eq!(kinds(&k, grown, SurfaceKind::Cylinder), 12);
    assert_eq!(kinds(&k, grown, SurfaceKind::Sphere), 8);
    assert_eq!(k.face_count(grown).unwrap(), 6 + 12 + 8);
    // springrep: the names `r.round`, `k.round`, `c.bevel`, `g.corner` and `g.round_*` (naming).
}
