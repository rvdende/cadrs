//! Ported from springrep's tests/creased_rounds.rs (MIT,
//! https://github.com/springcraft-ai/springrep): rounds that end on a face cut at a slant: where a
//! round on a leaning edge meets its neighbour's they cross in a crease, and a concave round that
//! runs past a short edge is trimmed by the ramp beyond it. springrep's ruled loft between two
//! rectangles is a loft of the two (ruled, as any loft of two sections); edges are picked by the
//! geometry of the faces springrep names.

#![allow(dead_code)]

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

/// A wedge ruled between two rectangles 30 deep, each from `x0` to `x1` on the plane at its `z`
/// (springrep's ruled loft; a loft of two sections is ruled).
fn wedge(k: &mut K, sections: [(f64, f64, f64); 2]) -> BodyId {
    let sections = sections
        .iter()
        .map(|&(z, x0, x1)| {
            let r = rect(0.5 * (x0 + x1), 0.0, x1 - x0, 30.0);
            LoftSection::Profile(Profile::of(Plane::xy(z), vec![r]))
        })
        .collect();
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections,
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 0,
    };
    k.loft_with(&spec).unwrap().bodies[0]
}

/// The 20 × 20 × 10 block with its +x top edge cut back along x + z = 15, a 135° edge on the top.
fn leaning_block(k: &mut K) -> BodyId {
    let block = extrude(k, 0.0, rect(0.0, 0.0, 20.0, 20.0), vec![], 10.0);
    let lean = wedge(k, [(5.0, 10.0, 11.0), (11.0, 4.0, 11.0)]);
    difference(k, block, lean)
}

/// The leaning face, x + z = 15 (springrep's `lean.side`). cadrs's loft makes it a B-spline
/// surface, not a plane, so it is picked by where it lies and which way it faces, not its kind.
fn lean() -> impl Fn(&FaceInfo, &V3) -> bool {
    let n = V3::new(1.0, 0.0, 1.0).normalize();
    move |f, m| {
        m.cross(&n).norm() < 1e-9 && (f.center.coords.dot(&n) - 15.0 / 2f64.sqrt()).abs() < 1e-7
    }
}

/// A triangular prism 10 tall on the xy plane whose first side is its wall.
fn corner_cut(k: &mut K, points: [[f64; 2]; 3]) -> BodyId {
    extrude(k, 0.0, Loop::polygon(&points), vec![], 10.0)
}

/// The wall y = x + 2 (`diag.wall`).
fn diag_wall() -> impl Fn(&FaceInfo, &V3) -> bool {
    plane([-1.0, 1.0, 0.0], 2f64.sqrt())
}

/// The wall y = −x − 2 (`diag2.wall`).
fn diag2_wall() -> impl Fn(&FaceInfo, &V3) -> bool {
    plane([1.0, 1.0, 0.0], -(2f64.sqrt()))
}

#[test]
fn rounds_on_a_square_and_a_leaning_edge_meet_in_a_crease() {
    let mut k = K::new();
    let body = leaning_block(&mut k);
    let mut e = pair(&k, body, plane(Z, 10.0), plane(Y, 10.0));
    e.extend(pair(&k, body, plane(Z, 10.0), lean()));
    assert_eq!(e.len(), 2);
    let b = k.fillet(body, &e, 1.5).unwrap().bodies[0];
    let b = unify(&mut k, b);
    valid(&k, b);
}

#[test]
fn rounds_on_a_diagonal_and_a_leaning_edge_meet_in_a_crease() {
    // A vertical wall along y = x + 2 cuts the block's +y corner, meeting the leaning edge at 45°.
    let mut k = K::new();
    let leaning = leaning_block(&mut k);
    let diag = corner_cut(&mut k, [[-15.0, -13.0], [15.0, 17.0], [-15.0, 17.0]]);
    let body = difference(&mut k, leaning, diag);
    let mut e = pair(&k, body, plane(Z, 10.0), diag_wall());
    e.extend(pair(&k, body, plane(Z, 10.0), lean()));
    assert_eq!(e.len(), 2);
    let b = k.fillet(body, &e, 1.5).unwrap().bodies[0];
    let b = unify(&mut k, b);
    valid(&k, b);
}

