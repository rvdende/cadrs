//! Times the region search of every sketch in a feature list dumped by the importer
//! (`CADRS_ONSHAPE_DUMP=<file>`), Derived features' sources included: finds a sketch whose
//! regions are slow to work out.
//!
//! `cargo run -r -p cadrs_onshape --example regions_dump -- <file.ron>`

use cadrs_core::document::{Feature, FeatureKind};

fn walk(features: &[Feature], depth: usize) {
    for f in features {
        if let Some(sk) = f.sketch() {
            let g = &sk.geometry;
            let started = std::time::Instant::now();
            let regions = cadrs_sketch::region::regions(g);
            let holes: usize = regions.iter().map(|r| r.holes.len()).sum();
            eprintln!(
                "{:indent$}{:<16} {:8.3} s  {} curves, {} imprinted, {} regions, {} holes",
                "",
                f.name,
                started.elapsed().as_secs_f64(),
                g.curves.len(),
                g.imprint.len(),
                regions.len(),
                holes,
                indent = depth * 2
            );
        }
        if let FeatureKind::Derived(d) = &f.kind {
            eprintln!("{:indent$}{} (Derived):", "", f.name, indent = depth * 2);
            walk(&d.studio, depth + 1);
        }
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("a dumped feature list");
    let text = std::fs::read_to_string(&path).expect("readable");
    let features: Vec<Feature> = ron::from_str(&text).expect("a feature list");
    walk(&features, 0);
}
