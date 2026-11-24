//! The engine of the merged configurations: inputs and columns as variables, parameters
//! reading them by `#name`. (The fork's behaviour, end to end, is `tests/configuration.rs`.)

use super::*;
use crate::assembly::{Instance, InstanceSource, Pose};
use crate::command::History;
use crate::document::ExtrudeFeature;

/// A document with a Part Studio holding one extrude (no regions: nothing builds, the
/// expressions are what is checked) and an empty Assembly.
struct T {
    d: Document,
    h: History,
    ps: ElementId,
    asm: ElementId,
    ext: FeatureId,
}

impl T {
    fn new() -> Self {
        let mut d = Document::new("Configurations");
        let ps = d.elements[0].id;
        let asm = d.elements[1].id;
        let ext = FeatureId::new();
        d.element_mut(ps)
            .unwrap()
            .features_mut()
            .unwrap()
            .push(Feature::new(
                ext,
                "Extrude 1",
                FeatureKind::Extrude(ExtrudeFeature::default()),
            ));
        Self {
            d,
            h: History::default(),
            ps,
            asm,
            ext,
        }
    }

    fn run(&mut self, c: &dyn Command) {
        self.h.execute(&mut self.d, c).unwrap();
    }

    fn el(&self) -> &Element {
        self.d.element(self.ps).unwrap()
    }

    fn config(&self) -> &Configuration {
        &self.el().configuration
    }

    fn extrude(&self) -> &ExtrudeFeature {
        self.el().feature(self.ext).unwrap().extrude().unwrap()
    }

    fn depth(&self) -> ParamTarget {
        ParamTarget::Field {
            feature: self.ext,
            label: "Depth".into(),
            occurrence: 0,
        }
    }

    /// A List "Size" (Short, Long) whose column configures the extrude's depth: 30 mm, 50 mm.
    fn with_size(&mut self) -> (ConfigInputId, ConfigRowId, ConfigRowId) {
        let size = ConfigInput::list("Size", &["Short", "Long"]);
        let InputKind::List { rows, .. } = &size.kind else {
            unreachable!()
        };
        let (id, short, long) = (size.id, rows[0].id, rows[1].id);
        let mut c = self.config().clone();
        c.inputs.push(size);
        self.run(&EditConfiguration {
            element: self.ps,
            configuration: c,
            label: "Add input".into(),
        });
        self.run(&ConfigureParameter {
            element: self.ps,
            input: id,
            target: self.depth(),
            name: Some("length".into()),
        });
        let mut c = self.config().clone();
        c.input_mut(id).unwrap().params[0].values = vec![
            CellValue::Expr("30 mm".into()),
            CellValue::Expr("50 mm".into()),
        ];
        self.run(&EditConfiguration {
            element: self.ps,
            configuration: c,
            label: "Edit table".into(),
        });
        (id, short, long)
    }

    fn pick(&mut self, input: ConfigInputId, value: InputValue) {
        let mut values = ConfigValues::default();
        values.set(input, value);
        self.run(&SetActiveConfiguration {
            element: self.ps,
            values,
        });
    }
}

/// A List's column is a variable: the configured depth reads `#length`, and picking a row
/// changes its value, never the expression (the features don't change with the
/// configuration).
#[test]
fn a_column_is_a_variable_the_parameter_reads() {
    let mut t = T::new();
    let (size, short, long) = t.with_size();
    assert_eq!(t.extrude().depth_expr, "#length");
    // The table starts as the depth was, then is edited: the default row (Short) shows.
    assert_eq!(t.extrude().depth, 30.0);
    let before = t.el().features().to_vec();
    t.pick(size, InputValue::Row(long));
    assert_eq!(t.extrude().depth, 50.0);
    assert_eq!(t.extrude().depth_expr, "#length");
    // Only the value moved: the stored features are the same but for it.
    let mut now = t.el().features().to_vec();
    if let FeatureKind::Extrude(e) = &mut now[0].kind {
        e.depth = 30.0;
    }
    assert_eq!(now, before);
    t.pick(size, InputValue::Row(short));
    assert_eq!(t.extrude().depth, 30.0);
    // The rebuild sees the column as a Variable ahead of the list, under an id of its own
    // that is the same in every configuration.
    let fs = t.el().active_features();
    let FeatureKind::Variable(v) = &fs[0].kind else {
        panic!("{:?}", fs[0].kind)
    };
    assert_eq!((v.name.as_str(), v.value), ("length", 30.0));
    let id = fs[0].id;
    t.pick(size, InputValue::Row(long));
    assert_eq!(t.el().active_features()[0].id, id);
    assert!(crate::variables::check(&t.el().active_features(), &t.d.units).is_empty());
}

