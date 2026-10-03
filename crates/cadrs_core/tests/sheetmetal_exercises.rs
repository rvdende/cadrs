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
    for (name, file) in ex::files(&e1_dxf()).unwrap() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/sheetmetal/{name}.cadrs"));
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
        if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
            std::fs::write(&path, &text).unwrap();
        }
        let stored = cadrs_core::Store::load_path(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(stored.document, file.document, "fixtures/sheetmetal/{name}.cadrs is out of date");
    }
}

fn e1_dxf() -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(ex::E1_DXF)).expect("the E1 DXF")
}

/// E1's mass in Carbon Steel (kg): the stand-in tray's quiz value.
pub const E1_MASS: f64 = 0.356733;

#[test]
fn e1_six_bends_on_the_imported_flat_and_its_mass() {
    let doc = ex::document_e1(&e1_dxf()).unwrap();
    let b = build(&doc, ex::E1_STUDIO);
    assert_eq!(b.parts.len(), 1);
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.model.joints.iter().filter(|j| j.bend().is_some()).count(), 6);
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1);
    // A Bend never changes the flat's size: still the tray's (the DXF's) flat, less the bends'
    // reliefs (the Bend features cut their own, as in Onshape).
    let p = cadrs_sheetmetal::Params { thickness: 1.0, bend_radius: 1.0, ..cadrs_core::sheetmetal::SheetMetalModelFeature::default_params() };
    let tray = cadrs_sheetmetal::flatten(&cadrs_sheetmetal::samples::e1_tray(p).unwrap());
    let size = |f: &cadrs_sheetmetal::flat::FlatPart| {
        let (lo, hi) = f.bounds().unwrap();
        let (w, h) = (hi.x - lo.x, hi.y - lo.y);
        (w.max(h), w.min(h))
    };
    let (a, b2) = (size(&ctx.flat.parts[0]), size(&tray.parts[0]));
    assert!((a.0 - b2.0).abs() < 1e-3 && (a.1 - b2.1).abs() < 1e-3, "{a:?} vs {b2:?}");
    assert!(ctx.flat.parts[0].area() <= tray.parts[0].area() + 1e-6);
    let part = &b.parts[0];
    let v = part.mass.unwrap().volume;
    assert!((v - predicted(&b, ex::E1_MODEL)).abs() < 1e-4 * v, "{v} vs {}", predicted(&b, ex::E1_MODEL));
    let m = mass_kg(&doc, ex::E1_STUDIO, part);
    println!("E1 mass: {m:.6} kg (volume {v:.3} mm³)");
    if E1_MASS > 0.0 {
        assert!((m - E1_MASS).abs() < 5e-7, "E1 mass {m} vs {E1_MASS}");
    }
}

/// E2's mass in Carbon Steel (kg): the stand-in's own value (its profile's arc is a trapezoid).
pub const E2_MASS: f64 = 0.111473;

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
    assert!(ctx.flat.is_ok() && ctx.flat.parts.len() == 1, "{:?}", ctx.flat.errors);
    let fp = &ctx.flat.parts[0];
    // Seven bends, as the exercise's part: front, back and both sides off the bottom, the shelf
    // off the back, a flange on each sloping edge.
    assert_eq!(ctx.model.joints.iter().filter(|j| j.bend().is_some()).count(), 7);
    assert_eq!(fp.bends.len(), 7);
    // All seven fold the same way: DOWN seen from the flat's top (`goal.png`).
    assert!(fp.bends.iter().all(|b| b.up == fp.bends[0].up), "{:?}", fp.bends.iter().map(|b| b.up).collect::<Vec<_>>());
    let p = &ctx.model.params;
    let (t, r, k) = (p.thickness, p.bend_radius, p.k_factor);
    let ba = |deg: f64| deg.to_radians() * (r + k * t);
    let ex::E3Size { width: w, depth: d, back: hb, front: hf, shelf: sh, flange } = ex::E3;
    let (lo, hi) = fp.bounds().unwrap();
    let (along, across) = (hi.x - lo.x, hi.y - lo.y);
    println!("E3 flat: {along:.4} × {across:.4}");
    // The faces are the sheet's outside (the material is inside the block, as the exercise's
    // part: 200 wide, 200 and 125 high, a 75 shelf), so a wall's flat runs from its bend's
    // tangent line, R + T in from the outside corner. Along the strip: front, bottom, back and
    // shelf less three bend deductions 2 (R + T) − π/2 (R + K·T): 650 − 3 · 2.5835 = 642.249,
    // `goal.png`'s 642.25.
    let bd = 2.0 * (r + t) - ba(90.0);
    let strip = hf + d + hb + sh - 3.0 * bd;
    assert!((strip - 642.2494).abs() < 1e-3);
    assert!((along - strip).abs() < 1e-3, "{along} vs {strip}");
    // Across: the bottom, a side's bend and its back corner each way, then the flange's far
    // corner beyond it, square to the sloping edge, which leans at θ = atan(75 / 175) =
    // 23.199° (`ex3-drawings/step-06`'s 23.2°). The flange bends from the edge (Hold line):
    // its bend allowance and its flat, F − (R + T).
    let theta = (hb - hf).atan2(d - sh);
    let across_want = 2.0 * ((w / 2.0 - (r + t)) + ba(90.0) + (hb - (r + t)) + theta.cos() * (ba(90.0) + flange - (r + t)));
    assert!((across - across_want).abs() < 1e-3, "{across} vs {across_want}");
    // The two flanges' bend lines are oblique: at θ to the sides' bend lines (the angle in the
    // flat between a side's front edge and its flange's edge is 90° + θ = 113.2°, as `goal.png`).
    let dir = |b: &cadrs_sheetmetal::flat::FlatBend| (b.center.b.y - b.center.a.y).atan2(b.center.b.x - b.center.a.x).to_degrees().rem_euclid(180.0);
    let oblique: Vec<f64> = fp.bends.iter().map(dir).filter(|a| a.abs() > 1e-6 && (a - 90.0).abs() > 1e-6 && (a - 180.0).abs() > 1e-6).collect();
    assert_eq!(oblique.len(), 2, "{oblique:?}");
    for a in &oblique {
        assert!((a.min(180.0 - a) - theta.to_degrees()).abs() < 1e-3, "{a} vs {}", theta.to_degrees());
    }
    // Each oblique line is as long as the sloping edge, √(175² + 75²) = 190.394.
    for b in fp.bends.iter().filter(|b| oblique.contains(&dir(b))) {
        let l = (b.center.b - b.center.a).norm();
        assert!((l - (d - sh).hypot(hb - hf)).abs() < 1e-3, "{l}");
    }
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



