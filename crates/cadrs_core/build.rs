//! The geometry fingerprint: a hash of the source that decides what a rebuild makes (this
//! crate, the kernel, the sketcher and the sheet metal definitions), of the locked OpenCASCADE and of the compiler, as
//! `CADRS_GEOMETRY_FINGERPRINT`. On-disk caches of built geometry are keyed by it, so a fix to a
//! feature's implementation, committed or not, never reuses a result the old code built.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let crates = manifest.parent().unwrap();
    let workspace = crates.parent().unwrap();
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    for dir in ["cadrs_core/src", "cadrs_kernel/src", "cadrs_sketch/src", "cadrs_sheetmetal/src"] {
        let dir = crates.join(dir);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = Vec::new();
        collect(&dir, &mut files);
        files.sort();
        for f in files {
            h.write(f.strip_prefix(crates).unwrap_or(&f).to_string_lossy().as_bytes());
            h.write(&std::fs::read(&f).unwrap_or_default());
        }
    }
    // The OpenCASCADE bindings and build the lock file pins.
    let lock = workspace.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let text = std::fs::read_to_string(&lock).unwrap_or_default();
    for package in text.split("[[package]]") {
        if package.contains("name = \"opencascade") || package.contains("name = \"occt-sys\"") {
            h.write(package.as_bytes());
        }
    }
    // The compiler: its code generation can change floating-point results.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    if let Ok(out) = std::process::Command::new(rustc).arg("-vV").output() {
        h.write(&out.stdout);
    }
    println!("cargo:rustc-env=CADRS_GEOMETRY_FINGERPRINT={:016x}", h.0);
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// FNV-1a: stable across Rust versions, unlike the standard hasher.
struct Fnv(u64);

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
