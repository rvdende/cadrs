//! The toolpath dialog's fields: its Tool path, Link, Axis and Collision tabs (the Misc and Beam
//! tabs are laid out with them in [`Draft::fields`]).

use super::*;

/// The Axis tab: the tool's axis, the work plane's or a picked face's normal (3+2).
pub(super) fn axis_fields(
    f: &mut Vec<Field>,
    ctx: &Ctx,
    setup: CamId,
    p: &Params,
    picking: Option<&str>,
) {
    use cadrs_core::cam::cadrs_cam::strategy::ToolAxis;
    f.push(Field::section("Tool axis"));
    f.push(Field::select(
        "axis",
        "Tool axis",
        vec!["Work plane Z", "Normal of a picked face"],
        usize::from(p.axis != ToolAxis::WorkPlane),
    ));
    let ToolAxis::Direction(z) = p.axis else {
        f.push(Field::note(
            "The tool cuts along the Z of the work plane above the toolpath (or the setup's).",
        ));
        return;
    };
    f.push(Field::picks(
        "axis-face",
        "Planar face for the tool axis",
        vec![format!("Z ({:.3}, {:.3}, {:.3})", z[0], z[1], z[2])],
        picking == Some("axis-face"),
    ));
    // How far it tilts from the setup's Z, and whether the machine can.
    let Some((job, s)) = ctx.cam.setup(setup) else {
        return;
    };
    let frame = cadrs_core::cam::program::setup_frame(ctx.cam, job, s, ctx.parts);
    let local = frame.dir_to_local(z);
    let tilt = local[2].clamp(-1.0, 1.0).acos().to_degrees();
    f.push(Field::note(format!(
        "Tilted {tilt:.1}° from the setup's Z: cut in a tilted working plane (3+2)"
    )));
    if let Some(m) = &job.machine
        && !m.can_tilt_to(local)
    {
        f.push(Field::error(if m.indexing.is_none() {
            "The machine has no indexing axes to tilt the tool"
        } else {
            "The machine can't tilt the tool that far"
        }));
    }
}

/// The Collision tab: the shank and holder checked against the part.
pub(super) fn collision_fields(f: &mut Vec<Field>, p: &Params, tool: Option<&Tool>) {
    use cadrs_core::cam::cadrs_cam::strategy::collision::CollisionAction;
    let c = &p.collision;
    f.push(Field::section("Tool assembly"));
    f.push(Field::check("coll", "Check the shank and holder", c.check));
    if !c.check {
        return;
    }
    f.push(Field::number(
        "coll-shank",
        "Shank clearance",
        c.shank_clearance,
        Unit::Mm,
    ));
    f.push(Field::number(
        "coll-holder",
        "Holder clearance",
        c.holder_clearance,
        Unit::Mm,
    ));
    f.push(Field::select(
        "coll-action",
        "Where they'd hit",
        labels(&CollisionAction::ALL, CollisionAction::label),
        index_of(&CollisionAction::ALL, c.action),
    ));
    if let Some(t) = tool {
        f.push(Field::note(match &t.holder {
            Some(h) => format!(
                "{}: flutes {} mm, stick-out {} mm, in {}",
                t.name,
                fmt(t.cutting_length),
                fmt(t.overall_length),
                h.name
            ),
            None => format!(
                "{}: flutes {} mm, stick-out {} mm, no holder",
                t.name,
                fmt(t.cutting_length),
                fmt(t.overall_length)
            ),
        }));
    }
}

