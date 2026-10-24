//! The dialogs' drafts without the app: the toolpath dialog's tabs, the Axis and Collision
//! tabs' fields and what their changes do, the machine's post and indexing settings, and a
//! thread mill's fields.

use cadrs_core::cam::cadrs_cam::strategy::ToolAxis;
use cadrs_core::cam::cadrs_cam::strategy::collision::CollisionAction;

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
