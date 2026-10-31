//! Compares an import with Onshape feature by feature: rebuilds the imported features up to
//! each Onshape feature and prints every part's faces (exact area mm², mesh box mm) beside
//! Onshape's at the same rollback bar (`rollback/bodydetails-NN.json`, fetched with
//! `bodydetails?rollbackBarIndex=NN`). Faces are paired by their boxes.
//!
//! `CADRS_ONSHAPE_DUMP=<file.ron> cadrs-onshape import <doc> --dry-run` writes the features;
//! `cargo run -r -p cadrs_onshape --example rollback_check -- <file.ron> <element dir> [N…]`

use cadrs_core::document::Feature;
use serde_json::Value;

type Box3 = ([f64; 3], [f64; 3]);

fn main() {
    let mut args = std::env::args().skip(1);
    let dump = args.next().expect("a dumped feature list");
    let dir = std::path::PathBuf::from(args.next().expect("the Part Studio's raw folder"));
    let only: Vec<usize> = args.filter_map(|a| a.parse().ok()).collect();
    let features: Vec<Feature> =
        ron::from_str(&std::fs::read_to_string(&dump).expect("readable")).expect("a feature list");
    let json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("features.json")).expect("features.json"),
    )
    .expect("json");
    let names: Vec<String> = json["features"]
        .as_array()
        .expect("features")
        .iter()
        .map(|f| f["name"].as_str().unwrap_or_default().to_string())
        .collect();
    for n in 1..=names.len() {
        if !only.is_empty() && !only.contains(&n) {
            continue;
        }
        let path = dir.join(format!("rollback/bodydetails-{n:02}.json"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let theirs: Value = serde_json::from_str(&text).expect("json");
        // The imported features up to the last one named as an Onshape feature so far.
        let upto = features
            .iter()
            .rposition(|f| names[..n].contains(&f.name))
            .map_or(0, |i| i + 1);
        let b = cadrs_core::rebuild::build(&features[..upto]);
        println!("== {n:2} after {}", names[n - 1]);
        for (id, e) in &b.errors {
            if let Some(f) = features.iter().find(|f| f.id == *id) {
                println!("   error in {}: {e}", f.name);
            }
        }
        let mut ours: Vec<(String, f64, Box3)> = Vec::new();
        for p in &b.parts {
            let s = &p.solid;
            for f in &s.faces {
                let tris =
                    &s.indices[f.first_triangle * 3..(f.first_triangle + f.triangle_count) * 3];
                let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                for &t in tris {
                    let q = s.positions[t as usize];
                    for k in 0..3 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
                ours.push((p.name.clone(), f.area.unwrap_or(f64::NAN), (lo, hi)));
            }
        }
        let mut used = vec![false; ours.len()];
        for body in theirs["bodies"].as_array().into_iter().flatten() {
            for f in body["faces"].as_array().into_iter().flatten() {
                let area = f["area"].as_f64().unwrap_or(0.0) * 1e6;
                let corner = |c: &Value| {
                    [
                        c["x"].as_f64().unwrap_or(0.0) * 1e3,
                        c["y"].as_f64().unwrap_or(0.0) * 1e3,
                        c["z"].as_f64().unwrap_or(0.0) * 1e3,
                    ]
                };
                let bx = (
                    corner(&f["box"]["minCorner"]),
                    corner(&f["box"]["maxCorner"]),
                );
                let gap = |o: &Box3| {
                    (0..3)
                        .map(|k| (o.0[k] - bx.0[k]).abs().max((o.1[k] - bx.1[k]).abs()))
                        .fold(0.0, f64::max)
                };
                let best = (0..ours.len())
                    .filter(|&i| !used[i])
                    .min_by(|&a, &b| gap(&ours[a].2).total_cmp(&gap(&ours[b].2)));
                let fmt = |b: &Box3| {
                    format!(
                        "[{:7.2} {:7.2} {:7.2} | {:7.2} {:7.2} {:7.2}]",
                        b.0[0], b.0[1], b.0[2], b.1[0], b.1[1], b.1[2]
                    )
                };
                match best.filter(|&i| gap(&ours[i].2) < 5.0) {
                    Some(i) => {
                        used[i] = true;
                        let o = &ours[i];
                        println!(
                            "   {:4} {:4} area {:10.2} vs {:10.2} ({:+.3}%)  box off {:.3}  {}",
                            body["id"].as_str().unwrap_or(""),
                            f["id"].as_str().unwrap_or(""),
                            o.1,
                            area,
                            (o.1 - area) / area * 100.0,
                            gap(&o.2),
                            fmt(&bx)
                        );
                    }
                    None => println!(
                        "   {:4} {:4} MISSING area {:10.2} {}",
                        body["id"].as_str().unwrap_or(""),
                        f["id"].as_str().unwrap_or(""),
                        area,
                        fmt(&bx)
                    ),
                }
            }
        }
        for (i, o) in ours.iter().enumerate() {
            if !used[i] {
                println!("   EXTRA {} area {:10.2} {:?}", o.0, o.1, o.2);
            }
        }
    }
}
