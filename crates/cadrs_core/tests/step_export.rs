//! STEP export of parts (`cadrs_core::export`, `Rebuilder::export_step` on the OCCT backend).
#![cfg(feature = "occt")]

use cadrs_core::document::{ExtrudeFeature, Feature, FeatureKind, RegionRef, SketchFeature};
use cadrs_core::export::StepRequest;
use cadrs_core::rebuild::Rebuilder;
use cadrs_core::{FeatureId, PartId};
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

fn sketch(name: &str, plane: PlaneRef, geometry: Sketch) -> Feature {
    Feature {
        id: FeatureId::new(),
        name: name.into(),
        kind: FeatureKind::Sketch(SketchFeature {
            plane: Some(plane),
            disable_imprinting: false,
            geometry,
        }),
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
    }
}

#[track_caller]
fn close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "got {actual}, expected {expected} ± {tol}"
    );
}

/// The extents of every CARTESIAN_POINT in a STEP file: (min, max) per axis.
fn extents(step: &str) -> [(f64, f64); 3] {
    let mut out = [(f64::MAX, f64::MIN); 3];
    for line in step.lines().filter(|l| l.contains("CARTESIAN_POINT")) {
        let inner = line.rsplit_once('(').map(|(_, r)| r).unwrap_or_default();
        let nums: Vec<f64> = inner
            .trim_end_matches([')', ';'])
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        if nums.len() == 3 {
            for (o, n) in out.iter_mut().zip(nums) {
                o.0 = o.0.min(n);
                o.1 = o.1.max(n);
            }
        }
    }
    out
}

/// Two boxes on Top: 100 × 60 × 25 at the origin and 10 × 10 × 5 beside it.
fn two_boxes() -> (Vec<Feature>, Vec<PartId>) {
    let mut g = Sketch::new();
    polyline(&mut g, &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 60.0), v(0.0, 60.0)], true);
    polyline(&mut g, &[v(200.0, 0.0), v(210.0, 0.0), v(210.0, 10.0), v(200.0, 10.0)], true);
    let s = sketch("Sketch 1", PlaneRef::Top, g);
    let e1 = extrude("Extrude 1", &s, &[v(50.0, 30.0)], 25.0);
    let e2 = extrude("Extrude 2", &s, &[v(205.0, 5.0)], 5.0);
    let features = vec![s, e1, e2];
    let build = Rebuilder::new().rebuild(&features);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let parts = build.parts.iter().map(|p| p.id).collect();
    (features, parts)
}

fn request(parts: &[PartId], y_up: bool, individual: bool) -> StepRequest {
    StepRequest {
        parts: parts.iter().enumerate().map(|(i, p)| (*p, format!("Bracket {}", i + 1))).collect(),
        y_up,
        individual,
    }
}

#[test]
fn one_part_exports_as_named_mm_step() {
    let (features, parts) = two_boxes();
    let files = Rebuilder::new().export_step(&features, &request(&parts[..1], false, true)).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].part.as_deref(), Some("Bracket 1"));
    let text = String::from_utf8(files[0].bytes.clone()).unwrap();
    assert!(text.starts_with("ISO-10303-21;"), "{}", &text[..80.min(text.len())]);
    assert!(text.contains("PRODUCT('Bracket 1','Bracket 1'"), "product not named");
    assert!(!text.contains("PRODUCT('Open CASCADE"));
    assert!(text.contains(".MILLI."), "not in millimetres");
    assert!(text.contains("MANIFOLD_SOLID_BREP"));
    let [x, y, z] = extents(&text);
    close(x.1 - x.0, 100.0, 1e-9);
    close(y.1 - y.0, 60.0, 1e-9);
    close(z.0, 0.0, 1e-9);
    close(z.1, 25.0, 1e-9);
}

/// Y up turns the Part Studio's +Z into +Y: (x, y, z) → (x, z, −y).
#[test]
fn y_up_turns_top_to_y() {
    let (features, parts) = two_boxes();
    let files = Rebuilder::new().export_step(&features, &request(&parts[..1], true, true)).unwrap();
    let [x, y, z] = extents(std::str::from_utf8(&files[0].bytes).unwrap());
    close(x.1 - x.0, 100.0, 1e-9);
    close(y.0, 0.0, 1e-9);
    close(y.1, 25.0, 1e-9);
    close(z.0, -60.0, 1e-9);
    close(z.1, 0.0, 1e-9);
}

#[test]
fn several_parts_one_file_or_one_each() {
    let (features, parts) = two_boxes();
    let mut rb = Rebuilder::new();
    let each = rb.export_step(&features, &request(&parts, false, true)).unwrap();
    assert_eq!(each.len(), 2);
    assert_eq!(each[1].part.as_deref(), Some("Bracket 2"));
    let one = rb.export_step(&features, &request(&parts, false, false)).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].part, None);
    let text = std::str::from_utf8(&one[0].bytes).unwrap();
    assert!(text.contains("PRODUCT('Bracket 1'") && text.contains("PRODUCT('Bracket 2'"));
    assert_eq!(text.matches("MANIFOLD_SOLID_BREP").count(), 2);
}

#[test]
fn missing_part_is_an_error() {
    let (features, parts) = two_boxes();
    let err = Rebuilder::new().export_step(&features[..2], &request(&parts, false, true)).unwrap_err();
    assert_eq!(err, "Bracket 2 no longer exists");
}

#[test]
fn worker_thread_exports() {
    let (features, parts) = two_boxes();
    let files = cadrs_core::rebuild::export_step(features, request(&parts, false, true)).wait().unwrap();
    assert_eq!(files.len(), 2);
}
