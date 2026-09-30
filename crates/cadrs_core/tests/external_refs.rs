//! P3G.1: external references and version-pinned links (`derived-and-linking-gaps.md` P3G.1
//! "Done when"; DV1.1, DV1.2, DV1.5, DV1.7, DV1.11, DV1.12, DV2.3, DV X1, DV X2, ER1.10, ER2.1,
//! ER3.1, ER3.5, ER4.1, ER X6; drawings D13.1, X14).
//!
//! The source is the linked-documents block ([`cadrs_core::samples::linked_block`]): a
//! 50 × 30 × 25 box, so V = 50·30·25 = 37 500 mm³, and after its depth is edited to 40,
//! 50·30·40 = 60 000 mm³ (computed here from the dimensions, not read back from the model).
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::assembly::commands::InsertInstance;
use cadrs_core::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::external::{self, InsertLinked, LinkError, LinkState, Resolver, SourceRef};
use cadrs_core::history_log::{HistoryLog, Origin, VersionId};
use cadrs_core::library::{Library, LibraryHistory, MoveToFolder, PurgeEntry, TrashEntry};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::linked_block as lb;
use cadrs_core::{Document, DocumentMeta, ElementId, FolderId, History, Store};

const V_25: f64 = lb::LENGTH * lb::WIDTH * lb::HEIGHT;
const V_40: f64 = lb::LENGTH * lb::WIDTH * lb::EDITED_HEIGHT;

fn temp_store(tag: &str) -> Store {
    Store::new(std::env::temp_dir().join(format!("cadrs-external-{tag}-{}-{}", std::process::id(), uuid::Uuid::new_v4())))
}

#[track_caller]
fn close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() < 1e-6 * want.abs().max(1.0), "{what}: got {got}, want {want}");
}

/// Saves `doc` with a history whose only version is "V1" (the document as it is).
fn save_with_v1(store: &Store, doc: &Document) -> VersionId {
    store.create(doc, &DocumentMeta::new("me", 1_000)).unwrap();
    let mut log = HistoryLog::start(doc, 1_000, "me");
    let v = log.create_version("", "", 1_001, "me");
    log.save(store).unwrap();
    v
}

/// Edits the block's depth in the stored document A (its workspace), and makes the next version.
fn edit_source(store: &Store, id: cadrs_core::DocumentId, studio: ElementId, depth: f64) -> VersionId {
    let file = store.load(id).unwrap();
    let mut doc = file.document;
    let mut h = History::default();
    lb::set_height(&mut DocHistory(&mut doc, &mut h), studio, depth).unwrap();
    store.save(&doc, &file.meta).unwrap();
    let mut log = HistoryLog::load(store, id).unwrap().unwrap();
    log.record(&doc, Origin::Command("Edit Extrude 1".into()), 2_000, "me");
    let v = log.create_version("", "", 2_001, "me");
    log.save(store).unwrap();
    v
}

/// The builds of every element an assembly needs.
fn build_of(doc: &Document) -> impl FnMut(ElementId) -> Option<Arc<cadrs_core::rebuild::Build>> + '_ {
    |e| doc.element(e).map(|el| cadrs_core::rebuild::build(el.features()))
}

/// The volume of each instance of assembly `asm` (by instance), and the total.
fn volumes(doc: &Document, asm: ElementId) -> (Vec<(InstanceId, f64)>, f64) {
    let model = doc.element(asm).unwrap().assembly_model().unwrap();
    let (parts, _) = assembly::instance_parts(doc, model, build_of(doc));
    let mut out: Vec<(InstanceId, f64)> = Vec::new();
    for p in &parts {
        let i = InstanceId::of_part(p.id);
        let v = p.mass.as_ref().map(|m| m.volume).unwrap_or(0.0);
        match out.iter_mut().find(|(x, _)| *x == i) {
            Some((_, t)) => *t += v,
            None => out.push((i, v)),
        }
    }
    let total = out.iter().map(|(_, v)| v).sum();
    (out, total)
}

fn volume_of(doc: &Document, asm: ElementId, i: InstanceId) -> f64 {
    volumes(doc, asm).0.into_iter().find(|(x, _)| *x == i).map(|(_, v)| v).unwrap_or(0.0)
}

