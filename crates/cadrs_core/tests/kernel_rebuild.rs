//! P3.1: Part Studio rebuilds through the kernel (`cadrs_core::rebuild`, OCCT backend).
//!
//! The acceptance numbers of `reference/onshape/training/intro-to-part-studios-gaps.md` (P3.1):
//! a 100 × 60 × 25 box, the Control Arm with both extrudes as New, and the time to rebuild a
//! 20-feature studio.
#![cfg(feature = "occt")]

use std::f64::consts::PI;
use std::time::{Duration, Instant};

use cadrs_core::document::{ExtrudeFeature, Feature, FeatureKind, RegionRef, SketchFeature};
use cadrs_core::rebuild::Rebuilder;
use cadrs_core::FeatureId;
use cadrs_sketch::region::{region_at, regions};
use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

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

fn circle(s: &mut Sketch, c: Vec2, r: f64) {
    SketchOp::AddCircle {
        center: c,
        radius: r,
        construction: false,
    }
    .apply(s)
    .unwrap();
}

fn sketch(name: &str, plane: PlaneRef, geometry: Sketch) -> Feature {
    Feature {
        id: FeatureId::new(),
        name: name.into(),
        kind: FeatureKind::Sketch(SketchFeature {
            plane: Some(plane),
            disable_imprinting: false,
            geometry,
        }),
        suppress_by: None,
    }
}

/// An extrude of the regions of `sketch` under the seed points.
fn extrude(name: &str, sketch: &Feature, seeds: &[Vec2], depth: f64) -> Feature {
    let g = &sketch.sketch().unwrap().geometry;
    let rs = regions(g);
    let refs = seeds
        .iter()
        .map(|p| {
            let i = region_at(&rs, *p).unwrap_or_else(|| panic!("no region at {p:?}"));
            RegionRef::new(sketch.id, &rs[i])
        })
        .collect();
    Feature {
        id: FeatureId::new(),
        name: name.into(),
        kind: FeatureKind::Extrude(ExtrudeFeature {
            regions: refs,
            depth,
            depth_expr: format!("{depth} mm"),
            ..ExtrudeFeature::default()
        }),
        suppress_by: None,
    }
}

#[track_caller]
fn close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "got {actual}, expected {expected} ± {tol}"
    );
}

/// V = 100·60·25 = 150 000 mm³; A = 2(100·60 + 100·25 + 60·25) = 20 000 mm².
#[test]
fn rectangle_100x60x25() {
    let mut g = Sketch::new();
    polyline(&mut g, &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 60.0), v(0.0, 60.0)], true);
    let s = sketch("Sketch 1", PlaneRef::Top, g);
    let e = extrude("Extrude 1", &s, &[v(50.0, 30.0)], 25.0);
    let build = Rebuilder::new().rebuild(&[s, e]);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let m = build.parts[0].mass.unwrap();
    close(m.volume, 150_000.0, 1e-6);
    close(m.surface_area, 20_000.0, 1e-6);
    // The display mesh encloses the same volume (a box tessellates exactly).
    close(build.parts[0].solid.volume(), 150_000.0, 1e-6);
}

/// The Control Arm sketch of PS6 (`intro-to-part-studios.md`, `ex1-step*.png`): hub Ø70 with a
/// Ø35 bore and a 45 × 10 keyway, eyes Ø35 with Ø20 holes 107.5 mm either side, joined by webs
/// tangent to the hub and the eyes.
fn control_arm_sketch() -> Sketch {
    const HUB_R: f64 = 35.0;
    const BORE_R: f64 = 17.5;
    const EYE_R: f64 = 17.5;
    const EYE_HOLE_R: f64 = 10.0;
    const X: f64 = 107.5;
    let mut g = Sketch::new();
    circle(&mut g, v(0.0, 0.0), HUB_R);
    circle(&mut g, v(0.0, 0.0), BORE_R);
    polyline(&mut g, &[v(-22.5, -5.0), v(22.5, -5.0), v(22.5, 5.0), v(-22.5, 5.0)], true);
    for side in [1.0, -1.0] {
        let cx = side * X;
        circle(&mut g, v(cx, 0.0), EYE_R);
        circle(&mut g, v(cx, 0.0), EYE_HOLE_R);
        // External tangents: the tangent points lie along (±cos t, ±sin t) from each center.
        let t = ((HUB_R - EYE_R) / X).acos();
        for s in [1.0, -1.0] {
            let a = v(side * HUB_R * t.cos(), s * HUB_R * t.sin());
            let b = v(cx + side * EYE_R * t.cos(), s * EYE_R * t.sin());
            polyline(&mut g, &[a, b], false);
        }
    }
    g
}

