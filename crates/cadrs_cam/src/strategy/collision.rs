//! The Collision tab: the tool assembly above the flutes (the shank, the holder, the arbor
//! stacked on the holder) checked against the part along every cutting move, as Simulate
//! checks it against the stock. Each piece is a disc at a height above the tip: the shank the
//! cutter's radius from the end of the flutes, the holder's and arbor's profile segments their
//! larger radius from where they start, each grown by its clearance. A move whose discs would
//! meet the part is reported (Warn) or left out (Retract: the tool lifts to the clearance height
//! before it and comes back in over where the cut goes on, ramping by the Link tab's entry).
//!
//! The workholding (vises, clamps: the job's workholding bodies) is checked whatever the tab
//! says, against the whole tool, its cutter too: nothing may cut or touch it.

use serde::{Deserialize, Serialize};

use crate::dropcutter::{TriIndex, drop_point};
use crate::geom::{P3, dist, lerp};
use crate::tool::{Tool, ToolKind};
use crate::toolpath::{FeedKind, Step, Toolpath, arc_points};

use super::Input;

/// What to do where the tool assembly would hit the part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CollisionAction {
    /// Keep the moves; list them in the toolpath's warnings.
    Warn,
    /// Leave the moves out, lifting clear round them.
    #[default]
    Retract,
}

impl CollisionAction {
    pub const ALL: [CollisionAction; 2] = [CollisionAction::Retract, CollisionAction::Warn];

    pub fn label(self) -> &'static str {
        match self {
            CollisionAction::Warn => "Warn",
            CollisionAction::Retract => "Retract",
        }
    }
}

/// The Collision tab.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Collision {
    /// Check the tool assembly at all.
    pub check: bool,
    /// How far the shank must stay from the part (mm).
    pub shank_clearance: f64,
    /// How far the holder and arbor must stay from the part (mm).
    pub holder_clearance: f64,
    pub action: CollisionAction,
}

impl Default for Collision {
    fn default() -> Self {
        Collision {
            check: true,
            shank_clearance: 0.0,
            holder_clearance: 1.0,
            action: CollisionAction::Retract,
        }
    }
}

/// The tool assembly above the flutes as discs: (radius, height above the tip it starts at).
pub fn assembly_discs(tool: &Tool, c: &Collision) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    if tool.cutting_length < tool.overall_length {
        out.push((tool.radius() + c.shank_clearance, tool.cutting_length));
    }
    let mut base = tool.overall_length;
    for profile in [&tool.holder, &tool.arbor].into_iter().flatten() {
        for w in profile.points.windows(2) {
            let r = w[0][0].max(w[1][0]);
            let h = w[0][1].min(w[1][1]);
            if r > 0.0 {
                out.push((r + c.holder_clearance, base + h));
            }
        }
        base += profile.length();
    }
    out
}

/// What a move would hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    /// The shank, holder or arbor the part.
    Part,
    /// Any of the tool (its cutter too) the workholding.
    Fixture,
}

