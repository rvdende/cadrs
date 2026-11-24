use super::*;
use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::command::History;
use crate::document::ExtrudeFeature;
use crate::ids::PartId;

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
        self.el().configuration().unwrap()
    }

    fn extrude(&self) -> &ExtrudeFeature {
        self.el().feature(self.ext).unwrap().extrude().unwrap()
    }

    /// `#length` (Length, default 30) with presets Short (30) and Long (50, active).
    fn with_presets(&mut self) -> (InputId, PresetId, PresetId) {
        let input = ConfigInput::new("length", InputKind::Length, 30.0);
        let i = input.id;
        self.run(&AddConfigInput {
            element: self.ps,
            input,
        });
        let short = self.config().presets[0].id;
        self.run(&RenamePreset {
            element: self.ps,
            preset: short,
            name: "Short".into(),
        });
        let long = PresetId::new();
        self.run(&AddPreset {
            element: self.ps,
            preset: long,
            name: "Long".into(),
            copy_of: None,
        });
        self.run(&SetPresetValue {
            element: self.ps,
            preset: long,
            input: i,
            value: Some(50.0),
        });
        (i, short, long)
    }
}

#[test]
fn an_input_drives_a_wired_field_per_preset() {
    let mut t = T::new();
    let (_, short, long) = t.with_presets();
    assert_eq!(t.config().active, long);
    t.run(&ConnectParameter {
        element: t.ps,
        feature: t.ext,
        slot: "Depth".into(),
        expr: "#length".into(),
    });
    assert_eq!(t.extrude().depth_expr, "#length");
    assert_eq!(t.extrude().depth, 50.0);
    t.run(&SetActivePreset {
        element: t.ps,
        preset: short,
    });
    assert_eq!(t.extrude().depth, 30.0);
    // One undo step each way.
    t.h.undo(&mut t.d);
    assert_eq!(t.extrude().depth, 50.0);
    t.h.redo(&mut t.d);
    assert_eq!(t.extrude().depth, 30.0);
    // The rebuild sees the input as a Variable ahead of the list, under a stable id.
    let fs = t.el().active_features();
    let FeatureKind::Variable(v) = &fs[0].kind else {
        panic!("{:?}", fs[0].kind)
    };
    assert_eq!((v.name.as_str(), v.value), ("length", 30.0));
    assert_eq!(fs[0].id, t.config().inputs[0].feature_id());
    assert!(crate::variables::check(&fs, &t.d.units).is_empty());
}

#[test]
fn disconnecting_keeps_the_value() {
    let mut t = T::new();
    t.with_presets();
    t.run(&ConnectParameter {
        element: t.ps,
        feature: t.ext,
        slot: "Depth".into(),
        expr: "#length * 2".into(),
    });
    assert_eq!(t.extrude().depth, 100.0);
    t.run(&DisconnectParameter {
        element: t.ps,
        feature: t.ext,
        slot: "Depth".into(),
    });
    assert_eq!(t.extrude().depth_expr, "100 mm");
    assert_eq!(t.extrude().depth, 100.0);
    // A field it doesn't have.
    let bad = ConnectParameter {
        element: t.ps,
        feature: t.ext,
        slot: "Width".into(),
        expr: "#length".into(),
    };
    assert!(t.h.execute(&mut t.d, &bad).is_err());
}

#[test]
fn renaming_an_input_rewrites_its_uses_and_removing_bakes_them() {
    let mut t = T::new();
    let (i, ..) = t.with_presets();
    t.run(&ConnectParameter {
        element: t.ps,
        feature: t.ext,
        slot: "Depth".into(),
        expr: "#length + 1 mm".into(),
    });
    let mut input = t.config().input(i).unwrap().clone();
    input.name = "len".into();
    t.run(&EditConfigInput {
        element: t.ps,
        input,
    });
    assert_eq!(t.extrude().depth_expr, "#len + 1 mm");
    assert_eq!(t.extrude().depth, 51.0);
    t.run(&RemoveConfigInput {
        element: t.ps,
        input: i,
    });
    assert!(!t.extrude().depth_expr.contains('#'));
    assert_eq!(t.extrude().depth, 51.0);
    assert!(t.config().inputs.is_empty());
}

