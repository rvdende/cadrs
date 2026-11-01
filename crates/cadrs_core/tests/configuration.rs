//! Configurations ([`cadrs_core::configuration`]) on the plate of
//! [`cadrs_core::samples::with_hole`]: 60 × 40, Extrude 1 (the plate) 10 mm deep, Extrude 2 a
//! Ø16 hole cut 10 mm deep from the same sketch plane.
#![cfg(feature = "brep")]

use cadrs_core::assembly::commands::InsertInstance;
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::commands::{AddElement, NewElementKind, SetFeature};
use cadrs_core::configuration::{
    CellValue, ConfigInput, ConfigValues, Configuration, ConfigureParameter, EditConfiguration,
    InputKind, InputValue, InstanceConfiguration, ParamTarget, SetActiveConfiguration,
    SetInstanceConfiguration, UnconfigureParameter,
};
use cadrs_core::document::{Document, Element, FeatureKind};
use cadrs_core::rebuild;
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::with_hole::{self as wh, HOLE, HOLE_VOLUME, PLATE, PLATE_PART};
use cadrs_core::{ElementId, History};

const AREA: f64 = 60.0 * 40.0;

#[track_caller]
fn rel(a: f64, b: f64) {
    assert!(((a - b) / b).abs() <= 1e-6, "got {a}, expected {b}");
}

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let mut d = Document::new("Configured plate");
        let el = d.elements[0].id;
        let mut h = History::default();
        wh::build_in(&mut DocHistory(&mut d, &mut h), el).unwrap();
        Self { d, h, el }
    }

    fn run(&mut self, cmd: &dyn cadrs_core::Command) {
        self.h.execute(&mut self.d, cmd).unwrap();
    }

    fn studio(&self) -> &Element {
        self.d.element(self.el).unwrap()
    }

    fn cfg(&self) -> Configuration {
        self.studio().configuration.clone()
    }

    fn edit(&mut self, f: impl FnOnce(&mut Configuration)) {
        let mut c = self.cfg();
        f(&mut c);
        self.run(&EditConfiguration {
            element: self.el,
            configuration: c,
            label: "Edit configuration".into(),
        });
    }

    fn input(&self, name: &str) -> ConfigInput {
        self.cfg().input_named(name).unwrap().clone()
    }

    fn row(&self, input: &str, row: &str) -> InputValue {
        let i = self.input(input);
        let InputKind::List { rows, .. } = &i.kind else {
            panic!("{input} is not a list")
        };
        InputValue::Row(rows.iter().find(|r| r.name == row).unwrap().id)
    }

    fn pick(&mut self, input: &str, v: InputValue) {
        let id = self.input(input).id;
        let mut values = ConfigValues::default();
        values.set(id, v);
        self.run(&SetActiveConfiguration {
            element: self.el,
            values,
        });
    }

    fn volume_of(el: &Element) -> f64 {
        let b = rebuild::build(&el.active_features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        b.part(PLATE_PART).expect("the plate").mass.unwrap().volume
    }

    fn volume(&self) -> f64 {
        Self::volume_of(self.studio())
    }

    fn depth(&self) -> (f64, String) {
        let e = self
            .studio()
            .feature(PLATE)
            .unwrap()
            .extrude()
            .unwrap()
            .clone();
        (e.depth, e.depth_expr)
    }

    fn set_depth(&mut self, expr: &str, value: f64) {
        let mut e = self
            .studio()
            .feature(PLATE)
            .unwrap()
            .extrude()
            .unwrap()
            .clone();
        e.depth = value;
        e.depth_expr = expr.into();
        self.run(&SetFeature {
            element: self.el,
            feature: PLATE,
            kind: FeatureKind::Extrude(e),
            label: "Edit Extrude 1".into(),
        });
    }

    /// A "Size" List (Thin, Thick) configuring Extrude 1's depth: Thin 10 mm, Thick 20 mm.
    fn sized() -> Self {
        let mut d = Self::new();
        d.edit(|c| c.inputs.push(ConfigInput::list("Size", &["Thin", "Thick"])));
        let size = d.input("Size").id;
        d.run(&ConfigureParameter {
            element: d.el,
            input: size,
            target: depth_target(),
        });
        d.edit(|c| {
            c.input_mut(size).unwrap().params[0].values[1] = CellValue::Expr("20 mm".into())
        });
        d
    }
}