/// A stable hash of the geometry an instance shows (its mesh positions to 1 µm).
fn geometry_hash(doc: &Document, asm: ElementId, i: InstanceId) -> u64 {
    let model = doc.element(asm).unwrap().assembly_model().unwrap();
    let (parts, _) = assembly::instance_parts(doc, model, build_of(doc));
    let mut bytes = Vec::new();
    for p in parts.iter().filter(|p| InstanceId::of_part(p.id) == i) {
        for q in &p.solid.positions {
            for v in q {
                bytes.extend_from_slice(&((v * 1e3).round() as i64).to_le_bytes());
            }
        }
    }
    cadrs_kernel::naming::stable_hash(&bytes)
}

/// Inserts one instance of the block's copy at `at` into `asm` of `doc`.
fn insert_linked(doc: &mut Document, h: &mut History, asm: ElementId, r: SourceRef, snap: external::LinkSnapshot, id: InstanceId) {
    let inst = Instance::new(id, InstanceSource::Part { element: snap.root, part: lb::PART }, Pose::IDENTITY);
    h.execute(doc, &InsertLinked { element: asm, snapshot: snap, instances: vec![inst], reference: r }).unwrap();
}

/// A consumer B with an instance of A@V1: A's workspace edit and V2 change nothing in B; a link
/// made at V2 reads the new volume (ER2.1, DV1.11).
#[test]
fn an_instance_of_a_version_keeps_its_volume_after_the_source_changes() {
    let store = temp_store("pinned");
    let a = lb::document().unwrap();
    let v1 = save_with_v1(&store, &a);
    let mut b = lb::consumer();
    let mut h = History::default();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v1);
    let snap = res.resolve(r, &b, None).unwrap();
    assert_eq!(snap.links.len(), 1, "a Part Studio needs only itself");
    assert_ne!(snap.root, lb::STUDIO, "the copy has a namespaced id");
    assert_eq!(snap.root_link().unwrap().version_name, "V1");
    let i1 = InstanceId::from_u128(1);
    insert_linked(&mut b, &mut h, lb::CONSUMER_ASSEMBLY, r, snap, i1);
    let inst = b.element(lb::CONSUMER_ASSEMBLY).unwrap().assembly_model().unwrap().instance(i1).unwrap().clone();
    assert_eq!(inst.link, Some(r), "the instance carries its reference");
    assert_eq!(inst.index, 1);
    close("B at V1", volume_of(&b, lb::CONSUMER_ASSEMBLY, i1), V_25);
    let hash = geometry_hash(&b, lb::CONSUMER_ASSEMBLY, i1);
    let text = ron::to_string(&b).unwrap();

    // A's workspace edit, then V2.
    let v2 = edit_source(&store, lb::DOCUMENT, lb::STUDIO, lb::EDITED_HEIGHT);
    let a_now = store.load(lb::DOCUMENT).unwrap().document;
    let a_parts = cadrs_core::rebuild::build(a_now.element(lb::STUDIO).unwrap().features());
    close("A's workspace", a_parts.parts[0].mass.as_ref().unwrap().volume, V_40);
    // B is untouched: the same document, volume and geometry.
    assert_eq!(ron::to_string(&b).unwrap(), text);
    close("B after A's edit", volume_of(&b, lb::CONSUMER_ASSEMBLY, i1), V_25);
    assert_eq!(geometry_hash(&b, lb::CONSUMER_ASSEMBLY, i1), hash, "the linked geometry is unchanged");
    // A link made at V2 reads 60 000 in the same assembly, next to the V1 instance.
    let r2 = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v2);
    let snap2 = res.resolve(r2, &b, None).unwrap();
    assert_ne!(snap2.root, b.element(lb::CONSUMER_ASSEMBLY).unwrap().assembly_model().unwrap().instance(i1).unwrap().source.element());
    let i2 = InstanceId::from_u128(2);
    insert_linked(&mut b, &mut h, lb::CONSUMER_ASSEMBLY, r2, snap2, i2);
    close("B at V2", volume_of(&b, lb::CONSUMER_ASSEMBLY, i2), V_40);
    // Numbered with the V1 instance: "Part 1 <2>".
    assert_eq!(b.element(lb::CONSUMER_ASSEMBLY).unwrap().assembly_model().unwrap().instance(i2).unwrap().index, 2);
    close("B's V1 instance", volume_of(&b, lb::CONSUMER_ASSEMBLY, i1), V_25);
    // ER X6: mass properties over both linked instances: 37 500 + 60 000.
    let model = b.element(lb::CONSUMER_ASSEMBLY).unwrap().assembly_model().unwrap();
    let (parts, props) = assembly::instance_parts(&b, model, build_of(&b));
    let rep = assembly::mass_report(&parts, &props, &[i1, i2]).unwrap();
    close("both", rep.volume, V_25 + V_40);
    // The same contents share one copy: linking V1 again adds nothing.
    let again = res.resolve(r, &b, None).unwrap();
    let n = b.linked.len();
    insert_linked(&mut b, &mut h, lb::CONSUMER_ASSEMBLY, r, again, InstanceId::from_u128(3));
    assert_eq!(b.linked.len(), n, "deduplicated by content");
    // Undo removes the instance (the copy stays with the step before it).
    h.undo(&mut b).unwrap();
    assert!(b.element(lb::CONSUMER_ASSEMBLY).unwrap().assembly_model().unwrap().instance(InstanceId::from_u128(3)).is_none());
    // B through the store format: the links survive.
    let back: Document = ron::from_str(&ron::to_string(&b).unwrap()).unwrap();
    assert_eq!(back, b);
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER3.1: in one document, an instance at V1 stays 37 500 while a workspace instance of the same
/// part follows the edit to 60 000.
#[test]
fn a_same_document_version_reference_is_frozen_and_a_workspace_one_follows() {
    let mut doc = lb::versions_document().unwrap();
    let mut h = History::default();
    let mut log = HistoryLog::start(&doc, 1_000, "me");
    let v1 = log.create_version("V1", "", 1_001, "me");
    let mut res = Resolver::new(temp_store("same"));
    let r = SourceRef::version(None, lb::VERSIONS_STUDIO, v1);
    let snap = res.resolve(r, &doc, Some(&log)).unwrap();
    let (iv, iw) = (InstanceId::from_u128(10), InstanceId::from_u128(11));
    insert_linked(&mut doc, &mut h, lb::VERSIONS_ASSEMBLY, r, snap, iv);
    let ws = Instance::new(iw, InstanceSource::Part { element: lb::VERSIONS_STUDIO, part: lb::PART }, Pose::translation([100.0, 0.0, 0.0]));
    h.execute(&mut doc, &InsertInstance { element: lb::VERSIONS_ASSEMBLY, instance: ws }).unwrap();
    close("version, before", volume_of(&doc, lb::VERSIONS_ASSEMBLY, iv), V_25);
    close("workspace, before", volume_of(&doc, lb::VERSIONS_ASSEMBLY, iw), V_25);
    lb::set_height(&mut DocHistory(&mut doc, &mut h), lb::VERSIONS_STUDIO, lb::EDITED_HEIGHT).unwrap();
    close("version, after", volume_of(&doc, lb::VERSIONS_ASSEMBLY, iv), V_25);
    close("workspace, after", volume_of(&doc, lb::VERSIONS_ASSEMBLY, iw), V_40);
    // One sequence per part, whatever the source (Onshape, ex1-step8): V1 <1>, workspace <2>, and
    // a V2 instance <3>.
    let index = |doc: &Document, i: InstanceId| doc.element(lb::VERSIONS_ASSEMBLY).unwrap().assembly_model().unwrap().instance(i).unwrap().index;
    assert_eq!((index(&doc, iv), index(&doc, iw)), (1, 2));
    log.record(&doc, Origin::Command("Edit".into()), 1_002, "me");
    let v2 = log.create_version("V2", "", 1_003, "me");
    let r2 = SourceRef::version(None, lb::VERSIONS_STUDIO, v2);
    let snap2 = res.resolve(r2, &doc, Some(&log)).unwrap();
    let i3 = InstanceId::from_u128(12);
    insert_linked(&mut doc, &mut h, lb::VERSIONS_ASSEMBLY, r2, snap2, i3);
    assert_eq!(index(&doc, i3), 3);
    close("V2 instance", volume_of(&doc, lb::VERSIONS_ASSEMBLY, i3), V_40);
    // The version instance's reference names no document (this one); the workspace instance has
    // no reference at all (it follows the tab).
    let model = doc.element(lb::VERSIONS_ASSEMBLY).unwrap().assembly_model().unwrap();
    assert_eq!(model.instance(iv).unwrap().link.unwrap().document, None);
    assert!(model.instance(iw).unwrap().link.is_none());
    // A workspace reference needs no link.
    assert_eq!(res.resolve(SourceRef { at: external::RefAt::Workspace, ..r }, &doc, Some(&log)), Err(LinkError::NoVersion));
}

