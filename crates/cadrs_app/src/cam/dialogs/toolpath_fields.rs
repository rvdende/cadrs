//! The toolpath dialog's fields: its Tool path, Link, Axis and Collision tabs (the Misc and Beam
//! tabs are laid out with them in [`Draft::fields`]).

use super::*;
use cadrs_ui::i18n::{tr, tr_with};

/// The Axis tab: the tool's axis, the work plane's or a picked face's normal (3+2).
pub(super) fn axis_fields(
    f: &mut Vec<Field>,
    ctx: &Ctx,
    setup: CamId,
    p: &Params,
    picking: Option<&str>,
) {
    use cadrs_core::cam::cadrs_cam::strategy::ToolAxis;
    f.push(Field::section(&tr("cam-tool-axis")));
    f.push(Field::select(
        "axis",
        &tr("cam-tool-axis"),
        vec![tr("cam-work-plane-z"), tr("cam-normal-of-picked-face")],
        usize::from(p.axis != ToolAxis::WorkPlane),
    ));
    let ToolAxis::Direction(z) = p.axis else {
        f.push(Field::note(tr("cam-tool-axis-work-plane-note")));
        return;
    };
    f.push(Field::picks(
        "axis-face",
        &tr("cam-planar-face-for-tool-axis"),
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
    f.push(Field::note(tr_with(
        "cam-tilted-from-setup-z",
        &[("angle", &format!("{tilt:.1}"))],
    )));
    if let Some(m) = &job.machine
        && !m.can_tilt_to(local)
    {
        f.push(Field::error(tr(if m.indexing.is_none() {
            "cam-machine-no-indexing-axes"
        } else {
            "cam-machine-cant-tilt-that-far"
        })));
    }
}

/// The Collision tab: the shank and holder checked against the part.
pub(super) fn collision_fields(f: &mut Vec<Field>, p: &Params, tool: Option<&Tool>) {
    use cadrs_core::cam::cadrs_cam::strategy::collision::CollisionAction;
    let c = &p.collision;
    f.push(Field::section(&tr("cam-tool-assembly-section")));
    f.push(Field::check(
        "coll",
        &tr("cam-check-shank-and-holder"),
        c.check,
    ));
    if !c.check {
        return;
    }
    f.push(Field::number(
        "coll-shank",
        &tr("cam-shank-clearance"),
        c.shank_clearance,
        Unit::Mm,
    ));
    f.push(Field::number(
        "coll-holder",
        &tr("cam-holder-clearance"),
        c.holder_clearance,
        Unit::Mm,
    ));
    f.push(Field::select(
        "coll-action",
        &tr("cam-where-theyd-hit"),
        labels(&CollisionAction::ALL, CollisionAction::label),
        index_of(&CollisionAction::ALL, c.action),
    ));
    if let Some(t) = tool {
        f.push(Field::note(match &t.holder {
            Some(h) => tr_with(
                "cam-tool-lengths-in-holder",
                &[
                    ("tool", &t.name),
                    ("flutes", &fmt(t.cutting_length)),
                    ("stickout", &fmt(t.overall_length)),
                    ("holder", &h.name),
                ],
            ),
            None => tr_with(
                "cam-tool-lengths-no-holder",
                &[
                    ("tool", &t.name),
                    ("flutes", &fmt(t.cutting_length)),
                    ("stickout", &fmt(t.overall_length)),
                ],
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
    f.push(Field::text("name", &tr("common-name"), &op.name));
    // Start from a template saved in the library (one this tool and machine can run).
    let templates = usable_templates(ctx.cam, tk, machine);
    if !templates.is_empty() {
        let mut opts = vec![tr("common-none")];
        opts.extend(templates.iter().map(|t| t.name.clone()));
        f.push(Field::select(
            "template",
            &tr("cam-start-from-template"),
            opts,
            0,
        ));
    }
    let types: Vec<Type> = Type::ALL
        .into_iter()
        .filter(|t| !Pattern::available(*t, tk, machine).is_empty())
        .collect();
    f.push(Field::select(
        "type",
        &tr("common-type"),
        labels(&types, Type::label),
        index_of(&types, kind),
    ));
    let pats = Pattern::available(kind, tk, machine);
    f.push(Field::select(
        "pattern",
        &tr("cam-toolpath-pattern"),
        labels(&pats, Pattern::label),
        index_of(&pats, p.pattern),
    ));
    use Pattern::*;
    let pat = p.pattern;
    if matches!(pat, Rough2D | Rough3D) {
        f.push(Field::select(
            "sub",
            &tr("cam-sub-pattern"),
            labels(&SubPattern::ALL, SubPattern::label),
            index_of(&SubPattern::ALL, p.sub_pattern),
        ));
    }
    // Drive geometry.
    let (placeholder, items): (&str, Vec<String>) = match kind {
        Type::Edges => (
            "cam-drive-edges",
            p.drive
                .edges
                .iter()
                .map(|k| entity_label(ctx, k, "common-edge", "common-edge-of"))
                .collect(),
        ),
        Type::Faces => (
            match pat {
                Fill => "cam-faces-to-fill",
                Face => "cam-faces-stock-top",
                _ => "cam-floor-faces",
            },
            p.drive
                .faces
                .iter()
                .map(|k| entity_label(ctx, k, "common-face", "common-face-of"))
                .collect(),
        ),
        Type::Bodies => (
            "cam-faces-to-finish",
            p.drive
                .faces
                .iter()
                .map(|k| entity_label(ctx, k, "common-face", "common-face-of"))
                .collect(),
        ),
        Type::Holes => (
            "cam-drive-holes",
            p.drive
                .holes
                .iter()
                .map(|h| tr_with("cam-hole-at", &[("x", &fmt(h[0])), ("y", &fmt(h[1]))]))
                .collect(),
        ),
    };
    if pat != Engrave || kind == Type::Edges {
        f.push(Field::picks(
            "drive",
            &tr(placeholder),
            items,
            picking == Some("drive"),
        ));
    }
    if kind == Type::Edges && !p.drive.edges.is_empty() {
        f.push(Field::check(
            "reverse",
            &tr("cam-reverse-direction"),
            p.drive.reverse,
        ));
    }
    if matches!(pat, Profile2D | Cut) {
        f.push(Field::select(
            "side",
            &tr("cam-side"),
            labels(&Side::ALL, Side::label),
            index_of(&Side::ALL, p.side),
        ));
    }
    if pat == Profile2D && p.side != Side::On {
        f.push(Field::select(
            "comp",
            &tr("cam-compensation"),
            labels(&Compensation::ALL, Compensation::label),
            index_of(&Compensation::ALL, p.compensation),
        ));
    }
    let beam = tk.is_beam();
    if !beam && !matches!(pat, Drill) {
        f.push(Field::section(&tr("common-offset")));
        if !matches!(pat, Face | Engrave | Chamfer2D) {
            f.push(Field::number(
                "wall",
                &tr("cam-wall-stock-to-leave"),
                p.wall_stock,
                Unit::Mm,
            ));
        }
        if !matches!(pat, Engrave | Chamfer2D) {
            f.push(Field::number(
                "floor",
                &tr("cam-floor-stock-to-leave"),
                p.floor_stock,
                Unit::Mm,
            ));
        }
    } else if beam && pat == Cut {
        f.push(Field::number(
            "wall",
            &tr("cam-extra-offset"),
            p.wall_stock,
            Unit::Mm,
        ));
    }
    if matches!(
        pat,
        Rough2D | Rough3D | Adaptive | Profile2D | ConstantZ | Face | Engrave | Bore
    ) {
        f.push(Field::section(&tr("cam-step-down")));
        f.push(Field::number(
            "stepdown",
            &tr(if pat == Bore {
                "cam-helix-pitch"
            } else {
                "cam-step-down"
            }),
            p.step_down,
            Unit::Mm,
        ));
        if matches!(pat, Rough2D | Rough3D | Profile2D) {
            f.push(Field::number(
                "first",
                &tr("cam-first-step-down"),
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
        f.push(Field::section(&tr("cam-step-over")));
        f.push(Field::number(
            "stepover",
            &tr(if pat == Fill {
                "cam-line-spacing"
            } else {
                "cam-step-over"
            }),
            p.step_over,
            Unit::Mm,
        ));
        if pat == Profile2D {
            f.push(Field::count("passes", &tr("cam-passes"), p.passes as i64));
        }
        if pat == Scallop {
            f.push(Field::number(
                "steep",
                &tr("cam-steep-angle"),
                p.steep_angle,
                Unit::Deg,
            ));
        }
    }
    if pat == ThreadMill {
        f.push(Field::section(&tr("cam-thread")));
        f.push(Field::number(
            "thread-d",
            &tr("cam-major-diameter"),
            p.thread_diameter,
            Unit::Mm,
        ));
        f.push(Field::count(
            "passes",
            &tr("cam-radial-passes"),
            p.passes as i64,
        ));
        f.push(Field::check(
            "left-thread",
            &tr("cam-left-hand-thread"),
            p.left_hand_thread,
        ));
        f.push(Field::note(tr("cam-thread-pitch-note")));
    }
    if !matches!(pat, Drill) {
        f.push(Field::section(&tr("cam-sorting")));
        f.push(Field::select(
            "direction",
            &tr(if beam {
                "common-direction"
            } else {
                "cam-cutting-direction"
            }),
            if beam {
                vec![tr("cam-part-on-the-right"), tr("cam-part-on-the-left")]
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
                &tr("cam-cutting-method"),
                labels(&CuttingMethod::ALL, CuttingMethod::label),
                index_of(&CuttingMethod::ALL, p.cutting_method),
            ));
            f.push(Field::number(
                "angle",
                &tr("common-angle"),
                p.angle,
                Unit::Deg,
            ));
        }
        if matches!(pat, Rough2D | Rough3D | Profile2D) {
            f.push(Field::select(
                "machine-by",
                &tr("cam-machine-by"),
                labels(&MachineBy::ALL, MachineBy::label),
                index_of(&MachineBy::ALL, p.machine_by),
            ));
        }
    }
    if !beam && !matches!(pat, Drill | Bore | Chamfer2D | Engrave) {
        f.push(Field::section(&tr("cam-heights")));
        for (k, from, offset, h) in [
            ("top", "cam-top-from", "cam-top-offset", p.heights.top),
            (
                "bottom",
                "cam-bottom-from",
                "cam-bottom-offset",
                p.heights.bottom,
            ),
        ] {
            f.push(Field::select(
                &format!("{k}-from"),
                &tr(from),
                labels(&HeightRef::ALL, HeightRef::label),
                index_of(&HeightRef::ALL, h.from),
            ));
            f.push(Field::number(
                &format!("{k}-off"),
                &tr(offset),
                h.offset,
                Unit::Mm,
            ));
        }
    }
    match pat {
        Chamfer2D => {
            f.push(Field::section(&tr("cam-chamfer")));
            f.push(Field::number(
                "depth",
                &tr("cam-chamfer-width"),
                p.depth,
                Unit::Mm,
            ));
            f.push(Field::number(
                "tip-off",
                &tr("cam-tip-offset"),
                p.tip_offset,
                Unit::Mm,
            ));
        }
        Engrave => {
            f.push(Field::section(&tr("cam-engrave")));
            f.push(Field::number(
                "depth",
                &tr("common-depth"),
                p.depth,
                Unit::Mm,
            ));
        }
        Profile2D => {
            f.push(Field::section(&tr("cam-tabs")));
            f.push(Field::check("tabs", &tr("cam-tabs"), p.tabs.enabled));
            if p.tabs.enabled {
                f.push(Field::count(
                    "tab-count",
                    &tr("cam-tabs-per-contour"),
                    p.tabs.count as i64,
                ));
                f.push(Field::number(
                    "tab-w",
                    &tr("cam-tab-width"),
                    p.tabs.width,
                    Unit::Mm,
                ));
                f.push(Field::number(
                    "tab-h",
                    &tr("cam-tab-height"),
                    p.tabs.height,
                    Unit::Mm,
                ));
            }
        }
        Drill | Bore => {
            f.push(Field::section(&tr(if pat == Drill {
                "cam-cycle"
            } else {
                "cam-holes"
            })));
            f.push(Field::select(
                "hole-select",
                &tr("cam-holes"),
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
                    &tr("cam-cycle"),
                    labels(&cycles, CycleKind::label),
                    index_of(&cycles, p.cycle.kind),
                ));
                if matches!(p.cycle.kind, CycleKind::Peck | CycleKind::ChipBreak) {
                    f.push(Field::number(
                        "peck",
                        &tr("cam-peck-depth"),
                        p.cycle.peck,
                        Unit::Mm,
                    ));
                }
                f.push(Field::number(
                    "dwell",
                    &tr("cam-dwell"),
                    p.cycle.dwell,
                    Unit::Seconds,
                ));
                f.push(Field::check(
                    "retract-full",
                    &tr("cam-retract-between-holes"),
                    p.cycle.retract_full,
                ));
            }
            f.push(Field::number(
                "break",
                &tr("cam-break-through"),
                p.cycle.break_through,
                Unit::Mm,
            ));
        }
        _ => {}
    }
    f.push(Field::section(&tr("cam-speeds")));
    if !beam {
        f.push(Field::number(
            "rpm",
            &tr("cam-spindle-speed"),
            p.speeds.spindle_rpm,
            Unit::Rpm,
        ));
    }
    f.push(Field::number(
        "feed",
        &tr("cam-feed-rate"),
        p.speeds.feed,
        Unit::Feed,
    ));
    if !beam {
        f.push(Field::number(
            "plunge",
            &tr("cam-plunge-rate"),
            p.speeds.plunge,
            Unit::Feed,
        ));
        if !matches!(pat, Drill) {
            f.push(Field::number(
                "ramp-feed",
                &tr("cam-ramp-rate"),
                p.speeds.ramp,
                Unit::Feed,
            ));
        }
    }
    f.push(Field::number(
        "lead-feed",
        &tr("cam-lead-rate"),
        p.speeds.lead,
        Unit::Feed,
    ));
    f.push(Field::section(&tr("cam-posting")));
    if !beam {
        f.push(Field::select(
            "coolant",
            &tr("cam-coolant"),
            labels(&Coolant::ALL, Coolant::label),
            index_of(&Coolant::ALL, p.posting.coolant),
        ));
        f.push(Field::check(
            "force-tc",
            &tr("cam-force-tool-change"),
            p.posting.force_tool_change,
        ));
    }
    f.push(Field::check(
        "stop",
        &tr("cam-stop-before"),
        p.posting.stop_before,
    ));
    f.push(Field::text(
        "comment",
        &tr("cam-comment"),
        &p.posting.comment,
    ));
    if matches!(pat, Rough2D | Rough3D | Adaptive) {
        f.push(Field::check("rest", &tr("cam-rest-machining"), p.rest));
    }
}

pub(super) fn link_fields(f: &mut Vec<Field>, p: &Params, beam: bool) {
    let l = &p.links;
    f.push(Field::section(&tr("cam-clearance-area")));
    f.push(Field::number(
        "clearance",
        &tr("cam-clearance-above-stock"),
        l.clearance,
        Unit::Mm,
    ));
    if !beam {
        f.push(Field::number(
            "rapid-d",
            &tr("cam-rapid-distance"),
            l.rapid_distance,
            Unit::Mm,
        ));
        f.push(Field::number(
            "feed-d",
            &tr("cam-feed-distance"),
            l.feed_distance,
            Unit::Mm,
        ));
        f.push(Field::section(&tr("cam-gaps")));
        f.push(Field::number(
            "gap",
            &tr("cam-small-gaps-up-to"),
            l.small_gap,
            Unit::Mm,
        ));
        let acts = labels(&LinkAction::ALL, LinkAction::label);
        f.push(Field::select(
            "small",
            &tr("cam-small-gaps"),
            acts.clone(),
            index_of(&LinkAction::ALL, l.small_gaps),
        ));
        f.push(Field::select(
            "large",
            &tr("cam-large-gaps"),
            acts,
            index_of(&LinkAction::ALL, l.large_gaps),
        ));
        f.push(Field::section(&tr("cam-first-entry")));
        f.push(Field::select(
            "entry",
            &tr("cam-entry"),
            labels(&Entry::ALL, Entry::label),
            index_of(&Entry::ALL, l.entry),
        ));
        if l.entry == Entry::Ramp {
            f.push(Field::number(
                "ramp-angle",
                &tr("cam-ramp-angle"),
                l.ramp_angle,
                Unit::Deg,
            ));
        }
    }
    f.push(Field::section(&tr("cam-leads")));
    let leads = labels(&LeadKind::ALL, LeadKind::label);
    f.push(Field::select(
        "lead-in",
        &tr("cam-lead-in"),
        leads.clone(),
        index_of(&LeadKind::ALL, l.lead_in),
    ));
    f.push(Field::select(
        "lead-out",
        &tr("cam-lead-out"),
        leads,
        index_of(&LeadKind::ALL, l.lead_out),
    ));
    if l.lead_in != LeadKind::None || l.lead_out != LeadKind::None {
        f.push(Field::number(
            "lead-r",
            &tr("cam-lead-radius"),
            l.lead_radius,
            Unit::Mm,
        ));
    }
    f.push(Field::number(
        "overlap",
        &tr("cam-overlap"),
        l.overlap,
        Unit::Mm,
    ));
}

/// The Misc tab's pattern: the toolpath repeated in a line or round a centre.
pub(super) fn repeat_fields(f: &mut Vec<Field>, r: &cadrs_core::cam::cadrs_cam::strategy::Repeat) {
    use cadrs_core::cam::cadrs_cam::strategy::Repeat;
    f.push(Field::section(&tr("cam-repeat-pattern")));
    f.push(Field::select(
        "repeat",
        &tr("cam-repeat"),
        vec![
            tr("common-none"),
            tr("cam-repeat-linear"),
            tr("cam-repeat-circular"),
        ],
        match r {
            Repeat::None => 0,
            Repeat::Linear { .. } => 1,
            Repeat::Circular { .. } => 2,
        },
    ));
    match *r {
        Repeat::None => {}
        Repeat::Linear { count, step } => {
            f.push(Field::count(
                "rep-count",
                &tr("cam-instances"),
                count as i64,
            ));
            f.push(Field::number(
                "rep-dx",
                &tr("cam-spacing-x"),
                step[0],
                Unit::Mm,
            ));
            f.push(Field::number(
                "rep-dy",
                &tr("cam-spacing-y"),
                step[1],
                Unit::Mm,
            ));
        }
        Repeat::Circular {
            count,
            center,
            angle,
        } => {
            f.push(Field::count(
                "rep-count",
                &tr("cam-instances"),
                count as i64,
            ));
            f.push(Field::number(
                "rep-cx",
                &tr("cam-centre-x"),
                center[0],
                Unit::Mm,
            ));
            f.push(Field::number(
                "rep-cy",
                &tr("cam-centre-y"),
                center[1],
                Unit::Mm,
            ));
            f.push(Field::number(
                "rep-angle",
                &tr("cam-angle-between"),
                angle,
                Unit::Deg,
            ));
        }
    }
    if *r != Repeat::None {
        f.push(Field::note(tr("cam-repeat-note")));
    }
}
