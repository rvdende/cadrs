//! Rebuilds a feature list dumped by the importer (`CADRS_ONSHAPE_DUMP=<file>`), one more
//! feature at a time, printing how long each step takes: finds the feature a hanging rebuild
//! is stuck on.
//!
//! `cargo run -p cadrs_onshape --example rebuild_dump -- <file.ron> [first step]`

use cadrs_core::document::Feature;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("a dumped feature list");
    let first: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let text = std::fs::read_to_string(&path).expect("readable");
    let features: Vec<Feature> = ron::from_str(&text).expect("a feature list");
    for n in first..=features.len() {
        let f = &features[n - 1];
        eprint!("{n:3} {:<24} ", f.name);
        let started = std::time::Instant::now();
        let b = cadrs_core::rebuild::build(&features[..n]);
        let err = b.errors.iter().find(|(id, _)| *id == f.id).map(|(_, e)| e.as_str()).unwrap_or("");
        eprintln!("{:8.3} s  {} parts  {err}", started.elapsed().as_secs_f64(), b.parts.len());
    }
}
