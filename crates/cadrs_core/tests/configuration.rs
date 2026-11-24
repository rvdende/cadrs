//! Configurations of a Part Studio ([`cadrs_core::configuration`]): presets of inputs that
//! drive feature fields and suppression, rebuilt by the kernel, in the studio and as the
//! generated studios an assembly's instances use. The model is the course's design-intent
//! cylinder (`samples::design_intent`): its Piston is a Ø`#piston_d` (40 mm) extrude whose depth
//! the input `#piston_len` drives here, so its volume is π/4·40²·len.
#![cfg(feature = "brep")]

use std::f64::consts::PI;

use cadrs_core::assembly::commands::InsertInstance;
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::commands::SetSuppressByVariable;
use cadrs_core::configuration::{
    self as cfg, AddConfigInput, AddPreset, ConfigInput, ConnectParameter, InputKind,
    SetActivePreset, SetInstancePreset, SetPresetValue,
};
use cadrs_core::document::Document;
use cadrs_core::ids::{InputId, PresetId};
use cadrs_core::rebuild;
use cadrs_core::samples::design_intent::{self as di};
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::variables::SuppressByVariable;
use cadrs_core::{ElementId, History};

#[track_caller]
fn rel(a: f64, b: f64, tol: f64) {
    assert!(
        ((a - b) / b).abs() <= tol,
        "got {a}, expected {b} (relative {tol})"
    );
}

fn piston(len: f64) -> f64 {
    PI / 4.0 * di::PISTON_D * di::PISTON_D * len
}

struct Doc {
    d: Document,
    h: History,
    part: ElementId,
    asm: ElementId,
    len: InputId,
    clamp: InputId,
    short: PresetId,
    long: PresetId,
}

impl Doc {
    /// The cylinder with `#piston_len` (Short 30 mm, Long 50 mm) wired into the Piston's depth
    /// and `#withClamp` (Short 1, Long 0) suppressing the Clamp; Long is active.
    fn new() -> Self {
        let mut d = Document::new("Configured cylinder");
        let part = d.elements[0].id;
        let asm = d.elements[1].id;
        let mut h = History::default();
        di::build_in(&mut DocHistory(&mut d, &mut h), part).unwrap();
        let len = ConfigInput::new("piston_len", InputKind::Length, 30.0);
        let clamp = ConfigInput::new("withClamp", InputKind::Boolean, 1.0);
        let (l, c) = (len.id, clamp.id);
        h.execute(
            &mut d,
            &AddConfigInput {
                element: part,
                input: len,
            },
        )
        .unwrap();
        h.execute(
            &mut d,
            &AddConfigInput {
                element: part,
                input: clamp,
            },
        )
        .unwrap();
        h.execute(
            &mut d,
            &ConnectParameter {
                element: part,
                feature: di::PISTON,
                slot: "Depth".into(),
                expr: "#piston_len".into(),
            },
        )
        .unwrap();
        h.execute(
            &mut d,
            &SetSuppressByVariable {
                element: part,
                feature: di::CLAMP,
                rule: Some(SuppressByVariable::variable("withClamp")),
            },
        )
        .unwrap();
        let short = d.element(part).unwrap().configuration().unwrap().presets[0].id;
        let long = PresetId::new();
        h.execute(
            &mut d,
            &AddPreset {
                element: part,
                preset: long,
                name: "Long".into(),
                copy_of: None,
            },
        )
        .unwrap();
        h.execute(
            &mut d,
            &SetPresetValue {
                element: part,
                preset: long,
                input: l,
                value: Some(50.0),
            },
        )
        .unwrap();
        h.execute(
            &mut d,
            &SetPresetValue {
                element: part,
                preset: long,
                input: c,
                value: Some(0.0),
            },
        )
        .unwrap();
        Self {
            d,
            h,
            part,
            asm,
            len: l,
            clamp: c,
            short,
            long,
        }
    }

    fn build(&self, element: ElementId) -> std::sync::Arc<rebuild::Build> {
        let b = rebuild::build(&self.d.element(element).unwrap().active_part_features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        b
    }
}

#[test]
fn presets_rebuild_the_studio() {
    let mut d = Doc::new();
    let b = d.build(d.part);
    rel(
        b.part(di::PISTON_PART).unwrap().mass.unwrap().volume,
        piston(50.0),
        1e-6,
    );
    assert!(b.part(di::CLAMP_PART).is_none(), "Long has no clamp");
    d.h.execute(
        &mut d.d,
        &SetActivePreset {
            element: d.part,
            preset: d.short,
        },
    )
    .unwrap();
    let b = d.build(d.part);
    rel(
        b.part(di::PISTON_PART).unwrap().mass.unwrap().volume,
        piston(30.0),
        1e-6,
    );
    assert!(b.part(di::CLAMP_PART).is_some(), "Short has the clamp");
    // The inputs' values are what the presets say.
    let c = d.d.element(d.part).unwrap().configuration().unwrap();
    assert_eq!(
        c.preset(d.long).unwrap().value(c.input(d.len).unwrap()),
        50.0
    );
    assert_eq!(
        c.preset(d.short).unwrap().value(c.input(d.clamp).unwrap()),
        1.0
    );
}

#[test]
fn assembly_instances_build_their_preset() {
    let mut d = Doc::new();
    let short = cfg::configured_id(d.part, d.short);
    let long = cfg::configured_id(d.part, d.long);
    for src in [short, long] {
        d.h.execute(
            &mut d.d,
            &InsertInstance {
                element: d.asm,
                instance: Instance::new(
                    InstanceId::new(),
                    InstanceSource::Part {
                        element: src,
                        part: di::PISTON_PART,
                    },
                    Pose::default(),
                ),
            },
        )
        .unwrap();
    }
    // Each instance's studio is its preset, whatever the studio shows.
    let vol = |d: &Doc, src| {
        d.build(src)
            .part(di::PISTON_PART)
            .unwrap()
            .mass
            .unwrap()
            .volume
    };
    rel(vol(&d, short), piston(30.0), 1e-6);
    rel(vol(&d, long), piston(50.0), 1e-6);
    // As an assembly's builds see them (`part_features`, the whole list).
    let pf = d.d.element(short).unwrap().part_features().to_vec();
    let b = rebuild::build(&pf);
    rel(
        b.part(di::PISTON_PART).unwrap().mass.unwrap().volume,
        piston(30.0),
        1e-6,
    );
    // Switch the first instance to Long: both build 50 mm pistons.
    let first = d.d.element(d.asm).unwrap().assembly.instances[0].id;
    d.h.execute(
        &mut d.d,
        &SetInstancePreset {
            assembly: d.asm,
            instances: vec![first],
            preset: d.long,
        },
    )
    .unwrap();
    let asm = &d.d.element(d.asm).unwrap().assembly;
    assert!(asm.instances.iter().all(|i| i.source.element() == long));
    // The unused Short studio is dropped.
    assert!(d.d.element(short).is_none());
}
