//! P3G.3: Move to document (`derived-and-linking-gaps.md` P3G.3 "Done when"; DV4.1, ER7, ER8).
//!
//! The document is ER Ex2's stand-in ([`cadrs_core::samples::piston::move_to_document`]):
//! Hexapod, Pneumatic Piston, Piston Assembly, Topplate, Baseplate. Every expected value is
//! computed here from the dimensions (closed form), not read back from the model:
//!
//! - the piston: V = 10³ + 60π(15.725/2)² + π·3²·28 + 8³, its centroid z from each part's
//!   (−5, 30, 74, 92);
//! - the plates: π(100² − 6·5²)·10 = 98 500π and π(80² − 6·5²)·10 = 62 500π, centroids z 5 and
//!   116 + 5;
//! - the Hexapod: both plates and six pistons raised 20.
#![cfg(feature = "occt")]

use std::f64::consts::PI;
use std::sync::Arc;

use cadrs_core::assembly::{self, InstanceId, InstanceSource, structure};
use cadrs_core::external::{InsertLinked, RefAt, Resolver, SourceRef};
use cadrs_core::history_log::HistoryLog;
use cadrs_core::link_update::{self as lu, RefSite, UpdateReferences};
use cadrs_core::move_doc::{self as md, MoveTarget};
use cadrs_core::samples::piston as ps;
use cadrs_core::{Document, DocumentId, DocumentMeta, ElementId, Filter, History, SortDir, SortKey, Store};

fn temp_store(tag: &str) -> Store {
    Store::new(std::env::temp_dir().join(format!("cadrs-move-{tag}-{}-{}", std::process::id(), uuid::Uuid::new_v4())))
}

#[track_caller]
fn close(what: &str, got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() < tol, "{what}: got {got}, want {want}");
}

/// The piston's parts: (volume, centroid z) from the dimensions.
fn piston_parts() -> [(f64, f64); 4] {
    let l = 28.0;
    [
        (1000.0, -5.0),
        (60.0 * PI * (15.725f64 / 2.0).powi(2), 30.0),
        (PI * 9.0 * l, 60.0 + l / 2.0),
        (512.0, 60.0 + l + 4.0),
    ]
}

fn piston_closed_form() -> (f64, f64) {
    let p = piston_parts();
    let v: f64 = p.iter().map(|x| x.0).sum();
    (v, p.iter().map(|x| x.0 * x.1).sum::<f64>() / v)
}

fn hexapod_closed_form() -> (f64, f64) {
    let (vp, zp) = piston_closed_form();
    let base = PI * (100.0f64.powi(2) - 6.0 * 25.0) * 10.0;
    let top = PI * (80.0f64.powi(2) - 6.0 * 25.0) * 10.0;
    let v = base + top + 6.0 * vp;
    (v, (base * 5.0 + top * 121.0 + 6.0 * vp * (zp + 20.0)) / v)
}

fn build_of(doc: &Document) -> impl FnMut(ElementId) -> Option<Arc<cadrs_core::rebuild::Build>> + '_ {
    |e| doc.element(e).map(|el| cadrs_core::rebuild::build(el.features()))
}

/// Volume and centroid (x, y, z) of assembly `asm`.
fn mass(doc: &Document, asm: ElementId) -> (f64, [f64; 3]) {
    let model = doc.element(asm).unwrap().assembly_model().unwrap();
    let (parts, _) = assembly::instance_parts(doc, model, build_of(doc));
    let mut v = 0.0;
    let mut c = [0.0; 3];
    for p in &parts {
        let m = p.mass.as_ref().expect("a solid part");
        v += m.volume;
        c[0] += m.volume * m.center_of_mass.x;
        c[1] += m.volume * m.center_of_mass.y;
        c[2] += m.volume * m.center_of_mass.z;
    }
    (v, c.map(|x| x / v))
}

/// Every occurrence's world pose in assembly `asm`, by occurrence id.
fn poses(doc: &Document, asm: ElementId) -> Vec<(InstanceId, assembly::Pose)> {
    let model = doc.element(asm).unwrap().assembly_model().unwrap();
    let mut v: Vec<(InstanceId, assembly::Pose)> = structure::occurrences(doc, model).into_iter().map(|o| (o.id, o.pose)).collect();
    v.sort_by_key(|x| x.0);
    v
}

#[track_caller]
fn same_poses(a: &[(InstanceId, assembly::Pose)], b: &[(InstanceId, assembly::Pose)]) {
    assert_eq!(a.len(), b.len(), "the same occurrences");
    for ((ia, pa), (ib, pb)) in a.iter().zip(b) {
        assert_eq!(ia, ib);
        for r in 0..3 {
            assert!((pa.translation[r] - pb.translation[r]).abs() < 1e-9, "{ia:?} translation");
            for c in 0..3 {
                assert!((pa.rotation[r][c] - pb.rotation[r][c]).abs() < 1e-9, "{ia:?} rotation");
            }
        }
    }
}

