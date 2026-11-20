//! The "Bull-nose profile" fixture (`fixtures/cnc/bull-nose-profile.cadrs`,
//! [`cadrs_core::samples::bull_nose_profile`]): brep-bench's `cnc/bull-nose-profile` as a part and
//! its CAM job. Its toolpath follows the flower's arcs exactly, and Simulate's exact stock cut
//! by it is the case's solid.

use std::collections::HashMap;

use cadrs_cam::toolpath::Step;
use cadrs_core::cam::{CamId, geometry, program};
use cadrs_core::samples::bull_nose_profile as bn;
use cadrs_core::{Document, ElementId};

fn document() -> Document {
    bn::document().expect("the sample builds")
}

fn toolpath(doc: &Document, studio: ElementId, op: CamId) -> cadrs_cam::Toolpath {
    let cam = doc.element(studio).unwrap().cam().unwrap();
    let parts = cadrs_core::rebuild::build(cam.part_features())
        .parts
        .clone();
    let input = program::op_input(cam, op, &parts).unwrap();
    let model = |levels: Vec<f64>| {
        geometry::model(program::model_request(cam, &input, levels))
            .wait()
            .expect("the kernel")
            .unwrap()
    };
    let first = model(vec![]);
    let levels = program::section_levels(&input, &first);
    let model = if levels.is_empty() {
        first
    } else {
        model(levels)
    };
    program::generate(&input, &model, None).unwrap()
}

#[test]
fn the_fixture_is_current() {
    // `fixtures/cnc/bull-nose-profile.cadrs` is the sample as `bull_nose_profile::document()`
    // builds it. Regenerate it with
    // `CADRS_WRITE_FIXTURES=1 cargo test -r -p cadrs_core --test bull_nose the_fixture`.
    let doc = document();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cnc/bull-nose-profile.cadrs");
    let text =
        ron::ser::to_string_pretty(&bn::file(doc.clone()), ron::ser::PrettyConfig::default())
            .unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(
        stored.document, doc,
        "fixtures/cnc/bull-nose-profile.cadrs is out of date"
    );
}

/// The plate as the Part Studio builds it is the case's solid (its mesh's volume within a
/// part in a thousand: the exact measure is the simulation's, below).
#[test]
fn the_part_is_the_case() {
    let doc = document();
    let features = doc.element(bn::PLATE).unwrap().active_features();
    let build = cadrs_core::rebuild::build(&features);
    for f in &features {
        assert!(
            build.error(f.id).is_none(),
            "{:?}: {:?}",
            f.id,
            build.error(f.id)
        );
    }
    let v = build.part(bn::PART).expect("the plate").solid.volume();
    assert!(
        (v - bn::VOLUME).abs() < 1e-3 * bn::VOLUME,
        "volume {v} (the case's {})",
        bn::VOLUME
    );
}

