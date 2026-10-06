//! Golden-image tests: run each scenario headless and compare its screenshots with
//! `tests/golden/<scenario>/*.png`.
//!
//! - They are `#[ignore]`d, so a plain `cargo test` stays fast: run them with
//!   `cargo test -r -p cadrs -F app-tests --test golden -- --ignored` (add a name to run some:
//!   `… -- --ignored section`).
//! - Goldens are git-ignored. When a scenario has none yet, the test records the current
//!   screenshots as the local baseline and passes.
//! - `CADRS_BLESS=1 cargo test -r -p cadrs -F app-tests --test golden -- --ignored` writes the current
//!   screenshots as the new goldens.
//! - If the app cannot start a GPU renderer (for example CI without a GPU), the test prints why
//!   and passes, rather than failing.
//! - `CADRS_SKIP_GOLDEN=1` skips the tests.
//! - The tests run in parallel (cargo's test threads), each scenario in its own app process; at
//!   most `CADRS_JOBS` (default 4) apps render at once, whatever the thread count. Each app is
//!   the same run as `cadrs --headless --scenario <name>`, so the images don't depend on it.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::GenericImageView;

/// A pixel counts as different if any channel differs by more than this.
const CHANNEL_TOLERANCE: u8 = 40;
/// A screenshot fails if more than this fraction of its pixels differ.
const MAX_DIFF_FRACTION: f64 = 0.002;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pngs(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".png"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn looks_like_missing_gpu(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    [
        "unable to find a gpu",
        "no suitable adapter",
        "failed to create wgpu adapter",
        "requestadaptererror",
        "no adapter",
        "failed to create device",
    ]
    .iter()
    .any(|p| s.contains(p))
}

/// At most `CADRS_JOBS` (default 4) scenario apps at once: a counting semaphore over the test
/// threads.
struct Jobs {
    running: std::sync::Mutex<usize>,
    freed: std::sync::Condvar,
}

static JOBS: Jobs = Jobs { running: std::sync::Mutex::new(0), freed: std::sync::Condvar::new() };

/// Holds one of the `CADRS_JOBS` slots until dropped.
struct JobSlot;

impl JobSlot {
    fn take() -> Self {
        let max = std::env::var("CADRS_JOBS").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(4).max(1);
        let mut n = JOBS.running.lock().unwrap();
        while *n >= max {
            n = JOBS.freed.wait(n).unwrap();
        }
        *n += 1;
        JobSlot
    }
}

impl Drop for JobSlot {
    fn drop(&mut self) {
        *JOBS.running.lock().unwrap() -= 1;
        JOBS.freed.notify_one();
    }
}

/// Runs a scenario and checks it against its goldens; returns its output directory, or
/// `None` if it was skipped.
fn run_scenario(name: &str) -> Option<PathBuf> {
    run_scenario_with(name, &[])
}

/// [`run_scenario`] with extra command-line arguments.
///
/// A scenario that doesn't match its goldens is rendered once more before the test fails: under
/// parallel load a rare input race can drop typed text (see PROGRESS.md, phase 3 log). A pass on
/// the second render is reported as FLAKY on stderr.
fn run_scenario_with(name: &str, extra: &[&std::ffi::OsStr]) -> Option<PathBuf> {
    match render_and_compare(name, extra) {
        Ok(out) => out,
        Err(first) => match render_and_compare(name, extra) {
            Ok(out) => {
                eprintln!("golden {name}: FLAKY, matched on the second render; the first render:\n{first}");
                out
            }
            Err(second) => panic!("{second}"),
        },
    }
}

/// Renders the scenario and compares it with its goldens; `Err` describes the mismatch.
fn render_and_compare(name: &str, extra: &[&std::ffi::OsStr]) -> Result<Option<PathBuf>, String> {
    if std::env::var_os("CADRS_SKIP_GOLDEN").is_some() {
        eprintln!("golden {name}: skipped (CADRS_SKIP_GOLDEN is set)");
        return Ok(None);
    }
    let out = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("golden")
        .join(name);
    let slot = JobSlot::take();
    let output = Command::new(env!("CARGO_BIN_EXE_cadrs"))
        .current_dir(workspace())
        .args(["--headless", "--scenario", name, "--out"])
        .arg(&out)
        .args(extra)
        .env("RUST_LOG", "warn")
        .output()
        .expect("failed to start the cadrs binary");
    drop(slot);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        if looks_like_missing_gpu(&stderr) {
            eprintln!("golden {name}: SKIPPED, no GPU renderer available:\n{stderr}");
            return Ok(None);
        }
        panic!("scenario {name} failed ({}):\n{stderr}", output.status);
    }

    let golden = workspace().join("tests/golden").join(name);
    let actual = pngs(&out);
    assert!(
        !actual.is_empty(),
        "scenario {name} produced no screenshots"
    );

    // Goldens are not checked in (they would bloat the repo): the first run on a machine records
    // them as that machine's baseline, and later runs compare against it.
    let expected = pngs(&golden);
    if std::env::var_os("CADRS_BLESS").is_some() || expected.is_empty() {
        let _ = std::fs::remove_dir_all(&golden);
        std::fs::create_dir_all(&golden).unwrap();
        for f in &actual {
            std::fs::copy(out.join(f), golden.join(f)).unwrap();
        }
        eprintln!(
            "golden {name}: recorded {} image(s) as the baseline in {}",
            actual.len(),
            golden.display()
        );
        return Ok(Some(out));
    }

    if actual != expected {
        return Err(format!("scenario {name} produced a different set of screenshots: {actual:?} vs {expected:?}"));
    }

    let mut failures = Vec::new();
    for f in &expected {
        let a = image::open(out.join(f)).unwrap().to_rgb8();
        let e = image::open(golden.join(f)).unwrap().to_rgb8();
        if a.dimensions() != e.dimensions() {
            failures.push(format!(
                "{f}: size {:?} != golden {:?}",
                a.dimensions(),
                e.dimensions()
            ));
            continue;
        }
        let mut diff = image::RgbImage::new(a.width(), a.height());
        let mut n = 0usize;
        for (x, y, pa) in a.enumerate_pixels() {
            let pe = e.get_pixel(x, y);
            let differs = (0..3).any(|c| pa[c].abs_diff(pe[c]) > CHANNEL_TOLERANCE);
            if differs {
                n += 1;
                diff.put_pixel(x, y, image::Rgb([255, 0, 0]));
            } else {
                let g = pa[0] / 3 + 170;
                diff.put_pixel(x, y, image::Rgb([g, g, g]));
            }
        }
        let fraction = n as f64 / (a.width() * a.height()) as f64;
        if fraction > MAX_DIFF_FRACTION {
            let diff_path = out.join(format!("diff-{f}"));
            diff.save(&diff_path).unwrap();
            failures.push(format!(
                "{f}: {n} pixels differ ({:.3}%), diff image at {}",
                fraction * 100.0,
                diff_path.display()
            ));
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "scenario {name} does not match its goldens (bless with CADRS_BLESS=1 if the change is intended):\n{}",
            failures.join("\n")
        ));
    }
    Ok(Some(out))
}

