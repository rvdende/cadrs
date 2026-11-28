//! When a loop is worth rayon's threads, and rayon's pool started without waiting for them.
//!
//! Left to itself, rayon makes its global pool the first time a loop runs on it and starts a
//! thread per core before the loop goes on: about a millisecond on a machine with dozens, more
//! than a simple solid's whole build takes; and once the pool is there, a loop handed to it
//! waits for a thread to wake, tens of microseconds, however small the loop. So the kernel's
//! loops go through [`map`] (and [`map_heavy`], where the items are known to take long): the
//! caller runs a loop itself while the work in it looks small, starts the threads only for a
//! loop that is worth them (the pool made off the caller's thread, [`start`]), and shares a loop
//! with them by taking its items itself as they do. [`threads_for`] and [`threads_for_some`]
//! are the older rule by a loop's size alone, for the loops that are not a plain map.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the threads are running (a loop has used them).
static WARM: AtomicBool = AtomicBool::new(false);

/// The pool's one worker is the thread that called [`run_on_this_thread`].
#[cfg(not(target_arch = "wasm32"))]
static ON_THIS_THREAD: AtomicBool = AtomicBool::new(false);

/// Items past which a loop of light ones is worth the threads.
const MANY: usize = 64;

/// The pool is the caller's own thread alone ([`run_on_this_thread`]): a loop is run straight
/// through, rayon's splitting and collecting gaining nothing.
fn alone() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        ON_THIS_THREAD.load(Ordering::Relaxed)
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

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
    if n < 2 || alone() {
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
    if n < 2 || alone() {
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
#[cfg(not(target_arch = "wasm32"))]
const COLD: f64 = 1.0e-3;

/// Work in a loop past which threads that are running are worth sharing it with: about what
/// the wait for a helper to wake costs.
#[cfg(not(target_arch = "wasm32"))]
const WARMED: f64 = 4.0e-5;

/// Items up to which a loop met for the first time, with the threads running, is shared from
/// its first item: a few items that take long are the loop that gains most from the threads
/// and loses most by one being run alone to find out, and a few that don't cost one wait.
#[cfg(not(target_arch = "wasm32"))]
const FEW: usize = 8;

/// `f` of each of `items`, in their order, on rayon's threads where that is worth it and
/// costing next to nothing where it is not:
///
/// - With no threads yet, the caller goes on alone while the work left looks small: what the
///   items done took, each, times those to come. A small build never starts the threads; a
///   loop that turns out long starts them and shares what is left ([`shared`]), and so do the
///   loops at one place once they have added up to as much.
/// - With the threads running, a loop is shared from its first item when its place in the code
///   last had items that took long ([`History`]), or is new and of a [`FEW`] items; otherwise
///   the caller starts alone, as above, and shares what is left once that looks worth a wait.
///
/// Sharing never idles the caller: it takes items itself while the threads help. (A loop
/// inside one of the threads' own jobs is split by rayon at once: that costs nothing.)
#[track_caller]
pub(crate) fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    map_heavy(items, false, f)
}

/// [`map`] for items the caller knows are `heavy` (a millisecond each, about: a fillet's edge,
/// a numeric surface intersection, a B-spline face's integrals): [`HEAVY`] or more of them
/// start the threads at once, and with the threads running any number is shared from the
/// first, since finding out by running one alone costs a whole item.
#[track_caller]
pub(crate) fn map_heavy<T: Sync, R: Send>(
    items: &[T],
    heavy: bool,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let n = items.len();
    if n < 2 || alone() {
        return items.iter().map(f).collect();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = heavy;
        items.iter().map(f).collect()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if rayon::current_thread_index().is_some() {
            return items.par_iter().map(&f).collect();
        }
        // (`CADRS_PAR_ALWAYS=1`, or part of a loop's `file:line`: that loop handed to the
        // threads whole, as rayon would, to compare with. `CADRS_PAR_SITES=1`: each loop's
        // place, items and time.)
        let switches = super::any_par_switch();
        let here = std::panic::Location::caller();
        if switches {
            static ALWAYS: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
            let place = format!("{}:{}", here.file(), here.line());
            if let Some(site) = ALWAYS.get_or_init(|| std::env::var("CADRS_PAR_ALWAYS").ok())
                && (site == "1" || place.contains(site.as_str()))
            {
                start();
                return items.par_iter().map(&f).collect();
            }
        }
        let t = web_time::Instant::now();
        let history = History::of(here);
        let mut out: Vec<R> = Vec::with_capacity(n);
        // From the first item where the items are known to take long: said to, or by the last
        // loop here; or, the threads running, where nothing is known of a few.
        let warm = WARM.load(Ordering::Relaxed);
        let known = history.per_item().map(|each| each * n as f64);
        let at_once = match known {
            _ if heavy => threads_for(n, true),
            Some(work) => work > if warm { WARMED } else { COLD },
            None => warm && n <= FEW,
        };
        if at_once {
            start();
        } else {
            // Alone while the work left looks small.
            while out.len() < n {
                out.push(f(&items[out.len()]));
                let left = n - out.len();
                let ahead = t.elapsed().as_secs_f64() / out.len() as f64 * left as f64;
                let worth = if WARM.load(Ordering::Relaxed) {
                    WARMED
                } else {
                    COLD
                };
                if left >= 2 && ahead > worth {
                    start();
                    break;
                }
            }
            if out.len() == n {
                let took = t.elapsed().as_secs_f64();
                history.set(took / n as f64);
                // Loops here that have added up to what the threads cost, each too short to
                // show it: the next ones are shared.
                if history.alone(took) > COLD {
                    start();
                }
            }
        }
        if out.len() < n {
            let alone = (out.len(), t.elapsed().as_secs_f64());
            let (helped, taken) = shared(items, &f, &mut out);
            // What an item took: by those the caller ran alone, else by all, on as many
            // threads as took part.
            let each = match alone {
                (0, _) => t.elapsed().as_secs_f64() * (helped + 1) as f64 / taken.max(1) as f64,
                (done, time) => time / done as f64,
            };
            history.set(each);
        }
        if switches && super::env_flag!("CADRS_PAR_SITES") {
            eprintln!(
                "[par] {}:{}: {n} items, {:.3} ms{}",
                here.file(),
                here.line(),
                t.elapsed().as_secs_f64() * 1e3,
                if at_once {
                    ", shared from the first"
                } else {
                    ""
                }
            );
        }
        out
    }
}

/// What an item of the loop at one place in the code took the last time it ran: a loop's items
/// are much alike from one call to the next (a face's split, an edge's strip), so the last
/// call's say whether this one is worth sharing before any of it has run.
#[cfg(not(target_arch = "wasm32"))]
struct History {
    /// Nanoseconds an item took (0: no loop yet).
    each: &'static std::sync::atomic::AtomicU32,
    /// Microseconds of loops run here by their caller alone.
    alone: &'static std::sync::atomic::AtomicU32,
}

#[cfg(not(target_arch = "wasm32"))]
impl History {
    /// The record of the loop called from `here` (places that fall on one slot share it).
    fn of(here: &'static std::panic::Location<'static>) -> Self {
        use std::sync::atomic::AtomicU32;
        static EACH: [AtomicU32; 256] = [const { AtomicU32::new(0) }; 256];
        static ALONE: [AtomicU32; 256] = [const { AtomicU32::new(0) }; 256];
        let mut key = here.line() as usize ^ ((here.column() as usize) << 7);
        for b in here.file().bytes().rev().take(12) {
            key = key.wrapping_mul(31).wrapping_add(b as usize);
        }
        let slot = key % EACH.len();
        History {
            each: &EACH[slot],
            alone: &ALONE[slot],
        }
    }

    /// Seconds an item took, if a loop here has run.
    fn per_item(&self) -> Option<f64> {
        match self.each.load(Ordering::Relaxed) {
            0 => None,
            ns => Some(ns as f64 * 1e-9),
        }
    }

    fn set(&self, seconds: f64) {
        let ns = (seconds * 1e9).clamp(1.0, u32::MAX as f64) as u32;
        self.each.store(ns, Ordering::Relaxed);
    }

    /// The seconds of loops run here by their caller alone, this one's `seconds` with them.
    fn alone(&self, seconds: f64) -> f64 {
        let us = (seconds * 1e6).clamp(0.0, 1e9) as u32;
        let before = self.alone.fetch_add(us, Ordering::Relaxed);
        before.saturating_add(us) as f64 * 1e-6
    }
}

/// The items of the loop from `out.len()` on, their values pushed onto `out` in order: taken
/// in turn by the caller and by the pool's threads ([`start`] made them). The caller starts on
/// the next item at once and asks one thread to help, which asks another while items are left:
/// a loop done before the helper wakes waits only for that one waking. Returns how many
/// threads helped and how many items the loop had left.
#[cfg(not(target_arch = "wasm32"))]
fn shared<T: Sync, R: Send>(
    items: &[T],
    f: &(impl Fn(&T) -> R + Sync),
    out: &mut Vec<R>,
) -> (usize, usize) {
    use std::sync::atomic::AtomicUsize;
    let (from, n) = (out.len(), items.len());
    let threads = rayon::current_num_threads();
    if n - from < 2 || threads < 2 {
        out.extend(items[from..].iter().map(f));
        return (0, n - from);
    }
    let values: Vec<std::sync::Mutex<Option<R>>> =
        (from..n).map(|_| std::sync::Mutex::new(None)).collect();
    let next = AtomicUsize::new(from);
    let helped = AtomicUsize::new(0);
    let spare = AtomicUsize::new((n - from - 1).min(threads - 1));
    // Takes items until none is left.
    let work = || loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        if i >= n {
            break;
        }
        let value = f(&items[i]);
        if let Ok(mut slot) = values[i - from].lock() {
            *slot = Some(value);
        }
    };
    // One of the pool's threads asked to help, and to ask the next while items are left.
    fn ask<'s>(
        scope: &rayon::Scope<'s>,
        spare: &'s AtomicUsize,
        next: &'s AtomicUsize,
        helped: &'s AtomicUsize,
        n: usize,
        work: &'s (impl Fn() + Sync),
    ) {
        let one_less = |x: usize| x.checked_sub(1);
        if spare
            .try_update(Ordering::Relaxed, Ordering::Relaxed, one_less)
            .is_err()
        {
            return;
        }
        scope.spawn(move |scope| {
            if next.load(Ordering::Relaxed) < n {
                helped.fetch_add(1, Ordering::Relaxed);
                if next.load(Ordering::Relaxed) + 1 < n {
                    ask(scope, spare, next, helped, n, work);
                }
                work();
            }
        });
    }
    rayon::in_place_scope(|scope| {
        ask(scope, &spare, &next, &helped, n, &work);
        work();
    });
    out.extend(values.into_iter().map(|slot| {
        slot.into_inner()
            .ok()
            .flatten()
            .expect("each item's value was written")
    }));
    (helped.load(Ordering::Relaxed), n - from)
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