fn depth_target() -> ParamTarget {
    ParamTarget::Field {
        feature: PLATE,
        label: "Depth".into(),
        occurrence: 0,
    }
}

#[test]
fn a_list_row_sets_its_configured_field() {
    let mut d = Doc::sized();
    // A new column starts with the value the field has, in every row.
    let p = &d.input("Size").params[0];
    assert_eq!(p.values[0], CellValue::Expr("10 mm".into()));
    // Thin is the default and active one: 10 mm.
    rel(d.volume(), AREA * 10.0 - HOLE_VOLUME);
    d.pick("Size", d.row("Size", "Thick"));
    assert_eq!(d.depth().0, 20.0);
    rel(d.volume(), AREA * 20.0 - HOLE_VOLUME);
    d.pick("Size", d.row("Size", "Thin"));
    rel(d.volume(), AREA * 10.0 - HOLE_VOLUME);
    // Undo goes back to Thick.
    d.h.undo(&mut d.d).unwrap();
    assert_eq!(d.depth().0, 20.0);
}

#[test]
fn editing_a_configured_field_edits_the_active_configuration() {
    let mut d = Doc::sized();
    d.pick("Size", d.row("Size", "Thick"));
    d.set_depth("25 mm", 25.0);
    let p = &d.input("Size").params[0];
    assert_eq!(p.values[0], CellValue::Expr("10 mm".into()));
    assert_eq!(p.values[1], CellValue::Expr("25 mm".into()));
    d.pick("Size", d.row("Size", "Thin"));
    assert_eq!(d.depth().0, 10.0);
    d.pick("Size", d.row("Size", "Thick"));
    assert_eq!(d.depth().0, 25.0);
    // Removing the column leaves the depth as it is now.
    d.run(&UnconfigureParameter {
        element: d.el,
        target: depth_target(),
    });
    assert!(d.input("Size").params.is_empty());
    assert_eq!(d.depth().0, 25.0);
}

#[test]
fn a_checkbox_suppresses_a_feature() {
    let mut d = Doc::new();
    d.edit(|c| c.inputs.push(ConfigInput::checkbox("Hole", true)));
    let hole = d.input("Hole").id;
    d.run(&ConfigureParameter {
        element: d.el,
        input: hole,
        target: ParamTarget::Suppressed { feature: HOLE },
    });
    // Unchecked: suppressed; checked: cut.
    d.edit(|c| c.input_mut(hole).unwrap().params[0].values[0] = CellValue::Bool(true));
    rel(d.volume(), AREA * 10.0 - HOLE_VOLUME);
    d.pick("Hole", InputValue::Checked(false));
    assert!(d.studio().is_suppressed(HOLE));
    rel(d.volume(), AREA * 10.0);
    d.pick("Hole", InputValue::Checked(true));
    assert!(!d.studio().is_suppressed(HOLE));
    // The checkbox is #Hole (1 or 0) in expressions too.
    let names: Vec<_> = d
        .studio()
        .variable_base()
        .into_iter()
        .map(|(n, v)| (n, v.value))
        .collect();
    assert!(names.contains(&("Hole".to_string(), 1.0)), "{names:?}");
}

#[test]
fn bad_configurations_are_refused() {
    let mut d = Doc::new();
    for bad in [
        vec![ConfigInput::list("Size", &[])],
        vec![ConfigInput::list("Size", &["A", "A"])],
        vec![ConfigInput::list("2x", &["A"])],
        vec![
            ConfigInput::checkbox("Same", false),
            ConfigInput::checkbox("Same", true),
        ],
    ] {
        let c = Configuration {
            inputs: bad,
            ..Default::default()
        };
        let r = d.h.execute(
            &mut d.d,
            &EditConfiguration {
                element: d.el,
                configuration: c,
                label: "Edit".into(),
            },
        );
        assert!(r.is_err());
    }
}

