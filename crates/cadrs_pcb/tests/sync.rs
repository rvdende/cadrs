//! P3H.5: Sync a Part Studio or assembly with PCB Studio (PCB5.2–5.6, PCB9.6, X8) and Export
//! this board to IDF (PCB9.7–9.8), on the Board Exercise stand-in (PCB6) and the fixtures. The
//! expected numbers are worked out by hand in each test.

use cadrs_core::assembly::commands::{InsertInstance, MoveInstances};
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::command::History;
use cadrs_core::commands::{AddElement, NewElementKind};
use cadrs_core::ids::ElementId;
use cadrs_core::pcb::{BoardId, BoardSource, ImportBoard, KeepKind, SyncPlaneChoice};
use cadrs_core::samples::phone_case as pc;
use cadrs_core::studio::DocHistory;
use cadrs_core::Document;
use cadrs_idf::IdfVersion;
use cadrs_pcb::sync::{SyncOutcome, plan, run_now, sources};
use cadrs_pcb::PcbBoard;

const PCB: ElementId = ElementId::from_u128(0x3a05_9000);

fn add_pcb(doc: &mut Document, h: &mut History) {
    h.execute(doc, &AddElement { id: PCB, kind: NewElementKind::PcbStudio, name: None, after: None }).unwrap();
}

fn sync(doc: &mut Document, h: &mut History, source: ElementId) -> SyncOutcome {
    let shown = doc.element(PCB).unwrap().pcb().unwrap().active;
    let p = plan(doc, PCB, source, SyncPlaneChoice::Top, shown).unwrap();
    let o = run_now(p).unwrap();
    h.execute(doc, &o.command).unwrap();
    o
}

fn board(doc: &Document) -> (BoardId, PcbBoard) {
    let s = doc.element(PCB).unwrap().pcb().unwrap();
    let b = s.active_board().unwrap();
    (b.id, b.board.clone())
}

fn points(b: &PcbBoard) -> Vec<(f64, f64, f64)> {
    b.board.outline.as_ref().unwrap().loops[0].points.iter().map(|p| (p.x, p.y, p.angle)).collect()
}

/// The stand-in with the board made in context (PCB6 steps 2–8) and a PCB Studio.
fn exercise() -> (Document, History) {
    let mut doc = pc::document().unwrap();
    let mut h = History::default();
    pc::board_in_context(&mut DocHistory(&mut doc, &mut h)).unwrap();
    add_pcb(&mut doc, &mut h);
    (doc, h)
}

/// A rounded rectangle's 9 IDF points (half sizes `a` × `b`, corner radius `r`), from its
/// lowest-right point counter-clockwise, as PCB6's `.emn` lists them.
fn rounded(a: f64, b: f64, r: f64) -> Vec<(f64, f64, f64)> {
    vec![
        (a - r, -b, 0.0),
        (a, -b + r, 90.0),
        (a, b - r, 0.0),
        (a - r, b, 90.0),
        (-(a - r), b, 0.0),
        (-a, b - r, 90.0),
        (-a, -(b - r), 0.0),
        (-(a - r), -b, 90.0),
        (a - r, -b, 0.0),
    ]
}

#[test]
fn the_sync_dialog_lists_part_studios_and_assemblies() {
    let (doc, _) = exercise();
    let labels: Vec<String> = sources(&doc).into_iter().map(|s| s.label).collect();
    assert_eq!(labels, ["Enclosure (Part Studio)", "Cell phone (Assembly)", "Board & Keep out (Part Studio)"]);
}

