//! P3C.6 / D13, D14: the Hand Brake stand-in (`cadrs_core::samples::hand_brake`,
//! `fixtures/hand_brake_standin.cadrs`) and updating its drawing from the workspace.
//!
//! **The fixture.** Onshape's public "Exercise: Hand Brake Update" can't be copied, so the Handle
//! Part Studio is rebuilt from the course's pictures in its state *before* the exercise's edits:
//! the fully defined Main Sketch of `ex3-step4.png` with its original values (225, 100, a 20 mm
//! bar, a 15 mm left offset, the Ø8.25 end hole and the small circle next to the Ø25.4 hole),
//! Extrude 1 (8 mm), an R30 corner fillet (our assumption for the Handle sheet's R30.00), the
//! Ø25 grip bar (Blind 10 with a 160 second end), its slot, and Hole 1 (ISO clearance
//! counterbore M5) on three points. The finished drawing has the sheets Assembly (a placeholder
//! until assemblies land), Handle and Grip. `CADRS_WRITE_FIXTURES=1` regenerates the file.
//!
//! **Independent values** after the course's edits and Update (D14.9–D14.11): 78.00 = 25 + 3 + 50
//! (the new bar height, the large hole's drop below the bar and the end hole's drop below it),
//! 46.00 = the sketch's horizontal distance between the large and the end hole, 185.00 = 175 + 10
//! (the grip's two extrude ends), 62.50 = 125 / 2, 8.50 = (25 − 8) / 2, and the callout's
//! 6.60 / 11.25 / 6.00 from ISO 273 (medium series, M6: 6.6) and the ISO 4762 M6 socket head
//! (dk 10 + 1.25, k 6).
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::commands::EditDrawing;
use cadrs_core::drawing_source::{self as ds, StudioState};
use cadrs_core::rebuild;
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::hand_brake::{self as hb, drawing as hd};
use cadrs_core::views::ViewGeometry;
use cadrs_core::{Document, History};
use cadrs_drawing::annotation::{
    AnnotationId, AnnotationKind, DimKind, Pick, PointOf, PointRef, Shape, annotation_graphics, hole_of, hole_text,
    is_dangling, resolve,
};
use cadrs_drawing::{Drawing, DrawingOp, View, ViewId};

fn drawing(doc: &Document) -> &Drawing {
    doc.element(hb::DRAWING).and_then(|e| e.drawing_data()).expect("the drawing")
}

/// View `id` projected from the drawing's snapshot (what it shows).
fn shown(doc: &Document, id: ViewId) -> (View, Arc<ViewGeometry>) {
    let d = drawing(doc);
    let (_, v) = d.view(id).unwrap();
    let st = StudioState::parse(&d.source(v.reference.element).unwrap().snapshot).unwrap();
    let g = cadrs_core::views::project(&st.features, ds::view_request(&st, v)).expect("the view projects");
    (v.clone(), g)
}

/// Each annotation of view `id`: its text (a dimension's, or a callout's lines joined) and
/// whether it dangles.
fn texts(doc: &Document, id: ViewId) -> Vec<(AnnotationId, String, bool)> {
    let d = drawing(doc);
    let (v, g) = shown(doc, id);
    v.annotations
        .iter()
        .map(|a| {
            let dangling = is_dangling(&v, &*g, a);
            let gr = annotation_graphics(&d.style, &v, &*g, a).expect("it draws");
            assert_eq!(gr.dangling, dangling);
            let text = match &a.kind {
                AnnotationKind::HoleCallout(hc) => {
                    let info = hole_of(&*g, &hc.edge).or(hc.last.as_ref()).unwrap();
                    hole_text(&d.style, info, &hc.prefix).plain()
                }
                _ => gr.texts.iter().map(|t| t.text.clone()).collect::<Vec<_>>().join(" "),
            };
            (a.id, text, dangling)
        })
        .collect()
}

fn text_of(t: &[(AnnotationId, String, bool)], id: AnnotationId) -> (String, bool) {
    t.iter().find(|x| x.0 == id).map(|x| (x.1.clone(), x.2)).unwrap_or_else(|| panic!("no annotation {id:?}"))
}

