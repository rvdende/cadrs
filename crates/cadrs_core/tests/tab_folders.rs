//! P3E.2: tab folders and reordering (`test-drive-gaps.md` P3E.2 "Done when"; TD5.3, TD5.4, X6;
//! essential tips T1.2, X1).
//!
//! - Creating, renaming, deleting a folder and moving tabs in, out and around are each **one
//!   undo step**, and redo brings them back.
//! - Folders and the order **survive a save and reload**; a document without folders is saved
//!   without a tree (older files load unchanged).
//! - **Element ids are kept**: every tab keeps its id and contents.
//! - **References between tabs still resolve** after tabs move into folders or are reordered:
//!   assembly instances (the Hexapod), a drawing's views (the Bar drawing), a Derived feature and
//!   its linked copy (of "Block source" V1). Checked by the reference lists and by volumes
//!   computed here from the dimensions.
#![cfg(feature = "occt")]

use std::f64::consts::PI;

use cadrs_core::command::Command;
use cadrs_core::derived::{self, AddDerived, DerivedFeature};
use cadrs_core::document::Element;
use cadrs_core::external::{Resolver, SourceRef};
use cadrs_core::history_log::HistoryLog;
use cadrs_core::link_update;
use cadrs_core::samples::{drawing_bar, linked_block as lb, piston as ps};
use cadrs_core::tab_tree::{self, CreateTabFolder, DeleteTabFolder, MoveTabItems, RenameTabFolder, TabItem};
use cadrs_core::{Document, DocumentMeta, ElementId, FeatureId, History, Store};

const HOST: ElementId = ElementId::from_u128(0x3e20_0000_0000_0000_0000_0000_0000_0001);
const DERIVED: FeatureId = FeatureId::from_u128(0x3e20_0000_0000_0000_0000_0000_0000_0002);
const F1: ElementId = ElementId::from_u128(0x3e20_0000_0000_0000_0000_0000_0000_0011);
const F2: ElementId = ElementId::from_u128(0x3e20_0000_0000_0000_0000_0000_0000_0012);

fn temp_store(tag: &str) -> Store {
    Store::new(std::env::temp_dir().join(format!("cadrs-tab-folders-{tag}-{}-{}", std::process::id(), uuid::Uuid::new_v4())))
}

fn run(doc: &mut Document, h: &mut History, c: &dyn Command) {
    h.execute(doc, c).unwrap_or_else(|e| panic!("{}: {e}", c.label()));
}

fn ids(doc: &Document) -> Vec<ElementId> {
    doc.elements.iter().map(|e| e.id).collect()
}

fn names(doc: &Document) -> Vec<String> {
    doc.elements.iter().map(|e| e.name.clone()).collect()
}

/// A document of `n` empty Part Studios "Tab 1" … "Tab n".
fn plain(n: usize) -> Document {
    let mut d = Document::empty("Tabs");
    for k in 0..n {
        d.elements.push(Element::part_studio(format!("Tab {}", k + 1)));
    }
    d
}

/// Every element's contents by id, to compare before and after (order-free).
fn contents(doc: &Document) -> Vec<(ElementId, Element)> {
    let mut v: Vec<(ElementId, Element)> = doc.elements.iter().map(|e| (e.id, e.clone())).collect();
    v.sort_by_key(|x| x.0);
    v
}

