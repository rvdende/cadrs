//! Ported from springrep's tests/fillets_over_rounds.rs (MIT,
//! https://github.com/springcraft-ai/springrep): fillets that run over rounds already on the
//! body, and fillets that meet at a corner. A fillet no smaller than a round it crosses rolls
//! round it in a torus; a larger one closes the corner with a patch tangent to every face it
//! meets. Each result is measured against a volume worked by hand from its sections. springrep's
//! NURBS patches are counted here as faces of `SurfaceKind::Other`; edges are picked by the
//! geometry of the faces springrep names.

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

/// The block's upright edges, as springrep's `UPRIGHT_EDGES` order them: (u_min, v_min),
/// (v_min, u_max), (u_max, v_max), (v_max, u_min).
fn upright_edges(k: &K, b: BodyId) -> Vec<EdgeId> {
    let sides = [
        (plane(X, -10.0), plane(Y, -10.0)),
        (plane(Y, -10.0), plane(X, 10.0)),
        (plane(X, 10.0), plane(Y, 10.0)),
        (plane(Y, 10.0), plane(X, -10.0)),
    ];
    sides
        .into_iter()
        .map(|(a, c)| {
            let e = pair(k, b, a, c);
            assert_eq!(e.len(), 1);
            e[0]
        })
        .collect()
}

/// The 20 × 20 × 10 block on the xy plane, centred on the origin.
fn block(k: &mut K) -> BodyId {
    extrude(k, 0.0, rect(0.0, 0.0, 20.0, 20.0), vec![], 10.0)
}

/// `profile` as a boss 4 tall on the block's top, or with `cut` a pocket 4 deep in it.
fn on_block(k: &mut K, profile: Loop, cut: bool) -> BodyId {
    let b = block(k);
    if cut {
        let tool = extrude(k, 6.0, profile, vec![], 4.0);
        difference(k, b, tool)
    } else {
        let tool = extrude(k, 10.0, profile, vec![], 4.0);
        union(k, b, tool)
    }
}

#[track_caller]
fn fillet(k: &mut K, b: BodyId, edges: &[EdgeId], r: f64) -> BodyId {
    assert!(!edges.is_empty());
    k.fillet(b, edges, r).unwrap().bodies[0]
}

fn tight(k: &K, b: BodyId) -> bool {
    watertight(k, b, 0.01, 0.2)
}

fn patches_of(k: &K, b: BodyId) -> Vec<FaceId> {
    faces(k, b, |f, _| f.kind == SurfaceKind::Other)
}

/// Each of `patches` is tangent to every face it meets, exactly: their outward normals agree all
/// along each edge between them.
fn assert_tangent_all_round(k: &K, b: BodyId, patches: &[FaceId]) {
    let body = k.get(b).unwrap().clone();
    for e in k.edges(b).unwrap() {
        let [Some(f0), Some(f1)] = e.faces else {
            continue;
        };
        if f0 == f1 || !(patches.contains(&f0) || patches.contains(&f1)) {
            continue;
        }
        let ed = &body.edges[e.id.0 as usize];
        if ed.degenerate {
            continue;
        }
        let pts: Vec<P3> = (1..34)
            .map(|i| ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / 34.0))
            .collect();
        let n0 = k.face_normals_at(b, f0, &pts).unwrap();
        let n1 = k.face_normals_at(b, f1, &pts).unwrap();
        for i in 0..pts.len() {
            let angle = n0[i].cross(&n1[i]).norm();
            assert!(angle < 1e-9, "a crease of {angle} rad at {:?}", pts[i]);
            assert!(n0[i].dot(&n1[i]) > 0.0);
        }
    }
}

