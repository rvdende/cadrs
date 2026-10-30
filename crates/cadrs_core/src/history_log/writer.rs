//! Writes history logs and documents on a thread of their own
//! ([`HistoryLog::save_in_background`], [`Store::save_in_background`]), one at a time and in
//! the order they were asked for. A write that a later one of the same file replaces before it
//! starts is skipped. [`flush`] waits until every write is done: saving or reading a log or a
//! document in the foreground does it first, so the files are always the latest.
//!
//! [`HistoryLog::save_in_background`]: super::HistoryLog::save_in_background
//! [`Store::save_in_background`]: crate::store::Store::save_in_background

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};

use crate::store::StoreError;

/// Writes a file (on the writer's thread).
type Job = Box<dyn FnOnce() -> Result<(), StoreError> + Send>;

struct Writer {
    jobs: Mutex<mpsc::Sender<(PathBuf, Job)>>,
    /// The writes asked for and not done yet.
    queued: Arc<(Mutex<usize>, Condvar)>,
}

static WRITER: OnceLock<Writer> = OnceLock::new();

fn writer() -> &'static Writer {
    WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(PathBuf, Job)>();
        let queued = Arc::new((Mutex::new(0usize), Condvar::new()));
        let counts = queued.clone();
        // Without the thread (a page has none), [`queue`] writes each log itself.
        let _ = std::thread::Builder::new()
            .name("history-writer".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let jobs: Vec<_> = std::iter::once(first).chain(rx.try_iter()).collect();
                    let n = jobs.len();
                    let last: Vec<bool> = (0..n)
                        .map(|i| !jobs[i + 1..].iter().any(|(p, _)| *p == jobs[i].0))
                        .collect();
                    for ((path, job), last) in jobs.into_iter().zip(last) {
                        if !last {
                            continue;
                        }
                        if let Err(e) = job() {
                            eprintln!("cannot save {}: {e}", path.display());
                        }
                    }
                    let (count, idle) = &*counts;
                    let mut c = count.lock().unwrap_or_else(|e| e.into_inner());
                    *c -= n;
                    if *c == 0 {
                        idle.notify_all();
                    }
                }
            });
        Writer {
            jobs: Mutex::new(tx),
            queued,
        }
    })
}

/// Writes `make()`'s text to `path` on the writer's thread.
pub(super) fn queue(
    path: PathBuf,
    make: impl FnOnce() -> Result<String, ron::Error> + Send + 'static,
) {
    let to = path.clone();
    queue_job(
        path,
        Box::new(move || {
            let text = make().map_err(|e| StoreError::Parse(e.to_string()))?;
            write(&to, &text)
        }),
    );
}

/// Runs `job`, which writes `path` (and what goes with it), on the writer's thread.
pub(crate) fn queue_job(path: PathBuf, job: Job) {
    let w = writer();
    *w.queued.0.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    let sent = w
        .jobs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send((path, job));
    if let Err(mpsc::SendError((path, job))) = sent {
        // (The thread is gone: write it here.)
        *w.queued.0.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        if let Err(e) = job() {
            eprintln!("cannot save {}: {e}", path.display());
        }
    }
}

/// Waits for every write asked for so far.
pub fn flush() {
    let Some(w) = WRITER.get() else { return };
    let (count, idle) = &*w.queued;
    let mut c = count.lock().unwrap_or_else(|e| e.into_inner());
    while *c > 0 {
        c = idle.wait(c).unwrap_or_else(|e| e.into_inner());
    }
}

/// Writes `text` to `path` through a temporary file, so a reader never sees half of it.
pub(super) fn write(path: &Path, text: &str) -> Result<(), StoreError> {
    if let Some(dir) = path.parent() {
        cadrs_fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("ron.tmp");
    cadrs_fs::write(&tmp, text.as_bytes())?;
    cadrs_fs::rename(&tmp, path)?;
    Ok(())
}