/// The Tool path tab: the name, type and pattern, the drive geometry and the pattern's own
/// settings.
#[allow(clippy::too_many_arguments)]
pub(super) fn toolpath_fields(
    f: &mut Vec<Field>,
    ctx: &Ctx,
    op: &Operation,
    p: &Params,
    kind: Type,
    tk: ToolKind,
    machine: MachineKind,
    picking: Option<&str>,
) {
    f.push(Field::text("name", "Name", &op.name));
    // Start from a template saved in the library (one this tool and machine can run).
    let templates = usable_templates(ctx.cam, tk, machine);
    if !templates.is_empty() {
        let mut opts = vec!["None".to_string()];
        opts.extend(templates.iter().map(|t| t.name.clone()));
        f.push(Field::select("template", "Start from template", opts, 0));
    }
    let types: Vec<Type> = Type::ALL
        .into_iter()
        .filter(|t| !Pattern::available(*t, tk, machine).is_empty())
        .collect();
    f.push(Field::select(
        "type",
        "Type",
        labels(&types, Type::label),
        index_of(&types, kind),
    ));
    let pats = Pattern::available(kind, tk, machine);
    f.push(Field::select(
        "pattern",
        "Pattern",
        labels(&pats, Pattern::label),
        index_of(&pats, p.pattern),
    ));
    use Pattern::*;
    let pat = p.pattern;
    if matches!(pat, Rough2D | Rough3D) {
        f.push(Field::select(
            "sub",
            "Sub-pattern",
            labels(&SubPattern::ALL, SubPattern::label),
            index_of(&SubPattern::ALL, p.sub_pattern),
        ));
    }
    // Drive geometry.
    let (placeholder, items): (&str, Vec<String>) = match kind {
        Type::Edges => (
            "Drive edges (the part's outline when empty)",
            p.drive
                .edges
                .iter()
                .map(|k| entity_label(ctx, k, "Edge"))
                .collect(),
        ),
        Type::Faces => (
            match pat {
                Fill => "Faces to fill",
                Face => "Faces (the stock's top when empty)",
                _ => "Floor faces (all when empty)",
            },
            p.drive
                .faces
                .iter()
                .map(|k| entity_label(ctx, k, "Face"))
                .collect(),
        ),
        Type::Bodies => (
            "Faces to finish (all when empty)",
            p.drive
                .faces
                .iter()
                .map(|k| entity_label(ctx, k, "Face"))
                .collect(),
        ),
        Type::Holes => (
            "Holes (by the cycle's selection when empty)",
            p.drive
                .holes
                .iter()
                .map(|h| format!("Hole at ({}, {})", fmt(h[0]), fmt(h[1])))
                .collect(),
        ),
    };
    if pat != Engrave || kind == Type::Edges {
        f.push(Field::picks(
            "drive",
            placeholder,
            items,
            picking == Some("drive"),
        ));
    }
    if kind == Type::Edges && !p.drive.edges.is_empty() {
        f.push(Field::check(
            "reverse",
            "Reverse direction",
            p.drive.reverse,
        ));
    }
    if matches!(pat, Profile2D | Cut) {
        f.push(Field::select(
            "side",
            "Side",
            labels(&Side::ALL, Side::label),
            index_of(&Side::ALL, p.side),
        ));
    }
    if pat == Profile2D && p.side != Side::On {
        f.push(Field::select(
            "comp",
            "Compensation",
            labels(&Compensation::ALL, Compensation::label),
            index_of(&Compensation::ALL, p.compensation),
        ));
    }
    let beam = tk.is_beam();
    if !beam && !matches!(pat, Drill) {
        f.push(Field::section("Offset"));
        if !matches!(pat, Face | Engrave | Chamfer2D) {
            f.push(Field::number(
                "wall",
                "Wall stock to leave",
                p.wall_stock,
                Unit::Mm,
            ));
        }
        if !matches!(pat, Engrave | Chamfer2D) {
            f.push(Field::number(
                "floor",
                "Floor stock to leave",
                p.floor_stock,
                Unit::Mm,
            ));
        }
    } else if beam && pat == Cut {
        f.push(Field::number(
            "wall",
            "Extra offset",
            p.wall_stock,
            Unit::Mm,
        ));
    }
    if matches!(
        pat,
        Rough2D | Rough3D | Adaptive | Profile2D | ConstantZ | Face | Engrave | Bore
    ) {
        f.push(Field::section("Step down"));
        f.push(Field::number(
            "stepdown",
            if pat == Bore {
                "Helix pitch"
            } else {
                "Step down"
            },
            p.step_down,
            Unit::Mm,
        ));
        if matches!(pat, Rough2D | Rough3D | Profile2D) {
            f.push(Field::number(
                "first",
                "First step down (0: same)",
                p.first_step_down,
                Unit::Mm,
            ));
        }
    }
    if matches!(
        pat,
        Rough2D
            | Rough3D
            | Adaptive
            | Face
            | Flatlands
            | Parallel
            | Scallop
            | Spiral
            | Fill
            | Profile2D
    ) {
        f.push(Field::section("Step over"));
        f.push(Field::number(
            "stepover",
            if pat == Fill {
                "Line spacing"
            } else {
                "Step over"
            },
            p.step_over,
            Unit::Mm,
        ));
        if pat == Profile2D {
            f.push(Field::count("passes", "Passes", p.passes as i64));
        }
        if pat == Scallop {
            f.push(Field::number(
                "steep",
                "Steep angle (walls left to Constant Z)",
                p.steep_angle,
                Unit::Deg,
            ));
        }
    }
    if pat == ThreadMill {
        f.push(Field::section("Thread"));
        f.push(Field::number(
            "thread-d",
            "Major diameter (0: from the hole)",
            p.thread_diameter,
            Unit::Mm,
        ));
        f.push(Field::count("passes", "Radial passes", p.passes as i64));
        f.push(Field::check(
            "left-thread",
            "Left-hand thread",
            p.left_hand_thread,
        ));
        f.push(Field::note(
            "The tool's pitch sets the thread's; climb milling follows the spindle's direction \
             (a left-hand tool turns M4).",
        ));
    }
    if !matches!(pat, Drill) {
        f.push(Field::section("Sorting"));
        f.push(Field::select(
            "direction",
            if beam {
                "Direction"
            } else {
                "Cutting direction"
            },
            if beam {
                vec![
                    "Part on the right".to_string(),
                    "Part on the left".to_string(),
                ]
            } else {
                labels(&Direction::ALL, Direction::label)
            },
            index_of(&Direction::ALL, p.direction),
        ));
        let rastered = matches!(pat, Face | Parallel | Fill)
            || (matches!(pat, Rough2D | Rough3D) && p.sub_pattern == SubPattern::Parallel);
        if rastered {
            f.push(Field::select(
                "method",
                "Cutting method",
                labels(&CuttingMethod::ALL, CuttingMethod::label),
                index_of(&CuttingMethod::ALL, p.cutting_method),
            ));
            f.push(Field::number("angle", "Angle", p.angle, Unit::Deg));
        }
        if matches!(pat, Rough2D | Rough3D | Profile2D) {
            f.push(Field::select(
                "machine-by",
                "Machine by",
                labels(&MachineBy::ALL, MachineBy::label),
                index_of(&MachineBy::ALL, p.machine_by),
            ));
        }
    }
    if !beam && !matches!(pat, Drill | Bore | Chamfer2D | Engrave) {
        f.push(Field::section("Heights"));
        for (k, name, h) in [
            ("top", "Top", p.heights.top),
            ("bottom", "Bottom", p.heights.bottom),
        ] {
            f.push(Field::select(
                &format!("{k}-from"),
                &format!("{name} from"),
                labels(&HeightRef::ALL, HeightRef::label),
                index_of(&HeightRef::ALL, h.from),
            ));
            f.push(Field::number(
                &format!("{k}-off"),
                &format!("{name} offset"),
                h.offset,
                Unit::Mm,
            ));
        }
    }
    match pat {
        Chamfer2D => {
            f.push(Field::section("Chamfer"));
            f.push(Field::number("depth", "Chamfer width", p.depth, Unit::Mm));
            f.push(Field::number(
                "tip-off",
                "Tip offset",
                p.tip_offset,
                Unit::Mm,
            ));
        }
        Engrave => {
            f.push(Field::section("Engrave"));
            f.push(Field::number("depth", "Depth", p.depth, Unit::Mm));
        }
        Profile2D => {
            f.push(Field::section("Tabs"));
            f.push(Field::check("tabs", "Tabs", p.tabs.enabled));
            if p.tabs.enabled {
                f.push(Field::count(
                    "tab-count",
                    "Tabs per contour",
                    p.tabs.count as i64,
                ));
                f.push(Field::number("tab-w", "Tab width", p.tabs.width, Unit::Mm));
                f.push(Field::number(
                    "tab-h",
                    "Tab height",
                    p.tabs.height,
                    Unit::Mm,
                ));
            }
        }
        Drill | Bore => {
            f.push(Field::section(if pat == Drill { "Cycle" } else { "Holes" }));
            f.push(Field::select(
                "hole-select",
                "Holes",
                labels(&HoleSelect::ALL, HoleSelect::label),
                index_of(&HoleSelect::ALL, p.cycle.select),
            ));
            if pat == Drill {
                let cycles = [
                    CycleKind::Drill,
                    CycleKind::Peck,
                    CycleKind::ChipBreak,
                    CycleKind::Tap,
                ];
                f.push(Field::select(
                    "cycle",
                    "Cycle",
                    labels(&cycles, CycleKind::label),
                    index_of(&cycles, p.cycle.kind),
                ));
                if matches!(p.cycle.kind, CycleKind::Peck | CycleKind::ChipBreak) {
                    f.push(Field::number("peck", "Peck depth", p.cycle.peck, Unit::Mm));
                }
                f.push(Field::number(
                    "dwell",
                    "Dwell",
                    p.cycle.dwell,
                    Unit::Seconds,
                ));
                f.push(Field::check(
                    "retract-full",
                    "Retract to clearance between holes",
                    p.cycle.retract_full,
                ));
            }
            f.push(Field::number(
                "break",
                "Break through",
                p.cycle.break_through,
                Unit::Mm,
            ));
        }
        _ => {}
    }
    f.push(Field::section("Speeds"));
    if !beam {
        f.push(Field::number(
            "rpm",
            "Spindle speed",
            p.speeds.spindle_rpm,
            Unit::Rpm,
        ));
    }
    f.push(Field::number(
        "feed",
        "Feed rate",
        p.speeds.feed,
        Unit::Feed,
    ));
    if !beam {
        f.push(Field::number(
            "plunge",
            "Plunge rate",
            p.speeds.plunge,
            Unit::Feed,
        ));
        if !matches!(pat, Drill) {
            f.push(Field::number(
                "ramp-feed",
                "Ramp rate",
                p.speeds.ramp,
                Unit::Feed,
            ));
        }
    }
    f.push(Field::number(
        "lead-feed",
        "Lead rate",
        p.speeds.lead,
        Unit::Feed,
    ));
    f.push(Field::section("Posting"));
    if !beam {
        f.push(Field::select(
            "coolant",
            "Coolant",
            labels(&Coolant::ALL, Coolant::label),
            index_of(&Coolant::ALL, p.posting.coolant),
        ));
        f.push(Field::check(
            "force-tc",
            "Force tool change",
            p.posting.force_tool_change,
        ));
    }
    f.push(Field::check(
        "stop",
        "Stop before (M0)",
        p.posting.stop_before,
    ));
    f.push(Field::text("comment", "Comment", &p.posting.comment));
    if matches!(pat, Rough2D | Rough3D | Adaptive) {
        f.push(Field::check(
            "rest",
            "Rest machining (what the operations above left)",
            p.rest,
        ));
    }
}

