//! P3H.6: Create an assembly from this ECAD data (PCB7, X7) and the two exercises built on it,
//! Vision Controller (PCB8) and IDF Assembly (PCB10), on the authored fixtures. The expected
//! numbers are worked out by hand in each test.

use cadrs_core::Document;
use cadrs_core::assembly::bom::{BomColumn, BomOptions, compute};
use cadrs_core::properties::PropertyKey;
use cadrs_core::assembly::commands::{InsertInstance, MoveInstances};
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::command::History;
use cadrs_core::commands::{AddElement, NewElementKind, RenamePart};
use cadrs_core::ids::{ElementId, FeatureId, PartId};
use cadrs_core::pcb::{BoardSource, ImportBoard, SyncPlaneChoice};
use cadrs_core::studio::DocHistory;
use cadrs_idf::{IdfVersion, Loop, MountSide};
use cadrs_pcb::PcbBoard;
use cadrs_pcb::create_assembly::{CreateOptions, components_studio_name, description, generate};
use cadrs_pcb::sync::{plan, run_now};

const PCB: ElementId = ElementId::from_u128(0x3a06_9000);

fn fixture(folder: &str, name: &str) -> PcbBoard {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/idf").join(folder);
    let emn = std::fs::read_to_string(dir.join(format!("{name}.emn"))).unwrap();
    let emp = std::fs::read_to_string(dir.join(format!("{name}.emp"))).unwrap();
    PcbBoard::read(&emn, &emp).unwrap()
}

/// A document with a PCB Studio and the fixture imported, then Create assembly with `opts`.
fn created(folder: &str, name: &str, opts: CreateOptions) -> (Document, History, cadrs_core::pcb::GeneratedAssembly) {
    let mut doc = Document::new("Exercise");
    let mut h = History::default();
    h.execute(&mut doc, &AddElement { id: PCB, kind: NewElementKind::PcbStudio, name: None, after: None }).unwrap();
    let pcb = fixture(folder, name);
    h.execute(&mut doc, &ImportBoard { element: PCB, board: Box::new(pcb), source: BoardSource::Idf { emn: format!("{name}.emn"), emp: None } }).unwrap();
    let board = doc.element(PCB).unwrap().pcb().unwrap().active.unwrap();
    let cmd = generate(&doc, PCB, board, &opts).unwrap();
    let g = cmd.generated.clone();
    h.execute(&mut doc, &cmd).unwrap();
    (doc, h, g)
}

fn features(doc: &Document, el: ElementId) -> Vec<cadrs_core::Feature> {
    doc.element(el).unwrap().active_features()
}

#[test]
fn create_assembly_makes_a_studio_and_an_assembly_named_after_the_board() {
    // PCB7.2, PCB7.4, PCB7.5 on Vision PCB (29 placements): tabs "Vision PCB" (Part Studio),
    // "Vision PCB components", "Vision PCB" (Assembly) right of the PCB Studio; the studio's
    // features are Sketch 1 and "Board [Vision PCB]", its part "Board [Vision PCB]"; the assembly
    // has the board and 29 components, no mates, named "<package> <n>"; one undo step.
    let (mut doc, mut h, g) = created("vision controller", "Vision PCB", CreateOptions::default());
    let names: Vec<&str> = doc.elements.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names[2..], ["PCB Studio 1", "Vision PCB", "Vision PCB components", "Vision PCB"]);
    let fs = features(&doc, g.studio);
    let fnames: Vec<&str> = fs.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(fnames, ["Sketch 1", "Board [Vision PCB]"]);
    let build = cadrs_core::rebuild::build(&fs);
    assert_eq!(build.parts.len(), 1);
    let props = doc.element(g.studio).unwrap().part_props();
    assert_eq!(cadrs_core::parts::display_name(&build.parts[0], props), "Board [Vision PCB]");
    let asm = doc.element(g.assembly).unwrap().assembly_model().unwrap().clone();
    assert!(doc.element(g.assembly).unwrap().name == "Vision PCB");
    assert_eq!(asm.instances.len(), 30, "the board and 29 components");
    assert!(asm.mates.is_empty(), "no mates (PCB7.4)");
    assert_eq!(g.components.len(), 29);
    // Instance names from the package, numbered per package.
    let u1 = g.components.iter().find(|c| c.refdes == "U1").unwrap();
    let inst = asm.instance(u1.instance).unwrap();
    let name = cadrs_core::assembly::source_part_name(&doc, &inst.source, None);
    assert_eq!(inst.name(&name), "QFP100_600MIL <1>");
    let r = g.components.iter().filter(|c| c.refdes.starts_with('R')).map(|c| asm.instance(c.instance).unwrap().index).max().unwrap();
    assert!(r >= 2, "resistors numbered <1>, <2>, …");
    // One undo step removes all three tabs and the link.
    h.undo(&mut doc);
    assert_eq!(doc.elements.len(), 3);
    assert!(doc.element(PCB).unwrap().pcb().unwrap().generated.is_empty());
}

