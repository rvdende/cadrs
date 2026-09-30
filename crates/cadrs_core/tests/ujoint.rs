//! P3C.3 / D8: the Universal Joint Flange stand-in (`cadrs_core::samples::ujoint`,
//! `fixtures/ujoint_flange_standin.cadrs`) and the Ex1 drawing's dimensions and hole callouts.
//!
//! **The fixture.** Onshape's public "Exercise: Universal Joint Drawing" can't be copied, so the
//! yoke is rebuilt from the course drawing's dimensions (`ex1-drawing.png`, `ex1-step9.png`), in
//! inches, with real Part Studio features: a Ø4.750 disc extruded 6.000 (from 0.500 below the
//! Top plane, which is the flange's top face), a 2.600 slot removed along Y, the lugs' side
//! profile removed along X (2.500 wide, top corners at 43.0° to the top edge, a flare at 120.0°
//! to the flange), flats at x = ±2.000, Ø1.750 bosses, a Ø1.250 cross hole, a 2 × 2 × .25 pocket,
//! a Hole feature for the four counterbored holes (1/4 clearance Ø.266 THRU, ⌴Ø.438 ↧.250, on a
//! 2.061 × 3.282 rectangle of centres), two Hole features for the eight lug holes (Ø.266 THRU on a
//! Ø2.125 bolt circle), a R.250 fillet where the flares meet the lugs and a .060 chamfer on the
//! bottom rim. The part is "Universal Joint Flange (stand-in)", described "Made by cadrs".
//! Assumed where the pictures are ambiguous: the Ø1.750 circle is a boss (not the bolt circle),
//! the angles' sides, the pocket, flats, fillet, chamfer and bolt circle. The module docs of
//! `samples::ujoint` give the details; `CADRS_WRITE_FIXTURES=1` regenerates the file.
#![cfg(feature = "occt")]

use cadrs_core::rebuild;
use cadrs_core::samples::ujoint as uj;

#[test]
fn the_flange_builds() {
    let doc = uj::document().expect("the stand-in builds");
    let el = &doc.elements[0];
    let b = rebuild::build(el.features());
    assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
    assert_eq!(b.parts.len(), 1);
    let p = &b.parts[0];
    assert_eq!(p.id, uj::PART);
    // Its extent: the Ø4.750 disc, −0.5..5.5 in high.
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for q in &p.solid.positions {
        for i in 0..3 {
            lo[i] = lo[i].min(q[i] / uj::IN);
            hi[i] = hi[i].max(q[i] / uj::IN);
        }
    }
    let close = |a: f64, b: f64| (a - b).abs() < 2e-3;
    assert!(close(lo[0], -uj::FLANGE_D / 2.0) && close(hi[0], uj::FLANGE_D / 2.0), "{lo:?} {hi:?}");
    assert!(close(lo[1], -uj::FLANGE_D / 2.0) && close(hi[1], uj::FLANGE_D / 2.0), "{lo:?} {hi:?}");
    assert!(close(lo[2], -uj::FLANGE_T) && close(hi[2], uj::TOP), "{lo:?} {hi:?}");
}

#[test]
fn the_fixture_is_current() {
    // `fixtures/ujoint_flange_standin.cadrs` is the stand-in document as `uj::document()` builds
    // it. Regenerate it with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test ujoint`.
    let doc = uj::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ujoint_flange_standin.cadrs");
    let file = cadrs_core::samples::gear_cover::file(doc.clone());
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/ujoint_flange_standin.cadrs is out of date");
}

// ---------------------------------------------------------------------------------------------
// The Ex1 drawing's dimensions (D8.9) and hole callouts (D8.8), read from the drawing's
// annotations and compared with the model's parameters (the independent values: the sample's
// sketch and feature parameters, `uj::*`).

use std::sync::Arc;

use cadrs_core::views::{ViewGeometry, ViewRequest};
use cadrs_drawing::annotation::{
    Annotation, AnnotationKind, DimTool, Dimension, EdgeRef, HoleCallout, Pick, PointOf, PointRef, Shape, ValueKind,
    annotation_graphics, dimension_display, hole_of, hole_text, propose, resolve,
};
use cadrs_drawing::view::{Placement, projected_view};
use cadrs_drawing::{DrawingStyle, DrawingUnits, NamedView, ObjectRef, Projection, Scale, Standard, View};
use cadrs_kernel::ProjectOptions;

