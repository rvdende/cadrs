//! P3H.7: Create assembly with **component documents** (PCB7.3, PCB7.10, PCB9.5, PCB11.1–PCB11.3,
//! X7) on a temporary document store: one stored document per package in the component folder,
//! versioned, inserted as version-pinned references; reused by later Creates; edited, versioned
//! and updated; moved to another folder. The expected numbers are worked out by hand in each
//! test from the fixtures' `.emp` outlines and heights.
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::assembly::bom::{BomColumn, BomOptions, compute};
use cadrs_core::assembly::commands::{InsertInstance, MoveInstances};
use cadrs_core::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::command::History;
use cadrs_core::commands::{AddElement, NewElementKind, RenamePart, SetExtrude};
use cadrs_core::external::{RefAt, Resolver};
use cadrs_core::history_log::{HistoryLog, Origin, VersionId};
use cadrs_core::ids::{DocumentId, ElementId, FeatureId, PartId};
use cadrs_core::library::{Library, LibraryHistory, MoveToFolder};
use cadrs_core::link_update::{self as lu, Target, UpdateReferences};
use cadrs_core::pcb::component_docs::{ComponentKey, DEFAULT_COMPONENT_FOLDER, find_component_document};
use cadrs_core::pcb::{BoardSource, FolderRef, GeneratedAssembly, ImportBoard, SyncPlaneChoice};
use cadrs_core::properties::PropertyKey;
use cadrs_core::studio::DocHistory;
use cadrs_core::{Document, DocumentMeta, ElementKind, FolderEntry, FolderId, Store};
use cadrs_idf::IdfVersion;
use cadrs_pcb::PcbBoard;
use cadrs_pcb::component_docs::ComponentDocuments;
use cadrs_pcb::create_assembly::{CreateOptions, generate_linked};
use cadrs_pcb::sync::{plan, run_now};

const PCB: ElementId = ElementId::from_u128(0x3a07_9000);
const USER: &str = "me";

fn temp_store(tag: &str) -> Store {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    Store::new(std::env::temp_dir().join(format!("cadrs-pcb-compdocs-{tag}-{}-{n}", std::process::id())))
}

#[track_caller]
fn close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() < 1e-6 * want.abs().max(1.0), "{what}: got {got}, want {want}");
}

fn read(folder: &str, name: &str) -> (String, String) {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/idf").join(folder);
    (std::fs::read_to_string(dir.join(format!("{name}.emn"))).unwrap(), std::fs::read_to_string(dir.join(format!("{name}.emp"))).unwrap())
}

fn fixture(folder: &str, name: &str) -> PcbBoard {
    let (emn, emp) = read(folder, name);
    PcbBoard::read(&emn, &emp).unwrap()
}

/// The secondary board with X1 (CRYSTAL_CX_4V) placed as a new package "TEST_CAN 9.0X5.0"
/// (9 × 5 mm, 2 mm high): it shares the other six packages with the original.
fn secondary_with_a_new_package() -> PcbBoard {
    let (emn, mut emp) = read("secondary board", "secondary board");
    let i = emn.lines().position(|l| l.starts_with("CRYSTAL_CX_4V ")).unwrap();
    let emn: String = emn.lines().enumerate().map(|(k, l)| if k == i { "TEST_CAN_9.0X5.0 9905000 X1".to_string() } else { l.to_string() }).collect::<Vec<_>>().join("\n");
    emp.push_str(".ELECTRICAL\nTEST_CAN_9.0X5.0 9905000 MM 2\n0 -4.5 -2.5 0\n0 4.5 -2.5 0\n0 4.5 2.5 0\n0 -4.5 2.5 0\n0 -4.5 -2.5 0\nPROP DESCRIPTION \"Test can\"\n.END_ELECTRICAL\n");
    PcbBoard::read(&emn, &emp).unwrap()
}

