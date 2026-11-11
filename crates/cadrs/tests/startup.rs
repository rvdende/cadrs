//! Startup smoke test: the whole app builds and runs a short scenario headless.
//!
//! Unlike the golden tests it also runs under `CADRS_SKIP_GOLDEN=1`, so a broken app build (a
//! plugin added twice, a system with conflicting parameters, a panic on entering a document)
//! fails `cargo test` even when the screenshot comparisons are skipped. It compares no images.
//! If the app cannot start a GPU renderer, it prints why and passes, as the golden tests do.
//!
//! Also here: what a file manager's *Open with* needs of the app (the app started with a model
//! file imports it; the Linux launcher entry names the files the app opens).

use std::path::Path;
use std::process::Command;

fn looks_like_missing_gpu(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    [
        "unable to find a gpu",
        "no suitable adapter",
        "failed to create wgpu adapter",
        "requestadaptererror",
        "no adapter",
        "failed to create device",
    ]
    .iter()
    .any(|p| s.contains(p))
}

#[test]
fn app_starts_and_opens_a_document() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("startup")
        .join("document_new");
    let output = Command::new(env!("CARGO_BIN_EXE_cadrs"))
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args(["--headless", "--scenario", "document_new", "--out"])
        .arg(&out)
        .env("RUST_LOG", "warn")
        .output()
        .expect("failed to start the cadrs binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && looks_like_missing_gpu(&stderr) {
        eprintln!("startup: SKIPPED, no GPU renderer available:\n{stderr}");
        return;
    }
    assert!(
        output.status.success(),
        "the app failed to start or run `document_new` ({}):\n{stderr}",
        output.status
    );
}

/// `cadrs <file.step>` (what a file manager's *Open with* runs) reads the file and asks how to
/// import it, as a drop on the documents page does.
#[test]
fn app_started_with_a_step_file_asks_how_to_import_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("startup")
        .join("open_step_at_start");
    let output = Command::new(env!("CARGO_BIN_EXE_cadrs"))
        .current_dir(&root)
        .args(["--headless", "--scenario"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/open_step_at_start.ron"))
        .arg("--out")
        .arg(&out)
        .arg(root.join("crates/cadrs_core/tests/fixtures/colored_assembly.step"))
        .env("RUST_LOG", "warn")
        .output()
        .expect("failed to start the cadrs binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && looks_like_missing_gpu(&stderr) {
        eprintln!("startup: SKIPPED, no GPU renderer available:\n{stderr}");
        return;
    }
    assert!(
        output.status.success(),
        "the app did not offer to import the STEP file it was started with ({}):\n{stderr}",
        output.status
    );
}

/// The Linux launcher entry takes a file and names every type of file the app opens; the MIME
/// package beside it gives a type to each extension the system's database has none for (a STEP
/// file was plain text to a file manager, which then never offered cadrs for it).
#[test]
fn the_launcher_entry_names_the_files_the_app_opens() {
    const DESKTOP: &str = include_str!("../../../tools/linux/cadrs.desktop");
    const MIME: &str = include_str!("../../../tools/linux/cadrs-mime.xml");
    let line = |key: &str| {
        DESKTOP
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .unwrap_or_else(|| panic!("no {key} in cadrs.desktop"))
    };
    assert!(line("Exec=").ends_with(" %f"));
    let named: Vec<&str> = line("MimeType=").split_terminator(';').collect();
    let defined: Vec<&str> = MIME
        .split("<mime-type type=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .collect();
    assert_eq!(
        defined,
        ["application/x-cadrs", "model/step", "model/x-brep"]
    );
    // (Without one a `model/…` type's icon is `model-x-generic`, which no icon theme has.)
    assert_eq!(MIME.matches("<generic-icon name=").count(), defined.len());
    // (shared-mime-info has these.)
    for t in defined
        .iter()
        .chain(&["model/iges", "model/stl", "model/gltf-binary"])
    {
        assert!(named.contains(t), "cadrs.desktop doesn't name {t}");
    }
    let known = ["iges", "igs", "stl", "glb"];
    let exts = cadrs_core::import::ImportFormat::EXTENSIONS;
    for ext in exts.iter().chain(&["cadrs"]) {
        assert!(
            known.contains(ext) || MIME.contains(&format!("<glob pattern=\"*.{ext}\"/>")),
            "no type for .{ext}"
        );
    }
}
