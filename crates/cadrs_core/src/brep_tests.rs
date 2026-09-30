//! Tests of [`super`]: kernel solids carry the prism mesh's names (the history-based names
//! agree with the P3.1 geometric tags).

use super::*;
use cadrs_kernel::backend::occt::OcctKernel;
use cadrs_sketch::region::regions;
use cadrs_sketch::{PlaneRef, Sketch, SketchOp};

fn polyline(s: &mut Sketch, points: &[Vec2], closed: bool) {
    SketchOp::AddPolyline {
        points: points.to_vec(),
        closed,
        construction: false,
        label: "Add line",
    }
    .apply(s)
    .unwrap();
}

fn circle(s: &mut Sketch, x: f64, y: f64, r: f64) {
    SketchOp::AddCircle {
        center: Vec2::new(x, y),
        radius: r,
        construction: false,
    }
    .apply(s)
    .unwrap();
}

fn rect(s: &mut Sketch, x: f64, y: f64, w: f64, h: f64) {
    let v = Vec2::new;
    polyline(s, &[v(x, y), v(x + w, y), v(x + w, y + h), v(x, y + h)], true);
}

fn close3(a: Vec3, b: Vec3) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

const OP: OpId = uuid::Uuid::from_u128(7);

/// The kernel's solid has the prism mesh's face names and planar frames, and edges in the same
/// places, so sketches on faces, Use links and imprints keep resolving. `edge_names`: the edge
/// names agree too (not where two regions touch: the prism gave each region's boundary its own
/// lateral edge there, the kernel has one edge between the two regions' sides).
fn same_as_prism(s: &Sketch, pick: impl Fn(&Region) -> bool, plane: PlaneRef, flip: bool) {
    same_as_prism_names(s, pick, plane, flip, true)
}

fn same_as_prism_names(
    s: &Sketch,
    pick: impl Fn(&Region) -> bool,
    plane: PlaneRef,
    flip: bool,
    edge_names: bool,
) {
    let rs: Vec<(u64, Region)> = regions(s)
        .into_iter()
        .filter(|r| pick(r))
        .enumerate()
        .map(|(i, r)| (i as u64, r))
        .collect();
    assert!(!rs.is_empty());
    let frame = plane.frame();
    let prism = crate::solid::extrude(OP, &frame, &rs, 12.0, flip);
    let mut k = OcctKernel::new();
    let group = ProfileGroup::new(frame, rs);
    let part = extrude(&mut k, OP, &[group], 12.0, flip).unwrap();
    let kern = part.solid;
    // Every face is named from the kernel's history, not from geometry.
    assert!(part.names.faces.iter().all(|n| n.is_stable()), "{:?}", part.names.faces);

    let mut want: Vec<FaceName> = prism.faces.iter().map(|f| f.name).collect();
    let mut got: Vec<FaceName> = kern.faces.iter().map(|f| f.name).collect();
    want.sort();
    got.sort();
    assert_eq!(got, want, "face names");
    for f in &prism.faces {
        let g = kern.face(&f.name).unwrap();
        match (f.plane, g.plane) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                assert!(close3(a.u, b.u) && close3(a.v, b.v), "{:?}: {a:?} vs {b:?}", f.name);
                assert!(close3(a.origin, b.origin), "{:?}: {a:?} vs {b:?}", f.name);
            }
            (a, b) => panic!("{:?}: plane {a:?} vs {b:?}", f.name),
        }
        assert!(!g.loops.is_empty(), "{:?} has no outline", f.name);
    }
    if edge_names {
        // The prism names both lateral edges between the same two sides alike; the kernel
        // numbers them.
        let mut want: Vec<EdgeName> = prism.edges.iter().map(|e| e.name.base()).collect();
        let mut got: Vec<EdgeName> = kern.edges.iter().map(|e| e.name.base()).collect();
        want.sort();
        got.sort();
        assert_eq!(got, want, "edge names");
    }
    // The same edges in the same places (compare the ends).
    for e in &prism.edges {
        let (a0, a1) = (e.points[0], *e.points.last().unwrap());
        let found = kern.edges.iter().any(|g| {
            let (b0, b1) = (g.points[0], *g.points.last().unwrap());
            if close3(a0, a1) {
                // A closed curve may start anywhere: compare the names.
                return g.name.base() == e.name.base();
            }
            (close3(a0, b0) && close3(a1, b1)) || (close3(a0, b1) && close3(a1, b0))
        });
        assert!(found, "{:?}: {a0:?}–{a1:?} has no kernel edge", e.name);
    }
    // Curved faces have rulings, and the volume is close to the prism's.
    assert_eq!(prism.rulings.is_empty(), kern.rulings.is_empty());
    let (a, b) = (prism.volume(), kern.volume());
    assert!((a - b).abs() / a < 1e-3, "volume {a} vs {b}");
    assert!(part.mass.volume > 0.0);
    // Triangles wind with their normals (outward).
    for t in 0..kern.triangle_count() {
        let [a, b, c] = [0, 1, 2].map(|i| kern.positions[kern.indices[3 * t + i] as usize]);
        let w = cross(sub(b, a), sub(c, a));
        let n = kern.normals[kern.indices[3 * t] as usize];
        assert!(dot(w, n) >= -1e-9, "triangle {t} winds against its normal");
    }
}