/// DV1.7, ER1.10: the source trashed, then purged: the messages, and the geometry stays.
#[test]
fn a_trashed_or_purged_source_keeps_the_geometry_and_says_why() {
    let store = temp_store("trash");
    let a = lb::document().unwrap();
    let v1 = save_with_v1(&store, &a);
    let mut b = lb::consumer();
    let mut h = History::default();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v1);
    let snap = res.resolve(r, &b, None).unwrap();
    let i = InstanceId::from_u128(1);
    insert_linked(&mut b, &mut h, lb::CONSUMER_ASSEMBLY, r, snap, i);
    assert_eq!(res.state(lb::DOCUMENT), LinkState::Ok);
    let (mut lib, _) = store.list();
    let mut lh = LibraryHistory::default();
    let step = |lib: &mut Library, lh: &mut LibraryHistory, cmd: &dyn cadrs_core::LibraryCommand| {
        let before = lib.clone();
        lh.execute(lib, cmd).unwrap();
        store.sync(&before, lib).unwrap();
    };
    step(&mut lib, &mut lh, &TrashEntry { id: lb::DOCUMENT, now: 3_000 });
    assert_eq!(res.state(lb::DOCUMENT), LinkState::Trashed);
    assert_eq!(res.state(lb::DOCUMENT).message(), Some("Cannot open a document in the trash. Restore the document from Trash."));
    assert_eq!(res.resolve(r, &b, None).unwrap_err().to_string(), "Cannot open a document in the trash. Restore the document from Trash.");
    close("trashed", volume_of(&b, lb::CONSUMER_ASSEMBLY, i), V_25);
    step(&mut lib, &mut lh, &PurgeEntry { id: lb::DOCUMENT });
    assert_eq!(res.state(lb::DOCUMENT), LinkState::Gone);
    assert_eq!(res.state(lb::DOCUMENT).message(), Some("Resource does not exist"));
    assert_eq!(res.resolve(r, &b, None), Err(LinkError::State(LinkState::Gone)));
    close("purged", volume_of(&b, lb::CONSUMER_ASSEMBLY, i), V_25);
    // Undo of the purge brings it back to the trash.
    let before = lib.clone();
    lh.undo(&mut lib).unwrap();
    store.sync(&before, &lib).unwrap();
    assert_eq!(res.state(lb::DOCUMENT), LinkState::Trashed);
    // A document that can't be read: the "no access" stand-in.
    std::fs::write(store.document_path(lb::DOCUMENT), "not a document").unwrap();
    assert_eq!(res.state(lb::DOCUMENT), LinkState::Inaccessible);
    assert_eq!(
        res.state(lb::DOCUMENT).message(),
        Some("You cannot modify this feature because you cannot access the referenced document")
    );
    close("inaccessible", volume_of(&b, lb::CONSUMER_ASSEMBLY, i), V_25);
    let _ = std::fs::remove_dir_all(store.root());
}