#[test]
fn create_assembly_second_create_reuses_the_components_studio() {
    // PCB7.3: a second Create (of the same board, then of another board of the same PCB Studio)
    // reuses the components Part Studio: the known packages' parts are used again, only new
    // packages are added, and no second components tab appears. One undo step each.
    let (mut doc, mut h, g1) = created("vision controller", "Vision PCB", CreateOptions::default());
    let comps = g1.components_studio.unwrap();
    let n1 = cadrs_core::rebuild::build(&features(&doc, comps)).parts.len();
    assert_eq!(g1.packages.len(), n1);
    let tabs = doc.elements.len();
    let board = doc.element(PCB).unwrap().pcb().unwrap().active.unwrap();
    let cmd = generate(&doc, PCB, board, &CreateOptions::default()).unwrap();
    assert_eq!(cmd.elements.len(), 2, "a Part Studio and an Assembly, no components tab");
    let g2 = cmd.generated.clone();
    h.execute(&mut doc, &cmd).unwrap();
    assert_eq!(doc.elements.len(), tabs + 2);
    assert_eq!(g2.components_studio, Some(comps));
    assert_eq!(cadrs_core::rebuild::build(&features(&doc, comps)).parts.len(), n1, "nothing new to add");
    // U1's instance uses the same part as the first assembly's.
    let src = |g: &cadrs_core::pcb::GeneratedAssembly| {
        let u1 = g.components.iter().find(|c| c.refdes == "U1").unwrap();
        doc.element(g.assembly).unwrap().assembly_model().unwrap().instance(u1.instance).unwrap().source
    };
    assert_eq!(src(&g1), src(&g2));
    assert_eq!(doc.element(g2.assembly).unwrap().assembly_model().unwrap().instances.len(), 30);
    // Another board: its packages are added after the others.
    let pcb2 = fixture("secondary board", "secondary board");
    let new_packages: std::collections::BTreeSet<(String, String)> = pcb2.board.placements.iter().map(|p| (p.package.clone(), p.part_number.clone())).collect();
    h.execute(&mut doc, &ImportBoard { element: PCB, board: Box::new(pcb2), source: BoardSource::Idf { emn: "secondary board.emn".into(), emp: None } }).unwrap();
    let board2 = doc.element(PCB).unwrap().pcb().unwrap().active.unwrap();
    assert_ne!(board2, board);
    let cmd = generate(&doc, PCB, board2, &CreateOptions::default()).unwrap();
    let g3 = cmd.generated.clone();
    h.execute(&mut doc, &cmd).unwrap();
    assert_eq!(g3.components_studio, Some(comps));
    let known: std::collections::BTreeSet<(String, String)> = g1.packages.iter().map(|p| (p.package.clone(), p.part_number.clone())).collect();
    let added = new_packages.difference(&known).count();
    assert!(added > 0);
    let b = cadrs_core::rebuild::build(&features(&doc, comps));
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.parts.len(), n1 + added);
    // The parts don't overlap: laid side by side along x.
    let mut ranges: Vec<(f64, f64)> = b.parts.iter().map(|p| { let (lo, hi) = p.solid.bounds().unwrap(); (lo[0], hi[0]) }).collect();
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(ranges.windows(2).all(|w| w[0].1 <= w[1].0 + 1e-9), "{ranges:?}");
    // Undo takes the new parts out of the reused studio again.
    h.undo(&mut doc);
    assert_eq!(cadrs_core::rebuild::build(&features(&doc, comps)).parts.len(), n1);
}

