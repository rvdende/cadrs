//! P3E.5: the test drive exercise (`test-drive.md` "Exercises", `test-drive-gaps.md` "What each
//! exercise needs") on its stand-in (`samples::drill`, `fixtures/drill_standin.cadrs`): steps
//! 2–7 and 9 in code, with the stand-in's fixture check.
//!
//! The expected volumes are worked out here from the stand-in's dimensions, written out again
//! (not read from the model or from the sample's constants): the MANIFOLD's mounting face is a
//! 40 × 30 rectangle less a Ø20 bore and two Ø5.5 holes,
//! A = 40·30 − π(10² + 2·2.75²) = 1200 − 115.125π = 838.3241 mm², so the gasket extruded from it
//! is 2·A = 1676.6483 mm³ at 2 mm and 1·A = 838.3241 mm³ at 1 mm.
#![cfg(feature = "occt")]

use std::f64::consts::PI;

use cadrs_core::assembly::bom::{BomColumn, BomOptions, compute_with};
use cadrs_core::command::Command;
use cadrs_core::drawing_assembly as da;
use cadrs_core::history_log::{HistoryLog, Origin, RestoreDocument, WorkspaceId};
use cadrs_core::properties::{GenerateMissingPartNumbers, PropertyKey, PropertyOwner, PropertyValue, SetProperties, text};
use cadrs_core::samples::drill as dr;
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::workspace_merge::{self as wm, MergeWorkspace, TabChange};
use cadrs_core::{Document, ElementId, History};
use cadrs_drawing::annotation::{DimTool, EdgeRef, ModelData, Pick, Shape, dimension_display, propose};
use cadrs_drawing::annotation::resolve as resolve_edge;
use cadrs_drawing::assembly::{BomOrder, BomType, Border, Callout, CalloutFields, SheetModel, bom_table, callout_texts, fit_within, snap_table_corner};
use cadrs_drawing::table::Corner;
use cadrs_drawing::title_block::{TitleContext, TitleField, resolve};
use cadrs_drawing::view::{Placement, projected_view};
use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Scale, View, template};

/// The mounting face, from the dimensions: 40·30 − π(10² + 2·2.75²) = 1200 − 115.125π.
fn area() -> f64 {
    let (l, w, bore_d, hole_d) = (40.0, 30.0, 20.0, 5.5);
    l * w - PI * ((bore_d / 2.0f64).powi(2) + 2.0 * (hole_d / 2.0f64).powi(2))
}

#[track_caller]
fn rel(what: &str, got: f64, want: f64) {
    assert!(((got - want) / want).abs() < 1e-6, "{what}: got {got}, want {want}");
}

// ---------------------------------------------------------------------------------------------
// The fixture

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// `fixtures/drill_standin.cadrs` is `samples::drill::document()`
/// (`CADRS_REGENERATE_FIXTURES=1` writes it).
#[test]
fn drill_fixture_is_current() {
    let doc = dr::document().unwrap();
    let file = dr::file(doc.clone());
    let path = fixtures().join("drill_standin.cadrs");
    if std::env::var_os("CADRS_REGENERATE_FIXTURES").is_some() {
        std::fs::write(&path, ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap()).unwrap();
    }
    let on_disk = cadrs_core::Store::load_path(&path).unwrap_or_else(|e| panic!("fixtures/drill_standin.cadrs: {e}"));
    assert_eq!(on_disk, file, "fixtures/drill_standin.cadrs is stale (CADRS_REGENERATE_FIXTURES=1)");
    // Four tabs; three parts (the gasket is the exercise's to make).
    let names: Vec<&str> = doc.elements.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["CARBURETOR", "CARBURETOR", "DRILL BODY", "FUEL AND POWER TRAIN"]);
    for (el, n) in [(dr::CARBURETOR_STUDIO, 2), (dr::DRILL_STUDIO, 1)] {
        let b = cadrs_core::rebuild::build(doc.element(el).unwrap().features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert_eq!(b.parts.len(), n);
    }
    // The manifold: 40·30·20 less the bore and the holes through it.
    let b = cadrs_core::rebuild::build(doc.element(dr::CARBURETOR_STUDIO).unwrap().features());
    let m = b.part(dr::MANIFOLD_PART).unwrap().mass.as_ref().unwrap().volume;
    rel("manifold", m, area() * 20.0);
}

