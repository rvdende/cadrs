//! The Variable Studio (Onshape's `VariableStudio/variable_studio.htm`): a tab of variables
//! the document's Part Studios and Assemblies name, inserted tables, availability, undo, and
//! the rebuild seeing the studio's values. Expected volumes are derived in
//! `samples::design_intent` (`body_volume`).
#![cfg(feature = "brep")]

use cadrs_core::commands::{AddElement, NewElementKind, SetFeature};
use cadrs_core::document::{Document, ElementKind, FeatureKind};
use cadrs_core::rebuild;
use cadrs_core::samples::design_intent::{self as di, body_volume};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::variable_studio::{self as vs, InsertedTable, SetVariableStudio, VariableStudio};
use cadrs_core::variables::{VariableFeature, VariableType};
use cadrs_core::{ElementId, FeatureId, History};
use cadrs_vars::studio::{self, StudioVariable};

#[track_caller]
fn rel(a: f64, b: f64, tol: f64) {
    assert!(
        ((a - b) / b).abs() <= tol,
        "got {a}, expected {b} (relative {tol})"
    );
}

struct Doc {
    d: Document,
    h: History,
    part: ElementId,
}

impl Doc {
    fn new() -> Self {
        let mut d = Document::new("Variable Studio");
        let part = d.elements[0].id;
        let mut h = History::default();
        di::build_in(&mut DocHistory(&mut d, &mut h), part).unwrap();
        Self { d, h, part }
    }

    fn add_studio(&mut self, name: &str) -> ElementId {
        let id = ElementId::new();
        self.h
            .execute(
                &mut self.d,
                &AddElement {
                    id,
                    kind: NewElementKind::VariableStudio,
                    name: Some(name.into()),
                    after: None,
                },
            )
            .unwrap();
        id
    }

    fn studio(&self, id: ElementId) -> VariableStudio {
        vs::studio_of(self.d.element(id).unwrap()).unwrap().clone()
    }

    fn set_studio(&mut self, id: ElementId, s: VariableStudio) {
        let edit = SetVariableStudio::new(&self.d, id, s, "Edit Variable Studio");
        self.h.execute(&mut self.d, &edit).unwrap();
    }

    fn add_row(&mut self, id: ElementId, name: &str, expr: &str) -> uuid::Uuid {
        let mut s = self.studio(id);
        let mut r = studio::new_row(name, &self.d.units);
        r.var.expr = expr.into();
        let row = r.id;
        s.variables.push(r);
        self.set_studio(id, s);
        row
    }

    fn set_row(&mut self, id: ElementId, row: uuid::Uuid, expr: &str) {
        let mut s = self.studio(id);
        s.row_mut(row).unwrap().var.expr = expr.into();
        self.set_studio(id, s);
    }

    fn set_part_variable(&mut self, f: FeatureId, expr: &str) {
        let el = self.d.element(self.part).unwrap();
        let FeatureKind::Variable(v) = &el.feature(f).unwrap().kind else {
            panic!("not a variable")
        };
        let v = VariableFeature {
            expr: expr.into(),
            ..v.clone()
        };
        self.h
            .execute(
                &mut self.d,
                &SetFeature {
                    element: self.part,
                    feature: f,
                    kind: FeatureKind::Variable(v),
                    label: "Variable".into(),
                },
            )
            .unwrap();
    }

    fn part_variable(&self, f: FeatureId) -> f64 {
        match &self.d.element(self.part).unwrap().feature(f).unwrap().kind {
            FeatureKind::Variable(v) => v.value,
            _ => panic!("not a variable"),
        }
    }

