//! GPU feasibility spike: cadrs_gpu_kernels compiled by rust-gpu to SPIR-V, run through wgpu
//! on Vulkan, against the same Rust on the CPU (one thread, and rayon on every core).
//!
//! `cargo run -r -p cadrs_gpu` (`shaders/cadrs_gpu_kernels.spv`, and for the f64 run
//! `shaders/f64/cadrs_gpu_kernels.spv`, built with `cargo gpu build`; see the kernels' Cargo.toml).

use std::borrow::Cow;
use std::time::Instant;

use cadrs_gpu_kernels::{GridParams, Real, SurfParams, classify_point, nurbs_sample};
use rayon::prelude::*;
use wgpu::util::DeviceExt;

const SPV_F32: &[u8] = include_bytes!("../shaders/cadrs_gpu_kernels.spv");
const SPV_F64: Option<&[u8]> = include_spv_f64();
const WG: u32 = 64;

const fn include_spv_f64() -> Option<&'static [u8]> {
    #[cfg(feature = "f64")]
    {
        Some(include_bytes!("../shaders/f64/cadrs_gpu_kernels.spv"))
    }
    #[cfg(not(feature = "f64"))]
    {
        None
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    f64: bool,
}

/// GPU timings in ms: everything (upload, kernel, readback), and the kernel alone.
struct GpuTime {
    total: f64,
    kernel: f64,
}

impl Gpu {
    fn new() -> Gpu {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = futures_lite::future::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            },
        ))
        .expect("no adapter");
        let info = adapter.get_info();
        let f64 = adapter.features().contains(wgpu::Features::SHADER_F64);
        println!(
            "adapter: {} ({:?}, {}), wgpu SHADER_F64: {f64}",
            info.name, info.backend, info.driver_info
        );
        let limits = adapter.limits();
        let features = if f64 {
            wgpu::Features::SHADER_F64
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) =
            futures_lite::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("cadrs_gpu"),
                required_features: features,
                required_limits: limits,
                ..Default::default()
            }))
            .expect("no device");
        Gpu { device, queue, f64 }
    }

    fn pipeline(&self, spv: &[u8], entry: &str) -> wgpu::ComputePipeline {
        let code: Vec<u32> = spv
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(entry),
                source: wgpu::ShaderSource::SpirV(Cow::Owned(code)),
            });
        self.device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
    }

    fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Uploads `uniform` (binding 0) and `inputs` (bindings 1..), runs `entry` over `n`
    /// invocations, reads back the `out_bytes` output (the last binding).
    fn run(
        &self,
        pipe: &wgpu::ComputePipeline,
        uniform: &[u8],
        inputs: &[&[u8]],
        out_bytes: u64,
        n: u32,
    ) -> (Vec<u8>, GpuTime) {
        let t0 = Instant::now();
        let d = &self.device;
        let ub = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: uniform,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let ins: Vec<_> = inputs
            .iter()
            .map(|b| {
                d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: b,
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
        let groups = n.div_ceil(WG);
        let (gx, gy) = if groups <= 65535 {
            (groups, 1)
        } else {
            (65535, groups.div_ceil(65535))
        };
        let dispatch = |copy: bool| {
            let mut enc = d.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(gx, gy, 1);
            }
            if copy {
                enc.copy_buffer_to_buffer(&out, 0, &read, 0, out_bytes);
            }
            self.queue.submit([enc.finish()]);
        };
        dispatch(true);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.wait();
        let bytes = slice
            .get_mapped_range()
            .expect("the readback buffer is mapped")
            .to_vec();
        read.unmap();
        let total = t0.elapsed().as_secs_f64() * 1e3;
        // The kernel alone: best of three dispatches on the resident buffers.
        let mut kernel = f64::MAX;
        for _ in 0..3 {
            let t = Instant::now();
            dispatch(false);
            self.wait();
            kernel = kernel.min(t.elapsed().as_secs_f64() * 1e3);
        }
        (bytes, GpuTime { total, kernel })
    }
}

fn ms<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed().as_secs_f64() * 1e3)
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

// ---------------------------------------------------------------------------------------------
// NURBS surface

