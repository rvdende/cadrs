//! The GPU search: [`cadrs_router_kernels`]' relaxation compiled to SPIR-V by rust-gpu
//! (`shaders/*.spv`, embedded at build time), run through wgpu. Many nets are searched at
//! once, one distance grid each: every pass relaxes every cell of every grid, and passes run
//! until no distance falls (the wavefront has spread as far as it can). The CPU then walks
//! each grid back from its nearest target ([`crate::search::trace_back`]) with the same costs.
//!
//! The device is made once per process ([`Gpu::shared`]): making it, and the driver compiling
//! the shader, takes far longer than a search. `CADRS_ROUTER_TRACE=1` prints each batch's
//! passes and time.

use std::borrow::Cow;

use cadrs_router_kernels::{INF, Params};
use wgpu::util::DeviceExt;

use crate::negotiate::{Job, Negotiation, NetRoute, Pricing};
use crate::search::trace_back;

/// The kernels as rust-gpu built them (empty when no shader has been built yet).
static SPIRV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/router.spv"));

/// Relaxation passes recorded per submission (between checks for whether anything changed).
const PASSES: u32 = 16;
const WORKGROUP: u32 = 64;

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    max_binding: u64,
    max_groups: u32,
    /// The adapter's name (for reports).
    pub name: String,
}

fn words(p: &Params) -> [u32; 12] {
    [
        p.nx,
        p.ny,
        p.nl,
        p.slots,
        p.ortho,
        p.diag,
        p.via,
        p.wrong_way,
        p.h_layers,
        p.v_layers,
        0,
        0,
    ]
}

