//! GPU compute for the native B-rep kernel: `cadrs_gpu_kernels` compiled by rust-gpu to SPIR-V
//! (`shaders/f64/cadrs_gpu_kernels.spv`), run through wgpu (Vulkan, Metal or DX12).
//!
//! [`get`] opens the device once, lazily; it is `None` without a GPU that runs `f64` shaders, or
//! with `CADRS_GPU=0` in the environment. Callers keep a CPU path for that case: the kernels
//! are the same Rust (`cadrs_gpu_kernels`), so both give the same numbers to rounding.
//!
//! - [`Gpu::quad_inner`]: the inner integrals of the mass-property quadrature on a B-spline face
//!   (one job per boundary quadrature point; 14 sums come back per job).

#![recursion_limit = "512"]

use std::borrow::Cow;
use std::sync::{Mutex, OnceLock};

pub use cadrs_gpu_kernels::{NQ, QuadParams};
use wgpu::util::DeviceExt;

const SPV_F64: &[u8] = include_bytes!("../shaders/f64/cadrs_gpu_kernels.spv");
const WG: u32 = 64;

/// The device and the pipelines.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: wgpu::ComputePipeline,
    /// One submission at a time (the rebuild worker and tests may share the device).
    lock: Mutex<()>,
    pub adapter_name: String,
}

static GPU: OnceLock<Option<Gpu>> = OnceLock::new();

/// The GPU, opened on first use; `None` when there is none that runs `f64` compute shaders or
/// `CADRS_GPU=0` is set.
pub fn get() -> Option<&'static Gpu> {
    GPU.get_or_init(|| {
        // WebGPU has no f64 shaders, and a page can't wait for an adapter.
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        if std::env::var("CADRS_GPU").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off")) {
            return None;
        }
        Gpu::open()
    })
    .as_ref()
}

impl Gpu {
    fn open() -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = futures_lite::future::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            },
        ))
        .ok()?;
        if !adapter.features().contains(wgpu::Features::SHADER_F64) {
            return None;
        }
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return None;
        }
        let (device, queue) =
            futures_lite::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("cadrs_gpu"),
                required_features: wgpu::Features::SHADER_F64,
                required_limits: adapter.limits(),
                ..Default::default()
            }))
            .ok()?;
        let code: Vec<u32> = SPV_F64
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cadrs_gpu_kernels"),
            source: wgpu::ShaderSource::SpirV(Cow::Owned(code)),
        });
        let quad = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("quad_inner_f64"),
            layout: None,
            module: &module,
            entry_point: Some("quad_inner_f64"),
            compilation_options: Default::default(),
            cache: None,
        });
        Some(Gpu {
            device,
            queue,
            quad,
            lock: Mutex::new(()),
            adapter_name: info.name,
        })
    }

    /// Uploads `uniform` (binding 0) and `inputs` (bindings 1..), runs `pipe` over `n`
    /// invocations and reads back the `out_bytes` output (the last binding).
    fn run(
        &self,
        pipe: &wgpu::ComputePipeline,
        uniform: &[u8],
        inputs: &[&[u8]],
        out_bytes: u64,
        n: u32,
    ) -> Vec<u8> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let d = &self.device;
        let ub = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: uniform,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let ins: Vec<_> = inputs
            .iter()
            .map(|b| {
                // Bindings can't be empty.
                let contents: &[u8] = if b.is_empty() { &[0; 8] } else { b };
                d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents,
                    usage: wgpu::BufferUsages::STORAGE,
                })
            })
            .collect();
        let out = d.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: out_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = d.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: out_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: ub.as_entire_binding(),
        }];
        for (i, b) in ins.iter().chain([&out]).enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: i as u32 + 1,
                resource: b.as_entire_binding(),
            });
        }
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipe.get_bind_group_layout(0),
            entries: &entries,
        });
        let groups = n.div_ceil(WG).max(1);
        let (gx, gy) = if groups <= 65535 {
            (groups, 1)
        } else {
            (65535, groups.div_ceil(65535))
        };
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        enc.copy_buffer_to_buffer(&out, 0, &read, 0, out_bytes);
        self.queue.submit([enc.finish()]);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = d.poll(wgpu::PollType::wait_indefinitely());
        let bytes = slice
            .get_mapped_range()
            .expect("the readback buffer is mapped")
            .to_vec();
        read.unmap();
        bytes
    }

    /// The inner integrals of Green's theorem on a B-spline face for each job `[u0, u1, v, w]`
    /// (see `cadrs_gpu_kernels::inner_job`). `ctrl` is u-major homogeneous `(w x, w y, w z, w)`;
    /// `consts` is the shift (3), the Gauss nodes (16) and weights (16).
    pub fn quad_inner(
        &self,
        q: QuadParams,
        ctrl: &[f64],
        ku: &[f64],
        kv: &[f64],
        consts: &[f64],
        jobs: &[[f64; 4]],
    ) -> Vec<[f64; NQ]> {
        if jobs.is_empty() {
            return Vec::new();
        }
        let q = QuadParams {
            jobs: jobs.len() as u32,
            ..q
        };
        let uniform: Vec<u8> = [q.nu, q.nv, q.du, q.dv, q.nku, q.nkv, q.jobs, 0]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        let bytes = |v: &[f64]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let flat: Vec<f64> = jobs.iter().flatten().copied().collect();
        let (c, a, b, k, j) = (
            bytes(ctrl),
            bytes(ku),
            bytes(kv),
            bytes(consts),
            bytes(&flat),
        );
        let out = self.run(
            &self.quad,
            &uniform,
            &[&c, &a, &b, &k, &j],
            (jobs.len() * NQ * 8) as u64,
            jobs.len() as u32,
        );
        out.chunks_exact(NQ * 8)
            .map(|ch| {
                let mut r = [0.0; NQ];
                for (i, x) in ch.chunks_exact(8).enumerate() {
                    r[i] = f64::from_le_bytes(x.try_into().unwrap());
                }
                r
            })
            .collect()
    }
}
