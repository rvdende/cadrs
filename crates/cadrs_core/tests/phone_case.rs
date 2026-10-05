//! P3H.5: the Board Exercise's stand-in phone case (`samples::phone_case`) and the managed
//! in-context workflow (Create Part Studio in context, Insert and go to Assembly, Update
//! context after the case is resized).

use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::context;
use cadrs_core::assembly::mate::MateKind;
use cadrs_core::command::History;
use cadrs_core::samples::phone_case as pc;
use cadrs_core::studio::DocHistory;

fn cavity_extent(doc: &cadrs_core::Document) -> (f64, f64) {
    // The Board's bounding box in x and y (its outline is the cavity's).
    let features = doc.element(pc::BOARD_STUDIO).unwrap().active_features();
    let solids = context::solids(doc, pc::BOARD_STUDIO);
    let _ = solids;
    let b = cadrs_core::rebuild::build(&features);
    let board = b.part(pc::BOARD_PART).expect("the board builds");
    let (lo, hi) = board.solid.bounds().unwrap();
    assert!((hi[2] - lo[2] - pc::BOARD_THICKNESS).abs() < 1e-9, "thickness {}", hi[2] - lo[2]);
    assert!((lo[2] - pc::BATTERY_Z.1).abs() < 1e-9, "on the battery's top face");
    (hi[0] - lo[0], hi[1] - lo[1])
}

#[test]
fn the_phone_case_builds() {
    let doc = pc::document().unwrap();
    assert_eq!(doc.elements.len(), 2);
    assert_eq!(doc.elements[0].name, "Enclosure");
    assert_eq!(doc.elements[1].name, "Cell phone");
    let b = cadrs_core::rebuild::build(&doc.element(pc::ENCLOSURE_STUDIO).unwrap().active_features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), 3);
    let case = b.part(pc::ENCLOSURE).unwrap();
    let (lo, hi) = case.solid.bounds().unwrap();
    assert!((hi[0] - lo[0] - pc::WIDTH).abs() < 1e-6 && (hi[1] - lo[1] - pc::LENGTH).abs() < 1e-6);
    let asm = doc.element(pc::CELL_PHONE).unwrap().assembly_model().unwrap();
    assert_eq!(asm.instances.len(), 3);
    assert!(matches!(&asm.mate(pc::GROUP_1).unwrap().kind, MateKind::Group { instances } if instances.len() == 3));
}

#[test]
fn the_phone_case_fixture_is_current() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test phone_case`.
    let doc = pc::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/phone_case_standin.cadrs");
    let text = ron::ser::to_string_pretty(&pc::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/phone_case_standin.cadrs is out of date");
}

#[test]
fn board_in_context_follows_the_resized_case() {
    let mut doc = pc::document().unwrap();
    let mut h = History::default();
    let s = &mut DocHistory(&mut doc, &mut h);
    pc::board_in_context(s).unwrap();
    // The studio: its context is the assembly at the Origin; three parts, named.
    let st = doc.element(pc::BOARD_STUDIO).unwrap();
    assert_eq!(st.name, "Board & Keep out");
    let ctx = st.contexts[0].clone();
    assert_eq!(ctx.assembly, pc::CELL_PHONE);
    // MC3.3: made at the Origin, then the first inserted part became the primary instance.
    assert_ne!(ctx.instance, InstanceId::ORIGIN);
    assert_eq!(cadrs_core::assembly::context::studio_of(&doc, pc::CELL_PHONE, ctx.instance), Some(pc::BOARD_STUDIO));
    assert_eq!(ctx.parts.len(), 3, "Enclosure, Battery, Antenna");
    // Inserted where the studio has them, and grouped.
    let asm = doc.element(pc::CELL_PHONE).unwrap().assembly_model().unwrap();
    assert_eq!(asm.instances.len(), 6);
    assert!(matches!(&asm.mate(pc::GROUP_1).unwrap().kind, MateKind::Group { instances } if instances.len() == 6));
    // The board fills the cavity: 66 × 136.
    let (w, l) = cavity_extent(&doc);
    assert!((w - 66.0).abs() < 1e-6 && (l - 136.0).abs() < 1e-6, "{w} × {l}");
    // A re-snapshot now changes nothing (the board studio's own parts aren't context).
    assert!(!pc::update_context(&mut DocHistory(&mut doc, &mut h)).unwrap());
    // Step 12: the case goes to 85 × 150; the context is out of date until Update context.
    pc::resize(&mut DocHistory(&mut doc, &mut h), pc::RESIZED.0, pc::RESIZED.1).unwrap();
    let (w, l) = cavity_extent(&doc);
    assert!((w - 66.0).abs() < 1e-6 && (l - 136.0).abs() < 1e-6, "not yet: {w} × {l}");
    assert!(pc::update_context(&mut DocHistory(&mut doc, &mut h)).unwrap());
    let (w, l) = cavity_extent(&doc);
    assert!((w - 81.0).abs() < 1e-6 && (l - 146.0).abs() < 1e-6, "{w} × {l}");
    // One undo step takes the update back.
    h.undo(&mut doc).unwrap();
    let (w, _) = cavity_extent(&doc);
    assert!((w - 66.0).abs() < 1e-6);
}
