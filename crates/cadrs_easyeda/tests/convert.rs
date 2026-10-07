//! Converting EasyEDA part data, offline: `tests/data/C2764087.json` is the API's answer for
//! the Ai-Thinker Ra-01SH LoRa module (LCSC C2764087), from the JLCEDA/EasyEDA official library
//! (https://easyeda.com, https://lceda.cn).

use cadrs_eda::footprint::{PadKind, PadShape};
use cadrs_eda::layer::Layer;
use cadrs_eda::symbol::{PinType, fields};
use cadrs_eda::units::SCHEMATIC_GRID;
use cadrs_easyeda::convert::{ModelRef, field_names, obj_extent, place_model};
use cadrs_easyeda::{PartInfo, convert};

fn ra01sh() -> cadrs_easyeda::Converted {
    let v: serde_json::Value = serde_json::from_str(include_str!("data/C2764087.json")).unwrap();
    convert(&v["result"], "LCSC", &PartInfo::default()).unwrap()
}

#[test]
fn ra01sh_symbol() {
    let c = ra01sh();
    let s = &c.symbol;
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(s.name(), "RA-01SH");
    assert_eq!(s.pins.len(), 16);
    let pin = |n: &str| s.pins.iter().find(|p| p.number == n).unwrap();
    assert_eq!((pin("1").name.as_str(), pin("9").name.as_str(), pin("12").name.as_str()), ("ANT", "GND", "SCK"));
    // Pins 1–8 down the left, pointing right into the body; 9–16 up the right.
    assert_eq!((pin("1").angle, pin("9").angle), (0.0, 180.0));
    assert!(pin("1").at.x < 0 && pin("9").at.x > 0);
    assert!(pin("1").at.y > pin("8").at.y);
    assert_eq!(pin("1").length, cadrs_eda::units::mm(2.54));
    for p in &s.pins {
        assert_eq!((p.at.x % SCHEMATIC_GRID, p.at.y % SCHEMATIC_GRID), (0, 0), "pin {}", p.number);
        assert_eq!(p.kind, PinType::Passive);
    }
    assert_eq!(s.field(fields::REFERENCE).unwrap().value(), "L?");
    assert_eq!(s.field(fields::FOOTPRINT).unwrap().value(), "LCSC:WIRELM-SMD_RA-01SH");
    assert_eq!(s.field(field_names::LCSC).unwrap().value(), "C2764087");
    assert_eq!(s.field(field_names::MPN).unwrap().value(), "Ra-01SH");
    assert!(s.field(field_names::SOURCE).unwrap().value().contains("EasyEDA"));
    // The body rectangle and the pin-1 dot.
    assert_eq!(s.graphics.len(), 2);
}

#[test]
fn ra01sh_footprint_matches_the_kicad_projects() {
    let c = ra01sh();
    let f = &c.footprint;
    assert_eq!(f.id, "LCSC:WIRELM-SMD_RA-01SH");
    assert_eq!(f.pads.len(), 16);
    // The redraw of the KiCad project (whose footprint came from EasyEDA too).
    let k = cadrs_eda::power_monitor::ra01sh_footprint();
    for kp in &k.pads {
        let p = f.pads.iter().find(|p| p.number == kp.number).unwrap();
        let d = p.at - kp.at;
        assert!(d.x.abs() <= 1000 && d.y.abs() <= 1000, "pad {}: {:?} vs {:?}", p.number, p.at, kp.at);
        assert!((p.size.w - kp.size.w).abs() <= 1000 && (p.size.h - kp.size.h).abs() <= 1000, "pad {} size", p.number);
        assert_eq!((p.kind, &p.shape), (PadKind::Smd, &PadShape::Rect));
    }
    assert_eq!(f.pads[0].pin_function, "ANT");
    // Silkscreen, the body outline on fab, and a courtyard round it all.
    assert!(f.shapes.iter().filter(|s| s.layer == Layer::TopSilk).count() >= 20);
    let court: Vec<_> = f.shapes.iter().filter(|s| s.layer == Layer::TopCourtyard).collect();
    assert_eq!(court.len(), 1);
    assert_eq!(cadrs_eda::lib_edit::check_footprint(f), Vec::<String>::new());
    // The 3D model: the outline is centred on the footprint's origin, on the board.
    let m = c.model.as_ref().unwrap();
    assert_eq!(m.uuid, "1d334852bc03468d8ea3a0895b2b9130");
    assert!(m.center[0].abs() < 1e-3 && m.center[1].abs() < 1e-3, "{:?}", m.center);
    assert_eq!((m.z, m.rotation), (0.0, [0.0; 3]));
}