/// An 8×8 rational bicubic patch, 100 mm across, wavy, with weights between 0.5 and 1.5.
fn surface() -> (SurfParams, Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = 8;
    let mut ctrl = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (x, y) = (i as f64 * 100.0 / 7.0, j as f64 * 100.0 / 7.0);
            let z = 15.0 * ((i as f64) * 0.9).sin() * ((j as f64) * 0.7).cos();
            let w = 1.0 + 0.5 * ((i * 3 + j * 5) as f64).sin();
            ctrl.extend([w * x, w * y, w * z, w]);
        }
    }
    let k = vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0];
    (
        SurfParams {
            nu: n as u32,
            nv: n as u32,
            su: 0,
            sv: 0,
        },
        ctrl,
        k.clone(),
        k,
    )
}

fn cpu_nurbs<R: Real + Send + Sync>(
    p: &SurfParams,
    c: &[R],
    ku: &[R],
    kv: &[R],
    par: bool,
) -> Vec<R> {
    let mut out = vec![R::zero(); (p.su * p.sv * 6) as usize];
    let f = |(i, o): (usize, &mut [R])| o.copy_from_slice(&nurbs_sample(p, c, ku, kv, i as u32));
    if par {
        out.par_chunks_mut(6).enumerate().for_each(f);
    } else {
        out.chunks_mut(6).enumerate().for_each(f);
    }
    out
}

/// Max point error (mm) and normal error between two sample sets.
fn nurbs_err(a: &[f64], b: impl Iterator<Item = f64>) -> (f64, f64) {
    let (mut ep, mut en) = (0.0f64, 0.0f64);
    for (i, y) in b.enumerate() {
        let e = (a[i] - y).abs();
        if i % 6 < 3 {
            ep = ep.max(e)
        } else {
            en = en.max(e)
        }
    }
    (ep, en)
}

fn bench_nurbs(gpu: &Gpu) {
    let (mut p, c64, ku64, kv64) = surface();
    let c32: Vec<f32> = c64.iter().map(|&x| x as f32).collect();
    let k32: Vec<f32> = ku64.iter().map(|&x| x as f32).collect();
    let pipe = gpu.pipeline(SPV_F32, "nurbs_f32");
    let pipe64 = SPV_F64
        .filter(|_| gpu.f64)
        .map(|s| gpu.pipeline(s, "nurbs_f64"));
    println!("\n## NURBS 3x3 rational, 8x8 control points: points + normals (ms)");
    println!(
        "{:>12} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9} {:>9} | {:>9} {:>9}",
        "samples",
        "cpu1 f32",
        "cpu1 f64",
        "rayon f32",
        "rayon f64",
        "gpu f32",
        "kern f32",
        "Msamp/s",
        "gpu f64",
        "kern f64"
    );
    for s in [256u32, 1024, 2048] {
        p.su = s;
        p.sv = s;
        let n = s * s;
        let (r32, t1_32) = ms(|| cpu_nurbs(&p, &c32, &k32, &k32, false));
        let (r64, t1_64) = ms(|| cpu_nurbs(&p, &c64, &ku64, &kv64, false));
        let (_, tp32) = ms(|| cpu_nurbs(&p, &c32, &k32, &k32, true));
        let (_, tp64) = ms(|| cpu_nurbs(&p, &c64, &ku64, &kv64, true));
        let u = words(&[p.nu, p.nv, p.su, p.sv]);
        let _ = gpu.run(
            &pipe,
            &u,
            &[
                bytemuck::cast_slice(&c32),
                bytemuck::cast_slice(&k32),
                bytemuck::cast_slice(&k32),
            ],
            n as u64 * 24,
            n,
        ); // warm-up
        let (g, gt) = gpu.run(
            &pipe,
            &u,
            &[
                bytemuck::cast_slice(&c32),
                bytemuck::cast_slice(&k32),
                bytemuck::cast_slice(&k32),
            ],
            n as u64 * 24,
            n,
        );
        let g32: &[f32] = bytemuck::cast_slice(&g);
        let (g64t, e64) = match &pipe64 {
            Some(pp) => {
                let ins: [&[u8]; 3] = [
                    bytemuck::cast_slice(&c64),
                    bytemuck::cast_slice(&ku64),
                    bytemuck::cast_slice(&kv64),
                ];
                let _ = gpu.run(pp, &u, &ins, n as u64 * 48, n);
                let (g, t) = gpu.run(pp, &u, &ins, n as u64 * 48, n);
                (
                    Some(t),
                    Some(nurbs_err(
                        &r64,
                        bytemuck::cast_slice::<u8, f64>(&g).iter().copied(),
                    )),
                )
            }
            None => (None, None),
        };
        let f = |t: Option<f64>| t.map_or("-".into(), |t| format!("{t:.2}"));
        println!(
            "{:>12} | {t1_32:>9.2} {t1_64:>9.2} | {tp32:>9.2} {tp64:>9.2} | {:>9.2} {:>9.2} {:>9.0} | {:>9} {:>9}",
            format!("{s}x{s}"),
            gt.total,
            gt.kernel,
            n as f64 / gt.kernel / 1e3,
            f(g64t.as_ref().map(|t| t.total)),
            f(g64t.as_ref().map(|t| t.kernel))
        );
        let (gp, gn) = nurbs_err(&r64, g32.iter().map(|&x| x as f64));
        let (cp, cn) = nurbs_err(&r64, r32.iter().map(|&x| x as f64));
        let (dp, dn) = nurbs_err(
            &r32.iter().map(|&x| x as f64).collect::<Vec<_>>(),
            g32.iter().map(|&x| x as f64),
        );
        print!(
            "    max err vs CPU f64: gpu f32 point {gp:.2e} mm normal {gn:.2e}; cpu f32 point {cp:.2e} normal {cn:.2e}; gpu f32 vs cpu f32 point {dp:.2e} normal {dn:.2e}"
        );
        if let Some((a, b)) = e64 {
            print!("; gpu f64 point {a:.2e} normal {b:.2e}");
        }
        println!();
    }
}

