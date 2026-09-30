//! Sketch splines through the kernel: a closed spline's region extruded, an open spline closed
//! by a line, a thin wall along an open spline, and a spline as a sweep path (volumes from the
//! kernel against the spline's exact area and length).
#![cfg(feature = "occt")]

use cadrs_core::advanced::{PathRef, SweepFeature};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{BodyType, Document, ExtrudeFeature, FeatureKind, RegionRef, ThinWall};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sketch::spline::{bez_area_term, length, spans};
use cadrs_sketch::{CurveKind, PlaneRef, Sketch, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Splines");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn g(&self, f: FeatureId) -> Sketch {
        self.d.element(self.el).unwrap().feature(f).unwrap().sketch().unwrap().geometry.clone()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn extrude(&mut self, e: ExtrudeFeature) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() })
            .unwrap();
        f
    }

    fn parts(&self) -> Vec<Part> {
        let b = rebuild::build(&self.features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
    }
}

fn spline(points: &[Vec2], periodic: bool) -> SketchOp {
    SketchOp::AddSpline { points: points.to_vec(), periodic, start_tangent: None, end_tangent: None, construction: false }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

#[test]
fn a_closed_spline_extrudes_to_its_area_times_depth() {
    let mut st = Studio::new();
    let pts: Vec<Vec2> =
        [(0.0, 0.0), (30.0, -5.0), (45.0, 15.0), (30.0, 35.0), (5.0, 30.0), (-8.0, 12.0)].iter().map(|(x, y)| Vec2::new(*x, *y)).collect();
    let s = st.sketch(PlaneRef::Top, vec![spline(&pts, true)]);
    let area: f64 = spans(&pts, true, None, None).iter().map(bez_area_term).sum::<f64>().abs();
    let g = st.g(s);
    let regions = cadrs_sketch::region::regions(&g);
    assert_eq!(regions.len(), 1);
    assert!((regions[0].area() - area).abs() < 1e-9);
    st.extrude(ExtrudeFeature { regions: vec![RegionRef::new(s, &regions[0])], depth: 10.0, depth_expr: "10 mm".into(), ..Default::default() });
    let parts = st.parts();
    assert_eq!(parts.len(), 1);
    let v = volume(&parts[0]);
    assert!((v - area * 10.0).abs() / (area * 10.0) < 1e-6, "{v} vs {}", area * 10.0);
}

#[test]
fn an_open_spline_closed_by_a_line() {
    let mut st = Studio::new();
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 12.0), Vec2::new(25.0, 8.0), Vec2::new(40.0, 0.0)];
    let s = st.sketch(
        PlaneRef::Front,
        vec![
            spline(&pts, false),
            SketchOp::AddPolyline { points: vec![pts[3], pts[0]], closed: false, construction: false, label: "Add line" },
        ],
    );
    let area: f64 = spans(&pts, false, None, None).iter().map(bez_area_term).sum::<f64>() + pts[3].cross(pts[0]) / 2.0;
    let g = st.g(s);
    let regions = cadrs_sketch::region::regions(&g);
    assert_eq!(regions.len(), 1);
    st.extrude(ExtrudeFeature { regions: vec![RegionRef::new(s, &regions[0])], depth: 5.0, depth_expr: "5 mm".into(), ..Default::default() });
    let v = volume(&st.parts()[0]);
    assert!((v - area.abs() * 5.0).abs() / (area.abs() * 5.0) < 1e-6, "{v} vs {}", area.abs() * 5.0);
}

#[test]
fn a_thin_wall_along_an_open_spline() {
    // A thin wall along the spline alone: about length × thickness × depth.
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 12.0), Vec2::new(25.0, 8.0), Vec2::new(40.0, 0.0)];
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![spline(&pts, false)]);
    let len = length(&spans(&pts, false, None, None));
    st.extrude(ExtrudeFeature {
        sketches: vec![s],
        body: BodyType::Thin,
        thin: ThinWall { thickness1: 1.0, thickness1_expr: "1 mm".into(), ..Default::default() },
        depth: 20.0,
        depth_expr: "20 mm".into(),
        ..Default::default()
    });
    let v = volume(&st.parts()[0]);
    // The wall's area is t·L − t²/2·Δθ on the left (Δθ the curve's signed turning).
    let sp = spans(&pts, false, None, None);
    let t0 = cadrs_sketch::spline::bez_tangent(&sp[0], 0.0);
    let t1 = cadrs_sketch::spline::bez_tangent(&sp[sp.len() - 1], 1.0);
    let turn = t0.cross(t1).atan2(t0.dot(t1));
    let want = 20.0 * (len - 0.5 * turn);
    assert!((v - want).abs() / want < 1e-3, "{v} vs {want}");
    assert!(st.g(s).curves.values().any(|c| matches!(c.kind, CurveKind::Spline { .. })));
}

#[test]
fn a_spline_is_a_sweep_path() {
    let mut st = Studio::new();
    // The path in the Front plane (XZ), a small circle profile on the Top plane at its start.
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 20.0), Vec2::new(0.0, 40.0), Vec2::new(-5.0, 60.0)];
    let path = st.sketch(
        PlaneRef::Front,
        vec![SketchOp::AddSpline { points: pts.to_vec(), periodic: false, start_tangent: Some(Vec2::new(0.0, 60.0)), end_tangent: None, construction: false }],
    );
    let prof = st.sketch(PlaneRef::Top, vec![SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 1.0, construction: false }]);
    let g = st.g(prof);
    let regions = cadrs_sketch::region::regions(&g);
    let curve = st.g(path).curves.keys().next().unwrap();
    let f = FeatureId::new();
    st.h
        .execute(
            &mut st.d,
            &AddFeature {
                element: st.el,
                feature: f,
                base_name: "Sweep".into(),
                kind: FeatureKind::Sweep(SweepFeature {
                    regions: vec![RegionRef::new(prof, &regions[0])],
                    path: vec![PathRef::SketchCurve { sketch: path, curve }],
                    ..Default::default()
                }),
            },
        )
        .unwrap();
    let len = length(&spans(&pts, false, Some(Vec2::new(0.0, 60.0)), None));
    let v = volume(&st.parts()[0]);
    let want = std::f64::consts::PI * len;
    assert!((v - want).abs() / want < 1e-2, "{v} vs {want}");
}