/// DV1.5: A's assembly links B's assembly, which links A's: refused with the path; nothing added.
#[test]
fn a_circular_insert_is_refused() {
    use cadrs_core::Element;
    let store = temp_store("cycle");
    let asm_a = ElementId::from_u128(0xa1);
    let asm_a2 = ElementId::from_u128(0xa2);
    let asm_b = ElementId::from_u128(0xb1);
    let mut a = lb::document().unwrap();
    let mut e = Element::assembly("Assembly A");
    e.id = asm_a;
    a.elements.push(e);
    let mut e = Element::assembly("Assembly A2");
    e.id = asm_a2;
    a.elements.push(e);
    // Assembly A holds the block, so it has something to link.
    let mut h = History::default();
    h.execute(&mut a, &InsertInstance { element: asm_a, instance: Instance::new(InstanceId::from_u128(1), InstanceSource::Part { element: lb::STUDIO, part: lb::PART }, Pose::IDENTITY) }).unwrap();
    let va = save_with_v1(&store, &a);
    let mut b = Document::empty("Document B");
    b.id = cadrs_core::DocumentId::from_u128(0xb0);
    let mut e = Element::assembly("Assembly B");
    e.id = asm_b;
    b.elements.push(e);
    let mut res = Resolver::new(store.clone());
    let ra = SourceRef::version(Some(a.id), asm_a, va);
    let snap = res.resolve(ra, &b, None).unwrap();
    assert_eq!(snap.links.len(), 2, "Assembly A and the Block it needs");
    let mut hb = History::default();
    let sub = Instance::new(InstanceId::from_u128(2), InstanceSource::Assembly { element: snap.root }, Pose::IDENTITY);
    hb.execute(&mut b, &InsertLinked { element: asm_b, snapshot: snap, instances: vec![sub], reference: ra }).unwrap();
    close("B holds A's block", volumes(&b, asm_b).1, V_25);
    let vb = save_with_v1(&store, &b);
    // Back in A: Assembly B at V1 reaches Assembly A.
    let rb = SourceRef::version(Some(b.id), asm_b, vb);
    let snap_b = res.resolve(rb, &a, None).unwrap();
    let inst = Instance::new(InstanceId::from_u128(3), InstanceSource::Assembly { element: snap_b.root }, Pose::IDENTITY);
    let before = a.clone();
    let err = h.execute(&mut a, &InsertLinked { element: asm_a, snapshot: snap_b.clone(), instances: vec![inst.clone()], reference: rb }).unwrap_err();
    assert_eq!(err.to_string(), "Circular reference: Block source › Assembly A → Document B › Assembly B → Block source");
    assert_eq!(a, before, "nothing is added");
    // Into another assembly of A it is no cycle: B's copy holds Assembly A, not Assembly A2.
    h.execute(&mut a, &InsertLinked { element: asm_a2, snapshot: snap_b, instances: vec![inst], reference: rb }).unwrap();
    close("A2 holds B's (A's) block", volumes(&a, asm_a2).1, V_25);
    // A version of an assembly inserted into itself (same document) is refused too.
    let mut log = HistoryLog::start(&a, 1_000, "me");
    let v = log.create_version("", "", 1_001, "me");
    let r = SourceRef::version(None, asm_a, v);
    let snap = res.resolve(r, &a, Some(&log)).unwrap();
    let inst = Instance::new(InstanceId::from_u128(4), InstanceSource::Assembly { element: snap.root }, Pose::IDENTITY);
    let err = h.execute(&mut a, &InsertLinked { element: asm_a, snapshot: snap, instances: vec![inst], reference: r }).unwrap_err();
    assert!(err.to_string().starts_with("Circular reference: Block source › Assembly A → Block source › Assembly A"), "{err}");
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER4.1: C links B's assembly, which links A's block: the chain resolves through B's copy.
#[test]
fn a_chain_of_links_resolves_through_the_copies() {
    use cadrs_core::Element;
    let store = temp_store("chain");
    let a = lb::document().unwrap();
    let va = save_with_v1(&store, &a);
    let mut b = Document::empty("Sub document");
    b.id = cadrs_core::DocumentId::from_u128(0xb00);
    let e = Element::assembly("Sub");
    let sub = e.id;
    b.elements.push(e);
    let mut res = Resolver::new(store.clone());
    let ra = SourceRef::version(Some(a.id), lb::STUDIO, va);
    let snap = res.resolve(ra, &b, None).unwrap();
    let mut h = History::default();
    insert_linked(&mut b, &mut h, sub, ra, snap, InstanceId::from_u128(1));
    let vb = save_with_v1(&store, &b);
    let mut c = lb::consumer();
    let rb = SourceRef::version(Some(b.id), sub, vb);
    let snap_b = res.resolve(rb, &c, None).unwrap();
    assert_eq!(snap_b.links.len(), 2, "Sub and the Block copy it holds");
    let block = snap_b.links.iter().find(|l| l.source.element == lb::STUDIO).unwrap();
    assert_eq!(block.source.document, Some(a.id), "B's link, still naming A");
    let inst = Instance::new(InstanceId::from_u128(2), InstanceSource::Assembly { element: snap_b.root }, Pose::translation([0.0, 0.0, 10.0]));
    h.execute(&mut c, &InsertLinked { element: lb::CONSUMER_ASSEMBLY, snapshot: snap_b, instances: vec![inst], reference: rb }).unwrap();
    close("C", volumes(&c, lb::CONSUMER_ASSEMBLY).1, V_25);
    // The block sits where B put it, moved 10 up: its centroid (25, 15, 12.5 + 10).
    let model = c.element(lb::CONSUMER_ASSEMBLY).unwrap().assembly_model().unwrap();
    let (parts, _) = assembly::instance_parts(&c, model, build_of(&c));
    let (mut v, mut m) = (0.0, [0.0; 3]);
    for p in &parts {
        let mp = p.mass.as_ref().unwrap();
        v += mp.volume;
        for (k, x) in m.iter_mut().enumerate() {
            *x += mp.volume * mp.center_of_mass[k];
        }
    }
    close("x", m[0] / v, 25.0);
    close("y", m[1] / v, 15.0);
    close("z", m[2] / v, 22.5);
    let _ = std::fs::remove_dir_all(store.root());
}

/// D13.1, X14: a drawing view of V1 doesn't change (and isn't out of date) when the workspace
/// does; a view of the workspace is out of date.
#[test]
fn a_drawing_view_of_a_version_is_unchanged_by_a_workspace_edit() {
    use cadrs_core::drawing_source::{self as ds, StudioState};
    use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Scale, View, template};
    let mut doc = lb::versions_document().unwrap();
    let mut h = History::default();
    let mut log = HistoryLog::start(&doc, 1_000, "me");
    let v1 = log.create_version("V1", "", 1_001, "me");
    let mut res = Resolver::new(temp_store("drawing"));
    let snap = res.resolve(SourceRef::version(None, lb::VERSIONS_STUDIO, v1), &doc, Some(&log)).unwrap();
    let linked = snap.root;
    h.execute(&mut doc, &external::AddLinks { links: snap.links.clone() }).unwrap();
    let t = template::builtin("ANSI_A_MM.dwt").unwrap();
    let mut d = Drawing::from_template(&t, None);
    let sheet = d.sheets[0].id;
    let add = |d: &mut Drawing, element: ElementId, at: [f64; 2]| -> cadrs_drawing::ViewId {
        let src = ds::live_source(&doc, element).unwrap();
        let part = ObjectRef { element: element.0, part: ds::part_key(Some(lb::PART)) };
        let mut v = View::base(part, NamedView::Front, Scale::new(1, 1), at);
        v.source_hash = src.hash_of(part.part);
        if d.source(element.0).is_none() {
            d.apply(&DrawingOp::SetSource(src)).unwrap();
        }
        d.apply(&DrawingOp::InsertView { sheet, view: v.clone() }).unwrap();
        v.id
    };
    let at_v1 = add(&mut d, linked, [60.0, 100.0]);
    let at_ws = add(&mut d, lb::VERSIONS_STUDIO, [160.0, 100.0]);
    let el = cadrs_core::Element::drawing("Drawing 1", d);
    let drawing_id = el.id;
    h.execute(&mut doc, &cadrs_core::commands::InsertElement { element: el, after: None, label: "Create Drawing".into() }).unwrap();
    let drawing = |doc: &Document| doc.element(drawing_id).unwrap().drawing_data().unwrap().clone();
    let shown = |doc: &Document, id| {
        let d = drawing(doc);
        let (_, v) = d.view(id).unwrap();
        let st = StudioState::parse(&d.source(v.reference.element).unwrap().snapshot).unwrap();
        cadrs_core::views::project(&st.features, ds::view_request(&st, v)).unwrap()
    };
    assert!(ds::out_of_date(&doc, &drawing(&doc)).is_empty());
    let before = shown(&doc, at_v1);
    lb::set_height(&mut DocHistory(&mut doc, &mut h), lb::VERSIONS_STUDIO, lb::EDITED_HEIGHT).unwrap();
    assert_eq!(ds::out_of_date(&doc, &drawing(&doc)), vec![at_ws], "only the workspace view is out of date");
    // Updating the drawing brings the workspace view to 40 and leaves V1's at 25.
    let op = ds::update_now(&doc, &drawing(&doc)).unwrap();
    h.execute(&mut doc, &cadrs_core::commands::EditDrawing { element: drawing_id, op }).unwrap();
    assert!(ds::out_of_date(&doc, &drawing(&doc)).is_empty());
    assert_eq!(shown(&doc, at_v1).projection, before.projection, "the V1 view is unchanged");
    // Its front view is 50 × 25 (x × z); the workspace view's 50 × 40.
    let height = |g: &cadrs_core::views::ViewGeometry| {
        let ys: Vec<f64> = g.projection.edges.iter().flat_map(|e| e.points.iter().map(|p| p.y)).collect();
        ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min)
    };
    close("V1 view height", height(&shown(&doc, at_v1)), lb::HEIGHT);
    close("workspace view height", height(&shown(&doc, at_ws)), lb::EDITED_HEIGHT);
}

