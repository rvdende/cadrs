//! Board edits by name and number (`cadrs_core::pcb::ops`, the MCP server's board tools) and a
//! PCB Studio board inserted straight into an assembly, on
//! the mains-sensor sample: its AC sensor board derived onto "Board location" in the
//! Enclosure, whose plug's Live, Earth and Neutral pins stand at (0, 8), (0, 0) and (0, −8).
#![cfg(feature = "brep")]

use cadrs_core::assembly::bom::{self, BomOptions};
use cadrs_core::assembly::commands::InsertInstance;
use cadrs_core::assembly::structure::occurrences;
use cadrs_core::assembly::{Instance, InstanceId, Pose};
use cadrs_core::derived::{AddDerived, DerivedBoard, DerivedFeature};
use cadrs_core::document::Element;
use cadrs_core::external::{RefAt, Resolver, SourceRef};
use cadrs_core::mate::ConnectorRef;
use cadrs_core::pcb::{BoardId, SetDesign, ops};
use cadrs_core::samples::pcb_enclosure as pe;
use cadrs_core::{Document, ElementId, FeatureId, History, Store};

const DERIVED: FeatureId = FeatureId::from_u128(0x7d00_0000_0000_0000_0000_0000_0000_0001);

fn run(doc: &mut Document, h: &mut History, c: &dyn cadrs_core::Command) {
    h.execute(doc, c)
        .unwrap_or_else(|e| panic!("{}: {e}", c.label()));
}

/// The sample with its board derived onto Board location.
fn derived() -> (Document, History, BoardId) {
    let mut doc = pe::document().unwrap();
    let mut h = History::default();
    let board = doc.element(pe::PCB).unwrap().pcb().unwrap().boards[0].id;
    let d = DerivedFeature {
        locations: vec![ConnectorRef::Feature(pe::BOARD_LOCATION)],
        board: Some(Box::new(DerivedBoard::new(board, "", Default::default()))),
        ..Default::default()
    };
    let mut res = Resolver::new(Store::new(
        std::env::temp_dir().join("cadrs-board-ops-unused"),
    ));
    let r = SourceRef {
        document: None,
        at: RefAt::Workspace,
        element: pe::PCB,
        pinned: false,
    };
    let got = cadrs_core::derived::resolve(&mut res, &doc, None, d, r).unwrap();
    run(
        &mut doc,
        &mut h,
        &AddDerived {
            element: pe::STUDIO,
            feature: DERIVED,
            derived: got.derived,
            links: got.links,
        },
    );
    (doc, h, board)
}

fn store(
    doc: &mut Document,
    h: &mut History,
    board: BoardId,
    f: impl FnOnce(&mut cadrs_eda::Design) -> Result<(), String>,
) {
    let mut d = ops::design_of(doc, pe::PCB, board).unwrap();
    f(&mut d).unwrap();
    run(
        doc,
        h,
        &SetDesign {
            element: pe::PCB,
            board,
            design: Box::new(d),
            label: "Edit".into(),
        },
    );
}

fn pad(doc: &Document, reference: &str, number: &str) -> [f64; 2] {
    let r = ops::board_report(
        doc,
        pe::PCB,
        doc.element(pe::PCB).unwrap().pcb().unwrap().boards[0].id,
    )
    .unwrap();
    r.footprints
        .iter()
        .find(|f| f.reference == reference)
        .unwrap()
        .pads
        .iter()
        .find(|p| p.number == number)
        .unwrap()
        .at
}