impl Gpu {
    /// The process's device, made on first use (`Err`: why there is none).
    pub fn shared() -> Result<&'static Gpu, String> {
        static GPU: std::sync::OnceLock<Result<Gpu, String>> = std::sync::OnceLock::new();
        GPU.get_or_init(Gpu::new).as_ref().map_err(|e| e.clone())
    }

    /// A device running the kernels, or why there is none (no shader built, no adapter).
    pub fn new() -> Result<Gpu, String> {
        // WebGPU takes no SPIR-V, and a page can't wait for an adapter.
        if cfg!(target_arch = "wasm32") {
            return Err("no GPU search in the browser".into());
        }
        if SPIRV.len() < 20 || !SPIRV.len().is_multiple_of(4) {
            return Err("no router shader built (cargo gpu build --shader-crate crates/cadrs_router_kernels --output-dir crates/cadrs_router/shaders)".into());
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = futures_lite::future::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            },
        ))
        .map_err(|e| e.to_string())?;
        let limits = adapter.limits();
        let name = adapter.get_info().name;
        let (device, queue) =
            futures_lite::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("cadrs_router"),
                required_limits: limits.clone(),
                ..Default::default()
            }))
            .map_err(|e| e.to_string())?;
        let code: Vec<u32> = SPIRV
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cadrs_router_kernels"),
            source: wgpu::ShaderSource::SpirV(Cow::Owned(code)),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("relax"),
            layout: None,
            module: &module,
            entry_point: Some("relax"),
            compilation_options: Default::default(),
            cache: None,
        });
        let layout = pipeline.get_bind_group_layout(0);
        Ok(Gpu {
            device,
            queue,
            pipeline,
            layout,
            max_binding: limits.max_storage_buffer_binding_size,
            max_groups: limits.max_compute_workgroups_per_dimension,
            name,
        })
    }

    /// How many nets one batch can search on a grid of `cells` (their distance grids in one
    /// buffer); 0 when the grid is too large for the device.
    pub fn slots(&self, cells: u32) -> u32 {
        if cells.div_ceil(WORKGROUP) > self.max_groups {
            return 0;
        }
        (self.max_binding / (cells as u64 * 4)).min(64) as u32
    }

    fn storage(&self, label: &str, data: &[u32], extra: wgpu::BufferUsages) -> wgpu::Buffer {
        // An empty binding is not allowed: one word at least.
        let data: Cow<[u32]> = if data.is_empty() {
            Cow::Owned(vec![0])
        } else {
            Cow::Borrowed(data)
        };
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::STORAGE | extra,
            })
    }

    fn read(&self, buf: &wgpu::Buffer) -> Vec<u32> {
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let out = bytemuck::cast_slice::<u8, u32>(
            &slice
                .get_mapped_range()
                .expect("the readback buffer is mapped"),
        )
        .to_vec();
        buf.unmap();
        out
    }

    /// Each search's distance grid: `searches` are (net, its source cells), at most
    /// [`Gpu::slots`] of them; `cong` is what entering each cell costs besides the move.
    pub fn distances(
        &self,
        params: &Params,
        owner: &[u32],
        via_owner: &[u32],
        cong: &[u32],
        searches: &[(u32, &[u32])],
    ) -> Vec<Vec<u32>> {
        let cells = params.cells();
        let slots = searches.len() as u32;
        let mut p = *params;
        p.slots = slots;
        let mut dist = vec![INF; (cells * slots) as usize];
        for (k, (_, sources)) in searches.iter().enumerate() {
            for &s in *sources {
                dist[k * cells as usize + s as usize] = 0;
            }
        }
        let nets: Vec<u32> = searches.iter().map(|s| s.0).collect();
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params"),
                contents: bytemuck::cast_slice(&words(&p)),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let none = wgpu::BufferUsages::empty();
        let owner_b = self.storage("owner", owner, none);
        let via_b = self.storage("via_owner", via_owner, none);
        let cong_b = self.storage("cong", cong, none);
        let nets_b = self.storage("nets", &nets, none);
        let dist_b = self.storage("dist", &dist, wgpu::BufferUsages::COPY_SRC);
        let changed_b = self.storage(
            "changed",
            &vec![0; slots as usize],
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        );
        let read_changed = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("changed-read"),
            size: slots as u64 * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("relax"),
            layout: &self.layout,
            entries: &[
                &uniform, &owner_b, &via_b, &cong_b, &nets_b, &dist_b, &changed_b,
            ]
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
        });
        let groups = cells.div_ceil(WORKGROUP);
        let zeros = vec![0u32; slots as usize];
        // Bellman–Ford needs at most as many passes as cells; a wavefront settles long before.
        let mut left = cells / PASSES + 2;
        let t0 = web_time::Instant::now();
        let mut rounds = 0;
        loop {
            rounds += 1;
            self.queue
                .write_buffer(&changed_b, 0, bytemuck::cast_slice(&zeros));
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("relax"),
                });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("relax"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                for _ in 0..PASSES {
                    pass.dispatch_workgroups(groups, slots, 1);
                }
            }
            enc.copy_buffer_to_buffer(&changed_b, 0, &read_changed, 0, slots as u64 * 4);
            self.queue.submit([enc.finish()]);
            let changed = self.read(&read_changed);
            left -= 1;
            if changed.iter().all(|c| *c == 0) || left == 0 {
                break;
            }
        }
        let read_dist = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dist-read"),
            size: (cells * slots) as u64 * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dist"),
            });
        enc.copy_buffer_to_buffer(&dist_b, 0, &read_dist, 0, (cells * slots) as u64 * 4);
        self.queue.submit([enc.finish()]);
        let all = self.read(&read_dist);
        if std::env::var_os("CADRS_ROUTER_TRACE").is_some() {
            eprintln!(
                "distances: {slots} slots, {cells} cells, {rounds} rounds of {PASSES} passes, {:.3} s",
                t0.elapsed().as_secs_f64()
            );
        }
        all.chunks(cells as usize).map(|c| c.to_vec()).collect()
    }
}

/// A net's tree being grown on the GPU, a connection per round.
struct Growing {
    job: usize,
    tree: Vec<u32>,
    left: Vec<Vec<u32>>,
    route: NetRoute,
}

