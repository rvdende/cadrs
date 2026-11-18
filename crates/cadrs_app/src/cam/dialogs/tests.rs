//! The dialogs' drafts without the app: the toolpath dialog's tabs, the Axis and Collision
//! tabs' fields and what their changes do, the machine's post and indexing settings, and a
//! thread mill's fields. Multi-axis machining: the patterns a machine's rotary axes allow, the
//! rotary patterns' settings, the Axis tab's leaning modes, the machine's rotary post, a
//! setup's frame on the machine's mount, and the picks that set axes, points and frames.

use cadrs_core::cam::cadrs_cam::Frame;
use cadrs_core::cam::cadrs_cam::post::{RotaryFeed, TiltedPlane};
use cadrs_core::cam::cadrs_cam::strategy::collision::CollisionAction;
use cadrs_core::cam::cadrs_cam::strategy::{AxisMode, RotaryStyle, ToolAxis};

use super::*;

struct Studio {
    doc: cadrs_core::Document,
    cam: CamStudio,
    setup: CamId,
    tool_op: CamId,
}

/// A studio with one job on `machine` and a setup holding `tool`.
fn studio(machine: Machine, tool: Tool) -> Studio {
    let tool_op = Operation {
        id: CamId::new(),
        name: tool.name.clone(),
        kind: OpKind::Tool(Box::new(ToolOp { tool, number: 1 })),
        suppressed: false,
        hidden: false,
    };
    let setup = Setup {
        id: CamId::new(),
        name: "Setup 1".into(),
        program_number: 1001,
        mating: Mating::Automatic,
        wcs: WorkPlane::default(),
        operations: vec![tool_op.clone()],
        suppressed: false,
    };
    let mut cam = CamStudio::default();
    let (sid, tid) = (setup.id, tool_op.id);
    cam.jobs.push(Job {
        id: CamId::new(),
        name: "Job 1".into(),
        bodies: vec![],
        stock: StockDef::default(),
        workholdings: vec![],
        machine: Some(machine),
        machine_source: None,
        setups: vec![setup],
        suppressed: false,
    });
    Studio {
        doc: cadrs_core::Document::new("CAM dialogs"),
        cam,
        setup: sid,
        tool_op: tid,
    }
}

fn mill(indexing: bool) -> Machine {
    catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Mill && m.indexing.is_some() == indexing)
        .unwrap()
}

/// A mill with one rotary axis (A, under the work): the generic trunnion without its platter.
fn four_axis() -> Machine {
    let mut chain = cadrs_core::cam::cadrs_cam::kinematics::Chain::generic(Some(('A', 'C', 120.0)));
    chain.table.truncate(1);
    let mut m = mill(false);
    m.chain = Some(chain);
    assert_eq!(m.rotary_axes(), 1);
    m
}

fn toolpath(s: &Studio, pattern: Pattern, tool: &Tool, machine: MachineKind) -> Draft {
    Draft::Toolpath {
        setup: s.setup,
        after: Some(s.tool_op),
        op: Box::new(Operation {
            id: CamId::new(),
            name: "Toolpath".into(),
            kind: OpKind::Toolpath(Box::new(ToolpathOp {
                params: Params::new(pattern, tool, machine),
                name_edited: false,
            })),
            suppressed: false,
            hidden: false,
        }),
        is_new: true,
        kind: pattern.type_of(),
    }
}

fn fields(s: &Studio, d: &Draft, tab: usize) -> Vec<Field> {
    let ctx = Ctx {
        doc: &s.doc,
        cam: &s.cam,
        parts: &[],
        versions: &[],
    };
    d.fields(&ctx, None, tab)
}