#[test]
fn a_board_is_found_by_name_and_reported() {
    let (doc, _, board) = derived();
    assert_eq!(
        ops::find_board(&doc, None, "AC sensor").unwrap(),
        (pe::PCB, board)
    );
    assert!(
        ops::find_board(&doc, None, "Nope")
            .unwrap_err()
            .contains("AC sensor")
    );
    let r = ops::board_report(&doc, pe::PCB, board).unwrap();
    assert_eq!(r.name, "AC sensor");
    assert!((r.thickness - 1.6).abs() < 1e-9);
    let j4 = r.footprints.iter().find(|f| f.reference == "J4").unwrap();
    assert_eq!(j4.pads[0].net, "LIVE");
    // Derived into the Enclosure, with PS1's collisions as its warning.
    assert_eq!(r.derived_in.len(), 1);
    assert!(
        r.derived_in[0]
            .warning
            .as_deref()
            .is_some_and(|w| w.contains("PS1")),
        "{:?}",
        r.derived_in
    );
}

#[test]
fn a_pad_goes_onto_a_pin_of_the_part_studio() {
    let (mut doc, mut h, board) = derived();
    // The Live pin's axis on the layout: the pin's (0, 8) with the board's outline centre
    // (15.5, 19.5) on the origin.
    let axes = ops::axes_of_part(&doc, pe::STUDIO, DERIVED, "Live pin", 0).unwrap();
    let at = axes[0].0;
    assert!(
        (at[0] - 15.5).abs() < 1e-6 && (at[1] - 27.5).abs() < 1e-6,
        "{at:?}"
    );
    store(&mut doc, &mut h, board, |d| ops::pad_onto(d, "J4", "1", at));
    let p = pad(&doc, "J4", "1");
    assert!(
        (p[0] - at[0]).abs() < 1e-3 && (p[1] - at[1]).abs() < 1e-3,
        "{p:?}"
    );
    // In the Part Studio, J4's wire end is on the pin's axis.
    let b = cadrs_core::rebuild::build(doc.element(pe::STUDIO).unwrap().features());
    let j4 = b.parts.iter().find(|x| x.name.starts_with("J4 ")).unwrap();
    let (lo, hi) = j4.solid.bounds().unwrap();
    assert!(
        ((lo[0] + hi[0]) / 2.0).abs() < 1e-3 && ((lo[1] + hi[1]) / 2.0 - 8.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
}

#[test]
fn footprints_are_placed_copied_and_removed() {
    let (mut doc, mut h, board) = derived();
    // Placed, turned and flipped.
    store(&mut doc, &mut h, board, |d| {
        ops::place_footprint(
            d,
            "R1",
            Some([10.0, 12.0]),
            Some(90.0),
            Some(cadrs_eda::layer::Side::Bottom),
        )
    });
    let r = ops::board_report(&doc, pe::PCB, board).unwrap();
    let r1 = r.footprints.iter().find(|f| f.reference == "R1").unwrap();
    assert_eq!((r1.at, r1.side), ([10.0, 12.0], "bottom"));
    // A copy of J4 as an Earth input on the Earth pin, on its own net.
    let earth = ops::axes_of_part(&doc, pe::STUDIO, DERIVED, "Earth pin", 0).unwrap()[0].0;
    let parts_before = cadrs_core::rebuild::build(doc.element(pe::STUDIO).unwrap().features())
        .parts
        .len();
    store(&mut doc, &mut h, board, |d| {
        ops::copy_footprint(
            d,
            "J4",
            "J6",
            "AC Earth IN",
            earth,
            &[("1".into(), "EARTH".into())],
        )
        .map(|_| ())
    });
    let r = ops::board_report(&doc, pe::PCB, board).unwrap();
    let j6 = r.footprints.iter().find(|f| f.reference == "J6").unwrap();
    assert_eq!(
        (j6.value.as_str(), j6.pads[0].net.as_str()),
        ("AC Earth IN", "EARTH")
    );
    assert!(r.nets.contains(&"EARTH".to_string()));
    assert!(
        ops::copy_footprint(
            &mut ops::design_of(&doc, pe::PCB, board).unwrap(),
            "J4",
            "J6",
            "",
            [0.0, 0.0],
            &[]
        )
        .is_err(),
        "J6 is taken"
    );
    // Its part comes into the Part Studio.
    assert_eq!(
        cadrs_core::rebuild::build(doc.element(pe::STUDIO).unwrap().features())
            .parts
            .len(),
        parts_before + 1
    );
    // PS1 removed: gone from the board and the schematic, and its collisions with it.
    store(&mut doc, &mut h, board, |d| ops::remove_footprint(d, "PS1"));
    let design = ops::design_of(&doc, pe::PCB, board).unwrap();
    assert!(
        design
            .board
            .footprints
            .iter()
            .all(|f| f.reference() != "PS1")
    );
    assert!(
        design
            .schematic
            .sheets
            .iter()
            .flat_map(|s| &s.symbols)
            .all(|s| s.reference() != "PS1")
    );
    let r = ops::board_report(&doc, pe::PCB, board).unwrap();
    assert!(
        r.derived_in[0]
            .warning
            .as_deref()
            .is_none_or(|w| !w.contains("PS1")),
        "{:?}",
        r.derived_in
    );
    // Shifting the layout moves the parts and the tracks, not the outline.
    let mut d = ops::design_of(&doc, pe::PCB, board).unwrap();
    let before = cadrs_core::pcb::board3d::outline_center(&d.board);
    let r1_at = d
        .board
        .footprints
        .iter()
        .find(|f| f.reference() == "R1")
        .unwrap()
        .placement
        .at;
    ops::shift_layout(&mut d.board, [5.0, -2.0]);
    assert_eq!(cadrs_core::pcb::board3d::outline_center(&d.board), before);
    assert_eq!(
        d.board
            .footprints
            .iter()
            .find(|f| f.reference() == "R1")
            .unwrap()
            .placement
            .at,
        r1_at + cadrs_eda::units::Pt::mm(5.0, -2.0)
    );
}

#[test]
fn a_board_in_an_assembly_follows_its_layout_and_nests_in_the_bom() {
    let (mut doc, mut h, board) = derived();
    let mut asm = Element::assembly("Assembly 1");
    let asm_id = ElementId::new();
    asm.id = asm_id;
    doc.elements.push(asm);
    let parts = doc
        .element(pe::PCB)
        .unwrap()
        .pcb()
        .unwrap()
        .board_parts(board);
    let inst = InstanceId::new();
    run(
        &mut doc,
        &mut h,
        &InsertInstance {
            element: asm_id,
            instance: Instance::board(inst, pe::PCB, board, parts.clone(), Pose::IDENTITY),
        },
    );
    let count = |doc: &Document| {
        occurrences(doc, doc.element(asm_id).unwrap().assembly_model().unwrap())
            .iter()
            .filter(|o| o.top == inst)
            .count()
    };
    assert_eq!(count(&doc), parts.len());
    // The parts are the PCB Studio's rebuild's: the board's and its components'.
    let build = cadrs_core::rebuild::build(&doc.element(pe::PCB).unwrap().part_features());
    for p in &parts {
        assert!(build.part(*p).is_some(), "{p:?} is built");
    }
    // A footprint added to the layout comes into the assembly.
    store(&mut doc, &mut h, board, |d| {
        ops::copy_footprint(d, "J4", "J6", "AC Earth IN", [15.5, 19.5], &[]).map(|_| ())
    });
    assert_eq!(count(&doc), parts.len() + 1);
    // The structured BOM: the board, its components under it.
    let builds = |e: ElementId| {
        doc.element(e)
            .map(|x| cadrs_core::rebuild::build(&x.part_features()))
    };
    let options = BomOptions {
        expand_all: true,
        ..Default::default()
    };
    let b = bom::compute(&doc, asm_id, &options, &doc.units.clone(), builds).unwrap();
    let board_row = b
        .rows
        .iter()
        .find(|r| r.has_children)
        .expect("the board's row has children");
    assert_eq!(board_row.depth, 0);
    assert_eq!(
        b.rows.iter().filter(|r| r.depth == 1).count(),
        parts.len(),
        "its components (J6 too) under it"
    );
}

#[test]
fn the_export_dialog_path_writes_a_board_and_its_components() {
    use cadrs_core::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
    let (doc, _, _) = derived();
    let features = std::sync::Arc::new(doc.element(pe::STUDIO).unwrap().active_features());
    let b = cadrs_core::rebuild::build(&features);
    let items: Vec<ExportItem> = b
        .parts
        .iter()
        .filter(|p| p.feature == DERIVED)
        .map(|p| ExportItem {
            features: features.clone(),
            part: p.id,
            name: p.name.clone(),
            pose: None,
            source: (pe::STUDIO, p.id),
            source_name: p.name.clone(),
            color: None,
        })
        .collect();
    let n = items.len();
    for format in [ModelFormat::Step, ModelFormat::Stl { binary: true }] {
        let files = cadrs_core::rebuild::exchange::export_files(ExportRequest::new(
            format,
            "Board",
            items.clone(),
        ))
        .wait()
        .expect("the job ends")
        .unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert!(!files[0].bytes.is_empty());
        if format == ModelFormat::Step {
            assert!(
                String::from_utf8_lossy(&files[0].bytes)
                    .matches("MANIFOLD_SOLID_BREP")
                    .count()
                    >= n,
                "a solid per part"
            );
        }
    }
}

/// An assembly with the Enclosure's plug and the board, the board's layout origin at
/// `offset` (the board's own origin is its layout's).
fn assembly_with_board(offset: [f64; 3]) -> (Document, History, ElementId, InstanceId, InstanceId) {
    let (mut doc, mut h, board) = derived();
    let mut asm = Element::assembly("Assembly 1");
    let asm_id = ElementId::new();
    asm.id = asm_id;
    doc.elements.push(asm);
    let plug = cadrs_core::PartId::new(pe::PLUG, 0);
    let (pi, bi) = (InstanceId::new(), InstanceId::new());
    run(
        &mut doc,
        &mut h,
        &InsertInstance {
            element: asm_id,
            instance: Instance::new(
                pi,
                cadrs_core::assembly::InstanceSource::Part {
                    element: pe::STUDIO,
                    part: plug,
                },
                Pose::IDENTITY,
            ),
        },
    );
    let parts = doc
        .element(pe::PCB)
        .unwrap()
        .pcb()
        .unwrap()
        .board_parts(board);
    let pose = Pose {
        translation: offset,
        ..Pose::IDENTITY
    };
    run(
        &mut doc,
        &mut h,
        &InsertInstance {
            element: asm_id,
            instance: Instance::board(bi, pe::PCB, board, parts, pose),
        },
    );
    (doc, h, asm_id, pi, bi)
}

#[test]
fn interference_finds_a_board_component_in_a_part() {
    use cadrs_core::assembly::interference;
    // The outline's centre (15.5, 19.5) over the plug: PS1, hanging 15 under the board, runs
    // into the plug (z −12…0).
    let (doc, _, asm_id, pi, bi) = assembly_with_board([-15.5, -19.5, 0.0]);
    let items = interference::items(&doc, doc.element(asm_id).unwrap().assembly_model().unwrap());
    let clashes = interference::check_now(items.clone(), vec![]).expect("checks");
    let ps1 = clashes
        .iter()
        .find(|c| (c.a.feature.0 == pi.0 || c.b.feature.0 == pi.0) && c.volume > 1.0);
    assert!(ps1.is_some(), "{clashes:?}");
    // Moved 40 mm aside, nothing.
    let (doc, _, asm_id, ..) = assembly_with_board([40.0, -19.5, 0.0]);
    let items = interference::items(&doc, doc.element(asm_id).unwrap().assembly_model().unwrap());
    assert!(interference::check_now(items, vec![]).unwrap().is_empty());
    let _ = bi;
}

#[test]
fn an_assembly_export_places_the_board_instance() {
    use cadrs_core::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
    let (doc, _, asm_id, _, bi) = assembly_with_board([100.0, 0.0, 50.0]);
    let asm = doc.element(asm_id).unwrap().assembly_model().unwrap();
    let pcb = std::sync::Arc::new(doc.element(pe::PCB).unwrap().active_part_features());
    let items: Vec<ExportItem> = cadrs_core::assembly::structure::occurrences(&doc, asm)
        .into_iter()
        .filter(|o| o.top == bi)
        .map(|o| ExportItem {
            features: pcb.clone(),
            part: o.part,
            name: format!("{:?}", o.part),
            pose: Some(o.pose),
            source: (o.element, o.part),
            source_name: String::new(),
            color: None,
        })
        .collect();
    let files = cadrs_core::rebuild::exchange::export_files(ExportRequest::new(
        ModelFormat::Stl { binary: false },
        "Board",
        items,
    ))
    .wait()
    .expect("ends")
    .expect("exports");
    // Every vertex moved by the instance's pose: x around 100 + the layout's 0…31.
    let text = String::from_utf8_lossy(&files[0].bytes);
    let xs: Vec<f64> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("vertex "))
        .filter_map(|v| v.split_whitespace().next()?.parse().ok())
        .collect();
    assert!(!xs.is_empty());
    let (lo, hi) = xs
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    assert!(lo > 90.0 && hi < 140.0, "{lo} … {hi}");
}

#[test]
fn a_board_in_a_derived_part_studio_exports_its_components_models() {
    use cadrs_core::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
    let (mut doc, mut h, _) = derived();
    // J4 moved apart from its board there (7 along x) by a Transform of its own.
    let enc = cadrs_core::rebuild::build(doc.element(pe::STUDIO).unwrap().features());
    let j4_there = enc
        .parts
        .iter()
        .find(|p| p.name.starts_with("J4 "))
        .unwrap()
        .id;
    let t = cadrs_core::transform::TransformFeature {
        transform_type: cadrs_core::transform::TransformType::TranslateXyz,
        parts: vec![j4_there],
        dx: 7.0,
        dx_expr: "7 mm".into(),
        ..Default::default()
    };
    run(
        &mut doc,
        &mut h,
        &cadrs_core::commands::AddFeature {
            element: pe::STUDIO,
            feature: FeatureId::new(),
            base_name: "Transform".into(),
            kind: cadrs_core::document::FeatureKind::Transform(t),
        },
    );
    // A second Part Studio deriving the Enclosure (its board and components with it).
    let mut ps = Element::part_studio("Outer");
    let outer = ElementId::new();
    ps.id = outer;
    doc.elements.push(ps);
    let mut res = Resolver::new(Store::new(
        std::env::temp_dir().join("cadrs-board-ops-unused"),
    ));
    let r = SourceRef {
        document: None,
        at: RefAt::Workspace,
        element: pe::STUDIO,
        pinned: false,
    };
    let got =
        cadrs_core::derived::resolve(&mut res, &doc, None, DerivedFeature::default(), r).unwrap();
    let f = FeatureId::new();
    run(
        &mut doc,
        &mut h,
        &AddDerived {
            element: outer,
            feature: f,
            derived: got.derived,
            links: got.links,
        },
    );
    let features = std::sync::Arc::new(doc.element(outer).unwrap().active_features());
    let b = cadrs_core::rebuild::build(&features);
    let j4 = b
        .parts
        .iter()
        .find(|p| p.name.starts_with("J4 "))
        .expect("J4 comes along");
    let items = vec![ExportItem {
        features: features.clone(),
        part: j4.id,
        name: "J4".into(),
        pose: None,
        source: (outer, j4.id),
        source_name: "J4".into(),
        color: None,
    }];
    let files = cadrs_core::rebuild::exchange::export_files(ExportRequest::new(
        ModelFormat::Step,
        "J4",
        items,
    ))
    .wait()
    .expect("ends")
    .expect("exports");
    // Its model's solid (the layout found in the Enclosure's features the Derived feature
    // keeps), not its mesh faceted: a round wire.
    let text = String::from_utf8_lossy(&files[0].bytes).to_string();
    assert!(
        text.contains("MANIFOLD_SOLID_BREP") && text.contains("CYLINDRICAL_SURFACE"),
        "{}",
        &text[..text.len().min(400)]
    );
    // Where its mesh is.
    let mut back = Document::empty("Read back");
    let mut e = Element::part_studio("Read");
    let rid = ElementId::new();
    e.id = rid;
    back.elements.push(e);
    let mut h2 = History::default();
    run(
        &mut back,
        &mut h2,
        &cadrs_core::import::AddImport {
            element: rid,
            feature: FeatureId::new(),
            file_name: "j4.step".into(),
            bytes: std::sync::Arc::new(files[0].bytes.clone()),
            y_axis_up: false,
            units: None,
        },
    );
    let read = cadrs_core::rebuild::build(back.element(rid).unwrap().features());
    let (lo, hi) = read.parts[0].solid.bounds().unwrap();
    let (mlo, mhi) = j4.solid.bounds().unwrap();
    assert!(
        (0..3).all(|k| (lo[k] - mlo[k]).abs() < 0.3 && (hi[k] - mhi[k]).abs() < 0.3),
        "STEP {lo:?} … {hi:?}, mesh {mlo:?} … {mhi:?}"
    );
}

#[test]
fn a_mate_to_a_board_component_holds_when_the_layout_moves_it() {
    use cadrs_core::assembly::connector::{
        EntityRef, ImplicitPoint, MateConnector, implicit_points,
    };
    use cadrs_core::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateType};
    let (mut doc, mut h, board) = derived();
    let mut asm = Element::assembly("Assembly 1");
    let asm_id = ElementId::new();
    asm.id = asm_id;
    doc.elements.push(asm);
    // The Live pin, fixed; the board somewhere.
    let studio = cadrs_core::rebuild::build(doc.element(pe::STUDIO).unwrap().features());
    let props = doc.element(pe::STUDIO).unwrap().part_props().to_vec();
    let live = studio
        .parts
        .iter()
        .find(|p| cadrs_core::parts::display_name(p, &props) == "Live pin")
        .unwrap()
        .id;
    let (pi, bi) = (InstanceId::new(), InstanceId::new());
    let mut pin = Instance::new(
        pi,
        cadrs_core::assembly::InstanceSource::Part {
            element: pe::STUDIO,
            part: live,
        },
        Pose::IDENTITY,
    );
    pin.fixed = true;
    run(
        &mut doc,
        &mut h,
        &InsertInstance {
            element: asm_id,
            instance: pin,
        },
    );
    let parts = doc
        .element(pe::PCB)
        .unwrap()
        .pcb()
        .unwrap()
        .board_parts(board);
    run(
        &mut doc,
        &mut h,
        &InsertInstance {
            element: asm_id,
            instance: Instance::board(
                bi,
                pe::PCB,
                board,
                parts,
                Pose {
                    translation: [40.0, 0.0, 30.0],
                    ..Pose::IDENTITY
                },
            ),
        },
    );
    // J4's occurrence and its wire's end face (flat, facing up); the pin's top face.
    let solids = cadrs_core::assembly::document_occurrence_solids(&doc, asm_id);
    let pcb_build = cadrs_core::rebuild::build(&doc.element(pe::PCB).unwrap().part_features());
    let occ = occurrences(&doc, doc.element(asm_id).unwrap().assembly_model().unwrap());
    let j4 = occ
        .iter()
        .find(|o| {
            o.top == bi
                && pcb_build
                    .part(o.part)
                    .is_some_and(|p| p.name.starts_with("J4 "))
        })
        .expect("J4's occurrence");
    let top_face = |s: &cadrs_core::Solid| {
        s.faces
            .iter()
            .filter(|f| f.plane.is_some_and(|p| p.normal()[2] > 0.99))
            .max_by(|a, b| a.center.unwrap()[2].total_cmp(&b.center.unwrap()[2]))
            .unwrap()
            .name
    };
    let centroid = |s: &cadrs_core::Solid, face| {
        implicit_points(s, &EntityRef::Face(face))
            .into_iter()
            .find(|p| matches!(p.point, ImplicitPoint::FaceCentroid(_)))
            .unwrap()
    };
    let j4_solid = &solids[&j4.id];
    let a = centroid(j4_solid, top_face(j4_solid));
    let b = centroid(&solids[&pi], top_face(&solids[&pi]));
    // J4's wire end on the pin's top: Fastened (the pin's face is flush with the board's top,
    // J4's wire end 4 above it, so the board ends 4 lower; what counts is that it follows).
    let m = Mate::new(
        MateType::Fastened,
        MateConnector::implicit(j4.id, &a),
        MateConnector::implicit(pi, &b),
    );
    run(
        &mut doc,
        &mut h,
        &cadrs_core::assembly::commands::AddMateFeature {
            element: asm_id,
            feature: MateFeature::new(MateId::from_u128(0x77), "J4 on the pin", MateKind::Mate(m)),
            poses: vec![],
        },
    );
    assert!(
        cadrs_core::assembly::resolve_after_update(&mut doc, asm_id).is_empty(),
        "the mate resolves"
    );
    let j4_xy = |doc: &Document| {
        let solids = cadrs_core::assembly::document_occurrence_solids(doc, asm_id);
        let occ = occurrences(doc, doc.element(asm_id).unwrap().assembly_model().unwrap());
        let o = occ
            .iter()
            .find(|o| o.top == bi && o.part == j4.part)
            .unwrap();
        let (lo, hi) = solids[&o.id].bounds().unwrap();
        let c = o
            .pose
            .apply([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, hi[2]]);
        [c[0], c[1]]
    };
    let at = j4_xy(&doc);
    assert!(
        at[0].abs() < 1e-3 && (at[1] - 8.0).abs() < 1e-3,
        "J4 over the Live pin: {at:?}"
    );
    // J4 moved 3 mm on the layout: the mate still holds (its face is found by name), the board
    // moves so J4 stays on the pin.
    store(&mut doc, &mut h, board, |d| {
        ops::place_footprint(d, "J4", Some([17.0, 3.5]), None, None)
    });
    assert!(
        cadrs_core::assembly::resolve_after_update(&mut doc, asm_id).is_empty(),
        "the mate still resolves"
    );
    let at = j4_xy(&doc);
    assert!(
        at[0].abs() < 1e-3 && (at[1] - 8.0).abs() < 1e-3,
        "still over the Live pin: {at:?}"
    );
}

