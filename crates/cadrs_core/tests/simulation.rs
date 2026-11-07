//! P3F.5 (`intro-to-parametric-cad-gaps.md`, "P3.5 simulation"; `intro-to-assemblies.md`
//! A1.7, A6.3): the simulation end to end through the kernel: a beam modelled with features,
//! its Loads list edited by commands, the model built from the rebuilt part and solved; the
//! same beam as two instances joined by a Fastened mate with Simulation connection.
//!
//! **Expected values** (Euler–Bernoulli, derived in `cadrs_fea/tests/acceptance.rs`): the beam is
//! 100 × 10 × 10 mm, Steel - A36 (E = 200 000 MPa), fixed at x = 0, 100 N down at x = 100:
//! I = 10·10³/12 = 833.33 mm⁴, tip deflection PL³/(3EI) = 100·10⁶/(3·200 000·833.33) =
//! **0.200 mm**; mid-span top fibre σ = P·(L/2)·(h/2)/I = 100·50·5/833.33 = **30.0 MPa**.

use std::sync::Arc;

use cadrs_core::command::History;
use cadrs_core::document::Document;
use cadrs_core::parts::Part;
use cadrs_core::rebuild::Rebuilder;
use cadrs_core::samples::gear_cover::DocHistory;
use cadrs_core::samples::simulation as sample;
use cadrs_core::simulation::{
    self, AddLoad, DeleteLoad, ForceDirection, LoadId, LoadKind, SetLoad, SimFace, SimLoad,
};
use cadrs_core::{Element, ElementId, PartProps};

fn no_progress(_: cadrs_fea::Stage, _: f32) {}

/// The parts of a Part Studio as rebuilt.
fn studio_parts(doc: &Document, el: ElementId) -> Vec<Part> {
    Rebuilder::new()
        .rebuild(doc.element(el).unwrap().features())
        .parts
}

/// The face of `part` whose centre has the given x.
fn face_at_x(part: &Part, x: f64) -> SimFace {
    let f = part
        .solid
        .faces
        .iter()
        .find(|f| {
            f.center
                .is_some_and(|c| (c[0] - x).abs() < 1e-6 && (c[1] - 5.0).abs() < 1e-6)
        })
        .expect("face");
    SimFace {
        part: part.id,
        face: f.name,
    }
}

fn fixed(name: &str, faces: Vec<SimFace>) -> SimLoad {
    SimLoad {
        id: LoadId::new(),
        name: name.into(),
        kind: LoadKind::Fixed,
        faces,
        suppressed: false,
    }
}

fn down_100n(faces: Vec<SimFace>) -> SimLoad {
    SimLoad {
        id: LoadId::new(),
        name: "Force 1".into(),
        kind: LoadKind::Force {
            newtons: 100.0,
            direction: ForceDirection::Z,
            flip: true,
        },
        faces,
        suppressed: false,
    }
}

/// The mean −z displacement of the solution's nodes at x = 100.
fn tip(s: &cadrs_fea::Solution) -> f64 {
    let (mut sum, mut n) = (0.0, 0);
    for b in &s.bodies {
        for (p, u) in b.mesh.nodes.iter().zip(&b.displacement) {
            if (p[0] - 100.0).abs() < 1e-6 {
                sum -= u[2];
                n += 1;
            }
        }
    }
    sum / n as f64
}

fn beam_document() -> (Document, History, ElementId) {
    let mut doc = Document::empty("Beam");
    let el = Element::part_studio("Part Studio 1");
    let id = el.id;
    doc.elements.push(el);
    let mut h = History::default();
    sample::beam_in(&mut DocHistory(&mut doc, &mut h), id).unwrap();
    (doc, h, id)
}

