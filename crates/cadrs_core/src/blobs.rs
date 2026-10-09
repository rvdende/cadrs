//! File data stored with documents (the Import feature's CAD files, the Image feature's pictures).
//!
//! A **blob** is the bytes of a file, named by a hash of its content ([`hash_of`]: 32 hex
//! digits). Features refer to blobs by that hash ([`crate::import::ImportFeature::blob`]), so a
//! document stays small and the same file imported twice is stored once.
//!
//! - **On disk** each document keeps the blobs its features use in its own folder:
//!   `<root>/<uuid>/blobs/<hash>.<ext>` ([`BLOBS_DIR`], [`file_name`]). [`crate::Store::save`]
//!   writes the missing ones, [`crate::Store::load`] reads them, and
//!   [`crate::Store::copy_document`] copies the folder, so a copied document keeps its imports.
//! - **In memory** blobs live in one process-wide cache ([`insert`], [`get`]), which the rebuild
//!   reads (it only sees feature lists). The key is the content hash, so a cache shared by all
//!   documents can never hand one document another's file.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use crate::document::{Document, FeatureKind};

/// The folder in a document's directory that holds its blobs.
pub const BLOBS_DIR: &str = "blobs";

fn cache() -> &'static RwLock<HashMap<String, Arc<Vec<u8>>>> {
    static CACHE: OnceLock<RwLock<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The content hash of `bytes`: 128 bits of two FNV-1a passes with different offsets, as 32
/// lower-case hex digits. It never changes between cadrs versions (it names files on disk).
pub fn hash_of(bytes: &[u8]) -> String {
    let fnv = |seed: u64| {
        let mut h = seed;
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Mix in the length so prefixes of each other differ in both halves.
        h ^ (bytes.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
    };
    format!("{:016x}{:016x}", fnv(0xcbf2_9ce4_8422_2325), fnv(0x6c62_272e_07bb_0142))
}

/// Keeps `bytes` in the cache and returns their hash.
pub fn insert(bytes: Vec<u8>) -> String {
    let hash = hash_of(&bytes);
    if let Ok(mut c) = cache().write() {
        c.entry(hash.clone()).or_insert_with(|| Arc::new(bytes));
    }
    hash
}

/// The bytes with this hash, if they are loaded.
pub fn get(hash: &str) -> Option<Arc<Vec<u8>>> {
    cache().read().ok()?.get(hash).cloned()
}

/// Whether the bytes with this hash are loaded.
pub fn contains(hash: &str) -> bool {
    cache().read().is_ok_and(|c| c.contains_key(hash))
}

/// The file a blob is stored in: `<hash>.<ext>` (the extension is the imported file's, lower
/// case, for people browsing the folder; the hash alone identifies it).
pub fn file_name(hash: &str, ext: &str) -> String {
    let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    if ext.is_empty() { hash.to_string() } else { format!("{hash}.{ext}") }
}

/// The blobs a document's features use: (hash, extension), each once. A linked copy's features
/// count too (P3G.1): the copy rebuilds in this document, from this document's files; so do a
/// Derived feature's source studio's (it rebuilds here too).
pub fn used_by(doc: &Document) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let all = doc.elements.iter().chain(doc.standard_content.iter().map(|s| &s.element)).chain(doc.linked.iter().map(|l| &l.element));
    for el in all {
        used_in(el.features(), &mut out);
        // A PCB Studio's 3D model files (its boards' footprints and its components').
        if let Some(s) = el.pcb() {
            let boards = s.boards.iter().filter_map(|b| b.design.as_ref()).flat_map(|d| d.board.footprints.iter().map(|f| &f.footprint));
            let parts = s.components.iter().filter_map(|c| c.component.footprint.as_ref());
            for m in boards.chain(parts).flat_map(|f| &f.models) {
                if let Some(h) = &m.blob
                    && !out.iter().any(|(x, _)| x == h)
                {
                    let ext = std::path::Path::new(&m.source).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                    out.push((h.clone(), ext));
                }
            }
        }
    }
    out
}

fn used_in(features: &[crate::document::Feature], out: &mut Vec<(String, String)>) {
    for f in features {
        match &f.kind {
            FeatureKind::Import(x) if !out.iter().any(|(h, _)| *h == x.blob) => out.push((x.blob.clone(), x.extension())),
            FeatureKind::Image(x) if !out.iter().any(|(h, _)| *h == x.blob) => out.push((x.blob.clone(), x.extension())),
            FeatureKind::Derived(x) => used_in(&x.studio, out),
            _ => {}
        }
    }
}

/// Writes the blobs `doc` uses that `dir` (a document's blobs folder) doesn't have yet. Blobs
/// that aren't loaded are skipped (they may already be on disk under another extension).
pub fn save_dir(dir: &Path, doc: &Document) -> io::Result<()> {
    let used = used_by(doc);
    if used.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    for (hash, ext) in used {
        let path = dir.join(file_name(&hash, &ext));
        if path.is_file() {
            continue;
        }
        if let Some(bytes) = get(&hash) {
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, &bytes[..])?;
            std::fs::rename(&tmp, &path)?;
        }
    }
    Ok(())
}

/// Loads every blob in `dir` (a document's blobs folder) into the cache. A file whose content
/// doesn't match its name is skipped.
pub fn load_dir(dir: &Path) -> io::Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for e in entries.flatten() {
        let path = e.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if stem.len() != 32 || contains(stem) || path.extension().is_some_and(|x| x == "tmp") {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        if hash_of(&bytes) == stem {
            insert(bytes);
        }
    }
    Ok(())
}

/// Copies the blobs folder `from` to `to` (a copied document).
pub fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    let Ok(entries) = std::fs::read_dir(from) else {
        return Ok(());
    };
    std::fs::create_dir_all(to)?;
    for e in entries.flatten() {
        if e.path().is_file() {
            std::fs::copy(e.path(), to.join(e.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable_and_distinct() {
        assert_eq!(hash_of(b"abc"), hash_of(b"abc"));
        assert_ne!(hash_of(b"abc"), hash_of(b"abd"));
        assert_ne!(hash_of(b""), hash_of(b"\0"));
        assert_eq!(hash_of(b"abc").len(), 32);
        let h = insert(b"hello blob".to_vec());
        assert_eq!(get(&h).unwrap().as_slice(), b"hello blob");
        assert_eq!(file_name(&h, "STL"), format!("{h}.stl"));
    }
}