fn setup(tag: &str) -> (Store, Document) {
    let store = temp_store(tag);
    let doc = ps::move_to_document().unwrap();
    store.create(&doc, &DocumentMeta::new("me", 1_000)).unwrap();
    (store, doc)
}

fn tab_names(doc: &Document) -> Vec<&str> {
    doc.elements.iter().map(|e| e.name.as_str()).collect()
}

/// ER7.3, ER8.2–ER8.6, DV4.1: moving the Piston Assembly to a new document "Pneumatic Piston"
/// takes 2 tabs (it and the Part Studio it references); the Hexapod's six instances link to the
/// new document's auto version V1; every world pose, the volume and the centroid are unchanged;
/// the new document is the user's and holds both tabs; undo puts the tabs back.
#[test]
fn moving_the_piston_assembly_takes_two_tabs_and_keeps_every_world_pose() {
    let (store, mut doc) = setup("piston");
    assert_eq!(md::referenced_tabs(&doc, &[ps::PISTON_ASSEMBLY]), vec![ps::PISTON_STUDIO], "1 referenced tab: \"2 referenced tabs will be moved\" with the assembly");
    let (hv, hz) = hexapod_closed_form();
    let (v0, c0) = mass(&doc, ps::HEXAPOD);
    close("Hexapod volume before", v0, hv, 1e-3);
    close("Hexapod centroid z before", c0[2], hz, 1e-6);
    close("centroid x", c0[0], 0.0, 1e-9);
    close("centroid y", c0[1], 0.0, 1e-9);
    let poses0 = poses(&doc, ps::HEXAPOD);
    assert_eq!(poses0.len(), 2 + 6 * 4, "2 plates and 6 × 4 piston parts");
    let before = doc.clone();
    let mut log = None;
    let tabs = [ps::PISTON_ASSEMBLY, ps::PISTON_STUDIO];
    let out = md::move_tabs(&store, &doc, &mut log, &tabs, &MoveTarget::New { name: "Pneumatic Piston".into() }, 2_000, "me").unwrap();
    assert!(log.is_none(), "no tab left behind: no version of the source");
    assert_eq!(out.ids.len(), 2);
    assert_eq!(out.summary(), "Moved 2 tabs to Pneumatic Piston");
    assert_eq!(out.version_name, "V1");
    let mut h = History::default();
    h.execute(&mut doc, &out.command).unwrap();
    assert_eq!(tab_names(&doc), ["Hexapod", "Topplate", "Baseplate"], "ER8.4: the two tabs are gone");
    let model = doc.element(ps::HEXAPOD).unwrap().assembly_model().unwrap();
    for k in 0..6 {
        let i = model.instance(ps::piston(k)).unwrap();
        let r = i.link.expect("ER7.5: a link to the new document");
        assert_eq!(r.document, Some(out.target));
        assert_eq!(r.at, RefAt::Version(out.version));
        assert_eq!(r.element, ps::PISTON_ASSEMBLY);
        assert!(!r.pinned);
        assert_eq!(i.index as usize, k + 1, "the same instance, renumbered never");
    }
    same_poses(&poses0, &poses(&doc, ps::HEXAPOD));
    let (v1, c1) = mass(&doc, ps::HEXAPOD);
    close("Hexapod volume after", v1, hv, 1e-3);
    close("Hexapod centroid z after", c1[2], hz, 1e-6);
    assert_eq!(doc.moved.len(), 2, "where each tab went (ER7.6)");
    // The new document: the user's, with both tabs and its version.
    let target = store.load(out.target).unwrap();
    assert_eq!(target.document.name, "Pneumatic Piston");
    assert_eq!(tab_names(&target.document), ["Pneumatic Piston", "Piston Assembly"], "ER8.6");
    let (lib, _) = store.list();
    let created: Vec<String> = lib.view(Filter::CreatedByMe, "me", "", SortKey::Modified, SortDir::Descending).into_iter().map(|e| e.name).collect();
    assert!(created.contains(&"Pneumatic Piston".to_string()), "ER8.5: Created by me lists it");
    let tlog = HistoryLog::load(&store, out.target).unwrap().unwrap();
    assert_eq!(tlog.versions().len(), 1);
    assert!(tlog.versions()[0].auto(), "made by the move");
    let (pv, pz) = piston_closed_form();
    let (tv, tc) = mass(&target.document, ps::PISTON_ASSEMBLY);
    close("Piston Assembly volume in the new document", tv, pv, 1e-3);
    close("its centroid z", tc[2], pz, 1e-6);
    close("13 956.271", pv, 13_956.271, 1e-3);
    close("32.263", pz, 32.263, 1e-3);
    close("589 534.041", hv, 589_534.041, 1e-3);
    close("50.348", hz, 50.348, 1e-3);
    // Undo: the tabs are back; the new document stays.
    h.undo(&mut doc).unwrap();
    assert_eq!(doc, before, "undo restores the document exactly");
    assert!(store.load(out.target).is_ok(), "the new document and its version stay");
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER7.3: the referenced Part Studio left behind: the moved Piston Assembly references it back
/// at an auto version V1 of the source; the Hexapod still reads the same values and poses.
#[test]
fn a_referenced_tab_left_behind_becomes_a_link_back() {
    let (store, mut doc) = setup("back");
    let poses0 = poses(&doc, ps::HEXAPOD);
    let mut log = None;
    let out = md::move_tabs(&store, &doc, &mut log, &[ps::PISTON_ASSEMBLY], &MoveTarget::New { name: String::new() }, 2_000, "me").unwrap();
    assert_eq!(out.target_name, "Piston Assembly", "named after the tab by default");
    let (sv, sv_name) = out.source_version.clone().expect("a version of the source");
    assert_eq!(sv_name, "V1");
    let slog = HistoryLog::load(&store, ps::MOVE_DOCUMENT).unwrap().unwrap();
    assert!(slog.version(sv).is_some_and(|v| v.auto()), "an auto version, saved");
    assert_eq!(out.links_back, 4, "the four part instances");
    let target = store.load(out.target).unwrap().document;
    let asm = target.element(ps::PISTON_ASSEMBLY).unwrap().assembly_model().unwrap();
    for i in &asm.instances {
        let r = i.link.expect("a link back");
        assert_eq!(r.document, Some(ps::MOVE_DOCUMENT));
        assert_eq!((r.at, r.element), (RefAt::Version(sv), ps::PISTON_STUDIO));
    }
    let (pv, pz) = piston_closed_form();
    let (tv, tc) = mass(&target, ps::PISTON_ASSEMBLY);
    close("the new document's assembly", tv, pv, 1e-3);
    close("its centroid z", tc[2], pz, 1e-6);
    let mut h = History::default();
    h.execute(&mut doc, &out.command).unwrap();
    assert_eq!(tab_names(&doc), ["Hexapod", "Pneumatic Piston", "Topplate", "Baseplate"]);
    same_poses(&poses0, &poses(&doc, ps::HEXAPOD));
    let (hv, hz) = hexapod_closed_form();
    let (v, c) = mass(&doc, ps::HEXAPOD);
    close("Hexapod volume through the chain", v, hv, 1e-3);
    close("centroid z", c[2], hz, 1e-6);
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER7.2: moving to an existing document makes a version there (named by the user; "V2" after
/// its V1); the Hexapod's Topplate instance links to it.
#[test]
fn moving_to_an_existing_document_creates_a_version_there() {
    let (store, mut doc) = setup("existing");
    let mut other = Document::new("Plates");
    other.id = DocumentId::from_u128(0x9157_0000_0000_0000_0000_0000_0000_0099);
    store.create(&other, &DocumentMeta::new("me", 1_100)).unwrap();
    let mut olog = HistoryLog::start(&other, 1_100, "me");
    olog.create_version("", "", 1_200, "me");
    olog.save(&store).unwrap();
    assert!(md::referenced_tabs(&doc, &[ps::TOPPLATE]).is_empty());
    let mut log = None;
    let out = md::move_tabs(&store, &doc, &mut log, &[ps::TOPPLATE], &MoveTarget::Existing { document: other.id, version_name: String::new() }, 2_000, "me").unwrap();
    assert!(!out.created);
    assert_eq!(out.version_name, "V2");
    let olog = HistoryLog::load(&store, other.id).unwrap().unwrap();
    assert_eq!(olog.versions().len(), 2);
    assert!(!olog.versions()[1].auto(), "named in the dialog");
    assert!(olog.versions()[1].description().contains("Topplate"));
    let target = store.load(other.id).unwrap().document;
    assert_eq!(tab_names(&target), ["Part Studio 1", "Assembly 1", "Topplate"]);
    let poses0 = poses(&doc, ps::HEXAPOD);
    let mut h = History::default();
    h.execute(&mut doc, &out.command).unwrap();
    let top = doc.element(ps::HEXAPOD).unwrap().assembly_model().unwrap().instance(ps::TOP_INSTANCE).unwrap().clone();
    assert_eq!(top.link.map(|r| (r.document, r.at)), Some((Some(other.id), RefAt::Version(out.version))));
    same_poses(&poses0, &poses(&doc, ps::HEXAPOD));
    let (hv, _) = hexapod_closed_form();
    close("Hexapod volume", mass(&doc, ps::HEXAPOD).0, hv, 1e-3);
    let _ = std::fs::remove_dir_all(store.root());
}

/// ER7.6: an instance of the Pneumatic Piston's UJoint at V1 of the same document keeps
/// pointing at V1 after the studio moved (1 000 mm³); the move is recorded, so the Reference
/// manager offers the new document, and "Update to the new document" re-points it there.
#[test]
fn a_same_document_version_reference_to_a_moved_tab_offers_the_new_document() {
    let (store, mut doc) = setup("er76");
    let mut log = Some(HistoryLog::start(&doc, 1_000, "me"));
    let v1 = log.as_mut().unwrap().create_version("", "", 1_001, "me");
    log.as_ref().unwrap().save(&store).unwrap();
    let mut h = History::default();
    let r = SourceRef::version(None, ps::PISTON_STUDIO, v1);
    let snap = cadrs_core::external::snapshot(&doc, r, "V1").unwrap();
    let i = InstanceId::from_u128(0x77);
    let inst = assembly::Instance::new(i, InstanceSource::Part { element: snap.root, part: ps::UJOINT }, assembly::Pose::translation([300.0, 0.0, 0.0]));
    h.execute(&mut doc, &InsertLinked { element: ps::HEXAPOD, snapshot: snap, instances: vec![inst], reference: r }).unwrap();
    let site = RefSite::Instance { element: ps::HEXAPOD, instance: i };
    let ujoint = |d: &Document| {
        let model = d.element(ps::HEXAPOD).unwrap().assembly_model().unwrap();
        let (parts, _) = assembly::instance_parts(d, model, build_of(d));
        parts.iter().filter(|p| InstanceId::of_part(p.id) == i).map(|p| p.mass.as_ref().unwrap().volume).sum::<f64>()
    };
    close("the UJoint at V1", ujoint(&doc), 1000.0, 1e-6);
    let out = md::move_tabs(&store, &doc, &mut log, &[ps::PISTON_ASSEMBLY, ps::PISTON_STUDIO], &MoveTarget::New { name: "Pneumatic Piston".into() }, 2_000, "me").unwrap();
    h.execute(&mut doc, &out.command).unwrap();
    let u = lu::use_at(&doc, site).unwrap();
    assert_eq!(u.reference, r, "it keeps pointing at V1 of this document");
    close("still 1 000 mm³", ujoint(&doc), 1000.0, 1e-6);
    let mut load = |d: DocumentId| store.load(d).ok().map(|f| f.document);
    let m = md::moved_record(&doc, &u, &mut load).expect("the studio moved");
    assert_eq!((m.document, m.to), (out.target, ps::PISTON_STUDIO));
    assert_eq!(m.document_name, "Pneumatic Piston");
    let mut res = Resolver::new(store.clone());
    let c = md::change_to_new_document(&mut res, &doc, log.as_ref(), &u, &m).unwrap();
    h.execute(&mut doc, &UpdateReferences { changes: vec![c], label: "Update to the new document".into() }).unwrap();
    let u = lu::use_at(&doc, site).unwrap();
    assert_eq!(u.reference.document, Some(out.target));
    assert_eq!(u.reference.at, RefAt::Version(out.version));
    assert!(md::moved_record(&doc, &u, &mut load).is_none(), "nothing more to offer");
    close("the UJoint from the new document", ujoint(&doc), 1000.0, 1e-6);
    let _ = std::fs::remove_dir_all(store.root());
}

/// The multi-document transaction checks before it writes: a target that can't be read, or
/// moving every tab, is refused with nothing written (no document, no version).
#[test]
fn a_refused_move_writes_nothing() {
    let (store, doc) = setup("refused");
    let mut log = None;
    let missing = DocumentId::from_u128(0xdead);
    let e = md::move_tabs(&store, &doc, &mut log, &[ps::PISTON_ASSEMBLY], &MoveTarget::Existing { document: missing, version_name: "V1".into() }, 2_000, "me").unwrap_err();
    assert_eq!(e.to_string(), "Resource does not exist");
    let all: Vec<ElementId> = doc.elements.iter().map(|e| e.id).collect();
    assert!(md::move_tabs(&store, &doc, &mut log, &all, &MoveTarget::New { name: "All".into() }, 2_000, "me").is_err());
    assert!(log.is_none(), "no version of the source");
    assert!(HistoryLog::load(&store, ps::MOVE_DOCUMENT).unwrap().is_none());
    assert_eq!(store.list().0.entries.len(), 1, "no new document");
    let _ = std::fs::remove_dir_all(store.root());
}