#[test]
fn the_cantilever_through_the_kernel_matches_beam_theory() {
    let (mut doc, mut h, el) = beam_document();
    let parts = studio_parts(&doc, el);
    assert_eq!(parts.len(), 1);
    let beam = &parts[0];
    h.execute(
        &mut doc,
        &AddLoad {
            element: el,
            load: fixed("Fixed 1", vec![face_at_x(beam, 0.0)]),
        },
    )
    .unwrap();
    h.execute(
        &mut doc,
        &AddLoad {
            element: el,
            load: down_100n(vec![face_at_x(beam, 100.0)]),
        },
    )
    .unwrap();
    let e = doc.element(el).unwrap();
    let setup = simulation::setup(
        &e.simulation,
        &parts,
        e.part_props(),
        &simulation::studio_bonds(&parts),
    )
    .unwrap();
    assert_eq!(setup.model.bodies[0].material.youngs, 200_000.0);
    let s = cadrs_fea::solve(&setup.model, &setup.options, &no_progress).unwrap();
    let d = tip(&s);
    eprintln!("kernel beam: {} elements, tip {d:.5} mm", s.stats.elements);
    assert!((d - 0.200).abs() / 0.200 < 0.03, "tip {d}");
    let sigma = s.stress_at(0, [50.0, 5.0, 10.0]).unwrap()[0];
    eprintln!("kernel beam: mid-span σxx {sigma:.3} MPa");
    assert!((sigma - 30.0).abs() / 30.0 < 0.05, "σ {sigma}");
}

#[test]
fn two_halves_fastened_with_simulation_connection_bend_like_the_beam() {
    let doc = sample::halves_document().unwrap();
    let build = Arc::new(Rebuilder::new().rebuild(doc.element(sample::HALVES).unwrap().features()));
    let asm = doc
        .element(sample::BEAM_ASSEMBLY)
        .unwrap()
        .assembly_model()
        .unwrap();
    let (parts, props): (Vec<Part>, Vec<PartProps>) =
        cadrs_core::assembly::instance_parts(&doc, asm, |_| Some(build.clone()));
    assert_eq!(parts.len(), 2);
    let h1 = parts
        .iter()
        .find(|p| p.id == sample::HALF_1.part_id())
        .unwrap();
    let h2 = parts
        .iter()
        .find(|p| p.id == sample::HALF_2.part_id())
        .unwrap();
    let mut sim = simulation::Simulation::default();
    sim.loads.push(fixed("Fixed 1", vec![face_at_x(h1, 0.0)]));
    sim.loads.push(down_100n(vec![face_at_x(h2, 100.0)]));
    let (bonds, notes) = simulation::assembly_bonds(asm);
    assert!(notes.is_empty());
    assert_eq!(
        bonds.len(),
        1,
        "Fastened 1 has Simulation connection checked"
    );
    let setup = simulation::setup(&sim, &parts, &props, &bonds).unwrap();
    assert_eq!(setup.model.bonds.len(), 1);
    let s = cadrs_fea::solve(&setup.model, &setup.options, &no_progress).unwrap();
    let d = tip(&s);
    eprintln!(
        "halves: {} elements, {} tied nodes, tip {d:.5} mm",
        s.stats.elements, s.stats.bonded_nodes[0]
    );
    // The solid beam's own solve, for the comparison the gap list asks for.
    let (mut bdoc, mut bh, el) = beam_document();
    let beam_parts = studio_parts(&bdoc, el);
    bh.execute(
        &mut bdoc,
        &AddLoad {
            element: el,
            load: fixed("Fixed 1", vec![face_at_x(&beam_parts[0], 0.0)]),
        },
    )
    .unwrap();
    bh.execute(
        &mut bdoc,
        &AddLoad {
            element: el,
            load: down_100n(vec![face_at_x(&beam_parts[0], 100.0)]),
        },
    )
    .unwrap();
    let e = bdoc.element(el).unwrap();
    let bs = simulation::setup(&e.simulation, &beam_parts, e.part_props(), &[]).unwrap();
    let solid = tip(&cadrs_fea::solve(&bs.model, &bs.options, &no_progress).unwrap());
    eprintln!(
        "halves: solid beam {solid:.5} mm, halves {d:.5} mm ({:+.2} %)",
        (d - solid) / solid * 100.0
    );
    assert!((d - solid).abs() / solid < 0.05);

    // Unchecked, the mate doesn't connect: Half 2 is free.
    let mut unchecked = asm.clone();
    if let cadrs_core::assembly::mate::MateKind::Mate(m) = &mut unchecked.mates[0].kind {
        m.simulation = false;
    }
    let (bonds, _) = simulation::assembly_bonds(&unchecked);
    assert!(bonds.is_empty());
    let setup = simulation::setup(&sim, &parts, &props, &bonds).unwrap();
    // Half 2 carries the force but nothing holds it.
    assert_eq!(
        cadrs_fea::solve(&setup.model, &setup.options, &no_progress).unwrap_err(),
        cadrs_fea::FeaError::Free("Half 2 <1>".into())
    );
}