#[test]
fn assembly_instances_build_their_own_configurations() {
    let mut d = Doc::sized();
    let asm = ElementId::new();
    d.run(&AddElement {
        id: asm,
        kind: NewElementKind::Assembly,
        name: None,
        after: None,
    });
    let size = d.input("Size").id;
    let config = |row: InputValue| {
        let mut values = ConfigValues::default();
        values.set(size, row);
        InstanceConfiguration {
            studio: d.el,
            values,
        }
    };
    let (thin, thick) = (
        config(d.row("Size", "Thin")),
        config(d.row("Size", "Thick")),
    );
    let (a, b) = (InstanceId::new(), InstanceId::new());
    for (id, c) in [(a, thin.clone()), (b, thick.clone())] {
        let mut i = Instance::new(
            id,
            InstanceSource::Part {
                element: d.el,
                part: PLATE_PART,
            },
            Pose::default(),
        );
        i.configuration = Some(c);
        d.run(&InsertInstance {
            element: asm,
            instance: i,
        });
    }
    let source = |d: &Doc, id| {
        d.d.element(asm)
            .unwrap()
            .assembly_model()
            .unwrap()
            .instance(id)
            .unwrap()
            .source
            .element()
    };
    assert_eq!(source(&d, a), thin.copy_id());
    assert_eq!(source(&d, b), thick.copy_id());
    assert_eq!(d.d.configured.len(), 2);
    let vol = |d: &Doc, id| Doc::volume_of(d.d.element(source(d, id)).unwrap());
    rel(vol(&d, a), AREA * 10.0 - HOLE_VOLUME);
    rel(vol(&d, b), AREA * 20.0 - HOLE_VOLUME);
    // The studio itself still shows Thin.
    rel(d.volume(), AREA * 10.0 - HOLE_VOLUME);

    // Editing the table reaches the copy.
    d.edit(|c| c.input_mut(size).unwrap().params[0].values[1] = CellValue::Expr("30 mm".into()));
    rel(vol(&d, b), AREA * 30.0 - HOLE_VOLUME);

    // Both in Thick: they share one copy, and the Thin one is dropped.
    d.run(&SetInstanceConfiguration {
        assembly: asm,
        instances: vec![a],
        values: Some(thick.values.clone()),
    });
    assert_eq!(source(&d, a), source(&d, b));
    assert_eq!(d.d.configured.len(), 1);
    rel(vol(&d, a), AREA * 30.0 - HOLE_VOLUME);

    // Undo and redo find the copies again.
    d.h.undo(&mut d.d).unwrap();
    assert_eq!(d.d.configured.len(), 2);
    rel(vol(&d, a), AREA * 10.0 - HOLE_VOLUME);
    d.h.redo(&mut d.d).unwrap();
    assert_eq!(d.d.configured.len(), 1);

    // Unconfigured: the studio itself.
    d.run(&SetInstanceConfiguration {
        assembly: asm,
        instances: vec![a, b],
        values: None,
    });
    assert_eq!(source(&d, a), d.el);
    assert!(d.d.configured.is_empty());

    // A saved document keeps it all.
    d.run(&SetInstanceConfiguration {
        assembly: asm,
        instances: vec![b],
        values: Some(thick.values.clone()),
    });
    let text = ron::to_string(&d.d).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back, d.d);
}

#[test]
fn instances_of_a_configured_studio_take_its_configuration_and_one_numbering() {
    let mut doc = Document::new("Configured plate");
    let el = doc.elements[0].id;
    let mut h = History::default();
    let asm = wh::configured_in(&mut DocHistory(&mut doc, &mut h), el).unwrap();
    let mut d = Doc { d: doc, h, el };
    let instances = |d: &Doc| {
        d.d.element(asm)
            .unwrap()
            .assembly_model()
            .unwrap()
            .instances
            .clone()
    };
    // Thin <1> and Thick <2>: one sequence for the studio's part.
    let ii = instances(&d);
    assert_eq!(ii.iter().map(|i| i.index).collect::<Vec<_>>(), vec![1, 2]);
    // An instance inserted without a configuration takes the studio's active one (Thin).
    d.pick("Size", d.row("Size", "Thick"));
    let id = InstanceId::new();
    d.run(&InsertInstance {
        element: asm,
        instance: Instance::new(
            id,
            InstanceSource::Part {
                element: d.el,
                part: PLATE_PART,
            },
            Pose::default(),
        ),
    });
    let i = instances(&d).into_iter().find(|i| i.id == id).unwrap();
    assert_eq!(i.index, 3);
    let c = i.configuration.expect("configured");
    assert_eq!(
        c.values.get(d.input("Size").id),
        Some(&d.row("Size", "Thick"))
    );
    // It keeps it when the studio shows another.
    d.pick("Size", d.row("Size", "Thin"));
    let src = instances(&d)
        .into_iter()
        .find(|i| i.id == id)
        .unwrap()
        .source;
    rel(
        Doc::volume_of(d.d.element(src.element()).unwrap()),
        AREA * 20.0 - HOLE_VOLUME,
    );
}