#[test]
fn create_assembly_keep_areas_are_parts_only_when_asked() {
    // PCB7.1: Keep-In and Keep-Out Areas off by default. The cell phone board has two place
    // keep-outs: with the box checked the studio has the board and two keep-out parts, which the
    // assembly also holds.
    let (doc, _, g) = created("cell phone", "Cell phone", CreateOptions::default());
    assert_eq!(cadrs_core::rebuild::build(&features(&doc, g.studio)).parts.len(), 1);
    let (doc, _, g) = created("cell phone", "Cell phone", CreateOptions { keep_areas: true, ..CreateOptions::default() });
    let build = cadrs_core::rebuild::build(&features(&doc, g.studio));
    let props = doc.element(g.studio).unwrap().part_props();
    let names: Vec<&str> = build.parts.iter().map(|p| cadrs_core::parts::display_name(p, props)).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names[1..].iter().all(|n| cadrs_pcb::role_of(n) == Some(cadrs_pcb::PartRole::KeepOut)), "{names:?}");
    assert_eq!(doc.element(g.assembly).unwrap().assembly_model().unwrap().instances.len(), 3);
}

#[test]
fn create_assembly_bom_has_one_row_per_part_number_with_description() {
    // PCB7.6: the BOM has one row per part number with its quantity and Description. Vision PCB
    // places four 0603R (RC0603-10K, RESISTANCE 10000, TOLERANCE 1) → "Resistor 10K OHM ±1%".
    let (doc, _, g) = created("vision controller", "Vision PCB", CreateOptions::default());
    let pcb = fixture("vision controller", "Vision PCB");
    let packages: std::collections::BTreeSet<(String, String)> = pcb.board.placements.iter().map(|p| (p.package.clone(), p.part_number.clone())).collect();
    let comps = g.components_studio.unwrap();
    assert_eq!(doc.element(comps).unwrap().name, components_studio_name("Vision PCB"));
    assert_eq!(cadrs_core::rebuild::build(&features(&doc, comps)).parts.len(), packages.len(), "one part per package");
    let b = compute(&doc, g.assembly, &BomOptions::default(), &Default::default(), |e| Some(cadrs_core::rebuild::build(&features(&doc, e)))).unwrap();
    let col = |k: PropertyKey| b.columns.iter().position(|c| *c == BomColumn::Property(k)).unwrap();
    let rows = &b.rows;
    let r = rows.iter().find(|r| r.cells[col(PropertyKey::PartNumber)] == "RC0603-10K").expect("the resistor row");
    let n = pcb.board.placements.iter().filter(|p| p.part_number == "RC0603-10K").count();
    assert!(n >= 2);
    assert_eq!(r.quantity as usize, n);
    assert_eq!(r.cells[col(PropertyKey::Description)], "Resistor 10K OHM ±1%");
    assert_eq!(r.cells[col(PropertyKey::Name)], "0603R");
    let lib = &pcb.library;
    let c = lib.packages.iter().find(|p| p.name == "0603C").unwrap();
    assert_eq!(description(c).as_deref(), Some("Capacitor 0.1uF"));
    let x = lib.packages.iter().find(|p| p.name == "CRYSTAL_HC49").unwrap();
    assert_eq!(description(x).as_deref(), Some("Crystal 16MHz"));
    let q = lib.packages.iter().find(|p| p.name == "QFP100_600MIL").unwrap();
    // Its THETA_JC record comes first: a thermal record isn't a description, its DESCRIPTION is.
    assert_eq!(description(q).as_deref(), Some("Vision processor QFP-100"));
    // P3H.6 judge: every package has one, so every BOM row has a Description (`v6` poster).
    for p in &lib.packages {
        assert!(description(p).is_some(), "{} has no description", p.name);
    }
    // Every row is a part number (the board has none) or the board.
    assert_eq!(rows.len(), packages.len() + 1);
}

