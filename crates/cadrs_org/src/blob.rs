//! Bytes too large for the log, kept beside it: `blobs/<hash>` in the document's folder.
//!
//! The log names them by hash and size ([`BlobRef`]): an organization's logo, and the frozen
//! document a client sent with a request for a quote. Members' devices fetch from one another
//! what the log names and they lack ([`missing`]), each checking the bytes against the hash, so
//! a blob can come from any of them. A blob the log no longer wants (a declined request's
//! document, a logo that was replaced) is removed by [`sweep`], a day after the request was
//! declined: an acceptance written on another device at the same moment may yet come before the
//! decline in the log's order, and then the document is wanted after all.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use cadrs_social::post::Blobs;

use crate::Org;
use crate::profile::BlobRef;

/// The folder beside `org.log`.
pub const DIR: &str = "blobs";
/// The largest blob kept or sent: a request's document (as large as a letter may be).
pub const MAX_BLOB: u64 = 64 << 20;
/// The most bytes of blobs one answer carries (one blob at least, whatever its size): the asker
/// asks again for the rest.
pub const ANSWER_BUDGET: u64 = 24 << 20;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn file(doc_dir: &Path, hash: &[u8; 32]) -> PathBuf {
    doc_dir.join(DIR).join(hex(hash))
}

/// Keeps `bytes`; its name and size.
pub fn put(doc_dir: &Path, bytes: &[u8]) -> io::Result<BlobRef> {
    let blob = BlobRef::of(bytes);
    if blob.len > MAX_BLOB {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "larger than a blob may be",
        ));
    }
    let path = file(doc_dir, &blob.hash);
    if cadrs_fs::is_file(&path) {
        return Ok(blob);
    }
    cadrs_fs::create_dir_all(doc_dir.join(DIR))?;
    let tmp = path.with_extension("tmp");
    cadrs_fs::write(&tmp, bytes)?;
    cadrs_fs::rename(&tmp, &path)?;
    Ok(blob)
}

/// Keeps bytes that came from another device, if they are the blob `wanted` names.
pub fn accept(doc_dir: &Path, wanted: &BlobRef, bytes: &[u8]) -> io::Result<bool> {
    if BlobRef::of(bytes) != *wanted {
        return Ok(false);
    }
    put(doc_dir, bytes).map(|_| true)
}

/// The blob's bytes, when they are here and are what the hash says.
pub fn get(doc_dir: &Path, hash: &[u8; 32]) -> Option<Vec<u8>> {
    let bytes = cadrs_fs::read(file(doc_dir, hash)).ok()?;
    (blake3::hash(&bytes).as_bytes() == hash).then_some(bytes)
}

pub fn has(doc_dir: &Path, hash: &[u8; 32]) -> bool {
    cadrs_fs::is_file(file(doc_dir, hash))
}

impl Org {
    /// Every blob the document still wants kept: the logo (both sizes) and the documents of
    /// the requests that weren't declined (a declined request's document is let go: members'
    /// devices stop fetching it, and the app removes it).
    pub fn blobs(&self) -> BTreeSet<BlobRef> {
        let logo = self
            .profile
            .logo
            .iter()
            .flat_map(|logo| [logo.full, logo.thumb]);
        let snapshots = self
            .requests
            .values()
            .filter(|r| r.stage != crate::Stage::Declined)
            .filter_map(|r| r.snapshot_blob());
        logo.chain(snapshots).collect()
    }

    /// The blobs anyone may ask this organization's devices for: what its listing shows.
    pub fn public_blobs(&self) -> BTreeSet<BlobRef> {
        if !self.listed() {
            return BTreeSet::new();
        }
        self.profile
            .logo
            .iter()
            .flat_map(|logo| [logo.full, logo.thumb])
            .collect()
    }
}

/// How long a declined request's document is kept, in milliseconds: long enough for the
/// members' devices to agree that it was declined.
pub const KEEP_DECLINED: u64 = 24 * 60 * 60 * 1000;

