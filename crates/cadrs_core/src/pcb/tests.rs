//! P3H.3: the PCB Studio element model (boards, undo/redo, persistence), Import ECAD files'
//! pairing and reading, settings.

use std::path::PathBuf;

use super::import::{chosen_label, import_files, pair_files, read_board};
use super::*;
use crate::command::History;
use crate::commands::{AddElement, DeleteElement, NewElementKind};

fn idf_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/idf")
}

fn fixture(dir: &str, name: &str) -> (PathBuf, PathBuf) {
    let d = idf_dir().join(dir);
    (d.join(format!("{name}.emn")), d.join(format!("{name}.emp")))
}

fn vision() -> PcbBoard {
    let (emn, emp) = fixture("vision controller", "Vision PCB");
    PcbBoard::read(&std::fs::read_to_string(emn).unwrap(), &std::fs::read_to_string(emp).unwrap()).unwrap()
}

fn secondary() -> PcbBoard {
    let (emn, emp) = fixture("secondary board", "secondary board");
    PcbBoard::read(&std::fs::read_to_string(emn).unwrap(), &std::fs::read_to_string(emp).unwrap()).unwrap()
}

fn idf_source(name: &str) -> BoardSource {
    BoardSource::Idf { emn: format!("{name}.emn"), emp: Some(format!("{name}.emp")) }
}

/// A document with a Part Studio and a new PCB Studio (its creation already undoable).
fn doc_with_studio() -> (Document, History, ElementId) {
    let mut d = Document::new("PCB");
    let mut h = History::default();
    let id = ElementId::new();
    h.execute(&mut d, &AddElement { id, kind: NewElementKind::PcbStudio, name: None, after: None }).unwrap();
    (d, h, id)
}

fn studio(d: &Document, id: ElementId) -> &PcbStudio {
    d.element(id).unwrap().pcb().unwrap()
}

fn import(d: &mut Document, h: &mut History, id: ElementId, b: PcbBoard) {
    let name = b.name().to_string();
    h.execute(d, &ImportBoard { element: id, board: Box::new(b), source: idf_source(&name) }).unwrap();
}

fn names(s: &PcbStudio) -> Vec<&str> {
    s.boards.iter().map(|b| b.name()).collect()
}

#[test]
fn add_pcb_studio_is_undoable() {
    let (mut d, mut h, id) = doc_with_studio();
    let el = d.element(id).unwrap();
    assert_eq!(el.name, "PCB Studio 1");
    assert!(el.pcb().is_some_and(|s| s.boards.is_empty() && s.active.is_none()));
    assert_eq!(h.undo_label(), Some("Create PCB Studio"));
    // The next one is "PCB Studio 2".
    let id2 = ElementId::new();
    h.execute(&mut d, &AddElement { id: id2, kind: NewElementKind::PcbStudio, name: None, after: None }).unwrap();
    assert_eq!(d.element(id2).unwrap().name, "PCB Studio 2");
    h.undo(&mut d);
    h.undo(&mut d);
    assert!(d.element(id).is_none());
    assert_eq!(d.elements.len(), 2);
    h.redo(&mut d);
    assert_eq!(d.element(id).unwrap().name, "PCB Studio 1");
    // Deleting the tab and undoing it brings the boards back.
    import(&mut d, &mut h, id, vision());
    h.execute(&mut d, &DeleteElement { id }).unwrap();
    assert!(d.element(id).is_none());
    h.undo(&mut d);
    assert_eq!(names(studio(&d, id)), ["Vision PCB"]);
}

#[test]
fn import_adds_boards_and_makes_them_active() {
    let (mut d, mut h, id) = doc_with_studio();
    import(&mut d, &mut h, id, vision());
    let s = studio(&d, id);
    assert_eq!(names(s), ["Vision PCB"]);
    assert_eq!(s.active_board().unwrap().name(), "Vision PCB");
    assert_eq!(s.active_board().unwrap().board.board.placements.len(), 29);
    // Importing again adds another board (PCB4.4), shown.
    import(&mut d, &mut h, id, secondary());
    let s = studio(&d, id);
    assert_eq!(names(s), ["Vision PCB", "secondary board"]);
    assert_eq!(s.active_board().unwrap().name(), "secondary board");
    assert_ne!(s.boards[0].id, s.boards[1].id);
    // The same board again gets a distinct name.
    import(&mut d, &mut h, id, vision());
    assert_eq!(names(studio(&d, id)), ["Vision PCB", "secondary board", "Vision PCB (1)"]);
    // Undo and redo, one import at a time.
    assert_eq!(h.undo_label(), Some("Import Vision PCB"));
    h.undo(&mut d);
    h.undo(&mut d);
    let s = studio(&d, id);
    assert_eq!(names(s), ["Vision PCB"]);
    assert_eq!(s.active_board().unwrap().name(), "Vision PCB");
    h.redo(&mut d);
    assert_eq!(studio(&d, id).active_board().unwrap().name(), "secondary board");
}