#[test]
fn an_in_context_snapshot_of_a_board_goes_stale_when_its_layout_changes() {
    use cadrs_core::assembly::context::{self, ContextStatus};
    let (mut doc, mut h, _, pi, _) = assembly_with_board([-15.5, -19.5, 0.0]);
    let asm_id = doc
        .elements
        .iter()
        .find(|e| e.assembly_model().is_some())
        .unwrap()
        .id;
    let ctx = context::snapshot(&doc, asm_id, pi).unwrap();
    // The board's parts are in the plug's context, as of now.
    assert!(
        ctx.parts.iter().any(|p| p.element == pe::PCB),
        "{:?}",
        ctx.parts
    );
    assert_eq!(
        context::status(&doc, pe::STUDIO, &ctx),
        ContextStatus::UpToDate
    );
    // A footprint moved in the PCB Studio puts it out of date.
    let board = doc.element(pe::PCB).unwrap().pcb().unwrap().boards[0].id;
    store(&mut doc, &mut h, board, |d| {
        ops::place_footprint(d, "C1", Some([5.0, 5.0]), None, None)
    });
    assert_eq!(
        context::status(&doc, pe::STUDIO, &ctx),
        ContextStatus::OutOfDate
    );
    // Updated, it is up to date again.
    let now = context::resnapshot(&doc, pe::STUDIO, &ctx).unwrap();
    assert_eq!(
        context::status(&doc, pe::STUDIO, &now),
        ContextStatus::UpToDate
    );
}
