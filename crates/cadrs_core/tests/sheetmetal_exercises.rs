//! The sheet metal exercises' stand-ins (P3I.8, X7; `samples::sheetmetal_exercises`): each
//! builds without errors, its fixture is current, and its measured value is checked: E2's mass
//! in Carbon Steel (the exercise's quiz value) against the flat pattern's volume, E3's flat
//! size, E4's flat unchanged by the rework after Finish.
//!
//! Regenerate the fixtures with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test
//! sheetmetal_exercises`.
#![cfg(feature = "occt")]

use cadrs_core::command::History;
use cadrs_core::rebuild::Build;
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::sheetmetal_exercises as ex;
use cadrs_core::{Document, ElementId, FeatureId, Part};

fn build(doc: &Document, el: ElementId) -> std::sync::Arc<Build> {
    let b = cadrs_core::rebuild::build(&doc.element(el).unwrap().active_features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    b
}

/// The folded volume the flat pattern predicts (see `tests/sheetmetal.rs`).
fn predicted(b: &Build, model: FeatureId) -> f64 {
    use cadrs_sheetmetal::flat::PieceSource;
    let ctx = b.sheet_metal.iter().find(|c| c.feature == model).unwrap();
    let p = &ctx.model.params;
    let t = p.thickness;
    let mut v = 0.0;
    for part in &ctx.flat.parts {
        for piece in &part.pieces {
            let area: f64 = piece.cut.iter().map(|c| c.area()).sum();
            v += match piece.source {
                PieceSource::Wall(_) => area * t,
                PieceSource::Bend(j) => {
                    let bend = ctx.model.joint(j).unwrap().bend().unwrap();
                    let k = match bend.value_or_model(p) {
                        cadrs_sheetmetal::BendValue::KFactor(k) => k,
                        _ => p.k_factor,
                    };
                    area * t * (bend.radius + t / 2.0) / (bend.radius + k * t)
                }
            };
        }
    }
    v
}

fn name(doc: &Document, el: ElementId, part: &Part) -> String {
    cadrs_core::parts::display_name(part, doc.element(el).unwrap().part_props()).to_string()
}

fn mass_kg(doc: &Document, el: ElementId, part: &Part) -> f64 {
    let props = doc.element(el).unwrap().part_props().to_vec();
    cadrs_core::parts::mass_report(&[part], &props).and_then(|r| r.mass).expect("a material").mass
}

#[test]
fn the_fixtures_are_current() {
    for (name, file) in ex::files().unwrap() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/{name}.cadrs"));
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
        if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
            std::fs::write(&path, &text).unwrap();
        }
        let stored = cadrs_core::Store::load_path(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(stored.document, file.document, "fixtures/{name}.cadrs is out of date");
    }
}

/// E2's mass in Carbon Steel (kg): the stand-in's own value (its profile's arc is a trapezoid).
pub const E2_MASS: f64 = 0.111469;

#[test]
fn e2_is_one_part_and_weighs_its_flat_in_carbon_steel() {
    let doc = ex::document_e2().unwrap();
    let b = build(&doc, ex::E2_STUDIO);
    assert_eq!(b.parts.len(), 1);
    let part = &b.parts[0];
    assert_eq!(part.id, ex::E2_PART);
    let ctx = &b.sheet_metal[0];
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1, "{:?}", ctx.flat.errors);
    // Its joints: the extrude's 6 bends, Flange 1's 2 bends and miter, Hem 1's 2, Flange 2's and
    // 3's bends, the butt joint, the corner override.
    assert!(ctx.model.joints.iter().any(|j| matches!(j.kind, cadrs_sheetmetal::JointKind::Rip { style: cadrs_sheetmetal::RipStyle::ButtDirection1, .. })));
    assert_eq!(ctx.model.corner_overrides.len(), 1);
    let v = part.mass.unwrap().volume;
    // The Round – Sized corner relief is cut from the bends as a wedge (an approximation of its
    // round, P3I.2), so the folded volume is within 1e-4 of the flat's.
    assert!((v - predicted(&b, ex::E2_MODEL)).abs() < 1e-4 * v, "{v} vs {}", predicted(&b, ex::E2_MODEL));
    let m = mass_kg(&doc, ex::E2_STUDIO, part);
    assert!((m - v * 7850e-9).abs() < 1e-12);
    println!("E2 mass: {m:.6} kg (volume {v:.3} mm³)");
    if E2_MASS > 0.0 {
        assert!((m - E2_MASS).abs() < 5e-7, "E2 mass {m} vs {E2_MASS}");
    }
}

