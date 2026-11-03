//! Rebuilds a saved document's Part Studios and prints their errors:
//! `CADRS_DOC=<document dir or document.ron> cargo test -r -p cadrs_core --test debug_document -- --ignored --nocapture`
//! (with `CADRS_DUMP_DIR` to dump the failing kernel operations' inputs).

#[test]
#[ignore]
fn debug_rebuild_document() {
    let Ok(path) = std::env::var("CADRS_DOC") else {
        return;
    };
    let mut p = std::path::PathBuf::from(path);
    if p.is_dir() {
        p.push("document.ron");
    }
    let file = cadrs_core::store::Store::load_path(&p).unwrap();
    for el in &file.document.elements {
        if el.features().is_empty() {
            continue;
        }
        // As the app builds it: with the Variable Studios' variables the list names first.
        let features = &el.active_features();
        let b = cadrs_core::rebuild::build(features);
        println!("{}: {} parts in {:?}", el.name, b.parts.len(), b.elapsed);
        if std::env::var("CADRS_DOC_FACES").is_ok() {
            for part in &b.parts {
                for (i, f) in part.solid.faces.iter().enumerate() {
                    println!("  face {i} {:?} at {:?}", f.name, part.solid.face_point(i));
                }
            }
        }
        for (f, e) in &b.errors {
            let name = features
                .iter()
                .find(|x| x.id == *f)
                .map_or("?", |x| x.name.as_str());
            println!("  ERROR {name}: {e}");
        }
    }
}