/// A Value input is `#name` itself: wired into a field, set per configuration.
#[test]
fn a_value_input_is_typed_per_configuration() {
    let mut t = T::new();
    let len = ConfigInput::value("len", ValueKind::Length, 12.0);
    let id = len.id;
    let mut c = t.config().clone();
    c.inputs.push(len);
    t.run(&EditConfiguration {
        element: t.ps,
        configuration: c,
        label: "Add input".into(),
    });
    t.run(&ConnectParameter {
        element: t.ps,
        feature: t.ext,
        slot: "Depth".into(),
        expr: "#len * 2".into(),
    });
    assert_eq!(t.extrude().depth, 24.0);
    t.pick(id, InputValue::Number(7.5));
    assert_eq!(t.extrude().depth, 15.0);
    assert_eq!(t.config().describe(&t.config().active), "len 7.5 mm");
    // A Value has no table.
    assert!(
        t.h.execute(
            &mut t.d,
            &ConfigureParameter {
                element: t.ps,
                input: id,
                target: ParamTarget::Free {
                    kind: ColumnKind::Length
                },
                name: None,
            }
        )
        .is_err()
    );
}

/// A Checkbox is `#name` (1 or 0): wired into a feature's Bypass in the node view, it
/// suppresses it while unchecked; configured with Suppress, its column does.
#[test]
fn a_checkbox_suppresses_through_its_variable() {
    let mut t = T::new();
    let hole = ConfigInput::checkbox("hole", true);
    let id = hole.id;
    let mut c = t.config().clone();
    c.inputs.push(hole);
    t.run(&EditConfiguration {
        element: t.ps,
        configuration: c,
        label: "Add input".into(),
    });
    t.run(&crate::commands::SetSuppressByVariable {
        element: t.ps,
        feature: t.ext,
        rule: Some(crate::variables::SuppressByVariable::variable("hole")),
    });
    let off = |t: &T| t.el().suppressed_by_variable().contains(&t.ext);
    assert!(!off(&t));
    t.pick(id, InputValue::Checked(false));
    assert!(off(&t));
    // Configured instead: its column (suppressed when checked, here) reads the same way.
    t.run(&crate::commands::SetSuppressByVariable {
        element: t.ps,
        feature: t.ext,
        rule: None,
    });
    t.run(&ConfigureParameter {
        element: t.ps,
        input: id,
        target: ParamTarget::Suppressed { feature: t.ext },
        name: None,
    });
    let mut c = t.config().clone();
    c.input_mut(id).unwrap().params[0].values = vec![CellValue::Bool(false), CellValue::Bool(true)];
    t.run(&EditConfiguration {
        element: t.ps,
        configuration: c,
        label: "Edit table".into(),
    });
    assert!(!off(&t));
    t.pick(id, InputValue::Checked(true));
    assert!(off(&t));
    // Suppressed from the list while checked: that is this configuration's cell (already
    // true), and the feature still follows its column.
    t.pick(id, InputValue::Checked(false));
    t.run(&crate::commands::SetSuppressed {
        element: t.ps,
        features: vec![t.ext],
        suppressed: true,
        label: "Suppress".into(),
    });
    assert_eq!(
        t.config().input(id).unwrap().params[0].values[0],
        CellValue::Bool(true)
    );
    assert!(!t.el().suppressed().contains(&t.ext));
    assert!(off(&t));
}

/// Renaming a column renames it wherever it is read; removing one leaves its parameter (and
/// anything else reading it) the value it shows.
#[test]
fn renaming_and_removing_columns() {
    let mut t = T::new();
    let (size, _, long) = t.with_size();
    t.pick(size, InputValue::Row(long));
    t.run(&RenameConfigName {
        element: t.ps,
        from: "length".into(),
        to: "depth".into(),
    });
    assert_eq!(t.extrude().depth_expr, "#depth");
    assert_eq!(t.config().input(size).unwrap().params[0].name, "depth");
    assert_eq!(t.extrude().depth, 50.0);
    let target = t.depth();
    t.run(&UnconfigureParameter {
        element: t.ps,
        target,
    });
    assert!(!t.extrude().depth_expr.contains('#'));
    assert_eq!(t.extrude().depth, 50.0);
    assert!(t.config().input(size).unwrap().params.is_empty());
}

