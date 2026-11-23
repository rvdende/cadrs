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

/// Heavy items (a millisecond each, about) past which a loop is worth starting the threads
/// for: starting them costs about one item, and a few more in the first loop's waking.
const HEAVY: usize = 4;

/// Items of some weight past which a loop is worth the threads ([`threads_for_some`]).
const SOME: usize = 16;

/// Whether a loop over `n` items should run on the threads: `heavy` when each may take long
/// (a B-spline face's integrals, a numeric surface intersection). When it should, the pool is
/// started ([`start`]).
#[track_caller]
pub(crate) fn threads_for(n: usize, heavy: bool) -> bool {
    if n < 2 {
        return false;
    }
    if WARM.load(Ordering::Relaxed) {
        return true;
    }
    if (heavy && n >= HEAVY) || n >= MANY {
        start();
        return true;
    }
    false
}

/// [`threads_for`] a loop of `n` items of some weight each (a face split, a face's p-curves
/// fitted: tens of microseconds): worth the threads from [`SOME`] of them.
#[track_caller]
pub(crate) fn threads_for_some(n: usize) -> bool {
    if n < 2 {
        return false;
    }
    if WARM.load(Ordering::Relaxed) {
        return true;
    }
    if n >= SOME {
        start();
        return true;
    }
    false
}

/// Work left in a loop (by its items so far) past which the threads are worth starting: about
/// what starting them costs, with the first loop's waking.
const COLD: f64 = 1.0e-3;

/// Work left in a loop past which threads that are running are worth handing it to: about what
/// the hand-over and the wait for its end cost from a thread outside the pool.
const WARMED: f64 = 4.0e-5;

/// `f` of each of `items`, in their order: on the caller's thread while the work left looks
/// small, on rayon's threads from the item where it looks worth them. The work left is what the
/// items done took, each, times those to come, so a loop of a few microseconds never starts the
/// threads or waits on them, whatever its items are, and a loop whose first item takes a
/// millisecond has the rest side by side. (A loop inside one of the threads' own jobs is split
/// at once: that costs it nothing.)
#[track_caller]
pub(crate) fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use rayon::prelude::*;
    let n = items.len();
    if n < 2 {
        return items.iter().map(f).collect();
    }
    #[cfg(not(target_arch = "wasm32"))]
    if rayon::current_thread_index().is_some() {
        return items.par_iter().map(&f).collect();
    }
    serial_then_threads(items, f)
}

/// [`map`] for items the caller knows are `heavy` (a millisecond each, about: a fillet's edge,
/// a numeric surface intersection, a B-spline face's integrals): on the threads from the first
/// where they are running or the items are [`HEAVY`] or more, since finding out by running one
/// alone costs a whole item. Light items, and a few heavy ones before the threads are up, go as
/// [`map`]'s do.
#[track_caller]
pub(crate) fn map_heavy<T: Sync, R: Send>(
    items: &[T],
    heavy: bool,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    use rayon::prelude::*;
    if heavy && threads_for(items.len(), true) {
        #[cfg(not(target_arch = "wasm32"))]
        if rayon::current_thread_index().is_none() && rayon::current_num_threads() < 2 {
            return items.iter().map(f).collect();
        }
        return items.par_iter().map(&f).collect();
    }
    map(items, f)
}

#[track_caller]
fn serial_then_threads<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use rayon::prelude::*;
    let n = items.len();
    let mut out: Vec<R> = Vec::with_capacity(n);
    let t = web_time::Instant::now();
    for (i, item) in items.iter().enumerate() {
        out.push(f(item));
        let left = n - i - 1;
        if left < 2 {
            continue;
        }
        let ahead = t.elapsed().as_secs_f64() / (i + 1) as f64 * left as f64;
        let worth = if WARM.load(Ordering::Relaxed) {
            WARMED
        } else {
            COLD
        };
        if ahead > worth {
            start();
            // (On one thread the hand-over to it and back costs more than it saves.)
            #[cfg(not(target_arch = "wasm32"))]
            if rayon::current_num_threads() < 2 {
                out.extend(items[i + 1..].iter().map(&f));
                break;
            }
            out.par_extend(items[i + 1..].par_iter().map(&f));
            break;
        }
    }
    out
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
#[track_caller]
pub(crate) fn start() {
    // (`CADRS_PAR_DEBUG=1` says which loop started the threads: a small build's shouldn't.)
    if !WARM.load(Ordering::Relaxed) && std::env::var_os("CADRS_PAR_DEBUG").is_some() {
        eprintln!("PAR first started at {}", std::panic::Location::caller());
    }
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