    fn body_volume(&self) -> f64 {
        let b = rebuild::build(&self.d.element(self.part).unwrap().active_features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        b.part(di::BODY_PART).unwrap().mass.unwrap().volume
    }
}

#[test]
fn a_part_studio_follows_a_studio_variable() {
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    let bore = d.add_row(s, "bore", "50 mm");
    // The Part Studio's own #piston_d now reads the studio's #bore.
    d.set_part_variable(di::VAR_PISTON, "#bore");
    assert_eq!(d.part_variable(di::VAR_PISTON), 50.0);
    rel(d.body_volume(), body_volume(50.0, 0.5), 1e-6);
    // The rebuild sees #bore as a Variable ahead of the list, at its exact value.
    let active = d.d.element(d.part).unwrap().active_features();
    assert_eq!(active[0].id, FeatureId(bore));
    assert_eq!(active[0].name, "#bore");
    // Changing the studio is one undo step that reaches the Part Studio.
    d.set_row(s, bore, "1.5 in + 2 mm");
    rel(d.part_variable(di::VAR_PISTON), 40.1, 1e-12);
    rel(d.body_volume(), body_volume(40.1, 0.5), 1e-6);
    assert_eq!(d.h.undo(&mut d.d).as_deref(), Some("Edit Variable Studio"));
    assert_eq!(d.part_variable(di::VAR_PISTON), 50.0);
    rel(d.body_volume(), body_volume(50.0, 0.5), 1e-6);
    d.h.redo(&mut d.d).unwrap();
    rel(d.part_variable(di::VAR_PISTON), 40.1, 1e-12);
}

#[test]
fn unused_studio_variables_stay_out_of_the_rebuild() {
    let mut d = Doc::new();
    let s = d.add_studio("Variable Studio 1");
    d.add_row(s, "unused", "3 mm");
    let el = d.d.element(d.part).unwrap();
    assert_eq!(el.studio_variables.len(), 1);
    assert!(!el.studio_variables[0].used);
    // Not named: the rebuild's list (and its cache keys) are as without a studio.
    assert!(!el.active_features().iter().any(|f| f.name == "#unused"));
    // But the Part Studio's fields can name it.
    assert!(el.variable_base().iter().any(|(n, _)| n == "unused"));
}

#[test]
fn availability_and_shadowing() {
    let mut d = Doc::new();
    let s = d.add_studio("Variable Studio 1");
    d.add_row(s, "bore", "60 mm");
    // A Part Studio's own Variable of the same name shadows the studio's.
    d.add_row(s, "piston_d", "70 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore - 20 mm");
    assert_eq!(d.part_variable(di::VAR_PISTON), 40.0);
    rel(d.body_volume(), body_volume(40.0, 0.5), 1e-6);
    // Unchecked (a draft): the variables don't reach the Part Studio, whose use fails.
    let mut st = d.studio(s);
    st.available = false;
    d.set_studio(s, st);
    let el = d.d.element(d.part).unwrap();
    assert!(el.studio_variables.is_empty());
    let b = rebuild::build(&el.active_features());
    let why = b.errors.first().map(|(_, w)| w.clone()).unwrap_or_default();
    assert!(why.contains("#bore is not defined"), "{why}");
    // Checked again, all is well.
    d.h.undo(&mut d.d).unwrap();
    rel(d.body_volume(), body_volume(40.0, 0.5), 1e-6);
}

#[test]
fn inserted_tables_live_and_frozen() {
    let mut d = Doc::new();
    let base = d.add_studio("Base");
    let w = d.add_row(base, "w", "100 mm");
    d.add_row(base, "h", "20 mm");
    let derived = d.add_studio("Derived");
    // Insert only #w of Base, followed live; Derived's own row names it.
    let mut st = d.studio(derived);
    st.inserted
        .push(InsertedTable::live(base, "Base", vec!["w".into()]));
    st.variables.push(StudioVariable {
        var: VariableFeature {
            name: "half".into(),
            var_type: VariableType::Length,
            expr: "#w / 2".into(),
            ..Default::default()
        },
        ..studio::new_row("half", &d.d.units)
    });
    d.set_studio(derived, st);
    let e = vs::evaluate(&d.d, derived).unwrap();
    assert_eq!(e.inserted[0].vars.len(), 1);
    assert_eq!(e.inserted[0].label, "Base");
    assert_eq!(e.exported.last().unwrap().var.value, 50.0);
    d.set_row(base, w, "80 mm");
    let e = vs::evaluate(&d.d, derived).unwrap();
    assert_eq!(e.exported.last().unwrap().var.value, 40.0);
    // Inserting Derived into Base would be a cycle: refused.
    let mut st = d.studio(base);
    st.inserted
        .push(InsertedTable::live(derived, "Derived", vec![]));
    assert!(vs::would_cycle(&d.d, base, derived));
    let insert = SetVariableStudio::new(&d.d, base, st, "Insert");
    assert!(d.h.execute(&mut d.d, &insert).is_err());
    // A frozen copy keeps its values whatever the source does.
    let r = cadrs_core::external::SourceRef {
        document: Some(d.d.id),
        at: cadrs_core::external::RefAt::Workspace,
        element: base,
        pinned: false,
    };
    let frozen = vs::snapshot(&d.d, r, "V1", vec![]).unwrap();
    assert_eq!(frozen.frozen.len(), 2);
    assert_eq!(frozen.label(&d.d), "Variable Studio › Base V1");
    let third = d.add_studio("Third");
    let mut st = d.studio(third);
    st.inserted.push(frozen);
    d.set_studio(third, st);
    d.set_row(base, w, "10 mm");
    let e = vs::evaluate(&d.d, third).unwrap();
    assert_eq!(e.exported[0].var.value, 80.0);
    // Deleting the live source leaves the inserting studio saying so.
    let doc_els: Vec<ElementId> = d.d.elements.iter().map(|e| e.id).collect();
    assert!(doc_els.contains(&base));
    d.d.elements.retain(|e| e.id != base);
    let e = vs::evaluate(&d.d, derived).unwrap();
    assert_eq!(e.inserted[0].error.as_deref(), Some("Base was deleted"));
    assert!(e.table.row(e.table.rows[0].id).unwrap().error.is_some());
}

#[test]
fn a_studio_survives_save_and_load() {
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    d.add_row(s, "bore", "50 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    let text = ron::to_string(&d.d).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    let el = back.element(s).unwrap();
    assert!(matches!(el.kind, ElementKind::VariableStudio(_)));
    assert_eq!(back.element(d.part).unwrap().studio_variables.len(), 1);
    let b = rebuild::build(&back.element(d.part).unwrap().active_features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
}

#[test]
fn an_assembly_variable_follows_a_studio_variable() {
    use cadrs_core::assembly::commands::AddMateFeature;
    use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind};
    let mut d = Doc::new();
    let asm = ElementId::new();
    d.h.execute(
        &mut d.d,
        &AddElement {
            id: asm,
            kind: NewElementKind::Assembly,
            name: None,
            after: None,
        },
    )
    .unwrap();
    let s = d.add_studio("Dimensions");
    let gap = d.add_row(s, "gap", "4 mm");
    // The assembly's own Variable names the studio's: #half = #gap / 2.
    let id = MateId::new();
    let var = VariableFeature {
        name: "half".into(),
        expr: "#gap / 2".into(),
        ..Default::default()
    };
    d.h.execute(
        &mut d.d,
        &AddMateFeature {
            element: asm,
            feature: MateFeature::new(id, "#half", MateKind::Variable(var)),
            poses: Vec::new(),
        },
    )
    .unwrap();
    let half = |d: &Doc| match &d
        .d
        .element(asm)
        .unwrap()
        .assembly_model()
        .unwrap()
        .mate(id)
        .unwrap()
        .kind
    {
        MateKind::Variable(v) => v.value,
        _ => unreachable!(),
    };
    assert_eq!(half(&d), 2.0);
    let el = d.d.element(asm).unwrap();
    assert!(
        el.studio_variables
            .iter()
            .any(|v| v.name == "gap" && v.used)
    );
    let base = el.variable_base();
    assert!(
        cadrs_core::assembly::vars::check(&base, el.assembly_model().unwrap(), &d.d.units)
            .is_empty()
    );
    d.set_row(s, gap, "10 mm");
    assert_eq!(half(&d), 5.0);
    d.h.undo(&mut d.d).unwrap();
    assert_eq!(half(&d), 2.0);
}

fn body_in(b: &rebuild::Build, part: cadrs_core::PartId) -> f64 {
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    b.part(part).expect("the part").mass.unwrap().volume
}

#[test]
fn a_derived_part_studio_follows_the_studio_at_once() {
    use cadrs_core::commands::AddFeature;
    use cadrs_core::derived::{self, DerivedFeature};
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    let bore = d.add_row(s, "bore", "50 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    // Part Studio 2 derives the design-intent studio, live.
    let b = ElementId::new();
    d.h.execute(
        &mut d.d,
        &AddElement {
            id: b,
            kind: NewElementKind::PartStudio,
            name: None,
            after: None,
        },
    )
    .unwrap();
    let f = FeatureId::new();
    d.h.execute(
        &mut d.d,
        &AddFeature {
            element: b,
            feature: f,
            base_name: "Derived".into(),
            kind: FeatureKind::Derived(Box::new(DerivedFeature::new(None, d.part))),
        },
    )
    .unwrap();
    let body = derived::part_id(f, di::BODY_PART);
    let build_b = |d: &Doc| rebuild::build(&d.d.element(b).unwrap().active_features());
    rel(body_in(&build_b(&d), body), body_volume(50.0, 0.5), 1e-6);
    // One studio edit: the derived copy follows in the same step, not one command later.
    d.set_row(s, bore, "40 mm");
    rel(body_in(&build_b(&d), body), body_volume(40.0, 0.5), 1e-6);
    d.h.undo(&mut d.d).unwrap();
    rel(body_in(&build_b(&d), body), body_volume(50.0, 0.5), 1e-6);
}

#[test]
fn deleting_or_renaming_a_studio_variable_fails_its_uses() {
    use cadrs_core::commands::DeleteElement;
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    let bore = d.add_row(s, "bore", "50 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    let error = |d: &Doc| {
        let b = rebuild::build(&d.d.element(d.part).unwrap().active_features());
        b.errors.first().map(|(_, w)| w.clone()).unwrap_or_default()
    };
    // Renamed: the Part Studio's #bore is no longer defined.
    let mut st = d.studio(s);
    st.row_mut(bore).unwrap().var.name = "hole".into();
    d.set_studio(s, st);
    assert!(error(&d).contains("#bore is not defined"), "{}", error(&d));
    d.h.undo(&mut d.d).unwrap();
    assert_eq!(error(&d), "");
    // The studio deleted: the same, and undo brings it all back.
    d.h.execute(&mut d.d, &DeleteElement { id: s }).unwrap();
    assert!(d.d.element(d.part).unwrap().studio_variables.is_empty());
    assert!(error(&d).contains("#bore is not defined"), "{}", error(&d));
    d.h.undo(&mut d.d).unwrap();
    assert_eq!(error(&d), "");
    rel(d.body_volume(), body_volume(50.0, 0.5), 1e-6);
}

#[test]
fn workspace_units_keep_the_values_typed_with_units() {
    use cadrs_core::commands::SetUnits;
    use cadrs_sketch::units::{LengthUnit, Units};
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    let bore = d.add_row(s, "bore", "50 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    d.h.execute(
        &mut d.d,
        &SetUnits {
            units: Units::new(LengthUnit::Inch, 3),
        },
    )
    .unwrap();
    assert_eq!(d.part_variable(di::VAR_PISTON), 50.0);
    // A bare number typed now is in inches.
    d.set_row(s, bore, "2");
    rel(d.part_variable(di::VAR_PISTON), 50.8, 1e-12);
}

#[test]
fn an_unused_variable_leaves_the_rebuild_keys_alone() {
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    d.add_row(s, "bore", "50 mm");
    let spare = d.add_row(s, "spare", "1 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    let keys = |d: &Doc| rebuild::chain_keys(&d.d.element(d.part).unwrap().active_features());
    let before = keys(&d);
    d.set_row(s, spare, "2 mm");
    assert_eq!(keys(&d), before);
}

#[test]
fn a_name_in_two_studios_is_reported_and_the_later_wins() {
    let mut d = Doc::new();
    let a = d.add_studio("A");
    d.add_row(a, "w", "10 mm");
    let b = d.add_studio("B");
    d.add_row(b, "w", "20 mm");
    let c = vs::conflicts(&d.d);
    assert_eq!(c, vec![("w".to_string(), b, vec![a])]);
    let el = d.d.element(d.part).unwrap();
    assert_eq!(el.studio_variables[0].value, 20.0);
    // A studio inserted live doesn't hand its variables on again (only through itself).
    let mut st = d.studio(b);
    st.variables.clear();
    st.inserted.push(InsertedTable::live(a, "A", vec![]));
    d.set_studio(b, st);
    assert!(vs::conflicts(&d.d).is_empty());
    let mut st = d.studio(a);
    st.available = false;
    d.set_studio(a, st);
    assert!(d.d.element(d.part).unwrap().studio_variables.is_empty());
}

#[test]
fn stale_copies_are_put_right_on_opening() {
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    d.add_row(s, "bore", "50 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    // As if the file's copy were from before the studio changed.
    let mut doc = d.d.clone();
    doc.element_mut(d.part).unwrap().studio_variables[0].value = 1.0;
    vs::propagate(&mut doc, None);
    assert_eq!(doc.element(d.part).unwrap().studio_variables[0].value, 50.0);
}

#[test]
fn undo_puts_back_the_sketches_exactly() {
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    let bore = d.add_row(s, "bore", "50 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    let sketch = |d: &Doc| {
        format!(
            "{:?}",
            d.d.element(d.part)
                .unwrap()
                .feature(di::MASTER_SKETCH)
                .unwrap()
                .sketch()
                .unwrap()
                .geometry
        )
    };
    let before = sketch(&d);
    d.set_row(s, bore, "44 mm");
    assert_ne!(sketch(&d), before);
    d.h.undo(&mut d.d).unwrap();
    assert_eq!(sketch(&d), before);
}

#[test]
fn a_change_nothing_uses_keeps_a_small_undo_step() {
    use cadrs_core::Command;
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    let spare = d.add_row(s, "spare", "1 mm");
    let mut st = d.studio(s);
    st.row_mut(spare).unwrap().var.expr = "2 mm".into();
    // No tab uses a studio variable and no name changes: only the studio is kept for undo.
    let edit = SetVariableStudio::new(&d.d, s, st.clone(), "Edit");
    assert_eq!(edit.scope(), cadrs_core::Scope::Element(s));
    // A rename may define a name some tab uses: the whole document.
    st.row_mut(spare).unwrap().var.name = "bore".into();
    let rename = SetVariableStudio::new(&d.d, s, st, "Rename");
    assert_eq!(rename.scope(), cadrs_core::Scope::Whole);
    // Once a Part Studio uses one of its variables, every change of it is.
    d.add_row(s, "bore", "50 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    let mut st = d.studio(s);
    st.row_mut(spare).unwrap().var.expr = "3 mm".into();
    assert_eq!(
        SetVariableStudio::new(&d.d, s, st, "Edit").scope(),
        cadrs_core::Scope::Whole
    );
    // An unrelated studio's edit stays small.
    let other = d.add_studio("Other");
    let x = d.add_row(other, "x", "1 mm");
    let mut st = d.studio(other);
    st.row_mut(x).unwrap().var.expr = "2 mm".into();
    assert_eq!(
        SetVariableStudio::new(&d.d, other, st, "Edit").scope(),
        cadrs_core::Scope::Element(other)
    );
}

#[test]
fn every_build_of_the_studio_sees_its_studio_variables() {
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    d.add_row(s, "bore", "50 mm");
    d.set_part_variable(di::VAR_PISTON, "#bore");
    let el = d.d.element(d.part).unwrap();
    // An assembly's instance or a drawing builds the whole list (not only what is above the
    // rollback bar): the studio's variables come first there too.
    for features in [
        el.part_features().into_owned(),
        el.features_to_build(),
        cadrs_core::drawing_source::StudioState::of(el)
            .unwrap()
            .features,
    ] {
        assert_eq!(features[0].name, "#bore");
        rel(
            body_in(&rebuild::build(&features), di::BODY_PART),
            body_volume(50.0, 0.5),
            1e-6,
        );
    }
    // The list as the user sees it doesn't hold them.
    assert!(!el.features().iter().any(|f| f.name == "#bore"));
}

#[test]
fn change_version_keeps_the_table_in_place() {
    let mut d = Doc::new();
    let a = d.add_studio("A");
    d.add_row(a, "w", "10 mm");
    let b = d.add_studio("B");
    d.add_row(b, "h", "5 mm");
    let this = d.d.id;
    let r = |e| cadrs_core::external::SourceRef {
        document: Some(this),
        at: cadrs_core::external::RefAt::Workspace,
        element: e,
        pinned: false,
    };
    let v1 = vs::snapshot(&d.d, r(a), "V1", vec![]).unwrap();
    let other = vs::snapshot(&d.d, r(b), "V1", vec![]).unwrap();
    let mut st = VariableStudio::default();
    st.inserted.push(v1.clone());
    st.inserted.push(other);
    d.add_row(a, "w2", "1 mm");
    let v2 = vs::snapshot(&d.d, r(a), "V2", vec![]).unwrap();
    assert!(st.replace_inserted(v1.id, v2));
    assert_eq!(st.inserted[0].id, v1.id);
    assert_eq!(st.inserted[0].version_name, "V2");
    assert_eq!(st.inserted[0].frozen.len(), 2);
    assert_eq!(st.inserted[1].name, "B");
    assert!(!st.replace_inserted(uuid::Uuid::new_v4(), v1));
}

/// Every rebuild of a Part Studio's list must start with the studio variables it names
/// (`active_features`, `part_features`, `features_to_build`): `features()` is the list as the
/// user sees it. Catches a new `rebuild::build(x.features())` in the core or the app, or a face
/// or plane looked up in it (the samples use no Variable Studio).
#[test]
fn no_rebuild_takes_the_bare_feature_list() {
    fn scan(dir: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "samples") {
                    scan(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                for (i, line) in text.lines().enumerate() {
                    // (And the lookups that rebuild what is above a feature: a face to sketch on,
                    // a Plane feature, the parts.)
                    let rebuilds = [
                        "rebuild::build(",
                        "parts::face_plane(",
                        "parts::plane_feature_ref(",
                        "parts::parts(",
                    ];
                    if rebuilds.iter().any(|r| line.contains(r)) && line.contains(".features()") {
                        out.push(format!("{}:{}", p.display(), i + 1));
                    }
                }
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    scan(&root.join("src"), &mut found);
    scan(&root.join("../cadrs_app/src"), &mut found);
    assert!(
        found.is_empty(),
        "rebuilds of the bare feature list: {found:?}"
    );
}

/// A face of a feature sized by a studio variable is a face to sketch on, and the sketch follows
/// it when the variable changes (presso df64's foot: Extrude 4, `#foot_height` deep, had no
/// sketch plane on its cap, as its rebuild didn't see `#foot_height`).
#[test]
fn a_sketch_on_a_face_sized_by_a_studio_variable() {
    use cadrs_core::commands::{AddExtrude, AddSketch, EditSketch, SetExtrude};
    use cadrs_core::document::{ExtrudeFeature, RegionRef};
    use cadrs_core::parts::{cap_name, face_plane};
    use cadrs_sketch::{PlaneRef, SketchOp, Vec2};
    let mut d = Doc::new();
    let s = d.add_studio("Dimensions");
    let h = d.add_row(s, "h", "10 mm");
    let el = d.part;
    // A 50 × 30 box on Top, #h high, at the end of the list.
    let sk = FeatureId::new();
    let add = AddSketch {
        element: el,
        feature: sk,
        plane: Some(PlaneRef::Top),
    };
    d.h.execute(&mut d.d, &add).unwrap();
    let points = vec![
        Vec2::new(200.0, 0.0),
        Vec2::new(250.0, 0.0),
        Vec2::new(250.0, 30.0),
        Vec2::new(200.0, 30.0),
    ];
    let edit = EditSketch {
        element: el,
        feature: sk,
        op: SketchOp::AddPolyline {
            points,
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    };
    d.h.execute(&mut d.d, &edit).unwrap();
    let g =
        &d.d.element(el)
            .unwrap()
            .feature(sk)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry;
    let regions: Vec<RegionRef> = cadrs_sketch::region::regions(g)
        .iter()
        .map(|r| RegionRef::new(sk, r))
        .collect();
    let ex = FeatureId::new();
    let add = AddExtrude {
        element: el,
        feature: ex,
        extrude: ExtrudeFeature::default(),
    };
    d.h.execute(&mut d.d, &add).unwrap();
    let set = SetExtrude {
        element: el,
        feature: ex,
        extrude: ExtrudeFeature {
            regions,
            depth: 10.0,
            depth_expr: "#h".into(),
            ..ExtrudeFeature::default()
        },
        label: "Depth".into(),
    };
    d.h.execute(&mut d.d, &set).unwrap();
    let part = d.d.element(el).unwrap();
    let built = part.part_features().into_owned();
    let top = cap_name(&built, ex, 0, true).unwrap();
    // The list as the user sees it doesn't build the extrude (#h is defined above it); as the
    // rebuild sees it, the top face is 10 mm up.
    assert!(face_plane(part.features(), ex, top).is_none());
    let plane = face_plane(&built, ex, top).unwrap();
    assert!((plane.frame().origin[2] - 10.0).abs() < 1e-9);
    let on = FeatureId::new();
    let add = AddSketch {
        element: el,
        feature: on,
        plane: Some(plane),
    };
    d.h.execute(&mut d.d, &add).unwrap();
    // #h at 25 mm: the sketch moves up with its face.
    d.set_row(s, h, "25 mm");
    let z = |d: &Doc| {
        let f = d.d.element(el).unwrap().feature(on).unwrap();
        f.sketch().unwrap().plane.unwrap().frame().origin[2]
    };
    assert!((z(&d) - 25.0).abs() < 1e-9, "{}", z(&d));
    d.h.undo(&mut d.d).unwrap();
    assert!((z(&d) - 10.0).abs() < 1e-9, "{}", z(&d));
}