/// For a host that calls the kernel from this one thread on one core (an adapter serving a test
/// suite's requests on a core of its own): makes this thread the parallel pool's only worker, so
/// the kernel's parallel loops run here, inline, rather than each being handed to a thread of
/// their own on the same core and waited for. Call it before anything else uses the kernel, from
/// a thread that lives as long as the process; it does nothing (and returns false) with more
/// than one core, or once the pool is made. (Any other thread's parallel work would wait for
/// this one to take it: not for a host with threads of its own calling the kernel.)
pub fn run_on_this_thread() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if std::thread::available_parallelism().map_or(1, |n| n.get()) != 1 {
            return false;
        }
        let made = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .use_current_thread()
            .build_global()
            .is_ok();
        if made {
            ON_THIS_THREAD.store(true, Ordering::Relaxed);
        }
        made
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
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

/// The kept caches' emptying functions, each registered as its cache is first made.
static CLEARERS: std::sync::Mutex<Vec<fn()>> = std::sync::Mutex::new(Vec::new());

/// Registers a kept cache's emptying function ([`clear_caches`]).
pub(crate) fn kept_cache(clear: fn()) {
    CLEARERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(clear);
}

/// Empties every kept cache (p-curve fits, intersections, polygons, meshes, gaps, mass
/// integrals): for a caller that starts on unrelated work and wants none of the old kept
/// (a test suite's next case), freeing their memory now rather than as they turn over.
pub fn clear_caches() {
    let clearers = CLEARERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for c in clearers {
        c();
    }
}

