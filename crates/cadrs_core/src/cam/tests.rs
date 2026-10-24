//! CAM Studio: components, jobs, setups and operations through the commands; the kernel's
//! model of a part; toolpaths, Verify and posts end to end on the with-hole plate (60 × 40 × 10
//! with a Ø16 hole through at (30, 20)).

use std::collections::HashMap;

use cadrs_cam::machine::catalog;
use cadrs_cam::strategy::{Pattern, Side};
use cadrs_cam::toolpath::Step;
use cadrs_cam::{MachineKind, Params, Tool, ToolKind};

use super::*;
use crate::command::History;
use crate::commands::{AddElement, NewElementKind};
use crate::samples::with_hole;
use crate::studio::DocHistory;

struct Fixture {
    doc: Document,
    history: History,
    studio: ElementId,
    cam: ElementId,
}

impl Fixture {
    fn new() -> Fixture {
        let mut doc = Document::new("CAM test");
        let mut history = History::new(100);
        let studio = doc.elements[0].id;
        with_hole::build_in(&mut DocHistory(&mut doc, &mut history), studio).unwrap();
        let cam = ElementId::new();
        history
            .execute(
                &mut doc,
                &AddElement {
                    id: cam,
                    kind: NewElementKind::CamStudio,
                    name: None,
                    after: None,
                },
            )
            .unwrap();
        Fixture {
            doc,
            history,
            studio,
            cam,
        }
    }

    fn run(&mut self, edit: Edit) -> Result<(), CommandError> {
        self.history
            .execute(&mut self.doc, &EditCam::new(self.cam, edit))
    }

    fn cam(&self) -> &CamStudio {
        self.doc.element(self.cam).unwrap().cam().unwrap()
    }

    fn parts(&self) -> Vec<crate::parts::Part> {
        crate::rebuild::build(self.cam().part_features())
            .parts
            .clone()
    }

    /// Inserts the plate, creates a job, a machine of `kind` and a setup; returns (job, setup).
    fn job(&mut self, kind: MachineKind) -> (CamId, CamId) {
        let machine = catalog().into_iter().find(|m| m.kind == kind).unwrap();
        self.job_on(machine)
    }

    /// [`Fixture::job`] on `machine`.
    fn job_on(&mut self, machine: cadrs_cam::Machine) -> (CamId, CamId) {
        let c =
            workspace_component(&self.doc, self.studio, with_hole::PLATE_PART, Role::Part).unwrap();
        self.run(Edit::AddComponents {
            components: vec![c],
            links: vec![],
        })
        .unwrap();
        let job = Job {
            id: CamId::new(),
            name: "Job 1".into(),
            bodies: vec![],
            stock: StockDef::default(),
            workholdings: vec![],
            machine: None,
            setups: vec![],
            suppressed: false,
        };
        let jid = job.id;
        self.run(Edit::AddJob(Box::new(job))).unwrap();
        self.run(Edit::SetMachine {
            job: jid,
            machine: Some(Box::new(machine)),
        })
        .unwrap();
        let setup = Setup {
            id: CamId::new(),
            name: "Setup 1".into(),
            program_number: 1001,
            mating: Mating::Automatic,
            wcs: WorkPlane::default(),
            operations: vec![],
            suppressed: false,
        };
        let sid = setup.id;
        self.run(Edit::AddSetup {
            job: jid,
            setup: Box::new(setup),
        })
        .unwrap();
        (jid, sid)
    }

    fn add_tool(&mut self, setup: CamId, tool: Tool, number: u32) -> CamId {
        let op = Operation {
            id: CamId::new(),
            name: tool.name.clone(),
            kind: OpKind::Tool(Box::new(ToolOp { tool, number })),
            suppressed: false,
            hidden: false,
        };
        let id = op.id;
        self.run(Edit::AddOperation {
            setup,
            after: None,
            op: Box::new(op),
        })
        .unwrap();
        id
    }

    fn add_toolpath(&mut self, setup: CamId, params: Params) -> CamId {
        let name = self.cam().next_name(params.pattern.label());
        let op = Operation {
            id: CamId::new(),
            name,
            kind: OpKind::Toolpath(Box::new(ToolpathOp {
                params,
                name_edited: false,
            })),
            suppressed: false,
            hidden: false,
        };
        let id = op.id;
        self.run(Edit::AddOperation {
            setup,
            after: None,
            op: Box::new(op),
        })
        .unwrap();
        id
    }

    /// Computes a toolpath through the kernel (its model in the work plane's coordinates).
    fn compute(&self, op: CamId) -> Result<cadrs_cam::Toolpath, String> {
        let parts = self.parts();
        let cam = self.cam();
        let input = program::op_input(cam, op, &parts)?;
        let first = geometry::model(program::model_request(cam, &input, vec![]))
            .wait()
            .ok_or("no kernel")??;
        let levels = program::section_levels(&input, &first);
        let model = if levels.is_empty() {
            first
        } else {
            geometry::model(program::model_request(cam, &input, levels))
                .wait()
                .ok_or("no kernel")??
        };
        program::generate(&input, &model, None)
    }
}

