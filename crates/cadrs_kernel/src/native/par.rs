//! When a loop is worth rayon's threads, and rayon's pool started without waiting for them.
//!
//! Left to itself, rayon makes its global pool the first time a loop runs on it and starts a
//! thread per core before the loop goes on: about a millisecond on a machine with dozens, more
//! than a simple solid's whole build takes. So a loop runs on the threads only when its items are
//! many or heavy, or once the pool is there anyway (a heavier step before made it); and the
//! kernel makes the pool itself, before its first parallel loop ([`start`]), its threads started
//! off the caller's thread.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the threads are running (a loop has used them).
static WARM: AtomicBool = AtomicBool::new(false);

/// Items past which a loop of light ones is worth the threads.
const MANY: usize = 64;

/// Whether a loop over `n` items should run on the threads: `heavy` when each may take long
/// (a B-spline face's integrals, a numeric surface intersection). When it should, the pool is
/// started ([`start`]).
pub(crate) fn threads_for(n: usize, heavy: bool) -> bool {
    if n < 2 {
        return false;
    }
    if WARM.load(Ordering::Relaxed) {
        return true;
    }
    if heavy || n >= MANY {
        start();
        return true;
    }
    false
}

/// Makes rayon's global pool (once) with its threads started on a thread of their own, so the
/// caller goes on as soon as the pool exists: its first job waits for one thread to start, not
/// for all of them. Called before each of the kernel's parallel loops (the first does the work).
/// A pool made already (by the app, or by a loop elsewhere first) is left as it is.
///
/// Rayon's `build_global` makes the pool, handing each of its threads to the spawn handler as it
/// goes, then waits for every one to start. Here it runs on a thread of its own, its handler
/// passing the threads to another that starts them; the caller waits only until the pool is
/// being made (inside rayon's once-only initialization, which a loop of the caller's then waits
/// out: the handing over takes microseconds).
pub(crate) fn start() {
    WARM.store(true, Ordering::Relaxed);
    #[cfg(not(target_arch = "wasm32"))]
    {
        static STARTED: std::sync::Once = std::sync::Once::new();
        STARTED.call_once(start_pool);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start_pool() {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel::<()>();
    let making = tx.clone();
    let maker = std::thread::Builder::new()
        .name("rayon-start".to_owned())
        .spawn(move || {
            let mut spawner: Option<mpsc::Sender<rayon::ThreadBuilder>> = None;
            let _ = rayon::ThreadPoolBuilder::new()
                .spawn_handler(move |thread| {
                    if spawner.is_none() {
                        let (to, from) = mpsc::channel::<rayon::ThreadBuilder>();
                        let started = std::thread::Builder::new()
                            .name("rayon-spawner".to_owned())
                            .spawn(move || {
                                for thread in from {
                                    let _ = spawn(thread);
                                }
                            });
                        if started.is_ok() {
                            spawner = Some(to);
                        }
                        let _ = making.send(());
                    }
                    // (Without the spawner, started here: rayon's own way, on this thread.)
                    match &spawner {
                        Some(to) => to.send(thread).or_else(|e| spawn(e.0)),
                        None => spawn(thread),
                    }
                })
                .build_global();
            // (Also when the pool was there already, or couldn't be made.)
            let _ = tx.send(());
        });
    if maker.is_ok() {
        let _ = rx.recv();
    }
}

/// One of the pool's threads started, as rayon's default spawn does.
#[cfg(not(target_arch = "wasm32"))]
fn spawn(thread: rayon::ThreadBuilder) -> std::io::Result<()> {
    let mut b = std::thread::Builder::new();
    if let Some(name) = thread.name() {
        b = b.name(name.to_owned());
    }
    if let Some(size) = thread.stack_size() {
        b = b.stack_size(size);
    }
    b.spawn(move || thread.run()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use rayon::prelude::*;

    /// The pool made by [`super::start`] runs loops, on more than one thread where there are
    /// cores for them, and starting it again changes nothing.
    #[test]
    fn started_pool_runs_loops() {
        super::start();
        super::start();
        let sum: u64 = (0..10_000u64).into_par_iter().map(|x| x * 2).sum();
        assert_eq!(sum, 99_990_000);
        let threads = rayon::current_num_threads();
        assert!(threads >= 1);
        let ids: std::collections::HashSet<usize> = (0..threads * 64)
            .into_par_iter()
            .map(|_| {
                std::thread::sleep(std::time::Duration::from_micros(200));
                rayon::current_thread_index().unwrap_or(usize::MAX)
            })
            .collect();
        assert!(threads == 1 || ids.len() > 1, "{ids:?}");
    }
}