#[test]
fn delete_board_is_undoable_and_moves_the_active_board() {
    let (mut d, mut h, id) = doc_with_studio();
    import(&mut d, &mut h, id, vision());
    import(&mut d, &mut h, id, secondary());
    let (a, b) = (studio(&d, id).boards[0].id, studio(&d, id).boards[1].id);
    // Deleting the active (last) board shows the one before it.
    h.execute(&mut d, &DeleteBoard { element: id, board: b }).unwrap();
    assert_eq!(names(studio(&d, id)), ["Vision PCB"]);
    assert_eq!(studio(&d, id).active, Some(a));
    h.undo(&mut d);
    assert_eq!(names(studio(&d, id)), ["Vision PCB", "secondary board"]);
    assert_eq!(studio(&d, id).active, Some(b));
    h.redo(&mut d);
    assert_eq!(studio(&d, id).active, Some(a));
    // Deleting the first of two while it's active shows the next one; the last leaves none.
    h.undo(&mut d);
    d.element_mut(id).unwrap().pcb_mut().unwrap().active = Some(a);
    h.execute(&mut d, &DeleteBoard { element: id, board: a }).unwrap();
    assert_eq!(studio(&d, id).active, Some(b));
    h.execute(&mut d, &DeleteBoard { element: id, board: b }).unwrap();
    assert!(studio(&d, id).boards.is_empty() && studio(&d, id).active.is_none());
    // A new board never reuses a deleted board's id.
    import(&mut d, &mut h, id, vision());
    assert!(studio(&d, id).boards[0].id.0 > b.0);
}

#[test]
fn delete_board_keeps_generated_tabs() {
    // PCB3.6: tabs made from a board (a Part Studio and an Assembly here) stay.
    let (mut d, mut h, id) = doc_with_studio();
    import(&mut d, &mut h, id, vision());
    let tabs: Vec<ElementId> = d.elements.iter().map(|e| e.id).collect();
    let b = studio(&d, id).boards[0].id;
    h.execute(&mut d, &DeleteBoard { element: id, board: b }).unwrap();
    assert_eq!(d.elements.iter().map(|e| e.id).collect::<Vec<_>>(), tabs);
    assert_eq!(d.elements.len(), 3);
}

