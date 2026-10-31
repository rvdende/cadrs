//! Prints each part of a dumped feature list's rebuild with its faces' names and areas.
//!
//! `cargo run -r -p cadrs_onshape --example parts_dump -- <file.ron> [features]`

use cadrs_core::document::Feature;

fn main() {
    let mut args = std::env::args().skip(1);
    let dump = args.next().expect("a dumped feature list");
    let features: Vec<Feature> =
        ron::from_str(&std::fs::read_to_string(&dump).expect("readable")).expect("a feature list");
    let n = args
        .next()
        .and_then(|a| a.parse().ok())
        .unwrap_or(features.len());
    let b = cadrs_core::rebuild::build(&features[..n]);
    for (id, e) in &b.errors {
        let name = features
            .iter()
            .find(|f| f.id == *id)
            .map_or("?", |f| &f.name);
        println!("error in {name}: {e}");
    }
    for p in &b.parts {
        println!("part {} {:?} ({:?})", p.name, p.kind, p.id);
        for f in &p.solid.faces {
            println!(
                "   {:<40} {:10.2}  {:?} centre {:?}",
                f.name.describe(),
                f.area.unwrap_or(f64::NAN),
                f.name.origin,
                f.center.map(|c| c.map(|x| (x * 10.0).round() / 10.0))
            );
        }
        if std::env::var_os("FREE_EDGES").is_some() {
            for e in &p.solid.edges {
                if e.name.faces[0] == e.name.faces[1] {
                    println!(
                        "   free edge of {:?} idx {}: {:?} .. {:?} ({} pts)",
                        e.name.faces[0].origin,
                        e.name.index,
                        e.points
                            .first()
                            .map(|q| q.map(|x| (x * 100.0).round() / 100.0)),
                        e.points
                            .last()
                            .map(|q| q.map(|x| (x * 100.0).round() / 100.0)),
                        e.points.len()
                    );
                }
            }
        }
        for a in &p.solid.face_aliases {
            println!(
                "   alias {:?} -> {:?} split {}",
                a.name.origin, a.face.origin, a.face.split
            );
        }
    }
}
