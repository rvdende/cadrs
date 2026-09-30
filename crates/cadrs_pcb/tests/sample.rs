//! The PCB board built into a Part Studio through the command layer (`cadrs_pcb::sample`).

use std::f64::consts::PI;
use std::path::PathBuf;

use cadrs_core::studio::DocHistory;
use cadrs_core::{Document, Element, History, parts::display_name};
use cadrs_pcb::{BodyClass, PcbBoard, sample};

fn fixture(dir: &str, name: &str) -> PcbBoard {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/idf").join(dir);
    let emn = std::fs::read_to_string(d.join(format!("{name}.emn"))).unwrap();
    let emp = std::fs::read_to_string(d.join(format!("{name}.emp"))).unwrap();
    PcbBoard::read(&emn, &emp).unwrap()
}

#[test]
fn vision_pcb_part_studio() {
    let pcb = fixture("vision controller", "Vision PCB");
    let mut d = Document::empty("PCB");
    let el = Element::part_studio("Vision PCB");
    let id = el.id;
    d.elements.push(el);
    let mut h = History::default();
    let made = sample::build_in(&mut DocHistory(&mut d, &mut h), id, &pcb).unwrap();
    // The board and 29 components (the route keep-outs aren't parts).
    assert_eq!(made.len(), 30);
    let e = d.element(id).unwrap();
    let build = cadrs_core::rebuild::build(e.features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 30);
    let props = e.part_props().to_vec();
    let board = build.parts.iter().find(|p| display_name(p, &props) == "Board [Vision PCB]").unwrap();
    let t = pcb.thickness();
    let v = board.mass.unwrap().volume;
    let expect = (101.6 * 76.2 - 4.0 * PI * (3.175f64 / 2.0).powi(2)) * t;
    assert!((v - expect).abs() < 1e-6 * expect, "{v} vs {expect}");
    let u1 = build.parts.iter().find(|p| display_name(p, &props) == "U1 QFP100_600MIL").unwrap();
    assert!((u1.mass.unwrap().volume - 232.2576 * 1.6002).abs() < 1e-6);
    // Bottom components hang below the board.
    let c10 = build.parts.iter().find(|p| display_name(p, &props).starts_with("C10 ")).unwrap();
    assert!(c10.mass.unwrap().center_of_mass.z < 0.0);
    assert_eq!(made.iter().filter(|p| p.class == BodyClass::Board).count(), 1);
}

#[test]
fn cell_phone_part_studio() {
    let pcb = fixture("cell phone", "Cell phone");
    let mut d = Document::empty("PCB");
    let el = Element::part_studio("Cell phone");
    let id = el.id;
    d.elements.push(el);
    let mut h = History::default();
    let made = sample::build_in(&mut DocHistory(&mut d, &mut h), id, &pcb).unwrap();
    assert_eq!(made.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Board [Cell phone]", "Keep-out 1", "Keep-out 2"]);
    let build = cadrs_core::rebuild::build(d.element(id).unwrap().features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let vols: Vec<f64> = build.parts.iter().map(|p| p.mass.unwrap().volume).collect();
    assert!((vols[0] - (11570.0 + 64.0 * PI) * 0.062).abs() < 1e-6, "{vols:?}");
    assert!((vols[1] - 5400.0).abs() < 1e-6 && (vols[2] - 900.0).abs() < 1e-6, "{vols:?}");
    // The keep-outs are below the board.
    assert!(build.parts[1].mass.unwrap().center_of_mass.z < 0.0);
    // Everything is undoable.
    assert!(h.can_undo());
}

#[test]
fn board_mesh_has_every_body() {
    // P3H.3: the viewport's meshes (one per body, in the board frame) and their bounds.
    let pcb = fixture("vision controller", "Vision PCB");
    let m = cadrs_pcb::mesh::board_mesh(&pcb).unwrap();
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    // Board, 8 keep-out markers, 29 components.
    assert_eq!(m.bodies.len(), 1 + 8 + 29);
    assert!(m.bodies.iter().all(|b| !b.indices.is_empty() && b.positions.len() == b.normals.len() && !b.edges.is_empty()));
    let (lo, hi) = m.bounds().unwrap();
    assert!((lo[0] - 0.0).abs() < 1e-3 && (hi[0] - 101.6).abs() < 1e-3, "{lo:?} {hi:?}");
    assert!((lo[1] - 0.0).abs() < 1e-3 && (hi[1] - 76.2).abs() < 1e-3, "{lo:?} {hi:?}");
    assert!(m.bodies.iter().any(|b| b.class == BodyClass::Board && b.color == cadrs_pcb::colors::BOARD_GREEN));
}

#[test]
fn custom_part_document_has_one_off_centre_part() {
    // P3H.4: the custom representation fixture. One part: the 14 × 14 × 1.4 body plus the
    // 8 × 9 × 2 heatsink block, modelled from the origin; Center moves it by (−7, −7, 0).
    let (doc, el, part) = sample::custom_part_document().unwrap();
    assert_eq!(doc.name, sample::CUSTOM_PART_DOCUMENT);
    let e = doc.element(el).unwrap();
    let build = cadrs_core::rebuild::build(e.features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 1);
    let p = build.part(part).unwrap();
    assert_eq!(display_name(p, e.part_props()), sample::CUSTOM_PART_NAME);
    let v = p.mass.unwrap().volume;
    let expect = 14.0 * 14.0 * 1.4 + 8.0 * 9.0 * 2.0;
    assert!((v - expect).abs() < 1e-6 * expect, "{v} vs {expect}");
    let t = cadrs_core::pcb::PartTransform::default().centered(&p.solid.positions);
    for (a, b) in t.translate.iter().zip([-7.0, -7.0, 0.0]) {
        assert!((a - b).abs() < 1e-9, "{:?}", t.translate);
    }
}
