//! Cuts by swept tools that CAM's exact simulation (`cadrs_core::cam::exact`) once failed to
//! make exactly, replayed: each `<name>.target` and `<name>.tool` a body as the kernel writes
//! it, `<name>.motion` the tool's rigid motion a step a line (`turn ox oy oz dx dy dz from to`,
//! `slide x0 y0 z0 x1 y1 z1`; `#` starts a comment, the failure it once gave). Each must be
//! cut exactly (`Kernel::cut_swept_exact`), leave a sound solid, and take material away.
//!
//! The cases are in `tests/swept_cuts/`; `SWEPT_CUTS=<dir>` replays another folder instead (a
//! simulation's dumps, `CADRS_EXACT_DUMP=<dir>`), `SWEPT_CUT=<name>` one case.

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point3, Vector3};

fn motion(text: &str) -> SolidMotion {
    let mut steps = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let kind = words.next().unwrap();
        let v: Vec<f64> = words.map(|w| w.parse().unwrap()).collect();
        steps.push(match kind {
            "turn" => RigidStep::turn(
                Axis::new(
                    Point3::new(v[0], v[1], v[2]),
                    Vector3::new(v[3], v[4], v[5]),
                ),
                v[6],
                v[7],
            ),
            "slide" => RigidStep::slide(
                Vector3::new(v[0], v[1], v[2]),
                Vector3::new(v[3], v[4], v[5]),
            ),
            k => panic!("unknown step {k}"),
        });
    }
    SolidMotion::rigid(steps)
}

/// The cut `name` in `dir`: `Err` why it isn't made exactly and soundly.
fn replay(dir: &std::path::Path, name: &str) -> std::result::Result<(usize, f64), String> {
    let read = |ext: &str| std::fs::read(dir.join(format!("{name}.{ext}"))).unwrap();
    let mut k = NativeKernel::new();
    let target = k.read_body(&read("target")).map_err(|e| e.to_string())?;
    let tool = k.read_body(&read("tool")).map_err(|e| e.to_string())?;
    let m = motion(&String::from_utf8(read("motion")).unwrap());
    let before = k.mass_properties(target).unwrap().volume;
    let cut = k
        .cut_swept_exact(target, tool, &m)
        .and_then(|r| r.first())
        .map_err(|e| e.to_string())?;
    let check = k.get(cut).unwrap().check();
    if !check.is_empty() {
        return Err(format!("not sound: {check:?}"));
    }
    let after = k.mass_properties(cut).unwrap().volume;
    if !(after <= before + 1e-9 && after > 0.0) {
        return Err(format!("the volume went from {before} to {after}"));
    }
    Ok((k.faces(cut).unwrap().len(), before - after))
}

#[test]
fn every_cut_a_simulation_failed_is_made_exactly() {
    let dir = std::env::var_os("SWEPT_CUTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/swept_cuts")
        });
    let only = std::env::var("SWEPT_CUT").ok();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|d| {
            d.filter_map(|e| {
                let p = e.ok()?.path();
                if p.extension()? != "motion" {
                    return None;
                }
                Some(p.file_stem()?.to_string_lossy().into_owned())
            })
            .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.retain(|n| only.as_ref().is_none_or(|o| o == n));
    let mut failed = Vec::new();
    for n in &names {
        let t = std::time::Instant::now();
        let r = replay(&dir, n);
        println!("{n}: {r:?} in {:.3} s", t.elapsed().as_secs_f64());
        if let Err(e) = r {
            failed.push(format!("{n}: {e}"));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} cuts failed:\n{}",
        failed.len(),
        names.len(),
        failed.join("\n")
    );
}