/// Checks a toolpath's cutting moves against the part (the tool assembly above the flutes, when
/// the tab's check is on) and the workholding (the whole tool and its holder, always). With
/// Retract the moves that would hit are left out, the tool lifting clear round them and coming
/// back in by the Link tab's entry; either way a warning counts them and says where the first is.
pub fn apply(input: &Input, tp: Toolpath) -> Toolpath {
    let c = input.params.collision;
    if input.tool.is_beam() {
        return tp;
    }
    let part_check = c.check && !input.model.triangles.is_empty();
    if !part_check && input.fixtures.is_empty() {
        return tp;
    }
    let discs = assembly_discs(input.tool, &c);
    let lowest = discs.iter().map(|d| d.1).fold(f64::INFINITY, f64::min);
    let top = input.model.bounds().max[2];
    let idx = TriIndex::new(&input.model.triangles);
    let probes: Vec<(Tool, f64)> = discs
        .iter()
        .map(|(r, h)| (Tool::new(ToolKind::EndMill, 2.0 * (r - 1e-3).max(1e-3)), *h))
        .collect();
    // The workholding: the cutter itself (a hair smaller, so running along it isn't a hit) and
    // the holder's discs.
    let fixtures = (!input.fixtures.is_empty()).then(|| TriIndex::new(input.fixtures));
    let fixture_top = input
        .fixtures
        .iter()
        .flatten()
        .map(|p| p[2])
        .fold(f64::NEG_INFINITY, f64::max);
    let mut cutter = input.tool.clone();
    cutter.diameter = (cutter.diameter - 0.002).max(1e-3);
    if cutter.kind == ToolKind::BallMill {
        cutter.corner_radius = cutter.diameter / 2.0;
    }
    let step = (input.tool.radius() / 2.0).clamp(0.05, 1.0);
    let hits_at = |q: P3, scratch: &mut Vec<u32>| -> Option<Hit> {
        if let Some(f) = &fixtures
            && q[2] < fixture_top
        {
            let low = drop_point(&cutter, f, [q[0], q[1]], f64::NEG_INFINITY, scratch);
            let holder = probes
                .iter()
                .filter(|(_, h)| *h >= input.tool.overall_length - 1e-9);
            if low > q[2] + 1e-3
                || holder.into_iter().any(|(t, h)| {
                    q[2] + h < fixture_top
                        && drop_point(t, f, [q[0], q[1]], f64::NEG_INFINITY, scratch)
                            > q[2] + h + 1e-6
                })
            {
                return Some(Hit::Fixture);
            }
        }
        // Nothing of the part reaches the lowest disc.
        if !part_check || q[2] + lowest >= top {
            return None;
        }
        probes
            .iter()
            .any(|(t, h)| {
                q[2] + h < top
                    && drop_point(t, &idx, [q[0], q[1]], f64::NEG_INFINITY, scratch)
                        > q[2] + h + 1e-6
            })
            .then_some(Hit::Part)
    };
    let mut scratch = Vec::new();
    // Whether a straight rapid from `a` to `b` takes the tool or holder into the workholding.
    let fixture_on_way = |a: P3, b: P3, scratch: &mut Vec<u32>| -> bool {
        if fixtures.is_none() {
            return false;
        }
        let n = (dist(a, b) / step).ceil().max(1.0) as usize;
        (0..=n).any(|i| hits_at(lerp(a, b, i as f64 / n as f64), scratch) == Some(Hit::Fixture))
    };
    // A drill cycle's tool goes down its own hole: against the part only what is wider than
    // the tool (the holder, the arbor, a wider shank) can meet it.
    let wide: Vec<(Tool, f64)> = probes
        .iter()
        .filter(|(t, _)| t.radius() > input.tool.radius() + 0.05)
        .cloned()
        .collect();
    let drill_hit = |d: &crate::toolpath::DrillCycle, scratch: &mut Vec<u32>| {
        let n = ((d.retract - d.bottom) / step).ceil().clamp(1.0, 10_000.0) as usize;
        let mut worst = None;
        for i in 0..=n {
            let q = [d.at[0], d.at[1], lerp_z(d.retract, d.bottom, i, n)];
            if hits_at(q, scratch) == Some(Hit::Fixture) {
                return Some(Hit::Fixture);
            }
            if part_check
                && q[2] + lowest < top
                && wide.iter().any(|(t, h)| {
                    q[2] + h < top
                        && drop_point(t, &idx, [q[0], q[1]], f64::NEG_INFINITY, scratch)
                            > q[2] + h + 1e-6
                })
            {
                worst = Some(Hit::Part);
            }
        }
        worst
    };
    let mut scratch2 = Vec::new();
    let mut hits_move = |from: P3, s: &Step| -> Option<Hit> {
        let pts: Vec<P3> = match s {
            Step::Feed { to, .. } => {
                let n = (dist(from, *to) / step).ceil().max(1.0) as usize;
                (0..=n)
                    .map(|i| lerp(from, *to, i as f64 / n as f64))
                    .collect()
            }
            Step::Arc { to, center, cw, .. } => {
                let mut v = vec![from];
                v.extend(arc_points(from, *to, *center, *cw, step.min(0.05)));
                v
            }
            _ => return None,
        };
        let mut worst = None;
        for q in pts {
            match hits_at(q, &mut scratch) {
                Some(Hit::Fixture) => return Some(Hit::Fixture),
                Some(h) => worst = Some(h),
                None => {}
            }
        }
        worst
    };
    let p = input.params;
    let clearance = input.clearance();
    let fd = p.links.feed_distance;
    let retract = c.action == CollisionAction::Retract;
    let mut out: Vec<Step> = Vec::with_capacity(tp.steps.len());
    let mut at: Option<P3> = None;
    let mut skipping = false;
    let mut counts = [0usize; 2];
    let mut first: [Option<P3>; 2] = [None; 2];
    // Cutter compensation on (a profile's wear compensation): cancelled before a lift clear,
    // never carried through the lift and the way back in.
    let mut compensating = false;
    let mut cancelled = false;
    for s in tp.steps {
        match &s {
            Step::Compensation(c) => {
                compensating = c.is_some();
                out.push(s);
            }
            Step::Feed { to, .. } | Step::Arc { to, .. } => {
                let to = *to;
                if let Some(from) = at
                    && let Some(hit) = hits_move(from, &s)
                {
                    let k = (hit == Hit::Fixture) as usize;
                    counts[k] += 1;
                    first[k].get_or_insert(from);
                    if retract {
                        if !skipping {
                            if compensating {
                                out.push(Step::Compensation(None));
                                compensating = false;
                                cancelled = true;
                            }
                            out.push(Step::Rapid([from[0], from[1], clearance.max(from[2])]));
                            skipping = true;
                        }
                        at = Some(to);
                        continue;
                    }
                }
                if skipping && let Some(from) = at {
                    reenter(&mut out, input, from, &s, clearance, fd);
                    skipping = false;
                }
                at = Some(to);
                out.push(s);
            }
            Step::Rapid(q) => {
                let q = *q;
                if skipping {
                    if q[2] < clearance {
                        out.push(Step::Rapid([q[0], q[1], clearance]));
                    }
                    skipping = false;
                }
                // Over the workholding: up to the clearance height (over it all), across, down;
                // a descent into it isn't made (the cut after it comes in by its entry).
                if let Some(from) = at
                    && fixture_on_way(from, q, &mut scratch2)
                {
                    counts[1] += 1;
                    first[1].get_or_insert(from);
                    let over = [q[0], q[1], clearance.max(q[2])];
                    out.push(Step::Rapid([from[0], from[1], clearance.max(from[2])]));
                    out.push(Step::Rapid(over));
                    if fixture_on_way(over, q, &mut scratch2) {
                        at = Some(q);
                        skipping = retract;
                        if !retract {
                            out.push(Step::Rapid(q));
                        }
                        continue;
                    }
                    out.push(Step::Rapid(q));
                    at = Some(q);
                    continue;
                }
                at = Some(q);
                out.push(s);
            }
            Step::Drill(d) => {
                at = Some([d.at[0], d.at[1], d.retract]);
                if let Some(hit) = drill_hit(d, &mut scratch2) {
                    let k = (hit == Hit::Fixture) as usize;
                    counts[k] += 1;
                    first[k].get_or_insert([d.at[0], d.at[1], d.bottom]);
                    // Retract: the hole left out.
                    if retract {
                        continue;
                    }
                }
                out.push(s);
            }
            _ => out.push(s),
        }
    }
    let mut warnings = tp.warnings;
    if cancelled {
        // A contour compensated in places and not in others would step its wall by the wear:
        // the whole toolpath without it.
        out.retain(|s| !matches!(s, Step::Compensation(_)));
        warnings.push(
            "Wear compensation is off for this toolpath: the tool lifts clear of a collision \
             in a compensated contour (G41/G42 can't run through the lift and back)"
                .into(),
        );
    }
    let what = [
        if input.tool.holder.is_some() {
            "the shank or holder into the part"
        } else {
            "the shank into the part"
        },
        "the tool or its holder into the workholding",
    ];
    for k in 0..2 {
        if let Some(q) = first[k] {
            let n = counts[k];
            warnings.push(format!(
                "{n} move{} would bring {} (first near {:.1}, {:.1}, {:.1}){}",
                if n == 1 { "" } else { "s" },
                what[k],
                q[0],
                q[1],
                q[2],
                if retract {
                    ": left out, the tool lifting clear round them"
                } else {
                    ""
                }
            ));
        }
    }
    Toolpath {
        steps: out,
        warnings,
    }
}

