//! P3H.4: BOM grouping, designator edits, search, the component library's mappings and the
//! workspace-level settings.

use super::*;
use crate::pcb::bom::{bom, designator_list};
use crate::pcb::library::{LibrarySync, PcbWorkspace, create_library_document, library_of, load_library};
use crate::pcb::search::{Hit, SearchState, search};

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn designator_lists_use_ranges_for_runs_of_three() {
    assert_eq!(designator_list(&s(&["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10"])), "C1-C10");
    assert_eq!(designator_list(&s(&["J1", "J2"])), "J1, J2");
    assert_eq!(designator_list(&s(&["R1", "R2", "R3", "R7"])), "R1-R3, R7");
    assert_eq!(designator_list(&s(&["U1"])), "U1");
    assert_eq!(designator_list(&s(&["R1", "R3", "R5"])), "R1, R3, R5");
    assert_eq!(designator_list(&s(&["TP", "X1", "X2", "X3"])), "TP, X1-X3");
    assert_eq!(designator_list(&[]), "");
}

#[test]
fn bom_groups_by_part_number() {
    let b = vision();
    let rows = bom(&b);
    let got: Vec<(String, &str, &str, usize)> = rows.iter().map(|r| (r.designators(), r.package.as_str(), r.part_number.as_str(), r.quantity())).collect();
    let want: Vec<(String, &str, &str, usize)> = vec![
        ("C1-C10".into(), "0603C", "CC0603-100N", 10),
        ("D1-D3".into(), "SOD123F", "DSS16", 3),
        ("J1, J2".into(), "HDR_1X20", "HDR-1X20-254", 2),
        ("Q1".into(), "SOT23", "MMBT3904", 1),
        ("R1-R10".into(), "0603R", "RC0603-10K", 10),
        ("U1".into(), "QFP100_600MIL", "VPU-7100", 1),
        ("U2".into(), "QFN64_400MIL", "MCU-4064", 1),
        ("Y1".into(), "CRYSTAL_HC49", "XTAL-16M", 1),
    ];
    assert_eq!(got, want);
    assert_eq!(rows.iter().map(|r| r.quantity()).sum::<usize>(), 29);
    // Each row's items are its components, in designator order.
    let r = &rows[4];
    let refs: Vec<&str> = r.items.iter().map(|i| b.component(*i).unwrap().refdes.as_str()).collect();
    assert_eq!(refs, ["R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10"]);
}

fn item_of(d: &Document, id: ElementId, refdes: &str) -> (BoardId, ItemId) {
    let st = studio(d, id);
    let b = st.active_board().unwrap();
    (b.id, b.board.components().find(|(_, p)| p.refdes == refdes).unwrap().0)
}

#[test]
fn refdes_edit_is_undoable_and_rejects_duplicates() {
    let (mut d, mut h, id) = doc_with_studio();
    import(&mut d, &mut h, id, vision());
    let (board, u1) = item_of(&d, id, "U1");
    h.execute(&mut d, &SetRefdes { element: id, board, item: u1, refdes: " U10 ".into() }).unwrap();
    let refdes = |d: &Document| studio(d, id).active_board().unwrap().board.component(u1).unwrap().refdes.clone();
    assert_eq!(refdes(&d), "U10");
    assert_eq!(h.undo_label(), Some("Rename U10"));
    // The BOM follows.
    assert!(bom(&studio(&d, id).active_board().unwrap().board).iter().any(|r| r.designators() == "U10"));
    h.undo(&mut d);
    assert_eq!(refdes(&d), "U1");
    h.redo(&mut d);
    assert_eq!(refdes(&d), "U10");
    // Duplicates (any case), empty and spaced designators are refused and change nothing.
    let steps = h.undo_len();
    for bad in ["U2", "u2", "R1", "", "  ", "U 3"] {
        let e = h.execute(&mut d, &SetRefdes { element: id, board, item: u1, refdes: bad.into() });
        assert!(e.is_err(), "{bad:?} was accepted");
    }
    assert_eq!(h.undo_len(), steps);
    assert_eq!(refdes(&d), "U10");
    let e = h.execute(&mut d, &SetRefdes { element: id, board, item: u1, refdes: "R1".into() }).unwrap_err();
    assert_eq!(e.to_string(), "R1 is already used on this board");
    // Its own designator in another case is fine.
    h.execute(&mut d, &SetRefdes { element: id, board, item: u1, refdes: "u10".into() }).unwrap();
    assert_eq!(refdes(&d), "u10");
}

