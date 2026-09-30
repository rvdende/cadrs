//! P3G.2: updating, pinning and Update all (`derived-and-linking-gaps.md` P3G.2 "Done when";
//! DV1.4, DV1.6, DV1.10, ER2–ER5, ER X4; drawings D2.10).
//!
//! The source is the linked-documents block ([`cadrs_core::samples::linked_block`]): a
//! 50 × 30 × h box, so V = 50·30·h: 37 500 mm³ at h = 25, 60 000 at h = 40 and 82 500 at h = 55
//! (computed here from the dimensions, not read back from the model).
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::external::{InsertLinked, LinkSnapshot, RefAt, Resolver, SourceRef};
use cadrs_core::history_log::{HistoryLog, Origin, VersionId};
use cadrs_core::link_update::{self as lu, RefSite, RowTarget, SetPinned, Target, UpdateReferences};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::linked_block as lb;
use cadrs_core::{Document, DocumentMeta, ElementId, History, Store};

const V_25: f64 = lb::LENGTH * lb::WIDTH * lb::HEIGHT;
const V_40: f64 = lb::LENGTH * lb::WIDTH * lb::EDITED_HEIGHT;
const V_55: f64 = lb::LENGTH * lb::WIDTH * 55.0;

fn temp_store(tag: &str) -> Store {
    Store::new(std::env::temp_dir().join(format!("cadrs-linkupdate-{tag}-{}-{}", std::process::id(), uuid::Uuid::new_v4())))
}

#[track_caller]
fn close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() < 1e-6 * want.abs().max(1.0), "{what}: got {got}, want {want}");
}

/// Saves `doc` with a history whose only version is "V1".
fn save_with_v1(store: &Store, doc: &Document) -> VersionId {
    store.create(doc, &DocumentMeta::new("me", 1_000)).unwrap();
    let mut log = HistoryLog::start(doc, 1_000, "me");
    let v = log.create_version("", "", 1_001, "me");
    log.save(store).unwrap();
    v
}

/// Sets the stored block's depth (its workspace) and makes the next version.
fn edit_source(store: &Store, depth: f64, t: i64) -> VersionId {
    let file = store.load(lb::DOCUMENT).unwrap();
    let mut doc = file.document;
    let mut h = History::default();
    lb::set_height(&mut DocHistory(&mut doc, &mut h), lb::STUDIO, depth).unwrap();
    store.save(&doc, &file.meta).unwrap();
    let mut log = HistoryLog::load(store, lb::DOCUMENT).unwrap().unwrap();
    log.record(&doc, Origin::Command("Edit Extrude 1".into()), t, "me");
    let v = log.create_version("", "", t + 1, "me");
    log.save(store).unwrap();
    v
}

fn build_of(doc: &Document) -> impl FnMut(ElementId) -> Option<Arc<cadrs_core::rebuild::Build>> + '_ {
    |e| doc.element(e).map(|el| cadrs_core::rebuild::build(el.features()))
}

/// The volume of instance `i` of `asm` (its parts, subassemblies expanded), or of all with `None`.
fn volume(doc: &Document, asm: ElementId, i: Option<InstanceId>) -> f64 {
    let model = doc.element(asm).unwrap().assembly_model().unwrap();
    let (parts, _) = assembly::instance_parts(doc, model, build_of(doc));
    parts.iter().filter(|p| i.is_none_or(|i| InstanceId::of_part(p.id) == i)).map(|p| p.mass.as_ref().map(|m| m.volume).unwrap_or(0.0)).sum()
}

fn insert(doc: &mut Document, h: &mut History, asm: ElementId, r: SourceRef, snap: LinkSnapshot, id: InstanceId, source: InstanceSource) {
    let inst = Instance::new(id, source, Pose::IDENTITY);
    h.execute(doc, &InsertLinked { element: asm, snapshot: snap, instances: vec![inst], reference: r }).unwrap();
}

fn part_of(snap: &LinkSnapshot) -> InstanceSource {
    InstanceSource::Part { element: snap.root, part: lb::PART }
}

fn site(i: InstanceId) -> RefSite {
    RefSite::Instance { element: lb::CONSUMER_ASSEMBLY, instance: i }
}

