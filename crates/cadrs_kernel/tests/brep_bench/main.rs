//! brep-bench (https://brepbench.org): every case, built as its cadrs runner builds it and graded
//! as its harness grades it, one test each (`cases`, written by `tools/brep_bench_sync.py` from a
//! brep-bench checkout, with the runner's code: `bench`, `shared`); and the kernel's own
//! regressions of cases it once got wrong (`regressions`).
//!
//! A case passes when it builds within `BREP_TIMEOUT` seconds (10), its result is a solid whose
//! volume and area are within 1e-6 of the case's (relative; a case no two kernels agree on yet,
//! unsettled, has none to meet), its triangles close (every edge used once each way) and bound
//! its volume to 2e-2, and it isn't a mesh: at least 50 faces, half of them flat triangles, is
//! the board's mark of a faceted result, which an exact kernel never needs. Each prints
//! `PASS case time` (with `--nocapture`).

mod regressions;

#[allow(dead_code, unused_imports, clippy::all, clippy::pedantic)]
mod bench;
#[allow(dead_code, unused_imports, clippy::all, clippy::pedantic)]
mod cases;
#[allow(dead_code, unused_imports, clippy::all, clippy::pedantic)]
mod shared;

use cadrs_kernel::native::curve::Curve;
use cadrs_kernel::native::surface::Surface;
use cadrs_kernel::*;
use std::time::{Duration, Instant};

/// Why a case wasn't built (the runner's refusal, or a kernel error).
#[derive(Debug)]
pub struct Refusal(String);

impl From<KernelError> for Refusal {
    fn from(e: KernelError) -> Self {
        Refusal(e.to_string())
    }
}

impl From<&str> for Refusal {
    fn from(why: &str) -> Self {
        Refusal(why.to_owned())
    }
}

