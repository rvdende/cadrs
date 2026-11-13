//! The gears tester: every generated gear family over a grid of inputs, extremes included, in
//! parallel, as a table: whether each pair builds (ok, a warning, or a failure), how long it
//! takes, how many faces its members have, whether they mesh (how far each member's display
//! mesh reaches into the other's), and how long its Fast preview takes.
//!
//! `cargo gears-tester` (the alias for `cargo test -r -p cadrs_core --test gears_tester --
//! --ignored --nocapture`): the Fast preview of every pair (seconds for the whole grid).
//! `MODE=exact` builds the exact members instead (minutes); `GEARS=hypoid,worm,bevel` picks the
//! families; `JOBS=n` runs n cases at once (the rebuilds share one kernel thread, so more only
//! helps the previews, and makes the times a queue's).

#![cfg(feature = "brep")]

#[path = "support_reach.rs"]
mod support_reach;

use cadrs_core::commands::AddFeature;
use cadrs_core::document::{Document, FeatureKind};
use cadrs_core::gear::GearFeature;
use cadrs_core::{History, Part, rebuild};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// One case: its family, what it is, and the pair's two features.
struct Case {
    family: &'static str,
    what: String,
    pair: [GearFeature; 2],
}

/// What a case came to.
struct Outcome {
    status: &'static str,
    took: Duration,
    /// The pair with Fast preview on (generated gears: hypoid members, worm wheels).
    preview: Option<Duration>,
    faces: [usize; 2],
    deep: Option<f64>,
    note: String,
}

/// Builds `gears` in a Part Studio of their own.
fn build(gears: &[GearFeature]) -> std::sync::Arc<rebuild::Build> {
    let mut d = Document::new("Gears tester");
    let el = d.elements[0].id;
    let mut h = History::default();
    for g in gears {
        h.execute(
            &mut d,
            &AddFeature {
                element: el,
                feature: cadrs_core::FeatureId::new(),
                base_name: "Gear".into(),
                kind: FeatureKind::Gear(g.clone()),
            },
        )
        .unwrap();
    }
    rebuild::build(&d.element(el).unwrap().features().to_vec())
}

fn run(case: &Case, exact: bool) -> Outcome {
    // Outside what the feature takes (its own checks): not a breakage.
    if let Some(p) = case.pair.iter().find_map(|g| g.problem()) {
        return Outcome {
            status: "range",
            took: Duration::ZERO,
            preview: None,
            faces: [0, 0],
            deep: None,
            note: p.into(),
        };
    }
    let t = Instant::now();
    let pair: Vec<GearFeature> = case
        .pair
        .iter()
        .map(|g| GearFeature {
            fast_preview: !exact,
            ..g.clone()
        })
        .collect();
    let b = build(&pair);
    let took = t.elapsed();
    let generated = case.pair.iter().any(|g| {
        matches!(
            g.gear_type,
            cadrs_core::gear::GearType::Hypoid | cadrs_core::gear::GearType::Worm
        )
    });
    let preview = (generated && exact).then(|| {
        let fast: Vec<GearFeature> = case
            .pair
            .iter()
            .map(|g| GearFeature {
                fast_preview: true,
                ..g.clone()
            })
            .collect();
        let t = Instant::now();
        let _ = build(&fast);
        t.elapsed()
    });
    let mut note = String::new();
    for (_, e) in &b.errors {
        note.push_str(e);
        note.push(' ');
    }
    for (_, w) in &b.warnings {
        note.push_str(w);
        note.push(' ');
    }
    let parts: Vec<Part> = b.parts.clone();
    let faces = [0, 1].map(|i| parts.get(i).map_or(0, |p| p.solid.faces.len()));
    let deep = (parts.len() == 2).then(|| support_reach::reach_of(&parts).0);
    // (A pair that can't be laid out at all: outside what the design takes.)
    let unplaceable = b
        .errors
        .iter()
        .all(|(_, e)| e.contains("can't be laid out"));
    let status = if !b.errors.is_empty() && unplaceable {
        "range"
    } else if !b.errors.is_empty() || parts.len() != 2 {
        "FAIL"
    } else if !b.warnings.is_empty() {
        "warn"
    // (Display meshes lie a few hundredths of a millimetre off their surfaces: flank on flank
    // they may reach that far into each other.)
    } else if deep.is_some_and(|d| d > 0.1) {
        // (A worm wheel's preview is a helical gear of the worm's lead, not hobbed: within a
        // few tenths of the worm.)
        if !exact && case.family == "worm" && deep.is_some_and(|d| d < 0.35) {
            "approx"
        } else {
            "CLASH"
        }
    } else {
        "ok"
    };
    Outcome {
        status,
        took,
        preview,
        faces,
        deep,
        note,
    }
}