fn two(v: f64) -> String {
    format!("{v:.2}")
}

#[test]
fn the_handle_builds() {
    let doc = hb::document().expect("the stand-in builds");
    let el = doc.element(hb::STUDIO).unwrap();
    let sk = el.feature(hb::MAIN_SKETCH).unwrap().sketch().unwrap();
    let a = cadrs_sketch::solve::analyze(&sk.geometry);
    assert!(a.fully_constrained(), "Main Sketch dof {} conflicts {:?} {:?}", a.dof, a.conflicting, a.conflicting_dimensions);
    let b = rebuild::build(el.features());
    assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
    assert_eq!(b.parts.len(), 2);
    let plate = b.part(hb::PLATE).unwrap();
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for q in &plate.solid.positions {
        for i in 0..3 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    }
    // 8 thick (−Y), from the origin to the R16 arc round the end hole.
    // The end hole's centre before the edits: 225 + 46 across, 20 + 3 + 50 down.
    let c2 = (225.0 + 46.0, -(20.0 + 3.0 + 50.0));
    assert!((lo[1] + 8.0).abs() < 1e-6 && hi[1].abs() < 1e-6, "{lo:?} {hi:?}");
    assert!((hi[0] - (c2.0 + 16.0)).abs() < 0.05 && (lo[2] - (c2.1 - 16.0)).abs() < 0.05, "{lo:?} {hi:?}");
}