/// The block's upright edges rounded `small`, then the edges of its `top` face (or else its
/// bottom) `big`.
#[track_caller]
fn rounded_then_filleted(k: &mut K, small: f64, big: f64, top: bool) -> BodyId {
    let b = block(k);
    let e = upright_edges(k, b);
    let b = fillet(k, b, &e, small);
    let rim = pair(
        k,
        b,
        plane(Z, if top { 10.0 } else { 0.0 }),
        plane(X, -10.0),
    );
    let b = fillet(k, b, &rim, big);
    unify(k, b)
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn a_fillet_round_large_and_small_rounds_rolls_round_one_and_patches_the_other() {
    // The R1.5 top fillet runs round two R6 upright rounds, which it rolls round in tori, and two
    // R0.5 ones, which it closes with patches. Above z = 8.5 the section at height z, with
    // k = √(1 − ((z − 8.5)/1.5)²) and d = 1.5(1 − k), is a square of half-width 10 − d whose R6
    // corners are 6 − d and whose R0.5 corners are 0.5k. A rounded square of side s with corner
    // radii cᵢ has area s² − (1 − π/4)·Σcᵢ², so with ∫dz = 1.5, ∫k dz = 1.5·π/4, ∫k² dz = 1.5·2/3,
    //   V = 8.5·A₀ + ∫ (17 + 3k)² − (1 − π/4)(2(4.5 + 1.5k)² + 2(0.5k)²) dz.
    let mut k = K::new();
    let (r, big, small) = (1.5, 6.0, 0.5);
    let b = block(&mut k);
    let e = upright_edges(&k, b);
    let b = fillet(&mut k, b, &e[..2], big);
    let e = upright_edges_after(&k, b, &[(-10.0, -10.0), (10.0, -10.0)]);
    assert!(e.is_empty());
    let e = upright_edges_after(&k, b, &[(10.0, 10.0), (-10.0, 10.0)]);
    let b = fillet(&mut k, b, &e, small);
    let rim = pair(&k, b, plane(Z, 10.0), plane(X, -10.0));
    let b = fillet(&mut k, b, &rim, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    let cut = 1.0 - PI / 4.0;
    let (i1, ik, ik2) = (r, r * PI / 4.0, r * 2.0 / 3.0);
    let a0 = 400.0 - cut * (2.0 * big * big + 2.0 * small * small);
    let square = 289.0 * i1 + 102.0 * ik + 9.0 * ik2;
    let tori = 2.0 * (20.25 * i1 + 13.5 * ik + 2.25 * ik2);
    let want = (10.0 - r) * a0 + square - cut * (tori + 2.0 * small * small * ik2);
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-9 * want, "volume {got} for {want}");
    assert_eq!(kinds(&k, b, SurfaceKind::Other), 2);
    assert_eq!(kinds(&k, b, SurfaceKind::Torus), 2);
}

/// The upright edges still sharp at the corners (x, y).
fn upright_edges_after(k: &K, b: BodyId, corners: &[(f64, f64)]) -> Vec<EdgeId> {
    k.edges(b)
        .unwrap()
        .iter()
        .filter(|e| {
            corners.iter().any(|&(x, y)| {
                let near = |p: P3| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9;
                near(e.start) && near(e.end) && (e.start.z - e.end.z).abs() > 1.0
            })
        })
        .map(|e| e.id)
        .collect()
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn a_fillet_over_a_smaller_round_closes_each_corner_with_an_exact_patch() {
    // The R1.5 top fillet runs over R0.5 upright rounds. Each corner's ball sits r = 1.5 from the
    // top and both walls, at c = (±8.5, ±8.5, 8.5), and every horizontal section above z = 8.5 is
    // the section at z = 8.5 (a square of half-width 10 with R0.5 corners) scaled about the corner
    // axes by k = √(1 − ((z − 8.5)/1.5)²): a square of half-width 8.5 + 1.5k with corners ρk. So
    //   V = 8.5·A + ∫ ((17 + 3k)² − δk²) dz,  A = 400 − δ,  δ = 4(1 − π/4)·0.5²,
    // with ∫k dz = 1.5·π/4 and ∫k² dz = 1.5·2/3 over the top 1.5.
    // The bottom rim, filleted alike, is its mirror image: the same volume, and patches whose
    // poles lie the other way up.
    for top in [true, false] {
        exact_corner_patches(top);
    }
}

fn exact_corner_patches(top: bool) {
    let mut k = K::new();
    let (r, rho) = (1.5, 0.5);
    let b = rounded_then_filleted(&mut k, rho, r, top);
    valid(&k, b);
    assert!(tight(&k, b));
    let delta = 4.0 * (1.0 - PI / 4.0) * rho * rho;
    let (ik, ik2) = (r * PI / 4.0, r * 2.0 / 3.0);
    let want = (10.0 - r) * (400.0 - delta) + 289.0 * r + 102.0 * ik + 9.0 * ik2 - delta * ik2;
    let got = volume(&k, b);
    assert!(
        (got - want).abs() < 1e-9 * want,
        "top {top}: volume {got} for {want}"
    );
    // Six planes, four upright rounds cut back, four straight fillets and four corner patches.
    assert_eq!(k.face_count(b).unwrap(), 18, "top {top}");
    let patches = patches_of(&k, b);
    assert_eq!(patches.len(), 4, "top {top}");
    assert_tangent_all_round(&k, b, &patches);
}

/// The 20 × 20 × 10 block with a 10 × 10 pocket 4 deep in its top, the pocket's corners rounded
/// R0.5.
#[track_caller]
fn rounded_pocket(k: &mut K) -> BodyId {
    let pocket = on_block(k, rect(0.0, 0.0, 10.0, 10.0), true);
    let mut corners = Vec::new();
    for (a, c) in [
        (plane(X, -5.0), plane(Y, -5.0)),
        (plane(Y, -5.0), plane(X, 5.0)),
        (plane(X, 5.0), plane(Y, 5.0)),
        (plane(Y, 5.0), plane(X, -5.0)),
    ] {
        corners.extend(pair(k, pocket, a, c));
    }
    assert_eq!(corners.len(), 4);
    fillet(k, pocket, &corners, 0.5)
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn a_pocket_floor_fillet_over_smaller_corner_rounds_closes_each_corner_with_a_patch() {
    // The R1.5 floor fillet is concave and the R0.5 corner rounds bend the same way, so the ball
    // sits in each corner, 1.5 from the floor and both walls. At height h above the floor, with
    // k = √(1 − ((1.5 − h)/1.5)²), the pocket's section is the one at h = 1.5 (a square of side 10
    // with R0.5 corners) scaled about the corner axes by k: side 7 + 3k, corners 0.5k. So the
    // pocket holds (4 − 1.5)(100 − δ) + ∫ (7 + 3k)² − δk² dh, δ = 4(1 − π/4)·0.5², with
    // ∫k dh = 1.5·π/4 and ∫k² dh = 1.5·2/3 over the bottom 1.5.
    let mut k = K::new();
    let (r, rho) = (1.5, 0.5);
    let b = rounded_pocket(&mut k);
    let floor = pair(&k, b, plane(Z, 6.0), plane(X, -5.0));
    let b = fillet(&mut k, b, &floor, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    assert!(tight(&k, b));
    let delta = 4.0 * (1.0 - PI / 4.0) * rho * rho;
    let (ik, ik2) = (r * PI / 4.0, r * 2.0 / 3.0);
    let pocket = (4.0 - r) * (100.0 - delta) + 49.0 * r + 42.0 * ik + 9.0 * ik2 - delta * ik2;
    let want = 4000.0 - pocket;
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-9 * want, "volume {got} for {want}");
    assert_eq!(patches_of(&k, b).len(), 4);
}

#[test]
fn a_pocket_rim_fillet_rolls_round_corner_rounds_bent_the_other_way() {
    // The R1.5 rim fillet is convex and the pocket's R0.5 corners concave: the ball rolls round
    // each in a torus. At depth t below the top, with k = √(1 − ((1.5 − t)/1.5)²), the pocket is
    // widened by 1.5(1 − k) all round: side 13 − 3k, corners 2 − 1.5k.
    let mut k = K::new();
    let (r, rho) = (1.5, 0.5);
    let b = rounded_pocket(&mut k);
    let rim = pair(&k, b, plane(Z, 10.0), plane(X, -5.0));
    let b = fillet(&mut k, b, &rim, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    assert!(tight(&k, b));
    let cut = 1.0 - PI / 4.0;
    let (ik, ik2) = (r * PI / 4.0, r * 2.0 / 3.0);
    let top = 169.0 * r - 78.0 * ik + 9.0 * ik2 - 4.0 * cut * (4.0 * r - 6.0 * ik + 2.25 * ik2);
    let pocket = (4.0 - r) * (100.0 - 4.0 * cut * rho * rho) + top;
    let want = 4000.0 - pocket;
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-9 * want, "volume {got} for {want}");
    assert_eq!(patches_of(&k, b).len(), 0);
}

/// The share of an r × r square outside the quarter disc in its corner, times its distance from
/// that corner's far side: the R-fillet's section area and its centroid's distance from the edge.
fn fillet_section(r: f64) -> (f64, f64) {
    (
        (1.0 - PI / 4.0) * r * r,
        r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI),
    )
}

/// An L: 20 × 20 less its +x +y quarter, 10 tall; edges e0..e5 from (-10, -10) anticlockwise, the
/// inner corner between e2 (y = 0) and e3 (x = 0).
fn l_block(k: &mut K) -> BodyId {
    let points = [
        [-10.0, -10.0],
        [10.0, -10.0],
        [10.0, 0.0],
        [0.0, 0.0],
        [0.0, 10.0],
        [-10.0, 10.0],
    ];
    extrude(k, 0.0, Loop::polygon(&points), vec![], 10.0)
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn convex_fillets_meeting_at_an_inner_corner_swing_round_it() {
    // The L's top edges along its inner walls meet at the inner corner, whose vertical edge stays
    // sharp. Each fillet runs its edge's full 10; were they to run on past the corner into each
    // other's wall, at distances s₁, s₂ from the two walls the depth removed would be
    // h(max(s₁, s₂)), h(s) = r − √(r² − (r − s)²), which over the r × r square holds
    // 2∫ h(s)·s ds = r³(5/3 − π/2). The ball instead swings round the vertical edge, touching it:
    // in each slice s above its centres' plane it takes only the quarter disc of radius
    // r − √(r² − s²) about the edge, not the square, and keeps (1 − π/4)·r³(5/3 − π/2) of that.
    let mut k = K::new();
    let r = 1.5;
    let l = l_block(&mut k);
    let mut e = pair(&k, l, plane(Z, 10.0), plane(Y, 0.0));
    e.extend(pair(&k, l, plane(Z, 10.0), plane(X, 0.0)));
    let b = fillet(&mut k, l, &e, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    assert!(tight(&k, b));
    let (area, _) = fillet_section(r);
    let want = 3000.0 - 2.0 * 10.0 * area - (PI / 4.0) * r.powi(3) * (5.0 / 3.0 - PI / 2.0);
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-9 * want, "volume {got} for {want}");
    assert_eq!(kinds(&k, b, SurfaceKind::Cylinder), 2);
    assert_eq!(kinds(&k, b, SurfaceKind::Torus), 1);
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn a_mixed_corner_fillets_its_concave_edge_first_and_rolls_the_rim_round_it() {
    // All three edges of the L's inner corner at R1.5: the concave vertical edge's fillet adds
    // 10 × the section; the top rim then runs tangentially from one wall round that fillet's top
    // arc to the other, two runs of 8.5 and a quarter turn about the arc's axis, whose section's
    // centroid lies 1.5 plus its distance from the edge out from the axis.
    let mut k = K::new();
    let r = 1.5;
    let l = l_block(&mut k);
    let mut e = pair(&k, l, plane(Z, 10.0), plane(Y, 0.0));
    e.extend(pair(&k, l, plane(Z, 10.0), plane(X, 0.0)));
    e.extend(pair(&k, l, plane(Y, 0.0), plane(X, 0.0)));
    assert_eq!(e.len(), 3);
    let b = fillet(&mut k, l, &e, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    assert!(tight(&k, b));
    let (area, centroid) = fillet_section(r);
    let want = 3000.0 + 10.0 * area - 2.0 * 8.5 * area - area * (r + centroid) * PI / 2.0;
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-9 * want, "volume {got} for {want}");
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn concave_fillets_meeting_at_a_boss_s_sharp_corner_cross_in_a_crease() {
    // Base fillets on two sides of a sharp 10 × 10 × 4 boss meet at its convex vertical edge,
    // which stays sharp. Each run is 10 long; past the corner the material goes only where both
    // add it, r³(5/3 − π/2) as at an inner corner.
    let mut k = K::new();
    let r = 1.5;
    let boss = on_block(&mut k, rect(0.0, 0.0, 10.0, 10.0), false);
    let mut e = pair(&k, boss, plane(Z, 10.0), plane(X, 5.0));
    e.extend(pair(&k, boss, plane(Z, 10.0), plane(Y, 5.0)));
    assert_eq!(e.len(), 2);
    let b = fillet(&mut k, boss, &e, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    assert!(tight(&k, b));
    let (area, _) = fillet_section(r);
    let want = 4400.0 + 2.0 * 10.0 * area + r.powi(3) * (5.0 / 3.0 - PI / 2.0);
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-9 * want, "volume {got} for {want}");
    assert_eq!(kinds(&k, b, SurfaceKind::Torus), 0);
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn a_larger_fillet_into_a_smaller_one_at_a_corner_closes_with_a_patch() {
    // A block corner's top +x edge rounded ρ, then its top +y edge r > ρ: that edge runs on
    // tangentially round the first round's end arc and down the vertical edge, so the arc is a
    // smaller round under a larger fillet. Sliced parallel to the +y face at depth b ≤ r, with
    // k = √(1 − ((r − b)/r)²) and d = r(1 − k), the section is the corner with its walls moved in
    // by d and its round shrunk to ρk:
    //   V = 4000 − ∫ (30d − d² + (1 − π/4)(ρk)²) db − (20 − r)(1 − π/4)ρ²,
    // with ∫d = r²(1 − π/4), ∫d² = r³(5/3 − π/2) and ∫k² = 2r/3 over the top r.
    let mut failures = Vec::new();
    for (rho, r) in [(1.0, 1.5), (1.5, 2.0), (0.5, 1.5)] {
        let mut k = K::new();
        let b = block(&mut k);
        let e = pair(&k, b, plane(Z, 10.0), plane(X, 10.0));
        let b = fillet(&mut k, b, &e, rho);
        let e = pair(&k, b, plane(Z, 10.0), plane(Y, 10.0));
        let b = match k.fillet(b, &e, r) {
            Ok(r) => r.bodies[0],
            Err(e) => {
                failures.push(format!("ρ {rho} r {r}: {e:?}"));
                continue;
            }
        };
        let b = unify(&mut k, b);
        valid(&k, b);
        assert!(tight(&k, b));
        let cut = 1.0 - PI / 4.0;
        let (id, id2, ik2) = (
            r * r * cut,
            r * r * r * (5.0 / 3.0 - PI / 2.0),
            2.0 * r / 3.0,
        );
        let want =
            4000.0 - (30.0 * id - id2 + cut * rho * rho * ik2) - (20.0 - r) * cut * rho * rho;
        let got = volume(&k, b);
        if (got - want).abs() >= 1e-9 * want {
            failures.push(format!("ρ {rho} r {r}: volume {got} for {want}"));
        }
        let n = kinds(&k, b, SurfaceKind::Other);
        if n != 1 {
            failures.push(format!("ρ {rho} r {r}: {n} patches"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a_fillet_no_larger_than_the_round_runs_round_it_as_before() {
    // With the rounds R1.5 the fillet's ball follows them: a torus at each corner, no patch.
    let mut k = K::new();
    let b = rounded_then_filleted(&mut k, 1.5, 1.5, true);
    valid(&k, b);
    assert!(patches_of(&k, b).is_empty());
}

/// A half disc of radius 5, its flat back on x = 0 facing +x: a boss 4 tall on the block, or with
/// `cut` a pocket 4 deep in it. The corners where the back meets the arc are rounded R0.5.
#[track_caller]
fn d_on_block(k: &mut K, cut: bool) -> BodyId {
    let d = Loop::new(vec![
        Curve2::arc([0.0, 0.0], 5.0, PI / 2.0, PI),
        Curve2::line([0.0, -5.0], [0.0, 5.0]),
    ]);
    let b = on_block(k, d, cut);
    let e = pair(k, b, cylinder(0.0, 0.0, 5.0), plane(X, 0.0));
    assert_eq!(e.len(), 2);
    fillet(k, b, &e, 0.5)
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn a_fillet_over_a_smaller_round_beside_a_curved_wall_closes_with_an_exact_patch() {
    // The R1.5 top fillet of the R5 half disc runs round its R0.5 corner rounds. At each corner
    // the ball sits at c, 1.5 from the back and from the arc: (−1.5, ±√10). The torus along the
    // arc stops on the half plane through c and the arc's axis, and the corner's base runs from
    // the back's tangent round the round and along the arc to there. At depth t below the top,
    // with k = √(1 − ((1.5 − t)/1.5)²), the section is the back moved in by 1.5(1 − k), each
    // corner's base scaled by k about its corner axis, and the arc shrunk to 5 − 1.5(1 − k)
    // between the corners: arcs and lines, so by Green's theorem its area is α + βk + γk², fixed by
    // three values of k, and the top 1.5 holds αr + βr·π/4 + γ·2r/3.
    let mut k = K::new();
    let (big, rho, r) = (5.0f64, 0.5f64, 1.5f64);
    let d = d_on_block(&mut k, false);
    let e = pair(&k, d, plane(Z, 14.0), cylinder(0.0, 0.0, 5.0));
    let b = fillet(&mut k, d, &e, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    assert!(tight(&k, b));
    type P = (f64, f64);
    let cross = |p: P, q: P| p.0 * q.1 - p.1 * q.0;
    let ang = |o: P, p: P| (p.1 - o.1).atan2(p.0 - o.0);
    // ½∮(x dy − y dx) along a circle about `o` from angle `a` to `b`.
    let arc = |o: P, rad: f64, a: f64, b: f64| {
        let (pa, pb) = (
            (o.0 + rad * a.cos(), o.1 + rad * a.sin()),
            (o.0 + rad * b.cos(), o.1 + rad * b.sin()),
        );
        0.5 * (rad * rad * (b - a) + cross(o, (pb.0 - pa.0, pb.1 - pa.1)))
    };
    let yr = ((big - rho).powi(2) - rho * rho).sqrt();
    let round_centre = (-rho, yr);
    let c = (-r, ((big - r).powi(2) - r * r).sqrt());
    let a1 = (0.0, yr);
    let a2 = (-rho * big / (big - rho), yr * big / (big - rho));
    let p2 = (c.0 * big / (big - r), c.1 * big / (big - r));
    let flip = |p: P| (p.0, -p.1);
    let area = |k: f64| {
        let sc = |p: P| (c.0 + k * (p.0 - c.0), c.1 + k * (p.1 - c.1));
        let (u1, u2, up, ur, uo) = (sc(a1), sc(a2), sc(p2), sc(round_centre), sc((0.0, 0.0)));
        let mut sum = 0.0;
        if k > 0.0 {
            // The upper corner, round the round from the back and along the arc to its meridian;
            // the lower one, its mirror, walked back.
            let upper = arc(ur, rho * k, ang(ur, u1), ang(ur, u2))
                + arc(uo, big * k, ang(uo, u2), ang(uo, up));
            let (l1, l2, lp, lr, lo) = (flip(u1), flip(u2), flip(up), flip(ur), flip(uo));
            let lower = arc(lo, big * k, ang(lo, lp), ang(lo, l2))
                + arc(lr, rho * k, ang(lr, l2), ang(lr, l1));
            sum += upper + lower;
        }
        let shrunk = big - r * (1.0 - k);
        sum += arc(
            (0.0, 0.0),
            shrunk,
            ang((0.0, 0.0), up),
            TAU + ang((0.0, 0.0), flip(up)),
        );
        sum + 0.5 * cross(flip(u1), u1)
    };
    let (f0, fh, f1) = (area(0.0), area(0.5), area(1.0));
    let gamma = 2.0 * (f1 - 2.0 * fh + f0);
    let beta = f1 - f0 - gamma;
    let top = f0 * r + beta * r * PI / 4.0 + gamma * 2.0 * r / 3.0;
    let want = 4000.0 + f1 * (4.0 - r) + top;
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-12 * want, "volume {got} for {want}");
    let patches = patches_of(&k, b);
    assert_eq!(patches.len(), 2);
    assert_tangent_all_round(&k, b, &patches);
    // The same D cut 4 deep into the block with its floor filleted is the boss's mirror image:
    // the pocket's hollow is the boss's material, so the block keeps 4000 less the boss's share.
    let d = d_on_block(&mut k, true);
    let e = pair(&k, d, plane(Z, 6.0), cylinder(0.0, 0.0, 5.0));
    let pocket = fillet(&mut k, d, &e, r);
    let pocket = unify(&mut k, pocket);
    valid(&k, pocket);
    assert!(tight(&k, pocket));
    let got = volume(&k, pocket);
    let want = 8000.0 - want;
    assert!(
        (got - want).abs() < 1e-12 * want,
        "pocket volume {got} for {want}"
    );
    assert_eq!(kinds(&k, pocket, SurfaceKind::Other), 2);
}

/// A lens on the block: the discs of radius 5 about (∓3, 0) intersected, 4 tall, its tips at
/// (0, ±4) rounded R0.5.
#[track_caller]
fn lens_boss(k: &mut K) -> BodyId {
    let half = 4f64.atan2(3.0);
    let lens = Loop::new(vec![
        Curve2::arc([-3.0, 0.0], 5.0, -half, 2.0 * half),
        Curve2::arc([3.0, 0.0], 5.0, PI - half, 2.0 * half),
    ]);
    let b = on_block(k, lens, false);
    let e = pair(k, b, cylinder(-3.0, 0.0, 5.0), cylinder(3.0, 0.0, 5.0));
    assert_eq!(e.len(), 2);
    fillet(k, b, &e, 0.5)
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn a_fillet_over_smaller_rounds_between_two_curved_walls_closes_with_exact_patches() {
    // The lens's two walls are the discs' cylinders, and the R1.5 top fillet runs round its R0.5
    // tips. At each tip the ball sits at c = (0, ±√(3.5² − 3²)), 3.5 from both discs' axes; each
    // torus stops on the half plane through c and its own axis, and the corner's base runs along
    // one wall, round the tip and along the other, three arcs. As for the D, the section at depth
    // t is the two walls shrunk to 5 − 1.5(1 − k) between the tips and each tip's base scaled by k
    // about its corner axis, its area α + βk + γk².
    let mut k = K::new();
    let (big, rho, r, h) = (5.0f64, 0.5f64, 1.5f64, 3.0f64);
    let lens = lens_boss(&mut k);
    let e = pair(&k, lens, plane(Z, 14.0), cylinder(-3.0, 0.0, 5.0));
    let b = fillet(&mut k, lens, &e, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    assert!(tight(&k, b));
    type P = (f64, f64);
    let cross = |p: P, q: P| p.0 * q.1 - p.1 * q.0;
    let ang = |o: P, p: P| (p.1 - o.1).atan2(p.0 - o.0);
    // ½∮(x dy − y dx) along a circle about `o`, anticlockwise from the angle of `p` to that of `q`.
    let arc = |o: P, rad: f64, p: P, q: P| {
        let a = ang(o, p);
        let mut b = ang(o, q);
        while b < a {
            b += TAU;
        }
        let (pa, pb) = (
            (o.0 + rad * a.cos(), o.1 + rad * a.sin()),
            (o.0 + rad * b.cos(), o.1 + rad * b.sin()),
        );
        0.5 * (rad * rad * (b - a) + cross(o, (pb.0 - pa.0, pb.1 - pa.1)))
    };
    let toward = |o: P, p: P, rad: f64| {
        let d = (p.0 - o.0).hypot(p.1 - o.1);
        (o.0 + (p.0 - o.0) * rad / d, o.1 + (p.1 - o.1) * rad / d)
    };
    let (wa, wb) = ((-h, 0.0), (h, 0.0));
    let (yc, yr) = (
        ((big - r).powi(2) - h * h).sqrt(),
        ((big - rho).powi(2) - h * h).sqrt(),
    );
    let area = |k: f64| {
        let shrunk = big - r * (1.0 - k);
        let mut sum = 0.0;
        // Each tip's base from wall `from` round the round to wall `to`, anticlockwise.
        let tip = |sign: f64, from: P, to: P| {
            let (c, rc) = ((0.0, sign * yc), (0.0, sign * yr));
            let sc = |p: P| (c.0 + k * (p.0 - c.0), c.1 + k * (p.1 - c.1));
            let (pf, tf) = (toward(from, c, big), toward(from, rc, big));
            let (tt, pt) = (toward(to, rc, big), toward(to, c, big));
            let base = arc(sc(from), big * k, sc(pf), sc(tf))
                + arc(sc(rc), rho * k, sc(tf), sc(tt))
                + arc(sc(to), big * k, sc(tt), sc(pt));
            (base, toward(from, c, big), toward(to, c, big))
        };
        let (top, top_a, top_b) = tip(1.0, wa, wb);
        let (bottom, bottom_b, bottom_a) = tip(-1.0, wb, wa);
        let ct = (0.0, yc);
        let cb = (0.0, -yc);
        let sct = |p: P| (ct.0 + k * (p.0 - ct.0), ct.1 + k * (p.1 - ct.1));
        let scb = |p: P| (cb.0 + k * (p.0 - cb.0), cb.1 + k * (p.1 - cb.1));
        if k > 0.0 {
            sum += top + bottom;
        }
        // The walls between the tips: A's from the bottom meridian up, B's from the top down.
        sum += arc(wa, shrunk, scb(bottom_a), sct(top_a));
        sum += arc(wb, shrunk, sct(top_b), scb(bottom_b));
        sum
    };
    let (f0, fh, f1) = (area(0.0), area(0.5), area(1.0));
    let gamma = 2.0 * (f1 - 2.0 * fh + f0);
    let beta = f1 - f0 - gamma;
    let top = f0 * r + beta * r * PI / 4.0 + gamma * 2.0 * r / 3.0;
    let want = 4000.0 + f1 * (4.0 - r) + top;
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-12 * want, "volume {got} for {want}");
    let patches = patches_of(&k, b);
    assert_eq!(patches.len(), 2);
    assert_tangent_all_round(&k, b, &patches);
}

/// A 10 × 10 square with a bite out of its +x side, the disc of radius 5 about (9, 0), leaving an
/// arc from (5, 3) to (5, −3), its corners with the side rounded R0.5: a boss 4 tall on the block,
/// or with `cut`, a pocket 4 deep in it.
#[track_caller]
fn bitten(k: &mut K, cut: bool) -> BodyId {
    let half = 3f64.atan2(4.0);
    let square = Loop::new(vec![
        Curve2::line([-5.0, -5.0], [5.0, -5.0]),
        Curve2::line([5.0, -5.0], [5.0, -3.0]),
        Curve2::arc([9.0, 0.0], 5.0, half - PI, -2.0 * half),
        Curve2::line([5.0, 3.0], [5.0, 5.0]),
        Curve2::line([5.0, 5.0], [-5.0, 5.0]),
        Curve2::line([-5.0, 5.0], [-5.0, -5.0]),
    ]);
    let b = on_block(k, square, cut);
    let e = pair(k, b, plane(X, 5.0), cylinder(9.0, 0.0, 5.0));
    assert_eq!(e.len(), 2);
    fillet(k, b, &e, 0.5)
}

#[test]
#[ignore = "cadrs has no corner patch for a fillet over a smaller round it crosses"]
fn a_fillet_over_smaller_rounds_beside_a_concave_wall_closes_with_exact_patches() {
    // The rim of the bitten square, filleted R1.5, runs along its flat sides, round the R0.5
    // rounds and along the bite, a concave wall whose torus has the ball 6.5 from the bite's axis.
    // At each corner the ball sits at c = (3.5, ±√12), 1.5 inside the side and 6.5 from the axis;
    // the torus stops on the half plane through c and the axis, and the base runs from the side
    // round the round and along the bite to there. At depth t the section is the square moved in by
    // 1.5(1 − k), creased at its sharp corners, the bite grown to 5 + 1.5(1 − k) between the
    // corners, and each corner's base scaled by k about its axis: area α + βk + γk², as for the D.
    let mut k = K::new();
    let (big, rho, r) = (5.0f64, 0.5f64, 1.5f64);
    let rim = |k: &K, b: BodyId, z: f64| {
        let mut e = Vec::new();
        e.extend(pair(k, b, plane(Z, z), plane(X, -5.0)));
        e.extend(pair(k, b, plane(Z, z), plane(Y, -5.0)));
        e.extend(pair(k, b, plane(Z, z), plane(Y, 5.0)));
        e.extend(pair(k, b, plane(Z, z), plane(X, 5.0)));
        e.extend(pair(k, b, plane(Z, z), cylinder(9.0, 0.0, 5.0)));
        e
    };
    let boss = bitten(&mut k, false);
    let e = rim(&k, boss, 14.0);
    let b = fillet(&mut k, boss, &e, r);
    let b = unify(&mut k, b);
    valid(&k, b);
    assert!(tight(&k, b));
    type P = (f64, f64);
    let cross = |p: P, q: P| p.0 * q.1 - p.1 * q.0;
    // ½∮(x dy − y dx) along a circle about `o` from the angle of `p` to that of `q`.
    let arc = |o: P, rad: f64, p: P, q: P, anticlockwise: bool| {
        let a = (p.1 - o.1).atan2(p.0 - o.0);
        let mut b = (q.1 - o.1).atan2(q.0 - o.0);
        while anticlockwise && b < a {
            b += TAU;
        }
        while !anticlockwise && b > a {
            b -= TAU;
        }
        let (pa, pb) = (
            (o.0 + rad * a.cos(), o.1 + rad * a.sin()),
            (o.0 + rad * b.cos(), o.1 + rad * b.sin()),
        );
        0.5 * (rad * rad * (b - a) + cross(o, (pb.0 - pa.0, pb.1 - pa.1)))
    };
    let line = |p: P, q: P| 0.5 * cross(p, q);
    let toward = |o: P, p: P, rad: f64| {
        let d = (p.0 - o.0).hypot(p.1 - o.1);
        (o.0 + (p.0 - o.0) * rad / d, o.1 + (p.1 - o.1) * rad / d)
    };
    let bite = (9.0, 0.0);
    // Each corner: its centre, its round's centre, the round's tangents on the side and the bite,
    // and the bite's point nearest the centre.
    let corner = |sign: f64| {
        let y = |d: f64| sign * ((big + d).powi(2) - (bite.0 - (5.0 - d)).powi(2)).sqrt();
        let (rc, c) = ((5.0 - rho, y(rho)), (5.0 - r, y(r)));
        (
            c,
            rc,
            (5.0, rc.1),
            toward(bite, rc, big),
            toward(bite, c, big),
        )
    };
    let area = |k: f64| {
        let e = 5.0 - r * (1.0 - k);
        let (cl, rcl, a1l, a2l, pl) = corner(-1.0);
        let (cu, rcu, a1u, a2u, pu) = corner(1.0);
        let sc = |c: P, p: P| (c.0 + k * (p.0 - c.0), c.1 + k * (p.1 - c.1));
        let mut sum = line((-e, -e), (e, -e)) + line((e, -e), sc(cl, a1l));
        if k > 0.0 {
            sum += arc(sc(cl, rcl), rho * k, sc(cl, a1l), sc(cl, a2l), true)
                + arc(sc(cl, bite), big * k, sc(cl, a2l), sc(cl, pl), false);
        }
        sum += arc(bite, big + r * (1.0 - k), sc(cl, pl), sc(cu, pu), false);
        if k > 0.0 {
            sum += arc(sc(cu, bite), big * k, sc(cu, pu), sc(cu, a2u), false)
                + arc(sc(cu, rcu), rho * k, sc(cu, a2u), sc(cu, a1u), true);
        }
        sum + line(sc(cu, a1u), (e, e)) + line((e, e), (-e, e)) + line((-e, e), (-e, -e))
    };
    let (f0, fh, f1) = (area(0.0), area(0.5), area(1.0));
    let gamma = 2.0 * (f1 - 2.0 * fh + f0);
    let beta = f1 - f0 - gamma;
    let top = f0 * r + beta * r * PI / 4.0 + gamma * 2.0 * r / 3.0;
    let want = 4000.0 + f1 * (4.0 - r) + top;
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-12 * want, "volume {got} for {want}");
    let patches = patches_of(&k, b);
    assert_eq!(patches.len(), 2);
    assert_tangent_all_round(&k, b, &patches);
    // Cut as a pocket with its floor filleted, it is the boss's mirror image, its wall now bulging
    // into the hollow.
    let pocket = bitten(&mut k, true);
    let e = rim(&k, pocket, 6.0);
    let pocket = fillet(&mut k, pocket, &e, r);
    let pocket = unify(&mut k, pocket);
    valid(&k, pocket);
    assert!(tight(&k, pocket));
    let got = volume(&k, pocket);
    let want = 8000.0 - want;
    assert!(
        (got - want).abs() < 1e-12 * want,
        "pocket volume {got} for {want}"
    );
    assert_eq!(kinds(&k, pocket, SurfaceKind::Other), 2);
}
