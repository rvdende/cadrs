//! Assembly drawings (P3C.5, D11, D12, X10): the Ex2 stand-in (`samples::ujoint_assembly`,
//! `fixtures/ujoint_assembly_standin.cadrs`), drawing BOM tables against the assembly's BOM,
//! callout fields, and the update model for assembly views.

use cadrs_core::assembly::bom::{BomColumn, BomOptions, BomView, SetBomSettings, compute_with};
use cadrs_core::drawing_assembly::{self as da, AssemblyState};
use cadrs_core::properties::PropertyKey;
use cadrs_core::samples::{pneumatic_ex2 as ex2, pneumatic_ex3 as ex3, ujoint_assembly as ua};
use cadrs_core::{Document, ElementId, History};
use cadrs_drawing::assembly::{
    AssemblyInfo, Border, BomOrder, BomType, Callout, CalloutFields, SheetModel, bom_table, callout_dangles, callout_texts,
};
use cadrs_drawing::table::Corner;
use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Scale, View, template};

fn ex2_doc() -> Document {
    ua::document().expect("the Ex2 stand-in builds")
}

/// The course's D12.2: Name added and moved next to Item.
fn with_name_column(doc: &mut Document) {
    let mut s = doc.element(ua::ASSEMBLY).unwrap().assembly_model().unwrap().bom.clone();
    s.columns.insert(1, BomColumn::Property(PropertyKey::Name));
    History::default().execute(doc, &SetBomSettings { element: ua::ASSEMBLY, settings: s, label: "Add column".into() }).unwrap();
}