fn lerp_z(a: f64, b: f64, i: usize, n: usize) -> f64 {
    a + (b - a) * i as f64 / n as f64
}

/// Back over `from` (where the cut goes on with `next`) from the clearance height and in: by a
/// ramp along `next`'s way when the Link tab ramps (the material there may be a step down
/// thick), else a plunge at the plunge feed.
fn reenter(out: &mut Vec<Step>, input: &Input, from: P3, next: &Step, clearance: f64, fd: f64) {
    let p = input.params;
    out.push(Step::Rapid([from[0], from[1], clearance.max(from[2])]));
    // The way the next move goes, along itself (an arc's own curve, never its chord).
    let way: Vec<crate::geom::P2> = match next {
        Step::Feed { to, .. } => vec![[from[0], from[1]], [to[0], to[1]]],
        Step::Arc { to, center, cw, .. } => std::iter::once([from[0], from[1]])
            .chain(
                arc_points(from, *to, *center, *cw, 0.01)
                    .into_iter()
                    .map(|q| [q[0], q[1]]),
            )
            .collect(),
        _ => Vec::new(),
    };
    let len: f64 = way.windows(2).map(|w| crate::geom::dist2(w[0], w[1])).sum();
    let ramp = p.links.entry == super::Entry::Ramp && len > 1e-6;
    // The material over it: as much as a step down (a level above cut down to it).
    let top = (from[2] + p.step_down.max(0.0)).min(clearance - fd);
    let start = top + fd;
    if start < clearance {
        out.push(Step::Rapid([from[0], from[1], start]));
    }
    if ramp {
        // Back and forth along the next move's first stretch, down at the ramp angle.
        let run = len.min(input.tool.diameter * 2.0).max(1e-3);
        let stretch = super::path::sub_walk(&way, 0.0, run);
        let back: Vec<crate::geom::P2> = stretch.iter().rev().copied().collect();
        let drop = run * p.links.ramp_angle.clamp(0.5, 45.0).to_radians().tan();
        let mut z = start;
        let mut out_leg = true;
        while z > from[2] + 1e-9 || !out_leg {
            let target = (z - drop).max(from[2]);
            let leg = if out_leg { &stretch } else { &back };
            let mut s = 0.0;
            for w in leg.windows(2) {
                s += crate::geom::dist2(w[0], w[1]);
                let zz = z + (target - z) * (s / run).min(1.0);
                out.push(Step::Feed {
                    to: [w[1][0], w[1][1], zz],
                    feed: p.speeds.ramp,
                    kind: FeedKind::Ramp,
                });
            }
            z = target;
            out_leg = !out_leg;
        }
    } else {
        out.push(Step::Feed {
            to: from,
            feed: p.speeds.plunge,
            kind: FeedKind::Plunge,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Aabb;
    use crate::machine::MachineKind;
    use crate::model::{Model, box_triangles};
    use crate::strategy::{Params, Pattern, Stock, generate};
    use crate::tool::Profile;

    /// A 40 mm tall wall beside a 10 mm deep slot: the profile round the 30 mm deep part runs
    /// the tool's 20 mm flutes 30 deep beside the wall.
    fn run(action: CollisionAction, holder: bool) -> Toolpath {
        let m = Model::from_triangles(
            "block",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 30.0])),
        );
        let stock = Stock::block(Aabb::new([-10.0, -10.0, 0.0], [50.0, 40.0, 30.0]));
        let mut tool = Tool::new(ToolKind::EndMill, 6.0);
        tool.cutting_length = 20.0;
        tool.overall_length = 25.0;
        if holder {
            tool.holder = Some(Profile::cylinder("ER20", 30.0, 40.0));
        }
        let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        p.step_down = 10.0;
        p.collision.action = action;
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        generate(&input).unwrap()
    }

    fn lowest_cut(tp: &Toolpath) -> f64 {
        tp.steps
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } => Some(to[2]),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// Wear compensation on a profile whose holder forces a lift clear: compensation is off
    /// before every rapid (never on through a lift or the way back in), and the toolpath says
    /// where it was cancelled.
    #[test]
    fn a_lift_clear_cancels_compensation() {
        let m = Model::from_triangles(
            "block",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 30.0])),
        );
        let stock = Stock::block(Aabb::new([-10.0, -10.0, 0.0], [50.0, 40.0, 30.0]));
        let mut tool = Tool::new(ToolKind::EndMill, 6.0);
        tool.cutting_length = 20.0;
        tool.overall_length = 25.0;
        tool.holder = Some(Profile::cylinder("ER20", 30.0, 40.0));
        let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        p.step_down = 10.0;
        p.compensation = crate::strategy::Compensation::Wear;
        p.collision.action = CollisionAction::Retract;
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        let mut on = false;
        let mut started = 0;
        for s in &tp.steps {
            match s {
                Step::Compensation(c) => {
                    on = c.is_some();
                    started += c.is_some() as usize;
                }
                Step::Rapid(q) => assert!(!on, "a rapid to {q:?} under compensation"),
                _ => {}
            }
        }
        // Compensated in places and not others would step the wall: none at all, and said.
        assert_eq!(started, 0);
        assert!(
            tp.warnings
                .iter()
                .any(|w| w.contains("Wear compensation is off")),
            "{:?}",
            tp.warnings
        );
        // Without the holder in the way, the same profile is compensated.
        let mut free = tool.clone();
        free.holder = None;
        let input = Input {
            tool: &free,
            ..input
        };
        let tp = generate(&input).unwrap();
        assert!(
            tp.steps
                .iter()
                .any(|s| matches!(s, Step::Compensation(Some(_))))
        );
    }

    #[test]
    fn shank_beside_its_wall_is_fine_holder_is_not() {
        // The shank runs beside the wall, never into it: nothing to report.
        let tp = run(CollisionAction::Retract, false);
        assert!(tp.warnings.is_empty(), "{:?}", tp.warnings);
        assert!(lowest_cut(&tp) < 0.01);
        // A Ø30 holder 25 above the tip reaches over the part below 5 mm.
        let warned = run(CollisionAction::Warn, true);
        assert_eq!(warned.warnings.len(), 1, "{:?}", warned.warnings);
        assert!(lowest_cut(&warned) < 0.01);
        let kept = run(CollisionAction::Retract, true);
        assert!(kept.warnings[0].contains("left out"));
        // The cuts the holder would hit are gone: none deeper than 30 − 25 − 1 clearance.
        assert!(lowest_cut(&kept) > 3.9, "{}", lowest_cut(&kept));
    }

    #[test]
    fn discs_stack_the_holder_and_arbor() {
        let mut t = Tool::new(ToolKind::EndMill, 10.0);
        t.cutting_length = 22.0;
        t.overall_length = 40.0;
        t.holder = Some(Profile::cylinder("h", 32.0, 50.0));
        t.arbor = Some(Profile::cylinder("a", 63.0, 20.0));
        let d = assembly_discs(&t, &Collision::default());
        assert_eq!(d, vec![(5.0, 22.0), (17.0, 40.0), (32.5, 90.0)]);
    }

    /// A vise jaw 10 high, 2 mm off the block's +X side: the profile's deepest pass would cut
    /// it. Retract leaves those moves out (lifting clear, ramping back in); nothing cuts the jaw.
    #[test]
    fn the_workholding_is_never_cut() {
        let m = Model::from_triangles(
            "block",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 30.0])),
        );
        let jaw = box_triangles(&Aabb::new([42.0, -10.0, 0.0], [60.0, 40.0, 10.0]));
        let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [45.0, 35.0, 30.0]));
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        for action in [CollisionAction::Retract, CollisionAction::Warn] {
            let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
            p.step_down = 10.0;
            p.collision.action = action;
            let input = Input {
                model: &m,
                stock: &stock,
                tool: &tool,
                machine: MachineKind::Mill,
                params: &p,
                rest: None,
                fixtures: &jaw,
            };
            let tp = generate(&input).unwrap();
            assert!(
                tp.warnings
                    .iter()
                    .any(|w| w.contains("into the workholding")),
                "{:?}",
                tp.warnings
            );
            let jaw_model = Model::from_triangles("jaw", jaw.clone());
            let (g, at) = crate::strategy::testing::max_gouge(&jaw_model, &tool, &tp);
            if action == CollisionAction::Retract {
                assert!(g < 1e-3, "{g} into the jaw at {at:?}");
                // The cut goes on elsewhere at the bottom, re-entered by a ramp.
                assert!(lowest_cut(&tp) < 0.01);
                assert!(tp.steps.iter().any(|s| matches!(
                    s,
                    Step::Feed {
                        kind: FeedKind::Ramp,
                        ..
                    }
                )));
                // The clearance height is over the jaw (and everything else).
                assert!(input.clearance() >= 40.0);
            } else {
                assert!(g > 1.0, "Warn keeps the moves");
            }
        }
    }

    /// A clamp 15 high just off the block: roughing round the block keeps its centre off the
    /// clamp's footprint, and no move (rapids and links too) meets it.
    #[test]
    fn rough_keeps_off_a_clamp() {
        let m = Model::from_triangles(
            "block",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 10.0])),
        );
        let clamp = box_triangles(&Aabb::new([44.0, 10.0, 0.0], [50.0, 20.0, 15.0]));
        let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [48.0, 35.0, 12.0]));
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        let p = Params::new(Pattern::Rough3D, &tool, MachineKind::Mill);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &clamp,
        };
        let tp = generate(&input).unwrap();
        let clamp_model = Model::from_triangles("clamp", clamp.clone());
        let (g, at) = crate::strategy::testing::max_gouge(&clamp_model, &tool, &tp);
        assert!(g < 1e-3, "{g} into the clamp at {at:?}");
        // It still roughs: down to the floor beside the block away from the clamp.
        assert!(lowest_cut(&tp) < 0.5);
        // Over the clamp at the clearance height.
        assert!(input.clearance() >= 25.0);
    }

    /// A rapid across a clamp at 5 goes over it at the clearance height instead.
    #[test]
    fn rapids_go_over_the_workholding() {
        let m = Model::from_triangles(
            "block",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [10.0, 10.0, 10.0])),
        );
        let clamp = box_triangles(&Aabb::new([20.0, -5.0, 0.0], [25.0, 5.0, 15.0]));
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]));
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        let p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &clamp,
        };
        let tp = Toolpath {
            steps: vec![
                Step::Rapid([15.0, 0.0, 5.0]),
                Step::Rapid([30.0, 0.0, 5.0]),
                Step::Feed {
                    to: [35.0, 0.0, 5.0],
                    feed: 500.0,
                    kind: FeedKind::Cut,
                },
            ],
            warnings: vec![],
        };
        let out = apply(&input, tp);
        let cl = input.clearance();
        assert!(cl >= 15.0);
        assert!(
            out.steps
                .iter()
                .any(|s| matches!(s, Step::Rapid(p) if (p[0] - 30.0).abs() < 1e-9 && (p[2] - cl).abs() < 1e-9)),
            "{:?}",
            out.steps
        );
        let clamp_model = Model::from_triangles("clamp", clamp.clone());
        let (g, _) = crate::strategy::testing::max_gouge(&clamp_model, &tool, &out);
        assert!(g < 1e-3);
        assert!(out.warnings.iter().any(|w| w.contains("workholding")));
    }

    /// A hole drilled beside a clamp: the drill would cut into it, so it is left out (Retract)
    /// or kept and warned of; the hole clear of the clamp stays.
    #[test]
    fn drill_cycles_are_checked_against_the_workholding() {
        let m = Model::from_triangles(
            "block",
            box_triangles(&Aabb::new([-20.0, -10.0, 0.0], [5.0, 10.0, 10.0])),
        );
        let clamp = box_triangles(&Aabb::new([6.0, -5.0, 0.0], [12.0, 5.0, 15.0]));
        let stock = Stock::block(Aabb::new([-20.0, -10.0, 0.0], [5.0, 10.0, 10.0]));
        let tool = Tool::new(ToolKind::Drill, 6.0);
        let cycle = |x: f64| {
            Step::Drill(crate::toolpath::DrillCycle {
                at: [x, 0.0],
                r_plane: 12.0,
                bottom: -2.0,
                retract: 20.0,
                feed: 200.0,
                kind: crate::toolpath::CycleKind::Drill,
                peck: 0.0,
                dwell: 0.0,
                pitch: 0.0,
            })
        };
        let drills = |action: CollisionAction| {
            let mut p = Params::new(Pattern::Drill, &tool, MachineKind::Mill);
            p.collision.action = action;
            let input = Input {
                model: &m,
                stock: &stock,
                tool: &tool,
                machine: MachineKind::Mill,
                params: &p,
                rest: None,
                fixtures: &clamp,
            };
            let tp = Toolpath {
                steps: vec![Step::Rapid([-10.0, 0.0, 20.0]), cycle(-10.0), cycle(4.0)],
                warnings: vec![],
            };
            apply(&input, tp)
        };
        let count = |tp: &Toolpath| {
            tp.steps
                .iter()
                .filter(|s| matches!(s, Step::Drill(_)))
                .count()
        };
        let kept = drills(CollisionAction::Retract);
        assert_eq!(count(&kept), 1, "{:?}", kept.steps);
        assert!(
            kept.warnings.iter().any(|w| w.contains("workholding")),
            "{:?}",
            kept.warnings
        );
        let warned = drills(CollisionAction::Warn);
        assert_eq!(count(&warned), 2);
        assert!(warned.warnings.iter().any(|w| w.contains("workholding")));
    }

    /// Back in after a skipped stretch, onto a corner arc: the ramp follows the arc, never its
    /// chord (which cuts inside the corner).
    #[test]
    fn reentry_ramps_along_an_arc() {
        let m = Model::from_triangles(
            "block",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [10.0, 10.0, 10.0])),
        );
        let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [15.0, 15.0, 10.0]));
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        let p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        // The corner arc round (10, 10) at radius 3, from (13, 10) to (10, 13).
        let arc = Step::Arc {
            to: [10.0, 13.0, 2.0],
            center: [10.0, 10.0],
            cw: false,
            feed: 500.0,
            kind: FeedKind::Cut,
        };
        let mut out = Vec::new();
        reenter(&mut out, &input, [13.0, 10.0, 2.0], &arc, 30.0, 1.0);
        let ramps: Vec<P3> = out
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Ramp,
                    ..
                } => Some(*to),
                _ => None,
            })
            .collect();
        assert!(!ramps.is_empty());
        for q in &ramps {
            let r = crate::geom::dist2([q[0], q[1]], [10.0, 10.0]);
            assert!((r - 3.0).abs() < 0.01, "{q:?} {r} off the arc");
        }
        // Back where the cut goes on, at its depth.
        let last = *ramps.last().unwrap();
        assert!(
            crate::geom::dist(last, [13.0, 10.0, 2.0]) < 1e-6,
            "{last:?}"
        );
    }
}
