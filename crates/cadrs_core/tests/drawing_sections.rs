//! P3C.8: section, detail, crop, broken-out and break views and thread display, checked against
//! hand-computed values:
//!
//! - a tube Ø40 outside, Ø20 inside and 30 long, cut through its axis, has a hatched area of
//!   2 × (40 − 20)/2 × 30 = **600 mm²**, from the hatch region's loops;
//! - a detail view at 2:1 of the bar's Ø6 hole reads Ø6 like its 1:1 parent;
//! - the bar's 200 mm length dimensioned across a break (x 50..150) still reads 200, while the
//!   drawn span is 200 − (100 − 8) = 108 mm on paper;
//! - the bar's M6 tapped hole's thread: major Ø6, minor Ø5 (the M6×1 tap drill), 8 long from
//!   the top face, down;
//! - every new view operation undoes.
#![cfg(feature = "occt")]

use cadrs_core::commands::{AddSketch, EditDrawing, EditSketch};
use cadrs_core::document::{Document, Element};
use cadrs_core::samples::drawing_bar::{self as bar, views as bar_views};
use cadrs_core::samples::drawing_bracket as bracket;
use cadrs_core::samples::gear_cover::{DocHistory, Studio};
use cadrs_core::views::{self, ViewRequest};
use cadrs_core::{ElementId, Feature, FeatureId, History};
use cadrs_drawing::annotation::{AnnotationKind, DimTool, EdgeRef, Pick, Shape, measure, propose, resolve};
use cadrs_drawing::view::{Placement, projected_view};
use cadrs_drawing::view_kinds::{self as vk, Boundary, Break, BrokenOut, detail_view, double, region_area, section_view};
use cadrs_drawing::{DrawingOp, NamedView, ObjectRef, Projection, Scale, View};
use cadrs_kernel::ProjectOptions;
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

fn request(view: &View) -> ViewRequest {
    ViewRequest {
        part: None,
        frame: view.frame.view_frame(),
        options: ProjectOptions { hidden: true, ..ProjectOptions::default() },
        shaded: false,
        props: Vec::new(),
        appearances: Vec::new(),
        cut: view.effective_cut(),
        intersections: false,
    }
}

fn reference() -> ObjectRef {
    ObjectRef { element: uuid::Uuid::nil(), part: None }
}

/// A tube on Top: Ø40 outside, Ø20 inside, 30 long (z 0..30), its axis the Z axis.
fn tube() -> Vec<Feature> {
    let mut d = Document::empty("Tube");
    let el = Element::part_studio("Part Studio 1");
    let id = el.id;
    d.elements.push(el);
    let mut h = History::default();
    let mut s = DocHistory(&mut d, &mut h);
    let sketch = FeatureId::new();
    s.run(&AddSketch { element: id, feature: sketch, plane: Some(PlaneRef::Top) }).unwrap();
    for r in [20.0, 10.0] {
        s.run(&EditSketch { element: id, feature: sketch, op: SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: r, construction: false } })
            .unwrap();
    }
    bracket::extrude(&mut s, id, sketch, FeatureId::new(), &[Vec2::new(15.0, 0.0)], |e| {
        e.depth = 30.0;
        e.depth_expr = "30 mm".into();
    })
    .unwrap();
    d.element(id).unwrap().features().to_vec()
}

#[test]
fn a_tube_cut_through_its_axis_has_600_mm2_of_hatch() {
    let f = tube();
    let front = View::base(reference(), NamedView::Front, Scale::new(1, 1), [100.0, 100.0]);
    // A vertical cutting line through the axis (x = 0) of the Front view, placed to the right.
    let s = section_view(&front, [0.0, -5.0], [0.0, 40.0], [180.0, 100.0], Projection::Third, "A").unwrap();
    assert_eq!(NamedView::of(&s.frame), Some(NamedView::Right));
    let g = views::project(&f, request(&s)).unwrap();
    assert!(!g.hatch.is_empty(), "no hatch region");
    // Two 10 × 30 rectangles: the wall on either side of the bore.
    close(region_area(&g.hatch), 600.0, 1e-6);
    assert_eq!(g.hatch.len(), 2);
    // The hatch lines lie inside the region: 45° on the sheet.
    let lines = vk::hatch_lines(&s, &*g);
    assert!(lines.len() > 10);
    for l in &lines {
        let (a, b) = (l[0], l[l.len() - 1]);
        close((b[1] - a[1]).abs(), (b[0] - a[0]).abs(), 1e-6);
    }
    // The uncut Right view has no hatch and more hidden lines' worth of geometry.
    let right = projected_view(&front, Placement::Ortho([1.0, 0.0]), Projection::Third, [180.0, 100.0], None);
    let plain = views::project(&f, request(&right)).unwrap();
    assert!(plain.hatch.is_empty());
}