// ---------------------------------------------------------------------------------------------
// Point-in-solid

const RMAJ: f32 = 40.0;
const RMIN: f32 = 15.0;

/// A torus (axis Z) of `2 nu nv` triangles, 9 floats each, outward-facing.
fn torus(nu: usize, nv: usize) -> Vec<f32> {
    let pt = |i: usize, j: usize| {
        let a = (i % nu) as f32 / nu as f32 * std::f32::consts::TAU;
        let b = (j % nv) as f32 / nv as f32 * std::f32::consts::TAU;
        let r = RMAJ + RMIN * b.cos();
        [r * a.cos(), r * a.sin(), RMIN * b.sin()]
    };
    let mut t = Vec::with_capacity(nu * nv * 18);
    for i in 0..nu {
        for j in 0..nv {
            let (p00, p10, p01, p11) = (pt(i, j), pt(i + 1, j), pt(i, j + 1), pt(i + 1, j + 1));
            for tri in [[p00, p10, p11], [p00, p11, p01]] {
                t.extend(tri.iter().flatten());
            }
        }
    }
    t
}

/// The (y, z) grid over the triangles' bounds: params, CSR offsets, triangle ids.
fn grid(tris: &[f32], g: u32) -> (GridParams, Vec<u32>, Vec<u32>) {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in tris.chunks(3) {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k + 1]);
            hi[k] = hi[k].max(p[k + 1]);
        }
    }
    let gp = GridParams {
        gy: g,
        gz: g,
        points: 0,
        _pad: 0,
        y0: lo[0],
        z0: lo[1],
        inv_dy: g as f32 / (hi[0] - lo[0]),
        inv_dz: g as f32 / (hi[1] - lo[1]),
    };
    let cells = (g * g) as usize;
    let mut lists = vec![Vec::new(); cells];
    for (ti, t) in tris.chunks(9).enumerate() {
        let ys = [t[1], t[4], t[7]];
        let zs = [t[2], t[5], t[8]];
        let (y0, z0) = cadrs_gpu_kernels::cell_of(
            &gp,
            ys.iter().copied().fold(f32::MAX, f32::min),
            zs.iter().copied().fold(f32::MAX, f32::min),
        );
        let (y1, z1) = cadrs_gpu_kernels::cell_of(
            &gp,
            ys.iter().copied().fold(f32::MIN, f32::max),
            zs.iter().copied().fold(f32::MIN, f32::max),
        );
        for cz in z0..=z1 {
            for cy in y0..=y1 {
                lists[(cz * g + cy) as usize].push(ti as u32);
            }
        }
    }
    let mut start = vec![0u32];
    let mut ids = Vec::new();
    for l in lists {
        ids.extend(l);
        start.push(ids.len() as u32);
    }
    (gp, start, ids)
}