#[test]
fn ex2_large_ic_top_face_area() {
    // PCB8's self-check: U1 (QFP100_600MIL, a 15.24 mm square, 0.6 in) — its top face in the
    // generated assembly is 15.24 × 15.24 = 232.2576 mm² (0.36 in²).
    let (doc, _, g) = created("vision controller", "Vision PCB", CreateOptions::default());
    let asm = doc.element(g.assembly).unwrap().assembly_model().unwrap();
    let u1 = g.components.iter().find(|c| c.refdes == "U1").unwrap();
    let InstanceSource::Part { element, part } = asm.instance(u1.instance).unwrap().source else { panic!() };
    let build = cadrs_core::rebuild::build(&features(&doc, element));
    let p = build.part(part).unwrap();
    let pose = asm.instance(u1.instance).unwrap().pose;
    let top = p
        .solid
        .faces
        .iter()
        .filter(|f| f.plane.is_some_and(|fr| pose.rotate(fr.normal())[2] > 0.999))
        .max_by(|a, b| a.plane.unwrap().origin[2].total_cmp(&b.plane.unwrap().origin[2]))
        .unwrap();
    assert!((top.area.unwrap() - 232.2576).abs() < 1e-6, "{:?}", top.area);
    // And it sits on the board's top face, at U1's IDF position.
    let pcb = fixture("vision controller", "Vision PCB");
    let q = pcb.board.placement("U1").unwrap();
    let c = pose.apply(u1.frame.apply([0.0, 0.0, 0.0]));
    assert!((c[0] - q.x).abs() < 1e-9 && (c[1] - q.y).abs() < 1e-9 && (c[2] - pcb.thickness()).abs() < 1e-9, "{c:?}");
}

/// The Ex3 keep-out: a 0.5 × 0.375 in corner profile with an R0.25 in fillet at the board's
/// bottom-left corner (−5.20972, −22.74338), counter-clockwise from that corner.
fn corner_profile() -> Loop {
    let (x0, y0) = (-5.20972, -22.74338);
    let (w, d, r) = (12.7, 9.525, 6.35);
    Loop::from_triples(0, &[(x0, y0, 0.0), (x0 + w, y0, 0.0), (x0 + w, y0 + d - r, 0.0), (x0 + w - r, y0 + d, 90.0), (x0, y0 + d, 0.0), (x0, y0, 0.0)])
}

/// PCB10 end to end: the exported `secondary board.emn`, read back, and the keep-out's area.
fn ex3_exported() -> (String, cadrs_idf::Board, f64) {
    // PCB10: import the secondary board, Create assembly (defaults), in the "secondary board"
    // Part Studio a keep-out on the board's top face (the corner profile extruded 0.125 in =
    // 3.175 mm, New part "Keep-out"), inserted at its studio position; uBGA48_7.4X7.1 (X2)
    // moved 1 in along Y; Group and Fix don't move anything; sync the assembly (Top plane) and
    // export IDF 3.0.
    let (mut doc, mut h, g) = created("secondary board", "secondary board", CreateOptions::default());
    // Step 6: Sketch 2 on the board's top face; step 7: Extrude Blind 0.125 in, New, "Keep-out".
    let (sk, ex) = (FeatureId::from_u128(0x3a06_0001), FeatureId::from_u128(0x3a06_0002));
    cadrs_pcb::sample::ex3_keepout_sketch(&mut DocHistory(&mut doc, &mut h), g.studio, sk).unwrap();
    let e = cadrs_core::document::ExtrudeFeature { sketches: vec![sk], depth: 3.175, depth_expr: "0.125 in".into(), ..Default::default() };
    h.execute(&mut doc, &cadrs_core::commands::AddExtrude { element: g.studio, feature: ex, extrude: e }).unwrap();
    let part = PartId::new(ex, 0);
    h.execute(&mut doc, &RenamePart { element: g.studio, part, name: "Keep-out".into() }).unwrap();
    let names: Vec<String> = features(&doc, g.studio).iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, ["Sketch 1", "Board [secondary board]", "Sketch 2", "Extrude 1"], "as in ex3-step6-keepout-sketch.png");
    // The Keep-out part's volume: area 12.7·9.525 − (1 − π/4)·6.35² = 112.3142… mm², times 3.175.
    let area = 12.7 * 9.525 - (1.0 - std::f64::consts::FRAC_PI_4) * 6.35 * 6.35;
    assert!((area - 112.314).abs() < 1e-3, "{area}");
    let build = cadrs_core::rebuild::build(&features(&doc, g.studio));
    let v = build.part(part).unwrap().mass.unwrap().volume;
    assert!((v - area * 3.175).abs() < 1e-6, "{v} vs {}", area * 3.175);
    h.execute(&mut doc, &InsertInstance { element: g.assembly, instance: Instance::new(InstanceId::new(), InstanceSource::Part { element: g.studio, part }, Pose::IDENTITY) }).unwrap();
    let x2 = g.components.iter().find(|c| c.refdes == "X2").unwrap().instance;
    let before = doc.element(g.assembly).unwrap().assembly_model().unwrap().instance(x2).unwrap().pose;
    let moved = before.then(&Pose::translation([0.0, 25.4, 0.0]));
    h.execute(&mut doc, &MoveInstances { element: g.assembly, poses: vec![(x2, moved)], label: "Move".into() }).unwrap();
    // Sync: the generated assembly updates the imported board in place.
    let shown = doc.element(PCB).unwrap().pcb().unwrap().active;
    let p = plan(&doc, PCB, g.assembly, SyncPlaneChoice::Top, shown).unwrap();
    assert_eq!(p.target, Some(g.board), "the board the assembly was made from");
    assert_eq!(p.components.len(), 20, "every component recognised by its link");
    let o = run_now(p).unwrap();
    assert!(o.unrecognised.is_empty(), "{:?}", o.unrecognised);
    h.execute(&mut doc, &o.command).unwrap();
    let s = doc.element(PCB).unwrap().pcb().unwrap();
    assert_eq!(s.boards.len(), 1, "updated in place, no second board");
    let b = &s.board(g.board).unwrap().board;
    assert_eq!(b.name(), "secondary board");
    // Export IDF 3.0 and read secondary board.emn.
    let zip = cadrs_idf::write_zip(&b.board, &b.library, IdfVersion::V3);
    let (_, emn) = cadrs_idf::zip_entries(&zip).unwrap().into_iter().find(|(n, _)| n.ends_with("secondary board.emn")).unwrap();
    let emn = String::from_utf8(emn).unwrap();
    let (_, emp) = cadrs_idf::zip_entries(&zip).unwrap().into_iter().find(|(n, _)| n.ends_with(".emp")).unwrap();
    let (board, _) = cadrs_idf::read_pair(&emn, &String::from_utf8(emp).unwrap()).unwrap();
    (emn, board, area)
}

