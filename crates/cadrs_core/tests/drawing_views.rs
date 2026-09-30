//! P3C.2: drawing views projected from Part Studio parts (the kernel's hidden-line removal
//! through `cadrs_core::views`) and placed by `cadrs_drawing::view`. The checks use the
//! geometry itself, not the code that placed it:
//!
//! - the P6.2 L-block (4 × 4 with a 2 × 2 notch in its top right corner): in **first angle** the
//!   view placed right of Front shows the notch's floor as a *hidden* line (so it is the view
//!   from the left) and the view below Front shows the step as a *visible* line (the view from
//!   above); in **third angle** the view right of Front shows the floor *visible* (the view from
//!   the right) and the view above shows the step visible (from above);
//! - a 20 mm cube with a Ø10 through hole shows exactly 2 dashed lines in its side view (the
//!   hole's two sides, 5 mm either side of its axis, 20 mm long);
//! - at 1:2 every projected line is half as long as at 1:1.
#![cfg(feature = "occt")]

use cadrs_core::commands::{AddSketch, EditSketch};
use cadrs_core::document::{Document, Element};
use cadrs_core::samples::drawing_bracket as bracket;
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::views::{self, ViewRequest};
use cadrs_core::{ElementId, Feature, FeatureId, History};
use cadrs_drawing::view::{LineKind, Placement, projected_view, view_bounds, view_lines};
use cadrs_drawing::{NamedView, ObjectRef, Projection, Scale, View};
use cadrs_kernel::ProjectOptions;
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

/// A document with one Part Studio.
fn studio() -> (Document, History, ElementId) {
    let mut d = Document::empty("Views");
    let el = Element::part_studio("Part Studio 1");
    let id = el.id;
    d.elements.push(el);
    (d, History::default(), id)
}

fn features(d: &Document, el: ElementId) -> Vec<Feature> {
    d.element(el).unwrap().features().to_vec()
}

/// A polygon on `plane`, extruded `depth` (seeded at `seed`).
fn prism(d: &mut Document, h: &mut History, el: ElementId, plane: PlaneRef, ops: Vec<SketchOp>, seed: Vec2, depth: f64) {
    let mut s = DocHistory(d, h);
    let sketch = FeatureId::new();
    use cadrs_core::samples::gear_cover::Studio;
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(plane) }).unwrap();
    for op in ops {
        s.run(&EditSketch { element: el, feature: sketch, op }).unwrap();
    }
    bracket::extrude(&mut s, el, sketch, FeatureId::new(), &[seed], |e| {
        e.depth = depth;
        e.depth_expr = format!("{depth} mm");
    })
    .unwrap();
}

fn poly(points: &[(f64, f64)]) -> SketchOp {
    SketchOp::AddPolyline {
        points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
        closed: true,
        construction: false,
        label: "Add polygon",
    }
}

fn request(view: &View, hidden: bool) -> ViewRequest {
    ViewRequest {
        part: None,
        frame: view.frame.view_frame(),
        options: ProjectOptions { hidden, ..ProjectOptions::default() },
        shaded: false,
        props: Vec::new(),
        appearances: Vec::new(),
        cut: None,
        intersections: false,
    }
}

fn reference() -> ObjectRef {
    ObjectRef { element: uuid::Uuid::nil(), part: None }
}

/// The L-block on Front: 4 wide (x 0..4), 4 tall (z 0..4), 4 deep, with the 2 × 2 notch at
/// x 2..4, z 2..4.
fn l_block() -> Vec<Feature> {
    let (mut d, mut h, el) = studio();
    let l = poly(&[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (2.0, 2.0), (2.0, 4.0), (0.0, 4.0)]);
    prism(&mut d, &mut h, el, PlaneRef::Front, vec![l], Vec2::new(1.0, 1.0), 4.0);
    features(&d, el)
}

/// Straight lines of `kind` in a view, in the view's 2D frame (model mm): (start, end).
fn lines(view: &View, f: &[Feature], kind: LineKind) -> Vec<([f64; 2], [f64; 2])> {
    let g = views::project(f, request(view, true)).unwrap();
    let mut v = view.clone();
    v.hidden_lines = true;
    view_lines(&v, &g.projection)
        .into_iter()
        .filter(|l| l.kind == kind)
        .map(|l| (v.from_sheet(l.points[0]), v.from_sheet(*l.points.last().unwrap())))
        .collect()
}

fn horizontal_at(ls: &[([f64; 2], [f64; 2])], y: f64) -> bool {
    ls.iter().any(|(a, b)| (a[1] - y).abs() < 1e-6 && (b[1] - y).abs() < 1e-6 && (a[0] - b[0]).abs() > 1.0)
}