#[test]
fn ex1_outline_before_resize() {
    // The cavity of the 70 × 140 case: 66 × 136 with R8 corners, so half sizes 33 × 68 and the
    // arcs' ends 8 in from each corner: (25, −68), (33, −60), (33, 60), (25, 68), ...
    let (mut doc, mut h) = exercise();
    let o = sync(&mut doc, &mut h, pc::CELL_PHONE);
    let (_, b) = board(&doc);
    assert_eq!(b.name(), "Cell phone");
    assert_eq!(points(&b), rounded(33.0, 68.0, 8.0));
    assert_eq!(
        points(&b),
        [(25.0, -68.0, 0.0), (33.0, -60.0, 90.0), (33.0, 60.0, 0.0), (25.0, 68.0, 90.0), (-25.0, 68.0, 0.0), (-33.0, 60.0, 90.0), (-33.0, -60.0, 0.0), (-25.0, -68.0, 90.0), (25.0, -68.0, 0.0)]
    );
    assert_eq!(b.board.outline.as_ref().unwrap().thickness, 0.062);
    // Two keep-outs, 1 mm deep below the board's top face.
    assert_eq!(b.board.place_keepouts.len(), 2);
    for k in &b.board.place_keepouts {
        assert_eq!(k.height, Some(1.0));
        assert_eq!(k.side, cadrs_idf::Side::Bottom);
    }
    // The battery's keep-out is its 50 × 60 rectangle; the antenna's its pentagon.
    let areas: Vec<f64> = b.board.place_keepouts.iter().map(|k| k.loops[0].area()).collect();
    assert!(areas.iter().any(|a| (a - 3000.0).abs() < 1e-6), "{areas:?}");
    let pentagon = 20.0 * 8.0 + (20.0 + 12.0) / 2.0 * 6.0;
    assert!(areas.iter().any(|a| (a - pentagon).abs() < 1e-6), "{areas:?} vs {pentagon}");
    assert_eq!(b.board.place_keepouts.iter().map(|k| k.loops[0].points.len() - 1).collect::<Vec<_>>().iter().sum::<usize>(), 4 + 5);
    // The case, the battery and the antenna aren't translated (PCB5.4).
    assert_eq!(o.unrecognised, ["Enclosure", "Battery", "Antenna"]);
    let msg = cadrs_pcb::sync::message(&o, "Cell phone");
    assert!(msg.contains("Not translated: Enclosure, Battery, Antenna"), "{msg}");
}

#[test]
fn ex1_exported_outline_matches_the_course() {
    // PCB6 steps 12–18: 85 × 150, Update context, sync again, export IDF 3.0. The cavity is
    // 81 × 146 R8: half sizes 40.5 × 73, so exactly the course's `Cell phone.emn` points.
    let (mut doc, mut h) = exercise();
    sync(&mut doc, &mut h, pc::CELL_PHONE);
    let (id0, b0) = board(&doc);
    let keep0 = b0.keep_ids.of(KeepKind::PlaceKeepout).clone();
    pc::resize(&mut DocHistory(&mut doc, &mut h), pc::RESIZED.0, pc::RESIZED.1).unwrap();
    assert!(pc::update_context(&mut DocHistory(&mut doc, &mut h)).unwrap());
    sync(&mut doc, &mut h, pc::CELL_PHONE);
    let s = doc.element(PCB).unwrap().pcb().unwrap();
    assert_eq!(s.boards.len(), 1, "re-sync updates the board, no duplicate");
    let (id1, b1) = board(&doc);
    assert_eq!(id0, id1, "same board");
    assert_eq!(b1.keep_ids.of(KeepKind::PlaceKeepout), &keep0, "the keep-outs keep their ids");
    let course = [(32.5, -73.0, 0.0), (40.5, -65.0, 90.0), (40.5, 65.0, 0.0), (32.5, 73.0, 90.0), (-32.5, 73.0, 0.0), (-40.5, 65.0, 90.0), (-40.5, -65.0, 0.0), (-32.5, -73.0, 90.0), (32.5, -73.0, 0.0)];
    assert_eq!(points(&b1), course);
    // The exported file.
    let exported = cadrs_idf::export_board(&b1.board, IdfVersion::V3);
    let emn = cadrs_idf::write_emn(&exported, IdfVersion::V3);
    let lines: Vec<&str> = emn.lines().map(str::trim).collect();
    assert!(emn.starts_with(".HEADER\nBOARD_FILE 3.0 \"cadrs PCB Studio v0.1\""), "{emn}");
    assert!(lines.contains(&"\"Cell phone\" MM"), "{emn}");
    let at = lines.iter().position(|l| l.starts_with(".BOARD_OUTLINE")).unwrap();
    assert_eq!(lines[at], ".BOARD_OUTLINE MCAD");
    assert_eq!(lines[at + 1], "0.062");
    let want = ["0 32.5 -73 0", "0 40.5 -65 90", "0 40.5 65 0", "0 32.5 73 90", "0 -32.5 73 0", "0 -40.5 65 90", "0 -40.5 -65 0", "0 -32.5 -73 90", "0 32.5 -73 0"];
    let got: Vec<String> = lines[at + 2..at + 11].iter().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    assert_eq!(got, want);
    assert_eq!(lines[at + 11], ".END_BOARD_OUTLINE", "one loop: the board is one region");
    // Empty drilled holes and placements, as the course's file; two keep-outs in 3.0.
    assert!(emn.contains(".DRILLED_HOLES\n.END_DRILLED_HOLES"), "{emn}");
    assert!(emn.contains(".PLACEMENT\n.END_PLACEMENT"), "{emn}");
    assert_eq!(emn.matches(".PLACE_KEEPOUT").count(), 2);
    // One undo step takes the re-sync back to the 66 × 136 outline.
    h.undo(&mut doc).expect("an undo step");
    assert_eq!(points(&board(&doc).1), rounded(33.0, 68.0, 8.0));
}