#[test]
fn box_matches_prism_tags() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 50.0, 30.0);
    for plane in [PlaneRef::Top, PlaneRef::Front, PlaneRef::Right] {
        for flip in [false, true] {
            same_as_prism(&s, |_| true, plane, flip);
        }
    }
}

#[test]
fn hole_matches_prism_tags() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 40.0, 40.0);
    circle(&mut s, 20.0, 20.0, 5.0);
    same_as_prism(&s, |r| r.curves.len() == 4, PlaneRef::Top, false);
    same_as_prism(&s, |r| r.curves.len() == 4, PlaneRef::Front, true);
}

#[test]
fn arcs_match_prism_tags() {
    // A D shape: a line and a half circle.
    let mut s = Sketch::new();
    polyline(&mut s, &[Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)], false);
    SketchOp::AddArc {
        center: Vec2::ZERO,
        start: Vec2::new(0.0, -5.0),
        end: Vec2::new(0.0, 5.0),
        construction: false,
    }
    .apply(&mut s)
    .unwrap();
    same_as_prism(&s, |_| true, PlaneRef::Front, false);
    same_as_prism(&s, |_| true, PlaneRef::Top, true);
}

#[test]
fn half_disc_matches_prism_tags() {
    // A circle cut by a longer line (as an imprinted face edge cuts it).
    let mut s = Sketch::new();
    circle(&mut s, 0.0, 0.0, 10.0);
    polyline(&mut s, &[Vec2::new(4.0, -20.0), Vec2::new(4.0, 20.0)], false);
    same_as_prism(&s, |r| r.contains(Vec2::new(8.0, 0.0)), PlaneRef::Top, false);
    same_as_prism(&s, |r| r.contains(Vec2::new(-5.0, 0.0)), PlaneRef::Front, false);
}