/// A column's or input's name is a variable's: one a Variable or another column has is
/// refused.
#[test]
fn names_are_checked() {
    let mut t = T::new();
    let (size, ..) = t.with_size();
    let mut c = t.config().clone();
    c.inputs
        .push(ConfigInput::value("length", ValueKind::Length, 1.0));
    assert!(
        t.h.execute(
            &mut t.d,
            &EditConfiguration {
                element: t.ps,
                configuration: c,
                label: "Add input".into(),
            }
        )
        .is_err()
    );
    // A Variable named like a column.
    let var = FeatureId::new();
    t.d.element_mut(t.ps)
        .unwrap()
        .features_mut()
        .unwrap()
        .insert(
            0,
            Feature::new(
                var,
                "Variable 1",
                FeatureKind::Variable(VariableFeature {
                    name: "width".into(),
                    ..Default::default()
                }),
            ),
        );
    assert!(
        t.h.execute(
            &mut t.d,
            &ConfigureParameter {
                element: t.ps,
                input: size,
                target: ParamTarget::Free {
                    kind: ColumnKind::Length
                },
                name: Some("width".into()),
            }
        )
        .is_err()
    );
    // Without a name given, one is made that is free.
    assert_eq!(
        t.config().fresh_name("width", &taken_names(t.el())),
        "width_2"
    );
}

/// A document from the fork (a column with no name, its value written into its parameter)
/// is converted on load: the column is named after the parameter, which reads it, with the
/// value it had.
#[test]
fn the_forks_documents_are_converted() {
    let mut t = T::new();
    let mut size = ConfigInput::list("Size", &["Thin", "Thick"]);
    size.params.push(ConfiguredParam {
        target: t.depth(),
        values: vec![
            CellValue::Expr("10 mm".into()),
            CellValue::Expr("20 mm".into()),
        ],
        name: String::new(),
    });
    let InputKind::List { rows, .. } = &size.kind else {
        unreachable!()
    };
    let (id, thick) = (size.id, rows[1].id);
    let el = t.d.element_mut(t.ps).unwrap();
    el.configuration.inputs.push(size);
    el.configuration.active.set(id, InputValue::Row(thick));
    // The fork wrote the shown row's value into the field.
    if let Some(FeatureKind::Extrude(e)) = el.features_mut().map(|f| &mut f[0].kind) {
        e.depth = 20.0;
        e.depth_expr = "20 mm".into();
    }
    // Through RON, as a file is read.
    let text = ron::to_string(&t.d).unwrap();
    assert!(!text.contains("Extrude_1_Depth"));
    t.d = ron::from_str(&text).unwrap();
    refresh_copies(&mut t.d);
    crate::commands::refresh_studio(&mut t.d, t.ps);
    let p = &t.config().input(id).unwrap().params[0];
    assert_eq!(p.name, "Extrude_1_Depth");
    assert_eq!(t.extrude().depth_expr, "#Extrude_1_Depth");
    assert_eq!(t.extrude().depth, 20.0);
    // And it works from there on.
    let thin = match &t.config().input(id).unwrap().kind {
        InputKind::List { rows, .. } => rows[0].id,
        _ => unreachable!(),
    };
    t.pick(id, InputValue::Row(thin));
    assert_eq!(t.extrude().depth, 10.0);
}

/// An assembly instance in a configuration is a copy of the studio in its values; the copy
/// isn't made again when the studio only shows another configuration or its nodes move, and is
/// when its model changes.
#[test]
fn copies_follow_the_model_not_the_view() {
    let mut t = T::new();
    let (size, short, long) = t.with_size();
    let mut values = ConfigValues::default();
    values.set(size, InputValue::Row(long));
    let c = InstanceConfiguration {
        studio: t.ps,
        values: t.config().complete(&values),
    };
    let mut i = Instance::new(
        InstanceId::new(),
        InstanceSource::Part {
            element: t.ps,
            part: crate::ids::PartId::new(t.ext, 0),
        },
        Pose::default(),
    );
    i.configuration = Some(c.clone());
    t.run(&crate::assembly::commands::InsertInstance {
        element: t.asm,
        instance: i,
    });
    let copy = |t: &T| {
        let c =
            t.d.configured
                .iter()
                .find(|x| x.element.id == c.copy_id())
                .unwrap();
        (
            c.stamp,
            c.element.feature(t.ext).unwrap().extrude().unwrap().depth,
        )
    };
    let (stamp, depth) = copy(&t);
    assert_eq!(depth, 50.0);
    // Shown in Short, nodes moved: the same copy.
    t.pick(size, InputValue::Row(short));
    t.run(&MoveGraphNodes {
        element: t.ps,
        positions: vec![(t.ext, [10.0, 20.0])],
        inputs_node: None,
        parts_node: None,
    });
    assert_eq!(copy(&t).0, stamp);
    // The table changed: made again.
    let mut cfg = t.config().clone();
    cfg.input_mut(size).unwrap().params[0].values[1] = CellValue::Expr("60 mm".into());
    t.run(&EditConfiguration {
        element: t.ps,
        configuration: cfg,
        label: "Edit table".into(),
    });
    assert_ne!(copy(&t).0, stamp);
    assert_eq!(copy(&t).1, 60.0);
}