#[test]
fn search_matches_and_steps() {
    let (mut d, mut h, id) = doc_with_studio();
    import(&mut d, &mut h, id, vision());
    import(&mut d, &mut h, id, secondary());
    let vision_id = studio(&d, id).boards[0].id;
    let st = studio(&d, id);
    let active = Some(vision_id);
    let refdes = |h: &Hit| match h {
        Hit::Board(b) => st.board(*b).unwrap().name().to_string(),
        Hit::Component(b, i) => st.board(*b).unwrap().board.component(*i).unwrap().refdes.clone(),
    };
    // "R" on the Vision PCB: the designators R1–R10, HDR_1X20 (J1, J2) and CRYSTAL_HC49 (Y1),
    // in designator order; the secondary board's hits come after.
    let hits = search(st, active, "r");
    // The shown board comes first even when another was imported last.
    assert_eq!(st.active, Some(st.boards[1].id));
    let vision_hits: Vec<String> = hits.iter().filter(|h| h.board() == vision_id).map(refdes).collect();
    assert_eq!(vision_hits, ["J1", "J2", "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10", "Y1"]);
    assert!(hits[..13].iter().all(|h| h.board() == vision_id));
    // Part numbers and packages match too; board names give board hits.
    assert_eq!(search(st, active, "vpu-7100").iter().map(refdes).collect::<Vec<_>>(), ["U1"]);
    assert_eq!(search(st, active, "qfn64").iter().map(refdes).collect::<Vec<_>>(), ["U2"]);
    let sec = search(st, active, "secondary");
    assert_eq!(sec.len(), 1);
    assert!(matches!(sec[0], Hit::Board(_)));
    assert!(search(st, active, "  ").is_empty());
    assert!(search(st, active, "zzz").is_empty());
    // Stepping wraps both ways; the counter says where we are.
    let mut state = SearchState::run(st, active, "U");
    let u: Vec<String> = state.hits.iter().filter(|h| h.board() == vision_id).map(refdes).collect();
    assert_eq!(u[..2], ["U1".to_string(), "U2".to_string()]);
    let m = state.hits.len();
    assert_eq!(state.counter(), format!("1 of {m}"));
    state.step_down();
    assert_eq!(state.counter(), format!("2 of {m}"));
    state.step_up();
    state.step_up();
    assert_eq!(state.counter(), format!("{m} of {m}"));
    state.step_down();
    assert_eq!(state.current, 0);
    let empty = SearchState::run(st, active, "zzz");
    assert_eq!(empty.counter(), "No results");
    assert!(empty.is_active());
}

fn custom(part: &str, t: PartTransform) -> Representation {
    Representation::Custom(Box::new(CustomPart {
        source: PartSource {
            document: crate::DocumentId::from_u128(42),
            document_name: "QFP model".into(),
            element: ElementId::from_u128(43),
            element_name: "Part Studio 1".into(),
            part: crate::PartId::new(crate::FeatureId::from_u128(44), 0),
            part_name: part.into(),
            version: None,
            version_name: None,
        },
        transform: t,
    }))
}

#[test]
fn representation_mapping_round_trips() {
    let (mut d, mut h, id) = doc_with_studio();
    let pkg = "QFP100_600MIL";
    assert_eq!(*studio(&d, id).library.get(pkg), Representation::FromEcad);
    h.execute(&mut d, &SetRepresentation { element: id, package: pkg.into(), representation: Representation::None, label: String::new() }).unwrap();
    assert_eq!(*studio(&d, id).library.get(pkg), Representation::None);
    assert_eq!(h.undo_label(), Some("Set representation of QFP100_600MIL"));
    let t = PartTransform { translate: [1.0, 2.0, 0.5], rotate: [0.0, 0.0, 90.0] };
    h.execute(&mut d, &SetRepresentation { element: id, package: pkg.into(), representation: custom("Chip", t), label: "Custom part".into() }).unwrap();
    // Saved and loaded with the document.
    let back: Document = ron::from_str(&ron::to_string(&d).unwrap()).unwrap();
    assert_eq!(back, d);
    assert_eq!(studio(&back, id).library.get(pkg).custom().unwrap().transform, t);
    // Undo walks back; From ECAD data leaves no entry.
    h.undo(&mut d);
    assert_eq!(*studio(&d, id).library.get(pkg), Representation::None);
    h.undo(&mut d);
    assert!(studio(&d, id).library.mappings.is_empty());
    h.redo(&mut d);
    h.execute(&mut d, &SetRepresentation { element: id, package: pkg.into(), representation: Representation::FromEcad, label: String::new() }).unwrap();
    assert!(studio(&d, id).library.mappings.is_empty());
}