#[test]
fn ex3_moved_component_is_25_4_mm_further() {
    // PCB10's self-check: uBGA48_7.4X7.1 (X2) moved 1 in along Y is exported at
    // (4.064182376174947, −16.5 + 25.4 = 8.9), rotation 90, TOP, PLACED; nothing else moved.
    let (emn, board, _) = ex3_exported();
    let q = board.placement("X2").unwrap();
    assert_eq!(q.package, "uBGA48_7.4X7.1");
    assert!((q.x - 4.064182376174947).abs() < 1e-9, "{}", q.x);
    assert!((q.y - (-16.5 + 25.4)).abs() < 1e-9, "{}", q.y);
    assert!((q.rotation - 90.0).abs() < 1e-9, "{}", q.rotation);
    assert_eq!(q.side, MountSide::Top);
    assert_eq!(q.status, cadrs_idf::Status::Placed);
    // The other components didn't move.
    let pcb = fixture("secondary board", "secondary board");
    for p in pcb.board.placements.iter().filter(|p| p.refdes != "X2") {
        let q = board.placement(&p.refdes).unwrap();
        assert!((q.x - p.x).abs() < 1e-9 && (q.y - p.y).abs() < 1e-9, "{}", p.refdes);
    }
    // The .emn lists X2 as two lines: package, part number, designator; x y offset angle side status.
    let i = emn.lines().position(|l| l.starts_with("uBGA48_7.4X7.1 ")).unwrap();
    let line = emn.lines().nth(i + 1).unwrap();
    assert_eq!(line.split_whitespace().next(), Some("4.064182376174947"), "{line}");
    // No float noise: Y prints as the course's 8.9, not 8.900000000000002.
    assert_eq!(line.split_whitespace().nth(1), Some("8.9"), "{line}");
    assert!(line.ends_with("90 TOP PLACED"), "{line}");
}