#[test]
fn a_leaning_edges_round_creases_into_diagonal_rounds_at_both_ends() {
    // Walls along y = ±(x + 2) cut both of the block's +x corners, so both ends of the
    // leaning edge's round crease into a diagonal round.
    let mut k = K::new();
    let leaning = leaning_block(&mut k);
    let diag = corner_cut(&mut k, [[-15.0, -13.0], [15.0, 17.0], [-15.0, 17.0]]);
    let diag2 = corner_cut(&mut k, [[15.0, -17.0], [-15.0, 13.0], [-15.0, -17.0]]);
    let b = difference(&mut k, leaning, diag);
    let b = difference(&mut k, b, diag2);
    let e = pair(&k, b, diag_wall(), diag2_wall());
    assert_eq!(e.len(), 1);
    let b = k.fillet(b, &e, 3.0).unwrap().bodies[0];
    let mut e = pair(&k, b, plane(Z, 10.0), diag_wall());
    e.extend(pair(&k, b, plane(Z, 10.0), lean()));
    assert_eq!(e.len(), 2);
    let b = k.fillet(b, &e, 1.5).unwrap().bodies[0];
    let b = unify(&mut k, b);
    valid(&k, b);
}

#[test]
#[ignore = "cadrs's fillet or chamfer end against a face it meets at a glancing angle, or past a short edge, is off"]
fn a_concave_round_past_a_short_edge_is_trimmed_by_the_ramp_beyond_it() {
    // A strut 10 × 10 × 10 (x 0–10, y ±5) against a block (x 10–15, y ±15, z 0–15), its top cut
    // back above z = x + 1, which leaves a 1 mm edge of its top at the block. The R2 rounds where
    // its walls meet the block run up to the top; past that 1 mm edge the ramp crosses their strip.
    let mut k = K::new();
    let r = 2.0;
    let block = extrude(&mut k, 0.0, rect(12.5, 0.0, 5.0, 30.0), vec![], 15.0);
    let strut = extrude(&mut k, 0.0, rect(5.0, 0.0, 10.0, 10.0), vec![], 10.0);
    let ramp = wedge(&mut k, [(1.0, -1.0, 0.0), (10.5, -1.0, 9.5)]);
    let b = union(&mut k, block, strut);
    let b = difference(&mut k, b, ramp);
    let mut e = pair(&k, b, plane(Y, -5.0), plane(X, 10.0));
    e.extend(pair(&k, b, plane(Y, 5.0), plane(X, 10.0)));
    assert_eq!(e.len(), 2);
    let b = k.fillet(b, &e, r).unwrap().bodies[0];
    let b = unify(&mut k, b);
    valid(&k, b);
    // Each round's section is the R2 spandrel: at distance u from the block it is
    // L(u) = r − √(r² − (r − u)²) wide, and stands 10 high for u ≤ 1 and 11 − u (the ramp) beyond.
    // With t = r − u and S = √(r² − t²): ∫S dt = (t·S + r²·asin(t/r))/2 and
    // ∫t·S dt = −(r² − t²)^1.5/3.
    let s = |t: f64| (r * r - t * t).max(0.0).sqrt();
    let i0 = |t: f64| 0.5 * (t * s(t) + r * r * (t / r).asin());
    let i1 = |t: f64| -(r * r - t * t).powf(1.5) / 3.0;
    let flat = 10.0 * (r - (i0(r) - i0(r - 1.0)));
    let (t0, t1) = (0.0, r - 1.0);
    let sloped = (11.0 - r) * r * (t1 - t0) + r * (t1 * t1 - t0 * t0) / 2.0
        - ((11.0 - r) * (i0(t1) - i0(t0)) + (i1(t1) - i1(t0)));
    // Block 5 × 30 × 15, the strut less the ramp's 9 × 9 / 2 triangle over its 10 width, and two
    // rounds.
    let want = 2250.0 + (1000.0 - 405.0) + 2.0 * (flat + sloped);
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-9 * want, "volume {got} for {want}");
    // Nothing of the rounds is left past the ramp: no face lies on a wall's plane beyond it.
    let walls = faces(&k, b, |f, n| {
        f.kind == SurfaceKind::Plane
            && n.y.abs() > 1.0 - 1e-12
            && (f.center.y.abs() - 5.0).abs() < 1e-9
    });
    for e in k.edges(b).unwrap() {
        if e.faces
            .iter()
            .any(|f| f.is_some_and(|f| walls.contains(&f)))
        {
            for at in [e.start, e.end] {
                assert!(at.z <= at.x + 1.0 + 1e-9, "a wall face reaches {at:?}");
            }
        }
    }
}