fn vertical_at(ls: &[([f64; 2], [f64; 2])], x: f64) -> bool {
    ls.iter().any(|(a, b)| (a[0] - x).abs() < 1e-6 && (b[0] - x).abs() < 1e-6 && (a[1] - b[1]).abs() > 1.0)
}

/// Sheet bounds of a view.
fn bounds(view: &View, f: &[Feature]) -> ([f64; 2], [f64; 2]) {
    let g = views::project(f, request(view, false)).unwrap();
    view_bounds(view, &g.projection).unwrap()
}

#[test]
fn l_block_first_angle_puts_left_on_the_right_and_top_below() {
    let f = l_block();
    let front = View::base(reference(), NamedView::Front, Scale::new(10, 1), [100.0, 150.0]);
    // The cursor off the fold lines: the views snap onto them.
    let side = projected_view(&front, Placement::Ortho([1.0, 0.0]), Projection::First, [200.0, 163.0], None);
    let below = projected_view(&front, Placement::Ortho([0.0, -1.0]), Projection::First, [91.0, 60.0], None);
    assert_eq!(side.anchor, [200.0, 150.0], "snapped onto Front's horizontal fold line");
    assert_eq!(below.anchor, [100.0, 60.0], "snapped onto Front's vertical fold line");
    // Placement: right of and below the front view, aligned with it.
    let (fb, sb, bb) = (bounds(&front, &f), bounds(&side, &f), bounds(&below, &f));
    assert!(sb.0[0] > fb.1[0], "the side view is right of Front");
    close(sb.0[1], fb.0[1], 1e-6);
    close(sb.1[1], fb.1[1], 1e-6);
    assert!(bb.1[1] < fb.0[1], "the view below is under Front");
    close(bb.0[0], fb.0[0], 1e-6);
    // The side view hides the notch's floor (z = 2): it is seen from the left.
    let side_hidden = lines(&side, &f, LineKind::Hidden);
    let side_visible = lines(&side, &f, LineKind::Visible);
    assert!(horizontal_at(&side_hidden, 2.0), "hidden floor: {side_hidden:?}");
    assert!(!horizontal_at(&side_visible, 2.0));
    // The view below shows the step (x = 2) as a visible line: it is seen from above.
    let below_visible = lines(&below, &f, LineKind::Visible);
    assert!(vertical_at(&below_visible, 2.0), "visible step: {below_visible:?}");
    assert_eq!(side.name, "Left");
    assert_eq!(below.name, "Top");
}

#[test]
fn l_block_third_angle_puts_top_above_and_right_on_the_right() {
    let f = l_block();
    let front = View::base(reference(), NamedView::Front, Scale::new(10, 1), [100.0, 100.0]);
    let side = projected_view(&front, Placement::Ortho([1.0, 0.0]), Projection::Third, [200.0, 87.5], None);
    let above = projected_view(&front, Placement::Ortho([0.0, 1.0]), Projection::Third, [111.0, 200.0], None);
    assert_eq!(side.anchor, [200.0, 100.0], "snapped onto Front's horizontal fold line");
    assert_eq!(above.anchor, [100.0, 200.0], "snapped onto Front's vertical fold line");
    let (fb, sb, ab) = (bounds(&front, &f), bounds(&side, &f), bounds(&above, &f));
    assert!(sb.0[0] > fb.1[0]);
    assert!(ab.0[1] > fb.1[1], "the view above is over Front");
    close(ab.0[0], fb.0[0], 1e-6);
    close(ab.1[0], fb.1[0], 1e-6);
    // The side view sees the notch's floor: it is seen from the right.
    let side_visible = lines(&side, &f, LineKind::Visible);
    assert!(horizontal_at(&side_visible, 2.0), "visible floor: {side_visible:?}");
    assert!(!horizontal_at(&lines(&side, &f, LineKind::Hidden), 2.0));
    // The view above sees the step.
    assert!(vertical_at(&lines(&above, &f, LineKind::Visible), 2.0));
    assert_eq!(side.name, "Right");
    assert_eq!(above.name, "Top");
}

