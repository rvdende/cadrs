//! P3F.3 (`essential-tips.md` T1.2, T4.2, T10, X6, X7): suppression skips kernel work and
//! restores the downstream volume; tapped holes are cosmetic; the scale budgets (250 features and
//! 10 parts in a studio; 40 tabs in a document).
//!
//! Timings are checked on this thread's CPU time where the OS reports it (other workers share the
//! machine), with a loose wall-clock bound, as `kernel_rebuild.rs` does. The release budgets are
//! the article's; a debug build gets 10× (the Rust side is unoptimised; OCCT is the same).
#![cfg(feature = "occt")]
#![allow(clippy::field_reassign_with_default)]

use std::f64::consts::PI;
use std::time::{Duration, Instant};

use cadrs_core::applied::{HoleFeature, HolePoint};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::commands::SetSuppressed;
use cadrs_core::document::{BooleanOp, Document, ExtrudeFeature, FeatureKind};
use cadrs_core::hole::{HoleEnd, HoleSpec, HoleType, Length};
use cadrs_core::rebuild::Rebuilder;
use cadrs_core::samples::{self, scale};
use cadrs_core::{DocumentMeta, ElementId, Feature, FeatureId, History, Store};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

/// This thread's CPU time (user + system), from /proc on Linux; `None` elsewhere.
fn thread_cpu_time() -> Option<Duration> {
    let stat = std::fs::read_to_string("/proc/thread-self/stat").ok()?;
    let rest = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ticks: u64 = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    Some(Duration::from_millis(ticks * 10))
}

/// Runs `f`, returning its result, its wall time and its CPU time on this thread.
fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration, Option<Duration>) {
    let cpu0 = thread_cpu_time();
    let t0 = Instant::now();
    let out = f();
    let wall = t0.elapsed();
    (out, wall, thread_cpu_time().zip(cpu0).map(|(a, b)| a - b))
}

/// Fails if `cpu` (or the wall time where CPU time isn't reported) is over the budget, or the
/// wall time over ten times it.
#[track_caller]
fn within(what: &str, wall: Duration, cpu: Option<Duration>, release: Duration) {
    let budget = if cfg!(debug_assertions) { release * 10 } else { release };
    eprintln!("{what}: {wall:?} wall, {cpu:?} CPU (budget {budget:?})");
    match cpu {
        Some(cpu) => {
            assert!(cpu < budget, "{what} took {cpu:?} of CPU time (budget {budget:?})");
            assert!(wall < budget * 10, "{what} took {wall:?} of wall time (bound {:?})", budget * 10);
        }
        None => assert!(wall < budget, "{what} took {wall:?} (budget {budget:?})"),
    }
}

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("P3F.3");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().active_features()
    }

    fn sketch(&mut self, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(PlaneRef::Top) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn extrude(&mut self, s: FeatureId, seed: Vec2, set: impl FnOnce(&mut ExtrudeFeature)) -> FeatureId {
        let g = self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let mut e = samples::extrude_of(samples::region_refs(s, &g, &[seed]), 0.0);
        set(&mut e);
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() }).unwrap();
        self.h.execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() }).unwrap();
        f
    }

    fn suppress(&mut self, f: FeatureId, on: bool) {
        self.h
            .execute(&mut self.d, &SetSuppressed { element: self.el, features: vec![f], suppressed: on, label: "Suppress".into() })
            .unwrap();
    }
}

fn volume(rb: &mut Rebuilder, features: &[Feature]) -> (f64, usize) {
    let b = rb.rebuild(features);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    (b.parts.iter().map(|p| p.mass.unwrap().volume).sum(), b.computed)
}

