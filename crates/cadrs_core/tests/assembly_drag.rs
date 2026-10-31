//! Issue #191: dragging a part of a chain whose first link is fastened to the assembly's Origin
//! must keep the mates. The fixture is the reporter's document (`tests/fixtures/
//! issue191_assembly_tests.ron`): "serial_3_break" is three links, the first fastened to the
//! Origin, the others hung off it by revolutes. Each frame continues from the last one's result,
//! as the app's drag does ([`cadrs_app::assembly::drag`]).

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_core::assembly::solver::{Pull, Solution};
use cadrs_core::assembly::{self, Assembly, InstanceId, structure};
use cadrs_core::{Document, ElementId};

fn document() -> Document {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/issue191_assembly_tests.ron"
    );
    cadrs_core::Store::load_path(std::path::Path::new(path))
        .unwrap()
        .document
}

/// The solver's model of the assembly named `name`, and its instances' solids.
fn flat(doc: &Document, name: &str) -> (Assembly, HashMap<InstanceId, Arc<cadrs_core::Solid>>) {
    let builds: HashMap<ElementId, Arc<cadrs_core::rebuild::Build>> = doc
        .elements
        .iter()
        .filter(|e| e.assembly_model().is_none())
        .map(|e| (e.id, cadrs_core::rebuild::build(e.features())))
        .collect();
    let el = doc.elements.iter().find(|e| e.name == name).unwrap();
    let asm = el.assembly_model().unwrap();
    (
        structure::solver_model(doc, asm),
        assembly::occurrence_solids(doc, asm, |e| builds.get(&e).cloned()),
    )
}

fn finite(s: &Solution) -> bool {
    s.poses.iter().all(|(_, p)| {
        p.translation.iter().all(|x| x.is_finite())
            && p.rotation.iter().flatten().all(|x| x.is_finite())
    })
}

/// Drags `instance` by the middle of its solid along a wide circle, with jumps as a fast pointer
/// makes, seen along `view`; every frame's result must hold the mates, stay finite, and leave
/// the link fastened to the Origin where it is.
fn drag_around(
    model: &Assembly,
    solids: &HashMap<InstanceId, Arc<cadrs_core::Solid>>,
    instance: InstanceId,
    fastened: InstanceId,
    view: [f64; 3],
) -> Result<(), String> {
    let mut model = model.clone();
    let (lo, hi) = solids[&instance].bounds().unwrap();
    let local = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let start = model.instance(instance).unwrap().pose.apply(local);
    let still = model.instance(fastened).unwrap().pose;
    for k in 0..600 {
        let t = k as f64 / 30.0;
        // A wide circle about the grab point; every 25th frame a jump of half a turn.
        let a = if k % 25 == 24 { t + 3.0 } else { t };
        let r = 60.0 + 40.0 * (t * 0.7).sin();
        let target = [
            start[0] + r * a.cos(),
            start[1] + r * a.sin(),
            start[2] + 20.0 * (t * 1.3).sin(),
        ];
        let pull = Pull {
            instance,
            point: local,
            target,
            view: Some(view),
        };
        let s = assembly::drag(&model, solids, &[pull]);
        if !finite(&s) {
            return Err(format!("frame {k}: a placement is not finite"));
        }
        if !s.converged {
            return Err(format!(
                "frame {k}: the mates broke (residual {} mm)",
                s.residual
            ));
        }
        for (id, p) in s.changed(&model) {
            model.instance_mut(id).unwrap().pose = p;
        }
        let now = model.instance(fastened).unwrap().pose;
        let moved = (0..3)
            .map(|i| (now.translation[i] - still.translation[i]).abs())
            .fold(0.0f64, f64::max);
        if moved > 1e-6 {
            return Err(format!("frame {k}: the fastened link moved {moved} mm"));
        }
    }
    Ok(())
}

/// Issue #191: any link of "serial_3_break", dragged around from the top or from the default
/// isometric view, keeps the chain together and the fastened link where it is.
#[test]
fn dragging_a_chain_fastened_to_the_origin_keeps_its_mates() {
    let doc = document();
    let (model, solids) = flat(&doc, "serial_3_break");
    // The first link is the one fastened to the Origin.
    let fastened = model.instances[0].id;
    let iso = [1.0 / 3f64.sqrt(); 3];
    let mut failures = Vec::new();
    for inst in &model.instances {
        for (name, view) in [("top", [0.0, 0.0, 1.0]), ("isometric", iso)] {
            if let Err(e) = drag_around(&model, &solids, inst.id, fastened, view) {
                failures.push(format!("{} from the {name}: {e}", inst.id.0));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// How far a placement's rotation is from orthonormal (the largest entry of RᵀR − I).
fn off_orthonormal(p: &cadrs_core::assembly::Pose) -> f64 {
    let r = p.rotation_matrix();
    (r.transpose() * r - nalgebra::Matrix3::identity())
        .abs()
        .max()
}

/// The cause of #191: a placement a little off orthonormal (as saved, or as composed with
/// rounding) comes back orthonormal from a drag, instead of a little further off each frame
/// (the error doubled every frame until the mates broke).
#[test]
fn a_drag_keeps_placements_orthonormal() {
    let doc = document();
    let (mut model, solids) = flat(&doc, "serial_3_break");
    let end = model.instances[2].id;
    let pose = &mut model.instance_mut(end).unwrap().pose;
    pose.rotation[0][1] += 1e-9;
    pose.rotation[2][0] -= 1e-9;
    let start = model.instance(end).unwrap().pose.translation;
    for k in 0..200 {
        let a = k as f64 / 20.0;
        let pull = Pull {
            instance: end,
            point: [0.0; 3],
            target: [
                start[0] + 30.0 * a.cos(),
                start[1] + 30.0 * a.sin(),
                start[2],
            ],
            view: Some([0.0, 0.0, 1.0]),
        };
        let s = assembly::drag(&model, &solids, &[pull]);
        assert!(s.converged, "frame {k}: residual {}", s.residual);
        for (id, p) in s.changed(&model) {
            model.instance_mut(id).unwrap().pose = p;
        }
        for i in &model.instances {
            let off = off_orthonormal(&i.pose);
            assert!(
                off < 1e-12,
                "frame {k}: {} is {off:e} off orthonormal",
                i.id.0
            );
        }
    }
}

/// A placement that isn't a number (a diverged solve's) is refused, not saved (#191).
#[test]
fn a_placement_that_is_not_a_number_is_refused() {
    use cadrs_core::assembly::commands::MoveInstances;
    let mut doc = document();
    let el = doc
        .elements
        .iter()
        .find(|e| e.name == "serial_3_break")
        .unwrap();
    let (element, instance) = (el.id, el.assembly_model().unwrap().instances[1].id);
    let mut pose = el.assembly_model().unwrap().instances[1].pose;
    pose.translation[0] = f64::NAN;
    let moved = MoveInstances {
        element,
        poses: vec![(instance, pose)],
        label: "Drag instance".into(),
    };
    let mut history = cadrs_core::History::default();
    assert!(history.execute(&mut doc, &moved).is_err());
    let asm = doc.element(element).unwrap().assembly_model().unwrap();
    assert!(asm.instance(instance).unwrap().pose.is_finite());
}