#[test]
fn cube_with_a_hole_shows_two_dashed_lines_from_the_side() {
    let (mut d, mut h, el) = studio();
    let square = poly(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]);
    let hole = SketchOp::AddCircle { center: Vec2::new(10.0, 10.0), radius: 5.0, construction: false };
    prism(&mut d, &mut h, el, PlaneRef::Top, vec![square, hole], Vec2::new(1.0, 1.0), 20.0);
    let f = features(&d, el);
    let mut side = View::base(reference(), NamedView::Right, Scale::new(1, 1), [0.0, 0.0]);
    side.hidden_lines = true;
    let g = views::project(&f, request(&side, true)).unwrap();
    let dashed: Vec<_> = view_lines(&side, &g.projection)
        .into_iter()
        .filter(|l| l.kind == LineKind::Hidden)
        .collect();
    assert_eq!(dashed.len(), 2, "{dashed:?}");
    // In the right view (2D x = model Y), the hole's sides are at y = 5 and y = 15, 20 long.
    let mut xs: Vec<f64> = dashed.iter().map(|l| l.points[0][0]).collect();
    xs.sort_by(f64::total_cmp);
    close(xs[0], 5.0, 1e-6);
    close(xs[1], 15.0, 1e-6);
    for l in &dashed {
        close((l.points[1][1] - l.points[0][1]).abs(), 20.0, 1e-6);
    }
    // With hidden lines off, none.
    side.hidden_lines = false;
    assert!(view_lines(&side, &g.projection).iter().all(|l| l.kind != LineKind::Hidden));
}

#[test]
fn half_scale_halves_every_projected_length() {
    let (mut d, mut h, el) = studio();
    {
        let mut s = DocHistory(&mut d, &mut h);
        bracket::build_in(&mut s, el).unwrap();
    }
    let f = features(&d, el);
    let mut full = View::base(reference(), NamedView::Front, Scale::new(1, 1), [30.0, 40.0]);
    full.hidden_lines = true;
    let mut half = full.clone();
    half.scale = Scale::new(1, 2);
    let g = views::project(&f, request(&full, true)).unwrap();
    let len = |pts: &[[f64; 2]]| -> f64 {
        pts.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).sum()
    };
    let a = view_lines(&full, &g.projection);
    let b = view_lines(&half, &g.projection);
    assert_eq!(a.len(), b.len());
    assert!(a.len() > 10);
    for (x, y) in a.iter().zip(&b) {
        let (lx, ly) = (len(&x.points), len(&y.points));
        // At 1:1 the sheet length is the projected edge's model length (collinear pieces that
        // overlap are drawn once, so a line is never longer than the edges it covers).
        if x.points.len() > 2 {
            close(lx, g.projection.edges[x.edge].length(), 1e-9);
        }
        close(ly, lx / 2.0, 1e-9);
    }
    // Independent value: the plate is 120 mm wide (2 × bracket::PLATE_X), so its bottom edge
    // (z = 0: the straight edge and the rounded corners seen edge-on) spans 120 mm on the sheet
    // at 1:1 and 60 mm at 1:2.
    let bottom = |lines: &[cadrs_drawing::view::ViewLine], v: &View| -> f64 {
        let xs: Vec<f64> = lines
            .iter()
            .filter(|l| l.kind != LineKind::Hidden)
            .filter(|l| l.points.iter().all(|p| v.from_sheet(*p)[1].abs() < 1e-6))
            .flat_map(|l| l.points.iter().map(|p| p[0]))
            .collect();
        xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min)
    };
    close(bottom(&a, &full), 120.0, 1e-6);
    close(bottom(&b, &half), 60.0, 1e-6);
}

#[test]
fn bracket_views_have_hidden_and_tangent_edges_with_names() {
    let (mut d, mut h, el) = studio();
    {
        let mut s = DocHistory(&mut d, &mut h);
        bracket::build_in(&mut s, el).unwrap();
    }
    let f = features(&d, el);
    let front = View::base(reference(), NamedView::Front, Scale::new(1, 2), [0.0, 0.0]);
    let mut req = request(&front, true);
    req.shaded = true;
    let g = views::project(&f, req).unwrap();
    use cadrs_kernel::{ProjClass, ProjVisibility};
    // The plate's four holes seen from the front: their sides are hidden.
    assert!(g.projection.of(ProjVisibility::Hidden, ProjClass::Outline).count() >= 4);
    // The plate's rounded corners meet its front and side faces in tangent edges.
    assert!(g.projection.of(ProjVisibility::Visible, ProjClass::Smooth).count() >= 2);
    // Sharp edges carry persistent names.
    let named = g
        .projection
        .edges
        .iter()
        .filter(|e| e.class == ProjClass::Sharp)
        .filter(|e| e.source.and_then(|s| s.edge_name).is_some())
        .count();
    let sharp = g.projection.edges.iter().filter(|e| e.class == ProjClass::Sharp).count();
    assert!(named * 10 >= sharp * 9, "{named} of {sharp} sharp edges named");
    // Shaded triangles cover the part's outline (120 wide, 70 tall).
    assert!(!g.shaded.is_empty());
    let (lo, hi) = g.bounds.unwrap();
    close(hi[0] - lo[0], 2.0 * bracket::PLATE_X, 1e-6);
    close(hi[1] - lo[1], bracket::UP_TOP, 1e-6);
}