#[test]
fn loads_undo_and_save() {
    let (mut doc, mut h, el) = beam_document();
    let parts = studio_parts(&doc, el);
    let f = fixed("Fixed 1", vec![face_at_x(&parts[0], 0.0)]);
    let id = f.id;
    h.execute(
        &mut doc,
        &AddLoad {
            element: el,
            load: f.clone(),
        },
    )
    .unwrap();
    let mut g = f.clone();
    g.name = "Wall".into();
    h.execute(
        &mut doc,
        &SetLoad {
            element: el,
            load: g,
        },
    )
    .unwrap();
    assert_eq!(doc.element(el).unwrap().simulation.loads[0].name, "Wall");
    h.undo(&mut doc);
    assert_eq!(doc.element(el).unwrap().simulation.loads[0].name, "Fixed 1");
    h.execute(
        &mut doc,
        &DeleteLoad {
            element: el,
            load: id,
        },
    )
    .unwrap();
    assert!(doc.element(el).unwrap().simulation.loads.is_empty());
    h.undo(&mut doc);
    assert_eq!(doc.element(el).unwrap().simulation.loads.len(), 1);
    // Saved and loaded with the element.
    let text = ron::to_string(&doc).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back, doc);
    // A document from before simulations loads with none.
    let plain = Element::part_studio("Old");
    let text = ron::to_string(&plain).unwrap();
    assert!(!text.contains("simulation"));
    assert_eq!(
        ron::from_str::<Element>(&text).unwrap().simulation,
        simulation::Simulation::default()
    );
}

#[test]
fn setup_explains_what_is_missing() {
    let (mut doc, mut h, el) = beam_document();
    let parts = studio_parts(&doc, el);
    let props = doc.element(el).unwrap().part_props().to_vec();
    let sim = |doc: &Document| doc.element(el).unwrap().simulation.clone();
    assert!(
        simulation::setup(&sim(&doc), &parts, &props, &[])
            .unwrap_err()
            .contains("Fixed")
    );
    h.execute(
        &mut doc,
        &AddLoad {
            element: el,
            load: fixed("Fixed 1", vec![face_at_x(&parts[0], 0.0)]),
        },
    )
    .unwrap();
    assert!(
        simulation::setup(&sim(&doc), &parts, &props, &[])
            .unwrap_err()
            .contains("Force or a Pressure")
    );
    h.execute(
        &mut doc,
        &AddLoad {
            element: el,
            load: down_100n(vec![]),
        },
    )
    .unwrap();
    assert!(
        simulation::setup(&sim(&doc), &parts, &props, &[])
            .unwrap_err()
            .contains("no faces")
    );
    let mut s = sim(&doc);
    s.loads[1].faces = vec![face_at_x(&parts[0], 100.0)];
    assert!(
        simulation::setup(&s, &parts, &[], &[])
            .unwrap_err()
            .contains("has no material")
    );
}

#[test]
fn the_simulation_fixture_is_current() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test simulation`.
    let doc = sample::halves_document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/simulation_halves.cadrs");
    let text = ron::ser::to_string_pretty(
        &sample::file(doc.clone()),
        ron::ser::PrettyConfig::default(),
    )
    .unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(
        stored.document, doc,
        "fixtures/simulation_halves.cadrs is out of date"
    );
}