/// A select's options and which is chosen.
fn select(f: &[Field], key: &str) -> (Vec<String>, usize) {
    f.iter()
        .find_map(|x| match x {
            Field::Select {
                key: k,
                options,
                selected,
                ..
            } if k == key => Some((options.clone(), *selected)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no select {key}"))
}

/// The studio after ✓ on a draft: its edits through the command layer, as the app runs them.
fn accepted(s: &Studio, d: &Draft) -> CamStudio {
    use cadrs_core::commands::{AddElement, NewElementKind};
    let mut doc = cadrs_core::Document::new("CAM dialogs");
    let mut history = cadrs_core::History::new(10);
    let id = ElementId::new();
    history
        .execute(
            &mut doc,
            &AddElement {
                id,
                kind: NewElementKind::CamStudio,
                name: None,
                after: None,
            },
        )
        .unwrap();
    *doc.element_mut(id).unwrap().cam_mut().unwrap() = s.cam.clone();
    let ctx = Ctx {
        doc: &s.doc,
        cam: &s.cam,
        parts: &[],
        versions: &[],
    };
    assert_eq!(d.problem(&ctx), None);
    for e in d.commit(&ctx, &mut World::new()) {
        history
            .execute(&mut doc, &cadrs_core::cam::EditCam::new(id, e))
            .unwrap();
    }
    doc.element(id).unwrap().cam().unwrap().clone()
}

/// The with-hole plate's part: 60 × 40 × 10 with a Ø16 hole through at (30, 20).
fn plate() -> Vec<cadrs_core::Part> {
    let mut doc = cadrs_core::Document::new("Plate");
    let mut history = cadrs_core::History::new(10);
    let studio = doc.elements[0].id;
    cadrs_core::samples::with_hole::build_in(
        &mut cadrs_core::studio::DocHistory(&mut doc, &mut history),
        studio,
    )
    .unwrap();
    cadrs_core::rebuild::build(&doc.element(studio).unwrap().active_features())
        .parts
        .clone()
}

fn setup_draft(s: &Studio) -> Draft {
    let (job, setup) = s.cam.setup(s.setup).unwrap();
    Draft::Setup {
        job: job.id,
        setup: Box::new(setup.clone()),
        is_new: false,
    }
}

fn tabs(f: &[Field]) -> Vec<String> {
    f.iter()
        .find_map(|x| match x {
            Field::Tabs { labels, .. } => Some(labels.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn has_key(f: &[Field], key: &str) -> bool {
    f.iter().any(|x| match x {
        Field::Number { key: k, .. }
        | Field::Count { key: k, .. }
        | Field::Text { key: k, .. }
        | Field::Select { key: k, .. }
        | Field::Check { key: k, .. }
        | Field::Picks { key: k, .. }
        | Field::Button { key: k, .. }
        | Field::Tabs { key: k, .. } => k == key,
        _ => false,
    })
}

fn errors(f: &[Field]) -> Vec<String> {
    f.iter()
        .filter_map(|x| match x {
            Field::Note { text, error: true } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn params(d: &Draft) -> &Params {
    match d {
        Draft::Toolpath { op, .. } => &op.toolpath().unwrap().params,
        _ => panic!("not a toolpath"),
    }
}

#[test]
fn mill_toolpaths_have_axis_and_collision_tabs_beams_a_beam_tab() {
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let s = studio(mill(false), em.clone());
    let d = toolpath(&s, Pattern::Profile2D, &em, MachineKind::Mill);
    assert_eq!(
        tabs(&fields(&s, &d, 0)),
        ["Tool path", "Link", "Axis", "Collision", "Misc"]
    );
    assert!(has_key(&fields(&s, &d, 2), "axis"));
    let collision = fields(&s, &d, 3);
    for k in ["coll", "coll-shank", "coll-holder", "coll-action"] {
        assert!(has_key(&collision, k), "{k}");
    }
    let plasma = catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Plasma)
        .unwrap();
    let torch = Tool::new(ToolKind::Plasma, 1.5);
    let s = studio(plasma, torch.clone());
    let d = toolpath(&s, Pattern::Cut, &torch, MachineKind::Plasma);
    assert_eq!(
        tabs(&fields(&s, &d, 0)),
        ["Tool path", "Link", "Misc", "Beam"]
    );
    assert!(has_key(&fields(&s, &d, 3), "micro"));
}

#[test]
fn a_tilted_tool_axis_needs_an_indexing_machine() {
    let em = Tool::new(ToolKind::EndMill, 6.0);
    for indexing in [false, true] {
        let s = studio(mill(indexing), em.clone());
        let mut d = toolpath(&s, Pattern::Face, &em, MachineKind::Mill);
        let mut world = World::new();
        // Picking "Normal of a picked face" starts from the work plane's own Z.
        d.set(&mut world, "axis", &Value::Index(1), &s.cam);
        assert_eq!(params(&d).axis, ToolAxis::Direction([0.0, 0.0, 1.0]));
        assert!(errors(&fields(&s, &d, 2)).is_empty());
        // The front face's normal: tilted 90°.
        if let Draft::Toolpath { op, .. } = &mut d
            && let OpKind::Toolpath(tp) = &mut op.kind
        {
            tp.params.axis = ToolAxis::Direction([0.0, -1.0, 0.0]);
        }
        let f = fields(&s, &d, 2);
        assert!(
            f.iter()
                .any(|x| matches!(x, Field::Note { text, .. } if text.contains("90.0°"))),
            "{f:?}"
        );
        assert_eq!(errors(&f).is_empty(), indexing, "{f:?}");
        d.set(&mut world, "axis", &Value::Index(0), &s.cam);
        assert_eq!(params(&d).axis, ToolAxis::WorkPlane);
    }
}

#[test]
fn collision_changes_reach_the_params() {
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let s = studio(mill(false), em.clone());
    let mut d = toolpath(&s, Pattern::Rough3D, &em, MachineKind::Mill);
    let mut world = World::new();
    d.set(&mut world, "coll-holder", &Value::Number(2.5), &s.cam);
    d.set(&mut world, "coll-action", &Value::Index(1), &s.cam);
    d.set(&mut world, "coll", &Value::Bool(false), &s.cam);
    let c = params(&d).collision;
    assert_eq!(c.holder_clearance, 2.5);
    assert_eq!(c.action, CollisionAction::Warn);
    assert!(!c.check);
    // Unchecked, its settings are hidden.
    assert!(!has_key(&fields(&s, &d, 3), "coll-holder"));
}

#[test]
fn machine_post_and_indexing_settings() {
    let s = studio(mill(false), Tool::new(ToolKind::EndMill, 6.0));
    let job = s.cam.jobs[0].id;
    let mut d = Draft::Machine {
        job,
        machine: Box::new(mill(false)),
        from: None,
        by_kind: true,
        from_assembly: false,
        source: None,
        other: Box::default(),
    };
    let f = fields(&s, &d, 0);
    assert!(has_key(&f, "dec-point") && has_key(&f, "indexing"));
    assert!(!has_key(&f, "max-tilt"));
    let mut world = World::new();
    d.set(&mut world, "dec-point", &Value::Bool(false), &s.cam);
    d.set(&mut world, "indexing", &Value::Bool(true), &s.cam);
    d.set(&mut world, "max-tilt", &Value::Number(95.0), &s.cam);
    let Draft::Machine { machine, .. } = &d else {
        unreachable!()
    };
    assert!(!machine.post.decimal_point);
    assert_eq!(machine.indexing.map(|i| i.max_tilt), Some(95.0));
    assert!(has_key(&fields(&s, &d, 0), "max-tilt"));
}

#[test]
fn thread_mill_fields() {
    let tm = Tool::new(ToolKind::ThreadMill, 6.0);
    let s = studio(mill(false), tm.clone());
    let d = toolpath(&s, Pattern::ThreadMill, &tm, MachineKind::Mill);
    let f = fields(&s, &d, 0);
    assert!(has_key(&f, "thread-d") && has_key(&f, "passes"));
    // The holes type offers it with a thread mill.
    assert!(
        f.iter()
            .any(|x| matches!(x, Field::Select { key, options, .. }
            if key == "pattern" && options.iter().any(|o| o == "Thread mill")))
    );
}

#[test]
fn misc_tab_repeats_the_toolpath() {
    use cadrs_core::cam::cadrs_cam::strategy::Repeat;
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let s = studio(mill(false), em.clone());
    let mut d = toolpath(&s, Pattern::Profile2D, &em, MachineKind::Mill);
    let misc = 4;
    assert!(has_key(&fields(&s, &d, misc), "repeat"));
    assert!(!has_key(&fields(&s, &d, misc), "rep-count"));
    let mut world = World::new();
    d.set(&mut world, "repeat", &Value::Index(2), &s.cam);
    d.set(&mut world, "rep-count", &Value::Number(6.0), &s.cam);
    d.set(&mut world, "rep-angle", &Value::Number(60.0), &s.cam);
    assert_eq!(
        params(&d).repeat,
        Repeat::Circular {
            count: 6,
            center: [0.0, 0.0],
            angle: 60.0
        }
    );
    let f = fields(&s, &d, misc);
    assert!(has_key(&f, "rep-cx") && !has_key(&f, "rep-dx"));
}

/// Cutting data worked out for a material: within the machine's spindle, again when the tool
/// changes size, and the operator's own once a value is typed over.
#[test]
fn tool_cutting_data_for_a_material() {
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let s = studio(mill(false), tool);
    let op = s.cam.operation(s.tool_op).unwrap().2.clone();
    let mut d = Draft::Tool {
        setup: s.setup,
        after: None,
        op: Box::new(op),
        is_new: false,
        library: false,
        filter: None,
    };
    let mut world = World::new();
    let cutting = |d: &Draft| match d {
        Draft::Tool { op, .. } => match &op.kind {
            OpKind::Tool(t) => t.tool.cutting,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    assert!(has_key(&fields(&s, &d, 0), "work-material"));
    d.set(&mut world, "work-material", &Value::Index(1), &s.cam);
    let alu = cutting(&d);
    assert_eq!(alu.material, Some(WorkMaterial::Aluminium));
    let max = mill(false).max_spindle;
    assert!(alu.spindle_rpm <= max && alu.spindle_rpm > 0.0);
    assert_eq!(alu.step_down, 3.0);
    // A 10 mm tool: half the depth again.
    d.set(&mut world, "diameter", &Value::Number(10.0), &s.cam);
    assert_eq!(cutting(&d).step_down, 5.0);
    // Typed over: by hand from then on.
    d.set(&mut world, "feed", &Value::Number(900.0), &s.cam);
    let own = cutting(&d);
    assert_eq!((own.feed, own.material), (900.0, None));
    d.set(&mut world, "diameter", &Value::Number(8.0), &s.cam);
    assert_eq!(cutting(&d).feed, 900.0);
}

/// A profile's compensation: offered off the line only, and set on its params.
#[test]
fn profile_compensation_field() {
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let s = studio(mill(false), em.clone());
    let mut d = toolpath(&s, Pattern::Profile2D, &em, MachineKind::Mill);
    let mut world = World::new();
    assert!(has_key(&fields(&s, &d, 0), "comp"));
    d.set(&mut world, "comp", &Value::Index(1), &s.cam);
    assert_eq!(params(&d).compensation, Compensation::Wear);
    d.set(&mut world, "side", &Value::Index(3), &s.cam);
    assert!(!has_key(&fields(&s, &d, 0), "comp"));
    // Not on a pocket.
    let pocket = toolpath(&s, Pattern::Rough2D, &em, MachineKind::Mill);
    assert!(!has_key(&fields(&s, &pocket, 0), "comp"));
}

/// A toolpath started from a library template takes its settings and keeps its own picks; a
/// template saved with a D12 scales its steps to the D6 it's used with and takes the D6's own
/// speeds. Templates the tool can't run aren't offered.
#[test]
fn toolpath_from_a_template() {
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let mut s = studio(mill(false), em.clone());
    let d12 = Tool::new(ToolKind::EndMill, 12.0);
    let mut saved = Params::new(Pattern::Profile2D, &d12, MachineKind::Mill);
    saved.step_down = 2.5;
    saved.step_over = 4.8;
    saved.speeds.feed = 5_000.0;
    saved.compensation = Compensation::Wear;
    saved.drive.edges = vec![Default::default()];
    s.cam
        .library
        .save_template(Template::new("Finish wall", &saved, &d12));
    let drill_tool = Tool::new(ToolKind::Drill, 5.0);
    let drill = Params::new(Pattern::Drill, &drill_tool, MachineKind::Mill);
    s.cam
        .library
        .save_template(Template::new("Spot", &drill, &drill_tool));
    // (Saved without what it machined.)
    assert!(s.cam.library.templates[0].params.drive.edges.is_empty());
    let mut d = toolpath(&s, Pattern::Profile2D, &em, MachineKind::Mill);
    let f = fields(&s, &d, 0);
    let options = f.iter().find_map(|x| match x {
        Field::Select { key, options, .. } if key == "template" => Some(options.clone()),
        _ => None,
    });
    // An end mill can't drill: only the profile template.
    assert_eq!(options.unwrap().len(), 2);
    let mut world = World::new();
    d.set(&mut world, "template", &Value::Index(1), &s.cam);
    let p = params(&d);
    assert_eq!(p.compensation, Compensation::Wear);
    assert!((p.step_down - 1.25).abs() < 1e-9 && (p.step_over - 2.4).abs() < 1e-9);
    assert_eq!(p.speeds.feed, em.cutting.feed);
    match &d {
        Draft::Toolpath { op, .. } => assert_eq!(op.name, "Finish wall"),
        _ => unreachable!(),
    }
}

/// The rotary and swarf patterns are offered where the job's machine has the rotary axes they
/// need: none on a 3-axis mill, the rotary ones with one axis, Swarf with two. An operation
/// left with a pattern its machine can't run any more still shows it, with what is missing.
#[test]
fn multi_axis_patterns_need_the_machines_rotary_axes() {
    let ball = Tool::new(ToolKind::BallMill, 6.0);
    let offered = |machine: Machine, pattern: Pattern| {
        let s = studio(machine, ball.clone());
        let d = toolpath(&s, pattern, &ball, MachineKind::Mill);
        select(&fields(&s, &d, 0), "pattern").0
    };
    let has = |list: &[String], name: &str| list.iter().any(|o| o == name);
    for (machine, rotary, swarf) in [
        (mill(false), false, false),
        (four_axis(), true, false),
        (mill(true), true, true),
    ] {
        let bodies = offered(machine.clone(), Pattern::Parallel);
        assert_eq!(has(&bodies, "Rotary"), rotary, "{bodies:?}");
        assert_eq!(has(&bodies, "Rotary rough"), rotary, "{bodies:?}");
        assert!(has(&bodies, "Parallel"));
        let faces = offered(machine, Pattern::Rough2D);
        assert_eq!(has(&faces, "Swarf"), swarf, "{faces:?}");
    }
    // Chosen from the list, by its place in it.
    let s = studio(mill(true), ball.clone());
    let mut d = toolpath(&s, Pattern::Parallel, &ball, MachineKind::Mill);
    let (options, _) = select(&fields(&s, &d, 0), "pattern");
    let at = options.iter().position(|o| o == "Rotary rough").unwrap();
    d.set(&mut World::new(), "pattern", &Value::Index(at), &s.cam);
    assert_eq!(params(&d).pattern, Pattern::RotaryRough);
    assert!(errors(&fields(&s, &d, 0)).is_empty());
    // The machine changed under a rotary toolpath: it stays what it is, with an error.
    let s = studio(mill(false), ball.clone());
    let d = toolpath(&s, Pattern::Rotary, &ball, MachineKind::Mill);
    let f = fields(&s, &d, 0);
    let (options, chosen) = select(&f, "pattern");
    assert_eq!(options[chosen], "Rotary");
    assert!(
        errors(&f)
            .iter()
            .any(|e| e.contains("Rotary needs a machine with a rotary axis")),
        "{f:?}"
    );
    let s = studio(four_axis(), ball.clone());
    let d = toolpath(&s, Pattern::Swarf, &ball, MachineKind::Mill);
    let f = fields(&s, &d, 0);
    let (options, chosen) = select(&f, "pattern");
    assert_eq!(options[chosen], "Swarf");
    assert!(
        errors(&f)
            .iter()
            .any(|e| e.contains("two rotary axes") && e.contains("has one")),
        "{f:?}"
    );
    // Another pattern picked, it's gone from the list.
    let mut d = d;
    d.set(&mut World::new(), "pattern", &Value::Index(0), &s.cam);
    assert_ne!(params(&d).pattern, Pattern::Swarf);
    let f = fields(&s, &d, 0);
    assert!(!has(&select(&f, "pattern").0, "Swarf") && errors(&f).is_empty());
    // A rotary template is offered only where the machine can run it.
    for (machine, usable) in [(mill(false), false), (four_axis(), true)] {
        let mut s = studio(machine, ball.clone());
        let saved = Params::new(Pattern::Rotary, &ball, MachineKind::Mill);
        s.cam
            .library
            .save_template(Template::new("Worm", &saved, &ball));
        let d = toolpath(&s, Pattern::Parallel, &ball, MachineKind::Mill);
        let f = fields(&s, &d, 0);
        assert_eq!(has_key(&f, "template"), usable);
        if usable {
            assert_eq!(select(&f, "template").0, ["None", "Worm"]);
        }
    }
}

/// A rotary toolpath's own fields: which show for which pattern and style, what each sets,
/// and that ✓ keeps them.
#[test]
fn rotary_fields_reach_the_params() {
    let ball = Tool::new(ToolKind::BallMill, 6.0);
    let s = studio(four_axis(), ball.clone());
    let mut d = toolpath(&s, Pattern::Rotary, &ball, MachineKind::Mill);
    let mut world = World::new();
    let f = fields(&s, &d, 0);
    for k in [
        "rot-axis",
        "rot-style",
        "rot-offset",
        "rot-core",
        "rot-whole",
        "rot-start",
        "stepover",
        "wall",
        "floor",
    ] {
        assert!(has_key(&f, k), "{k}");
    }
    // The whole part round the machine's axis: nothing to pick, no heights, no direction; a
    // spiral goes right round and has no lead.
    for k in [
        "drive",
        "top-from",
        "direction",
        "rot-axis-pick",
        "rot-from",
        "rot-sweep",
        "rot-lead",
    ] {
        assert!(!has_key(&f, k), "{k}");
    }
    let mut set = |d: &mut Draft, k: &str, v: Value| d.set(&mut world, k, &v, &s.cam);
    // A thread: its lead and hand. Without a lead it can't be cut.
    set(&mut d, "rot-style", Value::Index(3));
    assert_eq!(params(&d).rotary.style, RotaryStyle::Thread);
    let f = fields(&s, &d, 0);
    assert!(has_key(&f, "rot-lead") && has_key(&f, "rot-left"));
    assert!(errors(&f).iter().any(|e| e.contains("lead")), "{f:?}");
    set(&mut d, "rot-lead", Value::Number(6.0));
    set(&mut d, "rot-left", Value::Bool(true));
    assert!(errors(&fields(&s, &d, 0)).is_empty());
    // Lines cover a part of the turn.
    set(&mut d, "rot-style", Value::Index(1));
    assert!(has_key(&fields(&s, &d, 0), "rot-sweep"));
    set(&mut d, "rot-sweep", Value::Number(180.0));
    set(&mut d, "rot-start", Value::Number(30.0));
    set(&mut d, "rot-offset", Value::Number(2.0));
    set(&mut d, "rot-core", Value::Number(4.0));
    // Not the whole part: from and to along the axis, the right way round.
    set(&mut d, "rot-whole", Value::Bool(false));
    assert!(has_key(&fields(&s, &d, 0), "rot-from"));
    set(&mut d, "rot-from", Value::Number(45.0));
    set(&mut d, "rot-to", Value::Number(40.0));
    assert!(!errors(&fields(&s, &d, 0)).is_empty());
    set(&mut d, "rot-from", Value::Number(5.0));
    assert!(errors(&fields(&s, &d, 0)).is_empty());
    // An axis of its own, until one is picked along the work plane's X.
    set(&mut d, "rot-axis", Value::Index(1));
    assert!(has_key(&fields(&s, &d, 0), "rot-axis-pick"));
    let r = params(&d).rotary;
    assert_eq!(r.axis, Some(([0.0; 3], [1.0, 0.0, 0.0])));
    assert_eq!(r.style, RotaryStyle::Line);
    assert_eq!((r.lead, r.left_hand), (6.0, true));
    assert_eq!((r.start_angle, r.sweep), (30.0, 180.0));
    assert_eq!((r.offset, r.core), (2.0, 4.0));
    assert_eq!(r.range, Some([5.0, 40.0]));
    // ✓ keeps it all.
    let cam = accepted(&s, &d);
    let Draft::Toolpath { op, .. } = &d else {
        unreachable!()
    };
    let kept = &cam.operation(op.id).unwrap().2.toolpath().unwrap().params;
    assert_eq!(kept.rotary, r);
    // Back to the machine's axis and the whole part.
    set(&mut d, "rot-axis", Value::Index(0));
    set(&mut d, "rot-whole", Value::Bool(true));
    assert_eq!(params(&d).rotary.axis, None);
    assert_eq!(params(&d).rotary.range, None);
    // Roughing: levels of radius, the whole turn, no style.
    let rough = toolpath(&s, Pattern::RotaryRough, &ball, MachineKind::Mill);
    let f = fields(&s, &rough, 0);
    assert!(has_key(&f, "stepdown") && has_key(&f, "stepover") && has_key(&f, "rot-core"));
    assert!(!has_key(&f, "rot-style") && !has_key(&f, "rot-start"));
}

/// Swarf cuts picked walls in depth passes; a drill takes holes at any angle on a machine
/// that can turn the tool to them. Both are held in by the Axis tab's limit.
#[test]
fn swarf_and_any_angle_drilling() {
    let em = Tool::new(ToolKind::EndMill, 6.0);
    let s = studio(mill(true), em.clone());
    let mut d = toolpath(&s, Pattern::Swarf, &em, MachineKind::Mill);
    let f = fields(&s, &d, 0);
    for k in ["drive", "stepdown", "direction", "wall", "floor"] {
        assert!(has_key(&f, k), "{k}");
    }
    assert!(!has_key(&f, "top-from"));
    assert!(
        f.iter().any(
            |x| matches!(x, Field::Picks { key, placeholder, .. } if key == "drive" && placeholder.contains("Walls"))
        ),
        "{f:?}"
    );
    let axis = fields(&s, &d, 2);
    assert!(has_key(&axis, "tilt-max") && !has_key(&axis, "tilt-lead"));
    let mut world = World::new();
    d.set(&mut world, "tilt-max", &Value::Number(35.0), &s.cam);
    assert_eq!(params(&d).tilt.max_tilt, 35.0);
    // Drilling: the check only where the machine has rotary axes.
    let drill = Tool::new(ToolKind::Drill, 5.0);
    let flat = studio(mill(false), drill.clone());
    let d = toolpath(&flat, Pattern::Drill, &drill, MachineKind::Mill);
    assert!(!has_key(&fields(&flat, &d, 0), "any-axis"));
    let s = studio(mill(true), drill.clone());
    let mut d = toolpath(&s, Pattern::Drill, &drill, MachineKind::Mill);
    assert!(has_key(&fields(&s, &d, 0), "any-axis"));
    assert!(!has_key(&fields(&s, &d, 2), "tilt-max"));
    d.set(&mut world, "any-axis", &Value::Bool(true), &s.cam);
    assert!(params(&d).cycle.any_axis);
    assert!(has_key(&fields(&s, &d, 2), "tilt-max"));
    let cam = accepted(&s, &d);
    let Draft::Toolpath { op, .. } = &d else {
        unreachable!()
    };
    assert!(
        cam.operation(op.id)
            .unwrap()
            .2
            .toolpath()
            .unwrap()
            .params
            .cycle
            .any_axis
    );
    // Set on a machine that has lost its axes: still shown, to be taken off, with an error.
    let Draft::Toolpath { op, .. } = &d else {
        unreachable!()
    };
    let moved = Draft::Toolpath {
        setup: flat.setup,
        after: Some(flat.tool_op),
        op: op.clone(),
        is_new: true,
        kind: Type::Holes,
    };
    let f = fields(&flat, &moved, 0);
    assert!(has_key(&f, "any-axis") && !errors(&f).is_empty());
}

/// The Axis tab's one select: the two fixed directions on any machine, the four leaning modes
/// for a pattern that takes them on a machine with rotary axes; each maps to its tool axis and
/// leaning mode, and back to its place in the list.
#[test]
fn the_axis_select_covers_fixed_and_leaning_axes() {
    let ball = Tool::new(ToolKind::BallMill, 6.0);
    let s = studio(mill(true), ball.clone());
    let mut d = toolpath(&s, Pattern::Parallel, &ball, MachineKind::Mill);
    let mut world = World::new();
    let (options, chosen) = select(&fields(&s, &d, 2), "axis");
    assert_eq!(
        options,
        [
            "Work plane Z",
            "Normal of a picked face (3+2)",
            "Normal to the surface",
            "Away from a point",
            "Toward a point",
            "Through a line"
        ]
    );
    assert_eq!(chosen, 0);
    let z = [0.0, 0.0, 1.0];
    let line = AxisMode::ThroughLine {
        point: [0.0; 3],
        dir: [1.0, 0.0, 0.0],
    };
    for (i, axis, mode, keys) in [
        (
            1,
            ToolAxis::Direction(z),
            AxisMode::Fixed,
            &["axis-face"][..],
        ),
        (2, ToolAxis::WorkPlane, AxisMode::Normal, &["tilt-lead"][..]),
        (
            3,
            ToolAxis::WorkPlane,
            AxisMode::FromPoint([0.0; 3]),
            &["axis-point", "tilt-px", "tilt-py", "tilt-pz"][..],
        ),
        // (The point stays from one point mode to the other.)
        (
            4,
            ToolAxis::WorkPlane,
            AxisMode::TowardPoint([0.0; 3]),
            &["axis-point", "tilt-pz"][..],
        ),
        (5, ToolAxis::WorkPlane, line, &["axis-line"][..]),
        (0, ToolAxis::WorkPlane, AxisMode::Fixed, &[][..]),
        (
            4,
            ToolAxis::WorkPlane,
            AxisMode::TowardPoint([0.0, 0.0, 100.0]),
            &["axis-point"][..],
        ),
        (
            1,
            ToolAxis::Direction(z),
            AxisMode::Fixed,
            &["axis-face"][..],
        ),
    ] {
        d.set(&mut world, "axis", &Value::Index(i), &s.cam);
        let p = params(&d);
        assert_eq!((p.axis, p.tilt.mode), (axis, mode), "option {i}");
        assert_eq!(axis_choice(p), i);
        let f = fields(&s, &d, 2);
        assert_eq!(select(&f, "axis").1, i);
        for k in keys {
            assert!(has_key(&f, k), "option {i}: {k}");
        }
        // The lean's own settings with the leaning modes only.
        for k in ["tilt-lead", "tilt-side", "tilt-max", "tilt-smooth"] {
            assert_eq!(has_key(&f, k), i >= 2, "option {i}: {k}");
        }
        assert!(errors(&f).is_empty(), "option {i}: {f:?}");
    }
    // The lean's settings and a point typed in.
    d.set(&mut world, "axis", &Value::Index(4), &s.cam);
    for (k, v) in [
        ("tilt-lead", 5.0),
        ("tilt-side", -3.0),
        ("tilt-max", 30.0),
        ("tilt-smooth", 4.0),
        ("tilt-px", 10.0),
        ("tilt-py", 20.0),
        ("tilt-pz", 80.0),
    ] {
        d.set(&mut world, k, &Value::Number(v), &s.cam);
    }
    let t = params(&d).tilt;
    assert_eq!(t.mode, AxisMode::TowardPoint([10.0, 20.0, 80.0]));
    assert_eq!(
        (t.lead, t.side, t.max_tilt, t.smoothing),
        (5.0, -3.0, 30.0, 4.0)
    );
    let cam = accepted(&s, &d);
    let Draft::Toolpath { op, .. } = &d else {
        unreachable!()
    };
    let kept = &cam.operation(op.id).unwrap().2.toolpath().unwrap().params;
    assert_eq!((kept.tilt, kept.axis), (t, ToolAxis::WorkPlane));
    // The same toolpath on a 3-axis machine: its mode still shows, with why it can't be cut.
    let flat = studio(mill(false), ball.clone());
    let moved = Draft::Toolpath {
        setup: flat.setup,
        after: Some(flat.tool_op),
        op: op.clone(),
        is_new: true,
        kind: Type::Bodies,
    };
    let f = fields(&flat, &moved, 2);
    assert_eq!(select(&f, "axis"), (options.clone(), 4));
    assert!(
        errors(&f).iter().any(|e| e.contains("no rotary axes")),
        "{f:?}"
    );
    // A fresh one there is offered the fixed directions only; so is a pattern that doesn't
    // lean, on any machine.
    let fresh = toolpath(&flat, Pattern::Parallel, &ball, MachineKind::Mill);
    assert_eq!(select(&fields(&flat, &fresh, 2), "axis").0.len(), 2);
    let rough = toolpath(&s, Pattern::Rough3D, &ball, MachineKind::Mill);
    assert_eq!(select(&fields(&s, &rough, 2), "axis").0.len(), 2);
    // One rotary axis is enough to be offered them (a line through the axis is 4-axis work).
    let four = studio(four_axis(), ball.clone());
    let d = toolpath(&four, Pattern::Scallop, &ball, MachineKind::Mill);
    assert_eq!(select(&fields(&four, &d, 2), "axis").0.len(), 6);
}

/// The Machine dialog's rotary group: only for a machine with rotary axes, the tool centre
/// point blocks only with it on, each field on the machine's post, kept by ✓.
#[test]
fn machine_rotary_post_settings() {
    let s = studio(mill(true), Tool::new(ToolKind::EndMill, 6.0));
    let job = s.cam.jobs[0].id;
    let draft = |machine: Machine| Draft::Machine {
        job,
        machine: Box::new(machine),
        from: None,
        by_kind: true,
        from_assembly: false,
        source: None,
        other: Box::default(),
    };
    let keys = [
        "tcp",
        "tilted-plane",
        "rot-feed",
        "rot-tol",
        "rot-axis-tol",
        "rot-retract",
        "angle-decimals",
    ];
    let flat = fields(&s, &draft(mill(false)), 0);
    for k in keys {
        assert!(!has_key(&flat, k), "{k}");
    }
    // A machine with axes of its own has them too, and no catalogue indexing to tick.
    let own = fields(&s, &draft(four_axis()), 0);
    assert!(has_key(&own, "tcp") && !has_key(&own, "indexing"));
    let mut d = draft(mill(true));
    let mut world = World::new();
    let f = fields(&s, &d, 0);
    for k in keys.iter().chain(&["indexing", "max-tilt", "rotary"]) {
        assert!(has_key(&f, k), "{k}");
    }
    d.set(&mut world, "tcp", &Value::Bool(false), &s.cam);
    assert!(!has_key(&fields(&s, &d, 0), "tcp-on"));
    d.set(&mut world, "tcp", &Value::Bool(true), &s.cam);
    let f = fields(&s, &d, 0);
    assert!(has_key(&f, "tcp-on") && has_key(&f, "tcp-off"));
    for (k, v) in [
        ("tcp-on", Value::Text("G43.4 H{h}; M128".into())),
        ("tcp-off", Value::Text("M129".into())),
        ("tilted-plane", Value::Index(1)),
        ("rot-feed", Value::Index(2)),
        ("rot-tol", Value::Number(0.02)),
        ("rot-axis-tol", Value::Number(0.25)),
        ("rot-retract", Value::Number(40.0)),
        ("angle-decimals", Value::Number(4.0)),
    ] {
        d.set(&mut world, k, &v, &s.cam);
    }
    let Draft::Machine { machine, .. } = &d else {
        unreachable!()
    };
    let r = machine.post.rotary.clone();
    assert!(r.tcp);
    assert_eq!(
        (r.tcp_on.as_str(), r.tcp_off.as_str()),
        ("G43.4 H{h}; M128", "M129")
    );
    assert_eq!(r.tilted_plane, TiltedPlane::Positions);
    assert_eq!(r.feed, RotaryFeed::AllAxes);
    assert_eq!(
        (r.tolerance, r.axis_tolerance, r.retract),
        (0.02, 0.25, 40.0)
    );
    assert_eq!(r.angle_decimals, 4);
    assert_eq!(select(&fields(&s, &d, 0), "rot-feed").1, 2);
    let cam = accepted(&s, &d);
    assert_eq!(cam.jobs[0].machine.as_ref().unwrap().post.rotary, r);
    // A controller with no tool centre point code of its own says so until one is typed.
    let mut grbl = mill(true);
    grbl.dialect = cadrs_core::cam::cadrs_cam::Dialect::Grbl;
    grbl.post.rotary.tcp = true;
    grbl.post.rotary.tcp_on = String::new();
    let mut d = draft(grbl);
    assert_eq!(errors(&fields(&s, &d, 0)).len(), 1);
    d.set(&mut world, "tcp-on", &Value::Text("M128".into()), &s.cam);
    assert!(errors(&fields(&s, &d, 0)).is_empty());
}

/// A setup's third mating: a frame on the machine's mount, by its numbers; ✓ keeps it, and a
/// frame with no Z can't be kept.
#[test]
fn setup_mating_on_the_mount() {
    let s = studio(four_axis(), Tool::new(ToolKind::EndMill, 6.0));
    let mut d = setup_draft(&s);
    let mut world = World::new();
    let (options, chosen) = select(&fields(&s, &d, 0), "mating");
    assert_eq!((options.len(), chosen), (3, 0));
    assert!(!has_key(&fields(&s, &d, 0), "mount-face"));
    d.set(&mut world, "mating", &Value::Index(2), &s.cam);
    let mating = |d: &Draft| match d {
        Draft::Setup { setup, .. } => setup.mating,
        _ => unreachable!(),
    };
    // Until it's set: the setup's own origin and axes on the mount's.
    assert_eq!(
        mating(&d),
        Mating::Mount {
            origin: [0.0; 3],
            z: [0.0, 0.0, 1.0],
            x: [1.0, 0.0, 0.0]
        }
    );
    let f = fields(&s, &d, 0);
    assert_eq!(select(&f, "mating").1, 2);
    assert!(has_key(&f, "mount-face"));
    for stem in ["mount-o", "mount-z", "mount-x"] {
        for i in 0..3 {
            assert!(has_key(&f, &format!("{stem}-{i}")), "{stem}-{i}");
        }
    }
    // A chuck's face on the part's −X end: Z along the part, X up.
    for (k, v) in [
        ("mount-o-0", -30.0),
        ("mount-o-2", 12.5),
        ("mount-z-0", 1.0),
        ("mount-z-2", 0.0),
        ("mount-x-2", 1.0),
        ("mount-x-0", 0.0),
    ] {
        d.set(&mut world, k, &Value::Number(v), &s.cam);
    }
    let want = Mating::Mount {
        origin: [-30.0, 0.0, 12.5],
        z: [1.0, 0.0, 0.0],
        x: [0.0, 0.0, 1.0],
    };
    assert_eq!(mating(&d), want);
    let cam = accepted(&s, &d);
    assert_eq!(cam.setup(s.setup).unwrap().1.mating, want);
    // No Z: ✓ is off until it has one.
    d.set(&mut world, "mount-z-0", &Value::Number(0.0), &s.cam);
    assert_eq!(errors(&fields(&s, &d, 0)).len(), 1);
    d.set(&mut world, "mount-z-1", &Value::Number(-1.0), &s.cam);
    assert!(errors(&fields(&s, &d, 0)).is_empty());
    // Chosen again it stays; the other matings as before.
    let before = mating(&d);
    d.set(&mut world, "mating", &Value::Index(2), &s.cam);
    assert_eq!(mating(&d), before);
    d.set(&mut world, "mating", &Value::Index(1), &s.cam);
    assert_eq!(mating(&d), Mating::Manual([0.0; 3]));
    d.set(&mut world, "mating", &Value::Index(0), &s.cam);
    assert_eq!(mating(&d), Mating::Automatic);
}

/// Picks on the with-hole plate: the hole's face gives a rotary pattern's axis and the line a
/// tool leans through, a vertex the point it leans by, the bottom face a setup's frame on the
/// mount; a pick a list can't use is left for the next.
#[test]
fn picks_set_axes_points_and_the_mount_frame() {
    let parts = plate();
    let part = &parts[0];
    let hole = part
        .solid
        .faces
        .iter()
        .find(|f| f.axis.is_some())
        .expect("the hole's face");
    let normal = |f: &cadrs_core::solid::SolidFace| {
        f.plane
            .map(|p| cadrs_core::cam::cadrs_cam::geom::cross(p.u, p.v))
    };
    let bottom = part
        .solid
        .faces
        .iter()
        .find(|f| normal(f).is_some_and(|n| n[2] < -0.5))
        .expect("the bottom face");
    let corner = &part.solid.vertices[0];
    let ball = Tool::new(ToolKind::BallMill, 6.0);
    let s = studio(mill(true), ball.clone());
    let ctx = Ctx {
        doc: &s.doc,
        cam: &s.cam,
        parts: &parts,
        versions: &[],
    };
    let frame = Some(Frame::WORLD);
    let hole_pick = Pick::Face(part.id, hole.name);
    let bottom_pick = Pick::Face(part.id, bottom.name);
    let corner_pick = Pick::Vertex(part.id, corner.name);
    // A rotary pattern's axis: the hole's, pointing up (the positive way of the axis it's
    // along), through (30, 20).
    let mut d = toolpath(&s, Pattern::Rotary, &ball, MachineKind::Mill);
    assert!(!d.pick(&ctx, "rot-axis-pick", bottom_pick, frame));
    assert_eq!(params(&d).rotary.axis, None);
    assert!(d.pick(&ctx, "rot-axis-pick", hole_pick, frame));
    let (point, dir) = params(&d).rotary.axis.unwrap();
    assert!((point[0] - 30.0).abs() < 1e-9 && (point[1] - 20.0).abs() < 1e-9);
    assert_eq!(dir, [0.0, 0.0, 1.0]);
    // In the operation's frame: seen from one a step along X, the point is a step back.
    let moved = Frame {
        origin: [10.0, 0.0, 0.0],
        ..Frame::WORLD
    };
    assert!(d.pick(&ctx, "rot-axis-pick", hole_pick, Some(moved)));
    assert!((params(&d).rotary.axis.unwrap().0[0] - 20.0).abs() < 1e-9);
    // The line a tool leans through, and the point it leans toward.
    let mut d = toolpath(&s, Pattern::Parallel, &ball, MachineKind::Mill);
    let mut world = World::new();
    d.set(&mut world, "axis", &Value::Index(5), &s.cam);
    assert!(!d.pick(&ctx, "axis-line", corner_pick, frame));
    assert!(d.pick(&ctx, "axis-line", hole_pick, frame));
    assert_eq!(params(&d).tilt.mode, AxisMode::ThroughLine { point, dir });
    d.set(&mut world, "axis", &Value::Index(4), &s.cam);
    assert!(!d.pick(&ctx, "axis-point", hole_pick, frame));
    assert!(d.pick(&ctx, "axis-point", corner_pick, frame));
    assert_eq!(params(&d).tilt.mode, AxisMode::TowardPoint(corner.point));
    // The bottom face on the mount: the frame at its centre, Z up into the part, in the
    // setup's coordinates (its origin here 10 along X).
    let mut d = setup_draft(&s);
    d.set(&mut world, "p-origin", &Value::Index(3), &s.cam);
    d.set(&mut world, "p-man-0", &Value::Number(10.0), &s.cam);
    assert!(!d.pick(&ctx, "mount-face", corner_pick, None));
    assert!(d.pick(&ctx, "mount-face", bottom_pick, None));
    let Draft::Setup { setup, .. } = &d else {
        unreachable!()
    };
    let Mating::Mount { origin, z, x } = setup.mating else {
        panic!("{:?}", setup.mating)
    };
    assert_eq!(z, [0.0, 0.0, 1.0]);
    assert!((origin[0] - 20.0).abs() < 1e-6 && (origin[1] - 20.0).abs() < 1e-6);
    assert!((origin[2] - bottom.plane.unwrap().origin[2]).abs() < 1e-9);
    assert!(x[2].abs() < 1e-9 && (x[0].hypot(x[1]) - 1.0).abs() < 1e-9);
    // (The work plane's Z pick is as it was.)
    assert!(d.pick(&ctx, "p-face", bottom_pick, None));
    let Draft::Setup { setup, .. } = &d else {
        unreachable!()
    };
    assert_eq!(setup.wcs.z, [0.0, 0.0, -1.0]);
}

fn ctx(s: &Studio) -> Ctx<'_> {
    Ctx {
        doc: &s.doc,
        cam: &s.cam,
        parts: &[],
        versions: &[],
    }
}

fn tool_draft(s: &Studio, tool: Tool) -> Draft {
    Draft::Tool {
        setup: s.setup,
        after: None,
        op: Box::new(Operation {
            id: CamId::new(),
            name: tool.name.clone(),
            kind: OpKind::Tool(Box::new(ToolOp { tool, number: 2 })),
            suppressed: false,
            hidden: false,
        }),
        is_new: true,
        library: false,
        filter: None,
    }
}

fn tool_of(d: &Draft) -> &Tool {
    match d {
        Draft::Tool { op, .. } => &op.tool().unwrap().tool,
        _ => panic!("a tool draft"),
    }
}

/// The form tools' sizes: a taper ball's ball and angle, a lollipop's neck, a barrel's side
/// radius, each shown for its kind alone and taken by its key.
#[test]
fn form_tool_kinds_show_their_sizes() {
    let s = studio(mill(true), Tool::new(ToolKind::EndMill, 6.0));
    let mut world = World::new();
    let mut set = |d: &mut Draft, key: &str, v: Value| d.set(&mut world, key, &v, &s.cam);
    for (kind, has, lacks) in [
        (
            ToolKind::TaperBall,
            vec!["diameter", "tip", "angle"],
            vec!["neck", "profile-radius"],
        ),
        (
            ToolKind::Lollipop,
            vec!["diameter", "neck"],
            vec!["tip", "profile-radius"],
        ),
        (
            ToolKind::Barrel,
            vec!["diameter", "profile-radius", "cut-len"],
            vec!["neck", "tip"],
        ),
        (
            ToolKind::Form,
            vec!["form-r-0", "form-h-1", "form-add"],
            vec!["diameter", "cut-len", "form-b-0"],
        ),
    ] {
        let d = tool_draft(&s, Tool::new(kind, 10.0));
        let f = fields(&s, &d, 0);
        for k in has {
            assert!(has_key(&f, k), "{kind:?} shows {k}");
        }
        for k in lacks {
            assert!(!has_key(&f, k), "{kind:?} hides {k}");
        }
        assert!(
            d.problem(&ctx(&s)).is_none(),
            "{kind:?}: a new one is sound"
        );
    }
    let mut d = tool_draft(&s, Tool::new(ToolKind::TaperBall, 10.0));
    set(&mut d, "tip", Value::Number(3.0));
    set(&mut d, "angle", Value::Number(20.0));
    assert_eq!((tool_of(&d).tip_diameter, tool_of(&d).angle), (3.0, 20.0));
    // A ball wider than the shank is refused, in the tool's own words.
    set(&mut d, "tip", Value::Number(12.0));
    assert!(d.problem(&ctx(&s)).is_some_and(|e| e.contains("ball tip")));
    let mut d = tool_draft(&s, Tool::new(ToolKind::Lollipop, 10.0));
    set(&mut d, "neck", Value::Number(4.0));
    assert_eq!(tool_of(&d).neck_diameter, 4.0);
    set(&mut d, "neck", Value::Number(10.0));
    assert!(d.problem(&ctx(&s)).is_some_and(|e| e.contains("neck")));
    let mut d = tool_draft(&s, Tool::new(ToolKind::Barrel, 10.0));
    set(&mut d, "profile-radius", Value::Number(80.0));
    assert_eq!(tool_of(&d).profile_radius, 80.0);
    assert!(d.problem(&ctx(&s)).is_none());
    set(&mut d, "profile-radius", Value::Number(1.0));
    assert!(
        d.problem(&ctx(&s))
            .is_some_and(|e| e.contains("side radius"))
    );
}

/// A form tool's profile is edited a point at a time: the diameter and the cutting length
/// follow it, a broken profile says what is wrong, and two points are always left.
#[test]
fn a_form_tools_profile_is_edited_by_points() {
    let s = studio(mill(true), Tool::new(ToolKind::EndMill, 6.0));
    let mut world = World::new();
    let mut set = |d: &mut Draft, key: &str, v: Value| d.set(&mut world, key, &v, &s.cam);
    let mut d = tool_draft(&s, Tool::new(ToolKind::Form, 20.0));
    let n = tool_of(&d).form.as_ref().unwrap().points.len();
    assert_eq!(n, 4, "a T-slot cutter to start with");
    // A wider disc: the diameter follows.
    set(&mut d, "form-r-0", Value::Number(15.0));
    set(&mut d, "form-r-1", Value::Number(15.0));
    assert_eq!(tool_of(&d).diameter, 30.0);
    // A point added straight above the last: the cutting length follows.
    let top = tool_of(&d).cutting_length;
    set(&mut d, "form-add", Value::Activate);
    let t = tool_of(&d);
    let pts = &t.form.as_ref().unwrap().points;
    assert_eq!(pts.len(), n + 1);
    assert!(pts[n].h > top && pts[n].r == pts[n - 1].r && t.cutting_length == pts[n].h);
    set(&mut d, "form-h-4", Value::Number(40.0));
    set(&mut d, "form-b-4", Value::Number(0.2));
    let t = tool_of(&d);
    assert_eq!(
        (t.cutting_length, t.form.as_ref().unwrap().points[4].bulge),
        (40.0, 0.2)
    );
    set(&mut d, "form-b-4", Value::Number(0.0));
    assert!(d.problem(&ctx(&s)).is_none(), "{:?}", d.problem(&ctx(&s)));
    // A point below the one before it: the profile's own error shows, and ✓ is off.
    set(&mut d, "form-h-4", Value::Number(1.0));
    let f = fields(&s, &d, 0);
    assert!(
        f.iter()
            .any(|x| matches!(x, Field::Note { error: true, .. })),
        "the profile's error is listed"
    );
    assert!(d.problem(&ctx(&s)).is_some());
    // Removed again, down to two points and no further.
    set(&mut d, "form-del-4", Value::Activate);
    assert!(d.problem(&ctx(&s)).is_none());
    for _ in 0..5 {
        set(&mut d, "form-del-0", Value::Activate);
    }
    assert_eq!(tool_of(&d).form.as_ref().unwrap().points.len(), 2);
    assert!(!has_key(&fields(&s, &d, 0), "form-del-0"));
    // A key that names no point changes nothing.
    let before = tool_of(&d).clone();
    set(&mut d, "form-r-9", Value::Number(3.0));
    assert_eq!(*tool_of(&d), before);
}