#[test]
fn components_follow_the_workspace_and_undo() {
    let mut f = Fixture::new();
    let c = workspace_component(&f.doc, f.studio, with_hole::PLATE_PART, Role::Part).unwrap();
    let cid = c.id;
    f.run(Edit::AddComponents {
        components: vec![c],
        links: vec![],
    })
    .unwrap();
    assert_eq!(f.cam().components.len(), 1);
    assert_eq!(f.parts().len(), 1);
    // Renaming the Part Studio reaches the component's reference.
    f.history
        .execute(
            &mut f.doc,
            &crate::commands::RenameElement {
                id: f.studio,
                name: "Plates".into(),
            },
        )
        .unwrap();
    assert_eq!(f.cam().components[0].derived.source_name, "Plates");
    assert!(
        f.cam().components[0]
            .reference_label()
            .contains("workspace")
    );
    f.run(Edit::Hide(cid, true)).unwrap();
    assert!(f.cam().components[0].hidden);
    f.history.undo(&mut f.doc).expect("undo");
    assert!(!f.cam().components[0].hidden);
    f.run(Edit::Delete(cid)).unwrap();
    assert!(f.cam().components.is_empty());
}

#[test]
fn the_kernel_model_has_the_exact_hole() {
    let mut f = Fixture::new();
    let (_, setup) = f.job(MachineKind::Mill);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let op = f.add_toolpath(
        setup,
        Params::new(Pattern::Profile2D, &tool, MachineKind::Mill),
    );
    let parts = f.parts();
    let input = program::op_input(f.cam(), op, &parts).unwrap();
    let m = geometry::model(program::model_request(f.cam(), &input, vec![-5.0]))
        .wait()
        .unwrap()
        .unwrap();
    let holes = m.holes();
    assert_eq!(holes.len(), 1, "{holes:?}");
    assert!((holes[0].diameter - 16.0).abs() < 1e-9);
    assert!(holes[0].through);
    // The WCS is the stock's top centre: the stock is 2 mm round the plate and 1 mm over it.
    assert!((holes[0].center[0]).abs() < 1e-9 && (holes[0].center[1]).abs() < 1e-9);
    assert!((holes[0].top + 1.0).abs() < 1e-9);
    // The exact section halfway down: the plate minus the hole.
    let s = m.sections.get(-5.0).unwrap();
    let a = cadrs_cam::region::area(s);
    let expect = 60.0 * 40.0 - std::f64::consts::PI * 64.0;
    // (The cap is tessellated at 0.01 mm: its chords cut the hole's circle a little.)
    assert!((a - expect).abs() < 0.1, "{a} {expect}");
    // Faces carry their persistent names.
    assert!(m.faces.iter().all(|f| f.name.contains('/')));
}