/// The profile runs round the flower on its own arcs: thirteen a lap at most (the lobe it
/// starts on in two), three laps, at −4, −8 and −10.
#[test]
fn the_toolpath_follows_the_flowers_arcs() {
    let doc = document();
    let tp = toolpath(&doc, bn::CAM, bn::PROFILE);
    let d = bn::lobe_to_valley();
    // (In the model's coordinates: the toolpath is in the setup's.)
    let cam = doc.element(bn::CAM).unwrap().cam().unwrap();
    let parts = cadrs_core::rebuild::build(cam.part_features())
        .parts
        .clone();
    let (job, setup) = cam.setup(bn::SETUP).unwrap();
    let frame = program::setup_frame(cam, job, setup, &parts);
    let world = |p: [f64; 3]| frame.to_world(p);
    let mut depths: Vec<f64> = Vec::new();
    let mut arcs = 0;
    for s in &tp.steps {
        match s {
            Step::Arc { to, center, .. } => {
                arcs += 1;
                let (to, c) = (world(*to), world([center[0], center[1], to[2]]));
                let center = [c[0], c[1]];
                // About a lobe's centre at 12, or a valley's at d − 12.
                let c = (
                    (center[0] - 60.0).hypot(center[1] - 60.0),
                    (to[0] - center[0]).hypot(to[1] - center[1]),
                );
                let lobe = (c.0 - 30.0).abs() < 1e-9 && (c.1 - 12.0).abs() < 1e-9;
                let valley = (c.0 - 40.0).abs() < 1e-9 && (c.1 - (d - 12.0)).abs() < 1e-9;
                assert!(lobe || valley, "an arc about {center:?} to {to:?}");
                if !depths.iter().any(|z| (z - to[2]).abs() < 1e-9) {
                    depths.push(to[2]);
                }
            }
            Step::Feed { to, .. } => {
                let to = world(*to);
                // Feeds only straight down (or up) onto the flower: no stray moves across.
                let r = (0..6)
                    .map(|k| {
                        let a = (60.0 * k as f64).to_radians();
                        ((to[0] - 60.0 - 30.0 * a.cos()).hypot(to[1] - 60.0 - 30.0 * a.sin())
                            - 12.0)
                            .abs()
                    })
                    .chain((0..6).map(|k| {
                        let a = (30.0 + 60.0 * k as f64).to_radians();
                        ((to[0] - 60.0 - 40.0 * a.cos()).hypot(to[1] - 60.0 - 40.0 * a.sin())
                            - (d - 12.0))
                            .abs()
                    }))
                    .fold(f64::INFINITY, f64::min);
                assert!(r < 1e-9, "a feed to {to:?}, {r:.2e} off the flower");
            }
            _ => {}
        }
    }
    depths.sort_by(f64::total_cmp);
    assert_eq!(depths, vec![-10.0, -8.0, -4.0], "{:?}", tp.steps);
    assert!((36..=42).contains(&arcs), "{arcs} arcs: {:?}", tp.steps);
}

/// Simulate's exact stock at the run's end is the case's solid: its volume and area to a part
/// in a billion (the case's settled answer, worked out by hand).
#[test]
fn the_simulated_cut_is_the_case() {
    let doc = document();
    let cam = doc.element(bn::CAM).unwrap().cam().unwrap();
    let parts = cadrs_core::rebuild::build(cam.part_features())
        .parts
        .clone();
    let paths: HashMap<CamId, cadrs_cam::Toolpath> =
        HashMap::from([(bn::PROFILE, toolpath(&doc, bn::CAM, bn::PROFILE))]);
    let (job, setup) = cam.setup(bn::SETUP).unwrap();
    let run = program::setup_run(cam, job, setup, &parts, &paths, None).unwrap();
    let machine = job.machine.as_ref().unwrap();
    let shape = program::stock_shape(cam, job, &parts).unwrap();
    let stock = program::stock_in(&shape, &run.frame);
    let above = [0.0, 0.0, stock.bounds.max[2] + 30.0];
    let motions = program::run_motions(setup, job, &run, above);
    let track = program::axis_track(machine, &run, &motions);
    let moves: usize = motions.iter().map(|m| m.len()).sum();
    let mut exact =
        cadrs_core::cam::exact::ExactStock::new(machine, &run, &shape, &motions, &track.rows)
            .unwrap();
    assert!(
        (exact.volume() - 120.0 * 120.0 * 16.0).abs() < 1e-6,
        "the stock is the plate's box"
    );
    let t = std::time::Instant::now();
    exact.cut_to(moves - 1, 1.0);
    println!(
        "{moves} moves in {} cuts: {:.2} s, volume {}, area {}",
        exact.cut_count(),
        t.elapsed().as_secs_f64(),
        exact.volume(),
        exact.area()
    );
    assert!(exact.failed.is_empty(), "{:?}", exact.failed);
    let (v, a) = (exact.volume(), exact.area());
    assert!(
        (v - bn::VOLUME).abs() < 1e-9 * bn::VOLUME,
        "volume {v} (the case's {})",
        bn::VOLUME
    );
    assert!(
        (a - bn::AREA).abs() < 1e-9 * bn::AREA,
        "area {a} (the case's {})",
        bn::AREA
    );
}