// ---------------------------------------------------------------------------------------------
// The exercise

/// The document after steps 2–5 (the gasket, both inserts, the screws).
fn assembled() -> (Document, History) {
    let mut doc = dr::document().unwrap();
    let mut h = History::default();
    {
        let s = &mut DocHistory(&mut doc, &mut h);
        dr::add_gasket(s, 2.0).unwrap();
        dr::insert_gasket(s).unwrap();
        dr::insert_carburetor(s).unwrap();
    }
    dr::insert_screws(&mut doc, &mut h).unwrap();
    (doc, h)
}

fn gasket_volume(doc: &Document) -> f64 {
    let b = cadrs_core::rebuild::build(doc.element(dr::CARBURETOR_STUDIO).unwrap().features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let p = b.part(dr::GASKET_PART).expect("the gasket keeps its part id");
    p.mass.as_ref().map(|m| m.volume).expect("a kernel")
}

/// Step 2: the gasket from the mounting face, New, 2 mm, is its own part of 2·A.
#[test]
fn step2_the_gasket_is_the_mounting_face_2_mm_thick() {
    let (doc, _) = assembled();
    let b = cadrs_core::rebuild::build(doc.element(dr::CARBURETOR_STUDIO).unwrap().features());
    assert_eq!(b.parts.len(), 3, "MANIFOLD, CARBURETOR_BODY and the new gasket");
    let gasket = b.part(dr::GASKET_PART).unwrap();
    assert_eq!(doc.element(dr::CARBURETOR_STUDIO).unwrap().part_prop(dr::GASKET_PART).and_then(|p| p.name.clone()).as_deref(), Some("CARBURETOR_GASKET"));
    let v = gasket.mass.as_ref().unwrap().volume;
    rel("gasket", v, 2.0 * area());
    assert_eq!(format!("{v:.4}"), "1676.6483");
    // The manifold is unchanged (New, not Add).
    rel("manifold", b.part(dr::MANIFOLD_PART).unwrap().mass.as_ref().unwrap().volume, area() * 20.0);
}

/// One Session's document, undo and log, as the app keeps them (each committed command an
/// entry; a workspace switch a fresh undo).
struct Session {
    doc: Document,
    undo: History,
    log: HistoryLog,
    time: i64,
}

impl Session {
    fn commit(&mut self) {
        self.time += 10;
        let label = self.undo.undo_label().unwrap_or_default().to_string();
        let origin = match label.strip_prefix(wm::MERGE_LABEL) {
            Some(src) => Origin::Merge(src.to_string()),
            None => match label.strip_prefix("Restore to ") {
                Some(e) => Origin::Restore(e.to_string()),
                None => Origin::Command(label),
            },
        };
        self.log.record(&self.doc, origin, self.time, "me");
    }

    fn run(&mut self, c: &dyn Command) {
        self.undo.execute(&mut self.doc, c).unwrap();
        self.commit();
    }

    fn switch(&mut self, ws: WorkspaceId) {
        self.doc = self.log.switch_to(ws).unwrap();
        self.undo = History::default();
    }
}

/// Step 9: version "FUEL AND POWER TRAIN COMPLETE" → branch "Alternate Gasket Thickness" → the
/// gasket 1 mm there → merge into Main replacing the CARBURETOR Part Studio → Restore the entry
/// before the merge. The gasket reads 2·A in Main, 1·A in the branch (Main still 2·A), 1·A in
/// Main after the merge and 2·A again after the Restore.
#[test]
fn step9_gasket_volumes_in_each_workspace() {
    let (doc, _) = assembled();
    let log = HistoryLog::start(&doc, 1_000, "me");
    let mut s = Session { doc, undo: History::default(), log, time: 1_000 };
    let main_2mm = gasket_volume(&s.doc);
    rel("Main", main_2mm, 2.0 * area());
    assert_eq!(format!("{main_2mm:.4}"), "1676.6483");
    let v = s.log.create_version("FUEL AND POWER TRAIN COMPLETE", "", s.time, "me");
    let branch = s.log.branch(v, "Alternate Gasket Thickness", "", s.time + 5, "me").unwrap();
    s.switch(branch);
    let mut h = std::mem::take(&mut s.undo);
    dr::set_gasket_thickness(&mut DocHistory(&mut s.doc, &mut h), 1.0).unwrap();
    s.undo = h;
    s.commit();
    let in_branch = gasket_volume(&s.doc);
    rel("branch", in_branch, area());
    assert_eq!(format!("{in_branch:.4}"), "838.3241");
    // Main is untouched while the branch changes.
    let main_head = s.log.workspace_head(WorkspaceId::MAIN).unwrap();
    rel("Main during the branch", gasket_volume(&main_head), 2.0 * area());
    // Merge into Main: only the CARBURETOR Part Studio changed; replace it.
    let source = s.doc.clone();
    s.switch(WorkspaceId::MAIN);
    let base = s.log.merge_base(branch, WorkspaceId::MAIN);
    let changed = wm::changed_tabs(base.as_ref(), &source, &s.doc);
    assert_eq!(changed.iter().map(|c| (c.element, c.change)).collect::<Vec<_>>(), [(dr::CARBURETOR_STUDIO, TabChange::Changed)]);
    let before_merge = s.log.head_index();
    let merged = wm::merge(&s.doc, &source, &[dr::CARBURETOR_STUDIO]);
    s.run(&MergeWorkspace { state: Box::new(merged), source: s.log.workspace_name(branch) });
    assert_eq!(s.log.current_workspace(), WorkspaceId::MAIN);
    let after_merge = gasket_volume(&s.doc);
    rel("merged Main", after_merge, area());
    assert_eq!(format!("{after_merge:.4}"), "838.3241");
    // The assemblies kept their instances of the replaced studio's gasket.
    let asm = s.doc.element(dr::CARBURETOR).unwrap().assembly_model().unwrap();
    assert!(asm.instances.iter().any(|i| i.id == dr::GASKET_INSTANCE));
    // Restore the entry before the merge.
    let state = s.log.state_at(before_merge).unwrap();
    let label = s.log.entries[before_merge].label.clone();
    s.run(&RestoreDocument { state: Box::new(state), entry: label });
    let restored = gasket_volume(&s.doc);
    rel("restored Main", restored, 2.0 * area());
    assert_eq!(format!("{restored:.4}"), "1676.6483");
    // The branch still has its 1 mm gasket.
    rel("the branch after it all", gasket_volume(&s.log.workspace_head(branch).unwrap()), area());
}

/// The CARBURETOR assembly's BOM: its rows' Name and Part number cells.
fn bom_cells(doc: &Document) -> Vec<(String, String)> {
    let el = dr::CARBURETOR;
    let settings = doc.element(el).unwrap().assembly_model().unwrap().bom.clone();
    let mut build_of = |e: ElementId| Some(cadrs_core::rebuild::build(doc.element(e)?.features()));
    let bom = compute_with(doc, el, &settings, &BomOptions::default(), &doc.units, &mut build_of).unwrap();
    let col = |k| bom.columns.iter().position(|c| *c == BomColumn::Property(k)).unwrap();
    let (n, pn) = (col(PropertyKey::Name), col(PropertyKey::PartNumber));
    bom.rows.iter().map(|r| (r.cells[n].clone(), r.cells[pn].clone())).collect()
}

/// Step 6: the gasket's Part number generated from the BOM is the part's property, and a
/// number typed in the part's properties is the BOM's: one value, both ways.
#[test]
fn step6_part_number_syncs_both_ways() {
    let (mut doc, mut h) = assembled();
    let owner = PropertyOwner::Part { element: dr::CARBURETOR_STUDIO, part: dr::GASKET_PART };
    let gasket_row = |doc: &Document| bom_cells(doc).into_iter().find(|(n, _)| n == "CARBURETOR_GASKET").expect("the gasket's BOM row");
    assert_eq!(gasket_row(&doc).1, "", "no part number yet");
    // BOM → Properties: Generate next part number on the gasket's row.
    h.execute(&mut doc, &GenerateMissingPartNumbers { owners: vec![owner] }).unwrap();
    assert_eq!(text(&doc, owner, PropertyKey::PartNumber, None), dr::GASKET_PART_NUMBER, "the next free number of the scheme");
    assert_eq!(gasket_row(&doc).1, dr::GASKET_PART_NUMBER);
    // Properties → BOM: a number (and a description) typed in the part's Properties.
    let set = |k, v: &str| SetProperties { owners: vec![owner], values: vec![(k, PropertyValue::Text(v.into()))], label: "Properties".into() };
    h.execute(&mut doc, &set(PropertyKey::PartNumber, "PRT-000106")).unwrap();
    h.execute(&mut doc, &set(PropertyKey::Description, "Carburetor gasket, 2 mm")).unwrap();
    assert_eq!(gasket_row(&doc).1, "PRT-000106");
    assert_eq!(text(&doc, owner, PropertyKey::Description, None), "Carburetor gasket, 2 mm");
    // The other rows keep the fixture's numbers.
    let rows = bom_cells(&doc);
    assert!(rows.contains(&("MANIFOLD".into(), "PRT-000001".into())), "{rows:?}");
    assert!(rows.contains(&("CARBURETOR_BODY".into(), "PRT-000002".into())), "{rows:?}");
}

/// Step 7: a drawing of FUEL AND POWER TRAIN on ANSI_B_MM: Front 1:2 and its projected Right
/// view, a Structured – Top level BOM snapped with its bottom-right corner at the title block's
/// left edge; 3 rows (the drill body, CARBURETOR, the two M5 × 25 screws); the title block reads
/// the assembly's Name and Part number.
#[test]
fn step7_the_drawing() {
    let (doc, _) = assembled();
    let t = template::builtin("ANSI_B_MM.dwt").expect("the ANSI_B_MM template");
    let mut d = Drawing::from_template(&t, None);
    let src = da::live_source(&doc, dr::POWER_TRAIN).unwrap();
    d.sources.push(src);
    let sheet = d.sheets[0].id;
    let r = ObjectRef { element: dr::POWER_TRAIN.0, part: None };
    let front = View::base(r, NamedView::Front, Scale::new(1, 2), [120.0, 160.0]);
    d.apply(&DrawingOp::InsertView { sheet, view: front.clone() }).unwrap();
    let right = projected_view(&front, Placement::Ortho([1.0, 0.0]), d.projection, [220.0, 160.0], None);
    d.apply(&DrawingOp::InsertView { sheet, view: right.clone() }).unwrap();
    // Both views project (the right one is the side of the same assembly).
    for v in [&front, &right] {
        let g = cadrs_core::drawing_export::view_geometry(&doc, &d, v).unwrap();
        assert!(g.bounds.is_some());
    }
    // The BOM, its bottom-right corner snapped onto the title block's bottom-left.
    let s0 = &d.sheets[0];
    let frame = cadrs_drawing::standard::frame(s0.format).inner;
    let block = cadrs_drawing::title_block::placement(frame, s0.format.size);
    let at = snap_table_corner([block.min[0] - 2.0, block.min[1] + 1.5], Corner::BottomRight, (frame.min, frame.max), Some((block.min, block.max)), 8.0).unwrap();
    assert_eq!(at, block.min, "snapped to the title block's left edge, on the frame's bottom");
    let data = da::live_bom(&doc, dr::POWER_TRAIN, BomType::TopLevel, BomOrder::TopToBottom).unwrap();
    // Kept within the frame across (the six default columns are wider than the room left of
    // the block): its left edge moves in, the cells wrapping.
    let table = fit_within(&bom_table(data.clone(), Corner::BottomRight, at, &d.style), (frame.min, frame.max));
    assert!(table.rect().0[0] >= frame.min[0] - 1e-9, "the table stays within the frame: {:?}", table.rect());
    d.apply(&DrawingOp::AddTable { sheet, table: table.clone() }).unwrap();
    // 2 views, the 3-row top-level BOM.
    assert_eq!(d.sheets[0].views.len(), 2);
    assert_eq!(data.rows.len(), 3);
    let name = data.columns.iter().position(|c| c == "Name").unwrap();
    let qty = data.columns.iter().position(|c| c == "Quantity").unwrap();
    let rows: Vec<(String, String)> = data.rows.iter().map(|r| (r.cells[name].clone(), r.cells[qty].clone())).collect();
    assert_eq!(
        rows,
        [("DRILL_BODY".to_string(), "1".to_string()), ("CARBURETOR".into(), "1".into()), ("Socket head cap screw M5 x 25".into(), "2".into())],
        "the base part, the subassembly (top level only), the two screws"
    );
    assert_eq!(table.n_rows(), 4, "the header and the 3 rows");
    let (lo, _) = table.rect();
    assert!((lo[1] - block.min[1]).abs() < 1e-9, "the table stands on the frame's bottom");
    // The height (Front: the drill body's bottom, z −90, to the carburetor's top, z 2 + 20 + 36
    // = 58) and the depth (Right: the drill body's sides, y ±40), measured on the views' edges.
    let near = |a: f64, b: f64| (a - b).abs() < 1e-4;
    let edge = |v: &View, g: &cadrs_core::views::ViewGeometry, f: &dyn Fn(&Shape) -> bool| -> EdgeRef {
        g.projection.edges.iter().map(EdgeRef::of).find(|r| {
            let res = resolve_edge(v, g, r);
            f(&res.shape)
        }).expect("an edge there")
    };
    let hline = |at: f64, x: f64| move |s: &Shape| matches!(*s, Shape::Line { a, b } if near(a[1], at) && near(b[1], at) && a[0].min(b[0]) <= x && x <= a[0].max(b[0]));
    let vline = |at: f64, y: f64| move |s: &Shape| matches!(*s, Shape::Line { a, b } if near(a[0], at) && near(b[0], at) && a[1].min(b[1]) <= y && y <= a[1].max(b[1]));
    let gf = cadrs_core::drawing_export::view_geometry(&doc, &d, &front).unwrap();
    let gf: &cadrs_core::views::ViewGeometry = &gf;
    let gr = cadrs_core::drawing_export::view_geometry(&doc, &d, &right).unwrap();
    let gr: &cadrs_core::views::ViewGeometry = &gr;
    let height = propose(DimTool::Smart, &front, gf, &[Pick::Edge(edge(&front, gf, &hline(-90.0, 0.0))), Pick::Edge(edge(&front, gf, &hline(58.0, 0.0)))], [-100.0, 0.0]).expect("a dimension");
    let depth = propose(DimTool::Smart, &right, gr, &[Pick::Edge(edge(&right, gr, &vline(-40.0, -45.0))), Pick::Edge(edge(&right, gr, &vline(40.0, -45.0)))], [0.0, -110.0]).expect("a dimension");
    for (v, g, dim, want, text) in [(&front, gf, &height, 148.0, "148.00"), (&right, gr, &depth, 80.0, "80.00")] {
        let (block, m) = dimension_display(&d.style, v, g, dim).expect("it measures");
        assert!((m.value - want).abs() < 1e-6, "{text}: {}", m.value);
        assert_eq!(block.plain(), text);
    }
    // The Item No. callouts: the drill body 1, the carburetor (a part of it) 2, a screw 3.
    let info = d.source(dr::POWER_TRAIN.0).and_then(|s| s.assembly.clone()).expect("the assembly's info");
    let geo = ModelData::default();
    let tables = [table.clone()];
    let m = SheetModel { inner: &geo, assembly: Some(&info), tables: &tables };
    let callout = |occ: cadrs_core::assembly::InstanceId| Callout {
        occurrence: occ.0,
        attach: [0.0, 0.0],
        text: [50.0, 50.0],
        border: Border::Circle,
        size: 0,
        text_height: 3.5,
        fields: CalloutFields { center: "{Table: Item No.}".into(), ..Default::default() },
        last: None,
    };
    let manifold = cadrs_core::assembly::structure::derive(dr::CARBURETOR_INSTANCE, dr::MANIFOLD_INSTANCE);
    let items: Vec<String> = [dr::DRILL_INSTANCE, manifold, dr::SCREW_INSTANCES[1]].into_iter().map(|o| callout_texts(&m, &callout(o))[4].clone()).collect();
    assert_eq!(items, ["1", "2", "3"]);
    // The title block: the sheet's reference (its first view) is the assembly: its Name and
    // Part number.
    let props = cadrs_core::drawing_export::reference_props(&doc, Some(d.sheets[0].views[0].reference));
    let s0 = &d.sheets[0];
    let fields = resolve(&TitleContext { reference: &props, props: &d.title, size: s0.format.size, scale: s0.scale, projection: d.projection, sheet_index: 0, sheet_count: 1 });
    assert_eq!(fields[&TitleField::Title], "FUEL AND POWER TRAIN");
    assert_eq!(fields[&TitleField::Number], "PRT-000005");
    assert_eq!(fields[&TitleField::Size], "B");
}