#[test]
fn mill_job_end_to_end() {
    let mut f = Fixture::new();
    let (job, setup) = f.job(MachineKind::Mill);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let mut p = Params::new(Pattern::Profile2D, &em, MachineKind::Mill);
    p.step_down = 3.0;
    // Faster than the Fanuc mill's spindle goes (the post limits it).
    p.speeds.spindle_rpm = 18_000.0;
    let profile = f.add_toolpath(setup, p);
    let bore = f.add_toolpath(setup, Params::new(Pattern::Bore, &em, MachineKind::Mill));
    f.add_tool(setup, Tool::new(ToolKind::ChamferMill, 6.0), 2);
    let cm = Tool::new(ToolKind::ChamferMill, 6.0);
    let chamfer = f.add_toolpath(
        setup,
        Params::new(Pattern::Chamfer2D, &cm, MachineKind::Mill),
    );
    let mut paths = HashMap::new();
    for op in [profile, bore, chamfer] {
        paths.insert(op, f.compute(op).unwrap());
    }
    // The profile runs 3 mm outside the plate (the WCS is the stock's top centre).
    let pts: Vec<[f64; 3]> = paths[&profile]
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Feed { to, .. } => Some(*to),
            _ => None,
        })
        .collect();
    let maxx = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    assert!((maxx - 33.0).abs() < 0.01, "{maxx}");
    let minz = pts.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    assert!((minz + 11.0).abs() < 1e-6, "{minz}");
    // Post.
    let cam = f.cam();
    let parts = f.parts();
    let j = cam.job(job).unwrap();
    let s = &j.setups[0];
    let run = program::setup_run(cam, j, s, &parts, &paths, None).unwrap();
    let nc = program::post_setup(s, j, &run).unwrap();
    assert!(nc.contains("O1001"));
    assert!(nc.contains("T1 M6") && nc.contains("T2 M6"));
    assert!(nc.contains("G2 ") || nc.contains("G3 "), "arcs: {nc}");
    // Its setup sheet: the stock, the WCS, both tools and the three operations with times.
    let sheet = super::sheet::setup_sheet(cam, j, s, &run, &parts, "Job_1_Setup_1.nc");
    assert!(sheet.starts_with("<!doctype html>"));
    assert!(
        sheet.contains("Program O1001 · Job_1_Setup_1.nc"),
        "{sheet}"
    );
    assert!(sheet.contains("64 × 44 × 11 mm"), "{sheet}");
    assert!(
        sheet.contains("The stock's box: top, centre, centre"),
        "{sheet}"
    );
    assert!(sheet.contains("<td>T1</td><td>D6 end mill</td>"), "{sheet}");
    assert!(
        sheet.contains("<td>T2</td><td>D6 chamfer mill / v-bit</td>"),
        "{sheet}"
    );
    assert!(sheet.contains("3 operations, 2 tools"), "{sheet}");
    // On the table where the mating puts it: centred it fits; 400 mm off in X it runs past
    // the Fanuc mill's 762 mm X travel, and the sheet says so.
    let stock = program::stock_in(&program::stock_shape(cam, j, &parts).unwrap(), &run.frame);
    assert!(program::travel_warnings(j, s, &run, &stock).is_empty());
    let mut off = s.clone();
    off.mating = Mating::Manual([400.0, 0.0, 0.0]);
    let w = program::travel_warnings(j, &off, &run, &stock);
    assert!(w.iter().any(|w| w.contains("X travel")), "{w:?}");
    let sheet_off = super::sheet::setup_sheet(cam, j, &off, &run, &parts, "x.nc");
    assert!(
        sheet_off.contains("beyond the machine's X travel"),
        "{sheet_off}"
    );
    // The bore breaks through the plate's (and the stock's) bottom: it says so.
    assert!(
        paths[&bore]
            .warnings
            .iter()
            .any(|w| w.contains("below the stock's bottom")),
        "{:?}",
        paths[&bore].warnings
    );
    // The 18 000 rpm asked for runs at the Fanuc mill's 10 000, the feeds slowed with it,
    // in the program, the sheet and the toolpath's warnings alike.
    assert!(nc.contains("S10000 M3") && !nc.contains("S18000"), "{nc}");
    assert!(
        sheet.contains("10000 rpm") && sheet.contains("667 mm/min"),
        "{sheet}"
    );
    assert!(
        paths[&profile]
            .warnings
            .iter()
            .any(|w| w.contains("over the machine's 10000 rpm")),
        "{:?}",
        paths[&profile].warnings
    );
    for name in ["2-axis profile 1", "Bore 1", "2-axis chamfer 1"] {
        assert!(sheet.contains(name), "{name} in {sheet}");
    }
    // Verify the profile and bore (the chamfer cuts into the part by design: the model has no
    // chamfer): no gouges.
    let run = program::setup_run(cam, j, s, &parts, &paths, Some(&[profile, bore])).unwrap();
    match program::simulate(cam, j, s, &run, &parts, &paths, None, 0.2).unwrap() {
        program::SimResult::Mill(r) => {
            assert_eq!(r.gouge_cells, 0, "{:?}", &r.gouges[..r.gouges.len().min(4)]);
            assert!(r.rapid_hits.is_empty(), "{:?}", r.rapid_hits);
            assert!(r.removed > 1000.0);
        }
        _ => panic!("a mill simulates a height map"),
    }
}

#[test]
fn laser_job_cuts_the_hole_first() {
    let mut f = Fixture::new();
    let (job, setup) = f.job(MachineKind::Laser);
    let laser = Tool::new(ToolKind::Laser, 0.2);
    f.add_tool(setup, laser.clone(), 1);
    let mut p = Params::new(Pattern::Cut, &laser, MachineKind::Laser);
    p.side = Side::Auto;
    let cut = f.add_toolpath(setup, p);
    let tp = f.compute(cut).unwrap();
    let ons = tp
        .steps
        .iter()
        .filter(|s| matches!(s, Step::BeamOn { .. }))
        .count();
    assert_eq!(ons, 2);
    let mut paths = HashMap::new();
    paths.insert(cut, tp);
    let cam = f.cam();
    let parts = f.parts();
    let j = cam.job(job).unwrap();
    let run = program::setup_run(cam, j, &j.setups[0], &parts, &paths, None).unwrap();
    let nc = program::post_setup(&j.setups[0], j, &run).unwrap();
    assert!(nc.contains("M4 S"), "{nc}");
    match program::simulate(cam, j, &j.setups[0], &run, &parts, &paths, None, 0.2).unwrap() {
        program::SimResult::Sheet(r) => {
            assert_eq!(r.parts_free, 1);
            assert!(r.damage < 0.01, "{}", r.damage);
        }
        _ => panic!("a laser simulates a sheet"),
    }
}