#[test]
fn names_are_checked() {
    let mut t = T::new();
    t.with_presets();
    let dup = AddConfigInput {
        element: t.ps,
        input: ConfigInput::new("length", InputKind::Number, 1.0),
    };
    assert!(t.h.execute(&mut t.d, &dup).is_err());
    let bad = AddConfigInput {
        element: t.ps,
        input: ConfigInput::new("2x", InputKind::Number, 1.0),
    };
    assert!(t.h.execute(&mut t.d, &bad).is_err());
    let dup_preset = AddPreset {
        element: t.ps,
        preset: PresetId::new(),
        name: "Long".into(),
        copy_of: None,
    };
    assert!(t.h.execute(&mut t.d, &dup_preset).is_err());
}

#[test]
fn a_boolean_input_suppresses_by_variable() {
    let mut t = T::new();
    let (.., long) = t.with_presets();
    let flag = ConfigInput::new("withExtrude", InputKind::Boolean, 1.0);
    let f = flag.id;
    t.run(&AddConfigInput {
        element: t.ps,
        input: flag,
    });
    t.run(&crate::commands::SetSuppressByVariable {
        element: t.ps,
        feature: t.ext,
        rule: Some(crate::variables::SuppressByVariable::variable(
            "withExtrude",
        )),
    });
    assert!(!t.el().is_suppressed(t.ext));
    t.run(&SetPresetValue {
        element: t.ps,
        preset: long,
        input: f,
        value: Some(0.0),
    });
    assert!(t.el().is_suppressed(t.ext));
    assert!(t.el().active_features().iter().all(|x| x.id != t.ext));
    // Removing the input removes the suppression by it.
    t.run(&RemoveConfigInput {
        element: t.ps,
        input: f,
    });
    assert!(t.el().feature(t.ext).unwrap().suppress_by.is_none());
}

#[test]
fn removing_presets_keeps_one() {
    let mut t = T::new();
    let (_, short, long) = t.with_presets();
    t.run(&RemovePreset {
        element: t.ps,
        preset: long,
    });
    assert_eq!(t.config().active, short);
    let last = RemovePreset {
        element: t.ps,
        preset: short,
    };
    assert!(t.h.execute(&mut t.d, &last).is_err());
}

#[test]
fn saved_and_loaded() {
    let mut t = T::new();
    t.with_presets();
    t.run(&MoveGraphNodes {
        element: t.ps,
        positions: vec![(t.ext, [120.0, 40.0])],
        inputs_node: Some([-200.0, 0.0]),
        parts_node: None,
    });
    let text = ron::to_string(&t.d).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back, t.d);
    // A studio without them saves nothing new.
    let plain = Document::new("Plain");
    let text = ron::to_string(&plain).unwrap();
    assert!(!text.contains("configuration") && !text.contains("node_layout"));
}

#[test]
fn rename_in_whole_names_only() {
    assert_eq!(rename_in("#a + #ab*2", "a", "x"), "#x + #ab*2");
    assert_eq!(rename_in("#a", "a", "b"), "#b");
    assert_eq!(rename_in("3 mm", "a", "b"), "3 mm");
}

#[test]
fn configured_ids_are_stable_and_distinct() {
    let s = ElementId::from_u128(1);
    let a = PresetId::from_u128(2);
    let b = PresetId::from_u128(3);
    assert_eq!(configured_id(s, a), configured_id(s, a));
    assert_ne!(configured_id(s, a), configured_id(s, b));
    assert_ne!(configured_id(s, a), s);
}

