//! Startup smoke test: the whole app builds and runs a short scenario headless.
//!
//! Unlike the golden tests it also runs under `CADRS_SKIP_GOLDEN=1`, so a broken app build (a
//! plugin added twice, a system with conflicting parameters, a panic on entering a document)
//! fails `cargo test` even when the screenshot comparisons are skipped. It compares no images.
//! If the app cannot start a GPU renderer, it prints why and passes, as the golden tests do.

use std::path::Path;
use std::process::Command;

fn looks_like_missing_gpu(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    ["unable to find a gpu", "no suitable adapter", "failed to create wgpu adapter", "requestadaptererror", "no adapter", "failed to create device"]
        .iter()
        .any(|p| s.contains(p))
}

#[test]
fn app_starts_and_opens_a_document() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("startup").join("document_new");
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
    assert!(output.status.success(), "the app failed to start or run `document_new` ({}):\n{stderr}", output.status);
}