/// A 50 mm cube on Top, extruded down (z −50…0).
fn cube() -> Doc {
    let mut d = Doc::new();
    let v = Vec2::new;
    let s = d.sketch(vec![SketchOp::AddPolyline {
        points: vec![v(0.0, 0.0), v(50.0, 0.0), v(50.0, 50.0), v(0.0, 50.0)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }]);
    d.extrude(s, v(1.0, 1.0), |e| {
        e.depth = 50.0;
        e.depth_expr = "50 mm".into();
        e.flip = true;
        e.op = BooleanOp::New;
    });
    d
}

#[test]
fn suppressing_a_hole_skips_it_and_restores_the_volume() {
    // T10.1/T10.2, X6: a Ø10 × 20 flat-bottomed hole (a circle cut 20 down) in a 50 mm cube.
    //   V = 50³ − π·5²·20 = 125 000 − 500π = 123 429.204 mm³; suppressed, 125 000 exactly.
    let mut d = cube();
    let s = d.sketch(vec![SketchOp::AddCircle { center: Vec2::new(25.0, 25.0), radius: 5.0, construction: false }]);
    let hole = d.extrude(s, Vec2::new(25.0, 25.0), |e| {
        e.depth = 20.0;
        e.depth_expr = "20 mm".into();
        e.flip = true;
        e.op = BooleanOp::Remove;
    });
    let mut rb = Rebuilder::new();
    let (v, _) = volume(&mut rb, &d.features());
    close(v, 125_000.0 - PI * 25.0 * 20.0, 1e-6);
    // Suppressed: the hole is left out of the rebuild altogether (no kernel work: the cube's
    // result comes from the cache, nothing is computed) and the cube is whole again.
    d.suppress(hole, true);
    assert!(d.features().iter().all(|f| f.id != hole), "a suppressed feature isn't built");
    assert!(d.d.element(d.el).unwrap().feature(hole).is_some(), "but it is kept in the list");
    let (v, computed) = volume(&mut rb, &d.features());
    close(v, 125_000.0, 1e-6);
    assert_eq!(computed, 0, "nothing recomputed: the cube is cached");
    // Unsuppressed: the hole again (from the cache: nothing recomputed either).
    d.suppress(hole, false);
    let (v, computed) = volume(&mut rb, &d.features());
    close(v, 125_000.0 - PI * 25.0 * 20.0, 1e-6);
    assert!(computed <= 1, "{computed}");
    // A fresh rebuilder gives the same.
    let (v, _) = volume(&mut Rebuilder::new(), &d.features());
    close(v, 125_000.0 - PI * 25.0 * 20.0, 1e-6);
}

#[test]
fn a_hole_feature_suppressed_and_unsuppressed() {
    // The same with a Hole feature: Ø10 Blind 20 with its 118° drill point, from the cube's top
    // at its centre: V = 125 000 − π·25·20 − π·25·h/3, h = 5 / tan 59°.
    let mut d = cube();
    let p = d.sketch(vec![SketchOp::AddPoint { pos: Vec2::new(25.0, 25.0) }]);
    let g = d.d.element(d.el).unwrap().feature(p).unwrap().sketch().unwrap().geometry.clone();
    let mut spec = HoleSpec::default();
    spec.diameter = Length::mm(10.0);
    spec.end = HoleEnd::Blind;
    spec.depth = Length::mm(20.0);
    let f = FeatureId::new();
    let point = HolePoint { sketch: p, point: g.points.keys().next().unwrap() };
    d.h.execute(&mut d.d, &AddFeature::hole(d.el, f, HoleFeature { points: vec![point], spec, ..HoleFeature::default() })).unwrap();
    let tip = 5.0 / 59f64.to_radians().tan();
    let with_hole = 125_000.0 - PI * 25.0 * 20.0 - PI * 25.0 * tip / 3.0;
    let mut rb = Rebuilder::new();
    close(volume(&mut rb, &d.features()).0, with_hole, 1e-6);
    d.suppress(f, true);
    let (v, computed) = volume(&mut rb, &d.features());
    close(v, 125_000.0, 1e-6);
    assert_eq!(computed, 0);
    d.suppress(f, false);
    close(volume(&mut rb, &d.features()).0, with_hole, 1e-6);
}

#[test]
fn tapped_holes_are_cosmetic() {
    // T10.2: an M10×1.5 tapped hole, 20 deep with 15 of thread: the solid is the plain tap drill
    // (Ø8.5 and its point; no helical faces), the thread is cosmetic: its major diameter and
    // length come with the hole for the display on the face and the callout.
    let mut d = cube();
    let p = d.sketch(vec![SketchOp::AddPoint { pos: Vec2::new(25.0, 25.0) }]);
    let g = d.d.element(d.el).unwrap().feature(p).unwrap().sketch().unwrap().geometry.clone();
    let mut spec = HoleSpec::default();
    spec.hole_type = HoleType::Tapped;
    spec.size = "M10".into();
    spec.apply_table();
    spec.end = HoleEnd::Blind;
    spec.depth = Length::mm(20.0);
    spec.tapped_depth = Length::mm(15.0);
    let f = FeatureId::new();
    let point = HolePoint { sketch: p, point: g.points.keys().next().unwrap() };
    d.h.execute(&mut d.d, &AddFeature::hole(d.el, f, HoleFeature { points: vec![point], spec, ..HoleFeature::default() })).unwrap();
    let features = d.features();
    let b = Rebuilder::new().rebuild(&features);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let part = &b.parts[0];
    // The cube's 6 faces, the drill's wall and its point: nothing else (no thread geometry).
    assert_eq!(part.solid.faces.len(), 8);
    let (r, tip) = (4.25, 4.25 / 59f64.to_radians().tan());
    close(part.mass.unwrap().volume, 125_000.0 - PI * r * r * 20.0 - PI * r * r * tip / 3.0, 1e-6);
    // The cosmetic thread: M10 major, from the top face 15 down.
    let threads = cadrs_core::views::threads(&[part], &features);
    assert_eq!(threads.len(), 1);
    let t = &threads[0];
    close(t.major, 10.0, 1e-9);
    close(t.minor, 8.5, 1e-9);
    close(t.length, 15.0, 1e-9);
    close(t.center[2], 0.0, 1e-9);
    // The callout names the thread.
    let name = &d.d.element(d.el).unwrap().feature(f).unwrap().name;
    assert!(name.starts_with("M10x1.5"), "{name}");
}

#[test]
fn a_studio_of_250_features_and_10_parts() {
    // T4.2, X7: 10 plates of 11 holes, 250 features.
    let mut features = scale::studio_features(10, 11);
    assert_eq!(features.len(), 250);
    let _ = Rebuilder::new().rebuild(&features[..3]);
    let mut rb = Rebuilder::new();
    let (b, wall, cpu) = timed(|| rb.rebuild(&features));
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 10);
    for p in &b.parts {
        close(p.mass.unwrap().volume, scale::plate_volume(11), 1e-6);
    }
    eprintln!("250 features from scratch: {wall:?} wall, {cpu:?} CPU");
    if std::env::var_os("CADRS_SCALE_PROFILE").is_some() {
        let mut t: Vec<_> = b.times.iter().map(|(f, d)| (d.as_secs_f64() * 1e3, features.iter().find(|x| x.id == *f).map(|x| x.name.clone()).unwrap_or_default())).collect();
        let sum: f64 = t.iter().map(|x| x.0).sum();
        t.sort_by(|a, b| b.0.total_cmp(&a.0));
        eprintln!("features {sum:.0} ms in all; slowest {:?}; median {:.1} ms", &t[..5], t[t.len() / 2].0);
    }
    // Editing the last feature (the last hole's depth): only it is computed, < 100 ms.
    let last = features.len() - 1;
    if let FeatureKind::Extrude(e) = &mut features[last].kind {
        e.depth = 6.0;
        e.depth_expr = "6 mm".into();
    }
    let (b, wall, cpu) = timed(|| rb.rebuild(&features));
    assert!(b.errors.is_empty());
    assert_eq!(b.computed, 1);
    within("250 features, last edited", wall, cpu, Duration::from_millis(100));
    // Editing the first sketch (plate 1 wider): everything after it is computed, < 3 s.
    if let FeatureKind::Sketch(s) = &mut features[1].kind {
        let g = &mut s.geometry;
        let far: Vec<_> = g.points.iter().filter(|(_, p)| p.pos.x > 49.0).map(|(k, _)| k).collect();
        for k in far {
            g.points[k].pos.x = 55.0;
        }
    }
    let (b, wall, cpu) = timed(|| rb.rebuild(&features));
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    // (Its 120 part features: the plates and holes; the sketches and variables make no parts.)
    assert_eq!(b.computed, 120, "every part feature below the first sketch");
    close(b.part(scale::plate_part(0)).unwrap().mass.unwrap().volume, scale::plate_volume(11) + 5.0 * 40.0 * 10.0, 1e-6);
    within("250 features, first sketch edited", wall, cpu, Duration::from_secs(3));
}