#[test]
fn pcb_studio_survives_save_and_load() {
    let (mut d, mut h, id) = doc_with_studio();
    import(&mut d, &mut h, id, vision());
    import(&mut d, &mut h, id, secondary());
    let settings = PcbSettings {
        library: Some(LibraryRef { document: crate::DocumentId::from_u128(7), name: "Parts library".into() }),
        component_folder: Some(FolderRef { id: crate::FolderId::from_u128(8), name: "PCB Components".into() }),
    };
    h.execute(&mut d, &SetPcbSettings { element: id, settings: settings.clone() }).unwrap();
    // RON text, as the store writes it.
    let text = ron::to_string(&d).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back, d);
    // Through the store.
    let root = std::env::temp_dir().join(format!("cadrs-pcb-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = crate::Store::new(&root);
    let meta = crate::DocumentMeta::new("me", 0);
    store.save(&d, &meta).unwrap();
    let loaded = store.load(d.id).unwrap().document;
    let s = studio(&loaded, id);
    assert_eq!(names(s), ["Vision PCB", "secondary board"]);
    assert_eq!(s.active_board().unwrap().name(), "secondary board");
    assert_eq!(s.settings, settings);
    assert_eq!(loaded, d);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn settings_are_undoable() {
    let (mut d, mut h, id) = doc_with_studio();
    assert_eq!(studio(&d, id).settings, PcbSettings::default());
    assert_eq!(studio(&d, id).settings.library_name(), "Default library");
    let s = PcbSettings { library: None, component_folder: Some(FolderRef { id: crate::FolderId::from_u128(1), name: "PCB parts".into() }) };
    h.execute(&mut d, &SetPcbSettings { element: id, settings: s.clone() }).unwrap();
    assert_eq!(studio(&d, id).settings, s);
    assert_eq!(h.undo_label(), Some("Update PCB Studio settings"));
    h.undo(&mut d);
    assert_eq!(studio(&d, id).settings, PcbSettings::default());
    h.redo(&mut d);
    assert_eq!(studio(&d, id).settings, s);
    // Not a PCB Studio: refused.
    let ps = d.elements[0].id;
    assert!(h.execute(&mut d, &SetPcbSettings { element: ps, settings: PcbSettings::default() }).is_err());
}

#[test]
fn p3h3_path_settings_read_as_unset() {
    // P3H.3 kept plain paths; they read as "not chosen" rather than failing the document.
    let old = "(library: Some(\"/tmp/lib.ron\"), component_folder: Some(\"/tmp/parts\"))";
    let s: PcbSettings = ron::from_str(old).unwrap();
    assert_eq!(s, PcbSettings::default());
}

// ---------------------------------------------------------------------------------------------
// Import ECAD files: pairing and reading

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn pairs_emn_with_emp_by_name() {
    let r = pair_files(&[p("/d/Vision PCB.emp"), p("/d/Vision PCB.emn")]).unwrap();
    assert_eq!(r.pairs, [import::FilePair { emn: p("/d/Vision PCB.emn"), emp: Some(p("/d/Vision PCB.emp")) }]);
    assert!(r.warnings.is_empty());
    // Case-insensitive extensions and names; two boards at once.
    let r = pair_files(&[p("/a/B.EMN"), p("/a/b.emp"), p("/c/x.emn"), p("/c/X.emp")]).unwrap();
    assert_eq!(r.pairs.len(), 2);
    assert_eq!(r.pairs[0].emp, Some(p("/a/b.emp")));
    assert_eq!(r.pairs[1].emp, Some(p("/c/X.emp")));
    // One board and one library: paired whatever their names.
    let r = pair_files(&[p("/d/board.emn"), p("/d/lib.emp")]).unwrap();
    assert_eq!(r.pairs[0].emp, Some(p("/d/lib.emp")));
    // Two boards and one library of another name: neither gets it.
    let r = pair_files(&[p("/d/a.emn"), p("/d/b.emn"), p("/d/lib.emp")]).unwrap();
    assert!(r.pairs.iter().all(|x| x.emp.is_none()));
    assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings);
    // Other files are skipped with a warning.
    let r = pair_files(&[p("/d/a.emn"), p("/d/readme.txt")]).unwrap();
    assert_eq!(r.pairs[0].emp, None);
    assert!(r.warnings.iter().any(|w| w.contains("readme.txt")));
    assert!(r.warnings.iter().any(|w| w.contains("placeholder")));
    // No board file: an error.
    assert!(pair_files(&[p("/d/a.emp")]).unwrap_err().contains(".emn"));
    assert!(pair_files(&[]).is_err());
}

#[test]
fn chosen_file_labels() {
    assert_eq!(chosen_label(&[]), "No file chosen");
    assert_eq!(chosen_label(&[p("/d/Vision PCB.emn")]), "Vision PCB.emn");
    assert_eq!(chosen_label(&[p("/d/a.emn"), p("/d/a.emp")]), "2 files chosen");
}

#[test]
fn imports_the_fixture_pairs() {
    let (emn, emp) = fixture("vision controller", "Vision PCB");
    let (emn2, emp2) = fixture("secondary board", "secondary board");
    let r = import_files(&[emp.clone(), emn.clone(), emn2, emp2]);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let got: Vec<&str> = r.boards.iter().map(|b| b.board.name()).collect();
    assert_eq!(got, ["Vision PCB", "secondary board"]);
    assert_eq!(r.boards[0].source, BoardSource::Idf { emn: "Vision PCB.emn".into(), emp: Some("Vision PCB.emp".into()) });
    assert_eq!(r.boards[0].board.board.placements.len(), 29);
    assert!(!r.boards[0].board.library.packages.is_empty());
}

#[test]
fn missing_emp_gives_placeholders_and_a_warning() {
    let (emn, _) = fixture("vision controller", "Vision PCB");
    let r = import_files(&[emn]);
    assert!(r.errors.is_empty());
    assert_eq!(r.boards.len(), 1);
    let b = &r.boards[0];
    assert!(b.board.library.packages.is_empty());
    assert_eq!(b.board.board.placements.len(), 29);
    assert_eq!(b.source, BoardSource::Idf { emn: "Vision PCB.emn".into(), emp: None });
    assert!(r.warnings.iter().any(|w| w.contains("No library file") && w.contains("placeholder")), "{:?}", r.warnings);
}

#[test]
fn parse_errors_name_the_file() {
    let dir = std::env::temp_dir().join(format!("cadrs-pcb-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("broken.emn");
    std::fs::write(&bad, ".HEADER\nnot an idf file\n").unwrap();
    let r = import_files(std::slice::from_ref(&bad));
    assert!(r.boards.is_empty());
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].starts_with("broken.emn: .emn"), "{:?}", r.errors);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn empty_board_name_falls_back_to_the_file_stem() {
    let (emn, emp) = fixture("vision controller", "Vision PCB");
    let text = std::fs::read_to_string(emn).unwrap().replace("\"Vision PCB\"", "\"\"");
    let r = read_board(&text, Some(&std::fs::read_to_string(emp).unwrap()), "My board").unwrap();
    assert_eq!(r.board.name(), "My board");
}

// ---------------------------------------------------------------------------------------------
// Stable item ids (P3H.2 judge: keep-area ids per area, not by flattened order)

#[test]
fn keep_ids_are_stable_per_area() {
    let (emn, emp) = fixture("v2 sample", "v2 sample");
    let mut b = PcbBoard::read(&std::fs::read_to_string(emn).unwrap(), &std::fs::read_to_string(emp).unwrap()).unwrap();
    let before: Vec<(KeepKind, usize, ItemId)> = b.keep_areas().iter().map(|k| (k.kind, k.index, k.id)).collect();
    assert!(before.len() >= 6);
    let mut ids: Vec<ItemId> = before.iter().map(|x| x.2).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), before.len(), "unique ids");
    // Removing the place keep-out keeps every other area's id.
    let ko = b.keep_id(KeepKind::PlaceKeepout, 0).unwrap();
    assert!(b.remove_keep_area(ko));
    let after: Vec<(KeepKind, ItemId)> = b.keep_areas().iter().map(|k| (k.kind, k.id)).collect();
    let want: Vec<(KeepKind, ItemId)> = before.iter().filter(|x| x.2 != ko).map(|x| (x.0, x.2)).collect();
    assert_eq!(after, want);
    // A new route keep-out gets a fresh id; the place region's id doesn't move.
    let region = b.keep_id(KeepKind::PlaceRegion, 0).unwrap();
    let rk = b.board.route_keepouts[0].clone();
    b.board.route_keepouts.push(rk);
    b.fill_ids();
    assert_eq!(b.keep_id(KeepKind::PlaceRegion, 0), Some(region));
    let new = b.keep_id(KeepKind::RouteKeepout, 1).unwrap();
    assert!(!ids.contains(&new));
    assert_eq!(b.keep_position(new), Some((KeepKind::RouteKeepout, 1)));
    // Components likewise.
    let c: Vec<ItemId> = b.component_ids.clone();
    assert!(b.remove_component(c[0]));
    assert_eq!(b.component_ids, c[1..]);
}