#[test]
fn an_instance_of_a_preset_is_a_generated_studio() {
    let mut t = T::new();
    let (_, short, long) = t.with_presets();
    t.run(&ConnectParameter {
        element: t.ps,
        feature: t.ext,
        slot: "Depth".into(),
        expr: "#length".into(),
    });
    let src = configured_id(t.ps, short);
    let inst = InstanceId::new();
    t.run(&crate::assembly::commands::InsertInstance {
        element: t.asm,
        instance: Instance::new(
            inst,
            InstanceSource::Part {
                element: src,
                part: PartId::new(t.ext, 0),
            },
            Pose::default(),
        ),
    });
    let g = t.d.element(src).unwrap();
    assert_eq!(g.name, "Part Studio 1 (Short)");
    assert_eq!(g.feature(t.ext).unwrap().extrude().unwrap().depth, 30.0);
    // The studio's own active preset is Long: the instance stays Short.
    assert_eq!(t.extrude().depth, 50.0);
    assert_eq!(configured_of(&t.d, src), Some((t.ps, short)));
    // Its studio's edits reach it.
    let mut input = t.config().inputs[0].clone();
    input.default = 35.0;
    t.run(&EditConfigInput {
        element: t.ps,
        input,
    });
    let g = t.d.element(src).unwrap();
    assert_eq!(g.feature(t.ext).unwrap().extrude().unwrap().depth, 35.0);
    // Another preset: same instance, same part id, Long's studio.
    t.run(&SetInstancePreset {
        assembly: t.asm,
        instances: vec![inst],
        preset: long,
    });
    let i = &t.d.element(t.asm).unwrap().assembly.instances[0];
    assert_eq!(i.id, inst);
    assert_eq!(i.source.element(), configured_id(t.ps, long));
    assert_eq!(i.source.part(), Some(PartId::new(t.ext, 0)));
    // Only the presets in use are kept.
    assert_eq!(t.d.configured.len(), 1);
    let g = t.d.element(configured_id(t.ps, long)).unwrap();
    assert_eq!(g.feature(t.ext).unwrap().extrude().unwrap().depth, 50.0);
    // Undo goes back to Short's studio.
    t.h.undo(&mut t.d);
    let i = &t.d.element(t.asm).unwrap().assembly.instances[0];
    assert_eq!(i.source.element(), src);
    assert!(t.d.element(src).is_some());
    // Saved with the document.
    let back: Document = ron::from_str(&ron::to_string(&t.d).unwrap()).unwrap();
    assert!(back.element(src).is_some());
}

#[test]
fn removing_the_configuration_bakes_the_values() {
    let mut t = T::new();
    t.with_presets();
    t.run(&ConnectParameter {
        element: t.ps,
        feature: t.ext,
        slot: "Depth".into(),
        expr: "#length".into(),
    });
    t.run(&SetConfigured {
        element: t.ps,
        configured: false,
    });
    assert!(t.el().configuration().is_none());
    assert_eq!(t.extrude().depth, 50.0);
    assert!(!t.extrude().depth_expr.contains('#'));
    assert!(t.el().studio_features().is_empty());
}

/// Every configuration command is one undo step: undo puts the document back as it was, redo
/// as it was after.
#[test]
fn every_command_undoes_and_redoes() {
    let mut t = T::new();
    let (i, short, long) = t.with_presets();
    let flag = ConfigInput::new("on", InputKind::Boolean, 1.0);
    let commands: Vec<Box<dyn Command>> = vec![
        Box::new(AddConfigInput {
            element: t.ps,
            input: flag.clone(),
        }),
        Box::new(EditConfigInput {
            element: t.ps,
            input: ConfigInput {
                default: 12.0,
                ..t.config().input(i).unwrap().clone()
            },
        }),
        Box::new(SetPresetValue {
            element: t.ps,
            preset: short,
            input: i,
            value: Some(7.0),
        }),
        Box::new(RenamePreset {
            element: t.ps,
            preset: short,
            name: "Tiny".into(),
        }),
        Box::new(SetActivePreset {
            element: t.ps,
            preset: short,
        }),
        Box::new(ConnectParameter {
            element: t.ps,
            feature: t.ext,
            slot: "Depth".into(),
            expr: "#length".into(),
        }),
        Box::new(DisconnectParameter {
            element: t.ps,
            feature: t.ext,
            slot: "Depth".into(),
        }),
        Box::new(MoveGraphNodes {
            element: t.ps,
            positions: vec![(t.ext, [1.0, 2.0])],
            inputs_node: Some([3.0, 4.0]),
            parts_node: Some([5.0, 6.0]),
        }),
        Box::new(SetNodeCollapsed {
            element: t.ps,
            feature: t.ext,
            collapsed: true,
        }),
        Box::new(ResetNodeLayout { element: t.ps }),
        Box::new(AddPreset {
            element: t.ps,
            preset: PresetId::new(),
            name: "Third".into(),
            copy_of: Some(long),
        }),
        Box::new(RemovePreset {
            element: t.ps,
            preset: long,
        }),
        Box::new(RemoveConfigInput {
            element: t.ps,
            input: i,
        }),
        Box::new(SetConfigured {
            element: t.ps,
            configured: false,
        }),
    ];
    for c in commands {
        let before = t.d.clone();
        t.h.execute(&mut t.d, c.as_ref())
            .unwrap_or_else(|e| panic!("{}: {e}", c.label()));
        let after = t.d.clone();
        assert_ne!(before, after, "{} changed nothing", c.label());
        t.h.undo(&mut t.d);
        assert_eq!(t.d, before, "undo of {}", c.label());
        t.h.redo(&mut t.d);
        assert_eq!(t.d, after, "redo of {}", c.label());
    }
}