/// A board document "<name> board" (stored, so Where used finds it) with a PCB Studio and `pcb`
/// imported.
fn board_document(store: &Store, name: &str, pcb: PcbBoard) -> (Document, History) {
    let mut doc = Document::new(format!("{name} board"));
    let mut h = History::default();
    h.execute(&mut doc, &AddElement { id: PCB, kind: NewElementKind::PcbStudio, name: None, after: None }).unwrap();
    h.execute(&mut doc, &ImportBoard { element: PCB, board: Box::new(pcb), source: BoardSource::Idf { emn: format!("{name}.emn"), emp: None } }).unwrap();
    store.create(&doc, &DocumentMeta::new(USER, 1_000)).unwrap();
    (doc, h)
}

/// Create assembly (defaults) with component documents in `folder`.
fn create(store: &Store, doc: &mut Document, h: &mut History, folder: Option<&FolderRef>, now: i64) -> (GeneratedAssembly, ComponentDocuments) {
    let board = doc.element(PCB).unwrap().pcb().unwrap().active.unwrap();
    let name = doc.element(PCB).unwrap().pcb().unwrap().board(board).unwrap().name().to_string();
    let mut docs = ComponentDocuments::new(store, folder, doc.id, &name, USER, now).unwrap();
    let cmd = generate_linked(doc, PCB, board, &CreateOptions::default(), &mut docs).unwrap();
    let g = cmd.generated.clone();
    h.execute(doc, &cmd).unwrap();
    store.save(doc, &store.load(doc.id).unwrap().meta).unwrap();
    (g, docs)
}

fn library(store: &Store) -> Library {
    store.list().0
}

fn build_of(doc: &Document) -> impl FnMut(ElementId) -> Option<Arc<cadrs_core::rebuild::Build>> + '_ {
    |e| doc.element(e).map(|el| cadrs_core::rebuild::build(&el.active_features()))
}

/// The volume of the instance of designator `refdes` in the generated assembly.
fn volume_of(doc: &Document, g: &GeneratedAssembly, refdes: &str) -> f64 {
    let i = g.components.iter().find(|c| c.refdes == refdes).unwrap().instance;
    let model = doc.element(g.assembly).unwrap().assembly_model().unwrap();
    let (parts, _) = assembly::instance_parts(doc, model, build_of(doc));
    parts.iter().filter(|p| InstanceId::of_part(p.id) == i).map(|p| p.mass.as_ref().map(|m| m.volume).unwrap_or(0.0)).sum()
}

/// Sets the height of the component document of `package` (its workspace) and makes the next
/// version, as a user editing it in its own document and pressing Create version.
fn edit_height(store: &Store, package: &str, part_number: &str, height: f64, t: i64) -> VersionId {
    let (id, key) = find_component_document(&library(store), package, part_number).unwrap();
    let file = store.load(id).unwrap();
    let mut doc = file.document;
    let mut h = History::default();
    let feature = key.part.feature;
    let mut e = doc.element(key.studio).unwrap().feature(feature).unwrap().extrude().unwrap().clone();
    e.depth = height;
    e.depth_expr = format!("{height} mm");
    h.execute(&mut doc, &SetExtrude { element: key.studio, feature, extrude: e, label: "Depth".into() }).unwrap();
    store.save(&doc, &file.meta).unwrap();
    let mut log = HistoryLog::load(store, id).unwrap().unwrap();
    log.record(&doc, Origin::Command("Edit Extrude".into()), t, USER);
    let v = log.create_version("", "", t + 1, USER);
    log.save(store).unwrap();
    v
}

/// Every component instance's link: version-pinned to a version of a component document.
fn assert_linked(doc: &Document, g: &GeneratedAssembly) {
    let model = doc.element(g.assembly).unwrap().assembly_model().unwrap();
    for c in &g.components {
        let inst = model.instance(c.instance).unwrap();
        let r = inst.link.unwrap_or_else(|| panic!("{} has no link", c.refdes));
        assert!(r.document.is_some_and(|d| d != doc.id), "another document");
        assert!(matches!(r.at, RefAt::Version(_)), "a version");
        let InstanceSource::Part { element, .. } = inst.source else { panic!() };
        assert!(doc.linked_element(element).is_some(), "its source is the frozen copy");
    }
}