/// P3.1 acceptance: the Control Arm with both extrudes as New (Extrude 1: hub ring, right web
/// and right eye ring, 40 mm; Extrude 2: left web and left eye ring, 25 mm) gives two parts
/// whose volumes add up to the course's 368 749.705 mm³ (the parts only touch, so the sum is
/// the volume of the course's single part).
#[test]
fn control_arm_both_new() {
    let s = sketch("Sketch 1", PlaneRef::Top, control_arm_sketch());
    let e1 = extrude(
        "Extrude 1",
        &s,
        &[v(0.0, 26.0), v(60.0, 0.0), v(107.5 + 14.0, 0.0)],
        40.0,
    );
    let e2 = extrude("Extrude 2", &s, &[v(-60.0, 0.0), v(-107.5 - 14.0, 0.0)], 25.0);
    let build = Rebuilder::new().rebuild(&[s, e1, e2]);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 2);
    let total: f64 = build.parts.iter().map(|p| p.mass.unwrap().volume).sum();
    eprintln!("Control Arm, both New: {total:.4} mm³");
    close(total, 368_749.705, 1e-3);
    // Curved faces are smooth: the hub's outer cylinder is one face with many triangles.
    let p1 = &build.parts[0].solid;
    assert!(p1.faces.iter().filter(|f| f.plane.is_none()).count() >= 4);
}

/// A 20-feature studio (10 sketches, each with a rounded slot and a hole, and 10 extrudes)
/// rebuilds from scratch in under 200 ms, debug or release (measured: about 40 ms in debug,
/// 110 ms of CPU in release on the 2026-10-01 nightly; our crates build at opt-level 1 in debug,
/// OCCT is always optimised).
#[test]
fn twenty_feature_rebuild_time() {
    let mut features = Vec::new();
    for i in 0..10 {
        let mut g = Sketch::new();
        let x = i as f64 * 50.0;
        // A slot: two lines and two half circles, with a hole.
        polyline(&mut g, &[v(x, 0.0), v(x + 30.0, 0.0)], false);
        polyline(&mut g, &[v(x + 30.0, 20.0), v(x, 20.0)], false);
        SketchOp::AddArc {
            center: v(x + 30.0, 10.0),
            start: v(x + 30.0, 0.0),
            end: v(x + 30.0, 20.0),
            construction: false,
        }
        .apply(&mut g)
        .unwrap();
        SketchOp::AddArc {
            center: v(x, 10.0),
            start: v(x, 20.0),
            end: v(x, 0.0),
            construction: false,
        }
        .apply(&mut g)
        .unwrap();
        circle(&mut g, v(x + 15.0, 10.0), 4.0);
        let s = sketch(&format!("Sketch {}", i + 1), PlaneRef::Top, g);
        let e = extrude(&format!("Extrude {}", i + 1), &s, &[v(x + 3.0, 10.0)], 10.0 + i as f64);
        features.push(s);
        features.push(e);
    }
    let mut rb = Rebuilder::new();
    // The first rebuild in a process also loads OCCT's lazily initialised tables.
    let _ = Rebuilder::new().rebuild(&features[..2]);
    let start = Instant::now();
    let cpu_start = thread_cpu_time();
    let build = rb.rebuild(&features);
    let cold = start.elapsed();
    let cold_cpu = thread_cpu_time().zip(cpu_start).map(|(end, start)| end - start);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 10);
    assert_eq!(build.computed, 10);
    // Each part: the slot's area times its depth (hole subtracted).
    for (i, p) in build.parts.iter().enumerate() {
        let area = 30.0 * 20.0 + PI * 100.0 - PI * 16.0;
        close(p.mass.unwrap().volume, area * (10.0 + i as f64), 1e-6);
    }

    // Editing the last extrude recomputes only it.
    let last = features.len() - 1;
    if let FeatureKind::Extrude(e) = &mut features[last].kind {
        e.depth = 42.0;
    }
    let start = Instant::now();
    let edited = rb.rebuild(&features);
    let incremental = start.elapsed();
    assert_eq!(edited.computed, 1);
    // Nothing changed: nothing is computed.
    assert_eq!(rb.rebuild(&features).computed, 0);
    eprintln!(
        "20-feature rebuild: {cold:?} from scratch ({cold_cpu:?} CPU), {incremental:?} after editing the last extrude"
    );

    let budget = Duration::from_millis(200);
    // The budget is checked against the thread's CPU time where the OS reports it, so that other
    // processes competing for the cores (parallel workers, the golden suite) don't fail it. The
    // wall clock still has to stay within a loose bound.
    match cold_cpu {
        Some(cpu) => {
            assert!(cpu < budget, "rebuild took {cpu:?} of CPU time (budget {budget:?})");
            assert!(cold < budget * 10, "rebuild took {cold:?} of wall time (bound {:?})", budget * 10);
        }
        None => assert!(cold < budget, "rebuild took {cold:?} (budget {budget:?})"),
    }
}

