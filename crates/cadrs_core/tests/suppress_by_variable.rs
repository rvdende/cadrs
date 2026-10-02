//! IR5.5: Dynamic suppression ▸ Suppress by variable. The plate of
//! [`cadrs_core::samples::with_hole`]: 60 × 40 × 10 = 24 000 mm³, its Ø16 through hole (Extrude 2,
//! Remove) π·8²·10 = 2 010.619 mm³, so the plate with the hole is 21 989.381 mm³.
#![cfg(feature = "occt")]

use cadrs_core::commands::{SetFeature, SetSuppressByVariable, SetSuppressed};
use cadrs_core::document::{Document, FeatureKind};
use cadrs_core::rebuild::{self, Build};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::with_hole::{self as wh, HOLE, HOLE_VOLUME, PLATE_PART, PLATE_VOLUME, WITH_HOLE};
use cadrs_core::variables::SuppressByVariable;
use cadrs_core::{DocumentMeta, ElementId, History, Store};

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
    fn new() -> Self {
        let mut d = Document::new("With hole");
        let el = d.elements[0].id;
        let mut h = History::default();
        wh::build_in(&mut DocHistory(&mut d, &mut h), el).unwrap();
        Self { d, h, el }
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.d.element(self.el).unwrap().active_features())
    }

    fn volume(&self) -> f64 {
        let b = self.build();
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        b.part(PLATE_PART).expect("the plate").mass.unwrap().volume
    }

    fn bind(&mut self, rule: Option<SuppressByVariable>) {
        self.h.execute(&mut self.d, &SetSuppressByVariable { element: self.el, feature: HOLE, rule }).unwrap();
    }

    fn set_with_hole(&mut self, expr: &str) {
        let kind = FeatureKind::Variable(wh::with_hole(expr));
        self.h.execute(&mut self.d, &SetFeature { element: self.el, feature: WITH_HOLE, kind, label: "Edit #withHole".into() }).unwrap();
    }

    fn hole_suppressed(&self) -> bool {
        self.d.element(self.el).unwrap().is_suppressed(HOLE)
    }
}

const WITH: f64 = PLATE_VOLUME - HOLE_VOLUME;

#[test]
fn the_variable_suppresses_and_unsuppresses_the_cut() {
    rel(WITH, 21_989.381, 1e-7);
    let mut d = Doc::new();
    rel(d.volume(), WITH, 1e-6);
    // Bound to #withHole = 1: still cut.
    d.bind(Some(SuppressByVariable::variable("withHole")));
    assert!(!d.hole_suppressed());
    rel(d.volume(), WITH, 1e-6);
    // #withHole = 0: Extrude 2 is suppressed (by its variable, not by Suppress), the plate whole.
    d.set_with_hole("0");
    assert!(d.hole_suppressed());
    let el = d.d.element(d.el).unwrap();
    assert!(el.suppressed().is_empty());
    assert_eq!(el.suppressed_by_variable(), vec![HOLE]);
    assert_eq!(el.all_suppressed(), vec![HOLE]);
    assert!(!el.active_features().iter().any(|f| f.id == HOLE));
    rel(d.volume(), PLATE_VOLUME, 1e-6);
    // An expression: suppressed while it is 0.
    d.set_with_hole("3 - 2");
    assert!(!d.hole_suppressed());
    rel(d.volume(), WITH, 1e-6);
    // Inverted (suppressed when true): #withHole = 1 now suppresses it.
    d.bind(Some(SuppressByVariable { invert: true, ..SuppressByVariable::variable("withHole") }));
    assert!(d.hole_suppressed());
    rel(d.volume(), PLATE_VOLUME, 1e-6);
    // Suppress still suppresses whatever the variable says.
    d.bind(Some(SuppressByVariable::variable("withHole")));
    d.h.execute(&mut d.d, &SetSuppressed { element: d.el, features: vec![HOLE], suppressed: true, label: "Suppress".into() }).unwrap();
    assert!(d.hole_suppressed());
    rel(d.volume(), PLATE_VOLUME, 1e-6);
}

#[test]
fn binding_is_one_undo_step_each_way() {
    let mut d = Doc::new();
    d.set_with_hole("0");
    rel(d.volume(), WITH, 1e-6);
    d.bind(Some(SuppressByVariable::variable("withHole")));
    assert_eq!(d.h.undo_label(), Some("Suppress by #withHole"));
    rel(d.volume(), PLATE_VOLUME, 1e-6);
    assert_eq!(d.h.undo(&mut d.d).as_deref(), Some("Suppress by #withHole"));
    assert!(d.d.element(d.el).unwrap().feature(HOLE).unwrap().suppress_by.is_none());
    rel(d.volume(), WITH, 1e-6);
    d.h.redo(&mut d.d);
    assert!(d.hole_suppressed());
    rel(d.volume(), PLATE_VOLUME, 1e-6);
    // Removing it: one step too.
    d.bind(None);
    assert_eq!(d.h.undo_label(), Some("Remove suppression variable"));
    assert!(!d.hole_suppressed());
    rel(d.volume(), WITH, 1e-6);
    d.h.undo(&mut d.d);
    assert!(d.hole_suppressed());
    // A suppression variable needs a variable.
    let r = d.h.execute(&mut d.d, &SetSuppressByVariable { element: d.el, feature: HOLE, rule: Some(SuppressByVariable { expr: "1".into(), invert: false }) });
    assert!(r.is_err());
}