#[test]
fn the_fixture_is_current() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test hand_brake`.
    let doc = hb::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hand_brake_standin.cadrs");
    let file = cadrs_core::samples::gear_cover::file(doc.clone());
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/hand_brake_standin.cadrs is out of date");
}

/// The Handle sheet's views and the Grip sheet's.
const HANDLE_VIEWS: [ViewId; 3] = [hd::HANDLE_FRONT, hd::HANDLE_TOP, hd::HANDLE_ISO];
const GRIP_VIEWS: [ViewId; 3] = [hd::GRIP_FRONT, hd::GRIP_TOP, hd::GRIP_ISO];
/// Every view: the two sheets' and the Assembly sheet's (P3C.5).
const ALL_VIEWS: [ViewId; 7] = [hd::HANDLE_FRONT, hd::HANDLE_TOP, hd::HANDLE_ISO, hd::GRIP_FRONT, hd::GRIP_TOP, hd::GRIP_ISO, hd::ASSEMBLY_ISO];

fn sorted(mut v: Vec<ViewId>) -> Vec<ViewId> {
    v.sort();
    v
}

#[test]
fn the_dependency_tracker_marks_exactly_the_affected_views() {
    let mut doc = hb::document().unwrap();
    let mut h = History::default();
    assert!(ds::out_of_date(&doc, drawing(&doc)).is_empty(), "the finished drawing is up to date");
    // A change in another Part Studio: nothing is out of date.
    let other = cadrs_core::Element::part_studio("Enclosures");
    let other_id = other.id;
    doc.elements.push(other);
    cadrs_core::samples::drawing_bracket::build_in(&mut DocHistory(&mut doc, &mut h), other_id).unwrap();
    assert!(ds::out_of_date(&doc, drawing(&doc)).is_empty(), "an unreferenced studio doesn't matter");
    // Hole 1 changes only the grip: the Grip sheet's views, not the plate's.
    let mut spec_doc = doc.clone();
    let mut hole = spec_doc.element(hb::STUDIO).unwrap().feature(hb::HOLE_1).unwrap().hole().unwrap().clone();
    hole.spec.size = "M6".into();
    hole.spec.apply_table();
    h.execute(
        &mut spec_doc,
        &cadrs_core::commands::SetFeature { element: hb::STUDIO, feature: hb::HOLE_1, kind: cadrs_core::FeatureKind::Hole(hole), label: "Size".into() },
    )
    .unwrap();
    // P3C.5: the assembly shows the grip too.
    assert_eq!(sorted(ds::out_of_date(&spec_doc, drawing(&spec_doc))), sorted([GRIP_VIEWS.to_vec(), vec![hd::ASSEMBLY_ISO]].concat()));
    // The Main Sketch changes only the plate: the Handle sheet's views.
    let el = spec_doc.element(hb::STUDIO).unwrap();
    let g = &el.feature(hb::MAIN_SKETCH).unwrap().sketch().unwrap().geometry;
    let (id, _) = g.dimensions.iter().find(|(_, d)| d.value == 225.0).unwrap();
    h.execute(
        &mut doc,
        &cadrs_core::commands::EditSketch {
            element: hb::STUDIO,
            feature: hb::MAIN_SKETCH,
            op: cadrs_sketch::SketchOp::SetDimensionValue { id, value: 250.0 },
        },
    )
    .unwrap();
    assert_eq!(sorted(ds::out_of_date(&doc, drawing(&doc))), sorted([HANDLE_VIEWS.to_vec(), vec![hd::ASSEMBLY_ISO]].concat()));
}

#[test]
fn before_update_the_views_show_the_old_model() {
    let mut doc = hb::document().unwrap();
    let before: Vec<Arc<ViewGeometry>> = [HANDLE_VIEWS, GRIP_VIEWS].concat().iter().map(|id| shown(&doc, *id).1).collect();
    let mut h = History::default();
    hb::course_edits(&mut DocHistory(&mut doc, &mut h), hb::STUDIO).unwrap();
    assert_eq!(sorted(ds::out_of_date(&doc, drawing(&doc))), sorted(ALL_VIEWS.to_vec()));
    let after: Vec<Arc<ViewGeometry>> = [HANDLE_VIEWS, GRIP_VIEWS].concat().iter().map(|id| shown(&doc, *id).1).collect();
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(b.projection, a.projection, "a view changed before Update");
    }
    // Nothing dangles yet: the annotations still read the old values.
    let t = texts(&doc, hd::HANDLE_FRONT);
    assert_eq!(text_of(&t, hd::DIM_LENGTH), ("225.00".into(), false));
    assert_eq!(text_of(&t, hd::DIM_75), ("75.00".into(), false));
}

#[test]
fn ex3_values_after_update() {
    let mut doc = hb::document().unwrap();
    let mut h = History::default();
    // Before the edits (the fixture's own values).
    let t = texts(&doc, hd::HANDLE_FRONT);
    assert_eq!(text_of(&t, hd::DIM_LENGTH).0, "225.00");
    assert_eq!(text_of(&t, hd::DIM_HEIGHT).0, two(20.0 + 3.0 + 50.0));
    assert_eq!(text_of(&t, hd::DIM_SMALL).0, "2x Ø8.25");
    let t = texts(&doc, hd::GRIP_FRONT);
    assert_eq!(text_of(&t, hd::CALLOUT_GRIP).0, "3x Ø5.50 THRU ⌴Ø9.75 ↧5.00");
    assert_eq!(text_of(&t, hd::DIM_GRIP_LENGTH).0, two(160.0 + 10.0));

    hb::course_edits(&mut DocHistory(&mut doc, &mut h), hb::STUDIO).unwrap();
    let op = ds::update_now(&doc, drawing(&doc)).expect("views are out of date");
    assert_eq!(op.label(), "Update from this workspace");
    h.execute(&mut doc, &EditDrawing { element: hb::DRAWING, op }).unwrap();
    assert!(ds::out_of_date(&doc, drawing(&doc)).is_empty(), "one update brings every view up to date");

    // D14.9: the Handle sheet.
    let t = texts(&doc, hd::HANDLE_FRONT);
    let expect = [
        (hd::DIM_LENGTH, two(250.0)),
        (hd::DIM_BAR, two(25.0)),
        (hd::DIM_FILLET, format!("R{}", two(30.0))),
        (hd::DIM_LARGE, format!("Ø{}", two(25.4))),
        // 78.00 = the bar (25) + the large hole's drop (3) + the end hole's drop (50).
        (hd::DIM_HEIGHT, two(25.0 + 3.0 + 50.0)),
        (hd::DIM_END_R, format!("R{}", two(16.0))),
        (hd::DIM_HOLES, format!("3x Ø{}", two(5.5))),
    ];
    for (id, text) in &expect {
        assert_eq!(text_of(&t, *id), (text.clone(), false), "{id:?}");
    }
    // The small circle is gone: its diameter, the 75.00 and its centermark dangle (red) and keep
    // their last values.
    assert_eq!(text_of(&t, hd::DIM_SMALL), ("2x Ø8.25".into(), true));
    assert_eq!(text_of(&t, hd::DIM_75), ("75.00".into(), true));
    // Only its dead end is frozen: the live end is on the Ø15 hole's new centre (250 + 46); the
    // dead end stays where the small circle was (225 − 29 = 196), the same point as the red
    // centermark, the red 2x Ø8.25 and (in the Top view) the red centerline.
    {
        let (v, g) = shown(&doc, hd::HANDLE_FRONT);
        let find = |id| v.annotations.iter().find(|a| a.id == id).unwrap();
        let AnnotationKind::Dimension(d) = &find(hd::DIM_75).kind else { unreachable!() };
        let DimKind::Distance { a: Pick::Point(pa), b: Pick::Point(pb), .. } = &d.kind else { unreachable!() };
        let live = cadrs_drawing::annotation::resolve_point(&v, &*g, pb).unwrap().0;
        let dead = cadrs_drawing::annotation::resolve_point(&v, &*g, pa).unwrap().0;
        assert!((live[0] - 296.0).abs() < 1e-6, "{live:?}");
        assert!((dead[0] - 196.0).abs() < 1e-6 && (dead[1] + 23.0).abs() < 1e-6, "{dead:?}");
        let AnnotationKind::Centermark(cm) = &find(hd::CENTERMARK_SMALL).kind else { unreachable!() };
        assert_eq!(resolve(&v, &*g, cm).shape.circle().unwrap().0, dead);
        let AnnotationKind::Dimension(sd) = &find(hd::DIM_SMALL).kind else { unreachable!() };
        let DimKind::Diameter(se) = &sd.kind else { unreachable!() };
        assert_eq!(resolve(&v, &*g, se).shape.circle().unwrap().0, dead);
        let (tv, tg) = shown(&doc, hd::HANDLE_TOP);
        let cl = tv.annotations.iter().find(|a| a.id == hd::CENTERLINE_SMALL).unwrap();
        let AnnotationKind::Centerline(c) = &cl.kind else { unreachable!() };
        let (e0, e1) = cadrs_drawing::annotation::centerline_ends(&tv, &*tg, c).unwrap();
        assert!((e0[0] - 196.0).abs() < 1e-6 && (e1[0] - 196.0).abs() < 1e-6, "{e0:?} {e1:?}");
    }
    assert!(text_of(&t, hd::CENTERMARK_SMALL).1);
    for id in hd::CENTERMARKS {
        assert!(!text_of(&t, id).1, "{id:?} still resolves");
    }
    let t = texts(&doc, hd::HANDLE_TOP);
    assert_eq!(text_of(&t, hd::DIM_THICKNESS), (two(8.0), false));
    assert!(text_of(&t, hd::CENTERLINE_SMALL).1, "the small circle's centerline dangles");
    assert!(!text_of(&t, hd::CENTERLINE_LARGE).1);

    // D14.10: the 75.00's end grip dragged onto the large hole's centre: 46.00, black.
    let (v, g) = shown(&doc, hd::HANDLE_FRONT);
    // The large hole after the edits: 250 from the left edge.
    let c1x = 250.0;
    let large = g
        .projection
        .edges
        .iter()
        .map(cadrs_drawing::annotation::EdgeRef::of)
        .find(|r| matches!(resolve(&v, &*g, r).shape, Shape::Circle { center, radius, arc: None } if (radius - 12.7).abs() < 1e-6 && (center[0] - c1x).abs() < 1e-6))
        .expect("the large hole");
    let mut a = v.annotations.iter().find(|a| a.id == hd::DIM_75).unwrap().clone();
    if let AnnotationKind::Dimension(d) = &mut a.kind
        && let DimKind::Distance { a: end, .. } = &mut d.kind
    {
        *end = Pick::Point(PointRef { edge: large, of: PointOf::Center, hint: [0.0, 0.0] });
    }
    h.execute(
        &mut doc,
        &EditDrawing { element: hb::DRAWING, op: DrawingOp::SetAnnotation { view: hd::HANDLE_FRONT, annotation: a, label: "Re-attach dimension".into() } },
    )
    .unwrap();
    // 46.00 is the sketch's horizontal distance from the large hole to the end hole.
    assert_eq!(text_of(&texts(&doc, hd::HANDLE_FRONT), hd::DIM_75), (two(46.0), false));
    h.undo(&mut doc);
    assert_eq!(text_of(&texts(&doc, hd::HANDLE_FRONT), hd::DIM_75), ("75.00".into(), true), "undo: dangling again");
    h.redo(&mut doc);
    assert_eq!(text_of(&texts(&doc, hd::HANDLE_FRONT), hd::DIM_75), (two(46.0), false));
    // Delete the dangling ones: no red remains.
    h.execute(
        &mut doc,
        &EditDrawing {
            element: hb::DRAWING,
            op: DrawingOp::DeleteAnnotations {
                ids: vec![
                    (hd::HANDLE_TOP, hd::CENTERLINE_SMALL),
                    (hd::HANDLE_FRONT, hd::DIM_SMALL),
                    (hd::HANDLE_FRONT, hd::CENTERMARK_SMALL),
                ],
            },
        },
    )
    .unwrap();
    for id in [hd::HANDLE_FRONT, hd::HANDLE_TOP, hd::GRIP_FRONT, hd::GRIP_TOP] {
        assert!(texts(&doc, id).iter().all(|x| !x.2), "red remains in {id:?}");
    }

    // D14.11: the Grip sheet.
    let t = texts(&doc, hd::GRIP_FRONT);
    assert_eq!(text_of(&t, hd::DIM_GRIP_D), (format!("Ø{}", two(25.0)), false));
    // 185.00 = the second end (175) + the first (10).
    assert_eq!(text_of(&t, hd::DIM_GRIP_LENGTH), (two(175.0 + 10.0), false));
    assert_eq!(text_of(&t, hd::DIM_GRIP_35), (two(25.0 + 10.0), false));
    assert_eq!(text_of(&t, hd::DIM_GRIP_62), (two(125.0 / 2.0), false));
    // ISO 273 medium (Normal) M6: Ø6.6; ISO 4762 M6 head dk 10 → ⌴ 10 + 1.25, k 6 deep.
    let callout = format!("3x Ø{} THRU ⌴Ø{} ↧{}", two(6.6), two(10.0 + 1.25), two(6.0));
    assert_eq!(text_of(&t, hd::CALLOUT_GRIP), (callout, false));
    let t = texts(&doc, hd::GRIP_TOP);
    assert_eq!(text_of(&t, hd::DIM_SLOT), (two(8.0), false));
    assert_eq!(text_of(&t, hd::DIM_SLOT_WALL), (two((25.0 - 8.0) / 2.0), false));

    // Undoing the deletions, the re-attach and the update puts the old drawing back: out of date.
    for _ in 0..3 {
        h.undo(&mut doc);
    }
    assert_eq!(sorted(ds::out_of_date(&doc, drawing(&doc))), sorted(ALL_VIEWS.to_vec()));
    assert_eq!(text_of(&texts(&doc, hd::HANDLE_FRONT), hd::DIM_LENGTH).0, "225.00");
}

/// P3C.5, D14.7, D14.8: the Assembly sheet's BOM is the course's 10 rows; editing the three
/// ISO 4762 cap screws to M6 marks only the assembly view and the BOM out of date; after Update
/// row 8 is the M6 configuration (its Description says M6), the balloons still point at their
/// parts and the M6 heads are bigger in the view.
#[test]
fn d14_7_m6_cap_screws_update_the_assembly_sheet() {
    use cadrs_core::drawing_assembly as da;
    use cadrs_core::samples::hand_brake::assembly as ha;
    use cadrs_drawing::assembly::{SheetModel, callout_dangles, callout_texts};
    let mut doc = hb::document().unwrap();
    // The course's 12 mates (P3C wrap-up): Fastened, each met where the parts are.
    let asm = doc.element(ha::ASSEMBLY).and_then(|e| e.assembly_model()).unwrap();
    assert_eq!(asm.mates.len(), 12);
    assert!(asm.mates.iter().all(|m| matches!(&m.kind, cadrs_core::assembly::mate::MateKind::Mate(m) if m.mate_type == cadrs_core::assembly::mate::MateType::Fastened)));
    let d = drawing(&doc);
    let sheet = d.sheet(hd::ASSEMBLY_SHEET).unwrap();
    let bom = sheet.tables[0].bom.as_ref().unwrap();
    assert_eq!(bom.columns, ["Item No.", "Name", "Quantity"]);
    let rows: Vec<(String, String, u32)> = bom.rows.iter().map(|r| (r.item.clone(), r.cells[1].clone(), r.quantity)).collect();
    let want: Vec<(String, String, u32)> = ha::COURSE_BOM.iter().enumerate().map(|(i, (n, q))| ((i + 1).to_string(), n.to_string(), *q)).collect();
    assert_eq!(rows, want);
    // Ten balloons, one per item, each reading its item number.
    let (_, v) = d.view(hd::ASSEMBLY_ISO).unwrap();
    let geo = cadrs_drawing::annotation::ModelData::default();
    let m = SheetModel::new(d, sheet, v, &geo);
    let items: Vec<String> = v
        .annotations
        .iter()
        .filter_map(|a| match &a.kind {
            AnnotationKind::Callout(c) => Some(callout_texts(&m, c)[4].clone()),
            _ => None,
        })
        .collect();
    assert_eq!(items, (1..=10).map(|i| i.to_string()).collect::<Vec<_>>());
    let screw_owner = |doc: &Document| {
        let d = drawing(doc);
        let row = &d.sheet(hd::ASSEMBLY_SHEET).unwrap().tables[0].bom.as_ref().unwrap().rows[7];
        let owner = da::owner_of(&row.owner).unwrap();
        doc.standard_part(owner.element()).unwrap().spec.size.clone()
    };
    assert_eq!(screw_owner(&doc), ha::SCREW_BEFORE);
    let head = |doc: &Document| {
        let d = drawing(doc);
        let st = da::AssemblyState::parse(&d.source(ha::ASSEMBLY.0).unwrap().snapshot).unwrap();
        let o = st.occurrences.iter().find(|o| o.id == ha::CAP_SCREWS[0]).unwrap();
        let spec = &doc.standard_part(o.element).unwrap().spec;
        spec.size_row().unwrap().head.unwrap()
    };
    assert_eq!(head(&doc), 8.5, "ISO 4762 M5: dk 8.5");
    // D14.7.
    let mut h = History::default();
    ha::course_d14_7(&mut DocHistory(&mut doc, &mut h)).unwrap();
    assert_eq!(ds::out_of_date(&doc, drawing(&doc)), vec![hd::ASSEMBLY_ISO], "only the assembly view");
    assert_eq!(ds::bom_updates(&doc, drawing(&doc)).len(), 1, "and its BOM");
    assert_eq!(screw_owner(&doc), ha::SCREW_BEFORE, "nothing changes before Update");
    // D14.8.
    let op = ds::update_now(&doc, drawing(&doc)).unwrap();
    assert_eq!(op.label(), "Update from this workspace");
    h.execute(&mut doc, &EditDrawing { element: hb::DRAWING, op }).unwrap();
    assert!(ds::out_of_date(&doc, drawing(&doc)).is_empty());
    assert!(ds::bom_updates(&doc, drawing(&doc)).is_empty());
    assert_eq!(screw_owner(&doc), ha::SCREW_AFTER, "row 8 is the M6 cap screw");
    let d = drawing(&doc);
    let row = &d.sheet(hd::ASSEMBLY_SHEET).unwrap().tables[0].bom.as_ref().unwrap().rows[7];
    let owner = da::owner_of(&row.owner).unwrap();
    let desc = cadrs_core::properties::text(&doc, owner, cadrs_core::properties::PropertyKey::Description, None);
    assert!(desc.contains("M6 x 16"), "{desc}");
    assert_eq!((row.cells[1].as_str(), row.quantity), ("Hex socket head cap screw ISO 4762", 3), "the course's row");
    assert_eq!(head(&doc), 10.0, "ISO 4762 M6: dk 10");
    let (_, v) = d.view(hd::ASSEMBLY_ISO).unwrap();
    let m = SheetModel::new(d, d.sheet(hd::ASSEMBLY_SHEET).unwrap(), v, &geo);
    for a in &v.annotations {
        if let AnnotationKind::Callout(c) = &a.kind {
            assert!(!callout_dangles(&m, c), "the balloons keep their parts");
        }
    }
}

/// P3C wrap-up (the stray dashed arcs of `course_drw_ex3_update` 30): after the course's edits the
/// grown Handle Plate runs into the enclosure, and OCCT's HLR of the separate solids left edges
/// inside it visible in short alternating pieces (the enclosure opening's circle in 4 visible
/// pieces of 1.2–30 mm between hidden ones). The assembly view now hides the pieces a part's
/// triangles cover: no edge is left in more than 3 visible pieces. Show part intersections (X6)
/// adds the curves where the plate enters the enclosure.
#[test]
fn assembly_view_hides_edges_inside_overlapping_parts() {
    use cadrs_core::drawing_assembly as da;
    use cadrs_core::samples::hand_brake::assembly as ha;
    use cadrs_kernel::ProjVisibility;
    let mut doc = hb::document().unwrap();
    let mut h = History::default();
    hb::course_edits(&mut DocHistory(&mut doc, &mut h), hb::STUDIO).unwrap();
    ha::course_d14_7(&mut DocHistory(&mut doc, &mut h)).unwrap();
    let op = ds::update_now(&doc, drawing(&doc)).unwrap();
    h.execute(&mut doc, &EditDrawing { element: hb::DRAWING, op }).unwrap();
    let d = drawing(&doc);
    let (_, v) = d.view(hd::ASSEMBLY_ISO).unwrap();
    let st = da::AssemblyState::parse(&d.source(ha::ASSEMBLY.0).unwrap().snapshot).unwrap();
    let g = da::project(&st, v).unwrap();
    let mut by: std::collections::BTreeMap<(usize, u64), Vec<(ProjVisibility, f64)>> = Default::default();
    for e in &g.projection.edges {
        if let Some(s) = &e.source
            && let Some(id) = s.edge
        {
            by.entry((s.body, id.0)).or_default().push((e.visibility, e.length()));
        }
    }
    for (k, pieces) in &by {
        let vis = pieces.iter().filter(|p| p.0 == ProjVisibility::Visible).count();
        assert!(vis <= 3, "edge {k:?} in {vis} visible pieces: {pieces:?}");
    }
    // X6, Show part intersections: the curves where the grown Handle Plate enters the enclosure
    // (visible, sharp, no source), none without the option.
    let crossings = |g: &ViewGeometry| {
        g.projection
            .edges
            .iter()
            .filter(|e| e.source.is_none() && e.class == cadrs_kernel::ProjClass::Sharp && e.visibility == ProjVisibility::Visible)
            .map(|e| e.length())
            .sum::<f64>()
    };
    assert_eq!(crossings(&g), 0.0);
    let mut shown = v.clone();
    shown.part_intersections = true;
    let gi = da::project(&st, &shown).unwrap();
    assert!(crossings(&gi) > 10.0, "{}", crossings(&gi));
    assert_eq!(gi.projection.edges.len() - g.projection.edges.len(), gi.projection.edges.iter().filter(|e| e.source.is_none() && e.class == cadrs_kernel::ProjClass::Sharp).count() - g.projection.edges.iter().filter(|e| e.source.is_none() && e.class == cadrs_kernel::ProjClass::Sharp).count());
}
