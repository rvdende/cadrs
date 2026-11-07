//! P3.8 / PS27: the Rocket Guidance Reflector stand-in's fixture
//! (`fixtures/reflector_standin.cadrs`) is the document `cadrs_core::samples::reflector`
//! builds. `course_ps27_reflector` makes the course steps through the UI.
#![cfg(feature = "brep")]

use cadrs_core::samples::reflector as rf;

#[test]
fn the_fixture_is_current() {
    // `fixtures/reflector_standin.cadrs` is the stand-in document as `rf::document()` builds
    // it. Regenerate it with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test reflector`.
    let doc = rf::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reflector_standin.cadrs");
    let file = rf::file(doc.clone());
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(
        stored.document, doc,
        "fixtures/reflector_standin.cadrs is out of date"
    );
}