/// This thread's CPU time (user + system), from /proc on Linux; `None` elsewhere.
fn thread_cpu_time() -> Option<Duration> {
    let stat = std::fs::read_to_string("/proc/thread-self/stat").ok()?;
    // Fields after the command name, which is in parentheses and may contain spaces.
    let rest = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // utime and stime are fields 14 and 15 of the full line, 12 and 13 after the name (0-based:
    // 11 and 12), in clock ticks of 1/100 s on Linux.
    let ticks: u64 = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    Some(Duration::from_millis(ticks * 10))
}

/// A feature that can't be built reports why, and the others still build.
#[test]
fn failed_features_report_errors() {
    let mut g = Sketch::new();
    polyline(&mut g, &[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)], true);
    let s = sketch("Sketch 1", PlaneRef::Top, g);
    let good = extrude("Extrude 1", &s, &[v(5.0, 5.0)], 5.0);
    let mut gone = extrude("Extrude 2", &s, &[v(5.0, 5.0)], 5.0);
    // Its sketch is not in the list (deleted).
    if let FeatureKind::Extrude(e) = &mut gone.kind {
        e.regions[0].sketch = FeatureId::new();
    }
    let build = Rebuilder::new().rebuild(&[s, good.clone(), gone.clone()]);
    assert_eq!(build.parts.len(), 1);
    assert_eq!(build.parts[0].feature, good.id);
    assert_eq!(
        build.error(gone.id),
        Some("The selected sketch regions no longer exist")
    );
    assert!(build.error(good.id).is_none());
}

/// The worker thread gives the same result as a local rebuild, and caches across requests.
#[test]
fn worker_thread_builds() {
    let mut g = Sketch::new();
    circle(&mut g, v(0.0, 0.0), 10.0);
    let s = sketch("Sketch 1", PlaneRef::Front, g);
    let e = extrude("Extrude 1", &s, &[v(0.0, 0.0)], 7.0);
    let features = vec![s, e];
    let a = cadrs_core::rebuild::build(&features);
    close(a.parts[0].mass.unwrap().volume, PI * 100.0 * 7.0, 1e-6);
    let b = cadrs_core::rebuild::build(&features);
    assert_eq!(b.computed, 0);
    assert!(std::sync::Arc::ptr_eq(&a.parts[0].solid, &b.parts[0].solid));
    let mut pending = cadrs_core::rebuild::request(features);
    assert!(pending.wait(None).is_some());
}

/// A Remove's end cap becomes a face of the part it cut, looking the other way (a pocket's
/// ceiling here): its plane frame has that face's outward normal, so a sketch on it faces out
/// of the material (Onshape import: sketches on pocket floors).
#[test]
fn pocket_faces_look_out_of_the_part() {
    let mut g = Sketch::new();
    polyline(&mut g, &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 60.0), v(0.0, 60.0)], true);
    let s1 = sketch("Sketch 1", PlaneRef::Top, g);
    let e1 = extrude("Extrude 1", &s1, &[v(50.0, 30.0)], 25.0);
    let mut g = Sketch::new();
    polyline(&mut g, &[v(40.0, 20.0), v(60.0, 20.0), v(60.0, 40.0), v(40.0, 40.0)], true);
    let s2 = sketch("Sketch 2", PlaneRef::Top, g);
    let mut e2 = extrude("Extrude 2", &s2, &[v(50.0, 30.0)], 10.0);
    if let FeatureKind::Extrude(x) = &mut e2.kind {
        x.op = cadrs_core::document::BooleanOp::Remove;
    }
    let pocket = e2.id;
    let build = Rebuilder::new().rebuild(&[s1, e1, s2, e2]);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    close(build.parts[0].mass.unwrap().volume, 150_000.0 - 4_000.0, 1e-6);
    let solid = &build.parts[0].solid;
    let ceiling = solid
        .faces
        .iter()
        .find(|f| f.name.op == pocket.0 && matches!(f.name.origin, cadrs_kernel::naming::FaceOrigin::Cap { end: true, .. }))
        .expect("the pocket's ceiling");
    let n = ceiling.plane.expect("planar").normal();
    close(n[2], -1.0, 1e-9);
}