#[test]
fn ex3_place_keepout_written() {
    // The inserted Keep-out part is exported as a .PLACE_KEEPOUT whose loop is the corner
    // profile, 12.7 × 9.525 mm with R6.35: area 12.7·9.525 − (1 − π/4)·6.35² = 112.314 mm²,
    // height 0.125 in = 3.175 mm.
    let (emn, board, area) = ex3_exported();
    // The keep-out: one .PLACE_KEEPOUT whose loop is the corner profile.
    assert!(emn.contains(".PLACE_KEEPOUT"));
    assert_eq!(board.place_keepouts.len(), 1);
    let k = &board.place_keepouts[0];
    assert_eq!(k.loops.len(), 1);
    assert!((k.loops[0].area() - area).abs() < 1e-6, "{} vs {area}", k.loops[0].area());
    let bb = k.loops[0].bbox();
    assert!((bb.width() - 12.7).abs() < 1e-9 && (bb.height() - 9.525).abs() < 1e-9);
    let want = corner_profile();
    assert!((want.area() - area).abs() < 1e-9, "the hand-built profile has the same area");
    let wb = want.bbox();
    assert!((bb.min[0] - wb.min[0]).abs() < 1e-9 && (bb.min[1] - wb.min[1]).abs() < 1e-9, "at the board's bottom-left corner");
    let arcs = k.loops[0].points.iter().filter(|p| p.angle.abs() > 1e-9).count();
    assert_eq!(arcs, 1, "one 90° arc: {:?}", k.loops[0].points);
    assert!(k.height.is_some_and(|h| (h - 3.175).abs() < 1e-9), "{:?}", k.height);
}

#[test]
fn sync_prefers_the_link_over_part_names() {
    // A generated component's part renamed to anything still syncs as its placement.
    let (mut doc, mut h, g) = created("secondary board", "secondary board", CreateOptions::default());
    let comps = g.components_studio.unwrap();
    let build = cadrs_core::rebuild::build(&features(&doc, comps));
    for p in &build.parts {
        h.execute(&mut doc, &RenamePart { element: comps, part: p.id, name: format!("Renamed {}", p.id.index) }).unwrap();
    }
    let shown = doc.element(PCB).unwrap().pcb().unwrap().active;
    let p = plan(&doc, PCB, g.assembly, SyncPlaneChoice::Top, shown).unwrap();
    assert_eq!(p.components.len(), 20);
    assert!(p.unrecognised.is_empty(), "{:?}", p.unrecognised);
}

#[test]
fn generated_assembly_works_as_a_subassembly() {
    // PCB7.7–7.8: the generated assembly inserted into another assembly as a subassembly still
    // syncs (the links are found at depth), and Show assembly only makes it one BOM line.
    let (mut doc, mut h, g) = created("secondary board", "secondary board", CreateOptions::default());
    let top = ElementId::from_u128(0x3a06_9002);
    h.execute(&mut doc, &AddElement { id: top, kind: NewElementKind::Assembly, name: Some("Product".into()), after: None }).unwrap();
    let sub = InstanceId::new();
    h.execute(&mut doc, &InsertInstance { element: top, instance: Instance::new(sub, InstanceSource::Assembly { element: g.assembly }, Pose::translation([100.0, 0.0, 0.0])) }).unwrap();
    let shown = doc.element(PCB).unwrap().pcb().unwrap().active;
    let p = plan(&doc, PCB, top, SyncPlaneChoice::Top, shown).unwrap();
    assert_eq!(p.components.len(), 20, "found inside the subassembly");
    h.execute(
        &mut doc,
        &cadrs_core::properties::SetProperties {
            owners: vec![cadrs_core::properties::PropertyOwner::Assembly { element: g.assembly }],
            values: vec![(PropertyKey::BomBehavior, cadrs_core::properties::PropertyValue::BomBehavior(cadrs_core::properties::SubassemblyBom::AssemblyOnly))],
            label: "BOM behavior".into(),
        },
    )
    .unwrap();
    let b = compute(&doc, top, &BomOptions::default(), &Default::default(), |e| Some(cadrs_core::rebuild::build(&features(&doc, e)))).unwrap();
    assert_eq!(b.rows.len(), 1, "one line for the PCB");
    let name = b.columns.iter().position(|c| *c == BomColumn::Property(PropertyKey::Name)).unwrap();
    assert_eq!(b.rows[0].cells[name], "secondary board");
}
