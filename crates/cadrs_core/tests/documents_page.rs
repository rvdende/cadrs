//! P3E.1: the documents page's data (`test-drive-gaps.md` P3E.1): bundled samples open as an
//! editable copy with the fixture's geometry, and the details panel's versions list is the
//! document's history's versions.
//!
//! The block sample (`fixtures/linked_block_standin.cadrs`) is a 50 × 30 × 25 mm box, so its
//! copy's volume is 50·30·25 = 37 500 mm³ (closed form, not read from the model).
#![cfg(feature = "occt")]

use cadrs_core::documents_page::{self, SAMPLES};
use cadrs_core::history_log::HistoryLog;
use cadrs_core::library::{AddEntry, LibraryHistory};
use cadrs_core::{Document, DocumentId, ElementKind, Library, Store};

fn temp_store(tag: &str) -> Store {
    Store::new(std::env::temp_dir().join(format!("cadrs-documents-page-{tag}-{}-{}", std::process::id(), uuid::Uuid::new_v4())))
}

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

#[test]
fn every_sample_exists_and_loads() {
    for s in SAMPLES {
        let f = Store::load_path(&s.path(&fixtures())).unwrap_or_else(|e| panic!("{}: {e}", s.key));
        assert!(!f.document.elements.is_empty(), "{}", s.key);
    }
}

#[test]
fn a_sample_copy_has_the_fixtures_volume() {
    let store = temp_store("block");
    let sample = SAMPLES.iter().find(|s| s.key == "linked_block_standin").unwrap();
    let id = DocumentId::new();
    let entry = store.copy_from_file(&sample.path(&fixtures()), id, sample.title, "me", 1_000).unwrap();
    let mut lib = Library::default();
    LibraryHistory::default().execute(&mut lib, &AddEntry { entry }).unwrap();
    let copy = store.load(id).unwrap().document;
    let studio = copy.elements.iter().find(|e| matches!(e.kind, ElementKind::PartStudio { .. })).unwrap();
    let build = cadrs_core::rebuild::build(studio.features());
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    let m = cadrs_core::parts::combined_mass(build.parts.iter()).unwrap();
    let want = 50.0 * 30.0 * 25.0;
    assert!((m.volume - want).abs() < 1e-6 * want, "volume {} want {want}", m.volume);
    std::fs::remove_dir_all(store.root()).unwrap();
}

#[test]
fn the_versions_list_matches_the_history() {
    let store = temp_store("versions");
    let mut doc = Document::new("Versioned");
    let mut log = HistoryLog::start(&doc, 100, "me");
    log.create_version("V1", "first", 200, "me");
    doc.name = "Versioned (edited)".into();
    log.record(&doc, cadrs_core::history_log::Origin::Command("Rename".into()), 300, "me");
    log.create_version("Release", "", 400, "alice");
    log.save(&store).unwrap();
    let rows = documents_page::versions(&store, doc.id).unwrap();
    let want: Vec<(String, String, i64, String)> =
        log.versions().iter().rev().map(|v| (v.name().to_string(), v.description().to_string(), v.time(), v.user().to_string())).collect();
    let got: Vec<(String, String, i64, String)> = rows.iter().map(|r| (r.name.clone(), r.description.clone(), r.time, r.user.clone())).collect();
    assert_eq!(got, want);
    assert_eq!(got[0].0, "Release");
    assert_eq!(got[1].0, "V1");
    // A document with no history has no versions.
    assert!(documents_page::versions(&store, DocumentId::new()).unwrap().is_empty());
    std::fs::remove_dir_all(store.root()).unwrap();
}