mod p3h4;

#[test]
fn plus_creates_named_boards_and_components() {
    let (mut d, mut h, id) = doc_with_studio();
    h.execute(&mut d, &AddBoard::new_board(id)).unwrap();
    h.execute(&mut d, &AddBoard::new_board(id)).unwrap();
    assert_eq!(names(studio(&d, id)), ["Board 1", "Board 2"]);
    assert_eq!(h.undo_label(), Some("Create board"));
    let s = studio(&d, id);
    let b = &s.boards[1];
    assert_eq!(s.active, Some(b.id));
    assert!(matches!(b.source, BoardSource::Native { imported_from: None }));
    // Its mechanical board: the 100 × 80 mm outline, 1.6 mm thick, no parts.
    let o = b.board.board.outline.as_ref().unwrap();
    assert!((o.loops[0].area() - 8000.0).abs() < 1e-9);
    assert_eq!(b.board.thickness(), 1.6);
    assert!(b.design.is_some());

    // Renamed (as the inline edit after + does); names stay unique.
    let b1 = s.boards[0].id;
    h.execute(&mut d, &RenameBoard { element: id, board: b1, name: " Power monitor ".into() }).unwrap();
    assert_eq!(names(studio(&d, id)), ["Power monitor", "Board 2"]);
    let b2 = studio(&d, id).boards[1].id;
    assert!(h.execute(&mut d, &RenameBoard { element: id, board: b2, name: "Power monitor".into() }).is_err());
    assert!(h.execute(&mut d, &RenameBoard { element: id, board: b2, name: "  ".into() }).is_err());
    // The next unnamed board takes the first free number.
    h.execute(&mut d, &AddBoard::new_board(id)).unwrap();
    assert_eq!(names(studio(&d, id)), ["Power monitor", "Board 2", "Board 1"]);

    h.execute(&mut d, &AddComponent { element: id, name: None }).unwrap();
    h.execute(&mut d, &AddComponent { element: id, name: Some("RA-01SH".into()) }).unwrap();
    h.execute(&mut d, &AddComponent { element: id, name: Some("RA-01SH".into()) }).unwrap();
    let comps = |d: &Document| studio(d, id).components.iter().map(|c| c.component.name.clone()).collect::<Vec<_>>();
    assert_eq!(comps(&d), ["Component 1", "RA-01SH", "RA-01SH (1)"]);
    let c0 = studio(&d, id).components[0].id;
    h.execute(&mut d, &RenameComponent { element: id, component: c0, name: "LoRa module".into() }).unwrap();
    assert_eq!(comps(&d)[0], "LoRa module");

    // Saved and loaded.
    let back: Document = ron::from_str(&ron::to_string(&d).unwrap()).unwrap();
    assert_eq!(studio(&back, id), studio(&d, id));

    // Undo takes them away again (four component steps, then the third board).
    for _ in 0..5 {
        h.undo(&mut d);
    }
    assert!(studio(&d, id).components.is_empty());
    assert_eq!(names(studio(&d, id)), ["Power monitor", "Board 2"]);
}