/// The newest version of each document, read through the resolver.
fn latest(res: &mut Resolver) -> impl FnMut(cadrs_core::DocumentId) -> Option<(VersionId, String)> + '_ {
    |d| res.latest(d).map(|v| (v.id(), v.name().to_string()))
}

/// DV1.4, ER2.4: B's instance of A@V1 reads 37 500; after A's V2 it is out of date (V2 newer);
/// Update to latest brings 60 000 (the V1 copy is dropped); undo restores 37 500 and B exactly.
#[test]
fn update_to_latest_brings_the_new_version_and_undo_restores_it() {
    let store = temp_store("latest");
    let v1 = save_with_v1(&store, &lb::document().unwrap());
    let mut b = lb::consumer();
    let mut h = History::default();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v1);
    let snap = res.resolve(r, &b, None).unwrap();
    let i = InstanceId::from_u128(1);
    insert(&mut b, &mut h, lb::CONSUMER_ASSEMBLY, r, snap.clone(), i, part_of(&snap));
    close("at V1", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_25);
    let u = lu::use_at(&b, site(i)).unwrap();
    assert!(!lu::staleness(&b, &u, &mut latest(&mut res)).any(), "nothing newer yet");
    let v2 = edit_source(&store, lb::EDITED_HEIGHT, 2_000);
    let st = lu::staleness(&b, &u, &mut latest(&mut res));
    assert_eq!(st.newer, Some((v2, "V2".to_string())), "the blue badge: V2 is newer");
    assert!(!st.nested);
    close("still V1 until updated", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_25);
    let before = b.clone();
    let c = lu::change_for(&mut res, &b, None, &u, Target::Latest).unwrap().unwrap();
    h.execute(&mut b, &UpdateReferences { changes: vec![c], label: "Update to latest".into() }).unwrap();
    close("after Update to latest", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_40);
    let inst = b.element(lb::CONSUMER_ASSEMBLY).unwrap().assembly_model().unwrap().instance(i).unwrap().clone();
    assert_eq!(inst.link.unwrap().at, RefAt::Version(v2));
    assert_eq!(inst.index, 1, "the same instance, renumbered never");
    assert_eq!(b.linked.len(), 1, "the V1 copy nothing uses is dropped");
    assert!(!lu::staleness(&b, &lu::use_at(&b, site(i)).unwrap(), &mut latest(&mut res)).any(), "up to date: no badge");
    h.undo(&mut b).unwrap();
    close("after undo", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_25);
    assert_eq!(b, before, "undo restores the document exactly");
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER2.7, ER3.3: Selective update from V3 (82 500) back to V1 (37 500), and to V2 (60 000).
#[test]
fn selective_update_goes_back_to_an_older_version() {
    let store = temp_store("selective");
    let v1 = save_with_v1(&store, &lb::document().unwrap());
    let v2 = edit_source(&store, lb::EDITED_HEIGHT, 2_000);
    let v3 = edit_source(&store, 55.0, 3_000);
    let mut b = lb::consumer();
    let mut h = History::default();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v3);
    let snap = res.resolve(r, &b, None).unwrap();
    let i = InstanceId::from_u128(1);
    insert(&mut b, &mut h, lb::CONSUMER_ASSEMBLY, r, snap.clone(), i, part_of(&snap));
    close("at V3", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_55);
    let u = lu::use_at(&b, site(i)).unwrap();
    assert!(lu::change_for(&mut res, &b, None, &u, Target::Latest).unwrap().is_none(), "V3 is the newest: nothing to update");
    let c = lu::change_for(&mut res, &b, None, &u, Target::Version(v1)).unwrap().unwrap();
    h.execute(&mut b, &UpdateReferences { changes: vec![c], label: "Update selected".into() }).unwrap();
    close("at V1", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_25);
    let u = lu::use_at(&b, site(i)).unwrap();
    assert_eq!(lu::staleness(&b, &u, &mut latest(&mut res)).newer.map(|x| x.1), Some("V3".into()), "an older version shows the badge");
    let c = lu::change_for(&mut res, &b, None, &u, Target::Version(v2)).unwrap().unwrap();
    h.execute(&mut b, &UpdateReferences { changes: vec![c], label: "Update selected".into() }).unwrap();
    close("at V2", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_40);
    h.undo(&mut b).unwrap();
    close("undo: V1", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_25);
    h.undo(&mut b).unwrap();
    close("undo: V3", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i)), V_55);
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER5.1–ER5.3, ER5.7, ER5.8: two instances at V1, one pinned; after V2, Update all lists one
/// reference and updates only the unpinned one (60 000 + 37 500); a workspace reference can't
/// be pinned; unpinned, it is updated too.
#[test]
fn update_all_skips_a_pinned_reference() {
    let store = temp_store("pinned");
    let v1 = save_with_v1(&store, &lb::document().unwrap());
    let mut b = lb::consumer();
    let mut h = History::default();
    let mut res = Resolver::new(store.clone());
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v1);
    let snap = res.resolve(r, &b, None).unwrap();
    let (i1, i2) = (InstanceId::from_u128(1), InstanceId::from_u128(2));
    insert(&mut b, &mut h, lb::CONSUMER_ASSEMBLY, r, snap.clone(), i1, part_of(&snap));
    insert(&mut b, &mut h, lb::CONSUMER_ASSEMBLY, r, snap.clone(), i2, part_of(&snap));
    h.execute(&mut b, &SetPinned { sites: vec![site(i2)], pinned: true }).unwrap();
    assert!(lu::use_at(&b, site(i2)).unwrap().reference.pinned);
    let v2 = edit_source(&store, lb::EDITED_HEIGHT, 2_000);
    // The pinned one is out of date too (its icon changes, not to blue: the app's choice).
    assert!(lu::staleness(&b, &lu::use_at(&b, site(i2)).unwrap(), &mut latest(&mut res)).newer.is_some());
    let rows = lu::plan_update_all(&mut res, &b, None, None);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].sites, vec![site(i1)], "the pinned reference is skipped");
    assert_eq!(rows[0].to, RowTarget::Version(v2, "V2".into()));
    assert_eq!(rows[0].arrow(), "V1 ⇒ V2");
    assert_eq!(rows[0].name, lb::DOCUMENT_NAME);
    let (cmd, autos) = lu::execute_update_all(&mut res, &b, None, &rows, 3_000, "me").unwrap();
    assert!(autos.is_empty(), "no chain: no auto version");
    h.execute(&mut b, &cmd).unwrap();
    close("unpinned", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i1)), V_40);
    close("pinned", volume(&b, lb::CONSUMER_ASSEMBLY, Some(i2)), V_25);
    close("both", volume(&b, lb::CONSUMER_ASSEMBLY, None), V_40 + V_25);
    assert!(lu::use_at(&b, site(i2)).unwrap().reference.pinned, "still pinned");
    // Unpinned (ER5.7): Update all now takes it.
    h.execute(&mut b, &SetPinned { sites: vec![site(i2)], pinned: false }).unwrap();
    let rows = lu::plan_update_all(&mut res, &b, None, None);
    assert_eq!(rows.iter().flat_map(|r| r.sites.clone()).collect::<Vec<_>>(), vec![site(i2)]);
    let (cmd, _) = lu::execute_update_all(&mut res, &b, None, &rows, 3_100, "me").unwrap();
    h.execute(&mut b, &cmd).unwrap();
    close("all at V2", volume(&b, lb::CONSUMER_ASSEMBLY, None), 2.0 * V_40);
    // ER5.8: a workspace instance can't be pinned.
    let mut v = lb::versions_document().unwrap();
    let w = InstanceId::from_u128(9);
    h.execute(&mut v, &assembly::commands::InsertInstance { element: lb::VERSIONS_ASSEMBLY, instance: Instance::new(w, InstanceSource::Part { element: lb::VERSIONS_STUDIO, part: lb::PART }, Pose::IDENTITY) }).unwrap();
    let err = h.execute(&mut v, &SetPinned { sites: vec![RefSite::Instance { element: lb::VERSIONS_ASSEMBLY, instance: w }], pinned: true }).unwrap_err();
    assert_eq!(err.to_string(), lu::PIN_WORKSPACE);
    let _ = std::fs::remove_dir_all(store.root());
}