/// A rectangle with a circle on the middle of its right side, all three regions extruded: a
/// bar with a round end (the course_ps_kernel_extrude part).
#[test]
fn round_end_bar() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 50.0, 30.0);
    circle(&mut s, 50.0, 15.0, 12.0);
    let rs: Vec<(u64, Region)> = regions(&s).into_iter().enumerate().map(|(i, r)| (i as u64, r)).collect();
    assert_eq!(rs.len(), 3);
    let mut k = OcctKernel::new();
    let frame = PlaneRef::Top.frame();
    let part = extrude(&mut k, OP, &[ProfileGroup::new(frame, rs.clone())], 25.0, false).unwrap();
    let area = 1500.0 + std::f64::consts::PI * 144.0 / 2.0;
    assert!((part.mass.volume - area * 25.0).abs() < 1e-6, "{}", part.mass.volume);
    let v = part.solid.volume();
    assert!((v - area * 25.0).abs() / (area * 25.0) < 2e-3, "mesh volume {v}");
    // The round end has one silhouette seen from the isometric view (at 45°).
    let round = part.solid.faces.iter().find(|f| f.plane.is_none()).unwrap();
    let lines = crate::links::silhouettes(&part.solid, &round.name, [1.0, -1.0, 1.0]);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let q = lines[0].0;
    let at = [50.0 + 12.0 * 0.5f64.sqrt(), 15.0 + 12.0 * 0.5f64.sqrt()];
    assert!((q[0] - at[0]).abs() < 0.1 && (q[1] - at[1]).abs() < 0.1, "{q:?}");
    // (Not compared with the prism mesh: it left out every side face of a curve two selected
    // regions share, here the rectangle's right side and the circle, so its sides had gaps.)
}

/// Two touching rectangles extruded together: one 20 × 10 × 12 box of 6 faces (the kernel
/// merges the two regions' caps, and the sides along the same lines, as Onshape does), where the
/// prism mesh has a cap per region. Every prism face's name still finds the face it is part of,
/// with the frame the prism gives it (a sketch on either region's cap stays where it was).
#[test]
fn touching_regions_merge() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    rect(&mut s, 10.0, 0.0, 10.0, 10.0);
    let rs: Vec<(u64, Region)> = regions(&s).into_iter().enumerate().map(|(i, r)| (i as u64, r)).collect();
    assert_eq!(rs.len(), 2);
    let frame = PlaneRef::Top.frame();
    let prism = crate::solid::extrude(OP, &frame, &rs, 12.0, false);
    let mut k = OcctKernel::new();
    let part = extrude(&mut k, OP, &[ProfileGroup::new(frame, rs)], 12.0, false).unwrap();
    let kern = part.solid;
    assert!((part.mass.volume - 2400.0).abs() < 1e-6, "{}", part.mass.volume);
    assert_eq!(kern.faces.len(), 6, "{:?}", kern.faces.iter().map(|f| f.name).collect::<Vec<_>>());
    assert_eq!(kern.edges.len(), 12);
    // Two caps merged at each end, and the two sides along y = 0 and y = 10 merged.
    assert_eq!(kern.face_aliases.len(), 4, "{:?}", kern.face_aliases);
    for f in &prism.faces {
        let i = kern.faces.iter().position(|g| g.name == kern.canonical_face(&f.name));
        let i = i.unwrap_or_else(|| panic!("{:?} has no kernel face", f.name));
        let (Some(a), Some(b)) = (f.plane, kern.face_plane_as(i, &f.name)) else { panic!("{:?}: not planar", f.name) };
        assert!(close3(a.u, b.u) && close3(a.v, b.v) && close3(a.origin, b.origin), "{:?}: {a:?} vs {b:?}", f.name);
        assert!(kern.face(&f.name).is_some());
    }
    // An edge named by a merged-away cap's name finds the merged edge.
    let cap1 = kern.face_aliases.iter().find(|a| matches!(a.name.origin, FaceOrigin::Cap { end: true, .. })).unwrap();
    let side = kern.edges.iter().find(|e| e.name.touches(&cap1.face)).unwrap().name;
    let other = if side.faces[0] == cap1.face { side.faces[1] } else { side.faces[0] };
    let by_alias = EdgeName::new(cap1.name, other, side.index);
    assert_eq!(kern.edge(&by_alias).map(|e| e.name), Some(side));
}