#[test]
fn a_broken_out_section_hatches_inside_its_boundary_only() {
    let f = tube();
    let mut front = View::base(reference(), NamedView::Front, Scale::new(1, 1), [100.0, 100.0]);
    // A box around the right wall (x 5..25, z 5..25), cut to the axis (depth 0 along +Y).
    front.broken_out = Some(BrokenOut { boundary: Boundary::rectangle([5.0, 5.0], [25.0, 25.0]), depth: 0.0 });
    let g = views::project(&f, request(&front)).unwrap();
    // The wall x 10..20 inside z 5..25: 10 × 20.
    close(region_area(&g.hatch), 200.0, 1e-6);
}

#[test]
fn the_bar_s_thread_is_found_from_its_tapped_hole() {
    let (doc, studio, _) = bar::document().unwrap();
    let f = doc.element(studio).unwrap().features().to_vec();
    let top = View::base(reference(), NamedView::Top, Scale::new(1, 1), [50.0, 50.0]);
    let g = views::project(&f, request(&top)).unwrap();
    assert_eq!(g.threads.len(), 1, "{:?}", g.threads);
    let t = g.threads[0];
    close(t.major, 6.0, 1e-9);
    close(t.minor, 5.0, 1e-9);
    close(t.length, bar::TAP_DEPTH, 1e-6);
    close(t.center[0], bar::TAP_X, 1e-6);
    close(t.center[2], 0.0, 1e-6);
    close(t.axis[2], -1.0, 1e-9);
    // Seen end on in Top: a 3/4 circle of Ø6 drawn when threads are shown.
    let mut v = top.clone();
    v.threads = true;
    let d = vk::view_decor(&cadrs_drawing::DrawingStyle::default(), std::slice::from_ref(&v), &v, Some(&*g), &[]);
    let arc = d.thin.iter().find(|l| l.len() > 40).expect("the thread's arc");
    let c = v.to_sheet([bar::TAP_X, 0.0]);
    for p in arc {
        close(((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt(), 3.0, 1e-9);
    }
}

/// The bar's drawing, its Front view and the geometry of a view of it.
fn bar_drawing() -> (Document, ElementId, Vec<Feature>, cadrs_drawing::Drawing) {
    let (doc, studio, drawing) = bar::document().unwrap();
    let f = doc.element(studio).unwrap().features().to_vec();
    let d = doc.element(drawing).unwrap().drawing_data().unwrap().clone();
    (doc, drawing, f, d)
}

fn circle_ref(v: &View, g: &cadrs_core::views::ViewGeometry, c: [f64; 2], r: f64) -> EdgeRef {
    g.projection
        .edges
        .iter()
        .map(EdgeRef::of)
        .find(|e| matches!(resolve(v, g, e).shape, Shape::Circle { center, radius, .. } if (radius - r).abs() < 1e-6 && (center[0] - c[0]).abs() < 1e-6 && (center[1] - c[1]).abs() < 1e-6))
        .expect("the circle")
}

#[test]
fn a_detail_at_2_to_1_reads_the_same_diameter_as_its_parent() {
    let (_, _, f, d) = bar_drawing();
    let (_, top) = d.view(bar_views::TOP).unwrap();
    let gp = views::project(&f, request(top)).unwrap();
    let hole = circle_ref(top, &gp, [bar::HOLE_X, 0.0], bar::HOLE_D / 2.0);
    let dp = propose(DimTool::Diameter, top, &*gp, &[Pick::Edge(hole)], [bar::HOLE_X + 8.0, 8.0]).unwrap();
    let parent_value = measure(top, &*gp, &dp).unwrap().value;
    let detail = detail_view(top, [bar::HOLE_X, 0.0], 6.0, double(top.scale), [150.0, 60.0], "B");
    assert_eq!(detail.scale, Scale::new(2, 1));
    let gd = views::project(&f, request(&detail)).unwrap();
    let hole_d = circle_ref(&detail, &gd, [bar::HOLE_X, 0.0], bar::HOLE_D / 2.0);
    let dd = propose(DimTool::Diameter, &detail, &*gd, &[Pick::Edge(hole_d)], [bar::HOLE_X + 4.0, 4.0]).unwrap();
    let detail_value = measure(&detail, &*gd, &dd).unwrap().value;
    close(parent_value, 6.0, 1e-9);
    close(detail_value, parent_value, 1e-9);
    // Twice as big on paper.
    let (a, b) = (detail.to_sheet([bar::HOLE_X - 3.0, 0.0]), detail.to_sheet([bar::HOLE_X + 3.0, 0.0]));
    close(b[0] - a[0], 12.0, 1e-9);
    // The detail shows only what is inside its circle.
    let lines = cadrs_drawing::view::view_lines(&detail, &gd.projection);
    let c = detail.to_sheet([bar::HOLE_X, 0.0]);
    for l in &lines {
        for p in &l.points {
            assert!(((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt() <= 12.0 + 1e-3);
        }
    }
}

#[test]
fn a_dimension_across_a_break_reads_the_true_length() {
    let (_, _, f, d) = bar_drawing();
    let (_, front) = d.view(bar_views::FRONT).unwrap();
    let mut broken = front.clone();
    broken.breaks.push(Break { vertical: true, lo: 50.0, hi: 150.0 });
    let g = views::project(&f, request(&broken)).unwrap();
    let AnnotationKind::Dimension(dim) = &front.annotations[0].kind else { panic!("the length dimension") };
    close(measure(&broken, &*g, dim).unwrap().value, bar::LENGTH, 1e-9);
    let gr = cadrs_drawing::annotation::annotation_graphics(&d.style, &broken, &*g, &front.annotations[0]).unwrap();
    assert_eq!(gr.texts.iter().map(|t| t.text.as_str()).collect::<String>(), "200.00");
    // Drawn 108 mm apart (1:1): the ends' extension lines.
    let (a, b) = (broken.to_sheet([0.0, 0.0]), broken.to_sheet([bar::LENGTH, 0.0]));
    close(b[0] - a[0], bar::LENGTH - (100.0 - vk::BREAK_GAP), 1e-9);
    // No line is drawn inside the gap.
    let (g0, g1) = (broken.to_sheet([50.0, 0.0])[0], broken.to_sheet([150.0, 0.0])[0]);
    for l in cadrs_drawing::view::view_lines(&broken, &g.projection) {
        for w in l.points.windows(2) {
            let mid = (w[0][0] + w[1][0]) / 2.0;
            assert!(!(mid > g0 + 1e-6 && mid < g1 - 1e-6), "a line in the gap at x {mid}");
        }
    }
}

#[test]
fn new_view_operations_undo() {
    let (mut doc, drawing, _, d) = bar_drawing();
    let mut h = History::default();
    let sheet = d.sheets[0].id;
    let (_, front) = d.view(bar_views::FRONT).unwrap();
    let (_, top) = d.view(bar_views::TOP).unwrap();
    let section = section_view(front, [bar::HOLE_X, -12.0], [bar::HOLE_X, 2.0], [262.0, 112.0], d.projection, "A").unwrap();
    let detail = detail_view(top, [bar::HOLE_X, 0.0], 6.0, double(top.scale), [150.0, 60.0], "B");
    let mut cropped = top.clone();
    cropped.crop = Some(Boundary::rectangle([150.0, -12.0], [202.0, 12.0]));
    let mut broken_out = front.clone();
    broken_out.broken_out = Some(BrokenOut { boundary: Boundary::spline(vec![[20.0, -2.0], [40.0, -2.0], [40.0, -8.0], [20.0, -8.0]]), depth: 0.0 });
    let mut broken = front.clone();
    broken.breaks.push(Break { vertical: true, lo: 50.0, hi: 150.0 });
    let mut threads = top.clone();
    threads.threads = true;
    let ops = vec![
        DrawingOp::InsertView { sheet, view: section },
        DrawingOp::InsertView { sheet, view: detail },
        DrawingOp::SetView { view: cropped, label: "Crop view".into() },
        DrawingOp::SetView { view: broken_out, label: "Broken-out section".into() },
        DrawingOp::SetView { view: broken, label: "Break view".into() },
        DrawingOp::SetView { view: threads, label: "Show threads".into() },
    ];
    let before = doc.element(drawing).unwrap().drawing_data().unwrap().clone();
    let mut states = vec![before.clone()];
    for op in ops {
        h.execute(&mut doc, &EditDrawing { element: drawing, op }).unwrap();
        states.push(doc.element(drawing).unwrap().drawing_data().unwrap().clone());
    }
    for k in (0..states.len() - 1).rev() {
        assert!(h.undo(&mut doc).is_some());
        assert_eq!(doc.element(drawing).unwrap().drawing_data().unwrap(), &states[k]);
    }
    // And the stored views read back (serde).
    let text = ron::to_string(&states[states.len() - 1]).unwrap();
    let back: cadrs_drawing::Drawing = ron::from_str(&text).unwrap();
    assert_eq!(back, states[states.len() - 1]);
}
