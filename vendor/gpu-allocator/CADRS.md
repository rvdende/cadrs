# gpu-allocator 0.28.0, patched for cadrs

The crates.io release with one change, in `src/vulkan/mod.rs` (`Allocator::allocate`, marked
`cadrs:`): `MemoryLocation::CpuToGpu` allocations go to host-visible system memory first, not
to device-local host-visible memory (the PCIe BAR).

wgpu asks for `CpuToGpu` only for `MAP_WRITE` buffers: the staging buffers of
`Queue::write_buffer` and buffers made mapped at creation. The GPU copies out of them once, so
system memory costs it nothing, while the CPU's writes into the BAR can be uncached. On an
RTX 4090 without resizable BAR (Linux, NVIDIA 610) a 700 KB `write_buffer` took 3.4 ms (about
200 MB/s) against 0.03 ms in system memory, and Bevy writes the UI's vertices (about that size)
every frame: house_test went from 170 to 187 fps.

The root `Cargo.toml` patches crates.io's gpu-allocator with this copy. Drop it once wgpu or
gpu-allocator stage uploads in system memory (or the copy is updated with them).