#[test]
fn create_assembly_makes_one_document_per_new_package() {
    // PCB7.3, PCB11.1, X7: the secondary board places 20 components of 7 packages → 7 component
    // documents in the component folder, each with exactly one Part Studio (one part: the package
    // box, with Part number and Description) and one empty Assembly, and a version V1; the 20
    // instances are version-pinned references to those V1s, named "<package> <n>".
    let store = temp_store("one-per-package");
    let (mut doc, mut h) = board_document(&store, "secondary", fixture("secondary board", "secondary board"));
    let before = doc.clone();
    let (g, docs) = create(&store, &mut doc, &mut h, None, 2_000);
    assert_eq!(docs.created(), 7);
    let lib = library(&store);
    let folder = lib.folders.iter().find(|f| f.name == DEFAULT_COMPONENT_FOLDER).expect("the folder is made when none is chosen");
    let comps: Vec<_> = lib.entries.iter().filter(|e| e.meta.pcb_component.is_some()).collect();
    assert_eq!(comps.len(), 7);
    assert_eq!(g.documents.len(), 7);
    let mut res = Resolver::new(store.clone());
    for e in &comps {
        assert_eq!(e.meta.folder, Some(folder.id), "{}", e.name);
        let key: &ComponentKey = e.meta.pcb_component.as_ref().unwrap();
        assert_eq!(e.name, key.package);
        let d = store.load(e.id).unwrap().document;
        let studios: Vec<_> = d.elements.iter().filter(|x| matches!(x.kind, ElementKind::PartStudio { .. })).collect();
        let asms: Vec<_> = d.elements.iter().filter(|x| x.assembly_model().is_some()).collect();
        assert_eq!((studios.len(), asms.len(), d.elements.len()), (1, 1, 2), "{}", e.name);
        assert!(asms[0].assembly_model().unwrap().instances.is_empty(), "an empty Assembly");
        let b = cadrs_core::rebuild::build(&studios[0].active_features());
        assert_eq!(b.parts.len(), 1, "one part");
        assert_eq!(b.parts[0].id, key.part);
        assert_eq!(cadrs_core::parts::display_name(&b.parts[0], studios[0].part_props()), key.package);
        let versions = res.versions(e.id);
        assert_eq!(versions.iter().map(|v| v.name().to_string()).collect::<Vec<_>>(), ["V1"]);
    }
    // uBGA48_7.4X7.1: 7.4 × 7.1 × 1.2 = 63.048 mm³.
    close("X2 at V1", volume_of(&doc, &g, "X2"), 7.4 * 7.1 * 1.2);
    assert_linked(&doc, &g);
    let model = doc.element(g.assembly).unwrap().assembly_model().unwrap();
    assert_eq!(model.instances.len(), 21, "the board and 20 components");
    let (id, _) = find_component_document(&lib, "uBGA48_7.4X7.1", "7401048").unwrap();
    let x2 = model.instance(g.components.iter().find(|c| c.refdes == "X2").unwrap().instance).unwrap();
    let v1 = res.versions(id)[0].id();
    assert_eq!(x2.link.unwrap().document, Some(id));
    assert_eq!(x2.link.unwrap().at, RefAt::Version(v1));
    assert_eq!(x2.name(&assembly::source_part_name(&doc, &x2.source, None)), "uBGA48_7.4X7.1 <1>");
    // No in-document components studio any more.
    assert!(g.components_studio.is_none());
    assert_eq!(doc.elements.iter().filter(|e| e.name.ends_with("components")).count(), 0);
    // One undo step in the board document takes the tabs and the copies out; the component
    // documents and their versions stay (like Move to document's target).
    h.undo(&mut doc).unwrap();
    assert_eq!(doc, before);
    assert_eq!(library(&store).entries.iter().filter(|e| e.meta.pcb_component.is_some()).count(), 7);
    // Create again: everything is reused.
    let (g2, docs2) = create(&store, &mut doc, &mut h, None, 3_000);
    assert_eq!(docs2.created(), 0);
    assert_eq!(g2.documents.len(), 7);
    assert_linked(&doc, &g2);
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn second_create_reuses_component_documents() {
    // PCB7.3: a second board (another document) sharing six of its seven packages reuses their
    // documents (at their newest version) and adds one document, for the new package only; the
    // settings' folder is made (same id and name) when it was deleted.
    let store = temp_store("reuse");
    let chosen = FolderRef { id: FolderId::new(), name: "Components of my boards".into() };
    let (mut a, mut ha) = board_document(&store, "first", fixture("secondary board", "secondary board"));
    let (ga, da) = create(&store, &mut a, &mut ha, Some(&chosen), 2_000);
    assert_eq!(da.created(), 7);
    assert!(library(&store).folders.iter().any(|f| f.id == chosen.id && f.name == chosen.name));
    let (mut b, mut hb) = board_document(&store, "second", secondary_with_a_new_package());
    let (gb, db) = create(&store, &mut b, &mut hb, Some(&chosen), 3_000);
    assert_eq!(db.created(), 1, "only TEST_CAN_9.0X5.0 is new");
    let shared: std::collections::BTreeSet<String> = secondary_with_a_new_package().board.placements.iter().map(|p| p.package.clone()).filter(|p| p != "TEST_CAN_9.0X5.0").collect();
    assert_eq!(db.used.iter().filter(|u| !u.created).count(), shared.len(), "the shared packages are reused");
    let lib = library(&store);
    assert_eq!(lib.entries.iter().filter(|e| e.meta.pcb_component.is_some()).count(), 8);
    assert!(lib.entries.iter().filter(|e| e.meta.pcb_component.is_some()).all(|e| e.meta.folder == Some(chosen.id)));
    // The shared package's instances reference the same document and version in both boards.
    let link = |d: &Document, g: &GeneratedAssembly, r: &str| {
        let i = g.components.iter().find(|c| c.refdes == r).unwrap().instance;
        d.element(g.assembly).unwrap().assembly_model().unwrap().instance(i).unwrap().link.unwrap()
    };
    assert_eq!(link(&a, &ga, "X2"), link(&b, &gb, "X2"));
    assert_ne!(link(&a, &ga, "X1").document, link(&b, &gb, "X1").document);
    // X1 is the new 9 × 5 × 2 can: 90 mm³.
    close("the new package", volume_of(&b, &gb, "X1"), 9.0 * 5.0 * 2.0);
    // Its Description came from the .emp.
    let bom = compute(&b, gb.assembly, &BomOptions::default(), &Default::default(), build_of(&b)).unwrap();
    let col = |k: PropertyKey| bom.columns.iter().position(|c| *c == BomColumn::Property(k)).unwrap();
    let row = bom.rows.iter().find(|r| r.cells[col(PropertyKey::PartNumber)] == "9905000").unwrap();
    assert_eq!(row.cells[col(PropertyKey::Description)], "Test can");
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn edit_version_and_update_change_the_instances() {
    // PCB11.2, PCB9.5: the uBGA's component document edited in its own document (height 1.2 →
    // 2 mm) and versioned V2: the board assembly's X2 reference is out of date (V2 is newer) but
    // unchanged (63.048 mm³) until updated; Update to latest makes it 7.4 · 7.1 · 2 = 105.08 mm³,
    // the instance keeps its id, pose and designator tie; undo goes back to V1.
    let store = temp_store("update");
    let (mut doc, mut h) = board_document(&store, "secondary", fixture("secondary board", "secondary board"));
    let (g, _) = create(&store, &mut doc, &mut h, None, 2_000);
    let x2 = g.components.iter().find(|c| c.refdes == "X2").unwrap().instance;
    let pose = |d: &Document| d.element(g.assembly).unwrap().assembly_model().unwrap().instance(x2).unwrap().pose;
    let pose0 = pose(&doc);
    close("V1", volume_of(&doc, &g, "X2"), 7.4 * 7.1 * 1.2);
    let v2 = edit_height(&store, "uBGA48_7.4X7.1", "7401048", 2.0, 3_000);
    let mut res = Resolver::new(store.clone());
    let site = lu::RefSite::Instance { element: g.assembly, instance: x2 };
    let u = lu::use_at(&doc, site).unwrap();
    let mut latest = |d: DocumentId| res.latest(d).map(|v| (v.id(), v.name().to_string()));
    let st = lu::staleness(&doc, &u, &mut latest);
    assert_eq!(st.newer, Some((v2, "V2".to_string())), "the update badge");
    close("still V1 until updated", volume_of(&doc, &g, "X2"), 7.4 * 7.1 * 1.2);
    // The other packages' references are up to date.
    let stale: Vec<_> = lu::uses(&doc).into_iter().filter(|u| lu::staleness(&doc, u, &mut |d| res.latest(d).map(|v| (v.id(), v.name().to_string()))).any()).collect();
    assert_eq!(stale.len(), 1, "only the uBGA's one instance");
    let c = lu::change_for(&mut res, &doc, None, &u, Target::Latest).unwrap().unwrap();
    h.execute(&mut doc, &UpdateReferences { changes: vec![c], label: "Update to latest".into() }).unwrap();
    close("after Update", volume_of(&doc, &g, "X2"), 7.4 * 7.1 * 2.0);
    assert_eq!(pose(&doc), pose0, "the instance stays where it was");
    assert_eq!(doc.element(g.assembly).unwrap().assembly_model().unwrap().instance(x2).unwrap().link.unwrap().at, RefAt::Version(v2));
    // Sync still reads X2 through its tie after the update (its source is a new copy).
    let p = plan(&doc, PCB, g.assembly, SyncPlaneChoice::Top, None).unwrap();
    assert_eq!(p.components.len(), 20);
    h.undo(&mut doc).unwrap();
    close("undo", volume_of(&doc, &g, "X2"), 7.4 * 7.1 * 1.2);
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn moved_component_documents_stay_tracked() {
    // PCB11.3: the component documents moved to another folder (and one renamed): Where used
    // still finds the board document, an edit + version still shows as an update and updates,
    // and a later Create still reuses them (found by their key, not by name or folder).
    let store = temp_store("moved");
    let (mut doc, mut h) = board_document(&store, "secondary", fixture("secondary board", "secondary board"));
    let (g, _) = create(&store, &mut doc, &mut h, None, 2_000);
    // Move every component document into "Boards/secondary" and rename the uBGA's.
    let before = library(&store);
    let mut lib = before.clone();
    let mut lh = LibraryHistory::default();
    let target = FolderEntry { id: FolderId::new(), name: "secondary board parts".into(), created: 2_100, owned_by: USER.into() };
    lib.folders.push(target.clone());
    let comps: Vec<DocumentId> = lib.entries.iter().filter(|e| e.meta.pcb_component.is_some()).map(|e| e.id).collect();
    for id in &comps {
        lh.execute(&mut lib, &MoveToFolder { id: *id, folder: Some(target.id) }).unwrap();
    }
    let (ubga, _) = find_component_document(&lib, "uBGA48_7.4X7.1", "7401048").unwrap();
    lh.execute(&mut lib, &cadrs_core::library::RenameEntry { id: ubga, name: "My BGA".into(), user: USER.into(), now: 2_200 }).unwrap();
    store.sync(&before, &lib).unwrap();
    let lib = library(&store);
    assert!(comps.iter().all(|id| lib.get(*id).unwrap().meta.folder == Some(target.id)));
    // Where used.
    let used = lu::where_used(&store, ubga);
    assert_eq!(used.len(), 1, "{used:?}");
    assert_eq!(used[0].document, doc.id);
    assert_eq!(used[0].count, 1);
    // Edit, version, update.
    edit_height(&store, "uBGA48_7.4X7.1", "7401048", 3.0, 3_000);
    let mut res = Resolver::new(store.clone());
    let x2 = g.components.iter().find(|c| c.refdes == "X2").unwrap().instance;
    let u = lu::use_at(&doc, lu::RefSite::Instance { element: g.assembly, instance: x2 }).unwrap();
    let c = lu::change_for(&mut res, &doc, None, &u, Target::Latest).unwrap().unwrap();
    h.execute(&mut doc, &UpdateReferences { changes: vec![c], label: "Update".into() }).unwrap();
    close("updated from the moved document", volume_of(&doc, &g, "X2"), 7.4 * 7.1 * 3.0);
    // A new Create (in another document) reuses all seven, the renamed one too, at V2.
    let (mut other, mut ho) = board_document(&store, "again", fixture("secondary board", "secondary board"));
    let (go, docs) = create(&store, &mut other, &mut ho, None, 4_000);
    assert_eq!(docs.created(), 0);
    close("reused at its newest version", volume_of(&other, &go, "X2"), 7.4 * 7.1 * 3.0);
    assert!(docs.used.iter().any(|u| u.document == ubga && u.name == "My BGA" && u.version_name == "V2"));
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn component_documents_keep_bom_area_sync_and_export() {
    // PCB7.6, PCB8, PCB10 with linked components: Vision PCB's BOM has one row per part number
    // with its Description ("Resistor 10K OHM ±1%" ×4 …); U1's top face is still 15.24² =
    // 232.2576 mm²; and Ex3 (keep-out, X2 moved 1 in, sync, export IDF 3.0) still writes X2 at
    // (4.064182376174947, 8.9) with the .PLACE_KEEPOUT of 112.314 mm².
    let store = temp_store("downstream");
    let (mut v, mut hv) = board_document(&store, "vision", fixture("vision controller", "Vision PCB"));
    let (gv, _) = create(&store, &mut v, &mut hv, None, 2_000);
    let pcb = fixture("vision controller", "Vision PCB");
    let packages: std::collections::BTreeSet<(String, String)> = pcb.board.placements.iter().map(|p| (p.package.clone(), p.part_number.clone())).collect();
    let b = compute(&v, gv.assembly, &BomOptions::default(), &Default::default(), build_of(&v)).unwrap();
    let col = |k: PropertyKey| b.columns.iter().position(|c| *c == BomColumn::Property(k)).unwrap();
    assert_eq!(b.rows.len(), packages.len() + 1, "one row per part number, and the board");
    let r = b.rows.iter().find(|r| r.cells[col(PropertyKey::PartNumber)] == "RC0603-10K").unwrap();
    assert_eq!(r.quantity as usize, pcb.board.placements.iter().filter(|p| p.part_number == "RC0603-10K").count());
    assert_eq!(r.cells[col(PropertyKey::Description)], "Resistor 10K OHM ±1%");
    assert!(b.rows.iter().all(|r| !r.cells[col(PropertyKey::Description)].is_empty()), "every row has a Description");
    assert!(b.rows.iter().any(|r| r.cells[col(PropertyKey::Description)] == "Board, Vision PCB"), "the board's row");
    // U1's top face.
    let asm = v.element(gv.assembly).unwrap().assembly_model().unwrap();
    let u1 = gv.components.iter().find(|c| c.refdes == "U1").unwrap();
    let inst = asm.instance(u1.instance).unwrap();
    let InstanceSource::Part { element, part } = inst.source else { panic!() };
    let build = cadrs_core::rebuild::build(&v.element(element).unwrap().active_features());
    let p = build.part(part).unwrap();
    let top = p.solid.faces.iter().filter(|f| f.plane.is_some_and(|fr| inst.pose.rotate(fr.normal())[2] > 0.999)).max_by(|a, b| a.plane.unwrap().origin[2].total_cmp(&b.plane.unwrap().origin[2])).unwrap();
    close("U1 top face", top.area.unwrap(), 15.24 * 15.24);

    // Ex3.
    let (mut doc, mut h) = board_document(&store, "secondary", fixture("secondary board", "secondary board"));
    let (g, _) = create(&store, &mut doc, &mut h, None, 3_000);
    let (sk, ex) = (FeatureId::from_u128(0x3a07_0001), FeatureId::from_u128(0x3a07_0002));
    cadrs_pcb::sample::ex3_keepout_sketch(&mut DocHistory(&mut doc, &mut h), g.studio, sk).unwrap();
    let e = cadrs_core::document::ExtrudeFeature { sketches: vec![sk], depth: 3.175, depth_expr: "0.125 in".into(), ..Default::default() };
    h.execute(&mut doc, &cadrs_core::commands::AddExtrude { element: g.studio, feature: ex, extrude: e }).unwrap();
    let kpart = PartId::new(ex, 0);
    h.execute(&mut doc, &RenamePart { element: g.studio, part: kpart, name: "Keep-out".into() }).unwrap();
    h.execute(&mut doc, &InsertInstance { element: g.assembly, instance: Instance::new(InstanceId::new(), InstanceSource::Part { element: g.studio, part: kpart }, Pose::IDENTITY) }).unwrap();
    let x2 = g.components.iter().find(|c| c.refdes == "X2").unwrap().instance;
    let before = doc.element(g.assembly).unwrap().assembly_model().unwrap().instance(x2).unwrap().pose;
    h.execute(&mut doc, &MoveInstances { element: g.assembly, poses: vec![(x2, before.then(&Pose::translation([0.0, 25.4, 0.0])))], label: "Move".into() }).unwrap();
    let shown = doc.element(PCB).unwrap().pcb().unwrap().active;
    let p = plan(&doc, PCB, g.assembly, SyncPlaneChoice::Top, shown).unwrap();
    assert_eq!(p.components.len(), 20);
    let o = run_now(p).unwrap();
    assert!(o.unrecognised.is_empty(), "{:?}", o.unrecognised);
    h.execute(&mut doc, &o.command).unwrap();
    let s = doc.element(PCB).unwrap().pcb().unwrap();
    assert_eq!(s.boards.len(), 1);
    let bd = &s.board(g.board).unwrap().board;
    let zip = cadrs_idf::write_zip(&bd.board, &bd.library, IdfVersion::V3);
    let (_, emn) = cadrs_idf::zip_entries(&zip).unwrap().into_iter().find(|(n, _)| n.ends_with("secondary board.emn")).unwrap();
    let emn = String::from_utf8(emn).unwrap();
    let i = emn.lines().position(|l| l.starts_with("uBGA48_7.4X7.1 ")).unwrap();
    let line = emn.lines().nth(i + 1).unwrap();
    let w: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(&w[..2], ["4.064182376174947", "8.9"], "{line}");
    assert!(line.ends_with("90 TOP PLACED"), "{line}");
    assert!(emn.contains(".PLACE_KEEPOUT"));
    let (board, _) = cadrs_idf::read_pair(&emn, &String::from_utf8(cadrs_idf::zip_entries(&zip).unwrap().into_iter().find(|(n, _)| n.ends_with(".emp")).unwrap().1).unwrap()).unwrap();
    let area = 12.7 * 9.525 - (1.0 - std::f64::consts::FRAC_PI_4) * 6.35 * 6.35;
    close("keep-out area", board.place_keepouts[0].loops[0].area(), area);
    let _ = std::fs::remove_dir_all(store.root());
}

#[test]
fn sync_recognises_components_through_their_link() {
    // PCB5.5: an instance of a component document is a component whatever its part is called:
    // (a) the parts renamed in the component documents' new versions and updated into the
    // assembly; (b) the Create tie lost (an instance deleted and inserted again by hand, at the
    // same place): it is recognised by its document (the uBGA's package), at the nearest
    // placement; (c) at depth: the board assembly inside a "Product" assembly.
    let store = temp_store("sync");
    let (mut doc, mut h) = board_document(&store, "secondary", fixture("secondary board", "secondary board"));
    let (g, _) = create(&store, &mut doc, &mut h, None, 2_000);
    // (a) Rename the uBGA's part to "Renamed" in V2 and update.
    let (id, key) = find_component_document(&library(&store), "uBGA48_7.4X7.1", "7401048").unwrap();
    let file = store.load(id).unwrap();
    let mut cd = file.document;
    let mut ch = History::default();
    ch.execute(&mut cd, &RenamePart { element: key.studio, part: key.part, name: "Renamed".into() }).unwrap();
    store.save(&cd, &file.meta).unwrap();
    let mut log = HistoryLog::load(&store, id).unwrap().unwrap();
    log.record(&cd, Origin::Command("Rename".into()), 3_000, USER);
    log.create_version("", "", 3_001, USER);
    log.save(&store).unwrap();
    let mut res = Resolver::new(store.clone());
    let x2 = g.components.iter().find(|c| c.refdes == "X2").unwrap().instance;
    let u = lu::use_at(&doc, lu::RefSite::Instance { element: g.assembly, instance: x2 }).unwrap();
    let c = lu::change_for(&mut res, &doc, None, &u, Target::Latest).unwrap().unwrap();
    h.execute(&mut doc, &UpdateReferences { changes: vec![c], label: "Update".into() }).unwrap();
    let p = plan(&doc, PCB, g.assembly, SyncPlaneChoice::Top, None).unwrap();
    assert_eq!(p.components.len(), 20);
    assert!(p.unrecognised.is_empty(), "{:?}", p.unrecognised);
    // (b) X2 deleted and inserted again by hand (no tie), at the same pose.
    let inst = doc.element(g.assembly).unwrap().assembly_model().unwrap().instance(x2).unwrap().clone();
    h.execute(&mut doc, &cadrs_core::assembly::commands::DeleteInstances { element: g.assembly, instances: vec![x2] }).unwrap();
    let again = Instance { id: InstanceId::new(), ..inst.clone() };
    h.execute(&mut doc, &InsertInstance { element: g.assembly, instance: again }).unwrap();
    let p = plan(&doc, PCB, g.assembly, SyncPlaneChoice::Top, None).unwrap();
    assert_eq!(p.components.len(), 20, "recognised by its document");
    let c = p.components.iter().find(|c| c.refdes == "X2").expect("X2, the only uBGA placement");
    assert_eq!(c.package, "uBGA48_7.4X7.1");
    assert!(p.unrecognised.is_empty(), "{:?}", p.unrecognised);
    let o = run_now(p).unwrap();
    let q = o.command.board.placement("X2").unwrap();
    close("X2 x", q.x, 4.064182376174947);
    close("X2 y", q.y, -16.5);
    close("X2 rotation", q.rotation, 90.0);
    // (c) At depth.
    let top = ElementId::from_u128(0x3a07_9002);
    h.execute(&mut doc, &AddElement { id: top, kind: NewElementKind::Assembly, name: Some("Product".into()), after: None }).unwrap();
    h.execute(&mut doc, &InsertInstance { element: top, instance: Instance::new(InstanceId::new(), InstanceSource::Assembly { element: g.assembly }, Pose::IDENTITY) }).unwrap();
    let p = plan(&doc, PCB, top, SyncPlaneChoice::Top, None).unwrap();
    assert_eq!(p.components.len(), 20, "found inside the subassembly");
    let _ = std::fs::remove_dir_all(store.root());
}