pub(super) fn link_fields(f: &mut Vec<Field>, p: &Params, beam: bool) {
    let l = &p.links;
    f.push(Field::section("Clearance area"));
    f.push(Field::number(
        "clearance",
        "Clearance above stock",
        l.clearance,
        Unit::Mm,
    ));
    if !beam {
        f.push(Field::number(
            "rapid-d",
            "Rapid distance",
            l.rapid_distance,
            Unit::Mm,
        ));
        f.push(Field::number(
            "feed-d",
            "Feed distance",
            l.feed_distance,
            Unit::Mm,
        ));
        f.push(Field::section("Gaps"));
        f.push(Field::number(
            "gap",
            "Small gaps up to",
            l.small_gap,
            Unit::Mm,
        ));
        let acts = labels(&LinkAction::ALL, LinkAction::label);
        f.push(Field::select(
            "small",
            "Small gaps",
            acts.clone(),
            index_of(&LinkAction::ALL, l.small_gaps),
        ));
        f.push(Field::select(
            "large",
            "Large gaps",
            acts,
            index_of(&LinkAction::ALL, l.large_gaps),
        ));
        f.push(Field::section("First entry"));
        f.push(Field::select(
            "entry",
            "Entry",
            labels(&Entry::ALL, Entry::label),
            index_of(&Entry::ALL, l.entry),
        ));
        if l.entry == Entry::Ramp {
            f.push(Field::number(
                "ramp-angle",
                "Ramp angle",
                l.ramp_angle,
                Unit::Deg,
            ));
        }
    }
    f.push(Field::section("Leads"));
    let leads = labels(&LeadKind::ALL, LeadKind::label);
    f.push(Field::select(
        "lead-in",
        "Lead in",
        leads.clone(),
        index_of(&LeadKind::ALL, l.lead_in),
    ));
    f.push(Field::select(
        "lead-out",
        "Lead out",
        leads,
        index_of(&LeadKind::ALL, l.lead_out),
    ));
    if l.lead_in != LeadKind::None || l.lead_out != LeadKind::None {
        f.push(Field::number(
            "lead-r",
            "Lead radius",
            l.lead_radius,
            Unit::Mm,
        ));
    }
    f.push(Field::number("overlap", "Overlap", l.overlap, Unit::Mm));
}

