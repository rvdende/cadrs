//! Session snapshots of the app's rebuilds, kept in a [`BlobStore`] ([`persist`] has the
//! format): reopening a document restores its Part Studio's per-feature outputs instead of
//! rebuilding them, and an edit then recomputes only from the edited feature on.
//!
//! - **What**: after a rebuild the app asked for ([`super::request_persisted`]) that computed
//!   anything, the outputs it used (its Derived sources' too) are written once the rebuild
//!   thread has been idle for [`SAVE_AFTER`], so a run of edits writes one snapshot, not one per
//!   edit.
//! - **Key**: [`snapshot_key`]: the final feature chain key (every feature's parameters, in
//!   order), the geometry fingerprint and the format. The same features built by the same code
//!   always have the same key, on any machine, so peers may share snapshots; a fix to the
//!   rebuild's code changes the fingerprint and so every key.
//! - **When it's read**: before such a rebuild, if the session doesn't hold the outputs for its
//!   features already. A snapshot that can't be read is ignored (the features are rebuilt).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{GEOMETRY_FINGERPRINT, Rebuilder, final_key, persist};
use crate::blob_store::{BlobKey, BlobStore};
use crate::document::Feature;

/// How long the rebuild thread waits, idle, before writing the last rebuild's snapshot.
pub const SAVE_AFTER: Duration = Duration::from_secs(3);

static STORE: Mutex<Option<Arc<dyn BlobStore>>> = Mutex::new(None);

/// Keeps snapshots in `store` (`None`: none are read or written; tests and benchmarks build
/// every time). The app gives it a [`crate::blob_store::DiskStore`] next to the documents.
pub fn set_store(store: Option<Arc<dyn BlobStore>>) {
    if let Ok(mut s) = STORE.lock() {
        *s = store;
    }
}

fn store() -> Option<Arc<dyn BlobStore>> {
    STORE.lock().ok()?.clone()
}

/// The key a snapshot of `features`' rebuild is kept under.
pub fn snapshot_key(features: &[Feature]) -> BlobKey {
    BlobKey::of(&[
        b"cadrs session snapshot",
        GEOMETRY_FINGERPRINT.as_bytes(),
        &persist::FORMAT.to_le_bytes(),
        &final_key(features).to_le_bytes(),
    ])
}

/// A snapshot waiting for the rebuild thread to go idle.
pub(super) struct PendingSave {
    key: BlobKey,
    outputs: Vec<u64>,
}

impl Rebuilder {
    /// Before rebuilding `features` for the app: their outputs from the store, unless this
    /// session has them. Returns how many outputs were restored.
    pub fn restore_snapshot(&mut self, features: &[Feature]) -> usize {
        if !features.iter().any(Feature::is_part_feature) || self.entries.contains_key(&final_key(features)) {
            return 0;
        }
        let Some(store) = store() else { return 0 };
        let Some(blob) = store.get(&snapshot_key(features)) else { return 0 };
        self.restore(&blob).unwrap_or(0)
    }

    /// After rebuilding `features` for the app: what to write once the thread is idle (nothing
    /// without a store).
    pub(super) fn plan_save(&self, features: &[Feature]) -> Option<PendingSave> {
        store()?;
        features.iter().any(Feature::is_part_feature).then(|| PendingSave { key: snapshot_key(features), outputs: self.snapshot_keys(features) })
    }

    /// Writes a planned snapshot (the outputs that are still cached).
    pub(super) fn save_snapshot(&self, save: PendingSave) {
        let Some(store) = store() else { return };
        if let Ok(blob) = self.snapshot(&save.outputs) {
            store.put(&save.key, &blob);
        }
    }
}
