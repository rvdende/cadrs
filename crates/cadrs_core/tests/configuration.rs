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
    // An assembly's instance builds without it too (whatever the rollback bar).
    let built = d.studio().assembly_features();
    assert!(!built.iter().any(|f| f.id == HOLE));
    let b = rebuild::build(&built);
    rel(
        b.part(PLATE_PART).unwrap().mass.unwrap().volume,
        AREA * 10.0,
    );
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

#[test]
fn the_bom_lists_each_configuration_with_its_own_properties() {
    use cadrs_core::assembly::bom::{self, BomColumn, BomOptions};
    use cadrs_core::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};
    let mut doc = Document::new("Configured plate");
    let el = doc.elements[0].id;
    let mut h = History::default();
    let asm = wh::configured_in(&mut DocHistory(&mut doc, &mut h), el).unwrap();
    let mut d = Doc { d: doc, h, el };
    // A second Thick plate: Thin × 1, Thick × 2.
    let mut i = Instance::new(
        InstanceId::new(),
        InstanceSource::Part {
            element: d.el,
            part: PLATE_PART,
        },
        Pose::default(),
    );
    let mut values = ConfigValues::default();
    values.set(d.input("Size").id, d.row("Size", "Thick"));
    i.configuration = Some(InstanceConfiguration {
        studio: d.el,
        values,
    });
    d.run(&InsertInstance {
        element: asm,
        instance: i,
    });
    let compute = |d: &Doc| {
        bom::compute(
            &d.d,
            asm,
            &BomOptions::default(),
            &Default::default(),
            |e| Some(rebuild::build(&d.d.element(e)?.assembly_features())),
        )
        .unwrap()
    };
    let b = compute(&d);
    let col = |k| {
        b.columns
            .iter()
            .position(|c| *c == BomColumn::Property(k))
            .unwrap()
    };
    let (cfg, pn) = (
        col(PropertyKey::Configuration),
        col(PropertyKey::PartNumber),
    );
    let rows: Vec<(String, u32)> = b
        .rows
        .iter()
        .filter(|r| !r.top_level || r.depth > 0 || r.key.owner.element() != asm)
        .filter(|r| matches!(r.key.owner, PropertyOwner::Part { .. }))
        .map(|r| (r.cells[cfg].clone(), r.quantity))
        .collect();
    assert_eq!(
        rows,
        vec![("Thin, Hole".into(), 1), ("Thick, Hole".into(), 2)]
    );
    // A Part number set on the Thick row is Thick's alone; the studio's part keeps none.
    let thick = b
        .rows
        .iter()
        .find(|r| r.cells[cfg] == "Thick, Hole")
        .unwrap()
        .key
        .owner;
    d.run(&SetProperties {
        owners: vec![thick],
        values: vec![
            (PropertyKey::PartNumber, PropertyValue::Text("PL-20".into())),
            (PropertyKey::Name, PropertyValue::Text("Plate 20".into())),
        ],
        label: "Edit Part number".into(),
    });
    let b = compute(&d);
    let name = b
        .columns
        .iter()
        .position(|c| *c == BomColumn::Property(PropertyKey::Name))
        .unwrap();
    let cells: Vec<(String, String, String)> = b
        .rows
        .iter()
        .filter(|r| matches!(r.key.owner, PropertyOwner::Part { .. }))
        .map(|r| {
            (
                r.cells[cfg].clone(),
                r.cells[pn].clone(),
                r.cells[name].clone(),
            )
        })
        .collect();
    assert_eq!(cells[0].1, "", "{cells:?}");
    assert_eq!(cells[0].2, "Part 1", "{cells:?}");
    assert_eq!(
        cells[1],
        ("Thick, Hole".into(), "PL-20".into(), "Plate 20".into())
    );
    let studio_part = PropertyOwner::Part {
        element: d.el,
        part: PLATE_PART,
    };
    assert_eq!(
        cadrs_core::properties::text(&d.d, studio_part, PropertyKey::PartNumber, None),
        ""
    );
    // Kept when the studio changes (its copies are made again).
    d.pick("Size", d.row("Size", "Thick"));
    let b = compute(&d);
    assert!(
        b.rows
            .iter()
            .any(|r| r.cells.get(pn).is_some_and(|c| c == "PL-20"))
    );
}