/// Document "Sub document" (B) with an assembly holding A@`va`'s block, saved with V1.
fn chain_b(store: &Store, res: &mut Resolver, va: VersionId) -> (Document, ElementId, VersionId) {
    let mut b = Document::empty("Sub document");
    b.id = cadrs_core::DocumentId::from_u128(0xb00);
    let e = cadrs_core::Element::assembly("Sub");
    let sub = e.id;
    b.elements.push(e);
    let ra = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, va);
    let snap = res.resolve(ra, &b, None).unwrap();
    let mut h = History::default();
    insert(&mut b, &mut h, sub, ra, snap.clone(), InstanceId::from_u128(1), part_of(&snap));
    let vb = save_with_v1(store, &b);
    (b, sub, vb)
}

/// ER4.1, ER4.2, ER4.5–ER4.8: A → B → C. After A's V2, C's reference to B@V1 is out of date only
/// further down (the transitive indicator); Update all from C updates B's workspace to A@V2,
/// makes B's auto version V2, and points C at it: 60 000. Undo in C goes back to B@V1 (37 500)
/// but B's auto version stays.
#[test]
fn update_all_through_a_chain_makes_an_auto_version() {
    let store = temp_store("chain");
    let va = save_with_v1(&store, &lb::document().unwrap());
    let mut res = Resolver::new(store.clone());
    let (b, sub, vb) = chain_b(&store, &mut res, va);
    let mut c = lb::consumer();
    let mut h = History::default();
    let rb = SourceRef::version(Some(b.id), sub, vb);
    let snap = res.resolve(rb, &c, None).unwrap();
    let i = InstanceId::from_u128(7);
    insert(&mut c, &mut h, lb::CONSUMER_ASSEMBLY, rb, snap.clone(), i, InstanceSource::Assembly { element: snap.root });
    close("C at B@V1", volume(&c, lb::CONSUMER_ASSEMBLY, None), V_25);
    let u = lu::use_at(&c, site(i)).unwrap();
    assert!(!lu::staleness(&c, &u, &mut latest(&mut res)).any(), "all up to date");
    edit_source(&store, lb::EDITED_HEIGHT, 2_000);
    // The transitive query: B has no newer version, but A (inside B's copy) has.
    let st = lu::staleness(&c, &u, &mut latest(&mut res));
    assert_eq!(st.newer, None, "B itself has no newer version");
    assert!(st.nested, "A, further down, has: the transitive indicator");
    assert!(lu::needs_auto_version(&mut res, b.id), "B's workspace holds A@V1");
    let rows = lu::plan_update_all(&mut res, &c, None, None);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].to, RowTarget::NewVersion);
    assert_eq!(rows[0].arrow(), "V1 ⇒ new version");
    assert_eq!(rows[0].name, "Sub document");
    let (cmd, autos) = lu::execute_update_all(&mut res, &c, None, &rows, 3_000, "me").unwrap();
    assert_eq!(autos.len(), 1);
    assert_eq!((autos[0].document, autos[0].name.as_str()), (b.id, "V2"));
    // B's workspace now references A@V2, and its V2 is an auto version.
    let b_log = HistoryLog::load(&store, b.id).unwrap().unwrap();
    assert_eq!(b_log.versions().len(), 2);
    assert!(b_log.versions()[1].auto() && !b_log.versions()[0].auto());
    let b_now = store.load(b.id).unwrap().document;
    close("B's workspace", volume(&b_now, sub, None), V_40);
    h.execute(&mut c, &cmd).unwrap();
    close("C after Update all", volume(&c, lb::CONSUMER_ASSEMBLY, None), V_40);
    let u = lu::use_at(&c, site(i)).unwrap();
    assert_eq!(u.reference.at, RefAt::Version(autos[0].version));
    assert!(!lu::staleness(&c, &u, &mut latest(&mut res)).any(), "nothing out of date any more");
    // Undo re-points C; B's auto version stays (ER4.8).
    h.undo(&mut c).unwrap();
    close("C after undo", volume(&c, lb::CONSUMER_ASSEMBLY, None), V_25);
    assert_eq!(lu::use_at(&c, site(i)).unwrap().reference.at, RefAt::Version(vb));
    let b_log = HistoryLog::load(&store, b.id).unwrap().unwrap();
    assert_eq!(b_log.versions().len(), 2, "the auto version is not undone");
    // Planned again, B@V2 exists now: an ordinary update, no second auto version.
    let rows = lu::plan_update_all(&mut res, &c, None, None);
    assert_eq!(rows[0].to, RowTarget::Version(autos[0].version, "V2".into()));
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER3.3, ER3.6: a workspace instance changed to V1 and back to the workspace: the document is
/// the same as before, byte for byte (its RON), and the frozen V1 copy is gone again.
#[test]
fn change_to_version_and_back_to_workspace_round_trips() {
    let mut doc = lb::versions_document().unwrap();
    let mut h = History::default();
    let i = InstanceId::from_u128(3);
    h.execute(&mut doc, &assembly::commands::InsertInstance { element: lb::VERSIONS_ASSEMBLY, instance: Instance::new(i, InstanceSource::Part { element: lb::VERSIONS_STUDIO, part: lb::PART }, Pose::translation([10.0, 0.0, 0.0])) }).unwrap();
    let mut log = HistoryLog::start(&doc, 1_000, "me");
    let v1 = log.create_version("V1", "", 1_001, "me");
    let before = ron::to_string(&doc).unwrap();
    let mut res = Resolver::new(temp_store("roundtrip"));
    let s = RefSite::Instance { element: lb::VERSIONS_ASSEMBLY, instance: i };
    let u = lu::use_at(&doc, s).unwrap();
    assert_eq!(u.reference.at, RefAt::Workspace);
    assert!(lu::change_for(&mut res, &doc, Some(&log), &u, Target::Workspace).unwrap().is_none());
    let c = lu::change_for(&mut res, &doc, Some(&log), &u, Target::Version(v1)).unwrap().unwrap();
    h.execute(&mut doc, &UpdateReferences { changes: vec![c], label: "Change to version".into() }).unwrap();
    assert_eq!(doc.linked.len(), 1);
    // The workspace edit no longer reaches it (ER3.4).
    lb::set_height(&mut DocHistory(&mut doc, &mut h), lb::VERSIONS_STUDIO, lb::EDITED_HEIGHT).unwrap();
    close("frozen at V1", volume(&doc, lb::VERSIONS_ASSEMBLY, Some(i)), V_25);
    log.record(&doc, Origin::Command("Edit".into()), 1_002, "me");
    let v2 = log.create_version("V2", "", 1_003, "me");
    let u = lu::use_at(&doc, s).unwrap();
    let mut this_latest = |_| log.versions().last().map(|v| (v.id(), v.name().to_string()));
    assert_eq!(lu::staleness(&doc, &u, &mut this_latest).newer.map(|x| x.0), Some(v2), "the badge in the same document");
    lb::set_height(&mut DocHistory(&mut doc, &mut h), lb::VERSIONS_STUDIO, lb::HEIGHT).unwrap();
    let c = lu::change_for(&mut res, &doc, Some(&log), &u, Target::Workspace).unwrap().unwrap();
    h.execute(&mut doc, &UpdateReferences { changes: vec![c], label: "Change to workspace".into() }).unwrap();
    assert_eq!(ron::to_string(&doc).unwrap(), before, "back to the workspace: the same document");
    // Another document's workspace is never referenced.
    let other = lu::RefUse { reference: SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v1), ..u };
    assert!(lu::change_for(&mut res, &doc, Some(&log), &other, Target::Workspace).is_err());
}

