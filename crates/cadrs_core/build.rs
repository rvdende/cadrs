//! The geometry fingerprint: a hash of the source that decides what a rebuild makes (this
//! crate, the kernel, the sketcher and the sheet metal definitions) and of the compiler, as
//! `CADRS_GEOMETRY_FINGERPRINT`. On-disk caches of built geometry are keyed by it, so a fix to a
//! feature's implementation, committed or not, never reuses a result the old code built.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let crates = manifest.parent().unwrap();
    // FNV-1a's standard 64-bit offset basis (14,695,981,039,346,656,037), the hash's starting
    // state, paired with the FNV prime 0x100000001b3 in `Fnv::write`. It is the 64-bit FNV-0
    // hash of the string "chongo <Landon Curt Noll> /\../\" (the signature of one of FNV's
    // authors), chosen only to give the hash a non-zero start; the same constant turns up in
    // hash tables, bloom filters and checksums everywhere. Nothing secret or cryptographic: FNV
    // is a fast, non-cryptographic hash, used here only to key a cache by the source it was
    // built from.
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    for dir in [
        "cadrs_core/src",
        "cadrs_kernel/src",
        "cadrs_sketch/src",
        "cadrs_sheetmetal/src",
    ] {
        let dir = crates.join(dir);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = Vec::new();
        collect(&dir, &mut files);
        files.sort();
        for f in files {
            h.write(
                f.strip_prefix(crates)
                    .unwrap_or(&f)
                    .to_string_lossy()
                    .as_bytes(),
            );
            h.write(&std::fs::read(&f).unwrap_or_default());
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
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// FNV-1a (Fowler–Noll–Vo), 64-bit: stable across Rust versions, unlike the standard hasher.
/// Not a cryptographic hash, and not meant as one: see the offset basis in `main`.
struct Fnv(u64);

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
