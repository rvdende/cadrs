//! Ported from springrep's tests/shells_and_drafts.rs (MIT,
//! https://github.com/springcraft-ai/springrep): shells and drafts made by moving faces: a shell
//! is the solid less its own faces offset inward (or, grown outward, the offset solid less the
//! original), open where a face is left in place, and a draft tilts faces about where they meet a
//! neutral plane. Flat, cylindrical and spherical walls are each measured against closed forms.
//! springrep's shells move faces to their offsets, so they are cadrs's sharp shells; faces are
//! picked by geometry where springrep picks them by name, and its assertions on the names the
//! shell gives are left out.

#![allow(dead_code)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

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

/// The 20 × 20 × 10 block on the xy plane, centred on the origin.
fn block(k: &mut K) -> BodyId {
    extrude(k, 0.0, rect(0.0, 0.0, 20.0, 20.0), vec![], 10.0)
}

/// A cylinder R`r` × `h` standing on the xy plane.
fn can(k: &mut K, r: f64, h: f64) -> BodyId {
    extrude(k, 0.0, circle(0.0, 0.0, 2.0 * r), vec![], h)
}

fn shell_with(k: &mut K, solid: BodyId, open: Vec<FaceId>, t: f64, outward: bool) -> BodyId {
    assert!(!open.is_empty());
    let spec = ShellSpec {
        remove: open,
        thickness: t,
        outward,
        hollow: false,
        sharp: true,
    };
    let b = k.shell_with(solid, &spec).unwrap().bodies[0];
    unify(k, b)
}

/// `solid` hollowed to a wall `t` thick, open at the faces `open`.
fn shell(k: &mut K, solid: BodyId, open: Vec<FaceId>, t: f64) -> BodyId {
    shell_with(k, solid, open, t, false)
}

/// `solid` with a wall `t` thick grown round it, open at the faces `open`.
fn shell_outward(k: &mut K, solid: BodyId, open: Vec<FaceId>, t: f64) -> BodyId {
    shell_with(k, solid, open, t, true)
}

/// `solid` with `faces` tilted by `angle` about where they meet the xy plane.
fn drafted(k: &mut K, solid: BodyId, faces: Vec<FaceId>, angle: f64) -> BodyId {
    assert!(!faces.is_empty());
    let spec = DraftSpec {
        faces,
        angle,
        pull: Vector3::z_axis(),
        neutral: Point3::origin(),
        neutral_normal: None,
        tangent_propagation: false,
    };
    let b = k.draft(solid, &spec).unwrap().bodies[0];
    unify(k, b)
}

#[test]
fn a_shell_open_at_the_top_leaves_a_cup() {
    // A 20 × 20 × 10 mm block less a 16 × 16 × 8 cavity open at the top: 4000 − 2048.
    let mut k = K::new();
    let bl = block(&mut k);
    let top = faces(&k, bl, plane(Z, 10.0));
    let b = shell(&mut k, bl, top, 2.0);
    let v = volume(&k, b);
    assert!((v - 1952.0).abs() < 1e-9, "volume {v}");
    // springrep: five faces named `hollow.inner` and one `block.end` (naming).
}

#[test]
fn a_shell_grown_outward_keeps_the_inside() {
    // The 20 × 20 × 10 mm block grown 2 mm out on every side but the open top: 24 · 24 · 12 − 4000.
    let mut k = K::new();
    let bl = block(&mut k);
    let top = faces(&k, bl, plane(Z, 10.0));
    let b = shell_outward(&mut k, bl, top, 2.0);
    let v = volume(&k, b);
    assert!(
        (v - (24.0 * 24.0 * 12.0 - 4000.0)).abs() < 1e-9,
        "volume {v}"
    );
    // springrep: the inside keeps the block's names, the rim `block.end` (naming).
    assert_eq!(k.face_count(b).unwrap(), 11);
}

#[test]
fn a_draft_tapers_the_walls_from_the_neutral_plane() {
    // Two opposite walls of the 20 × 20 × 10 mm block tilted so they come 2 mm in at the top: a
    // trapezoid 20 at the foot and 16 at the top, 10 high, 20 deep.
    let mut k = K::new();
    let angle = (2.0f64 / 10.0).atan();
    let bl = block(&mut k);
    let mut walls = faces(&k, bl, plane(X, -10.0));
    walls.extend(faces(&k, bl, plane(X, 10.0)));
    let b = drafted(&mut k, bl, walls, angle);
    let want = 20.0 * 10.0 * 36.0 / 2.0;
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "volume {v} vs {want}");
    // springrep: the drafted wall keeps its name `block.u_min` (naming).
}

#[test]
fn a_cylinder_shelled_open_at_the_top_is_a_cup_with_a_round_wall() {
    // R10 × 20 hollowed 2 mm, the top open: the cavity is the cylinder R8 from z = 2 to 20, its
    // wall the cylinder's offset 2 mm in, exactly.
    let mut k = K::new();
    let c = can(&mut k, 10.0, 20.0);
    let top = faces(&k, c, plane(Z, 20.0));
    let b = shell(&mut k, c, top, 2.0);
    valid(&k, b);
    let want = PI * (100.0 * 20.0 - 64.0 * 18.0);
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "{v}");
    let walls: Vec<f64> = k
        .faces(b)
        .unwrap()
        .iter()
        .filter(|f| f.kind == SurfaceKind::Cylinder)
        .map(|f| f.radius.unwrap())
        .collect();
    assert_eq!(walls.len(), 2, "{walls:?}");
    assert!(walls.iter().any(|r| (r - 8.0).abs() < 1e-12), "{walls:?}");
}