#[test]
fn transform_rotates_then_translates() {
    // Rotate 90° about Z, then move by (1, 2, 0): (1, 0, 0) → (0, 1, 0) → (1, 3, 0).
    let t = PartTransform { translate: [1.0, 2.0, 0.0], rotate: [0.0, 0.0, 90.0] };
    let q = t.motion().point(&nalgebra::Point3::new(1.0, 0.0, 0.0));
    assert!((q - nalgebra::Point3::new(1.0, 3.0, 0.0)).norm() < 1e-12, "{q}");
    // X first, then Y, then Z: (0, 1, 0) about X 90° → (0, 0, 1); about Z 90° it stays.
    let t = PartTransform { translate: [0.0; 3], rotate: [90.0, 0.0, 90.0] };
    let q = t.motion().point(&nalgebra::Point3::new(0.0, 1.0, 0.0));
    assert!((q - nalgebra::Point3::new(0.0, 0.0, 1.0)).norm() < 1e-12, "{q}");
}

#[test]
fn center_puts_the_part_at_the_origin() {
    // A 10 × 4 × 2 box with a corner at (3, 5, 7), turned 90° about Z: its footprint is
    // x ∈ [−9, −5], y ∈ [3, 13], z ∈ [7, 9]. Center moves it to x ∈ [−2, 2], y ∈ [−5, 5],
    // z ∈ [0, 2]: translate (7, −8, −7), rotation kept.
    let mut pts = Vec::new();
    for x in [3.0, 13.0] {
        for y in [5.0, 9.0] {
            for z in [7.0, 9.0] {
                pts.push([x, y, z]);
            }
        }
    }
    let t = PartTransform { translate: [40.0, 40.0, 40.0], rotate: [0.0, 0.0, 90.0] }.centered(&pts);
    assert_eq!(t.rotate, [0.0, 0.0, 90.0]);
    for (a, b) in t.translate.iter().zip([7.0, -8.0, -7.0]) {
        assert!((a - b).abs() < 1e-9, "{:?}", t.translate);
    }
    let m = t.motion();
    let placed: Vec<_> = pts.iter().map(|p| m.point(&nalgebra::Point3::new(p[0], p[1], p[2]))).collect();
    let min_z = placed.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    let cx = placed.iter().map(|p| p.x).sum::<f64>() / 8.0;
    let cy = placed.iter().map(|p| p.y).sum::<f64>() / 8.0;
    assert!(min_z.abs() < 1e-9 && cx.abs() < 1e-9 && cy.abs() < 1e-9);
}

#[test]
fn rotating_turns_the_part_in_place() {
    // Centred first, then turned 90° about Z: the box centre (8, 7, 8) stays at (0, 0, 1) and it
    // still stands on z = 0.
    let pts: Vec<[f64; 3]> = [[3.0, 5.0, 7.0], [13.0, 9.0, 9.0]].to_vec();
    let centred = PartTransform::default().centered(&pts);
    assert_eq!(centred.translate, [-8.0, -7.0, -7.0]);
    let t = centred.rotated_in_place([0.0, 0.0, 90.0], &pts);
    let c = t.motion().point(&nalgebra::Point3::new(8.0, 7.0, 8.0));
    assert!((c - nalgebra::Point3::new(0.0, 0.0, 1.0)).norm() < 1e-9, "{c}");
    // Equivalent to centring the turned part.
    let again = PartTransform { translate: [0.0; 3], rotate: [0.0, 0.0, 90.0] }.centered(&pts);
    for (a, b) in t.translate.iter().zip(again.translate) {
        assert!((a - b).abs() < 1e-9, "{t:?} vs {again:?}");
    }
}