#[test]
fn each_folder_operation_is_one_undo_step() {
    let mut doc = plain(5);
    let t = ids(&doc);
    let start = doc.clone();
    let mut h = History::default();
    // Create a folder holding Tab 2 and Tab 3: one step.
    run(&mut doc, &mut h, &CreateTabFolder { id: F1, name: Some("Plates".into()), parent: None, before: None, items: vec![TabItem::Tab(t[1]), TabItem::Tab(t[2])] });
    assert_eq!(h.undo_len(), 1);
    let l = tab_tree::layout(&doc);
    assert_eq!(tab_tree::tabs_in(&l, F1), vec![t[1], t[2]]);
    assert_eq!(l.len(), 4, "Tab 1, Plates, Tab 4, Tab 5");
    assert_eq!(l[1].item(), TabItem::Folder(F1), "the folder takes its first tab's place");
    let after_create = doc.clone();
    // Rename: one step.
    run(&mut doc, &mut h, &RenameTabFolder { id: F1, name: "Brackets".into() });
    assert_eq!(doc.tab_tree.folder(F1).unwrap().name, "Brackets");
    // Drag Tab 5 into the folder, before Tab 3: one step.
    run(&mut doc, &mut h, &MoveTabItems { items: vec![TabItem::Tab(t[4])], parent: Some(F1), before: Some(TabItem::Tab(t[2])), label: "Move Tab 5".into() });
    assert_eq!(tab_tree::tabs_in(&tab_tree::layout(&doc), F1), vec![t[1], t[4], t[2]]);
    assert_eq!(names(&doc), ["Tab 1", "Tab 2", "Tab 5", "Tab 3", "Tab 4"], "the elements follow the tree's order");
    // Drag Tab 2 out, to the front of the top level: one step.
    run(&mut doc, &mut h, &MoveTabItems { items: vec![TabItem::Tab(t[1])], parent: None, before: Some(TabItem::Tab(t[0])), label: "Move Tab 2".into() });
    assert_eq!(tab_tree::layout(&doc)[0].item(), TabItem::Tab(t[1]));
    // A folder inside the folder, then the outer folder deleted keeping its tabs: one step each.
    run(&mut doc, &mut h, &CreateTabFolder { id: F2, name: None, parent: Some(F1), before: None, items: vec![TabItem::Tab(t[2])] });
    assert_eq!(doc.tab_tree.folder(F2).unwrap().name, "Folder 1");
    run(&mut doc, &mut h, &DeleteTabFolder { id: F1, delete_tabs: false });
    let l = tab_tree::layout(&doc);
    assert!(doc.tab_tree.folder(F1).is_none());
    assert_eq!(tab_tree::parent_of(&l, TabItem::Folder(F2)), None, "the subfolder moved up to the top level");
    assert_eq!(doc.elements.len(), 5, "no tab deleted");
    // Delete the subfolder with its tab: one step, and the tab is gone.
    run(&mut doc, &mut h, &DeleteTabFolder { id: F2, delete_tabs: true });
    assert_eq!(doc.elements.len(), 4);
    assert!(doc.element(t[2]).is_none());
    assert!(doc.tab_tree.is_empty(), "no folders left: the tree is empty again");
    assert_eq!(h.undo_len(), 7, "7 operations, 7 steps");
    // Undo one at a time back to the start; each undo is exactly one operation.
    h.undo(&mut doc).unwrap();
    assert_eq!(doc.elements.len(), 5, "undo brings the deleted tab back");
    assert!(doc.element(t[2]).is_some());
    for _ in 0..4 {
        h.undo(&mut doc).unwrap();
    }
    assert_eq!(doc, after_create.clone().tap_rename("Brackets"), "back to just after Rename");
    h.undo(&mut doc).unwrap();
    assert_eq!(doc, after_create);
    h.undo(&mut doc).unwrap();
    assert_eq!(doc, start, "undoing Create folder restores the document exactly");
    // Redo all.
    while h.redo(&mut doc).is_some() {}
    assert_eq!(doc.elements.len(), 4);
    assert!(doc.tab_tree.is_empty());
}

trait TapRename {
    fn tap_rename(self, name: &str) -> Self;
}

impl TapRename for Document {
    fn tap_rename(mut self, name: &str) -> Self {
        if let Some(f) = self.tab_tree.folders.iter_mut().find(|f| f.id == F1) {
            f.name = name.into();
        }
        self
    }
}

#[test]
fn a_folder_cannot_go_inside_itself_and_the_last_tab_stays() {
    let mut doc = plain(2);
    let t = ids(&doc);
    let mut h = History::default();
    run(&mut doc, &mut h, &CreateTabFolder { id: F1, name: None, parent: None, before: None, items: vec![TabItem::Tab(t[0]), TabItem::Tab(t[1])] });
    run(&mut doc, &mut h, &CreateTabFolder { id: F2, name: None, parent: Some(F1), before: None, items: vec![] });
    let before = doc.clone();
    assert!(h.execute(&mut doc, &MoveTabItems { items: vec![TabItem::Folder(F1)], parent: Some(F2), before: None, label: "x".into() }).is_err());
    assert!(h.execute(&mut doc, &DeleteTabFolder { id: F1, delete_tabs: true }).is_err(), "it holds every tab");
    assert_eq!(doc, before, "a refused command changes nothing");
    assert_eq!(h.undo_len(), 2);
}

