//! An organization document's log on disk: `org.log` beside its `document.ron`.
//!
//! The file holds every entry, parents first, so reading it back adds each in turn. It is
//! rewritten whole (to a temporary name, then renamed) after changes; logs of shops are small.
//! Through `cadrs_fs`, so a page keeps one too.
//!
//! It is written with its fields' names ([`cadrs_social::codec`]) and a version. A file that
//! can't be read is an error, never an empty log: the caller must not go on to save over it.

use std::io;
use std::path::{Path, PathBuf};

use cadrs_social::log::{Anyone, Entry, Log, LogId};
use serde::{Deserialize, Serialize};

/// The file name beside `document.ron`.
pub const FILE: &str = "org.log";
/// 2: written with names (1 was a pre-release layout, which isn't read).
const VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct LogFile {
    version: u32,
    log: LogId,
    entries: Vec<Entry>,
}

/// The log file in a document's folder.
pub fn path(doc_dir: &Path) -> PathBuf {
    doc_dir.join(FILE)
}

/// Writes `log` to `path`.
pub fn save(path: &Path, log: &Log) -> io::Result<()> {
    let file = LogFile {
        version: VERSION,
        log: log.id(),
        entries: log.since(&Default::default()),
    };
    let bytes = cadrs_social::codec::encode(&file);
    if let Some(dir) = path.parent() {
        cadrs_fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("log.tmp");
    cadrs_fs::write(&tmp, bytes)?;
    cadrs_fs::rename(&tmp, path)
}

/// Reads the log at `path`. Entries are checked again as they are added (signatures, order); a
/// bad one is skipped and counted.
pub fn load(path: &Path) -> io::Result<(Log, usize)> {
    let bytes = cadrs_fs::read(path)?;
    let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_owned());
    let file: LogFile = cadrs_social::codec::decode(&bytes)
        .map_err(|_| invalid("not an organization's log (damaged, or from a pre-release cadrs)"))?;
    if file.version > VERSION {
        return Err(invalid("made by a newer cadrs"));
    }
    if file.version < VERSION {
        return Err(invalid("made by a pre-release cadrs"));
    }
    let mut log = Log::new(file.log);
    let mut bad = 0;
    for entry in file.entries {
        // Entries the clock thinks are from the future were written by this device's clock
        // before it was set back; don't lose them.
        if log.insert(entry, &Anyone, u64::MAX / 2).is_err() {
            bad += 1;
        }
    }
    Ok((log, bad))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_social::keys::SecretKey;
    use cadrs_social::log::Kind;

    #[test]
    fn a_log_round_trips() {
        let dir = std::env::temp_dir().join(format!("cadrs-org-file-{}", std::process::id()));
        let key = SecretKey::generate();
        let mut log = Log::new(LogId::generate());
        for i in 0..5u8 {
            log.append(&key, Kind::Data(vec![i]), 1_000 + i as u64);
        }
        let path = path(&dir);
        save(&path, &log).unwrap();
        let (back, bad) = load(&path).unwrap();
        assert_eq!(bad, 0);
        assert_eq!(back.id(), log.id());
        assert_eq!(back.heads(), log.heads());
        assert_eq!(back.len(), 5);
        // A file that isn't one is an error (so nobody saves an empty log over it), as is one
        // a newer cadrs wrote.
        cadrs_fs::write(&path, b"\x01\x02 some earlier layout").unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let newer = cadrs_social::codec::encode(&LogFile {
            version: VERSION + 1,
            log: log.id(),
            entries: Vec::new(),
        });
        cadrs_fs::write(&path, newer).unwrap();
        assert!(load(&path).unwrap_err().to_string().contains("newer"));
        let _ = cadrs_fs::remove_dir_all(&dir);
    }
}