fn temp_store(tag: &str) -> crate::Store {
    let root = std::env::temp_dir().join(format!("cadrs-pcb-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    crate::Store::new(root)
}

/// A stored document with one PCB Studio, its history and a sync of its own.
fn stored_doc(store: &crate::Store, name: &str) -> (Document, History, ElementId, LibrarySync) {
    let mut d = Document::new(name);
    let mut h = History::default();
    let id = ElementId::new();
    h.execute(&mut d, &AddElement { id, kind: NewElementKind::PcbStudio, name: None, after: None }).unwrap();
    store.save(&d, &crate::DocumentMeta::new("me", 0)).unwrap();
    (d, h, id, LibrarySync::default())
}

#[test]
fn workspace_settings_are_shared_by_every_pcb_studio() {
    let store = temp_store("workspace");
    let lib = create_library_document(&store, "Parts library", "me", 0).unwrap();
    let folder = FolderRef { id: crate::FolderId::from_u128(9), name: "PCB Components".into() };
    let settings = PcbSettings { library: Some(LibraryRef { document: lib.id, name: lib.name.clone() }), component_folder: Some(folder) };
    // Studio A: Update the settings (one undo step), then sync pushes them to the workspace.
    let (mut a, mut ha, ida, mut sa) = stored_doc(&store, "A");
    sa.sync(&mut a, &store);
    ha.execute(&mut a, &SetPcbSettings { element: ida, settings: settings.clone() }).unwrap();
    let out = sa.sync(&mut a, &store);
    assert!(out.pushed_settings && out.errors.is_empty(), "{out:?}");
    assert_eq!(PcbWorkspace::load(store.root()).settings, settings);
    // Studio B in another document, opened later, sees the same library and folder.
    let (mut b, mut hb, idb, mut sb) = stored_doc(&store, "B");
    assert_eq!(studio(&b, idb).settings, PcbSettings::default());
    let steps = hb.undo_len();
    let out = sb.sync(&mut b, &store);
    assert_eq!(out.pulled, [idb]);
    assert_eq!(studio(&b, idb).settings, settings);
    assert_eq!(hb.undo_len(), steps, "pulling is not an undo step");
    // A second studio in the same document too.
    let idb2 = ElementId::new();
    hb.execute(&mut b, &AddElement { id: idb2, kind: NewElementKind::PcbStudio, name: None, after: None }).unwrap();
    sb.sync(&mut b, &store);
    assert_eq!(studio(&b, idb2).settings, settings);
    // Undo in A goes back to the defaults, for everyone who opens a studio next.
    ha.undo(&mut a);
    sa.sync(&mut a, &store);
    assert_eq!(PcbWorkspace::load(store.root()).settings, PcbSettings::default());
    let mut fresh = LibrarySync::default();
    fresh.sync(&mut b, &store);
    assert_eq!(studio(&b, idb).settings, PcbSettings::default());
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn mappings_live_in_the_library_document() {
    let store = temp_store("library");
    let lib = create_library_document(&store, "Parts library", "me", 0).unwrap();
    let settings = PcbSettings { library: Some(LibraryRef { document: lib.id, name: lib.name.clone() }), component_folder: None };
    PcbWorkspace { version: 1, settings: settings.clone() }.save(store.root()).unwrap();
    let (mut a, mut ha, ida, mut sa) = stored_doc(&store, "A");
    let (mut b, _hb, idb, mut sb) = stored_doc(&store, "B");
    sa.sync(&mut a, &store);
    sb.sync(&mut b, &store);
    // A maps QFP100 to None: the library document holds it; B picks it up when it syncs anew.
    let pkg = "QFP100_600MIL";
    ha.execute(&mut a, &SetRepresentation { element: ida, package: pkg.into(), representation: Representation::None, label: String::new() }).unwrap();
    let out = sa.sync(&mut a, &store);
    assert!(out.pushed_library, "{out:?}");
    let stored = store.load(lib.id).unwrap().document;
    assert_eq!(*library_of(&stored).get(pkg), Representation::None);
    assert_eq!(stored.elements.len(), 1, "the library document keeps its one PCB Studio tab");
    sb.reset();
    sb.sync(&mut b, &store);
    assert_eq!(*studio(&b, idb).library.get(pkg), Representation::None);
    // Undo in A restores From ECAD data in the library.
    ha.undo(&mut a);
    sa.sync(&mut a, &store);
    assert_eq!(load_library(&store, &settings, &a).unwrap(), ComponentLibrary::default());
    // Without a library document the default library file holds the mappings.
    let (mut c, mut hc, idc, mut sc) = stored_doc(&store, "C");
    let none = PcbSettings::default();
    sc.sync(&mut c, &store);
    hc.execute(&mut c, &SetPcbSettings { element: idc, settings: none.clone() }).unwrap();
    sc.sync(&mut c, &store);
    hc.execute(&mut c, &SetRepresentation { element: idc, package: "SOT23".into(), representation: Representation::None, label: String::new() }).unwrap();
    sc.sync(&mut c, &store);
    assert!(store.root().join(crate::pcb::DEFAULT_LIBRARY_FILE).is_file());
    assert_eq!(*load_library(&store, &none, &c).unwrap().get("SOT23"), Representation::None);
    let _ = std::fs::remove_dir_all(store.root());
}