impl Doc {
    /// The configuration "Size = `row`" of the plate.
    fn sized_as(&self, row: &str) -> InstanceConfiguration {
        let mut values = ConfigValues::default();
        values.set(self.input("Size").id, self.row("Size", row));
        InstanceConfiguration {
            studio: self.el,
            values: self.cfg().complete(&values),
        }
    }
}

#[test]
fn a_drawing_view_shows_its_own_configuration() {
    use cadrs_core::commands::{EditDrawing, InsertElement};
    use cadrs_core::configuration::WithConfiguration;
    use cadrs_core::drawing_source::{StudioState, live_source};
    use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Scale, View, template};

    let mut d = Doc::sized();
    let t = template::builtin("ANSI_A_INCH.dwt").unwrap();
    let el = Element::drawing("Drawing 1", Drawing::from_template(&t, None));
    let drawing = el.id;
    d.run(&InsertElement {
        element: el,
        after: Some(d.el),
        label: "Create Drawing".into(),
    });
    let thick = d.sized_as("Thick");
    let copy = thick.copy_id();
    // The view references the studio's copy in Thick; the drawing records its state.
    let view = View::base(
        ObjectRef {
            element: copy.0,
            part: Some((PLATE_PART.feature.0, PLATE_PART.index)),
        },
        NamedView::Front,
        Scale::new(1, 1),
        [100.0, 100.0],
    );
    let src = live_source(
        &cadrs_core::configuration::with_copy(&d.d, &thick).unwrap(),
        copy,
    )
    .unwrap();
    let sheet = match &d.d.element(drawing).unwrap().kind {
        cadrs_core::ElementKind::Drawing(x) => x.sheets[0].id,
        _ => unreachable!(),
    };
    d.run(&WithConfiguration {
        element: drawing,
        configuration: thick.clone(),
        command: EditDrawing {
            element: drawing,
            op: DrawingOp::Batch {
                ops: vec![
                    DrawingOp::SetSource(src),
                    DrawingOp::InsertView {
                        sheet,
                        view: view.clone(),
                    },
                ],
                label: "Insert Front view".into(),
            },
        },
    });
    let drawing_of = |d: &Doc| match &d.d.element(drawing).unwrap().kind {
        cadrs_core::ElementKind::Drawing(x) => (**x).clone(),
        _ => unreachable!(),
    };
    assert_eq!(
        d.d.element(drawing).unwrap().configured_sources,
        vec![thick.clone()]
    );
    rel(
        Doc::volume_of(d.d.element(copy).unwrap()),
        AREA * 20.0 - HOLE_VOLUME,
    );
    let depth = |snapshot: &str| {
        StudioState::parse(snapshot)
            .unwrap()
            .features
            .iter()
            .find(|f| f.id == PLATE)
            .unwrap()
            .extrude()
            .unwrap()
            .depth
    };
    rel(
        depth(&drawing_of(&d).source(copy.0).unwrap().snapshot),
        20.0,
    );
    // The studio itself still shows Thin, and isn't changed by the drawing.
    rel(d.volume(), AREA * 10.0 - HOLE_VOLUME);

    // The copy follows the studio: editing Thick's depth makes the view out of date.
    let size = d.input("Size").id;
    d.edit(|c| c.input_mut(size).unwrap().params[0].values[1] = CellValue::Expr("25 mm".into()));
    rel(
        Doc::volume_of(d.d.element(copy).unwrap()),
        AREA * 25.0 - HOLE_VOLUME,
    );
    let stale = cadrs_core::drawing_source::out_of_date(&d.d, &drawing_of(&d));
    assert_eq!(stale, vec![view.id]);

    // Undo and redo find the copy again; deleting the view lets it go.
    d.h.undo(&mut d.d).unwrap();
    d.h.undo(&mut d.d).unwrap();
    assert!(d.d.configured.is_empty());
    d.h.redo(&mut d.d).unwrap();
    assert_eq!(d.d.configured.len(), 1);
    d.run(&EditDrawing {
        element: drawing,
        op: DrawingOp::DeleteViews { ids: vec![view.id] },
    });
    // (The sheet's title block still reads the part in Thick.)
    let s = drawing_of(&d).sheets[0].clone();
    assert_eq!(s.reference.map(|r| r.element), Some(copy.0));
    assert_eq!(d.d.configured.len(), 1);
    let mut props = s.props();
    props.reference = None;
    d.run(&EditDrawing {
        element: drawing,
        op: DrawingOp::SetSheetProps { id: s.id, props },
    });
    assert!(d.d.element(drawing).unwrap().configured_sources.is_empty());
    assert!(d.d.configured.is_empty());
}

