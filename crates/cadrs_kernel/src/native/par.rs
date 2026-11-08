//! When a loop is worth rayon's threads. Rayon's pool is made the first time a loop runs on
//! it and spawns a thread per core: about a millisecond on a machine with dozens, more than a
//! simple solid's whole build takes. So a loop runs on the threads only when its items are many
//! or heavy, or once the pool is there anyway (a heavier step before made it); else in order.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the threads are running (a loop has used them).
static WARM: AtomicBool = AtomicBool::new(false);

/// Items past which a loop of light ones is worth the threads.
const MANY: usize = 64;

/// Whether a loop over `n` items should run on the threads: `heavy` when each may take long
/// (a B-spline face's integrals, a numeric surface intersection).
pub(crate) fn threads_for(n: usize, heavy: bool) -> bool {
    if n < 2 {
        return false;
    }
    if WARM.load(Ordering::Relaxed) {
        return true;
    }
    if heavy || n >= MANY {
        WARM.store(true, Ordering::Relaxed);
        return true;
    }
    false
}