#[test]
fn a_sketch_dimension_and_a_variable_feature_are_configured() {
    use cadrs_core::commands::EditSketch;
    use cadrs_core::samples::with_hole::{HOLE_SKETCH, WITH_HOLE};
    use cadrs_sketch::{Dimension, DimensionKind, SketchOp};
    let mut d = Doc::new();
    // The hole's circle, dimensioned Ø16.
    let curve = {
        let g = &d
            .studio()
            .feature(HOLE_SKETCH)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry;
        g.curves.iter().next().unwrap().0
    };
    d.run(&EditSketch {
        element: d.el,
        feature: HOLE_SKETCH,
        op: SketchOp::SetDimension {
            dimension: Dimension::new(DimensionKind::Diameter { curve }, 16.0, 0.5),
            moves: vec![],
            radii: vec![],
        },
    });
    let dimension = {
        let g = &d
            .studio()
            .feature(HOLE_SKETCH)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry;
        g.dimensions.iter().next().unwrap().0
    };
    d.edit(|c| c.inputs.push(ConfigInput::list("Size", &["Small", "Big"])));
    let size = d.input("Size").id;
    for target in [
        ParamTarget::Dimension {
            feature: HOLE_SKETCH,
            dimension,
        },
        ParamTarget::Variable { feature: WITH_HOLE },
    ] {
        d.run(&ConfigureParameter {
            element: d.el,
            input: size,
            target,
        });
    }
    // Big: Ø20 and no hole (#withHole = 0 suppresses nothing yet: bind it).
    d.edit(|c| {
        let p = &mut c.input_mut(size).unwrap().params;
        p[0].values[1] = CellValue::Expr("20 mm".into());
        p[1].values[1] = CellValue::Expr("0".into());
    });
    d.pick("Size", d.row("Size", "Big"));
    let g = d
        .studio()
        .feature(HOLE_SKETCH)
        .unwrap()
        .sketch()
        .unwrap()
        .geometry
        .clone();
    assert_eq!(g.dimensions[dimension].value, 20.0);
    // A plain value is the dimension's, not an expression.
    assert!(g.expressions.is_empty(), "{:?}", g.expressions);
    rel(
        d.volume(),
        AREA * 10.0 - std::f64::consts::PI * 100.0 * 10.0,
    );
    let FeatureKind::Variable(v) = &d.studio().feature(WITH_HOLE).unwrap().kind else {
        panic!()
    };
    assert_eq!((v.expr.as_str(), v.value), ("0", 0.0));
    d.pick("Size", d.row("Size", "Small"));
    rel(d.volume(), AREA * 10.0 - HOLE_VOLUME);
    // A cell's text stays while its field has the value it gives (the sketch keeps only the
    // number), through picking its row and others.
    d.edit(|c| {
        c.input_mut(size).unwrap().params[0].values[1] =
            CellValue::Expr("10 mm / cos(60 deg)".into())
    });
    d.pick("Size", d.row("Size", "Big"));
    d.pick("Size", d.row("Size", "Small"));
    d.pick("Size", d.row("Size", "Big"));
    let p = &d.input("Size").params[0];
    assert_eq!(p.values[0], CellValue::Expr("16 mm".into()));
    assert_eq!(p.values[1], CellValue::Expr("10 mm / cos(60 deg)".into()));
    let g = &d
        .studio()
        .feature(HOLE_SKETCH)
        .unwrap()
        .sketch()
        .unwrap()
        .geometry;
    assert!((g.dimensions[dimension].value - 20.0).abs() < 1e-9);
}