#[test]
fn a_render_studio_renders_its_own_configuration() {
    use cadrs_core::configuration::WithConfiguration;
    use cadrs_core::render::{SetRenderStudio, source_parts};

    let mut d = Doc::sized();
    let render = ElementId::new();
    d.run(&AddElement {
        id: render,
        kind: NewElementKind::RenderStudio(Some(d.el)),
        name: None,
        after: None,
    });
    let studio_of = |d: &Doc| cadrs_core::render::studio(&d.d, render).unwrap().clone();
    let thick = d.sized_as("Thick");
    let mut r = studio_of(&d);
    r.source = Some(thick.copy_id());
    d.run(&WithConfiguration {
        element: render,
        configuration: thick.clone(),
        command: SetRenderStudio {
            element: render,
            studio: r,
            label: "Render in Thick".into(),
        },
    });
    let volume = |d: &Doc| {
        let source = studio_of(d).source.unwrap();
        let (parts, _, _) = source_parts(&d.d, source, |e| {
            Some(rebuild::build(&d.d.element(e)?.active_features()))
        })
        .unwrap();
        parts
            .iter()
            .find(|p| p.id == PLATE_PART)
            .unwrap()
            .mass
            .unwrap()
            .volume
    };
    rel(volume(&d), AREA * 20.0 - HOLE_VOLUME);
    // Back to the studio itself: the copy goes.
    let mut r = studio_of(&d);
    r.source = Some(d.el);
    d.run(&SetRenderStudio {
        element: render,
        studio: r,
        label: "Render Part Studio 1".into(),
    });
    rel(volume(&d), AREA * 10.0 - HOLE_VOLUME);
    assert!(d.d.element(render).unwrap().configured_sources.is_empty());
    assert!(d.d.configured.is_empty());
}

#[test]
fn a_derived_feature_derives_its_own_configuration() {
    use cadrs_core::derived::{AddDerived, DerivedFeature};
    use cadrs_core::document::FeatureKind;

    let mut d = Doc::sized();
    let host = ElementId::new();
    d.run(&AddElement {
        id: host,
        kind: NewElementKind::PartStudio,
        name: None,
        after: None,
    });
    let thick = d.sized_as("Thick");
    let mut derived = DerivedFeature::new(None, d.el);
    derived.configuration = Some(thick.values.clone());
    let feature = cadrs_core::FeatureId::new();
    d.run(&AddDerived {
        element: host,
        feature,
        derived,
        links: Vec::new(),
    });
    let volume = |d: &Doc| -> f64 {
        let b = rebuild::build(&d.d.element(host).unwrap().active_features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        b.parts
            .iter()
            .filter_map(|p| p.mass)
            .map(|m| m.volume)
            .sum()
    };
    rel(volume(&d), AREA * 20.0 - HOLE_VOLUME);
    // It follows the studio's table, not the configuration the studio shows.
    let size = d.input("Size").id;
    d.edit(|c| c.input_mut(size).unwrap().params[0].values[1] = CellValue::Expr("30 mm".into()));
    rel(volume(&d), AREA * 30.0 - HOLE_VOLUME);
    d.pick("Size", d.row("Size", "Thick"));
    d.pick("Size", d.row("Size", "Thin"));
    rel(volume(&d), AREA * 30.0 - HOLE_VOLUME);
    // Without one: the configuration the studio shows.
    let mut f = match &d.d.element(host).unwrap().feature(feature).unwrap().kind {
        FeatureKind::Derived(x) => (**x).clone(),
        _ => unreachable!(),
    };
    f.configuration = None;
    d.run(&cadrs_core::derived::SetDerived {
        element: host,
        feature,
        derived: f,
        links: Vec::new(),
        label: "Derive".into(),
    });
    rel(volume(&d), AREA * 10.0 - HOLE_VOLUME);
}
