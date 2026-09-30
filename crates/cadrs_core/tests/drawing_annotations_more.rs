//! P3C.8: baseline, ordinate, chamfer and arc length dimensions, GD&T frames, datum, surface
//! finish and weld symbols on the bar's Top view, against hand-computed values:
//!
//! - ordinates from the chamfered end (x 0): the Ø6 hole 30, the tapped hole 170, the far end 200;
//! - baselines from the same end: 30 and 170;
//! - the chamfer dimension reads the Chamfer feature: "1.00 x 45°";
//! - the R8 round's quarter arc: ⌒ 8·π/2 = 12.566 → "⌒12.57";
//! - every new annotation adds, undoes and round-trips through RON.
#![cfg(feature = "occt")]

use cadrs_core::commands::EditDrawing;
use cadrs_core::samples::drawing_bar::{self as bar, views as bar_views};
use cadrs_core::views::ViewGeometry;
use cadrs_core::{Feature, History};
use cadrs_drawing::annotation::{Annotation, AnnotationKind, EdgeRef, Orient, Pick, PointOf, PointRef, Shape, annotation_graphics, resolve};
use cadrs_drawing::annotation_more::{
    ArcLength, Baseline, ChamferDim, Datum, FeatureControl, FinishKind, Gdt, Modifier, Ordinate, SurfaceFinish, Weld, WeldKind, arc_length,
    chamfer_text, chamfer_values, ordinate_values,
};
use cadrs_drawing::{DrawingOp, View};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

struct Top {
    doc: cadrs_core::Document,
    drawing: cadrs_core::ElementId,
    d: cadrs_drawing::Drawing,
    v: View,
    g: std::sync::Arc<ViewGeometry>,
}

fn top() -> Top {
    let (doc, studio, drawing) = bar::document().unwrap();
    let f: Vec<Feature> = doc.element(studio).unwrap().features().to_vec();
    let d = doc.element(drawing).unwrap().drawing_data().unwrap().clone();
    let v = d.view(bar_views::TOP).unwrap().1.clone();
    let g = bar_views::project(&f, &v).unwrap();
    Top { doc, drawing, d, v, g }
}

impl Top {
    fn find(&self, what: &str, f: impl Fn(&Shape) -> bool) -> EdgeRef {
        self.g
            .projection
            .edges
            .iter()
            .map(EdgeRef::of)
            .filter(|r| r.edge.is_some() || r.face.is_some())
            .map(|r| EdgeRef { shape: resolve(&self.v, &*self.g, &r).shape, ..r })
            .find(|r| f(&r.shape))
            .unwrap_or_else(|| panic!("no {what}"))
    }

    fn circle(&self, x: f64, r: f64) -> PointRef {
        let e = self.find("hole", |s| matches!(*s, Shape::Circle { center, radius, arc: None } if (center[0] - x).abs() < 1e-6 && (radius - r).abs() < 1e-6));
        PointRef { edge: e, of: PointOf::Center, hint: [x, 0.0] }
    }

    /// The straight part of an end (x), its midpoint.
    fn end(&self, x: f64) -> PointRef {
        let e = self.find("end", |s| matches!(*s, Shape::Line { a, b } if (a[0] - x).abs() < 1e-6 && (b[0] - x).abs() < 1e-6));
        PointRef { edge: e, of: PointOf::Mid, hint: [x, 0.0] }
    }
}

#[test]
fn ordinates_read_their_distances_from_the_zero_point() {
    let t = top();
    let o = Ordinate {
        origin: t.end(0.0),
        points: vec![t.circle(bar::HOLE_X, bar::HOLE_D / 2.0), t.circle(bar::TAP_X, 2.5), t.end(bar::LENGTH)],
        vertical: false,
        level: 22.0,
    };
    let v: Vec<f64> = ordinate_values(&t.v, &*t.g, &o).unwrap().into_iter().map(|x| x.unwrap()).collect();
    assert_eq!(v.len(), 3);
    close(v[0], 30.0, 1e-9);
    close(v[1], 170.0, 1e-9);
    close(v[2], 200.0, 1e-9);
    let a = Annotation::new(AnnotationKind::Ordinate(o));
    let g = annotation_graphics(&t.d.style, &t.v, &*t.g, &a).unwrap();
    let texts: Vec<&str> = g.texts.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["0.00", "30.00", "170.00", "200.00"]);
}

#[test]
fn baselines_measure_from_one_base() {
    let t = top();
    let b = Baseline {
        base: Pick::Point(t.end(0.0)),
        targets: vec![Pick::Point(t.circle(bar::HOLE_X, bar::HOLE_D / 2.0)), Pick::Point(t.circle(bar::TAP_X, 2.5))],
        orient: Orient::Horizontal,
        text: [15.0, 16.0],
        spacing: 8.0,
    };
    let g = annotation_graphics(&t.d.style, &t.v, &*t.g, &Annotation::new(AnnotationKind::Baseline(b))).unwrap();
    let texts: Vec<&str> = g.texts.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["30.00", "170.00"]);
    // Stacked 8 mm apart on paper (1:1).
    let ys: Vec<f64> = g.texts.iter().map(|x| x.pos[1]).collect();
    close(ys[1] - ys[0], 8.0, 1e-6);
}