/// Files from before configurations (no `configuration`, `node_layout` or `configured`) load
/// as studios without one.
#[test]
fn old_documents_load() {
    let doc = Document::new("Old");
    let text = ron::to_string(&doc).unwrap();
    assert!(!text.contains("configuration") && !text.contains("configured"));
    let back: Document = ron::from_str(&text).unwrap();
    assert!(back.elements[0].configuration().is_none());
    assert!(back.elements[0].node_layout().is_none());
    assert!(back.configured.is_empty());
}

fn insert_preset(t: &mut T, preset: PresetId) -> InstanceId {
    let id = InstanceId::new();
    t.run(&crate::assembly::commands::InsertInstance {
        element: t.asm,
        instance: Instance::new(
            id,
            InstanceSource::Part {
                element: configured_id(t.ps, preset),
                part: PartId::new(t.ext, 0),
            },
            Pose::default(),
        ),
    });
    id
}

/// A preset an assembly uses can't be removed, nor the configuration: the instances would lose
/// their parts. Once they use another, it can.
#[test]
fn presets_in_use_stay() {
    let mut t = T::new();
    let (_, short, long) = t.with_presets();
    let inst = insert_preset(&mut t, short);
    let e =
        t.h.execute(
            &mut t.d,
            &RemovePreset {
                element: t.ps,
                preset: short,
            },
        )
        .unwrap_err();
    assert!(e.to_string().contains("1 instance in Assembly 1"), "{e}");
    assert!(
        t.h.execute(
            &mut t.d,
            &SetConfigured {
                element: t.ps,
                configured: false,
            },
        )
        .is_err()
    );
    t.run(&SetInstancePreset {
        assembly: t.asm,
        instances: vec![inst],
        preset: long,
    });
    t.run(&RemovePreset {
        element: t.ps,
        preset: short,
    });
    assert_eq!(instances_using(&t.d, t.ps, None).0, 1);
}

/// A preset an assembly uses is generated again only when its studio's build changes: not when
/// the node view's nodes move, nor when the studio shows another preset.
#[test]
fn moving_nodes_or_switching_presets_keeps_generated_studios() {
    let mut t = T::new();
    let (input, short, long) = t.with_presets();
    insert_preset(&mut t, short);
    // (The source each was generated from: a new one is a regeneration.)
    let generated = |t: &T| std::sync::Arc::as_ptr(t.d.configured[0].source.as_ref().unwrap());
    let before = generated(&t);
    t.run(&MoveGraphNodes {
        element: t.ps,
        positions: vec![(t.ext, [100.0, 40.0])],
        inputs_node: Some([0.0, 0.0]),
        parts_node: None,
    });
    t.run(&SetActivePreset {
        element: t.ps,
        preset: long,
    });
    assert_eq!(generated(&t), before);
    // The source kept as compared: the layout and the active preset are the studio's again.
    assert_eq!(t.config().active, long);
    assert!(
        t.el()
            .node_layout()
            .is_some_and(|l| !l.positions.is_empty())
    );
    // A value of the preset is a change.
    t.run(&SetPresetValue {
        element: t.ps,
        preset: short,
        input,
        value: Some(12.0),
    });
    assert_ne!(generated(&t), before);
}