#[test]
fn e3_the_sheet_metal_box_and_its_flat() {
    let doc = ex::document_e3().unwrap();
    let b = build(&doc, ex::E3_STUDIO);
    assert_eq!(b.parts.len(), 1);
    assert_eq!(name(&doc, ex::E3_STUDIO, &b.parts[0]), "Sheet Metal Box");
    let ctx = &b.sheet_metal[0];
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1);
    assert_eq!(ctx.model.joints.iter().filter(|j| j.bend().is_some()).count(), 4);
    let (lo, hi) = ctx.flat.parts[0].bounds().unwrap();
    let (w, h) = ((hi.x - lo.x).max(hi.y - lo.y), (hi.x - lo.x).min(hi.y - lo.y));
    // The bottom (200 × 125, less the bends' setbacks) with a 150 wall each side, unrolled: the
    // flat is about 200 + 2 · 150 by 125 + 2 · 150 (the bends' allowances for their outer
    // setbacks).
    println!("E3 flat: {w:.4} × {h:.4}");
    assert!((w - 500.0).abs() < 5.0 && (h - 425.0).abs() < 5.0, "{w} × {h}");
    let v = b.parts[0].mass.unwrap().volume;
    assert!((v - predicted(&b, ex::E3_MODEL)).abs() < 1e-6 * v);
}

#[test]
fn e4_the_rework_after_finish_leaves_the_flat_alone() {
    let mut doc = ex::document_e4().unwrap();
    let b = build(&doc, ex::E4_STUDIO);
    assert_eq!(b.parts.len(), 1);
    assert_eq!(name(&doc, ex::E4_STUDIO, &b.parts[0]), "Lower Enclosure");
    let ctx = &b.sheet_metal[0];
    assert!(ctx.active);
    let flat = ctx.flat.clone();
    let holes: usize = flat.parts[0].outline.iter().map(|p| p.holes.len()).sum();
    assert_eq!(holes, 2, "the slot through both walls is in the flat");
    let v0 = b.parts[0].mass.unwrap().volume;
    let mut h = History::default();
    ex::rework_e4(&mut DocHistory(&mut doc, &mut h), ex::E4_STUDIO).unwrap();
    let b = build(&doc, ex::E4_STUDIO);
    assert_eq!(b.parts.len(), 1, "the rims are added to the Lower Enclosure");
    let ctx = &b.sheet_metal[0];
    assert!(!ctx.active, "finished");
    assert_eq!(ctx.flat, flat, "the flat doesn't show the rework");
    let v1 = b.parts[0].mass.unwrap().volume;
    // Two rims of 8 × 2 round the slot's outline (less the fillets).
    let perimeter = 2.0 * (ex::E4_SLOT_SIZE.0 - ex::E4_SLOT_SIZE.1) + std::f64::consts::PI * (ex::E4_SLOT_SIZE.1 + ex::E4_RIM.1);
    let rims = 2.0 * perimeter * ex::E4_RIM.0 * ex::E4_RIM.1;
    println!("E4 volume {v0:.3} → {v1:.3} (rims ≈ {rims:.3})");
    assert!(v1 > v0 + 0.9 * rims && v1 < v0 + 1.1 * rims, "{v0} → {v1}, rims {rims}");
}