/// The fraction of pixels that differ between two screenshots (by more than the tolerance).
fn diff_fraction(a: &Path, b: &Path) -> f64 {
    let a = image::open(a).unwrap().to_rgb8();
    let b = image::open(b).unwrap().to_rgb8();
    assert_eq!(a.dimensions(), b.dimensions());
    let n = a
        .pixels()
        .zip(b.pixels())
        .filter(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > CHANNEL_TOLERANCE))
        .count();
    n as f64 / (a.width() * a.height()) as f64
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_smoke() {
    run_scenario("smoke");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_ui_gallery() {
    run_scenario("ui_gallery");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_landing_empty() {
    run_scenario("landing_empty");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_landing_create_document() {
    run_scenario("landing_create_document");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_landing_many_documents() {
    run_scenario("landing_many_documents");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_document_new() {
    let Some(out) = run_scenario("document_new") else {
        return;
    };
    // The rename made while the Sketch dialog was open survives leaving the document.
    let docs: Vec<String> = std::fs::read_dir(out.join("data"))
        .unwrap()
        .filter_map(|e| std::fs::read_to_string(e.ok()?.path().join("document.ron")).ok())
        .collect();
    assert_eq!(docs.len(), 1);
    assert!(docs[0].contains("name: \"Bracket v2\""), "{}", docs[0]);
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_tabs_create_assembly() {
    run_scenario("tabs_create_assembly");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_viewport_orbit() {
    run_scenario("viewport_orbit");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_plane_hover_select() {
    run_scenario("plane_hover_select");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_create_on_top() {
    run_scenario("sketch_create_on_top");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_create_on_front() {
    run_scenario("sketch_create_on_front");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_edit_existing() {
    run_scenario("sketch_edit_existing");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_line_chain() {
    run_scenario("sketch_line_chain");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_rectangle() {
    run_scenario("sketch_rectangle");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_circle_arc() {
    run_scenario("sketch_circle_arc");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_select_delete() {
    run_scenario("sketch_select_delete");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_snap_endpoint_midpoint() {
    run_scenario("snap_endpoint_midpoint");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_snap_intersection() {
    run_scenario("snap_intersection");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_snap_hv_inference() {
    run_scenario("snap_hv_inference");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_constrain_rectangle() {
    run_scenario("constrain_rectangle");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_drag_underconstrained() {
    run_scenario("drag_underconstrained");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_overconstrained_red() {
    run_scenario("overconstrained_red");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_dimension_rectangle_full() {
    run_scenario("dimension_rectangle_full");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_dimension_circle() {
    run_scenario("dimension_circle");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_dimension_edit_value() {
    run_scenario("dimension_edit_value");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_dimension_conflict() {
    run_scenario("dimension_conflict");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_constraint_glyphs_more() {
    run_scenario("constraint_glyphs_more");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_keyboard_shortcuts() {
    run_scenario("keyboard_shortcuts");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_cursors_tooltips() {
    run_scenario("cursors_tooltips");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_extrude_rectangle() {
    run_scenario("extrude_rectangle");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_extrude_preview_drag() {
    run_scenario("extrude_preview_drag");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_on_face() {
    run_scenario("sketch_on_face");
}

// P3.1: kernel-backed extrudes and a failed feature.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps_kernel_extrude() {
    run_scenario("course_ps_kernel_extrude");
}

// P3.2: edge, face and vertex picking, and faces named after their feature.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps_edge_hover() {
    run_scenario("course_ps_edge_hover");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps_face_names() {
    run_scenario("course_ps_face_names");
}

// P3.3: Extrude complete, Boolean, Parts, Mass properties.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps6_control_arm() {
    run_scenario("course_ps6_control_arm");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps4_end_types() {
    run_scenario("course_ps4_end_types");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps4_surface_thin() {
    run_scenario("course_ps4_surface_thin");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps5_boolean() {
    run_scenario("course_ps5_boolean");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps7_revolve_types() {
    run_scenario("course_ps7_revolve_types");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps8_reducer_coupling() {
    run_scenario("course_ps8_reducer_coupling");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps1_whole_sketch() {
    run_scenario("course_ps1_whole_sketch");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps2_parts_list() {
    run_scenario("course_ps2_parts_list");
}

// T3: entity tools (one scenario per lesson).
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s3_midpoint_line() {
    run_scenario("course_s3_midpoint_line");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s3_aligned_rectangle() {
    run_scenario("course_s3_aligned_rectangle");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s4_three_point_circle() {
    run_scenario("course_s4_three_point_circle");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s6_slot() {
    run_scenario("course_s6_slot");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s7_polygon() {
    run_scenario("course_s7_polygon");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s8_ellipse() {
    run_scenario("course_s8_ellipse");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s9_fillet() {
    run_scenario("course_s9_fillet");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s9_chamfer() {
    run_scenario("course_s9_chamfer");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s10_point() {
    run_scenario("course_s10_point");
}

// Final re-audit of the sketching course: the phase 2 partial rows.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s8_ellipse_tangent() {
    run_scenario("course_s8_ellipse_tangent");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s9_fillet_line_arc() {
    run_scenario("course_s9_fillet_line_arc");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s12_normal_plane() {
    run_scenario("course_s12_normal_plane");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s12_curvature() {
    run_scenario("course_s12_curvature");
}

// T2: lifecycle and constraint UX (one scenario per lesson).
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s1_new_sketch_context_menu() {
    run_scenario("course_s1_new_sketch_context_menu");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s1_plane_visibility() {
    run_scenario("course_s1_plane_visibility");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s1_view_normal_menu() {
    run_scenario("course_s1_view_normal_menu");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s2_restore_toast() {
    run_scenario("course_s2_restore_toast");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s2_rename_sketch() {
    run_scenario("course_s2_rename_sketch");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s3_line_double_click_end() {
    run_scenario("course_s3_line_double_click_end");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s4_line_to_tangent_arc() {
    run_scenario("course_s4_line_to_tangent_arc");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s11_drag_inference() {
    run_scenario("course_s11_drag_inference");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s11_shift_keeps_glyphs() {
    run_scenario("course_s11_shift_keeps_glyphs");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s13_quick_dim_line_arc() {
    run_scenario("course_s13_quick_dim_line_arc");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s13_driving_driven_toggle() {
    run_scenario("course_s13_driving_driven_toggle");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_x1_workspace_units() {
    run_scenario("course_x1_workspace_units");
}

// T1: the Introduction to Sketching course's exercise enablers.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ex1_basic() {
    run_scenario("course_ex1_basic");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ex2_intermediate() {
    run_scenario("course_ex2_intermediate");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_x2_region_area() {
    run_scenario("course_x2_region_area");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s13_first_dim_scales() {
    run_scenario("course_s13_first_dim_scales");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s13_circle_dims() {
    run_scenario("course_s13_circle_dims");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s12_symmetric() {
    run_scenario("course_s12_symmetric");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s19_mirror() {
    run_scenario("course_s19_mirror");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s19_offset() {
    run_scenario("course_s19_offset");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_disabled_placeholders() {
    run_scenario("course_disabled_placeholders");
}

/// Saving and reopening a document gives back exactly what was on screen.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_reload_roundtrip() {
    let Some(out) = run_scenario("reload_roundtrip") else {
        return;
    };
    let f = diff_fraction(
        &out.join("01-before-closing.png"),
        &out.join("03-reopened-matches-01.png"),
    );
    assert!(
        f <= MAX_DIFF_FRACTION,
        "the reopened document differs from the one closed: {:.3}% of pixels",
        f * 100.0
    );
    // The right-edge panel strip (a narrow column the whole-frame fraction can't see) is rebuilt
    // for the Part Studio: no assembly buttons carried over.
    let strip = |p: &Path| image::open(p).unwrap().to_rgb8().view(1560, 40, 40, 920).to_image();
    let (a, b) = (strip(&out.join("01-before-closing.png")), strip(&out.join("03-reopened-matches-01.png")));
    let n = a.pixels().zip(b.pixels()).filter(|(p, q)| (0..3).any(|c| p[c].abs_diff(q[c]) > CHANNEL_TOLERANCE)).count();
    assert!(n < 20, "the reopened document's right panel strip differs from the one closed ({n} pixels)");
    // A new app instance on the same documents shows the same thing.
    let data = out.join("data");
    let Some(relaunch) = run_scenario_with(
        "reload_roundtrip_relaunch",
        &[std::ffi::OsStr::new("--data-dir"), data.as_os_str()],
    ) else {
        return;
    };
    let f = diff_fraction(
        &out.join("01-before-closing.png"),
        &relaunch.join("02-reopened-after-relaunch-matches-01.png"),
    );
    assert!(
        f <= MAX_DIFF_FRACTION,
        "the document reopened in a new app differs from the one closed: {:.3}% of pixels",
        f * 100.0
    );
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s17_trim() {
    run_scenario("course_s17_trim");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s17_trim_drag() {
    run_scenario("course_s17_trim_drag");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s17_regions_without_trim() {
    run_scenario("course_s17_regions_without_trim");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s17_extend() {
    run_scenario("course_s17_extend");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s18_split() {
    run_scenario("course_s18_split");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s12_normal() {
    run_scenario("course_s12_normal");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s20_use_edges() {
    run_scenario("course_s20_use_edges");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s20_use_updates() {
    run_scenario("course_s20_use_updates");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s21_imprinting() {
    run_scenario("course_s21_imprinting");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s21_disable_imprinting() {
    run_scenario("course_s21_disable_imprinting");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s12_pierce() {
    run_scenario("course_s12_pierce");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s16_text() {
    run_scenario("course_s16_text");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_s20_use_silhouette() {
    run_scenario("course_s20_use_silhouette");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_perf_500() {
    // The corner is dragged out and back and dropped where it started: the sketch must look as
    // it did before (only the cursor and its hover differ).
    if let Some(out) = run_scenario("perf_500") {
        let d = diff_fraction(
            &out.join("01-500-entities-zoom-to-fit.png"),
            &out.join("02-after-dragging-a-corner.png"),
        );
        assert!(d < 0.0005, "the drag changed the sketch ({d:.5} of the pixels differ)");
    }
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_create() {
    run_scenario("course_drw_create");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_sheets() {
    run_scenario("course_drw_sheets");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_views() {
    run_scenario("course_drw_views");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_four_views() {
    run_scenario("course_drw_four_views");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_ex1_ujoint() {
    run_scenario("course_drw_ex1_ujoint");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_dimension_palette() {
    run_scenario("course_drw_dimension_palette");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_notes() {
    run_scenario("course_drw_notes");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_tables() {
    run_scenario("course_drw_tables");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_ex3_update() {
    run_scenario("course_drw_ex3_update");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_export() {
    run_scenario("course_drw_export");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_insert_dxf_image() {
    run_scenario("course_drw_insert_dxf_image");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_section_detail() {
    run_scenario("course_drw_section_detail");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_more_views() {
    run_scenario("course_drw_more_views");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_more_annotations() {
    run_scenario("course_drw_more_annotations");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_ex2_assembly() {
    run_scenario("course_drw_ex2_assembly");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps3_folders() {
    run_scenario("course_ps3_folders");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps3_filter() {
    run_scenario("course_ps3_filter");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps11_dependencies() {
    run_scenario("course_ps11_dependencies");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps13_rollback_final() {
    run_scenario("course_ps13_rollback_final");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps2_search_tools() {
    run_scenario("course_ps2_search_tools");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_p6_cube_corner() {
    run_scenario("course_p6_cube_corner");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps2_regeneration_times() {
    run_scenario("course_ps2_regeneration_times");
}

// P3.10: the Part Studios remainder, features.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps4_draft() {
    run_scenario("course_ps4_draft");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps15_hole_options() {
    run_scenario("course_ps15_hole_options");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps14_fillet_options() {
    run_scenario("course_ps14_fillet_options");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps5_boolean_options() {
    run_scenario("course_ps5_boolean_options");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps20_loft_match() {
    run_scenario("course_ps20_loft_match");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps7_revolve_connector() {
    run_scenario("course_ps7_revolve_connector");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_x7_mass_options() {
    run_scenario("course_x7_mass_options");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps20_loft_direction() {
    run_scenario("course_ps20_loft_direction");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps25_curve_pattern() {
    run_scenario("course_ps25_curve_pattern");
}

// P3.11 fix round 1: the Part Studios course's exercises and the other course scenarios that
// had no goldens.

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps17_gear_cover() {
    run_scenario("course_ps17_gear_cover");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps21_funnel() {
    run_scenario("course_ps21_funnel");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps27_reflector() {
    run_scenario("course_ps27_reflector");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps10_material() {
    run_scenario("course_ps10_material");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps12_planes() {
    run_scenario("course_ps12_planes");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps14_chamfer() {
    run_scenario("course_ps14_chamfer");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps14_fillet() {
    run_scenario("course_ps14_fillet");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps15_hole() {
    run_scenario("course_ps15_hole");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps16_shell_fail() {
    run_scenario("course_ps16_shell_fail");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps18_split_options() {
    run_scenario("course_ps18_split_options");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps18_split() {
    run_scenario("course_ps18_split");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps19_sweep_planes() {
    run_scenario("course_ps19_sweep_planes");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps19_sweep() {
    run_scenario("course_ps19_sweep");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps20_loft() {
    run_scenario("course_ps20_loft");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps22_skip_instances() {
    run_scenario("course_ps22_skip_instances");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps26_mirror() {
    run_scenario("course_ps26_mirror");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps8_single_revolve() {
    run_scenario("course_ps8_single_revolve");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_ps9_appearance() {
    run_scenario("course_ps9_appearance");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_p310_profiles() {
    run_scenario("course_p310_profiles");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_x11_mate_connectors() {
    run_scenario("course_x11_mate_connectors");
}

// Stage 3D: Inspection and Repair Tools (P3D.1, P3D.2).
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_insp_error_states() {
    run_scenario("course_insp_error_states");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_insp_feature_menu() {
    run_scenario("course_insp_feature_menu");
}

// IR5.5: Dynamic suppression ▸ Suppress by variable.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_insp_suppress_by_variable() {
    run_scenario("course_insp_suppress_by_variable");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_insp_profile_inspector() {
    run_scenario("course_insp_profile_inspector");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_insp_constraint_manager() {
    run_scenario("course_insp_constraint_manager");
}

// Stage 3D: P3D.3 (history) and P3D.4 (Repair, Replace reference, the Conrod exercise).
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_insp_history_panel() {
    run_scenario("course_insp_history_panel");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_insp_edit_healthy_moment() {
    run_scenario("course_insp_edit_healthy_moment");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_insp_ex1_conrod() {
    run_scenario("course_insp_ex1_conrod");
}

// Stage 3F: P3F.2 (import and export).
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_tips_import_step() {
    run_scenario("course_tips_import_step");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcad_export() {
    run_scenario("course_pcad_export");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcad_design_intent() {
    run_scenario("course_pcad_design_intent");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcad_variables() {
    run_scenario("course_pcad_variables");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_variables() {
    run_scenario("course_asm_variables");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_tips_suppress_threads() {
    run_scenario("course_tips_suppress_threads");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_tips_export_assembly() {
    run_scenario("course_tips_export_assembly");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcad_simulation() {
    // P3F.5 (P3.5): the cantilever's loads, solve, von Mises map, legend and probe.
    run_scenario("course_pcad_simulation");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_simulation() {
    // P3F.5 (A1.7, A1.8, A6.3, X16): an assembly's loads and a Simulation connection.
    run_scenario("course_asm_simulation");
}

/// Mean and variance of an image's luminance (0–1).
fn luminance_stats(img: &image::RgbaImage) -> (f64, f64) {
    let n = (img.width() * img.height()) as f64;
    let (mut sum, mut sum2) = (0.0, 0.0);
    for p in img.pixels() {
        let l = (0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64) / 255.0;
        sum += l;
        sum2 += l * l;
    }
    let mean = sum / n;
    (mean, sum2 / n - mean * mean)
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcad_render() {
    // P3F.6 (P3.6): Render Studio renders the Control Arm to a 1920 × 1080 PNG that isn't blank
    // (its luminance varies: the model, its shading and its shadow on the backdrop).
    if run_scenario("course_pcad_render").is_none() {
        return;
    }
    let path = workspace().join("target/exports/course_pcad_render/Control Arm.png");
    let img = image::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).to_rgba8();
    assert_eq!(img.dimensions(), (1920, 1080));
    let (mean, variance) = luminance_stats(&img);
    assert!(variance > 0.005, "a blank-looking render: luminance variance {variance}");
    assert!(mean > 0.3 && mean < 0.95, "luminance mean {mean}");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcad_export_image() {
    // P3F.6 (P3.7): Export image… writes the sizes and formats asked for, from a Part Studio and
    // from an exploded view.
    if run_scenario("course_pcad_export_image").is_none() {
        return;
    }
    let dir = workspace().join("target/exports/course_pcad_export_image");
    for (file, size) in [
        ("Control Arm view.png", Some((1280, 720))),
        ("Control Arm view.jpg", Some((800, 600))),
        ("Control Arm transparent.png", None),
        ("Cylinder assembly - Exploded view 1.png", Some((1920, 1080))),
    ] {
        let img = image::open(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        if let Some(size) = size {
            assert_eq!((img.width(), img.height()), size, "{file}");
        }
        let (_, variance) = luminance_stats(&img.to_rgba8());
        assert!(variance > 0.001, "{file}: blank");
    }
    // The transparent one is clear around the model.
    let t = image::open(dir.join("Control Arm transparent.png")).unwrap().to_rgba8();
    assert_eq!(t.get_pixel(0, 0)[3], 0);
    assert!(t.pixels().any(|p| p[3] == 255));
}

/// The largest frame time (ms) in a `perf-<label>.txt` report ("… max 12.34 ms …").
fn perf_max(out: &Path, label: &str) -> f64 {
    let text = std::fs::read_to_string(out.join(format!("perf-{label}.txt"))).unwrap_or_else(|e| panic!("perf-{label}.txt: {e}"));
    let after = text.split("max ").nth(1).unwrap_or_else(|| panic!("no max in {text}"));
    after.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or_else(|| panic!("bad max in {text}"))
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_tips_scale() {
    // P3F.3 (T4.2, X7): the scale fixture's timings, from the app's own measurements. A debug
    // build (and the golden suite's parallel apps) is given 8×; the release numbers are the
    // article's (they are also checked on CPU time in `cadrs_core/tests/scale.rs`).
    let Some(out) = run_scenario("course_tips_scale") else { return };
    let k = if cfg!(debug_assertions) { 8.0 } else { 1.0 };
    // The background rebuild (everything below the first plate): no frame freezes.
    let background = perf_max(&out, "background");
    assert!(background < 50.0 * k, "a frame took {background} ms during the background rebuild");
    // A tab switch to a small studio.
    let switch = perf_max(&out, "switch");
    assert!(switch < 100.0 * k, "the tab switch took a {switch} ms frame");
    // Editing the last feature rebuilt only it.
    let last = std::fs::read_to_string(out.join("perf-edit-last-rebuilds.txt")).unwrap();
    assert!(last.lines().any(|l| l.contains("1 computed")), "{last}");
    // The document opened: shown at once, its 250 features rebuilt in the background without
    // a frame freezing (the load and first frames within the 2 s budget).
    let open = perf_max(&out, "open");
    assert!(open < 2000.0 * k, "opening took a {open} ms frame");
    let rebuilds = std::fs::read_to_string(out.join("perf-open-rebuilds.txt")).unwrap();
    assert!(rebuilds.lines().any(|l| l.contains("120 computed")), "{rebuilds}");
}

// Stage 3G: P3G.1 (external references, version-pinned links, Other documents in Insert).
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_insert_linked() {
    run_scenario("course_er_insert_linked");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_versions_in_document() {
    run_scenario("course_er_versions_in_document");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_drw_version_reference() {
    run_scenario("course_drw_version_reference");
}

// Stage 3G: P3G.2 (update badges, Reference manager, pinning, Update all).
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_update_linked() {
    run_scenario("course_er_update_linked");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_reference_manager() {
    run_scenario("course_er_reference_manager");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_update_all() {
    run_scenario("course_er_update_all");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_pinning() {
    run_scenario("course_er_pinning");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_change_to_version() {
    run_scenario("course_er_change_to_version");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_open_linked() {
    run_scenario("course_er_open_linked");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_move_to_document() {
    run_scenario("course_er_move_to_document");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_tips_move_tab() {
    run_scenario("course_tips_move_tab");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_dv_derived_dialog() {
    run_scenario("course_dv_derived_dialog");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_dv_derived_options() {
    run_scenario("course_dv_derived_options");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_dv_ex1_two_copies() {
    run_scenario("course_dv_ex1_two_copies");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_dv_ex2_update() {
    run_scenario("course_dv_ex2_update");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_dv_ex3_workspace() {
    run_scenario("course_dv_ex3_workspace");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_dv_ex4_assembly_update() {
    run_scenario("course_dv_ex4_assembly_update");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_dv_ex5_circular() {
    run_scenario("course_dv_ex5_circular");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_ex1_hexapod() {
    run_scenario("course_er_ex1_hexapod");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_er_ex2_move() {
    run_scenario("course_er_ex2_move");
}

// P3E.1: the documents page (labels, details, samples, import).
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_documents_labels() {
    run_scenario("course_td_documents_labels");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_documents_details() {
    run_scenario("course_td_documents_details");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_documents_samples() {
    run_scenario("course_td_documents_samples");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_documents_import() {
    run_scenario("course_td_documents_import");
}

// P3E.2: tab folders, the full Tab manager, and 60 tabs.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_tab_folders() {
    run_scenario("course_td_tab_folders");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_tab_manager() {
    run_scenario("course_td_tab_manager");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_many_tabs() {
    run_scenario("course_td_many_tabs");
}

// P3E.3a: render modes, perspective, zoom to window, named views; section views; selection.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_render_modes() {
    run_scenario("course_td_render_modes");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_section() {
    run_scenario("course_td_section");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_selection() {
    run_scenario("course_td_selection");
}

// P3E.3b: Measure (with its assembly frames), the analysis tools, the mouse preference.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_measure() {
    run_scenario("course_td_measure");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_analysis() {
    run_scenario("course_td_analysis");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_mouse_prefs() {
    run_scenario("course_td_mouse_prefs");
}

// P3E.4: workspaces, branches and merge.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_branch_merge() {
    run_scenario("course_td_branch_merge");
}

// P3E.5: the test drive walkthrough on the drill stand-in.
#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_td_ex1_drill() {
    run_scenario("course_td_ex1_drill");
}

// Final part 2: the assembly course scenarios (stage 3B) and the importer/list scenarios merged
// from main, which were not registered before.

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_animate() {
    run_scenario("course_asm_animate");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_bom() {
    run_scenario("course_asm_bom");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_bom_template() {
    run_scenario("course_asm_bom_template");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_connector_tool_origin() {
    run_scenario("course_asm_connector_tool_origin");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_edit_implicit_connector() {
    run_scenario("course_asm_edit_implicit_connector");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_edit_in_context() {
    run_scenario("course_asm_edit_in_context");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_mic_contexts() {
    run_scenario("course_mic_contexts");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_mic_ex2_slide() {
    run_scenario("course_mic_ex2_slide");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_mic_ex3_gripper() {
    run_scenario("course_mic_ex3_gripper");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_ex1_start() {
    run_scenario("course_asm_ex1_start");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_ex2_pneumatic() {
    run_scenario("course_asm_ex2_pneumatic");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_ex3_structure() {
    run_scenario("course_asm_ex3_structure");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_ex4_connectors() {
    run_scenario("course_asm_ex4_connectors");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_exploded_view() {
    run_scenario("course_asm_exploded_view");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_folders() {
    run_scenario("course_asm_folders");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_hide_show() {
    run_scenario("course_asm_hide_show");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_insert_placement() {
    run_scenario("course_asm_insert_placement");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_interference() {
    run_scenario("course_asm_interference");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_items() {
    run_scenario("course_asm_items");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_mate_connectors() {
    run_scenario("course_asm_mate_connectors");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_mate_dialog_options() {
    let Some(out) = run_scenario("course_asm_mate_dialog_options") else {
        return;
    };
    // A6.11: Solve re-seats the magnet the edit left floating (the two frames were identical).
    let f = diff_fraction(&out.join("07-a6.11-edit-moves-this-mate-only.png"), &out.join("08-a6.11-solve-moves-the-magnet-too.png"));
    assert!(f > 0.001, "Solve changed nothing: {:.4}% of pixels differ", f * 100.0);
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_mates_pinslot() {
    run_scenario("course_asm_mates_pinslot");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_mates_planar_ball_parallel() {
    run_scenario("course_asm_mates_planar_ball_parallel");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_mates_tangent_width() {
    run_scenario("course_asm_mates_tangent_width");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_named_positions() {
    run_scenario("course_asm_named_positions");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_relations() {
    run_scenario("course_asm_relations");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_replace() {
    run_scenario("course_asm_replace");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_replicate() {
    run_scenario("course_asm_replicate");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_rigid_insert() {
    run_scenario("course_asm_rigid_insert");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_show_mates() {
    run_scenario("course_asm_show_mates");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_std_batch() {
    run_scenario("course_asm_std_batch");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_std_bulk_edit() {
    run_scenario("course_asm_std_bulk_edit");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_subassemblies() {
    run_scenario("course_asm_subassemblies");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_triad() {
    run_scenario("course_asm_triad");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_asm_where_used() {
    run_scenario("course_asm_where_used");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_feature_list_hover() {
    run_scenario("feature_list_hover");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_feature_list_scroll() {
    run_scenario("feature_list_scroll");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_onshape_derived() {
    run_scenario("onshape_derived");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_onshape_import() {
    run_scenario("onshape_import");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_part_export_step() {
    run_scenario("part_export_step");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_transform_feature() {
    run_scenario("transform_feature");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_hole_construction_points() {
    run_scenario("hole_construction_points");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_snap_part_edges() {
    run_scenario("sketch_snap_part_edges");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_transform_xyz_arrows() {
    run_scenario("transform_xyz_arrows");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_bom() {
    run_scenario("course_pcb_bom");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_component_properties() {
    run_scenario("course_pcb_component_properties");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_component_view() {
    run_scenario("course_pcb_component_view");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_custom_part() {
    run_scenario("course_pcb_custom_part");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_delete_board() {
    run_scenario("course_pcb_delete_board");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_ex1_board() {
    run_scenario("course_pcb_ex1_board");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_ex2_vision() {
    run_scenario("course_pcb_ex2_vision");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_ex3_idf_assembly() {
    run_scenario("course_pcb_ex3_idf_assembly");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_export_idf() {
    run_scenario("course_pcb_export_idf");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_geometry_cellphone() {
    run_scenario("course_pcb_geometry_cellphone");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_geometry_vision() {
    run_scenario("course_pcb_geometry_vision");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_import_idf() {
    run_scenario("course_pcb_import_idf");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_one_part() {
    run_scenario("course_pcb_one_part");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_search() {
    run_scenario("course_pcb_search");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_settings() {
    run_scenario("course_pcb_settings");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_studio_create() {
    run_scenario("course_pcb_studio_create");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_sync_partstudio() {
    run_scenario("course_pcb_sync_partstudio");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_feature_list_select() {
    run_scenario("feature_list_select");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_readme_screenshots() {
    run_scenario("readme_screenshots");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_section_view_menu() {
    run_scenario("section_view_menu");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_section_view_planes() {
    run_scenario("section_view_planes");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sheetmetal_collision() {
    run_scenario("sheetmetal_collision");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sheetmetal_convert() {
    run_scenario("sheetmetal_convert");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sheetmetal_dialog() {
    run_scenario("sheetmetal_dialog");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sheetmetal_extrude() {
    run_scenario("sheetmetal_extrude");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sheetmetal_thicken() {
    run_scenario("sheetmetal_thicken");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sketch_spline() {
    run_scenario("sketch_spline");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_e1() {
    run_scenario("sm_e1");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_e2() {
    run_scenario("sm_e2");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_e3() {
    run_scenario("sm_e3");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_e4() {
    run_scenario("sm_e4");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i3_bend_feature() {
    run_scenario("sm_p3i3_bend_feature");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i3_edits() {
    run_scenario("sm_p3i3_edits");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i3_table() {
    run_scenario("sm_p3i3_table");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i4_e2() {
    run_scenario("sm_p3i4_e2");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i4_flange() {
    run_scenario("sm_p3i4_flange");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i4_flange_miter() {
    run_scenario("sm_p3i4_flange_miter");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i4_hem() {
    run_scenario("sm_p3i4_hem");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i4_hem_corner() {
    run_scenario("sm_p3i4_hem_corner");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i4_make_joint() {
    run_scenario("sm_p3i4_make_joint");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_bend() {
    run_scenario("sm_p3i5_bend");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_bend_relief() {
    run_scenario("sm_p3i5_bend_relief");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_corner() {
    run_scenario("sm_p3i5_corner");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_corner_break() {
    run_scenario("sm_p3i5_corner_break");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_cut() {
    run_scenario("sm_p3i5_cut");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_dialogs() {
    run_scenario("sm_p3i5_dialogs");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_finish() {
    run_scenario("sm_p3i5_finish");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_jog() {
    run_scenario("sm_p3i5_jog");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_mirror() {
    run_scenario("sm_p3i5_mirror");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i5_tab() {
    run_scenario("sm_p3i5_tab");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i6_e1_import() {
    run_scenario("sm_p3i6_e1_import");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i6_export_dialog() {
    run_scenario("sm_p3i6_export_dialog");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i6_flat_cut_and_tab() {
    run_scenario("sm_p3i6_flat_cut_and_tab");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i7_e3() {
    run_scenario("sm_p3i7_e3");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i7_forms() {
    run_scenario("sm_p3i7_forms");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i7_options() {
    run_scenario("sm_p3i7_options");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i8_flat_view() {
    run_scenario("sm_p3i8_flat_view");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i8_legacy() {
    run_scenario("sm_p3i8_legacy");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i8_topdown() {
    run_scenario("sm_p3i8_topdown");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i9_form() {
    run_scenario("sm_p3i9_form");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_sm_p3i9_loft() {
    run_scenario("sm_p3i9_loft");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_surfacing_fill() {
    run_scenario("surfacing_fill");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_surfacing_helix() {
    run_scenario("surfacing_helix");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_surfacing_thicken() {
    run_scenario("surfacing_thicken");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_course_pcb_component_documents() {
    run_scenario("course_pcb_component_documents");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_pcb_create_board_component() {
    run_scenario("pcb_create_board_component");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_eda_course_views() {
    run_scenario("eda_course_views");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_eda_schematic_gs04_12() {
    run_scenario("eda_schematic_gs04_12");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_eda_layout_gs13_21() {
    run_scenario("eda_layout_gs13_21");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_eda_parts_gs22_26() {
    run_scenario("eda_parts_gs22_26");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_eda_library_browser() {
    run_scenario("eda_library_browser");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_eda_power_monitor() {
    run_scenario("eda_power_monitor");
}

#[test]
#[ignore = "headless screenshots; run with: cargo test -r -p cadrs -F app-tests --test golden -- --ignored"]
fn golden_eda_schematic_editing() {
    run_scenario("eda_schematic_editing");
}