/// The nets of `todo` in batches searched together: each batch's nets far enough apart
/// (their terminals' boxes, grown by `margin` cells, not overlapping) that they would not have
/// met had they been routed one after another, so the negotiation goes as on the CPU.
fn batches(neg: &Negotiation, todo: &[usize], size: usize, margin: u32) -> Vec<Vec<usize>> {
    let p = &neg.grid.params;
    let boxes: Vec<(u32, u32, u32, u32)> = todo
        .iter()
        .map(|&j| {
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
            for &c in neg.jobs[j].groups.iter().flatten() {
                let (x, y, _) = p.coords(c);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
            (
                x0.saturating_sub(margin),
                y0.saturating_sub(margin),
                x1 + margin,
                y1 + margin,
            )
        })
        .collect();
    let apart = |a: &(u32, u32, u32, u32), b: &(u32, u32, u32, u32)| {
        a.2 < b.0 || b.2 < a.0 || a.3 < b.1 || b.3 < a.1
    };
    let mut left: Vec<usize> = (0..todo.len()).collect();
    let mut out = vec![];
    while !left.is_empty() {
        let mut batch: Vec<usize> = vec![];
        left.retain(|&k| {
            if batch.len() < size && batch.iter().all(|&b| apart(&boxes[k], &boxes[b])) {
                batch.push(k);
                false
            } else {
                true
            }
        });
        out.push(batch.into_iter().map(|k| todo[k]).collect());
    }
    out
}

/// The GPU passes ([`Negotiation::run_cpu`]'s, with the nets of each batch searched at once:
/// all ripped up, then grown a connection per round together, then put back). Returns the jobs
/// still in conflict.
pub fn run(neg: &mut Negotiation, gpu: &Gpu, pricing: &Pricing) -> Vec<usize> {
    let p = neg.grid.params;
    let size = gpu.slots(p.cells()).max(1) as usize;
    // Room for a detour round the boxes: a few millimetres.
    let margin = (cadrs_eda::units::mm(3.0) / neg.grid.pitch.max(1)) as u32;
    let mut todo: Vec<usize> = (0..neg.jobs.len()).collect();
    for pass in 0..pricing.passes.max(1) {
        neg.passes = pass + 1;
        for chunk in batches(neg, &todo, size, margin) {
            let chunk = chunk.as_slice();
            for &j in chunk {
                neg.rip(j);
            }
            let costs = neg.costs();
            let cong: Vec<u32> = (0..p.cells()).map(|i| costs.cong(i)).collect();
            let mut growing: Vec<Growing> = chunk
                .iter()
                .map(|&j| {
                    let job: &Job = &neg.jobs[j];
                    let (first, rest) = job
                        .groups
                        .split_first()
                        .map_or((vec![], vec![]), |(f, r)| (f.clone(), r.to_vec()));
                    Growing {
                        job: j,
                        tree: first,
                        left: rest,
                        route: NetRoute::default(),
                    }
                })
                .collect();
            loop {
                let active: Vec<usize> = (0..growing.len())
                    .filter(|&k| !growing[k].left.is_empty())
                    .collect();
                if active.is_empty() {
                    break;
                }
                let searches: Vec<(u32, &[u32])> = active
                    .iter()
                    .map(|&k| (neg.jobs[growing[k].job].net, growing[k].tree.as_slice()))
                    .collect();
                let dists =
                    gpu.distances(&p, &neg.grid.owner, &neg.grid.via_owner, &cong, &searches);
                for (&k, dist) in active.iter().zip(&dists) {
                    let g = &mut growing[k];
                    let net = neg.jobs[g.job].net;
                    // The nearest cell of a piece not yet reached.
                    let best = g
                        .left
                        .iter()
                        .enumerate()
                        .flat_map(|(gi, cells)| cells.iter().map(move |&c| (gi, c)))
                        .filter(|&(_, c)| dist[c as usize] != INF)
                        .min_by_key(|&(_, c)| (dist[c as usize], c));
                    let path = best.and_then(|(_, c)| trace_back(&costs, net, dist, c));
                    match (best, path) {
                        (Some((gi, _)), Some(path)) => {
                            g.tree.extend(path.iter().copied());
                            g.tree.extend(g.left.remove(gi));
                            g.route.paths.push(path);
                        }
                        _ => {
                            g.route.missing = g.left.len();
                            g.left.clear();
                        }
                    }
                }
            }
            for g in growing {
                neg.put(g.job, g.route);
            }
        }
        let (nets, cells) = neg.conflicts();
        if nets.is_empty() {
            return vec![];
        }
        neg.reprice(&cells, pricing);
        todo = nets;
    }
    neg.settle()
}
