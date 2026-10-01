//! Blobs of derived data by key: built geometry that any machine can make again from the
//! document (see [`crate::rebuild::persist`]), kept so it doesn't have to be.
//!
//! The documents and the commands applied to them are the source of truth (what peers sync);
//! a blob is only ever a cache of what they build. That makes a store free to lose or prune
//! anything, and lets peers share blobs safely: a [`BlobKey`] hashes everything the blob was
//! made from, including the geometry fingerprint of the code that made it, so two machines
//! agree on a key only when they would build the same bytes.
//!
//! [`DiskStore`] keeps blobs in a folder; a network-backed store (iroh blobs) can implement
//! [`BlobStore`] the same way, or wrap a local one and fall back to peers.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A blob's key: a BLAKE3 hash of what it was made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobKey(pub [u8; 32]);

impl BlobKey {
    /// The key of the parts (in order), each length-prefixed so that their boundaries count.
    pub fn of(parts: &[&[u8]]) -> Self {
        let mut h = blake3::Hasher::new();
        for p in parts {
            h.update(&(p.len() as u64).to_le_bytes());
            h.update(p);
        }
        Self(*h.finalize().as_bytes())
    }

    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Where blobs are kept. Losing a blob is never an error: the caller builds it again.
pub trait BlobStore: Send + Sync {
    fn get(&self, key: &BlobKey) -> Option<Vec<u8>>;
    fn put(&self, key: &BlobKey, bytes: &[u8]);
}

/// Blobs as files in a folder: `<hex key>.blob`, written aside and renamed into place (a reader
/// never sees half a blob). Reading one marks it used, for [`DiskStore::prune`].
#[derive(Debug, Clone)]
pub struct DiskStore {
    dir: PathBuf,
}

impl DiskStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: &BlobKey) -> PathBuf {
        self.dir.join(format!("{}.blob", key.hex()))
    }

    /// Removes the blobs not used for `max_age`, then the least recently used ones while the
    /// folder holds more than `max_bytes`; and writes interrupted long ago.
    pub fn prune(&self, max_age: Duration, max_bytes: u64) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        let now = SystemTime::now();
        let mut kept: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        for e in entries.flatten() {
            let path = e.path();
            let Ok(meta) = e.metadata() else { continue };
            let used = meta.modified().unwrap_or(now);
            let old = now.duration_since(used).is_ok_and(|age| age > max_age);
            let blob = path.extension().is_some_and(|x| x == "blob");
            if old || (!blob && now.duration_since(used).is_ok_and(|age| age > Duration::from_secs(3600))) {
                let _ = std::fs::remove_file(&path);
            } else if blob {
                kept.push((used, meta.len(), path));
            }
        }
        let mut total: u64 = kept.iter().map(|k| k.1).sum();
        kept.sort_by_key(|k| k.0);
        for (_, len, path) in kept {
            if total <= max_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }
}

impl BlobStore for DiskStore {
    fn get(&self, key: &BlobKey) -> Option<Vec<u8>> {
        let path = self.path(key);
        let bytes = std::fs::read(&path).ok()?;
        let _ = std::fs::File::options().write(true).open(&path).and_then(|f| f.set_modified(SystemTime::now()));
        Some(bytes)
    }

    fn put(&self, key: &BlobKey, bytes: &[u8]) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.path(key);
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        if std::fs::write(&tmp, bytes).is_err() || std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_count_part_boundaries() {
        assert_ne!(BlobKey::of(&[b"ab", b"c"]), BlobKey::of(&[b"a", b"bc"]));
        assert_eq!(BlobKey::of(&[b"ab", b"c"]), BlobKey::of(&[b"ab", b"c"]));
    }

    #[test]
    fn a_disk_store_round_trips_and_prunes() {
        let dir = std::env::temp_dir().join(format!("cadrs-blobs-{}", uuid::Uuid::new_v4()));
        let s = DiskStore::new(&dir);
        let (a, b) = (BlobKey::of(&[b"a"]), BlobKey::of(&[b"b"]));
        assert_eq!(s.get(&a), None);
        s.put(&a, b"first");
        s.put(&b, b"second, longer");
        assert_eq!(s.get(&a).as_deref(), Some(&b"first"[..]));
        // Over the size limit: the least recently used goes first.
        let _ = std::fs::File::options().write(true).open(s.path(&b)).and_then(|f| f.set_modified(SystemTime::now() - Duration::from_secs(60)));
        s.prune(Duration::from_secs(3600), 10);
        assert!(s.get(&a).is_some() && s.get(&b).is_none());
        // Unused for longer than the age limit: gone.
        let _ = std::fs::File::options().write(true).open(s.path(&a)).and_then(|f| f.set_modified(SystemTime::now() - Duration::from_secs(7200)));
        s.prune(Duration::from_secs(3600), u64::MAX);
        assert!(s.get(&a).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