/// A sketch's dimension driven by a column or input follows the configuration (in the studio
/// and in a copy), and keeps its value when unwired.
#[test]
fn an_input_drives_a_sketch_dimension() {
    use cadrs_sketch::{Dimension, DimensionKind, Vec2};
    let mut t = T::new();
    let (size, short, long) = t.with_size();
    let sk = FeatureId::new();
    let mut g = cadrs_sketch::Sketch::default();
    let a = g.add_point(Vec2::ZERO);
    let b = g.add_point(Vec2::new(10.0, 0.0));
    let dim = g.dimensions.insert(Dimension {
        kind: DimensionKind::Horizontal { a, b },
        value: 10.0,
        offset: 5.0,
        along: 0.0,
        driven: false,
    });
    t.d.element_mut(t.ps)
        .unwrap()
        .features_mut()
        .unwrap()
        .insert(
            0,
            Feature::new(
                sk,
                "Sketch 1",
                FeatureKind::Sketch(crate::document::SketchFeature {
                    geometry: g,
                    ..Default::default()
                }),
            ),
        );
    let slots = |t: &T| dimension_slots(t.el().feature(sk).unwrap());
    assert_eq!(slots(&t)[0].label, "D1");
    assert!(!slots(&t)[0].is_driven());
    t.run(&ConnectDimension {
        element: t.ps,
        feature: sk,
        dimension: dim,
        expr: "#length".into(),
    });
    let s = slots(&t);
    assert!(s[0].is_driven());
    assert_eq!(s[0].value, 30.0);
    t.pick(size, InputValue::Row(long));
    assert_eq!(slots(&t)[0].value, 50.0);
    let mut values = ConfigValues::default();
    values.set(size, InputValue::Row(short));
    let copy = generate(t.el(), &values, ElementId::new(), &t.d.units);
    let FeatureKind::Sketch(cs) = &copy.feature(sk).unwrap().kind else {
        panic!()
    };
    assert_eq!(cs.geometry.dimensions[dim].value, 30.0);
    t.run(&DisconnectDimension {
        element: t.ps,
        feature: sk,
        dimension: dim,
    });
    let s = slots(&t);
    assert!(!s[0].is_driven());
    assert_eq!(s[0].value, 50.0);
}

/// Every command undoes and redoes to the same document.
#[test]
fn undo_and_redo() {
    let mut t = T::new();
    let (size, _, long) = t.with_size();
    let before = t.d.clone();
    t.pick(size, InputValue::Row(long));
    t.run(&RenameConfigName {
        element: t.ps,
        from: "length".into(),
        to: "depth".into(),
    });
    let after = t.d.clone();
    t.h.undo(&mut t.d);
    t.h.undo(&mut t.d);
    assert_eq!(t.d.element(t.ps), before.element(t.ps));
    t.h.redo(&mut t.d);
    t.h.redo(&mut t.d);
    assert_eq!(t.d.element(t.ps), after.element(t.ps));
}

/// Saved and read back as it was; a document without a configuration reads as before.
#[test]
fn saved_and_loaded() {
    let mut t = T::new();
    t.with_size();
    let text = ron::to_string(&t.d).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back.element(t.ps).unwrap().configuration, *t.config());
    let plain = Document::new("Plain");
    let text = ron::to_string(&plain).unwrap();
    assert!(!text.contains("configuration"));
    let back: Document = ron::from_str(&text).unwrap();
    assert!(back.elements[0].configuration.is_empty());
}