#[test]
fn toolpaths_use_the_tool_and_work_plane_above() {
    let mut f = Fixture::new();
    let (job, setup) = f.job(MachineKind::Mill);
    let t1 = f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let a = f.add_toolpath(
        setup,
        Params::new(Pattern::Profile2D, &em, MachineKind::Mill),
    );
    // A work plane turned 90° about Z, written in the setup's coordinates.
    let wp = Operation {
        id: CamId::new(),
        name: "Work plane 1".into(),
        kind: OpKind::WorkPlane(Box::new(WorkPlane {
            x: [0.0, 1.0, 0.0],
            kind: WorkPlaneType::None,
            ..WorkPlane::default()
        })),
        suppressed: false,
        hidden: false,
    };
    let wid = wp.id;
    f.run(Edit::AddOperation {
        setup,
        after: None,
        op: Box::new(wp),
    })
    .unwrap();
    let t2 = f.add_tool(setup, Tool::new(ToolKind::EndMill, 10.0), 2);
    let em10 = Tool::new(ToolKind::EndMill, 10.0);
    let b = f.add_toolpath(
        setup,
        Params::new(Pattern::Profile2D, &em10, MachineKind::Mill),
    );
    let cam = f.cam();
    assert_eq!(cam.tool_for(a).unwrap().number, 1);
    assert_eq!(cam.tool_for(b).unwrap().number, 2);
    assert_eq!(cam.work_plane_for(a).unwrap().0, None);
    assert_eq!(cam.work_plane_for(b).unwrap().0, Some(wid));
    let _ = (t1, t2);
    // The second profile, computed turned, posts in the setup's coordinates.
    let mut paths = HashMap::new();
    paths.insert(a, f.compute(a).unwrap());
    paths.insert(b, f.compute(b).unwrap());
    let parts = f.parts();
    let cam = f.cam();
    let j = cam.job(job).unwrap();
    let run = program::setup_run(cam, j, &j.setups[0], &parts, &paths, None).unwrap();
    let cuts = cadrs_cam::Aabb::of(run.ops[1].toolpath.steps.iter().filter_map(|s| match s {
        Step::Feed {
            to,
            kind: cadrs_cam::toolpath::FeedKind::Cut,
            ..
        } => Some(*to),
        _ => None,
    }));
    // 5 mm round the 60 × 40 plate, in the setup's X and Y.
    assert!((cuts.max[0] - 35.0).abs() < 0.01, "{cuts:?}");
    assert!((cuts.max[1] - 25.0).abs() < 0.01, "{cuts:?}");
    // As the program moves: the second (a new tool) from where the first ended, up clear, down
    // to its first height and only then across.
    let m = program::run_motions(&j.setups[0], j, &run, [0.0, 0.0, 40.0]);
    let end = m[0].last().unwrap().to;
    let entry = &m[1][..3];
    assert_eq!(entry[0].from, end);
    let same_xy = |s: &cadrs_cam::toolpath::Segment| {
        (s.from[0] - s.to[0]).abs() < 1e-9 && (s.from[1] - s.to[1]).abs() < 1e-9
    };
    assert!(same_xy(&entry[0]) && same_xy(&entry[1]), "{entry:?}");
    assert!(
        (entry[2].from[2] - entry[2].to[2]).abs() < 1e-9,
        "{entry:?}"
    );
    // A Dynamic work plane can't be posted on a 3-axis machine.
    let mut dynamic = cam.operation(wid).unwrap().2.clone();
    if let OpKind::WorkPlane(w) = &mut dynamic.kind {
        w.kind = WorkPlaneType::Dynamic;
    }
    f.run(Edit::SetOperation(Box::new(dynamic))).unwrap();
    let cam = f.cam();
    let j = cam.job(job).unwrap();
    let run = program::setup_run(cam, j, &j.setups[0], &parts, &paths, None).unwrap();
    assert!(program::post_setup(&j.setups[0], j, &run).is_err());
}

#[test]
fn reorder_duplicate_suppress_and_names() {
    let mut f = Fixture::new();
    let (_, setup) = f.job(MachineKind::Mill);
    let t = f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let a = f.add_toolpath(
        setup,
        Params::new(Pattern::Profile2D, &em, MachineKind::Mill),
    );
    assert_eq!(f.cam().operation(a).unwrap().2.name, "2-axis profile 1");
    f.run(Edit::Duplicate { id: a, seed: 7 }).unwrap();
    let ops: Vec<String> = f.cam().jobs[0].setups[0]
        .operations
        .iter()
        .map(|o| o.name.clone())
        .collect();
    assert_eq!(ops, ["D6 end mill", "2-axis profile 1", "2-axis profile 2"]);
    // Moving the tool below its toolpaths leaves them without one.
    f.run(Edit::Move {
        op: t,
        setup,
        index: 2,
    })
    .unwrap();
    assert!(f.cam().tool_for(a).is_none());
    f.history.undo(&mut f.doc).expect("undo");
    assert!(f.cam().tool_for(a).is_some());
    f.run(Edit::Suppress(t, true)).unwrap();
    assert!(f.cam().tool_for(a).is_none());
    f.run(Edit::Rename(a, "Outline".into())).unwrap();
    match &f.cam().operation(a).unwrap().2.kind {
        OpKind::Toolpath(tp) => assert!(tp.name_edited),
        _ => unreachable!(),
    }
    assert!(f.run(Edit::Rename(a, "  ".into())).is_err());
    // The tool went into the library.
    assert!(
        f.cam()
            .library
            .tools
            .iter()
            .any(|x| x.name == "D6 end mill")
    );
}