/// ER5.6, D2.10: a drawing view of the workspace changed to V1 keeps V1's 25 mm height after an
/// edit; its reference pins (Update all skips it) and, unpinned, updates to V2's 40 mm.
#[test]
fn a_drawing_reference_changes_version_pins_and_updates() {
    use cadrs_core::drawing_source::{self as ds, StudioState};
    use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Scale, View, template};
    let mut doc = lb::versions_document().unwrap();
    let mut h = History::default();
    let t = template::builtin("ANSI_A_MM.dwt").unwrap();
    let mut d = Drawing::from_template(&t, None);
    let sheet = d.sheets[0].id;
    let src = ds::live_source(&doc, lb::VERSIONS_STUDIO).unwrap();
    let part = ObjectRef { element: lb::VERSIONS_STUDIO.0, part: ds::part_key(Some(lb::PART)) };
    let mut v = View::base(part, NamedView::Front, Scale::new(1, 1), [100.0, 100.0]);
    v.source_hash = src.hash_of(part.part);
    d.apply(&DrawingOp::SetSource(src)).unwrap();
    d.apply(&DrawingOp::InsertView { sheet, view: v.clone() }).unwrap();
    let el = cadrs_core::Element::drawing("Drawing 1", d);
    let dr = el.id;
    h.execute(&mut doc, &cadrs_core::commands::InsertElement { element: el, after: None, label: "Create Drawing".into() }).unwrap();
    let mut log = HistoryLog::start(&doc, 1_000, "me");
    let v1 = log.create_version("V1", "", 1_001, "me");
    let mut res = Resolver::new(temp_store("drawing"));
    let height = |doc: &Document| {
        let d = doc.element(dr).unwrap().drawing_data().unwrap();
        let (_, view) = d.view(v.id).unwrap();
        let st = StudioState::parse(&d.source(view.reference.element).unwrap().snapshot).unwrap();
        let g = cadrs_core::views::project(&st.features, ds::view_request(&st, view)).unwrap();
        let ys: Vec<f64> = g.projection.edges.iter().flat_map(|e| e.points.iter().map(|p| p.y)).collect();
        ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min)
    };
    let ws = lu::workspace_uses(&doc, Some(dr));
    assert_eq!(ws.len(), 1, "one source: the Block studio at the workspace");
    // Change to version V1 (the drawing tab's Change to version…, D2.10).
    let c = lu::change_for(&mut res, &doc, Some(&log), &ws[0], Target::Version(v1)).unwrap().unwrap();
    h.execute(&mut doc, &UpdateReferences { changes: vec![c], label: "Change to version".into() }).unwrap();
    let u = lu::uses(&doc).into_iter().find(|u| u.site.tab() == dr).unwrap();
    assert!(ds::out_of_date(&doc, doc.element(dr).unwrap().drawing_data().unwrap()).is_empty());
    lb::set_height(&mut DocHistory(&mut doc, &mut h), lb::VERSIONS_STUDIO, lb::EDITED_HEIGHT).unwrap();
    close("V1's view after the edit", height(&doc), lb::HEIGHT);
    assert!(ds::out_of_date(&doc, doc.element(dr).unwrap().drawing_data().unwrap()).is_empty(), "a version view never goes out of date");
    log.record(&doc, Origin::Command("Edit".into()), 1_002, "me");
    log.create_version("V2", "", 1_003, "me");
    // Pinned in the Sheets pane: Update all skips it.
    h.execute(&mut doc, &SetPinned { sites: vec![u.site], pinned: true }).unwrap();
    assert!(lu::plan_update_all(&mut res, &doc, Some(&log), None).is_empty());
    h.execute(&mut doc, &SetPinned { sites: vec![u.site], pinned: false }).unwrap();
    let rows = lu::plan_update_all(&mut res, &doc, Some(&log), None);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].arrow(), "V1 ⇒ V2");
    let (cmd, _) = lu::execute_update_all(&mut res, &doc, Some(&log), &rows, 2_000, "me").unwrap();
    h.execute(&mut doc, &cmd).unwrap();
    close("V2's view", height(&doc), lb::EDITED_HEIGHT);
    h.undo(&mut doc).unwrap();
    close("undo: V1's view", height(&doc), lb::HEIGHT);
}