/// The Misc tab's pattern: the toolpath repeated in a line or round a centre.
pub(super) fn repeat_fields(f: &mut Vec<Field>, r: &cadrs_core::cam::cadrs_cam::strategy::Repeat) {
    use cadrs_core::cam::cadrs_cam::strategy::Repeat;
    f.push(Field::section("Pattern"));
    f.push(Field::select(
        "repeat",
        "Repeat",
        vec!["None", "Linear", "Circular"],
        match r {
            Repeat::None => 0,
            Repeat::Linear { .. } => 1,
            Repeat::Circular { .. } => 2,
        },
    ));
    match *r {
        Repeat::None => {}
        Repeat::Linear { count, step } => {
            f.push(Field::count("rep-count", "Instances", count as i64));
            f.push(Field::number("rep-dx", "Spacing X", step[0], Unit::Mm));
            f.push(Field::number("rep-dy", "Spacing Y", step[1], Unit::Mm));
        }
        Repeat::Circular {
            count,
            center,
            angle,
        } => {
            f.push(Field::count("rep-count", "Instances", count as i64));
            f.push(Field::number("rep-cx", "Centre X", center[0], Unit::Mm));
            f.push(Field::number("rep-cy", "Centre Y", center[1], Unit::Mm));
            f.push(Field::number(
                "rep-angle",
                "Angle between",
                angle,
                Unit::Deg,
            ));
        }
    }
    if *r != Repeat::None {
        f.push(Field::note(
            "In the toolpath's work plane; each copy is checked for collisions like the first.",
        ));
    }
}