#[test]
fn folders_and_order_survive_a_reload_and_keep_element_ids() {
    let store = temp_store("reload");
    let mut doc = plain(6);
    let meta = DocumentMeta::new("me", 1_000);
    store.create(&doc, &meta).unwrap();
    let before = contents(&doc);
    let t = ids(&doc);
    let mut h = History::default();
    run(&mut doc, &mut h, &CreateTabFolder { id: F1, name: Some("Drawings".into()), parent: None, before: None, items: vec![TabItem::Tab(t[3]), TabItem::Tab(t[5])] });
    run(&mut doc, &mut h, &CreateTabFolder { id: F2, name: Some("Empty".into()), parent: None, before: Some(TabItem::Tab(t[0])), items: vec![] });
    run(&mut doc, &mut h, &MoveTabItems { items: vec![TabItem::Tab(t[2])], parent: None, before: Some(TabItem::Tab(t[1])), label: "Reorder".into() });
    store.save(&doc, &meta).unwrap();
    let loaded = store.load(doc.id).unwrap().document;
    assert_eq!(loaded, doc, "the same document after a reload");
    let l = tab_tree::layout(&loaded);
    let top: Vec<TabItem> = l.iter().map(|n| n.item()).collect();
    assert_eq!(top, [TabItem::Folder(F2), TabItem::Tab(t[0]), TabItem::Tab(t[2]), TabItem::Tab(t[1]), TabItem::Folder(F1), TabItem::Tab(t[4])]);
    assert_eq!(tab_tree::tabs_in(&l, F1), vec![t[3], t[5]]);
    assert_eq!(contents(&loaded), before, "every element keeps its id and contents");
}

#[test]
fn a_document_without_folders_saves_no_tree() {
    let store = temp_store("plain");
    let mut doc = plain(3);
    let meta = DocumentMeta::new("me", 1_000);
    store.create(&doc, &meta).unwrap();
    let t = ids(&doc);
    // A reorder without folders is just the element order.
    let mut h = History::default();
    run(&mut doc, &mut h, &MoveTabItems { items: vec![TabItem::Tab(t[2])], parent: None, before: Some(TabItem::Tab(t[0])), label: "Reorder".into() });
    assert!(doc.tab_tree.is_empty());
    assert_eq!(names(&doc), ["Tab 3", "Tab 1", "Tab 2"]);
    store.save(&doc, &meta).unwrap();
    let dir = std::fs::read_dir(store.root()).unwrap();
    let mut text = String::new();
    for e in dir.flatten() {
        let p = e.path().join("document.ron");
        if let Ok(s) = std::fs::read_to_string(&p) {
            text = s;
        }
    }
    assert!(!text.is_empty(), "the saved document");
    assert!(!text.contains("tab_tree"), "no tree is written for a document without folders");
    assert_eq!(store.load(doc.id).unwrap().document, doc);
}

// ---------------------------------------------------------------------------------------------
// References between tabs

/// The Bar and its drawing, the Hexapod exercise's five tabs, and a Part Studio "Derived host"
/// with a Derived feature of "Block source" V1 (its linked copy under `Document::linked`).
fn mixed(store: &Store) -> Document {
    let (mut doc, _, _) = drawing_bar::document().unwrap();
    doc.elements.extend(ps::move_to_document().unwrap().elements);
    let mut host = Element::part_studio("Derived host");
    host.id = HOST;
    doc.elements.push(host);
    // "Block source" V1.
    let a = lb::document().unwrap();
    store.create(&a, &DocumentMeta::new("me", 1_000)).unwrap();
    let mut log = HistoryLog::start(&a, 1_000, "me");
    let v = log.create_version("", "", 1_001, "me");
    log.save(store).unwrap();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v);
    let got = derived::resolve(&mut res, &doc, None, DerivedFeature::default(), r).unwrap();
    let mut h = History::default();
    run(&mut doc, &mut h, &AddDerived { element: HOST, feature: DERIVED, derived: got.derived, links: got.links });
    doc
}

fn hexapod_volume(doc: &Document) -> f64 {
    let model = doc.element(ps::HEXAPOD).unwrap().assembly_model().unwrap();
    let (parts, _) = cadrs_core::assembly::instance_parts(doc, model, |e| doc.element(e).map(|el| cadrs_core::rebuild::build(el.features())));
    parts.iter().map(|p| p.mass.as_ref().expect("a solid part").volume).sum()
}

