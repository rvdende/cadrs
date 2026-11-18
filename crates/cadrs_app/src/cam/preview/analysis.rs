//! Verify's check of a run (on a worker thread): the stock cut by every operation in order (the
//! tilted ones on voxels in their own frames), what ran into the stock, the part or the
//! workholding, and the summary line (a run that leans its tool is checked on a solid stock
//! instead, [`super::solid`]). Also the machine's own axes through the run, what they cannot
//! do of it and when, and where the machine runs into itself or the work ([`check_machine`]).

use super::*;
use cadrs_ui::i18n::{tr, tr_with};

/// What simulating the whole run found (worked out off the main thread).
pub(super) struct Analysis {
    pub(super) summary: String,
    pub(super) collisions: Vec<(f64, String)>,
    /// The part on the stock's grid, pulled in by a cell (mills).
    pub(super) part: Option<HeightMap>,
    /// The stock as the toolpaths before the previewed ones leave it (when there are any).
    pub(super) start: Option<HeightMap>,
    /// A cut sheet's pieces, each with whether it's a part cut free.
    pub(super) sheet: Option<Vec<(cadrs_core::cam::cadrs_cam::region::Region, bool)>>,
    /// Where the machine runs into itself, the workholding or the part ([`check_machine`]).
    pub(super) machine_hits: Vec<crate::cam::machine_view::MachineHit>,
    /// The solid stock after each operation, for a run simulated solid ([`super::solid`]).
    pub(super) solid: Vec<cadrs_core::cam::cadrs_cam::dexel::SimMesh>,
}

/// The machine's axes through a run's motions (an operation each, as
/// [`program::run_motions`] gives them; `segments`: the same moves in one list), for a machine
/// with a chain of its own or rotary axes (a catalogue 3-axis one is known by its travel
/// alone: that is the Post dialog's check). With them: what the machine cannot do of the run,
/// a line each, and when it first happens, for Simulate to pause at.
pub(super) fn machine_run(
    machine: &cadrs_core::cam::cadrs_cam::Machine,
    run: &program::SetupRun,
    motions: &[Vec<Segment>],
    segments: &[Segment],
) -> (Option<MachineRun>, Vec<String>, Vec<(f64, String)>) {
    if machine.chain.is_none() && machine.rotary_axes() == 0 {
        return (None, Vec::new(), Vec::new());
    }
    let track = program::axis_track(machine, run, motions);
    let (warnings, events) = if machine.rotary_axes() == 0 {
        (Vec::new(), Vec::new())
    } else {
        (
            program::axis_warnings(run, motions, &track),
            axis_events(run, motions, &track, &Timeline::new(segments.to_vec())),
        )
    };
    let mr = MachineRun {
        chain: machine.kinematics(),
        work: run.work,
        track,
    };
    (Some(mr), warnings, events)
}