#[test]
fn the_chamfer_dimension_reads_the_chamfer_feature() {
    let t = top();
    let e = t.find("the chamfer", |s| {
        matches!(*s, Shape::Line { a, b } if {
            let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
            (dx - bar::CHAMFER).abs() < 1e-6 && (dy - bar::CHAMFER).abs() < 1e-6 && a[0].min(b[0]).abs() < 1e-6
        })
    });
    // The edge is on the Chamfer feature's face: the values come from the feature.
    assert!(e.edge.unwrap().faces.iter().any(|f| f.op == bar::CHAMFER_1.0));
    assert!(t.g.chamfers.contains_key(&bar::CHAMFER_1.0));
    let c = ChamferDim { edge: e, text: [-12.0, 18.0] };
    let v = chamfer_values(&t.v, &*t.g, &c).unwrap();
    close(v.0, bar::CHAMFER, 1e-12);
    close(v.2, 45.0, 1e-12);
    assert_eq!(chamfer_text(&t.d.style, v), "1.00 x 45°");
    let g = annotation_graphics(&t.d.style, &t.v, &*t.g, &Annotation::new(AnnotationKind::ChamferDim(c))).unwrap();
    assert_eq!(g.texts.iter().map(|x| x.text.as_str()).collect::<String>(), "1.00 x 45°");
}

#[test]
fn the_arc_length_of_the_round_is_a_quarter_circle() {
    let t = top();
    let e = t.find("the round", |s| matches!(*s, Shape::Circle { radius, arc: Some(_), .. } if (radius - bar::FILLET_R).abs() < 1e-6));
    let a = ArcLength { edge: e, text: [bar::LENGTH + 2.0, 14.0] };
    close(arc_length(&t.v, &*t.g, &a).unwrap(), bar::FILLET_R * std::f64::consts::FRAC_PI_2, 1e-6);
    let g = annotation_graphics(&t.d.style, &t.v, &*t.g, &Annotation::new(AnnotationKind::ArcLength(a))).unwrap();
    assert_eq!(g.texts.iter().map(|x| x.text.as_str()).collect::<String>(), "12.57");
    // The ⌒ is drawn as a vector stroke before the value.
    assert!(!g.strokes.is_empty());
}

#[test]
fn every_new_annotation_adds_and_undoes() {
    let mut t = top();
    let edge = t.find("the top side", |s| matches!(*s, Shape::Line { a, b } if (a[1] - 10.0).abs() < 1e-6 && (b[1] - 10.0).abs() < 1e-6));
    let hole = t.circle(bar::HOLE_X, bar::HOLE_D / 2.0);
    let anns = vec![
        AnnotationKind::FeatureControl(FeatureControl {
            characteristic: Gdt::Position,
            tolerance: "0.05".into(),
            diameter: true,
            modifier: Some(Modifier::Mmc),
            datums: vec!["A".into(), "B".into()],
            edge: Some(hole.edge),
            text: [45.0, 20.0],
        }),
        AnnotationKind::Datum(Datum { letter: "A".into(), edge, text: [100.0, 20.0] }),
        AnnotationKind::SurfaceFinish(SurfaceFinish { kind: FinishKind::RemovalRequired, value: "Ra 1.6".into(), edge, text: [120.0, 10.0] }),
        AnnotationKind::Weld(Weld { edge, text: [140.0, 22.0], arrow_side: WeldKind::Fillet, other_side: WeldKind::None, size: "5".into(), all_around: false }),
        AnnotationKind::Ordinate(Ordinate { origin: t.end(0.0), points: vec![hole], vertical: false, level: 22.0 }),
    ];
    let mut h = History::default();
    let mut states = vec![t.doc.element(t.drawing).unwrap().drawing_data().unwrap().clone()];
    for k in anns {
        let a = Annotation::new(k);
        // Each draws.
        assert!(annotation_graphics(&t.d.style, &t.v, &*t.g, &a).is_some(), "{}", a.noun());
        h.execute(&mut t.doc, &EditDrawing { element: t.drawing, op: DrawingOp::AddAnnotation { view: bar_views::TOP, annotation: a } }).unwrap();
        states.push(t.doc.element(t.drawing).unwrap().drawing_data().unwrap().clone());
    }
    let last = states.last().unwrap().clone();
    let back: cadrs_drawing::Drawing = ron::from_str(&ron::to_string(&last).unwrap()).unwrap();
    assert_eq!(back, last);
    for k in (0..states.len() - 1).rev() {
        assert!(h.undo(&mut t.doc).is_some());
        assert_eq!(t.doc.element(t.drawing).unwrap().drawing_data().unwrap(), &states[k]);
    }
}