fn cases() -> Vec<Case> {
    use cadrs_core::samples::gears::{hypoid_pair, shaft_pair, worm_drive};
    let want: Vec<String> = std::env::var("GEARS")
        .unwrap_or_else(|_| "hypoid,worm,bevel".into())
        .split(',')
        .map(str::to_string)
        .collect();
    let mut out = Vec::new();
    if want.iter().any(|w| w == "hypoid") {
        // hypoid.sv's pair (24/48) at shaft angles from 30° to 150° and offsets from none (a
        // spiral bevel pair) to past the gear's radius.
        for shaft in [
            20.0, 30.0, 45.0, 60.0, 75.0, 90.0, 105.0, 120.0, 135.0, 150.0, 160.0,
        ] {
            for offset in [0.0, 10.0, 25.0, 40.0, 60.0, 80.0] {
                let pair = hypoid_pair().map(|mut g| {
                    g.shaft_angle = shaft;
                    g.shaft_angle_expr = format!("{shaft} deg");
                    g.offset = offset;
                    g.offset_expr = format!("{offset} mm");
                    g
                });
                out.push(Case {
                    family: "hypoid",
                    what: format!("24/48 shaft {shaft:>5}° offset {offset:>4} mm"),
                    pair,
                });
            }
        }
    }
    if want.iter().any(|w| w == "worm") {
        // Worm drives: 1 to 4 starts, few and many teeth, slender and stout worms.
        for starts in [1u32, 2, 3, 4] {
            for teeth in [20u32, 40] {
                for q in [8.0, 10.0, 14.0] {
                    let pair = worm_drive([0.0; 3]).map(|mut g| {
                        if g.member == cadrs_core::gear::BevelMember::Gear {
                            (g.teeth, g.mate_teeth) = (teeth, starts);
                        } else {
                            (g.teeth, g.mate_teeth) = (starts, teeth);
                        }
                        g.teeth_expr = g.teeth.to_string();
                        g.mate_teeth_expr = g.mate_teeth.to_string();
                        g.worm_quotient = q;
                        g.worm_quotient_expr = format!("{q}");
                        g
                    });
                    out.push(Case {
                        family: "worm",
                        what: format!("{starts} start(s) to {teeth} teeth, q {q}"),
                        pair,
                    });
                }
            }
        }
    }
    if want.iter().any(|w| w == "bevel") {
        // Face-milled spiral bevel pairs (24/18), the pinion with localised contact, at shaft
        // angles from 20° to 150°.
        for shaft in [20.0, 30.0, 45.0, 60.0, 90.0, 115.0, 120.0, 135.0, 150.0] {
            let pair = shaft_pair(shaft).map(|mut g| {
                g.teeth_expr = g.teeth.to_string();
                g.pitch_angle_expr = format!("{} deg", g.pitch_angle);
                g.rotation_expr = format!("{} deg", g.rotation);
                g.shaft_angle_expr = format!("{} deg", g.shaft_angle);
                g.tilt_expr = format!("{} deg", g.tilt);
                g
            });
            out.push(Case {
                family: "bevel",
                what: format!("24/18 localised, shaft {shaft:>5}°"),
                pair,
            });
        }
    }
    out
}

#[test]
#[ignore = "the gears tester: cargo gears-tester"]
fn gears_tester() {
    let cases = cases();
    let exact = std::env::var("MODE").is_ok_and(|m| m == "exact");
    let jobs: usize = std::env::var("JOBS")
        .ok()
        .and_then(|j| j.parse().ok())
        .unwrap_or(1);
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Outcome>>> = Mutex::new((0..cases.len()).map(|_| None).collect());
    let t = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..jobs.min(cases.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = cases.get(i) else {
                        break;
                    };
                    let o = run(case, exact);
                    eprintln!(
                        "  done: {:<7} {:<34} {:<5} {:>6.2} s",
                        case.family,
                        case.what,
                        o.status,
                        o.took.as_secs_f64()
                    );
                    results.lock().unwrap()[i] = Some(o);
                }
            });
        }
    });
    let results: Vec<Outcome> = results
        .into_inner()
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    println!(
        "\n{} mode\n{:<7} {:<34} {:<6} {:>8} {:>9} {:>11} {:>9}  note",
        if exact { "Exact" } else { "Fast preview" },
        "family",
        "case",
        "status",
        "build s",
        "preview",
        "faces",
        "reach mm"
    );
    for (c, o) in cases.iter().zip(&results) {
        println!(
            "{:<7} {:<34} {:<6} {:>8.2} {:>9} {:>5}/{:<5} {:>9}  {}",
            c.family,
            c.what,
            o.status,
            o.took.as_secs_f64(),
            o.preview
                .map_or("-".into(), |p| format!("{:.0}ms", p.as_secs_f64() * 1e3)),
            o.faces[0],
            o.faces[1],
            o.deep.map_or("-".into(), |d| format!("{d:.3}")),
            o.note.chars().take(110).collect::<String>()
        );
    }
    let count = |s: &str| results.iter().filter(|o| o.status == s).count();
    println!(
        "\n{} cases in {:.1} s on {jobs} threads: {} ok, {} approximate, {} with a warning, {} clashing, {} failed, {} out of range",
        results.len(),
        t.elapsed().as_secs_f64(),
        count("ok"),
        count("approx"),
        count("warn"),
        count("CLASH"),
        count("FAIL"),
        count("range")
    );
    assert_eq!(count("FAIL"), 0, "some pairs failed to build");
}