#[test]
fn export_idf2_has_no_keepouts() {
    let (mut doc, mut h) = exercise();
    sync(&mut doc, &mut h, pc::CELL_PHONE);
    let (_, b) = board(&doc);
    let zip3 = cadrs_idf::write_zip(&b.board, &b.library, IdfVersion::V3);
    let zip2 = cadrs_idf::write_zip(&b.board, &b.library, IdfVersion::V2);
    let entries = |z: &[u8]| cadrs_idf::zip_entries(z).unwrap();
    let e3 = entries(&zip3);
    let names: Vec<&str> = e3.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Cell phone/Cell phone.emn", "Cell phone/Cell phone.emp"]);
    let text = |e: &[(String, Vec<u8>)]| String::from_utf8(e[0].1.clone()).unwrap();
    assert_eq!(text(&e3).matches(".PLACE_KEEPOUT").count(), 2);
    let t2 = text(&entries(&zip2));
    assert!(t2.starts_with(".HEADER\nBOARD_FILE 2.0"), "{t2}");
    assert!(!t2.contains("KEEPOUT") && !t2.contains("PLACE_REGION"), "{t2}");
    assert!(t2.contains(".BOARD_OUTLINE"));
}

#[test]
fn untranslated_parts_are_reported() {
    // A Part Studio source: a 60 × 40 × 1.6 Mainboard, a 10 × 10 × 3 Keepout on it and an
    // Enclosure box round both: a board with one keep-out, and "Not translated: Enclosure".
    let mut doc = Document::new("Sync a Part Studio");
    let mut h = History::default();
    let el = doc.elements[0].id;
    {
        let s = &mut DocHistory(&mut doc, &mut h);
        cadrs_pcb::sample::sync_demo_studio(s, el).unwrap();
    }
    add_pcb(&mut doc, &mut h);
    let o = sync(&mut doc, &mut h, el);
    assert_eq!(o.unrecognised, ["Enclosure"]);
    let (_, b) = board(&doc);
    assert_eq!(b.name(), doc.elements[0].name);
    assert_eq!(b.board.place_keepouts.len(), 1);
    assert_eq!(b.board.place_keepouts[0].side, cadrs_idf::Side::Top);
    assert_eq!(b.board.place_keepouts[0].height, Some(3.0));
    assert!((b.board.outline.as_ref().unwrap().loops[0].area() - 2400.0).abs() < 1e-6);
    assert_eq!(b.board.outline.as_ref().unwrap().thickness, 1.6);
    // Nothing named as a board: an error, no board.
    let mut doc2 = Document::new("Nothing");
    let mut h2 = History::default();
    add_pcb(&mut doc2, &mut h2);
    let first = doc2.elements[0].id;
    assert!(plan(&doc2, PCB, first, SyncPlaneChoice::Top, None).is_err(), "an empty studio has no parts");
}