#[test]
fn setup_needs_a_machine() {
    let mut f = Fixture::new();
    let job = Job {
        id: CamId::new(),
        name: "Job 1".into(),
        bodies: vec![],
        stock: StockDef::default(),
        workholdings: vec![],
        machine: None,
        setups: vec![],
        suppressed: false,
    };
    let jid = job.id;
    f.run(Edit::AddJob(Box::new(job))).unwrap();
    let setup = Setup {
        id: CamId::new(),
        name: "Setup 1".into(),
        program_number: 1,
        mating: Mating::Automatic,
        wcs: WorkPlane::default(),
        operations: vec![],
        suppressed: false,
    };
    assert!(
        f.run(Edit::AddSetup {
            job: jid,
            setup: Box::new(setup),
        })
        .is_err()
    );
}

#[test]
fn saved_and_loaded() {
    let mut f = Fixture::new();
    let (_, setup) = f.job(MachineKind::Plasma);
    f.add_tool(setup, Tool::new(ToolKind::Plasma, 1.5), 1);
    let text = ron::to_string(&f.doc).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    let el = back.element(f.cam).unwrap();
    assert_eq!(el.cam().unwrap(), f.cam());
}

/// 3+2: a profile round the plate's front face, from a Dynamic work plane tipped 90° on a
/// trunnion mill; it posts as a tilted working plane and Verify leaves it out.
#[test]
fn dynamic_work_plane_posts_a_tilted_plane() {
    let mut f = Fixture::new();
    let five = catalog()
        .into_iter()
        .find(|m| m.indexing.is_some() && m.dialect == cadrs_cam::Dialect::Fanuc)
        .unwrap();
    let (job, setup) = f.job_on(five);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let top = f.add_toolpath(
        setup,
        Params::new(Pattern::Profile2D, &em, MachineKind::Mill),
    );
    let wp = Operation {
        id: CamId::new(),
        name: "Front".into(),
        kind: OpKind::WorkPlane(Box::new(WorkPlane {
            z: [0.0, -1.0, 0.0],
            x: [1.0, 0.0, 0.0],
            kind: WorkPlaneType::Dynamic,
            ..WorkPlane::default()
        })),
        suppressed: false,
        hidden: false,
    };
    f.run(Edit::AddOperation {
        setup,
        after: None,
        op: Box::new(wp),
    })
    .unwrap();
    let mut p = Params::new(Pattern::Face, &em, MachineKind::Mill);
    p.step_down = 0.5;
    let front = f.add_toolpath(setup, p.clone());
    // The right end faced on the Axis tab's own direction.
    p.axis = cadrs_cam::strategy::ToolAxis::Direction([1.0, 0.0, 0.0]);
    let right = f.add_toolpath(setup, p);
    let mut paths = HashMap::new();
    paths.insert(top, f.compute(top).unwrap());
    paths.insert(front, f.compute(front).unwrap());
    paths.insert(right, f.compute(right).unwrap());
    let parts = f.parts();
    let cam = f.cam();
    let j = cam.job(job).unwrap();
    let s = &j.setups[0];
    let run = program::setup_run(cam, j, s, &parts, &paths, None).unwrap();
    assert!(!run.ops[0].tilted && run.ops[1].tilted && run.ops[2].tilted);
    // The right end's facing runs over x = 30 and out.
    let right_cuts =
        cadrs_cam::Aabb::of(run.ops[2].toolpath.steps.iter().filter_map(|s| match s {
            Step::Feed {
                to,
                kind: cadrs_cam::toolpath::FeedKind::Cut,
                ..
            } => Some(*to),
            _ => None,
        }));
    assert!(right_cuts.min[0] > 29.9, "{right_cuts:?}");
    // In the setup's coordinates the facing runs over the plate's front (y = −20 and out).
    let cuts = cadrs_cam::Aabb::of(run.ops[1].toolpath.steps.iter().filter_map(|s| match s {
        Step::Feed {
            to,
            kind: cadrs_cam::toolpath::FeedKind::Cut,
            ..
        } => Some(*to),
        _ => None,
    }));
    assert!(cuts.max[1] < -19.9 && cuts.min[1] > -30.0, "{cuts:?}");
    let nc = program::post_setup(s, j, &run).unwrap();
    let g68 = nc.lines().find(|l| l.contains("G68.2")).expect(&nc);
    assert!(g68.contains("I0. J90. K0."), "{g68}");
    assert_eq!(nc.matches("G68.2").count(), 2, "{nc}");
    assert!(nc.contains("G53.1") && nc.contains("G69"), "{nc}");
    // Verify runs, the tilted facing cut on voxels: nothing runs into the stock.
    match program::simulate(cam, j, s, &run, &parts, &paths, None, 0.2).unwrap() {
        program::SimResult::Mill(r) => {
            assert!(r.rapid_hits.is_empty(), "{:?}", r.rapid_hits);
            assert!(r.holder_hits.is_empty(), "{:?}", r.holder_hits);
        }
        _ => panic!("a mill simulates a height map"),
    }
}