#[test]
fn a_cylinder_shelled_outward_grows_its_wall_round_it() {
    // R10 × 20 shelled 2 mm outward, the top open: the outside is R12 from z = −2 to 20 and the
    // inside the original cylinder.
    let mut k = K::new();
    let c = can(&mut k, 10.0, 20.0);
    let top = faces(&k, c, plane(Z, 20.0));
    let b = shell_outward(&mut k, c, top, 2.0);
    valid(&k, b);
    let want = PI * (144.0 * 22.0 - 100.0 * 20.0);
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "{v}");
}

#[test]
fn a_dome_shelled_open_at_its_base_is_a_spherical_cap() {
    // An R10 hemisphere standing on its flat, hollowed 1 mm with the flat open: the shell between
    // the spheres R10 and R9, 2π(10³ − 9³)/3.
    let mut k = K::new();
    let quarter = Loop::new(vec![
        Curve2::arc([0.0, 0.0], 10.0, 0.0, FRAC_PI_2),
        Curve2::line([0.0, 10.0], [0.0, 0.0]),
        Curve2::line([0.0, 0.0], [10.0, 0.0]),
    ]);
    // springrep's Frame::new(0, −y, x): sketch x along x, sketch y along z (cadrs's Plane::xz).
    let p = Profile::of(Plane::xz(0.0), vec![quarter]);
    let dome = k.revolve(&p, Axis::z(), TAU).unwrap().bodies[0];
    let base = faces(&k, dome, plane(Z, 0.0));
    let b = shell(&mut k, dome, base, 1.0);
    valid(&k, b);
    let want = 2.0 * PI * (1000.0 - 729.0) / 3.0;
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "{v}");
}

#[test]
fn a_rounded_block_shelled_keeps_its_rounds_as_offset_cylinders() {
    // The 20 × 20 × 10 block with its four vertical edges rounded R4, hollowed 1 mm with the top
    // open. Each section is the rounded square, and the cavity's the square 2 smaller with rounds
    // R3: (20² − (4 − π)·4²)·10 − (18² − (4 − π)·3²)·9.
    let mut k = K::new();
    let bl = block(&mut k);
    let mut uprights = Vec::new();
    for (a, c) in [
        (plane(X, -10.0), plane(Y, -10.0)),
        (plane(Y, -10.0), plane(X, 10.0)),
        (plane(X, 10.0), plane(Y, 10.0)),
        (plane(Y, 10.0), plane(X, -10.0)),
    ] {
        uprights.extend(pair(&k, bl, a, c));
    }
    assert_eq!(uprights.len(), 4);
    let rounded = k.fillet(bl, &uprights, 4.0).unwrap().bodies[0];
    let top = faces(&k, rounded, plane(Z, 10.0));
    let b = shell(&mut k, rounded, top, 1.0);
    valid(&k, b);
    let section = |w: f64, r: f64| w * w - (4.0 - PI) * r * r;
    let want = section(20.0, 4.0) * 10.0 - section(18.0, 3.0) * 9.0;
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "{v}");
}

#[test]
fn a_drafted_cylinder_boss_is_a_frustum() {
    // R10 × 10, its side drafted 5° from the base: a frustum from R10 to R10 − 10·tan 5°.
    let mut k = K::new();
    let c = can(&mut k, 10.0, 10.0);
    let side = faces(&k, c, cylinder(0.0, 0.0, 10.0));
    let b = drafted(&mut k, c, side, 5f64.to_radians());
    valid(&k, b);
    let r = 10.0 - 10.0 * 5f64.to_radians().tan();
    let want = PI * 10.0 / 3.0 * (100.0 + 10.0 * r + r * r);
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "{v}");
    assert_eq!(kinds(&k, b, SurfaceKind::Cone), 1);
}

#[test]
fn a_drafted_bore_widens_along_the_pull() {
    // The 20 × 20 × 10 block with a Ø8 through hole, the hole's wall drafted 3° from the base: the
    // hole is a frustum from R4 at z = 0 to R4 + 10·tan 3° at the top.
    let mut k = K::new();
    let drilled = extrude(
        &mut k,
        0.0,
        rect(0.0, 0.0, 20.0, 20.0),
        vec![circle(0.0, 0.0, 8.0)],
        10.0,
    );
    let bore = faces(&k, drilled, cylinder(0.0, 0.0, 4.0));
    let b = drafted(&mut k, drilled, bore, 3f64.to_radians());
    valid(&k, b);
    let r = 4.0 + 10.0 * 3f64.to_radians().tan();
    let want = 4000.0 - PI * 10.0 / 3.0 * (16.0 + 4.0 * r + r * r);
    let v = volume(&k, b);
    assert!((v - want).abs() < 1e-9 * want, "{v}");
}