#[test]
fn resync_keeps_board_and_keep_ids_and_reads_moved_components() {
    // PCB9.6 / PCB10's check: the secondary board imported, built into a Part Studio (one part
    // per component, "uBGA48_7.4X7.1" among them at (4.064182376174947, −16.5), rotation 90,
    // TOP) and assembled; syncing the assembly gives its placements back; moving that
    // component's instance 1 in (25.4 mm) along Y and syncing again gives Y = −16.5 + 25.4 =
    // 8.9, the same board and the same ids.
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/idf/secondary board");
    let emn = std::fs::read_to_string(dir.join("secondary board.emn")).unwrap();
    let emp = std::fs::read_to_string(dir.join("secondary board.emp")).unwrap();
    let pcb = PcbBoard::read(&emn, &emp).unwrap();
    let mut doc = Document::new("Round trip");
    let mut h = History::default();
    let studio = doc.elements[0].id;
    let parts = cadrs_pcb::sample::build_in(&mut DocHistory(&mut doc, &mut h), studio, &pcb).unwrap();
    add_pcb(&mut doc, &mut h);
    h.execute(&mut doc, &ImportBoard { element: PCB, board: Box::new(pcb.clone()), source: BoardSource::Idf { emn: "secondary board.emn".into(), emp: None } }).unwrap();
    let asm = ElementId::from_u128(0x3a05_9001);
    h.execute(&mut doc, &AddElement { id: asm, kind: NewElementKind::Assembly, name: Some("secondary board assembly".into()), after: None }).unwrap();
    let mut ubga = None;
    for p in &parts {
        let id = InstanceId::new();
        if p.name.contains("uBGA48_7.4X7.1") {
            ubga = Some(id);
        }
        h.execute(&mut doc, &InsertInstance { element: asm, instance: Instance::new(id, InstanceSource::Part { element: studio, part: p.part }, Pose::IDENTITY) }).unwrap();
    }
    let o = sync(&mut doc, &mut h, asm);
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert!(o.unrecognised.is_empty(), "{:?}", o.unrecognised);
    let (id0, b0) = board(&doc);
    assert_eq!(b0.name(), "secondary board assembly");
    assert_eq!(b0.board.placements.len(), pcb.board.placements.len());
    for p in &pcb.board.placements {
        let q = b0.board.placements.iter().find(|q| q.refdes == p.refdes).unwrap();
        assert!((q.x - p.x).abs() < 1e-9 && (q.y - p.y).abs() < 1e-9, "{} {} {} vs {} {}", p.refdes, q.x, q.y, p.x, p.y);
        assert!((q.rotation - p.rotation).abs() < 1e-9 && q.side == p.side, "{}", p.refdes);
    }
    let comp_ids = b0.component_ids.clone();
    let keep_ids = b0.keep_ids.clone();
    // Move the uBGA 25.4 mm along Y.
    let ubga = ubga.expect("the uBGA part");
    h.execute(&mut doc, &MoveInstances { element: asm, poses: vec![(ubga, Pose::translation([0.0, 25.4, 0.0]))], label: "Move".into() }).unwrap();
    sync(&mut doc, &mut h, asm);
    let (id1, b1) = board(&doc);
    assert_eq!(id0, id1);
    assert_eq!(doc.element(PCB).unwrap().pcb().unwrap().boards.len(), 2, "the import and the synced board");
    let (_, q) = b1.components().find(|(_, p)| p.package == "uBGA48_7.4X7.1").unwrap();
    assert!((q.x - 4.064182376174947).abs() < 1e-9, "{}", q.x);
    assert!((q.y - 8.9).abs() < 1e-9, "{}", q.y);
    assert_eq!(q.rotation, 90.0);
    assert_eq!(q.side, cadrs_idf::MountSide::Top);
    assert_eq!(b1.component_ids, comp_ids, "components keep their ids");
    assert_eq!(b1.keep_ids, keep_ids, "keep areas keep their ids");
    // A third sync with nothing moved changes nothing.
    sync(&mut doc, &mut h, asm);
    let (_, b2) = board(&doc);
    let (_, q2) = b2.components().find(|(_, p)| p.package == "uBGA48_7.4X7.1").unwrap();
    assert!((q2.y - 8.9).abs() < 1e-9, "no double move: {}", q2.y);
}