/// Drops `x` on the pool, off the caller's thread: a cache's old generation, thousands of
/// allocations, freed while the operation that filled it goes on (in a page, here and now).
pub(crate) fn discard<T: Send + 'static>(x: T) {
    // (With the pool on the caller's own thread ([`run_on_this_thread`]) a spawned job runs only
    // when that thread waits on the pool, which it never does: dropped here.)
    #[cfg(not(target_arch = "wasm32"))]
    if ON_THIS_THREAD.load(Ordering::Relaxed) {
        drop(x);
    } else {
        rayon::spawn(move || drop(x));
    }
    #[cfg(target_arch = "wasm32")]
    drop(x);
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

    /// [`super::map`] gives each item's value in the items' order whether the loop stays on the
    /// caller's thread (light items), goes to the threads part way (items that take long), or
    /// is known to be heavy from the start.
    #[test]
    fn map_keeps_the_order_on_either_side_of_the_hand_over() {
        let items: Vec<u64> = (0..200).collect();
        let light = super::map(&items, |x| x * 3);
        assert_eq!(light, items.iter().map(|x| x * 3).collect::<Vec<_>>());
        // (Each item a fifth of a millisecond: the work left is past what the threads cost
        // after the first few.)
        let slow = |x: &u64| {
            std::thread::sleep(std::time::Duration::from_micros(200));
            x + 1
        };
        let some: Vec<u64> = (0..40).collect();
        assert_eq!(super::map(&some, slow), (1..=40).collect::<Vec<_>>());
        assert_eq!(
            super::map_heavy(&some, true, slow),
            (1..=40).collect::<Vec<_>>()
        );
        assert_eq!(
            super::map_heavy(&some, false, |x| x + 1),
            (1..=40).collect::<Vec<_>>()
        );
        // (None, and one: nothing to hand over.)
        assert!(super::map(&[] as &[u64], |x| *x).is_empty());
        assert_eq!(super::map(&[7u64], |x| x * 2), vec![14]);
    }

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