#[test]
fn ellipse_extrudes() {
    let mut s = Sketch::new();
    SketchOp::AddEllipse {
        center: Vec2::new(3.0, 4.0),
        major: Vec2::new(23.0, 4.0),
        minor: 8.0,
        construction: false,
    }
    .apply(&mut s)
    .unwrap();
    let rs: Vec<(u64, Region)> = regions(&s).into_iter().map(|r| (0, r)).collect();
    let mut k = OcctKernel::new();
    let group = ProfileGroup::new(PlaneRef::Top.frame(), rs);
    let part = extrude(&mut k, OP, &[group], 5.0, false).unwrap();
    let want = std::f64::consts::PI * 20.0 * 8.0 * 5.0;
    assert!((part.mass.volume - want).abs() < 1e-6, "{}", part.mass.volume);
    // Two caps and one curved side, bounded by two ellipse edges.
    assert_eq!(part.solid.faces.len(), 3);
    assert_eq!(part.solid.edges.len(), 2);
    assert!(!part.solid.rulings.is_empty());
}

/// A curved face's silhouettes (from its rulings) are where the prism mesh had them.
#[test]
fn silhouettes_match_prism() {
    let mut s = Sketch::new();
    polyline(&mut s, &[Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)], false);
    SketchOp::AddArc {
        center: Vec2::ZERO,
        start: Vec2::new(0.0, 5.0),
        end: Vec2::new(0.0, -5.0),
        construction: false,
    }
    .apply(&mut s)
    .unwrap();
    let rs: Vec<(u64, Region)> = regions(&s).into_iter().map(|r| (0, r)).collect();
    let frame = PlaneRef::Top.frame();
    let prism = crate::solid::extrude(OP, &frame, &rs, 4.0, false);
    let mut k = OcctKernel::new();
    let group = ProfileGroup::new(frame, rs);
    let kern = extrude(&mut k, OP, &[group], 4.0, false).unwrap().solid;
    let curved = prism.faces.iter().find(|f| f.plane.is_none()).unwrap().name;
    for dir in [[1.0, 1.0, 1.0], [-1.0, 0.3, 0.5], [0.0, 1.0, 0.2], [-1.0, -1.0, 1.0]] {
        let a = crate::links::silhouettes(&prism, &curved, dir);
        let b = crate::links::silhouettes(&kern, &curved, dir);
        assert_eq!(a.len(), b.len(), "{dir:?}: {a:?} vs {b:?}");
        for ((p, _), (q, _)) in a.iter().zip(&b) {
            assert!(dist(*p, *q) < 0.05, "{dir:?}: {a:?} vs {b:?}");
        }
    }
}

/// Where an extrude's time goes (run with `--ignored --nocapture`).
#[test]
#[ignore]
fn profile_stages() {
    use std::time::Instant;
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 30.0, 20.0);
    circle(&mut s, 15.0, 10.0, 4.0);
    let rs: Vec<(u64, Region)> = regions(&s)
        .into_iter()
        .filter(|r| r.curves.len() == 4)
        .map(|r| (0, r))
        .collect();
    let group = ProfileGroup::new(PlaneRef::Top.frame(), rs);
    let mut k = OcctKernel::new();
    for _ in 0..3 {
        let t = Instant::now();
        let body = k
            .extrude(&profile(&group), kernel::Extent::Blind(10.0))
            .unwrap()
            .bodies[0];
        let t1 = t.elapsed();
        let mesh = k.tessellate(body, tessellation()).unwrap();
        let t2 = t.elapsed();
        let _ = k.edges(body).unwrap();
        let t3 = t.elapsed();
        let _ = k.mass_properties(body).unwrap();
        let t4 = t.elapsed();
        let names = naming::name_body(&k, body, OP, &kernel::History::default(), &[]).unwrap();
        let geoms: Geoms = std::iter::once((OP, std::sync::Arc::new(OpGeom::new(
            std::slice::from_ref(&group),
            [0.0, 0.0, 1.0],
            10.0,
        ))))
        .collect();
        let _ = solid_of(&k, body, &names, &geoms, Some(OP)).unwrap();
        let t5 = t.elapsed();
        eprintln!(
            "extrude {t1:?}, tessellate {:?} ({} tris), edges {:?}, mass {:?}, solid_of {:?}",
            t2 - t1,
            mesh.indices.len(),
            t3 - t2,
            t4 - t3,
            t5 - t4
        );
    }
}