#[test]
fn models_sit_as_easyeda_places_them() {
    // An OBJ whose origin is at one end (as a pin header's is) and that reaches 3 mm below its
    // origin: placed with its outline centred where EasyEDA's is, its lowest point at z.
    let obj = "v -100.33 -1.27 -3\nv 1.27 1.27 8.5\n";
    let m = ModelRef { uuid: "x".into(), name: "x".into(), center: [0.0, 0.0], z: -3.0, rotation: [0.0; 3] };
    let model = place_model(&m, "x.step", Some(obj));
    assert_eq!(model.offset, [49.53, 0.0, 0.0]);
    assert_eq!(obj_extent(obj, [0.0, 0.0, 90.0]).map(|(lo, _)| (lo[0] * 100.0).round() / 100.0), Some(-1.27));
    // Turned a quarter: the view turns by minus the stored angle.
    let turned = place_model(&ModelRef { rotation: [0.0, 0.0, 90.0], ..m }, "x.step", Some(obj));
    assert_eq!(turned.rotation, [0.0, 0.0, 270.0]);
    let body = turned.body.unwrap();
    assert_eq!(body.bounds().map(|(lo, hi)| (lo[0], hi[2])), Some((-100.33, 8.5)));
}

#[test]
fn search_results_parse() {
    let v = serde_json::json!({
        "code": 200,
        "data": { "componentPageInfo": { "total": 4, "list": [{
            "componentCode": "C2764087", "componentModelEn": "Ra-01SH", "componentBrandEn": "Ai-Thinker",
            "componentSpecificationEn": "SMD-16", "describe": "LoRa module", "componentTypeEn": "LoRa Modules",
            "stockCount": 1793, "componentLibraryType": "expand",
            "componentPrices": [{ "startNumber": 1, "productPrice": 4.5454 }],
            "dataManualUrl": "https://example.com/ds.pdf"
        }]}}
    });
    let (hits, total) = cadrs_easyeda::api::parse_search(&v).unwrap();
    assert_eq!(total, 4);
    assert_eq!((hits[0].lcsc.as_str(), hits[0].mpn.as_str(), hits[0].basic, hits[0].price), ("C2764087", "Ra-01SH", false, Some(4.5454)));
    assert_eq!(cadrs_easyeda::lcsc_number(" c2764087 ").as_deref(), Some("C2764087"));
    assert_eq!(cadrs_easyeda::lcsc_number("Ra-01SH"), None);
}

/// Downloads the Ra-01SH into a library folder and reads it back as a library (network).
#[test]
#[ignore]
fn import_ra01sh_online() {
    let dir = std::env::temp_dir().join(format!("cadrs-easyeda-{}", uuid::Uuid::new_v4())).join("LCSC");
    let r = cadrs_easyeda::import("C2764087", &dir, &PartInfo::default()).unwrap();
    assert_eq!((r.symbol.as_str(), r.footprint.as_str()), ("LCSC:RA-01SH", "LCSC:WIRELM-SMD_RA-01SH"));
    let step = r.model.clone().unwrap();
    assert!(std::fs::read(&step).unwrap().starts_with(b"ISO-10303-21"));
    let (lib, errors) = cadrs_eda::library::load_library(&dir, cadrs_eda::library::Scope::Global);
    assert!(errors.is_empty(), "{errors:?}");
    let f = &lib.footprints[0];
    assert_eq!(f.models[0].source, step.to_string_lossy());
    assert!(f.models[0].body.is_some());
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    eprintln!("{:?}", r.warnings);
}