fn bench_classify(gpu: &Gpu) {
    let pipe = gpu.pipeline(SPV_F32, "classify");
    // 1M query points in the torus's padded bounds (x y z _), deterministic.
    let npts = 1_000_000u32;
    let mut s = 0x2545F4914F6CDD1Du64;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 40) as f32 / (1u64 << 24) as f32
    };
    let ext = RMAJ + RMIN + 5.0;
    let pts: Vec<f32> = (0..npts)
        .flat_map(|_| {
            [
                (rnd() * 2.0 - 1.0) * ext,
                (rnd() * 2.0 - 1.0) * ext,
                (rnd() * 2.0 - 1.0) * (RMIN + 5.0),
                0.0,
            ]
        })
        .collect();
    println!("\n## Point-in-solid, torus mesh, {npts} points, ray parity + (y,z) grid (ms)");
    println!(
        "{:>8} | {:>7} {:>8} | {:>9} {:>9} | {:>9} {:>9} {:>8} | {:>8} {:>10}",
        "tris",
        "grid",
        "build",
        "cpu1",
        "rayon",
        "gpu",
        "kernel",
        "Mpts/s",
        "gpu!=cpu",
        "cpu!=exact"
    );
    for (nu, nv) in [(250, 100), (500, 200), (1000, 400)] {
        let tris = torus(nu, nv);
        let nt = tris.len() / 9;
        let g = ((nt as f64 / 2.0).sqrt() as u32).max(1);
        let ((mut gp, start, ids), tb) = ms(|| grid(&tris, g));
        gp.points = npts;
        let cls = |i: usize| {
            classify_point(
                &gp,
                &tris,
                &start,
                &ids,
                pts[i * 4],
                pts[i * 4 + 1],
                pts[i * 4 + 2],
            )
        };
        let (c1, t1) = ms(|| (0..npts as usize).map(cls).collect::<Vec<u32>>());
        let (_, tp) = ms(|| {
            (0..npts as usize)
                .into_par_iter()
                .map(cls)
                .collect::<Vec<u32>>()
        });
        let u = words(&[
            gp.gy,
            gp.gz,
            gp.points,
            0,
            gp.y0.to_bits(),
            gp.z0.to_bits(),
            gp.inv_dy.to_bits(),
            gp.inv_dz.to_bits(),
        ]);
        let ins: [&[u8]; 4] = [
            bytemuck::cast_slice(&tris),
            bytemuck::cast_slice(&start),
            bytemuck::cast_slice(&ids),
            bytemuck::cast_slice(&pts),
        ];
        let _ = gpu.run(&pipe, &u, &ins, npts as u64 * 4, npts);
        let (gb, gt) = gpu.run(&pipe, &u, &ins, npts as u64 * 4, npts);
        let gv: &[u32] = bytemuck::cast_slice(&gb);
        let diff = gv.iter().zip(&c1).filter(|(a, b)| a != b).count();
        // Against the analytic torus (differs near the surface: the mesh is facetted).
        let exact = (0..npts as usize)
            .filter(|&i| {
                let (x, y, z) = (pts[i * 4], pts[i * 4 + 1], pts[i * 4 + 2]);
                let q = (x * x + y * y).sqrt() - RMAJ;
                ((q * q + z * z < RMIN * RMIN) as u32) != c1[i]
            })
            .count();
        println!(
            "{nt:>8} | {:>7} {tb:>8.1} | {t1:>9.1} {tp:>9.1} | {:>9.1} {:>9.2} {:>8.0} | {diff:>8} {exact:>10}",
            format!("{g}x{g}"),
            gt.total,
            gt.kernel,
            npts as f64 / gt.kernel / 1e3
        );
    }
}

fn main() {
    println!("rayon threads: {}", rayon::current_num_threads());
    let gpu = Gpu::new();
    bench_nurbs(&gpu);
    bench_classify(&gpu);
}
