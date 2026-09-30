//! P3F.4: variables and expressions (`intro-to-parametric-cad.md` P5.1–P5.3, P5.6, X2).
//! Every expected value is derived by hand in the test's comment.
#![cfg(feature = "occt")]

use cadrs_core::commands::{AddFeature, EditSketch, MoveFeatures, SetExtrude, SetFeature};
use cadrs_core::document::{Document, FeatureKind};
use cadrs_core::rebuild::{self, Build};
use cadrs_core::samples::design_intent::{self as di, body_volume, clamp_volume};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::variables::{self, VariableFeature, VariableType};
use cadrs_core::{ElementId, Feature, FeatureId, History};
use cadrs_sketch::SketchOp;

#[track_caller]
fn rel(a: f64, b: f64, tol: f64) {
    assert!(((a - b) / b).abs() <= tol, "got {a}, expected {b} (relative {tol})");
}

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn design_intent() -> Self {
        let mut d = Document::new("Design intent");
        let el = d.elements[0].id;
        let mut h = History::default();
        di::build_in(&mut DocHistory(&mut d, &mut h), el).unwrap();
        Self { d, h, el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().active_features()
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    fn volume_of(&self, b: &Build, part: cadrs_core::PartId) -> f64 {
        b.part(part).unwrap_or_else(|| panic!("no part {part:?}: {:?}", b.errors)).mass.unwrap().volume
    }

    fn variable(&self, f: FeatureId) -> VariableFeature {
        match &self.d.element(self.el).unwrap().feature(f).unwrap().kind {
            FeatureKind::Variable(v) => v.clone(),
            _ => panic!("not a variable"),
        }
    }

    fn set_variable(&mut self, f: FeatureId, expr: &str) {
        let v = VariableFeature { expr: expr.into(), ..self.variable(f) };
        self.h
            .execute(&mut self.d, &SetFeature { element: self.el, feature: f, kind: FeatureKind::Variable(v), label: "Variable".into() })
            .unwrap();
    }
}

#[test]
fn design_intent_body_follows_the_piston_diameter() {
    // ID = #piston_d + #clearance, OD = ID + 10, 100 long, three grooves 2 deep and 2 wide:
    // V = π/4·(OD² − ID²)·100 − 3·π/4·((ID + 4)² − ID²)·2.
    //   #piston_d = 40: ID 40.5, OD 50.5: π/4·910·100 − 3·π/4·340·2 = 71 471.233 − 1 602.212
    //                   = 69 869.021 mm³;
    //   #piston_d = 50: ID 50.5, OD 60.5: π/4·1110·100 − 3·π/4·420·2 = 87 179.196 − 1 979.203
    //                   = 85 199.993 mm³.
    let mut d = Doc::design_intent();
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    rel(body_volume(40.0, 0.5), 69_869.021, 1e-8);
    rel(d.volume_of(&b, di::BODY_PART), 69_869.021, 1e-6);
    rel(d.volume_of(&b, di::BODY_PART), body_volume(40.0, 0.5), 1e-6);
    // The piston Ø40 × 30 and the clamp 80 × 80 × 10 less Ø40.
    let q = std::f64::consts::FRAC_PI_4;
    rel(d.volume_of(&b, di::PISTON_PART), q * 1600.0 * 30.0, 1e-6);
    rel(d.volume_of(&b, di::CLAMP_PART), clamp_volume(40.0), 1e-6);
    // The Variable table's values.
    assert_eq!(d.variable(di::VAR_PISTON).display(&Default::default()), "40 mm");
    // P5.6: one input changes; every child follows, with no feature errors.
    d.set_variable(di::VAR_PISTON, "50 mm");
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    rel(body_volume(50.0, 0.5), 85_199.993, 1e-8);
    rel(d.volume_of(&b, di::BODY_PART), 85_199.993, 1e-6);
    rel(d.volume_of(&b, di::PISTON_PART), q * 2500.0 * 30.0, 1e-6);
    // The clamp's bore follows the piston through Use (Ø50).
    rel(d.volume_of(&b, di::CLAMP_PART), clamp_volume(50.0), 1e-6);
    // The bore dimension kept its expression and took the new value.
    let el = d.d.element(d.el).unwrap();
    let g = &el.feature(di::MASTER_SKETCH).unwrap().sketch().unwrap().geometry;
    let (dim, expr) = g.expressions[0].clone();
    assert_eq!(expr, "#piston_d + #clearance");
    assert!((g.dimensions[dim].value - 50.5).abs() < 1e-9);
    // One undo step puts it all back.
    d.h.undo(&mut d.d).unwrap();
    let b = d.build();
    rel(d.volume_of(&b, di::BODY_PART), 69_869.021, 1e-6);
}

#[test]
fn mixed_unit_variables() {
    // #clearance typed in inches: 0.02 in = 0.508 mm, so ID = 40.508.
    let mut d = Doc::design_intent();
    d.set_variable(di::VAR_CLEARANCE, "0.02 in");
    let v = d.variable(di::VAR_CLEARANCE);
    assert!((v.value - 0.508).abs() < 1e-12);
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    rel(d.volume_of(&b, di::BODY_PART), body_volume(40.0, 0.508), 1e-6);
}

#[test]
fn a_variable_used_before_its_definition_is_an_error() {
    let mut d = Doc::design_intent();
    // Move #clearance below the master sketch: the sketch and the features naming #clearance
    // above it fail with why; below it, they are fine again.
    let order: Vec<FeatureId> = d.d.element(d.el).unwrap().features().iter().map(|f| f.id).collect();
    let at = order.iter().position(|f| *f == di::BODY).unwrap();
    d.h.execute(&mut d.d, &MoveFeatures { element: d.el, features: vec![di::VAR_CLEARANCE], to: at, folder: None, label: "Move".into() }).unwrap();
    let b = d.build();
    let why = b.error(di::MASTER_SKETCH).expect("the master sketch fails");
    assert!(why.contains("#clearance is used before it is defined"), "{why}");
    assert!(b.error(di::GROOVE_SKETCH).is_none(), "the groove sketch is below #clearance now");
    // The unit-level check says the same.
    let errs = variables::check(&d.features(), &Default::default());
    assert!(errs.iter().any(|(f, w)| *f == di::MASTER_SKETCH && w.contains("before")), "{errs:?}");
    // A feature field naming an undefined variable fails its feature (P3D.1 red, with why).
    let mut e = d.d.element(d.el).unwrap().feature(di::PISTON).unwrap().extrude().unwrap().clone();
    e.depth_expr = "#stroke".into();
    d.h.execute(&mut d.d, &SetExtrude { element: d.el, feature: di::PISTON, extrude: e.clone(), label: "Depth".into() }).unwrap();
    let b = d.build();
    assert_eq!(b.error(di::PISTON), Some("Depth: #stroke is not defined"));
    // Defining it (below the piston) is a forward use; above it, the depth follows it.
    let stroke = FeatureId::new();
    d.h.execute(&mut d.d, &AddFeature::variable(d.el, stroke, VariableFeature::length("stroke", "45 mm"))).unwrap();
    let b = d.build();
    assert!(b.error(di::PISTON).unwrap().contains("used before it is defined"));
    d.h.execute(&mut d.d, &MoveFeatures { element: d.el, features: vec![stroke], to: 0, folder: None, label: "Move".into() }).unwrap();
    let b = d.build();
    assert!(b.error(di::PISTON).is_none(), "{:?}", b.errors);
    let q = std::f64::consts::FRAC_PI_4;
    rel(d.volume_of(&b, di::PISTON_PART), q * 1600.0 * 45.0, 1e-6);
}

#[test]
fn variable_types_and_uses() {
    let d = Doc::design_intent();
    let features = d.d.element(d.el).unwrap().features().to_vec();
    let master = features.iter().find(|f| f.id == di::MASTER_SKETCH).unwrap();
    assert_eq!(variables::uses(master), vec!["piston_d".to_string(), "clearance".to_string()]);
    let piston = features.iter().find(|f| f.id == di::PISTON_SKETCH).unwrap();
    assert_eq!(variables::uses(piston), vec!["piston_d".to_string()]);
    let body = features.iter().find(|f| f.id == di::BODY).unwrap();
    assert!(variables::uses(body).is_empty());
    // Types: an angle, a number, Any.
    let units = Default::default();
    let env = variables::defined(&features, &units);
    let mut a = VariableFeature { name: "tilt".into(), var_type: VariableType::Angle, expr: "asin".into(), ..Default::default() };
    assert!(a.evaluate(&units, &env).is_err());
    a.expr = "15 deg * 2".into();
    a.evaluate(&units, &env).unwrap();
    assert_eq!(a.display(&units), "30 deg");
    let mut n = VariableFeature { name: "bolts".into(), var_type: VariableType::Number, expr: "2 * 3".into(), ..Default::default() };
    n.evaluate(&units, &env).unwrap();
    assert_eq!(n.display(&units), "6");
    let mut any = VariableFeature { name: "r".into(), var_type: VariableType::Any, expr: "#piston_d / 2".into(), ..Default::default() };
    any.evaluate(&units, &env).unwrap();
    assert_eq!(any.display(&units), "20 mm");
    // Names.
    assert_eq!(VariableFeature { name: "2x".into(), ..Default::default() }.problem(), Some("A name is letters, digits and _ (starting with a letter)"));
    // A plain dimension typed over an expression drops it.
    let mut d = Doc::design_intent();
    let g = d.d.element(d.el).unwrap().feature(di::PISTON_SKETCH).unwrap().sketch().unwrap().geometry.clone();
    let dim = g.expressions[0].0;
    d.h.execute(&mut d.d, &EditSketch {
        element: d.el,
        feature: di::PISTON_SKETCH,
        op: SketchOp::Batch(vec![SketchOp::SetDimensionValue { id: dim, value: 35.0 }, SketchOp::SetDimensionExpr { id: dim, expr: None }]),
    })
    .unwrap();
    d.set_variable(di::VAR_PISTON, "44 mm");
    let g = d.d.element(d.el).unwrap().feature(di::PISTON_SKETCH).unwrap().sketch().unwrap().geometry.clone();
    assert!((g.dimensions[dim].value - 35.0).abs() < 1e-12, "the plain value stays");
}