/// DV X2: resolving a version twice reads `history.ron` once (a new version is read again), and
/// a version of a long history loads quickly.
#[test]
fn resolving_a_version_twice_reads_the_history_once() {
    let store = temp_store("cache");
    let mut a = lb::document().unwrap();
    store.create(&a, &DocumentMeta::new("me", 1_000)).unwrap();
    let mut log = HistoryLog::start(&a, 1_000, "me");
    let v1 = log.create_version("", "", 1_001, "me");
    // 100 edits after V1 (7 full copies and a tail of deltas).
    let mut h = History::default();
    for k in 0..100 {
        lb::set_height(&mut DocHistory(&mut a, &mut h), lb::STUDIO, 25.0 + k as f64 * 0.1).unwrap();
        log.record(&a, Origin::Command("Edit".into()), 1_002 + k, "me");
    }
    let v2 = log.create_version("", "", 2_000, "me");
    log.save(&store).unwrap();
    let b = lb::consumer();
    let mut res = Resolver::new(store.clone());
    let start = std::time::Instant::now();
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v2);
    let s1 = res.resolve(r, &b, None).unwrap();
    let first = start.elapsed();
    let s2 = res.resolve(r, &b, None).unwrap();
    assert_eq!(s1, s2);
    assert_eq!(res.reads, 1, "the second resolve used the cache");
    let s0 = res.resolve(SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v1), &b, None).unwrap();
    assert_eq!(res.reads, 1, "another version of the same history: no read either");
    assert_ne!(s0.root, s1.root);
    assert!(first.as_secs_f64() < 2.0, "resolving took {first:?}");
    // Another version made through the resolver (the Other documents browser's Create version)
    // changes the file: the next resolve reads it again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let v3 = res.create_version(lb::DOCUMENT, "", "", 3_000, "me").unwrap();
    assert_eq!(res.versions(lb::DOCUMENT).last().map(|v| (v.id(), v.name().to_string())), Some((v3, "V3".into())));
    assert_eq!(res.reads, 2);
    let _ = std::fs::remove_dir_all(store.root());
}