#[test]
fn designs_and_components_edit_through_undo() {
    use cadrs_eda::library::LibraryTable;
    let (mut d, mut h, id) = doc_with_studio();
    h.execute(&mut d, &AddBoard::new_board(id)).unwrap();
    let board = studio(&d, id).boards[0].id;
    // The course's board replaces the empty one in one step; the 3D board follows.
    let lib = LibraryTable::builtin();
    let course = cadrs_eda::getting_started::gs18(&lib);
    h.execute(&mut d, &SetDesign { element: id, board, design: Box::new(course.clone()), label: "Fill zones".into() }).unwrap();
    assert_eq!(h.undo_label(), Some("Fill zones"));
    let b = studio(&d, id).board(board).unwrap();
    assert_eq!(b.design.as_deref(), Some(&course));
    assert_eq!(b.board.board.placements.len(), 3);
    assert_eq!(b.name(), "Board 1");
    h.undo(&mut d);
    assert_eq!(studio(&d, id).board(board).unwrap().board.board.placements.len(), 0);
    // An imported IDF board has no design to edit.
    import(&mut d, &mut h, id, vision());
    let idf = studio(&d, id).boards[1].id;
    assert!(h.execute(&mut d, &SetDesign { element: id, board: idf, design: Box::new(course), label: "x".into() }).is_err());
    // Components: the symbol editor's result, name kept.
    h.execute(&mut d, &AddComponent { element: id, name: Some("Switch".into()) }).unwrap();
    let c = studio(&d, id).components[0].id;
    let mut value = cadrs_eda::Component::new("ignored");
    value.symbol = Some(cadrs_eda::getting_started::switch_symbol());
    value.footprint = Some(cadrs_eda::getting_started::switch_footprint());
    h.execute(&mut d, &SetComponent { element: id, component: c, value: Box::new(value), label: "Edit symbol".into() }).unwrap();
    let got = &studio(&d, id).component(c).unwrap().component;
    assert_eq!(got.name, "Switch");
    assert_eq!(got.symbol.as_ref().unwrap().pins.len(), 2);
    // Saved and loaded.
    let back: Document = ron::from_str(&ron::to_string(&d).unwrap()).unwrap();
    assert_eq!(studio(&back, id), studio(&d, id));
}
