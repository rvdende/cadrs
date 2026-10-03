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

/// E1's mass in Carbon Steel (kg): the stand-in tray's quiz value, 45 443.678 mm³ × 7850 kg/m³,
/// built with step 5's settings (1 mm, R1, K 0.45, minimal gap 0.025, Closed corners, Tear
/// bend reliefs). It is the same as with the default reliefs: the DXF's flat already has the
/// tray's corner notches and every bend line ends on a cut-out or the outline, so no Bend needs
/// a relief cut (Tear and Rectangle cut nothing there), no corner is closed by the bends, and no
/// rip uses the minimal gap (K 0.45 and rolled K 0.5 are the defaults).
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
    // The feature list reads like the slides' (steps 2–9).
    let names: Vec<String> = doc.element(ex::E4_STUDIO).unwrap().features().iter().skip(4).map(|f| f.name.clone()).collect();
    assert_eq!(names, ["Finish sheet metal model 1", "Sketch 3", "Plane 1", "Sketch 4", "Sweep 1", "Mirror 1", "Fillet 1", "Fillet 2"]);
    let el = doc.element(ex::E4_STUDIO).unwrap();
    let sketch8 = el.feature(ex::E4_PATH_SKETCH).unwrap().sketch().unwrap();
    assert!(matches!(sketch8.plane, Some(cadrs_sketch::PlaneRef::Face(_))), "Sketch 8 is on the wall's face");
    assert_eq!(sketch8.geometry.curves.len(), 4, "Use took the slot's two lines and two arcs");
    assert!(matches!(&el.feature(ex::E4_PLANE).unwrap().kind, cadrs_core::FeatureKind::Plane(p) if p.kind == cadrs_core::plane::PlaneType::PlanePoint));
    assert!(matches!(&el.feature(ex::E4_MIRROR).unwrap().kind, cadrs_core::FeatureKind::Mirror(m) if m.reapply));
    assert_eq!(b.parts.len(), 1, "the rims are added to the Lower Enclosure");
    let ctx = &b.sheet_metal[0];
    assert!(!ctx.active, "finished");
    assert_eq!(ctx.flat, flat, "the flat doesn't show the rework");
    let v1 = b.parts[0].mass.unwrap().volume;
    // Two collars round the slot's outline: 8 × 2 out of the wall plus the 0.5 lining through
    // it (1.5), less the fillets (3 at the slot's inside entry, 1 on the collar's top outer edge).
    let perimeter = 2.0 * (ex::E4_SLOT_SIZE.0 - ex::E4_SLOT_SIZE.1) + std::f64::consts::PI * ex::E4_SLOT_SIZE.1;
    let section = ex::E4_RIM.0 * (ex::E4_RIM.1 + ex::E4_LINING) + ex::E4_WALL * ex::E4_LINING;
    let rims = 2.0 * perimeter * section;
    println!("E4 volume {v0:.3} → {v1:.3} (collars ≈ {rims:.3})");
    assert!(v1 > v0 + 0.8 * rims && v1 < v0 + 1.1 * rims, "{v0} → {v1}, collars {rims}");
    // Mirror 1 (Reapply) put a collar on the left wall too: the part reaches 8 out of both walls.
    let part = &b.parts[0];
    let xs = part.solid.positions.iter().map(|p| p[0]).fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
    let reach = ex::E4_CHAIN[3].0 + ex::E4_RIM.0;
    assert!((xs.0 + reach).abs() < 1e-3 && (xs.1 - reach).abs() < 1e-3, "{xs:?}: a collar on each wall");
    // The fillets as the slides make them: 3 mm, then 1 mm.
    for (f, r) in [(ex::E4_FILLET_1, ex::E4_FILLETS.0), (ex::E4_FILLET_2, ex::E4_FILLETS.1)] {
        let cadrs_core::FeatureKind::Fillet(x) = &doc.element(ex::E4_STUDIO).unwrap().feature(f).unwrap().kind else { panic!("a fillet") };
        assert_eq!((x.size, x.entities.len()), (r, 2));
    }
}