/// A second setup, the plate flipped over, starts from the stock the first left: the band the
/// profile cut through round the plate is gone, the plate still there (upside down), the
/// hole the bore made open.
#[test]
fn a_flipped_setup_starts_from_the_first_ones_stock() {
    let mut f = Fixture::new();
    let (job, setup) = f.job(MachineKind::Mill);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let mut p = Params::new(Pattern::Profile2D, &em, MachineKind::Mill);
    p.step_down = 4.0;
    let profile = f.add_toolpath(setup, p);
    let bore = f.add_toolpath(setup, Params::new(Pattern::Bore, &em, MachineKind::Mill));
    let flipped = Setup {
        id: CamId::new(),
        name: "Setup 2".into(),
        program_number: 1002,
        mating: Mating::Automatic,
        wcs: WorkPlane {
            z: [0.0, 0.0, -1.0],
            ..WorkPlane::default()
        },
        operations: vec![],
        suppressed: false,
    };
    let second = flipped.id;
    f.run(Edit::AddSetup {
        job,
        setup: Box::new(flipped),
    })
    .unwrap();
    let mut paths = HashMap::new();
    paths.insert(profile, f.compute(profile).unwrap());
    paths.insert(bore, f.compute(bore).unwrap());
    let parts = f.parts();
    let cam = f.cam();
    let j = cam.job(job).unwrap();
    let frame = program::setup_frame(cam, j, &j.setups[1], &parts);
    let map = program::stock_before(cam, j, second, None, &parts, &paths, &frame, 0.25)
        .expect("carried over the flip");
    use cadrs_cam::heightmap::EMPTY;
    // The setups' frames in the model: the first's origin at the stock's top centre, the
    // second's at the flipped stock's "top" (the old bottom), Y turned over.
    let at = |model: [f64; 3]| {
        let q = frame.to_local(model);
        map.sample([q[0], q[1]])
    };
    // The plate (0–60 × 0–40, the hole at (30, 20)): material, its top the old bottom.
    let v = at([50.0, 30.0, 0.0]);
    assert!(v != EMPTY && v.abs() < 0.05, "{v}");
    // The band round it the profile cut through: gone.
    assert_eq!(at([61.5, 20.0, 0.0]), EMPTY);
    // The Ø16 hole the bore made: gone.
    assert_eq!(at([30.0, 20.0, 0.0]), EMPTY);
}

/// A CAM Studio's components count as references: moving the tab to another document takes
/// the Part Studio they follow along, and updating references sees them.
#[test]
fn components_are_references() {
    let mut f = Fixture::new();
    let (_job, _setup) = f.job(MachineKind::Mill);
    let el = f.doc.element(f.cam).unwrap();
    assert_eq!(
        crate::move_doc::tab_dependencies(&f.doc, el),
        vec![f.studio]
    );
    let uses = crate::link_update::workspace_uses(&f.doc, Some(f.cam));
    assert_eq!(uses.len(), 1, "{uses:?}");
    assert_eq!(uses[0].source, f.studio);
    // The site resolves back to the component.
    assert!(crate::link_update::use_at(&f.doc, uses[0].site).is_some());
}

/// Deleting several rows is one undo step (a setup and its own operation together too); an
/// operation copies to a setup of another job.
#[test]
fn batch_delete_and_copy_across_jobs() {
    let mut f = Fixture::new();
    let (job, setup) = f.job(MachineKind::Mill);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let a = f.add_toolpath(
        setup,
        Params::new(Pattern::Profile2D, &em, MachineKind::Mill),
    );
    let b = f.add_toolpath(setup, Params::new(Pattern::Bore, &em, MachineKind::Mill));
    f.run(Edit::Duplicate { id: job, seed: 11 }).unwrap();
    let other_setup = f.cam().jobs[1].setups[0].id;
    f.run(Edit::CopyTo {
        op: b,
        setup: other_setup,
        seed: 5,
    })
    .unwrap();
    let copied = f.cam().jobs[1].setups[0].operations.last().unwrap().clone();
    assert_ne!(copied.id, b);
    assert_eq!(copied.name, "Bore 2");
    // Both toolpaths, and the second job's setup with its own, in one step.
    f.run(Edit::Batch(vec![
        Edit::Delete(a),
        Edit::Delete(b),
        Edit::Delete(other_setup),
        Edit::Delete(copied.id),
    ]))
    .unwrap();
    assert!(f.cam().find(a).is_none() && f.cam().find(b).is_none());
    assert!(f.cam().jobs[1].setups.is_empty());
    f.history.undo(&mut f.doc).expect("undo");
    assert!(f.cam().find(a).is_some() && f.cam().find(b).is_some());
    assert!(f.cam().find(copied.id).is_some());
}

