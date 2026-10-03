//! P3I.6: the flat DXF export read back (SM15), and exercise E1's import: the stand-in tray's
//! flat pattern DXF (`fixtures/sheetmetal/flat_pattern_e1.dxf`, written by our own flat export)
//! inserted into a sketch on Top in millimetres closes 7 sheet regions (split by the 6 bend
//! lines) around 9 cut-outs, and the Sheet metal model's Thicken of the 7 makes a flat part of
//! the flat's area.
#![cfg(feature = "occt")]

use cadrs_core::commands::{AddFeature, AddSketch};
use cadrs_core::document::{Document, FeatureKind, RegionRef, interior_point};
use cadrs_core::dxf_import::{DxfUnits, InsertDxf, sketch_of};
use cadrs_core::flat_export::{FlatExportOptions, flat_page};
use cadrs_core::sheetmetal::{SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::{FeatureId, History, rebuild};
use cadrs_drawing::dxf::{DxfVersion, read_dxf, write_dxf_version};
use cadrs_sheetmetal::{Params, flatten, samples};
use cadrs_sketch::{PlaneRef, Vec2};

fn params() -> Params {
    Params { thickness: 1.0, bend_radius: 1.0, ..SheetMetalModelFeature::default_params() }
}

fn fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sheetmetal/flat_pattern_e1.dxf")
}

#[test]
fn the_e1_fixture_is_our_tray_flat() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test flat_dxf`.
    let flat = flatten(&samples::e1_tray(params()).unwrap());
    assert!(flat.is_ok(), "{:?}", flat.errors);
    let page = flat_page(&flat.parts[0], &[], &FlatExportOptions::default(), "Flat Pattern");
    let text = write_dxf_version(&page, DxfVersion::R2000);
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(fixture().parent().unwrap()).unwrap();
        std::fs::write(fixture(), &text).unwrap();
    }
    let stored = std::fs::read_to_string(fixture()).expect("the fixture exists");
    assert_eq!(stored, text, "fixtures/sheetmetal/flat_pattern_e1.dxf is out of date");
    // Read back: the outline, 9 cut-outs and 6 bend lines.
    let d = read_dxf(&stored).unwrap();
    let on = |l: &str| d.layers.iter().filter(|x| *x == l).count();
    assert_eq!(on("OUTLINE"), 1);
    assert_eq!(on("CUTOUTS"), 9);
    assert_eq!(on("BEND_UP") + on("BEND_DOWN"), 6);
}

#[test]
fn e1_import_closes_seven_sheet_regions_and_thickens() {
    let d = read_dxf(&std::fs::read_to_string(fixture()).unwrap()).unwrap();
    let (geometry, report) = sketch_of(&d, DxfUnits::Millimeter, true);
    // The outline's and the slots' polyline segments, and the 6 bend lines.
    assert!(report.lines > 6, "{report:?}");
    assert_eq!(report.circles, 5, "{report:?}");
    // Through the command layer into a sketch on Top.
    let mut doc = Document::new("Exercise: Import DXF");
    let el = doc.elements[0].id;
    let mut h = History::default();
    let sk = FeatureId::new();
    h.execute(&mut doc, &AddSketch { element: el, feature: sk, plane: Some(PlaneRef::Top) }).unwrap();
    h.execute(&mut doc, &InsertDxf { element: el, feature: sk, geometry }).unwrap();
    assert_eq!(h.undo_label(), Some("Insert DXF/DWG"));
    let features = doc.element(el).unwrap().features().to_vec();
    let sketch = &features.iter().find(|f| f.id == sk).unwrap().sketch().unwrap().geometry;
    let regions = cadrs_sketch::region::regions(sketch);
    // The sheet regions: the ones with material inside (not a cut-out's inside).
    let flat = flatten(&samples::e1_tray(params()).unwrap());
    let outline = &flat.parts[0].outline[0];
    let sheet: Vec<&cadrs_sketch::Region> = regions
        .iter()
        .filter(|r| {
            let p = interior_point(r);
            outline.contains(cadrs_sheetmetal::poly::P2::new(p.x, p.y))
        })
        .collect();
    assert_eq!(regions.len(), 16, "7 sheet regions and 9 cut-outs");
    assert_eq!(sheet.len(), 7);
    let area: f64 = sheet.iter().map(|r| r.area()).sum();
    // The DXF's round holes are true circles in the sketch; the flat's are 64-sided polygons.
    assert!((area - flat.parts[0].area()).abs() < 2e-4 * area, "{area} vs {}", flat.parts[0].area());
    // Sheet metal model, Thicken of the 7 regions, 1 mm.
    let p = params();
    let x = SheetMetalModelFeature {
        operation: SheetMetalOp::Thicken,
        regions: sheet.iter().map(|r| RegionRef::new(sk, r)).collect(),
        params: p,
        exprs: cadrs_core::sheetmetal::SheetMetalExprs::of(&p),
        ..Default::default()
    };
    h.execute(&mut doc, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(x) }).unwrap();
    let b = rebuild::build(doc.element(el).unwrap().features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 1);
    let v = b.parts[0].mass.as_ref().unwrap().volume;
    assert!((v - area).abs() < 0.05 * 1.0 + 1e-3 * area, "{v} vs {area}");
    let _ = Vec2::ZERO;
}
