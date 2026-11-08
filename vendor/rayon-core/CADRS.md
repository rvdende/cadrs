# rayon-core 1.13.0, patched for cadrs

The crates.io release with one change, in `src/registry.rs` (`default_global_registry` and
`background_spawn`, marked `cadrs:`): the global pool's worker threads are started by one
spawner thread in the background, not one after another inside the pool's creation.

Rayon makes its global pool the first time a parallel loop runs and spawns a thread per core
there before the loop can start: on a 32-thread machine about a millisecond, which a simple
solid's whole build is shorter than (brep-bench's runner took 2.3 ms for a box, 1.3 ms with
`RAYON_NUM_THREADS=1`). Now making the pool costs one spawn; a job injected meanwhile is picked
up by the first worker to start, the rest join as they come up.

The root `Cargo.toml` patches crates.io's rayon-core with this copy. Drop it if rayon starts its
workers lazily itself (or update the copy with a new release).