const IN: f64 = uj::IN;

fn geometry(features: &[cadrs_core::Feature], v: &View) -> Arc<ViewGeometry> {
    cadrs_core::views::project(
        features,
        ViewRequest {
            part: Some(uj::PART),
            frame: v.frame.view_frame(),
            options: ProjectOptions { tolerance: 0.01, hidden: true },
            shaded: false,
            props: Vec::new(),
            appearances: Vec::new(),
            cut: None,
            intersections: false,
        },
    )
    .expect("the view projects")
}

/// The projected edges of `g` whose model shape (inches) matches `f`, as references.
fn refs(v: &View, g: &ViewGeometry, f: impl Fn(&Shape) -> bool) -> Vec<EdgeRef> {
    let mut out: Vec<EdgeRef> = Vec::new();
    for e in &g.projection.edges {
        let r = EdgeRef::of(e);
        let res = resolve(v, g, &r);
        if res.from_model && f(&inches(res.shape)) && !out.iter().any(|o| o.edge == r.edge) {
            out.push(r);
        }
    }
    out
}

fn inches(s: Shape) -> Shape {
    let k = |p: [f64; 2]| [p[0] / IN, p[1] / IN];
    match s {
        Shape::Point(p) => Shape::Point(k(p)),
        Shape::Line { a, b } => Shape::Line { a: k(a), b: k(b) },
        Shape::Circle { center, radius, arc } => Shape::Circle { center: k(center), radius: radius / IN, arc: arc.map(|a| a.map(k)) },
        Shape::Curve { a, b } => Shape::Curve { a: k(a), b: k(b) },
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

fn circle(center: [f64; 2], r: f64) -> impl Fn(&Shape) -> bool {
    move |s| matches!(*s, Shape::Circle { center: c, radius, .. } if near(radius, r) && near(c[0], center[0]) && near(c[1], center[1]))
}

/// A horizontal line at height `at` reaching over `x`.
fn hline(at: f64, x: f64) -> impl Fn(&Shape) -> bool {
    move |s| matches!(*s, Shape::Line { a, b } if near(a[1], at) && near(b[1], at) && a[0].min(b[0]) - 1e-4 <= x && x <= a[0].max(b[0]) + 1e-4)
}

/// A vertical line at `at` reaching over height `y`.
fn vline(at: f64, y: f64) -> impl Fn(&Shape) -> bool {
    move |s| matches!(*s, Shape::Line { a, b } if near(a[0], at) && near(b[0], at) && a[1].min(b[1]) - 1e-4 <= y && y <= a[1].max(b[1]) + 1e-4)
}

fn first(v: Vec<EdgeRef>, what: &str) -> EdgeRef {
    *v.first().unwrap_or_else(|| panic!("no edge: {what}"))
}

fn centre(e: EdgeRef) -> Pick {
    Pick::Point(PointRef { edge: e, of: PointOf::Center, hint: [0.0, 0.0] })
}

#[test]
fn ex1_dimensions_and_callouts_match_the_model() {
    let doc = uj::document().unwrap();
    let features = doc.elements[0].features().to_vec();
    let style = DrawingStyle::for_units(DrawingUnits::Inch, Standard::Ansi);
    let part = ObjectRef { element: doc.elements[0].id.0, part: Some((uj::PART.feature.0, uj::PART.index)) };
    // D8.4, D8.5: Front 1:2; Top above it and Right beside it (third angle).
    let front = View::base(part, NamedView::Front, Scale::new(1, 2), [65.0, 85.0]);
    let top = projected_view(&front, Placement::Ortho([0.0, 1.0]), Projection::Third, [65.0, 170.0], None);
    let right = projected_view(&front, Placement::Ortho([1.0, 0.0]), Projection::Third, [175.0, 85.0], None);
    assert_eq!((top.name.as_str(), right.name.as_str()), ("Top", "Right"));
    let (gf, gt, gr) = (geometry(&features, &front), geometry(&features, &top), geometry(&features, &right));
    let (hx, hy) = (uj::HOLES_X / 2.0, uj::HOLES_Y / 2.0);
    let w = uj::LUG_W / 2.0;
    let at = |x: f64, y: f64| [x * IN, y * IN];
    let d = |tool, v: &View, g: &ViewGeometry, picks: &[Pick], text: [f64; 2]| -> Dimension {
        propose(tool, v, g, picks, text).expect("the picks make a dimension")
    };
    // (view, its geometry, the dimension, its text, the model's value in inches or degrees)
    let mut dims: Vec<(&View, &ViewGeometry, Dimension, &str, f64)> = Vec::new();
    // Top view: Ø4.750 (the Diameter tool), and 3.282 and 2.061 between hole centres.
    let rim = first(refs(&top, &gt, circle([0.0, 0.0], uj::FLANGE_D / 2.0)), "the flange's rim");
    dims.push((&top, &gt, d(DimTool::Diameter, &top, &gt, &[Pick::Edge(rim)], at(2.6, 2.6)), "Ø4.750", uj::FLANGE_D));
    let hole = |x: f64, y: f64| first(refs(&top, &gt, circle([x, y], uj::CBORE_D / 2.0)), "a counterbore");
    let (h1, h2, h3) = (hole(-hx, -hy), hole(-hx, hy), hole(hx, -hy));
    dims.push((&top, &gt, d(DimTool::Smart, &top, &gt, &[centre(h1), centre(h2)], at(-2.9, 0.0)), "3.282", uj::HOLES_Y));
    dims.push((&top, &gt, d(DimTool::Smart, &top, &gt, &[centre(h1), centre(h3)], at(0.0, -2.7)), "2.061", uj::HOLES_X));
    // Front view: 6.000 overall and 2.600 between the lugs (line to line).
    let bottom = first(refs(&front, &gf, hline(-uj::FLANGE_T, 0.0)), "the flange's bottom");
    let lug_top = first(refs(&front, &gf, hline(uj::TOP, -2.0)), "a lug's top");
    dims.push((&front, &gf, d(DimTool::Smart, &front, &gf, &[Pick::Edge(bottom), Pick::Edge(lug_top)], at(-3.2, 2.5)), "6.000", uj::HEIGHT));
    let inner = |x: f64| first(refs(&front, &gf, vline(x, 3.0)), "a lug's inner face");
    dims.push((
        &front,
        &gf,
        d(DimTool::Smart, &front, &gf, &[Pick::Edge(inner(-uj::SLOT / 2.0)), Pick::Edge(inner(uj::SLOT / 2.0))], at(0.0, 2.0)),
        "2.600",
        uj::SLOT,
    ));
    // Right view: 2.500, 43.0°, 120.0°, Ø1.750 and Ø1.250.
    let side = |y: f64| first(refs(&right, &gr, vline(y, 3.0)), "a lug's side");
    dims.push((
        &right,
        &gr,
        d(DimTool::Smart, &right, &gr, &[Pick::Edge(side(-w)), Pick::Edge(side(w))], at(0.0, uj::TOP + 0.6)),
        "2.500",
        uj::LUG_W,
    ));
    let chamfer = first(
        refs(&right, &gr, |s| {
            matches!(*s, Shape::Line { a, b } if {
                let (lo, hi) = if a[1] < b[1] { (a, b) } else { (b, a) };
                near(lo[0], -w) && near(hi[0], -w + uj::CHAMFER_RUN) && near(hi[1], uj::TOP)
            })
        }),
        "the chamfer",
    );
    let top_edge = first(refs(&right, &gr, hline(uj::TOP, 0.0)), "the lug's top");
    // The text left of the corner, a little below the top: the chamfer's 43° side.
    let corner = [-w + uj::CHAMFER_RUN, uj::TOP];
    dims.push((
        &right,
        &gr,
        d(DimTool::Smart, &right, &gr, &[Pick::Edge(chamfer), Pick::Edge(top_edge)], at(corner[0] - 1.0, corner[1] - 0.3)),
        "43.0°",
        uj::CHAMFER_ANGLE,
    ));
    // The flare: a line at 60° to the floor left of the lug (it meets the cylinder just above
    // the floor, where the Ø4.750 disc is narrower than the flare's foot).
    let foot = w + uj::FLARE_RUN;
    let flare = first(
        refs(&right, &gr, |s| {
            matches!(*s, Shape::Line { a, b } if {
                let slope = ((b[1] - a[1]) / (b[0] - a[0])).abs();
                (slope - 3f64.sqrt()).abs() < 1e-3 && a[0].max(b[0]) < -w + 1e-3 && a[1].min(b[1]) < 0.1
            })
        }),
        "the flare",
    );
    let floor = first(refs(&right, &gr, hline(0.0, -2.2)), "the flange's top");
    // The text above the flange, outside the flare: the 120° side.
    dims.push((
        &right,
        &gr,
        d(DimTool::Smart, &right, &gr, &[Pick::Edge(flare), Pick::Edge(floor)], at(-foot - 0.6, 0.25)),
        "120.0°",
        uj::FLARE_ANGLE,
    ));
    let boss = first(refs(&right, &gr, circle([0.0, uj::CROSS_Z], uj::BOSS_D / 2.0)), "the boss");
    dims.push((&right, &gr, d(DimTool::Smart, &right, &gr, &[Pick::Edge(boss)], at(-1.8, 4.9)), "Ø1.750", uj::BOSS_D));
    let cross = first(refs(&right, &gr, circle([0.0, uj::CROSS_Z], uj::CROSS_D / 2.0)), "the cross hole");
    dims.push((&right, &gr, d(DimTool::Smart, &right, &gr, &[Pick::Edge(cross)], at(1.8, 2.8)), "Ø1.250", uj::CROSS_D));

    let mut texts = Vec::new();
    for (v, g, dim, text, model) in &dims {
        let (block, m) = dimension_display(&style, v, *g, dim).expect("the dimension measures");
        assert!(m.from_model, "{text} should be read from the model's topology");
        let value = if m.kind == ValueKind::Angle { m.value } else { m.value / IN };
        assert!((value - model).abs() < 1e-6, "{text}: measured {value}, the model says {model}");
        assert_eq!(block.plain(), *text);
        texts.push(block.plain());
        // It draws (arrowheads and text) and survives a save.
        let a = Annotation::new(AnnotationKind::Dimension(dim.clone()));
        let gr = annotation_graphics(&style, v, *g, &a).expect("it draws");
        assert!(!gr.fills.is_empty() && !gr.texts.is_empty(), "{text}");
        let back: Annotation = ron::from_str(&ron::to_string(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }
    assert_eq!(texts, ["Ø4.750", "3.282", "2.061", "6.000", "2.600", "2.500", "43.0°", "120.0°", "Ø1.750", "Ø1.250"]);

    // D8.8: the hole callouts, from the Hole features' specs, with their prefixes.
    let info = hole_of(&*gt, &h3).expect("the counterbore belongs to Hole 1");
    assert_eq!(hole_text(&style, info, "4x").plain(), "4x Ø.266 THRU ⌴Ø.438 ↧.250");
    let b = uj::BOLT_D / 2.0 * std::f64::consts::FRAC_1_SQRT_2;
    let lug_hole = first(refs(&right, &gr, circle([b, uj::CROSS_Z + b], uj::HOLE_D / 2.0)), "a lug hole");
    let info = hole_of(&*gr, &lug_hole).expect("the lug hole belongs to Hole 2 or 3");
    assert_eq!(hole_text(&style, info, "8x").plain(), "8x Ø.266 THRU");
    let callout = Annotation::new(AnnotationKind::HoleCallout(HoleCallout { edge: lug_hole, text: at(2.2, 4.8), prefix: "8x".into(), last: None }));
    let g = annotation_graphics(&style, &right, &*gr, &callout).expect("the callout draws");
    assert_eq!(g.texts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["8x Ø.266 THRU"]);
}
