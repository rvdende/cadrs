//! CLI smoke tests: `check` on the box case (the first fixture), and `import` into a temporary
//! document store (then read back and rebuilt).
//!
//! These use the real binary (`CARGO_BIN_EXE_cadrs-solidcraft`) and the fixtures copied into
//! `cadrs_core`'s tests folder, so `cargo test -r -p cadrs_solidcraft` exercises the binary end
//! to end without touching the app's store.

use std::path::{Path, PathBuf};
use std::process::Command;

fn box_case() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadrs_core/tests/reference_parts/solidcraft/01-box")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cadrs-solidcraft")
}

#[test]
fn check_box_case_is_green() {
    let recipe = box_case().join("recipe.json");
    let out = Command::new(bin())
        .arg("check")
        .arg(&recipe)
        .arg("--json")
        .output()
        .expect("the CLI runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "check failed ({}):\n{stdout}\n{stderr}",
        out.status
    );
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("check --json is JSON");
    assert_eq!(json["summary"]["cases"], 1);
    assert_eq!(json["summary"]["passed"], 1);
    assert_eq!(json["summary"]["failed"], 0);
    assert_eq!(json["cases"][0]["case"], "01-box");
    assert_eq!(json["cases"][0]["result"], "pass");
}

#[test]
fn import_box_case_writes_a_loadable_document() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let recipe = box_case().join("recipe.json");
    let out = Command::new(bin())
        .arg("import")
        .arg(&recipe)
        .arg("--data-dir")
        .arg(dir.path())
        .arg("--user")
        .arg("tester")
        .output()
        .expect("the CLI runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "import failed ({}):\n{stdout}\n{stderr}",
        out.status
    );
    // A normal cadrs store: one document, named after the recipe, that loads and rebuilds to the
    // box's 40 x 30 x 20 = 24000 mm3.
    let store = cadrs_core::Store::new(dir.path());
    let (library, errors) = store.list();
    assert!(errors.is_empty(), "unreadable documents: {errors:?}");
    assert_eq!(library.entries.len(), 1, "one document imported");
    let entry = &library.entries[0];
    assert_eq!(entry.name, "Box extrude");
    assert_eq!(entry.meta.owned_by, "tester");
    let file = store.load(entry.id).expect("the imported document loads");
    let studio = file
        .document
        .elements
        .iter()
        .find(|e| !e.features().is_empty())
        .expect("a Part Studio with features");
    let build = cadrs_core::rebuild::build(studio.features());
    assert!(
        build.errors.is_empty(),
        "the imported part builds: {:?}",
        build.errors
    );
    let volume: f64 = build
        .parts
        .iter()
        .filter(|p| p.kind == cadrs_core::PartKind::Solid)
        .map(|p| p.mass.expect("mass properties").volume)
        .sum();
    assert!(
        (volume - 24000.0).abs() < 1.0,
        "the imported box measures {volume} mm3"
    );
}