/// A side setup (looking down +X) after the profile round the plate: the band the profile cut
/// is gone, so the plate's side is the top it sees, 2 under the stock's side (the stock's 2 mm
/// margin), carried as voxels.
#[test]
fn a_side_setup_starts_from_the_first_ones_stock() {
    let mut f = Fixture::new();
    let (job, setup) = f.job(MachineKind::Mill);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let mut p = Params::new(Pattern::Profile2D, &em, MachineKind::Mill);
    p.step_down = 4.0;
    let profile = f.add_toolpath(setup, p);
    let side = Setup {
        id: CamId::new(),
        name: "Side".into(),
        program_number: 1002,
        mating: Mating::Automatic,
        wcs: WorkPlane {
            z: [1.0, 0.0, 0.0],
            x: [0.0, 1.0, 0.0],
            ..WorkPlane::default()
        },
        operations: vec![],
        suppressed: false,
    };
    let second = side.id;
    f.run(Edit::AddSetup {
        job,
        setup: Box::new(side),
    })
    .unwrap();
    let mut paths = HashMap::new();
    paths.insert(profile, f.compute(profile).unwrap());
    let parts = f.parts();
    let cam = f.cam();
    let j = cam.job(job).unwrap();
    let frame = program::setup_frame(cam, j, &j.setups[1], &parts);
    let map = program::stock_before(cam, j, second, None, &parts, &paths, &frame, 0.25)
        .expect("carried to the side");
    // The side's middle (model y 20, z 5): the plate's +X face at x = 60, the stock's at 62.
    let q = frame.to_local([62.0, 20.0, 5.0]);
    let v = map.sample([q[0], q[1]]);
    assert!((v as f64 + 2.0).abs() < 0.7, "{v}");
}

/// Work planes never share a work offset: an automatic one skips the numbers the others
/// override, and an override on another origin's number is refused.
#[test]
fn work_offsets_are_never_shared() {
    let mut f = Fixture::new();
    let fanuc = catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Mill && m.dialect == cadrs_cam::Dialect::Fanuc)
        .unwrap();
    let (job, setup) = f.job_on(fanuc);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let mut paths = HashMap::new();
    let mut planes = vec![];
    // (Each at an origin of its own: a work offset is an origin.)
    for (x, datum, at) in [
        ([0.0, 1.0, 0.0], None, [10.0, 0.0, 10.0]),
        ([-1.0, 0.0, 0.0], Some(2), [20.0, 0.0, 10.0]),
    ] {
        let wp = Operation {
            id: CamId::new(),
            name: format!("Plane {}", planes.len() + 1),
            kind: OpKind::WorkPlane(Box::new(WorkPlane {
                x,
                datum,
                origin: Origin::Manual(at),
                ..WorkPlane::default()
            })),
            suppressed: false,
            hidden: false,
        };
        planes.push(wp.id);
        let last = f.cam().job(job).unwrap().setups[0]
            .operations
            .last()
            .map(|o| o.id);
        f.run(Edit::AddOperation {
            setup,
            after: last,
            op: Box::new(wp),
        })
        .unwrap();
        let op = f.add_toolpath(
            setup,
            Params::new(Pattern::Profile2D, &em, MachineKind::Mill),
        );
        paths.insert(op, f.compute(op).unwrap());
    }
    let parts = f.parts();
    let post = |f: &Fixture| {
        let cam = f.cam();
        let j = cam.job(job).unwrap();
        let run = program::setup_run(cam, j, &j.setups[0], &parts, &paths, None).unwrap();
        program::post_setup(&j.setups[0], j, &run)
    };
    // The first (automatic) plane would have taken G55, which the second overrides.
    let nc = post(&f).unwrap();
    assert!(nc.contains("G56") && nc.contains("G55"), "{nc}");
    let g56 = nc.find("G56").unwrap();
    let g55 = nc.find("G55").unwrap();
    assert!(g56 < g55, "{nc}");
    // The second plane on the setup's own G54: refused.
    let mut second = f.cam().operation(planes[1]).unwrap().2.clone();
    if let OpKind::WorkPlane(w) = &mut second.kind {
        w.datum = Some(1);
    }
    f.run(Edit::SetOperation(Box::new(second))).unwrap();
    let err = post(&f).unwrap_err();
    assert!(err.contains("work offset 1"), "{err}");
}