#[test]
fn an_unknown_variable_fails_the_feature() {
    let mut d = Doc::new();
    d.bind(Some(SuppressByVariable::variable("nothere")));
    // Not suppressed: it fails, with why, and leaves the plate alone.
    assert!(!d.hole_suppressed());
    let b = d.build();
    assert_eq!(b.error(HOLE), Some("Suppression: #nothere is not defined"));
    rel(b.part(PLATE_PART).unwrap().mass.unwrap().volume, PLATE_VOLUME, 1e-6);
    // A variable defined only below the feature: used before it is defined.
    let el = d.d.element_mut(d.el).unwrap();
    let features = el.features_mut().unwrap();
    let v = features.remove(0);
    features.push(v);
    d.bind(Some(SuppressByVariable::variable("withHole")));
    let b = d.build();
    assert_eq!(b.error(HOLE), Some("Suppression: #withHole is used before it is defined; move its Variable above this feature"));
}

#[test]
fn the_binding_is_saved_and_old_files_load() {
    let mut d = Doc::new();
    // A document without one saves as before: no `suppress_by` anywhere.
    let before = ron::to_string(&d.d).unwrap();
    assert!(!before.contains("suppress_by"));
    d.bind(Some(SuppressByVariable { invert: true, ..SuppressByVariable::variable("withHole") }));
    d.set_with_hole("0");
    let dir = std::env::temp_dir().join(format!("cadrs-suppress-by-variable-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
    let store = Store::new(&dir);
    store.create(&d.d, &DocumentMeta::new("me", 1_000)).unwrap();
    let back = store.load(d.d.id).unwrap().document;
    let _ = std::fs::remove_dir_all(&dir);
    let rule = back.element(d.el).unwrap().feature(HOLE).unwrap().suppress_by.clone();
    assert_eq!(rule, Some(SuppressByVariable { expr: "#withHole".into(), invert: true }));
    // #withHole = 0 and inverted: built.
    assert!(!back.element(d.el).unwrap().is_suppressed(HOLE));
    // An older file (no field) loads without one.
    let text = ron::to_string(&back).unwrap();
    let old = text.replace(",suppress_by:Some((expr:\"#withHole\",invert:true))", "");
    assert_ne!(old, text);
    let old: Document = ron::from_str(&old).unwrap();
    assert!(old.element(d.el).unwrap().features().iter().all(|f| f.suppress_by.is_none()));
    assert_eq!(old.element(d.el).unwrap().features().len(), 5);
}

#[test]
fn a_suppressed_variable_defines_nothing_below() {
    use cadrs_core::variables::{self, VariableFeature, VariableType};
    use cadrs_core::{Feature, FeatureId};
    let number = |name: &str, expr: &str| {
        let mut v = VariableFeature { name: name.into(), var_type: VariableType::Number, expr: expr.into(), ..VariableFeature::default() };
        let _ = v.evaluate(&Default::default(), &Vec::new());
        FeatureKind::Variable(v)
    };
    let (a, b, c) = (FeatureId::new(), FeatureId::new(), FeatureId::new());
    // #a = 0; #b = 1, suppressed while #a is 0; #c = #b + 1, suppressed while #b is 0.
    let mut features = vec![
        Feature::new(a, "#a", number("a", "0")),
        Feature { suppress_by: Some(SuppressByVariable::variable("a")), ..Feature::new(b, "#b", number("b", "1")) },
        Feature { suppress_by: Some(SuppressByVariable::variable("b")), ..Feature::new(c, "#c", number("c", "#b + 1")) },
    ];
    // #b is suppressed, so #c's variable isn't defined: #c isn't suppressed, and fails.
    assert_eq!(variables::suppressed_by_variables(&features, &[]), vec![b]);
    let active: Vec<Feature> = features.iter().filter(|f| f.id != b).cloned().collect();
    let errors = variables::check(&active, &Default::default());
    assert!(errors.contains(&(c, "Suppression: #b is not defined".to_string())), "{errors:?}");
    // #a = 1: neither is suppressed, and #c evaluates to 2 on refresh.
    features[0].kind = number("a", "1");
    assert!(variables::suppressed_by_variables(&features, &[]).is_empty());
    variables::refresh(&mut features, &[], &Default::default());
    let FeatureKind::Variable(v) = &features[2].kind else { unreachable!() };
    assert_eq!(v.value, 2.0);
}
