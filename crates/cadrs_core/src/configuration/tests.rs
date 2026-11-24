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