/// An Absolute work plane turned 90° about Z on its own work offset: G55 only moves the
/// origin, so the program cuts the plane's moves in the setup's axes, from the plane's origin.
/// The header, the setup sheet and the offset list say where G55 is.
#[test]
fn a_turned_work_plane_posts_in_the_setups_axes() {
    let mut f = Fixture::new();
    let fanuc = catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Mill && m.dialect == cadrs_cam::Dialect::Fanuc)
        .unwrap();
    let (job, setup) = f.job_on(fanuc);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let wp = Operation {
        id: CamId::new(),
        name: "Turned".into(),
        kind: OpKind::WorkPlane(Box::new(WorkPlane {
            x: [0.0, 1.0, 0.0],
            origin: Origin::Manual([5.0, 5.0, 10.0]),
            ..WorkPlane::default()
        })),
        suppressed: false,
        hidden: false,
    };
    let last = f.cam().job(job).unwrap().setups[0]
        .operations
        .last()
        .map(|o| o.id);
    f.run(Edit::AddOperation {
        setup,
        after: last,
        op: Box::new(wp),
    })
    .unwrap();
    let op = f.add_toolpath(
        setup,
        Params::new(Pattern::Profile2D, &em, MachineKind::Mill),
    );
    let mut paths = HashMap::new();
    paths.insert(op, f.compute(op).unwrap());
    let parts = f.parts();
    let cam = f.cam();
    let j = cam.job(job).unwrap();
    let s = &j.setups[0];
    let run = program::setup_run(cam, j, s, &parts, &paths, None).unwrap();
    let p = program::nc_program(s, j, &run).unwrap();
    assert_eq!(p.operations[0].datum, 2);
    // The posted moves are the setup's, less the plane's origin: no turn.
    let wf = run.ops[0].plane.unwrap().0;
    let o = run.frame.to_local(wf.origin);
    let (a, b) = (
        run.ops[0].toolpath.bounds(),
        p.operations[0].toolpath.bounds(),
    );
    for k in 0..3 {
        assert!(
            (a.min[k] - o[k] - b.min[k]).abs() < 1e-6,
            "{a:?} {b:?} {o:?}"
        );
        assert!(
            (a.max[k] - o[k] - b.max[k]).abs() < 1e-6,
            "{a:?} {b:?} {o:?}"
        );
    }
    // Where to set G55.
    let lines = program::offset_lines(s, j, &run);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("G55: Turned at X "), "{lines:?}");
    let nc = program::post_setup(s, j, &run).unwrap();
    assert!(nc.contains("(G55: Turned at X"), "{nc}");
    let sheet = crate::cam::sheet::setup_sheet(cam, j, s, &run, &parts, "x.nc");
    assert!(sheet.contains("Turned at X"), "{sheet}");
}

/// What a toolpath is cached by includes the machine's spindle limit (its speeds and feeds
/// are scaled to it): lowering it calculates the toolpath again.
#[test]
fn a_toolpath_is_recalculated_for_a_new_spindle_limit() {
    let mut f = Fixture::new();
    let (_, setup) = f.job(MachineKind::Mill);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let op = f.add_toolpath(
        setup,
        Params::new(Pattern::Profile2D, &em, MachineKind::Mill),
    );
    let parts = f.parts();
    let mut input = program::op_input(f.cam(), op, &parts).unwrap();
    let before = program::op_key(&input, 0);
    input.machine.max_spindle /= 2.0;
    assert_ne!(program::op_key(&input, 0), before);
}

/// A work offset is an origin: two planes at one origin (turned differently) share one, and a
/// plane at the setup's own origin uses the setup's.
#[test]
fn planes_at_one_origin_share_its_work_offset() {
    let mut f = Fixture::new();
    let fanuc = catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Mill && m.dialect == cadrs_cam::Dialect::Fanuc)
        .unwrap();
    let (job, setup) = f.job_on(fanuc);
    f.add_tool(setup, Tool::new(ToolKind::EndMill, 6.0), 1);
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let mut paths = HashMap::new();
    for (x, origin) in [
        ([1.0, 0.0, 0.0], Origin::Manual([10.0, 0.0, 10.0])),
        ([0.0, 1.0, 0.0], Origin::Manual([10.0, 0.0, 10.0])),
        ([0.0, -1.0, 0.0], WorkPlane::default().origin),
    ] {
        let wp = Operation {
            id: CamId::new(),
            name: "Plane".into(),
            kind: OpKind::WorkPlane(Box::new(WorkPlane {
                x,
                origin,
                ..WorkPlane::default()
            })),
            suppressed: false,
            hidden: false,
        };
        let last = f.cam().job(job).unwrap().setups[0]
            .operations
            .last()
            .map(|o| o.id);
        f.run(Edit::AddOperation {
            setup,
            after: last,
            op: Box::new(wp),
        })
        .unwrap();
        let op = f.add_toolpath(
            setup,
            Params::new(Pattern::Profile2D, &em, MachineKind::Mill),
        );
        paths.insert(op, f.compute(op).unwrap());
    }
    let parts = f.parts();
    let cam = f.cam();
    let j = cam.job(job).unwrap();
    let run = program::setup_run(cam, j, &j.setups[0], &parts, &paths, None).unwrap();
    let p = program::nc_program(&j.setups[0], j, &run).unwrap();
    let datums: Vec<u32> = p.operations.iter().map(|o| o.datum).collect();
    assert_eq!(datums, vec![2, 2, 1]);
}