/// An instance switched to another preset keeps its id, place and the assembly's mates.
#[test]
fn switching_presets_keeps_mates() {
    use crate::assembly::commands::AddMateFeature;
    use crate::assembly::mate::{MateFeature, MateId, MateKind};
    let mut t = T::new();
    let (_, short, long) = t.with_presets();
    let inst = insert_preset(&mut t, short);
    t.run(&AddMateFeature {
        element: t.asm,
        feature: MateFeature::new(
            MateId::new(),
            "#gap",
            MateKind::Variable(VariableFeature::length("gap", "4 mm")),
        ),
        poses: Vec::new(),
    });
    let before = t.d.element(t.asm).unwrap().assembly.clone();
    t.run(&SetInstancePreset {
        assembly: t.asm,
        instances: vec![inst],
        preset: long,
    });
    let after = &t.d.element(t.asm).unwrap().assembly;
    assert_eq!(after.mates, before.mates);
    assert_eq!(after.instances[0].id, before.instances[0].id);
    assert_eq!(after.instances[0].pose, before.instances[0].pose);
    assert_ne!(
        after.instances[0].source.element(),
        before.instances[0].source.element()
    );
}

/// A Variable named like an input is reported: the presets would stop reaching what is below it.
#[test]
fn a_variable_may_not_redefine_an_input() {
    let mut t = T::new();
    t.with_presets();
    let var = Feature::new(
        FeatureId::new(),
        "Variable 1",
        FeatureKind::Variable(VariableFeature::length("length", "3 mm")),
    );
    let vid = var.id;
    t.d.element_mut(t.ps)
        .unwrap()
        .features_mut()
        .unwrap()
        .insert(0, var);
    let errors = crate::variables::check(&t.el().active_features(), &t.d.units);
    assert!(
        errors
            .iter()
            .any(|(f, w)| *f == vid && w.contains("is a configuration input")),
        "{errors:?}"
    );
}

/// A preset's part's properties are its studio's part's (one set for every preset).
#[test]
fn a_preset_parts_properties_are_the_studios() {
    use crate::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};
    let mut t = T::new();
    let (_, short, _) = t.with_presets();
    insert_preset(&mut t, short);
    let part = PartId::new(t.ext, 0);
    t.run(&SetProperties {
        owners: vec![PropertyOwner::Part {
            element: configured_id(t.ps, short),
            part,
        }],
        values: vec![(
            PropertyKey::Description,
            PropertyValue::Text("A block".into()),
        )],
        label: "Description".into(),
    });
    let own = |el: &Element| {
        el.part_prop(part)
            .map(|p| format!("{:?}", p.properties))
            .unwrap_or_default()
    };
    assert!(own(t.el()).contains("A block"));
    // The preset's studio took it up.
    assert!(own(t.d.element(configured_id(t.ps, short)).unwrap()).contains("A block"));
}

/// A sketch's dimension wired to an input follows the preset (in the studio and in a preset's
/// generated copy), is listed as driven, and keeps its value when unwired.
#[test]
fn an_input_drives_a_sketch_dimension() {
    use cadrs_sketch::{Dimension, DimensionKind, Vec2};
    let mut t = T::new();
    let (_, short, long) = t.with_presets();
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
    // Long is active: 50.
    let s = slots(&t);
    assert!(s[0].is_driven());
    assert_eq!(s[0].value, 50.0);
    t.run(&SetActivePreset {
        element: t.ps,
        preset: short,
    });
    assert_eq!(slots(&t)[0].value, 30.0);
    // A preset's copy (for an assembly instance) has its own value.
    let copy = generate(t.el(), long, &t.d.units).unwrap();
    let FeatureKind::Sketch(cs) = &copy.feature(sk).unwrap().kind else {
        panic!()
    };
    assert_eq!(cs.geometry.dimensions[dim].value, 50.0);
    // Unwired, it keeps the value it shows.
    t.run(&DisconnectDimension {
        element: t.ps,
        feature: sk,
        dimension: dim,
    });
    let s = slots(&t);
    assert!(!s[0].is_driven());
    assert_eq!(s[0].value, 30.0);
}