fn derived_volume(doc: &Document) -> f64 {
    let b = cadrs_core::rebuild::build(doc.element(HOST).unwrap().features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let parts: Vec<&cadrs_core::Part> = b.parts.iter().filter(|p| p.feature == DERIVED).collect();
    assert_eq!(parts.len(), 1);
    cadrs_core::parts::combined_mass(parts).unwrap().volume
}

/// Every reference, order-free: (site, source element) of the linked and the workspace uses,
/// and each drawing's source elements.
fn references(doc: &Document) -> Vec<String> {
    let mut v: Vec<String> = link_update::uses(doc)
        .into_iter()
        .chain(link_update::workspace_uses(doc, None))
        .map(|u| format!("{:?} -> {}", u.site, u.source))
        .collect();
    for el in &doc.elements {
        if let Some(d) = el.drawing_data() {
            for s in &d.sources {
                v.push(format!("drawing {} -> {}", el.id, s.element));
            }
        }
    }
    v.sort();
    v
}

#[track_caller]
fn all_resolve(doc: &Document) {
    for u in link_update::uses(doc).into_iter().chain(link_update::workspace_uses(doc, None)) {
        assert!(doc.element(u.source).is_some(), "{:?} resolves", u.site);
    }
    for el in &doc.elements {
        if let Some(d) = el.drawing_data() {
            for s in &d.sources {
                assert!(doc.element(ElementId(s.element)).is_some(), "the drawing's source resolves");
            }
        }
    }
}

#[test]
fn references_between_tabs_resolve_after_folders_and_reordering() {
    let store = temp_store("refs");
    let mut doc = mixed(&store);
    let refs0 = references(&doc);
    assert!(refs0.iter().any(|r| r.contains("Instance")), "assembly instances");
    assert!(refs0.iter().any(|r| r.contains("Drawing") || r.starts_with("drawing")), "a drawing");
    assert!(refs0.iter().any(|r| r.contains("Derived")), "a derived feature");
    assert!(!doc.linked.is_empty(), "a linked copy");
    all_resolve(&doc);
    // Volumes from the dimensions: the Hexapod (see tests/move_document.rs) and the block.
    let p: f64 = 1000.0 + 60.0 * PI * (15.725f64 / 2.0).powi(2) + PI * 9.0 * 28.0 + 512.0;
    let hex = PI * (100.0f64.powi(2) - 6.0 * 25.0) * 10.0 + PI * (80.0f64.powi(2) - 6.0 * 25.0) * 10.0 + 6.0 * p;
    let block = lb::LENGTH * lb::WIDTH * lb::HEIGHT;
    let (v_hex, v_block) = (hexapod_volume(&doc), derived_volume(&doc));
    assert!((v_hex - hex).abs() < 1e-3, "Hexapod {v_hex} vs {hex}");
    assert!((v_block - block).abs() < 1e-6 * block, "Derived {v_block} vs {block}");
    let before = contents(&doc);
    // Folders: "Assemblies" (Hexapod, Piston Assembly), "Studios" inside it (the piston's
    // studio), the drawing reordered to the front, the host into a new folder at the end.
    let t = ids(&doc);
    let drawing = doc.elements.iter().find(|e| e.drawing_data().is_some()).unwrap().id;
    let mut h = History::default();
    run(&mut doc, &mut h, &CreateTabFolder { id: F1, name: Some("Assemblies".into()), parent: None, before: None, items: vec![TabItem::Tab(ps::HEXAPOD), TabItem::Tab(ps::PISTON_ASSEMBLY)] });
    run(&mut doc, &mut h, &CreateTabFolder { id: F2, name: Some("Studios".into()), parent: Some(F1), before: None, items: vec![] });
    run(&mut doc, &mut h, &MoveTabItems { items: vec![TabItem::Tab(ps::PISTON_STUDIO)], parent: Some(F2), before: None, label: "Move".into() });
    run(&mut doc, &mut h, &MoveTabItems { items: vec![TabItem::Tab(drawing)], parent: None, before: Some(TabItem::Tab(t[0])), label: "Reorder".into() });
    run(&mut doc, &mut h, &CreateTabFolder { id: ElementId::new(), name: Some("Derived".into()), parent: None, before: None, items: vec![TabItem::Tab(HOST)] });
    assert_eq!(doc.elements[0].id, drawing, "the drawing moved to the front");
    assert_ne!(ids(&doc), t, "the order changed");
    let check = |doc: &Document| {
        assert_eq!(references(doc), refs0, "the same references");
        all_resolve(doc);
        assert_eq!(contents(doc), before, "every element keeps its id and contents");
        assert!((hexapod_volume(doc) - hex).abs() < 1e-3, "the Hexapod's instances still resolve");
        assert!((derived_volume(doc) - block).abs() < 1e-6 * block, "the Derived feature and its linked copy still resolve");
    };
    check(&doc);
    // And after a reload.
    let meta = DocumentMeta::new("me", 2_000);
    store.create(&doc, &meta).unwrap();
    let loaded = store.load(doc.id).unwrap().document;
    assert_eq!(loaded, doc);
    check(&loaded);
    // Undo everything: the original order, the same references.
    while h.undo(&mut doc).is_some() {}
    assert_eq!(ids(&doc), t);
    assert!(doc.tab_tree.is_empty());
    check(&doc);
}