#[test]
fn the_ex2_fixture_is_current() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test drawing_assembly`.
    let doc = ex2_doc();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ujoint_assembly_standin.cadrs");
    let text = ron::ser::to_string_pretty(&cadrs_core::samples::pneumatic::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/ujoint_assembly_standin.cadrs is out of date");
    // 2 flanges, 1 block, 4 bushes, 4 axles, 16 screws.
    let asm = doc.element(ua::ASSEMBLY).unwrap().assembly_model().unwrap();
    assert_eq!(asm.instances.len(), 27);
    assert_eq!(asm.mates.len(), 16, "each screw's Fastened mate");
}

/// D12: the drawing's BOM (Flattened, after D12.2) is the course's table exactly.
#[test]
fn the_ex2_bom_is_the_courses() {
    let mut doc = ex2_doc();
    with_name_column(&mut doc);
    let data = da::live_bom(&doc, ua::ASSEMBLY, BomType::Flattened, BomOrder::TopToBottom).unwrap();
    assert_eq!(data.columns, ["Item No.", "Name", "Quantity", "Part number", "Description"]);
    let rows: Vec<Vec<String>> = data.rows.iter().map(|r| r.cells.clone()).collect();
    let want: Vec<Vec<String>> = ua::COURSE_BOM
        .iter()
        .enumerate()
        .map(|(i, (n, q, pn, d))| vec![(i + 1).to_string(), n.to_string(), q.to_string(), pn.to_string(), d.to_string()])
        .collect();
    assert_eq!(rows, want);
    // Independent: the quantities are the instances of each source.
    assert_eq!(data.rows.iter().map(|r| r.quantity).collect::<Vec<_>>(), [2, 1, 4, 4, 16]);
    assert_eq!(data.rows.iter().map(|r| r.occurrences.len()).sum::<usize>(), 27);
}

/// D11.1: a drawing BOM's rows are the assembly's BOM rows (`compute_with`) for every BOM type
/// and both orders (on the Ex3 pneumatic cylinder, which has subassemblies).
#[test]
fn drawing_bom_rows_equal_the_assembly_bom() {
    let doc = ex3::document().unwrap();
    let el = ex2::ASSEMBLY;
    let settings = doc.element(el).unwrap().assembly_model().unwrap().bom.clone();
    let build_of = |e: ElementId| Some(cadrs_core::rebuild::build(doc.element(e)?.features()));
    for kind in BomType::ALL {
        let mut s = settings.clone();
        s.view = if kind == BomType::Flattened { BomView::Flattened } else { BomView::Structured };
        let opts = BomOptions { expand_all: kind == BomType::MultiLevel, ..Default::default() };
        let bom = compute_with(&doc, el, &s, &opts, &doc.units, &mut |e| build_of(e)).unwrap();
        for order in BomOrder::ALL {
            let data = da::live_bom(&doc, el, kind, order).unwrap();
            let got: Vec<&Vec<String>> = data.rows.iter().map(|r| &r.cells).collect();
            let want: Vec<&Vec<String>> = bom.rows.iter().map(|r| &r.cells).collect();
            assert_eq!(got, want, "{kind:?} {order:?}");
            // The table: header first (top to bottom) or last (bottom to top), rows reversed.
            let t = bom_table(data.clone(), Corner::TopRight, [260.0, 200.0], &cadrs_drawing::DrawingStyle::default());
            let cell = |r: usize, c: usize| t.cells[r][c].plain_text();
            let n = t.n_rows();
            assert_eq!(n, bom.rows.len() + 1);
            match order {
                BomOrder::TopToBottom => {
                    assert_eq!(cell(0, 0), "Item No.");
                    assert_eq!(cell(1, 0), bom.rows[0].cells[0]);
                }
                BomOrder::BottomToTop => {
                    assert_eq!(cell(n - 1, 0), "Item No.");
                    assert_eq!(cell(n - 2, 0), bom.rows[0].cells[0], "item 1 just above the header");
                    assert_eq!(cell(0, 0), bom.rows.last().unwrap().cells[0]);
                }
            }
        }
    }
    // The three types differ on this model (subassemblies).
    let flat = da::live_bom(&doc, el, BomType::Flattened, BomOrder::TopToBottom).unwrap();
    let top = da::live_bom(&doc, el, BomType::TopLevel, BomOrder::TopToBottom).unwrap();
    let multi = da::live_bom(&doc, el, BomType::MultiLevel, BomOrder::TopToBottom).unwrap();
    assert!(top.rows.len() < multi.rows.len());
    assert!(multi.rows.iter().any(|r| r.item.contains('.')));
    assert!(!flat.rows.iter().any(|r| r.item.contains('.')));
}

fn callout(occ: cadrs_core::assembly::InstanceId) -> Callout {
    Callout {
        occurrence: occ.0,
        attach: [0.0, 0.0],
        text: [60.0, 60.0],
        border: Border::Underline,
        size: 0,
        text_height: 3.048,
        fields: CalloutFields { center: "{Part: Name}".into(), right: "x {Table: Qty.}".into(), ..Default::default() },
        last: None,
    }
}

/// D11.4, D12.10: callout fields read the part's properties and the BOM table's row; a callout
/// dangles when its instance is deleted and the drawing updated.
#[test]
fn callouts_read_names_and_quantities_and_dangle() {
    let mut doc = ex2_doc();
    with_name_column(&mut doc);
    let src = da::live_source(&doc, ua::ASSEMBLY).unwrap();
    let info: &AssemblyInfo = src.assembly.as_ref().unwrap();
    let data = da::live_bom(&doc, ua::ASSEMBLY, BomType::Flattened, BomOrder::TopToBottom).unwrap();
    let table = bom_table(data, Corner::TopRight, [260.0, 200.0], &cadrs_drawing::DrawingStyle::default());
    let tables = [table];
    let geo = cadrs_drawing::annotation::ModelData::default();
    let m = SheetModel { inner: &geo, assembly: Some(info), tables: &tables };
    let want = [
        (ua::FLANGES[1], "Universal Joint Flange x 2"),
        (ua::CENTRE_BLOCK, "Universal Joint Centre Block x 1"),
        (ua::BUSHES[2], "Graphite Phosphor Bronze Bushes x 4"),
        (ua::AXLES[0], "Universal Joint Axle x 4"),
        (ua::screw(7), "Pan head machine screw 1/4-28 x 0.75 x 16"),
    ];
    for (occ, text) in want {
        let t = callout_texts(&m, &callout(occ));
        assert_eq!(format!("{} {}", t[4], t[3]), text);
    }
    // Item No. reads the row's number.
    let mut c = callout(ua::AXLES[0]);
    c.fields = CalloutFields { center: "{Table: Item No.}".into(), ..Default::default() };
    assert_eq!(callout_texts(&m, &c)[4], "4");
    // Delete the centre block from the assembly; after the update its callout dangles, red,
    // with its last text.
    let mut d = Drawing::from_template(&template::builtin("ANSI_A_INCH.dwt").unwrap(), None);
    let sheet = d.sheets[0].id;
    let mut v = View::base(ObjectRef { element: ua::ASSEMBLY.0, part: None }, NamedView::Isometric, Scale::new(1, 2), [100.0, 110.0]);
    v.source_hash = src.hash_of(None);
    let a = cadrs_drawing::annotation::Annotation::new(cadrs_drawing::annotation::AnnotationKind::Callout(callout(ua::CENTRE_BLOCK)));
    v.annotations.push(a.clone());
    let vid = v.id;
    d.apply(&DrawingOp::Batch { ops: vec![DrawingOp::SetSource(src.clone()), DrawingOp::InsertView { sheet, view: v }], label: "Insert".into() }).unwrap();
    d.apply(&DrawingOp::AddTable { sheet, table: tables[0].clone() }).unwrap();
    History::default()
        .execute(&mut doc, &cadrs_core::assembly::commands::DeleteInstances { element: ua::ASSEMBLY, instances: vec![ua::CENTRE_BLOCK] })
        .unwrap();
    assert_eq!(cadrs_core::drawing_source::out_of_date(&doc, &d), vec![vid], "the assembly changed: its view is out of date");
    let op = cadrs_core::drawing_source::update_now(&doc, &d).expect("an update");
    d.apply(&op).unwrap();
    assert!(cadrs_core::drawing_source::out_of_date(&doc, &d).is_empty());
    let v = d.view(vid).unwrap().1.clone();
    let cadrs_drawing::annotation::AnnotationKind::Callout(c) = &v.annotations[0].kind else { panic!() };
    let new_src = d.source(ua::ASSEMBLY.0).unwrap();
    let m2 = SheetModel::new(&d, &d.sheets[0], &v, &geo);
    assert!(new_src.assembly.as_ref().unwrap().occurrence(&ua::CENTRE_BLOCK.0).is_none());
    assert!(callout_dangles(&m2, c));
    let t = callout_texts(&m2, c);
    assert_eq!(format!("{} {}", t[4], t[3]), "Universal Joint Centre Block x 1", "its last text");
    // The BOM table was updated in the same step: 4 rows now.
    let bom = d.sheets[0].tables[0].bom.as_ref().unwrap();
    assert_eq!(bom.rows.len(), 4);
}

/// D13.2 for assemblies: a change to the assembly marks its views out of date; a change to an
/// assembly the drawing doesn't reference doesn't.
#[test]
fn assembly_changes_mark_only_its_views() {
    let mut doc = ex2_doc();
    let src = da::live_source(&doc, ua::ASSEMBLY).unwrap();
    let mut d = Drawing::from_template(&template::builtin("ANSI_A_INCH.dwt").unwrap(), None);
    let sheet = d.sheets[0].id;
    let mut v = View::base(ObjectRef { element: ua::ASSEMBLY.0, part: None }, NamedView::Isometric, Scale::new(1, 2), [100.0, 110.0]);
    v.source_hash = src.hash_of(None);
    let vid = v.id;
    d.apply(&DrawingOp::Batch { ops: vec![DrawingOp::SetSource(src), DrawingOp::InsertView { sheet, view: v }], label: "Insert".into() }).unwrap();
    assert!(cadrs_core::drawing_source::out_of_date(&doc, &d).is_empty(), "up to date at first");
    // Another assembly, changed: nothing.
    let other = ElementId::from_u128(0x0a1f_2a00_0000_0000_0000_0000_0000_0999);
    let mut h = History::default();
    h.execute(&mut doc, &cadrs_core::commands::AddElement { id: other, kind: cadrs_core::commands::NewElementKind::Assembly, name: Some("Other".into()), after: None }).unwrap();
    let inst = cadrs_core::assembly::Instance::new(
        cadrs_core::assembly::InstanceId::from_u128(77),
        cadrs_core::assembly::InstanceSource::Part { element: ua::COMPONENTS, part: ua::BLOCK },
        cadrs_core::assembly::Pose::IDENTITY,
    );
    h.execute(&mut doc, &cadrs_core::assembly::commands::InsertInstance { element: other, instance: inst }).unwrap();
    assert!(cadrs_core::drawing_source::out_of_date(&doc, &d).is_empty(), "an unreferenced assembly changed");
    // A property of one of its parts: out of date (the callouts and BOM read it).
    h.execute(
        &mut doc,
        &cadrs_core::properties::SetProperties {
            owners: vec![cadrs_core::properties::PropertyOwner::Part { element: ua::COMPONENTS, part: ua::AXLE }],
            values: vec![(PropertyKey::PartNumber, cadrs_core::properties::PropertyValue::Text("MSB-0003B".into()))],
            label: "Part number".into(),
        },
    )
    .unwrap();
    assert_eq!(cadrs_core::drawing_source::out_of_date(&doc, &d), vec![vid]);
    // An instance moved: out of date too.
    h.undo(&mut doc).unwrap();
    assert!(cadrs_core::drawing_source::out_of_date(&doc, &d).is_empty());
    h.execute(
        &mut doc,
        &cadrs_core::assembly::commands::MoveInstances {
            element: ua::ASSEMBLY,
            poses: vec![(ua::CENTRE_BLOCK, cadrs_core::assembly::Pose::translation([0.0, 0.0, 99.0]))],
            label: "Move".into(),
        },
    )
    .unwrap();
    assert_eq!(cadrs_core::drawing_source::out_of_date(&doc, &d), vec![vid]);
}

/// The assembly view projects every shown occurrence, each edge tagged with its occurrence.
#[test]
fn assembly_views_project_every_occurrence() {
    let doc = ex2_doc();
    let state = AssemblyState::of(&doc, ua::ASSEMBLY).unwrap();
    assert_eq!(state.occurrences.len(), 27);
    assert_eq!(state.studios.len(), 3, "the flange, the components and the screw's studio");
    let mut v = View::base(ObjectRef { element: ua::ASSEMBLY.0, part: None }, NamedView::Isometric, Scale::new(1, 2), [100.0, 110.0]);
    v.shaded = true;
    let g = da::project(&state, &v).expect("it projects");
    assert_eq!(g.parts.len(), 27);
    assert!(!g.shaded.is_empty());
    assert!(!g.projection.edges.is_empty(), "hidden-line removal ran");
    {
        let mut seen: Vec<cadrs_core::assembly::InstanceId> = (0..g.projection.edges.len()).filter_map(|i| da::edge_occurrence(&g, i)).collect();
        seen.sort();
        seen.dedup();
        assert!(seen.contains(&ua::CENTRE_BLOCK) && seen.contains(&ua::FLANGES[1]), "{} occurrences have edges", seen.len());
    }
}

/// X3, P3C.5: the P3C.3 `description` field is read into the Description property, which the
/// title block and notes read (through `reference_props`); undefined ones stay dashes.
#[test]
fn old_descriptions_migrate_to_the_property_model() {
    use cadrs_core::PartProps;
    let id = cadrs_core::PartId::new(cadrs_core::FeatureId::from_u128(5), 0);
    let old = format!("(part: {}, name: Some(\"Flange\"), hidden: false, description: Some(\"Made by cadrs\"))", ron::to_string(&id).unwrap());
    let p: PartProps = ron::from_str(&old).expect("an old file reads");
    assert_eq!(p.properties.description.as_deref(), Some("Made by cadrs"));
    let text = ron::to_string(&p).unwrap();
    assert!(!text.contains("description: Some(\"Made by cadrs\"), properties") && text.contains("properties"), "{text}");
    let back: PartProps = ron::from_str(&text).unwrap();
    assert_eq!(back, p, "it round-trips in the new form");
    // The Ex1 stand-in's title block: name and description from the properties.
    let doc = cadrs_core::samples::ujoint::document().unwrap();
    let el = doc.elements[0].id;
    let r = cadrs_drawing::ObjectRef { element: el.0, part: Some((cadrs_core::samples::ujoint::PART.feature.0, 0)) };
    let props = cadrs_core::drawing_export::reference_props(&doc, Some(r));
    assert_eq!(props.name.as_deref(), Some(cadrs_core::samples::ujoint::PART_NAME));
    assert_eq!(props.description.as_deref(), Some("Made by cadrs"));
    assert_eq!(props.part_number, None, "undefined: dashes");
    // The Ex2 flange: its Part number from the properties.
    let doc = ex2_doc();
    let r = cadrs_drawing::ObjectRef { element: ua::FLANGE_STUDIO.0, part: Some((cadrs_core::samples::ujoint::PART.feature.0, 0)) };
    let props = cadrs_core::drawing_export::reference_props(&doc, Some(r));
    assert_eq!(props.part_number.as_deref(), Some("MSB-0004"));
    assert_eq!(props.description.as_deref(), Some("Universal joint flange"));
    // An assembly reference reads the tab's name.
    let r = cadrs_drawing::ObjectRef { element: ua::ASSEMBLY.0, part: None };
    assert_eq!(cadrs_core::drawing_export::reference_props(&doc, Some(r)).name.as_deref(), Some(ua::ASSEMBLY_NAME));
}

/// Prints where each BOM item's parts are on the Ex2 sheet (for writing the scenario).
#[test]
#[ignore]
fn print_ex2_sheet_points() {
    let doc = ex2_doc();
    let state = AssemblyState::of(&doc, ua::ASSEMBLY).unwrap();
    let anchor = [78.0, 96.0];
    let v = View::base(ObjectRef { element: ua::ASSEMBLY.0, part: None }, NamedView::Isometric, Scale::new(1, 2), anchor);
    let g = da::project(&state, &v).unwrap();
    let (lo, hi) = g.bounds.unwrap();
    println!("sheet bounds {:?} {:?}", v.to_sheet(lo), v.to_sheet(hi));
    let groups = [vec![ua::FLANGES[0], ua::FLANGES[1]], vec![ua::CENTRE_BLOCK], ua::BUSHES.to_vec(), ua::AXLES.to_vec(), (0..16).map(ua::screw).collect()];
    for (k, occs) in groups.iter().enumerate() {
        for occ in occs {
            let pts: Vec<[f64; 2]> = g
                .projection
                .edges
                .iter()
                .enumerate()
                .filter(|(j, e)| e.visibility == cadrs_kernel::ProjVisibility::Visible && da::edge_occurrence(&g, *j) == Some(*occ))
                .flat_map(|(_, e)| e.points.iter().map(|p| v.to_sheet([p.x, p.y])))
                .collect();
            if pts.is_empty() {
                continue;
            }
            let n = pts.len() as f64;
            let m = [pts.iter().map(|p| p[0]).sum::<f64>() / n, pts.iter().map(|p| p[1]).sum::<f64>() / n];
            let best = pts.iter().min_by(|a, b| ((a[0] - m[0]).hypot(a[1] - m[1])).total_cmp(&(b[0] - m[0]).hypot(b[1] - m[1]))).unwrap();
            let right = pts.iter().max_by(|a, b| a[0].total_cmp(&b[0])).unwrap();
            println!("item {} occ {:?}: middle {:?} nearest-edge {:?} rightmost {:?}", k + 1, occ.0, m, best, right);
        }
    }
}

/// Prints the Ex2 BOM table's column lines after the left grip is dragged to x 117 (or
/// `EX2_LEFT`).
#[test]
#[ignore]
fn print_ex2_bom_columns() {
    let mut doc = ex2_doc();
    with_name_column(&mut doc);
    let data = da::live_bom(&doc, ua::ASSEMBLY, BomType::Flattened, BomOrder::TopToBottom).unwrap();
    let style = template::builtin("ANSI_A_INCH.dwt").unwrap().style();
    let t = bom_table(data, Corner::TopRight, [266.7, 203.2], &style);
    println!("cols {:?} left {}", t.cols, t.rect().0[0]);
    let x: f64 = std::env::var("EX2_LEFT").ok().and_then(|v| v.parse().ok()).unwrap_or(117.0);
    let r = t.resize(cadrs_drawing::table::Side::Left, [x, 170.0]).unwrap();
    let mut x = r.rect().0[0];
    let mut lines = Vec::new();
    for w in &r.cols {
        x += w;
        lines.push(x);
    }
    println!("resized cols {:?} lines {:?}", r.cols, lines);
}