/// DV1.6, TD3.8: where used lists the library's documents referencing A, with their versions.
#[test]
fn where_used_lists_the_documents_that_reference_a_document() {
    let store = temp_store("whereused");
    let va = save_with_v1(&store, &lb::document().unwrap());
    let mut res = Resolver::new(store.clone());
    let (b, _, _) = chain_b(&store, &mut res, va);
    let mut c = lb::consumer();
    let mut h = History::default();
    let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, va);
    let snap = res.resolve(r, &c, None).unwrap();
    for k in 1..=2 {
        insert(&mut c, &mut h, lb::CONSUMER_ASSEMBLY, r, snap.clone(), InstanceId::from_u128(k), part_of(&snap));
    }
    store.create(&c, &DocumentMeta::new("me", 1_000)).unwrap();
    let used = lu::where_used(&store, lb::DOCUMENT);
    let got: Vec<(String, String, String, usize)> = used.iter().map(|u| (u.document_name.clone(), u.tab.clone(), u.version.clone(), u.count)).collect();
    assert_eq!(
        got,
        vec![(lb::CONSUMER_NAME.to_string(), "Assembly 1".to_string(), "V1".to_string(), 2), ("Sub document".to_string(), "Sub".to_string(), "V1".to_string(), 1)]
    );
    assert!(lu::where_used(&store, b.id).is_empty(), "nothing references B");
    let _ = std::fs::remove_dir_all(store.root());
}