/// A version 4 document (before P3G.1) loads as version 5 with no links.
#[test]
fn a_v4_document_loads() {
    let store = temp_store("v4");
    let doc = lb::document().unwrap();
    store.create(&doc, &DocumentMeta::new("me", 1_000)).unwrap();
    let path = store.document_path(doc.id);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("version: 5"));
    assert!(!text.contains("linked") && !text.contains("folder:"), "nothing new is written for a document without links");
    std::fs::write(&path, text.replace("version: 5", "version: 4")).unwrap();
    let file = store.load(doc.id).unwrap();
    assert_eq!(file.version, cadrs_core::store::SCHEMA_VERSION);
    assert_eq!(file.version, 5);
    assert_eq!(file.document, doc);
    assert!(file.document.linked.is_empty());
    assert_eq!(file.meta.folder, None);
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER1.2, ER X8: documents in folders (the Other documents browser's locations).
#[test]
fn documents_move_into_folders() {
    let store = temp_store("folders");
    let a = lb::document().unwrap();
    store.create(&a, &DocumentMeta::new("me", 1_000)).unwrap();
    store.create(&lb::consumer(), &DocumentMeta::new("me", 1_000)).unwrap();
    let (mut lib, _) = store.list();
    let mut lh = LibraryHistory::default();
    let f = FolderId::from_u128(7);
    let before = lib.clone();
    lh.execute(&mut lib, &cadrs_core::library::CreateFolder { id: f, name: "Linked".into(), user: "me".into(), now: 1 }).unwrap();
    lh.execute(&mut lib, &MoveToFolder { id: lb::DOCUMENT, folder: Some(f) }).unwrap();
    store.sync(&before, &lib).unwrap();
    assert_eq!(lib.in_folder(f).iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec![lb::DOCUMENT_NAME]);
    let (read, _) = store.list();
    assert_eq!(read.get(lb::DOCUMENT).unwrap().meta.folder, Some(f), "stored");
    assert!(lh.execute(&mut lib, &MoveToFolder { id: lb::DOCUMENT, folder: Some(FolderId::from_u128(8)) }).is_err());
    lh.undo(&mut lib).unwrap();
    assert!(lib.in_folder(f).is_empty());
    let _ = std::fs::remove_dir_all(store.root());
}

/// DV2.3, ER6.6, ER6.9: linked instances mate like local ones (a Fastened mate puts the V1 block
/// on top of the workspace block), and the BOM counts them with the part's properties at the
/// version (its part number then, not the workspace's).
#[test]
fn linked_instances_mate_and_keep_their_properties_in_the_bom() {
    use cadrs_core::assembly::bom;
    use cadrs_core::assembly::commands::AddMateFeature;
    use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
    use cadrs_core::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateType};
    use cadrs_core::assembly::solver::SolveOptions;
    let mut doc = lb::versions_document().unwrap();
    let set_number = |doc: &mut Document, n: &str| {
        let props = doc.element_mut(lb::VERSIONS_STUDIO).unwrap().part_props_mut().unwrap();
        if props.iter().all(|p| p.part != lb::PART) {
            props.push(cadrs_core::PartProps::new(lb::PART));
        }
        props.iter_mut().find(|p| p.part == lb::PART).unwrap().properties.part_number = Some(n.into());
    };
    set_number(&mut doc, "BLK-001");
    let mut h = History::default();
    let mut log = HistoryLog::start(&doc, 1_000, "me");
    let v1 = log.create_version("V1", "", 1_001, "me");
    let mut res = Resolver::new(temp_store("mate"));
    let r = SourceRef::version(None, lb::VERSIONS_STUDIO, v1);
    let (w, l1, l2) = (InstanceId::from_u128(20), InstanceId::from_u128(21), InstanceId::from_u128(22));
    let mut ws = Instance::new(w, InstanceSource::Part { element: lb::VERSIONS_STUDIO, part: lb::PART }, Pose::IDENTITY);
    ws.fixed = true;
    h.execute(&mut doc, &InsertInstance { element: lb::VERSIONS_ASSEMBLY, instance: ws }).unwrap();
    let snap = res.resolve(r, &doc, Some(&log)).unwrap();
    insert_linked(&mut doc, &mut h, lb::VERSIONS_ASSEMBLY, r, snap.clone(), l1);
    let mut far = Instance::new(l2, InstanceSource::Part { element: snap.root, part: lb::PART }, Pose::translation([200.0, 0.0, 0.0]));
    far.link = Some(r);
    h.execute(&mut doc, &InsertLinked { element: lb::VERSIONS_ASSEMBLY, snapshot: snap, instances: vec![far], reference: r }).unwrap();
    // The V1 block's bottom-face centre (25, 15, 0) fastened to the workspace block's top-face
    // centre (25, 15, 25): it moves up 25.
    let bottom = ConnectorFrame { origin: [25.0, 15.0, 0.0], ..ConnectorFrame::default() };
    let top = ConnectorFrame { origin: [25.0, 15.0, lb::HEIGHT], ..ConnectorFrame::default() };
    let m = Mate::new(MateType::Fastened, MateConnector::at(l1, bottom), MateConnector::at(w, top));
    let f = MateFeature::new(MateId::from_u128(9), "Fastened 1", MateKind::Mate(m));
    let mut model = doc.element(lb::VERSIONS_ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    model.mates.push(f.clone());
    let solids = assembly::occurrence_solids(&doc, &model, build_of(&doc));
    let s = assembly::solve(&model, &solids, &SolveOptions::default());
    assert!(s.converged, "residual {}", s.residual);
    let poses = s.changed(&model);
    h.execute(&mut doc, &AddMateFeature { element: lb::VERSIONS_ASSEMBLY, feature: f, poses }).unwrap();
    let placed = doc.element(lb::VERSIONS_ASSEMBLY).unwrap().assembly_model().unwrap().instance(l1).unwrap().pose;
    for (k, want) in [0.0, 0.0, lb::HEIGHT].iter().enumerate() {
        close("the V1 block's placement", placed.translation[k], *want);
    }
    // The workspace's part number changes; the version's stays.
    set_number(&mut doc, "BLK-002");
    let units = cadrs_sketch::units::Units::default();
    let b = bom::compute(&doc, lb::VERSIONS_ASSEMBLY, &bom::BomOptions::default(), &units, build_of(&doc)).unwrap();
    let rows: Vec<(u32, String)> = b.rows.iter().filter(|r| !r.top_level).map(|r| (r.quantity, r.cells.join("|"))).collect();
    assert!(rows.iter().any(|(q, c)| *q == 2 && c.contains("BLK-001")), "{rows:?}");
    assert!(rows.iter().any(|(q, c)| *q == 1 && c.contains("BLK-002")), "{rows:?}");
    assert_eq!(b.total_quantity, 3);
}

/// A command together with the copies it needs is one undo step (a drawing view of a version).
#[test]
fn a_command_with_its_links_undoes_in_one_step() {
    let mut doc = lb::versions_document().unwrap();
    let mut h = History::default();
    let mut log = HistoryLog::start(&doc, 1_000, "me");
    let v1 = log.create_version("V1", "", 1_001, "me");
    let snap = Resolver::new(temp_store("with")).resolve(SourceRef::version(None, lb::VERSIONS_STUDIO, v1), &doc, Some(&log)).unwrap();
    let before = doc.clone();
    let cmd = external::WithLinks { links: snap.links.clone(), command: cadrs_core::commands::RenameDocument { name: "Renamed".into() } };
    h.execute(&mut doc, &cmd).unwrap();
    assert_eq!(doc.linked.len(), 1);
    assert_eq!(doc.name, "Renamed");
    assert!(doc.element(snap.root).is_some(), "the copy is found like a tab");
    assert!(doc.element_mut(snap.root).is_none(), "but it can't be edited");
    h.undo(&mut doc).unwrap();
    assert_eq!(doc, before);
}