impl From<String> for Refusal {
    fn from(why: String) -> Self {
        Refusal(why)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

type Build = fn(&mut DefaultKernel) -> std::result::Result<BodyId, Refusal>;

/// How near a measure must be to the expected value, relative to it (the harness's `EXACT`).
const EXACT: f64 = 1e-6;
/// How near the triangles' volume must be (the harness's `MESHED`).
const MESHED: f64 = 2e-2;
/// The fewest faces of a result the board marks faceted (half of them flat triangles).
const FACETED_LEAST: usize = 50;

/// One case at a time (each may use every core; its time is its own).
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Builds case `id` (the test `test`) and grades it (see the module docs), panicking with why it
/// fails. It runs in a process of its own (this test binary run for that one test), which is
/// killed when it runs over the time allowed.
pub fn check(test: &str, id: &str, build: Build, expect: Option<(f64, f64)>) {
    if std::env::var_os("BREP_CASE_CHILD").is_some() {
        let mut k = DefaultKernel::new();
        match build(&mut k).and_then(|b| graded(&k, b, expect)) {
            Ok(note) => {
                println!("BREP_PASS{note}");
                return;
            }
            Err(why) => panic!("{why}"),
        }
    }
    let limit: f64 = std::env::var("BREP_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10.0);
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let start = Instant::now();
    let mut child = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env("BREP_CASE_CHILD", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the case's process starts");
    let status = loop {
        if let Some(s) = child.try_wait().expect("the case's process is waited for") {
            break Some(s);
        }
        if start.elapsed().as_secs_f64() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let took = format!("{:.1} ms", start.elapsed().as_secs_f64() * 1e3);
    let Some(status) = status else {
        panic!("{id}: over {limit} s");
    };
    let out = child.wait_with_output().expect("the case's output reads");
    let stdout = String::from_utf8_lossy(&out.stdout);
    if status.success()
        && let Some(note) = stdout
            .lines()
            .find_map(|l| l.split_once("BREP_PASS").map(|(_, n)| n))
    {
        println!("PASS {id} {took}{note}");
        return;
    }
    // The child's panic message: the line after "panicked at".
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = stderr.lines().chain(stdout.lines()).collect();
    let why = lines
        .iter()
        .position(|l| l.contains("panicked at"))
        .and_then(|i| lines.get(i + 1))
        .map_or_else(|| format!("crashed ({status})"), |l| l.trim().to_string());
    panic!("{id}: {why} (in {took})");
}

/// Body `b` graded: Ok with a note on what it is, else why it fails.
fn graded(
    k: &DefaultKernel,
    b: BodyId,
    expect: Option<(f64, f64)>,
) -> std::result::Result<String, Refusal> {
    match k.body_class(b)? {
        BodyClass::Solid => {}
        other => return Err(format!("the result is not a solid but {other:?}").into()),
    }
    let m = k.mass_properties(b)?;
    let (volume, area) = (m.volume, m.surface_area);
    // A mesh: many faces, half of them flat triangles.
    let body = k.get(b)?;
    let triangles = body
        .faces
        .iter()
        .filter(|f| {
            matches!(f.surf, Surface::Plane { .. })
                && f.loops.len() == 1
                && f.loops[0].len() == 3
                && f.loops[0]
                    .iter()
                    .all(|c| matches!(body.edges[c.edge].curve, Curve::Line { .. }))
        })
        .count();
    if body.faces.len() >= FACETED_LEAST && 2 * triangles >= body.faces.len() {
        return Err(format!(
            "meshed: {} faces, {triangles} of them flat triangles",
            body.faces.len()
        )
        .into());
    }
    let mut why: Vec<String> = Vec::new();
    if let Some((v, a)) = expect {
        for (what, got, want) in [("volume", volume, v), ("area", area, a)] {
            if (got - want).abs() > EXACT * want.abs().max(1e-12) {
                why.push(format!(
                    "{what} {got} against {want} ({:.1e} off)",
                    (got - want).abs() / want.abs()
                ));
            }
        }
    }
    // The triangles, as the runner meshes them: closed, and bounding the volume.
    let (mesh_volume, unmatched) = meshed(k, b, volume, area)?;
    if unmatched > 0 {
        why.push(format!(
            "triangles not closed ({unmatched} edges unmatched)"
        ));
    }
    let want = expect.map_or(volume, |e| e.0);
    if (mesh_volume - want).abs() > MESHED * want.abs() {
        why.push(format!("triangles' volume {mesh_volume:.6}"));
    }
    if !why.is_empty() {
        return Err(why.join("; ").into());
    }
    Ok(if expect.is_none() {
        " (built: unsettled)".into()
    } else {
        String::new()
    })
}

/// The runner's mesh of body `b` (the coarsest of its ladder of chords that bounds the volume to
/// 1/100): the volume the triangles bound and how many of their edges (points rounded to single
/// precision, as an STL file has them) aren't matched by one the other way.
fn meshed(
    k: &DefaultKernel,
    b: BodyId,
    volume: f64,
    area: f64,
) -> std::result::Result<(f64, usize), Refusal> {
    let size = k.bounding_box(b)?.diagonal();
    let thickness = if area > 0.0 && volume > 0.0 {
        volume / area
    } else {
        size
    };
    let mut deflection = (1e-3 * size).min(5e-2 * thickness).max(1e-10);
    let mut angle = 15f64.to_radians();
    let finest = (1e-5 * size).min(5e-4 * thickness).max(1e-10);
    let bound = |mesh: &TriMesh| -> f64 {
        mesh.indices
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
                a.coords.dot(&b.coords.cross(&c.coords)) / 6.0
            })
            .sum()
    };
    let mesh = loop {
        let mesh = k
            .tessellate(b, Tessellation { deflection, angle })
            .map_err(|e| format!("not meshed: {e}"))?;
        let off = (bound(&mesh) - volume).abs() / volume.abs();
        if off <= 1e-2 || deflection <= finest {
            break mesh;
        }
        deflection = (deflection * (0.7e-2 / off).clamp(1.0 / 16.0, 0.5)).max(finest);
        angle = (angle / 2.0).max(5f64.to_radians());
    };
    let key = |i: u32| {
        let p = mesh.positions[i as usize];
        [p.x as f32, p.y as f32, p.z as f32].map(f32::to_bits)
    };
    let mut edges: std::collections::HashMap<([u32; 3], [u32; 3]), i64> = Default::default();
    for t in &mesh.indices {
        for (a, z) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, z) = (key(a), key(z));
            if a == z {
                continue;
            }
            *edges.entry((a, z)).or_default() += 1;
        }
    }
    let unmatched = edges
        .iter()
        .filter(|((a, z), n)| edges.get(&(*z, *a)).copied().unwrap_or(0) != **n)
        .count();
    Ok((bound(&mesh), unmatched))
}