/// When the machine first cannot do each thing [`program::axis_warnings`] lists: the start of
/// that move, with the warning's line.
fn axis_events(
    run: &program::SetupRun,
    motions: &[Vec<Segment>],
    track: &cadrs_core::cam::cadrs_cam::kinematics::AxisTrack,
    tl: &Timeline,
) -> Vec<(f64, String)> {
    let mut out: Vec<(f64, String)> = Vec::new();
    for (k, issue) in &track.issues {
        // (The line of this one issue, worded as the list's.)
        let one = cadrs_core::cam::cadrs_cam::kinematics::AxisTrack {
            rows: Vec::new(),
            issues: vec![(*k, issue.clone())],
        };
        for text in program::axis_warnings(run, motions, &one) {
            if !out.iter().any(|(_, t)| *t == text) {
                let at = if *k == 0 { 0.0 } else { tl.end_of(*k - 1) };
                out.push((at, text));
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// What the machine's own collision check needs (worked out on the worker).
pub(super) struct MachineCheck {
    pub(super) scene: crate::cam::machine_view::MachineScene,
    /// The setup's frame in the model, and in the table mount's coordinates.
    pub(super) setup: Frame,
    pub(super) work: Frame,
    /// The axis values before the first move, then at the end of each.
    pub(super) rows: Vec<Vec<f64>>,
    /// The workholding and the parts, in the model's coordinates.
    pub(super) fixed: Vec<crate::cam::machine_view::Fixed>,
}

/// The most positions of the machine a run is checked at.
const MACHINE_SAMPLES: usize = 2_000;

/// Steps the machine through the run, each operation with its tool and holder in the
/// spindle, and finds what it runs into: the hits for the view, and a collision each ("Swivel
/// head hits Vise (Rough 1)", "BT40 holder hits Platter (Finish 2)") at the time it starts.
pub(super) fn machine_hits(
    check: &MachineCheck,
    segments: &[Segment],
    spans: &[OpSpan],
) -> (
    Vec<crate::cam::machine_view::MachineHit>,
    Vec<(f64, String)>,
) {
    use crate::cam::machine_view::{Sample, ToolShape, machine_collisions};
    let tl = Timeline::new(segments.to_vec());
    let op_of = |segment: usize| spans.iter().rposition(|o| o.first <= segment).unwrap_or(0);
    // The tools of the run, each once, and which of them every operation has.
    let mut tools: Vec<(&Tool, ToolShape)> = Vec::new();
    let tool_of: Vec<Option<usize>> = spans
        .iter()
        .map(|o| {
            if let Some(k) = tools.iter().position(|(t, _)| **t == o.tool) {
                return Some(k);
            }
            let name = if o.tool.name.is_empty() {
                o.tool.default_name()
            } else {
                o.tool.name.clone()
            };
            let holder = tr_with("cam-collision-holder", &[("tool", &name)]);
            let shape = ToolShape::of(&o.tool, &name, &holder)?;
            tools.push((&o.tool, shape));
            Some(tools.len() - 1)
        })
        .collect();
    let tools: Vec<ToolShape> = tools.into_iter().map(|t| t.1).collect();
    let stride = check.rows.len().div_ceil(MACHINE_SAMPLES).max(1);
    // Row k is the machine at the end of move k − 1.
    let samples: Vec<(Sample, usize)> = check
        .rows
        .iter()
        .enumerate()
        .step_by(stride)
        .map(|(k, q)| {
            let segment = k.saturating_sub(1);
            let op = op_of(segment);
            (
                Sample {
                    t: if k == 0 { 0.0 } else { tl.end_of(k - 1) },
                    q: q.clone(),
                    tool_number: spans.get(op).map_or(0, |o| o.tool_number),
                    tool: tool_of.get(op).copied().flatten(),
                },
                op,
            )
        })
        .collect();
    let only: Vec<Sample> = samples.iter().map(|s| s.0.clone()).collect();
    let hits = machine_collisions(
        &check.scene,
        &check.setup,
        &check.work,
        &only,
        &check.fixed,
        &tools,
    );
    let lines = hits
        .iter()
        .map(|h| {
            let op = samples.get(h.sample).map_or(0, |s| s.1);
            (
                h.from,
                tr_with(
                    "cam-collision-machine",
                    &[
                        ("a", &h.a),
                        ("b", &h.b),
                        ("operation", spans.get(op).map_or("", |o| o.name.as_str())),
                    ],
                ),
            )
        })
        .collect();
    (hits, lines)
}

/// Adds what the machine runs into ([`machine_hits`]) to the analysis.
pub(super) fn check_machine(
    a: &mut Analysis,
    check: &MachineCheck,
    segments: &[Segment],
    spans: &[OpSpan],
) {
    let (hits, lines) = machine_hits(check, segments, spans);
    a.collisions.extend(lines);
    a.collisions.sort_by(|x, y| x.0.total_cmp(&y.0));
    a.machine_hits = hits;
}

/// Simulates the whole run: the summary line, the collisions' times and (mills) the part on
/// the stock's grid.
#[allow(clippy::too_many_arguments)]
pub(super) fn analyse(
    stock: &Stock,
    model: &cadrs_core::cam::cadrs_cam::model::Model,
    fixtures: &[[P3; 3]],
    start: Option<HeightMap>,
    segments: &[Segment],
    spans: &[OpSpan],
    beam: bool,
    cell: f64,
) -> Analysis {
    // Simulated in the run's order; a tilted operation (3+2) cuts the stock in its own frame.
    let ops: Vec<SimOp> = spans
        .iter()
        .enumerate()
        .map(|(k, sp)| {
            let end = spans.get(k + 1).map_or(segments.len(), |n| n.first);
            SimOp {
                tool: &sp.tool,
                segments: &segments[sp.first..end],
            }
        })
        .collect();
    let frames: Vec<_> = spans.iter().map(|s| s.frame.filter(|_| s.tilted)).collect();
    let r = match program::simulate_ops(
        stock,
        model,
        fixtures,
        start.clone(),
        &ops,
        &frames,
        beam,
        cell,
    ) {
        program::SimResult::Sheet(r) => {
            // (Items joined by " · ".)
            let summary = format!(
                "{} · {}{}",
                tr_with("cam-verify-cut-length", &[("length", &fmt0(r.cut_length))]),
                tr_with(
                    if r.parts_free == 1 {
                        "cam-verify-part-free"
                    } else {
                        "cam-verify-parts-free"
                    },
                    &[("count", &r.parts_free.to_string())],
                ),
                if r.damage > 0.01 {
                    format!(
                        " · {}",
                        tr_with("cam-verify-cut-from-part", &[("area", &fmt1(r.damage))])
                    )
                } else {
                    String::new()
                }
            );
            let sheet = r
                .pieces
                .iter()
                .zip(&r.part_pieces)
                .map(|(f, part)| (f.rings(), *part))
                .collect();
            return Analysis {
                summary,
                collisions: Vec::new(),
                part: None,
                start: None,
                sheet: Some(sheet),
                machine_hits: Vec::new(),
                solid: Vec::new(),
            };
        }
        program::SimResult::Mill(r) => r,
    };
    let tl = Timeline::new(segments.to_vec());
    let mut collisions = Vec::new();
    for (k, p) in r.rapid_hits.iter() {
        collisions.push((
            time_near(&tl, spans, *k, *p),
            tr_with(
                "cam-collision-rapid-into-stock",
                &[("operation", &spans[*k].name)],
            ),
        ));
    }
    for (k, p) in r.fixture_hits.iter() {
        collisions.push((
            time_near(&tl, spans, *k, *p),
            tr_with(
                "cam-collision-tool-into-workholding",
                &[("operation", &spans[*k].name)],
            ),
        ));
    }
    for (k, p) in r.holder_hits.iter() {
        collisions.push((
            time_near(&tl, spans, *k, *p),
            tr_with(
                "cam-collision-holder-meets-stock",
                &[("operation", &spans[*k].name)],
            ),
        ));
    }
    // The tilted operations: in their own frames, against the part and the workholding.
    let mut gouge_depth = if r.gouge_cells > 0 {
        r.gouge_depth
    } else {
        0.0
    };
    for (k, sp) in spans.iter().enumerate() {
        let Some(f) = sp.frame else { continue };
        let end = spans.get(k + 1).map_or(segments.len(), |n| n.first);
        let local = |p: P3| f.to_local(p);
        let segs: Vec<Segment> = segments[sp.first..end]
            .iter()
            .map(|s| Segment {
                from: local(s.from),
                to: local(s.to),
                ..*s
            })
            .collect();
        let tris = |t: &[[P3; 3]]| -> Vec<[P3; 3]> {
            t.iter()
                .map(|t| [local(t[0]), local(t[1]), local(t[2])])
                .collect()
        };
        let c = sim::check_against_part(
            &tris(&model.triangles),
            &tris(fixtures),
            &sp.tool,
            &segs,
            0.01,
        );
        if !c.gouges.is_empty() {
            gouge_depth = gouge_depth.max(c.gouge_depth);
        }
        // (`what` is a catalog key.)
        for (hits, what) in [
            (&c.rapid_hits, "cam-collision-rapid-through-part"),
            (&c.holder_hits, "cam-collision-holder-meets-part"),
            (&c.fixture_hits, "cam-collision-tool-into-workholding"),
        ] {
            for p in hits {
                collisions.push((
                    time_near(&tl, spans, k, f.to_world(*p)),
                    tr_with(what, &[("operation", &sp.name)]),
                ));
            }
        }
    }
    collisions.sort_by(|a, b| a.0.total_cmp(&b.0));
    // (Items joined by " · ".)
    let summary = format!(
        "{} · {} · {}{}",
        tr_with("cam-verify-removed", &[("volume", &fmt0(r.removed))]),
        if gouge_depth <= 0.0 {
            tr("cam-verify-no-gouges")
        } else {
            tr_with("cam-verify-gouges-up-to", &[("depth", &fmt2(gouge_depth))])
        },
        if collisions.is_empty() {
            tr("cam-verify-no-collisions")
        } else {
            tr_with(
                if collisions.len() == 1 {
                    "cam-verify-collision"
                } else {
                    "cam-verify-collisions"
                },
                &[("count", &collisions.len().to_string())],
            )
        },
        if r.excess > 0.0 {
            format!(
                " · {}",
                tr_with("cam-verify-left-up-to", &[("depth", &fmt2(r.excess))])
            )
        } else {
            String::new()
        }
    );
    let mut part = r.stock.blank();
    part.rasterize(&model.triangles, false);
    Analysis {
        summary,
        collisions,
        part: Some(part.eroded()),
        start,
        sheet: None,
        machine_hits: Vec::new(),
        solid: Vec::new(),
    }
}

/// The time of the move of operation `k` nearest `p`.
pub(super) fn time_near(tl: &Timeline, spans: &[OpSpan], k: usize, p: P3) -> f64 {
    let first = spans[k].first;
    let end = spans.get(k + 1).map_or(tl.segments.len(), |n| n.first);
    let mut best = (f64::INFINITY, first);
    for (i, s) in tl.segments.iter().enumerate().take(end).skip(first) {
        let d = cadrs_core::cam::cadrs_cam::geom::dist(s.to, p)
            .min(cadrs_core::cam::cadrs_cam::geom::dist(s.from, p));
        if d < best.0 {
            best = (d, i);
        }
    }
    tl.end_of(best.1.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cam::machine_view::tests::{boxed, held_mill, resolved, tip_at};
    use cadrs_core::samples::cnc_machines as cnc;

    /// A run of two operations on the sample's 5-axis mill: the first stays over the
    /// platter, the second drives the tool 5 into it. The collision is the tool's, with the
    /// second operation's name, at the time the move into the platter ends.
    #[test]
    fn a_tool_driven_into_the_platter_is_reported_with_its_operation_and_time() {
        let (doc, r) = resolved(cnc::MILL_5);
        let scene = boxed(&doc, cnc::MILL_5, r.clone());
        let mut tool = held_mill();
        tool.name = "End mill 10".into();
        let over: P3 = [50.0, 20.0, 60.0];
        let inside: P3 = [50.0, 20.0, -5.0];
        let seg = |from: P3, to: P3, minutes: f64| Segment {
            from,
            to,
            feed: Some(500.0),
            kind: None,
            beam: false,
            minutes,
            axis: None,
        };
        let segments = [
            seg(over, [60.0, 20.0, 60.0], 0.5),
            seg([60.0, 20.0, 60.0], over, 0.5),
            seg(over, inside, 0.25),
            seg(inside, over, 0.25),
        ];
        let span = |first: usize, name: &str| OpSpan {
            first,
            name: name.into(),
            tool: tool.clone(),
            tilted: false,
            frame: None,
            tool_number: 1,
            gauge: tool.gauge_length(),
        };
        let spans = [span(0, "Rough 1"), span(2, "Finish 2")];
        let mut rows = vec![tip_at(&r, &tool, over)];
        rows.extend(segments.iter().map(|s| tip_at(&r, &tool, s.to)));
        let check = MachineCheck {
            scene,
            setup: Frame::WORLD,
            work: Frame::WORLD,
            rows,
            fixed: Vec::new(),
        };
        let (hits, lines) = machine_hits(&check, &segments, &spans);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let (t, text) = &lines[0];
        assert!((t - 1.25).abs() < 1e-9, "{t}");
        assert_eq!(text, "End mill 10 hits Platter (Finish 2)");
        assert_eq!((hits[0].from, hits[0].to), (1.25, 1.25));
        // The first operation alone: nothing.
        let check_two = MachineCheck {
            rows: check.rows[..3].to_vec(),
            ..check
        };
        let (_, clear) = machine_hits(&check_two, &segments[..2], &spans[..1]);
        assert!(clear.is_empty(), "{clear:?}");
    }
}