#[test]
fn a_document_of_40_tabs() {
    // T1.2: a document with 40 tabs saves and opens (read and parsed, every tab's features) in
    // < 2 s; switching to a tab is its own rebuild, < 100 ms for a small studio. (The first
    // tab's 250-feature rebuild runs in the background in the app, under the budget of
    // `a_studio_of_250_features_and_10_parts`; its time is printed here.)
    let doc = scale::document(40);
    assert_eq!(doc.elements.len(), 40);
    let dir = std::env::temp_dir().join(format!("cadrs-scale-{}", std::process::id()));
    let store = Store::new(&dir);
    let meta = DocumentMeta::new("me", 0);
    store.save(&doc, &meta).unwrap();
    let (file, wall, cpu) = timed(|| store.load(doc.id).unwrap());
    assert_eq!(file.document.elements.len(), 40);
    assert_eq!(file.document.elements[0].features().len(), 250);
    within("40 tabs opened (read)", wall, cpu, Duration::from_secs(2));
    let _ = Rebuilder::new().rebuild(&doc.elements[1].active_features());
    let (b, wall, cpu) = timed(|| Rebuilder::new().rebuild(&file.document.elements[0].active_features()));
    assert_eq!(b.parts.len(), 10);
    eprintln!("its first tab (250 features) rebuilt: {wall:?} wall, {cpu:?} CPU");
    // Switching to each small studio in turn: its own rebuild.
    let mut worst = (Duration::ZERO, Some(Duration::ZERO));
    for el in doc.elements.iter().skip(1).filter(|e| !e.features().is_empty()).take(10) {
        let mut rb = Rebuilder::new();
        let (b, wall, cpu) = timed(|| rb.rebuild(&el.active_features()));
        assert_eq!(b.parts.len(), 1);
        if wall > worst.0 {
            worst = (wall, cpu);
        }
    }
    within("tab switch (worst of 10)", worst.0, worst.1, Duration::from_millis(100));
    let _ = std::fs::remove_dir_all(&dir);
}