/// Removes the blobs here that `org` no longer wants kept at `now` (a request's document, a day
/// after it was declined; a logo that was replaced). How many went.
pub fn sweep(doc_dir: &Path, org: &Org, now: u64) -> usize {
    let mut wanted: BTreeSet<[u8; 32]> = org.blobs().into_iter().map(|b| b.hash).collect();
    wanted.extend(
        org.requests
            .values()
            .filter(|r| {
                r.declined_at
                    .is_some_and(|at| at.saturating_add(KEEP_DECLINED) > now)
            })
            .filter_map(|r| r.snapshot_blob())
            .map(|blob| blob.hash),
    );
    let Ok(entries) = cadrs_fs::read_dir(doc_dir.join(DIR)) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let named = name.to_str().and_then(hash_of);
            named.is_some_and(|hash| !wanted.contains(&hash))
                && cadrs_fs::remove_file(entry.path()).is_ok()
        })
        .count()
}

/// The hash a blob's file name spells (64 hexadecimal digits), if it spells one.
pub fn hash_of(name: &str) -> Option<[u8; 32]> {
    if name.len() != 64 {
        return None;
    }
    let mut hash = [0u8; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(name.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(hash)
}

/// The answer to a device that asked for `wanted`: those among `allowed` (the blobs the asker
/// may have: all the organization names for a member's device, the public ones for anyone) that
/// are here, up to [`ANSWER_BUDGET`].
pub fn answer(doc_dir: &Path, wanted: &[[u8; 32]], allowed: &BTreeSet<BlobRef>) -> Blobs {
    let mut out = Vec::new();
    let mut sent = 0u64;
    for hash in wanted {
        let Some(blob) = allowed.iter().find(|b| b.hash == *hash) else {
            continue;
        };
        if !out.is_empty() && sent + blob.len > ANSWER_BUDGET {
            break;
        }
        if let Some(bytes) = get(doc_dir, hash) {
            sent += bytes.len() as u64;
            out.push(serde_bytes::ByteBuf::from(bytes));
        }
    }
    Blobs(out)
}

/// Keeps the blobs of an answer that `org` names and this device lacked. How many were kept.
pub fn take(doc_dir: &Path, org: &Org, blobs: Blobs) -> usize {
    let named = org.blobs();
    blobs
        .0
        .iter()
        .filter(|bytes| {
            let blob = BlobRef::of(bytes);
            named.contains(&blob) && !has(doc_dir, &blob.hash) && put(doc_dir, bytes).is_ok()
        })
        .count()
}

/// The blobs `org` names that aren't in `doc_dir` yet.
pub fn missing(doc_dir: &Path, org: &Org) -> Vec<BlobRef> {
    org.blobs()
        .into_iter()
        .filter(|blob| !has(doc_dir, &blob.hash))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_are_kept_by_their_hash() {
        let dir = std::env::temp_dir().join(format!("cadrs-org-blob-{}", std::process::id()));
        let _ = cadrs_fs::remove_dir_all(&dir);
        let blob = put(&dir, b"a logo").unwrap();
        assert_eq!(blob, BlobRef::of(b"a logo"));
        assert_eq!(put(&dir, b"a logo").unwrap(), blob, "twice is once");
        assert!(has(&dir, &blob.hash));
        assert_eq!(get(&dir, &blob.hash).as_deref(), Some(&b"a logo"[..]));
        assert_eq!(get(&dir, &[7; 32]), None);
        // From another device: only what was asked for.
        let wanted = BlobRef::of(b"a snapshot");
        assert!(!accept(&dir, &wanted, b"something else").unwrap());
        assert!(!has(&dir, &wanted.hash));
        assert!(accept(&dir, &wanted, b"a snapshot").unwrap());
        assert!(has(&dir, &wanted.hash));
        // An answer to another device: only what it may have, and what is here.
        let allowed = BTreeSet::from([blob]);
        let asked = [blob.hash, wanted.hash, [9; 32]];
        let answered = answer(&dir, &asked, &allowed);
        assert_eq!(answered.0.len(), 1);
        assert_eq!(answered.0[0].as_slice(), b"a logo");
        // A file that rotted isn't handed out.
        cadrs_fs::write(file(&dir, &blob.hash), b"not a logo").unwrap();
        assert_eq!(get(&dir, &blob.hash), None);
        assert!(answer(&dir, &asked, &allowed).0.is_empty());
        let _ = cadrs_fs::remove_dir_all(&dir);
    }
}
