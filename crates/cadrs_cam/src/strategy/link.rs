//! Putting cuts together: the tool's travel between them (the Link tab's actions), entries
//! (plunge or ramp), leads and tabs. Strategies hand the [`Builder`] contours at a depth; it
//! adds every move needed to get there safely and away again.

use crate::geom::{P2, P3, dist2, lerp2};
use crate::toolpath::{FeedKind, Step, Toolpath};

use super::path::{Contour, lead_arc, lead_out_arc, ramp_along, sub_walk, with_tabs};
use super::{Compensation, Entry, Input, LeadKind, LinkAction, Pattern};

/// Points along an XY arc from `a` to `b` about `c` (for checking what a lead sweeps).
pub(super) fn arc_path(a: P2, b: P2, c: P2, cw: bool) -> Vec<P2> {
    let mut v = vec![a];
    v.extend(
        crate::toolpath::arc_points([a[0], a[1], 0.0], [b[0], b[1], 0.0], c, cw, 0.01)
            .into_iter()
            .map(|p| [p[0], p[1]]),
    );
    v
}

/// How a contour is cut.
#[derive(Debug, Clone, Copy)]
pub struct Cut<'a> {
    /// The top of the material over the cut: feeding starts the feed distance above it.
    pub material_top: f64,
    /// The tool may stay down and feed straight from where it is, where the way there is
    /// clear: inside `free` or, swept, inside `clear` (with neither, it never stays down).
    pub direct_ok: bool,
    /// Where the tool's centre may go at the cut's depth without touching the part (the links
    /// that stay down must keep to it): `None` unknown.
    pub free: Option<&'a crate::region::Region>,
    /// The free side for leads (+1 left of the direction of travel, −1 right); `None`: no
    /// leads.
    pub free_side: Option<f64>,
    /// Tabs: (top of the tabs, count, width).
    pub tabs: Option<(f64, usize, f64)>,
    /// Ramp in along the contour (else plunge, or lead from the free side).
    pub ramp: bool,
    /// Where the tool may come down beside the cut (a lead-in's start must be in it): `None`
    /// anywhere.
    pub clear: Option<&'a crate::region::Region>,
}

pub struct Builder<'a> {
    pub input: &'a Input<'a>,
    pub steps: Vec<Step>,
    pub at: Option<P3>,
    /// The direction the tool last moved in across XY (unit).
    pub heading: Option<P2>,
    pub warnings: Vec<String>,
    /// The contour being cut (a profile's), for wear compensation: a contour is compensated at
    /// every depth or at none.
    pub contour: usize,
    /// Contours cut without compensation.
    pub comp_off: std::collections::HashSet<usize>,
    /// Contours with a depth that had no room for the lead-in compensation starts on.
    pub comp_missed: std::collections::HashSet<usize>,
}

impl<'a> Builder<'a> {
    /// A builder that starts the spindle (spindle machines).
    pub fn new(input: &'a Input<'a>) -> Builder<'a> {
        let mut b = Builder {
            input,
            steps: Vec::new(),
            at: None,
            heading: None,
            warnings: Vec::new(),
            contour: 0,
            comp_off: Default::default(),
            comp_missed: Default::default(),
        };
        let p = input.params;
        if !p.posting.comment.is_empty() {
            b.steps.push(Step::Comment(p.posting.comment.clone()));
        }
        if !input.tool.is_beam() {
            b.steps.push(Step::Spindle {
                rpm: p.speeds.spindle_rpm,
                cw: true,
            });
        }
        b
    }

    pub fn rapid(&mut self, p: P3) {
        if self.at.is_some_and(|a| a == p) {
            return;
        }
        self.steps.push(Step::Rapid(p));
        self.at = Some(p);
    }

    pub fn feed(&mut self, p: P3, kind: FeedKind) {
        if self.at.is_some_and(|a| a == p) {
            return;
        }
        let s = &self.input.params.speeds;
        let feed = match kind {
            FeedKind::Cut | FeedKind::Link => s.feed,
            FeedKind::Plunge => s.plunge,
            FeedKind::Ramp => s.ramp,
            FeedKind::LeadIn | FeedKind::LeadOut => s.lead,
            FeedKind::Retract => s.plunge.max(s.feed),
        };
        self.feed_at(p, kind, feed);
    }

    pub fn feed_at(&mut self, p: P3, kind: FeedKind, feed: f64) {
        if self.at.is_some_and(|a| a == p) {
            return;
        }
        self.steps.push(Step::Feed { to: p, feed, kind });
        self.turn_to(p);
    }

    /// Moves to `p`, keeping the heading.
    fn turn_to(&mut self, p: P3) {
        if let Some(a) = self.at {
            let d = [p[0] - a[0], p[1] - a[1]];
            if d[0].hypot(d[1]) > 1e-9 {
                self.heading = Some(super::path::unit(d));
            }
        }
        self.at = Some(p);
    }

    pub fn arc(&mut self, to: P3, center: P2, cw: bool, kind: FeedKind) {
        let s = &self.input.params.speeds;
        let feed = match kind {
            FeedKind::LeadIn | FeedKind::LeadOut => s.lead,
            FeedKind::Ramp => s.ramp,
            _ => s.feed,
        };
        self.steps.push(Step::Arc {
            to,
            center,
            cw,
            feed,
            kind,
        });
        // Leaving an arc along its tangent.
        let r = [to[0] - center[0], to[1] - center[1]];
        if r[0].hypot(r[1]) > 1e-9 {
            let t = if cw { [r[1], -r[0]] } else { [-r[1], r[0]] };
            self.heading = Some(super::path::unit(t));
        }
        self.at = Some(to);
    }

    /// Straight up (rapid) to `z` if lower.
    pub fn up_to(&mut self, z: f64) {
        if let Some(a) = self.at
            && a[2] < z - 1e-9
        {
            self.rapid([a[0], a[1], z]);
        }
    }

    /// The height a link action retracts to over material topped at `material_top`.
    fn retract_z(&self, action: LinkAction, material_top: f64) -> f64 {
        let l = &self.input.params.links;
        match action {
            LinkAction::Direct | LinkAction::RetractToFeed => material_top + l.feed_distance,
            LinkAction::RetractToRapid => self.input.rapid_plane(),
            LinkAction::RetractToClearance => self.input.clearance(),
        }
    }

    /// Gets the tool over `xy` ready to cut at depth `z`: straight there at feed when the link
    /// may stay down (returns true: it's at depth), else up, across and down to the feed
    /// distance above the material (returns false: an entry is still needed).
    pub fn travel_to(&mut self, xy: P2, z: f64, c: &Cut<'_>) -> bool {
        let l = &self.input.params.links;
        let Some(at) = self.at else {
            let cl = self.input.clearance();
            self.rapid([xy[0], xy[1], cl]);
            self.rapid([xy[0], xy[1], (c.material_top + l.feed_distance).min(cl)]);
            return false;
        };
        let d = dist2([at[0], at[1]], xy);
        let action = if d <= l.small_gap {
            l.small_gaps
        } else {
            l.large_gaps
        };
        let same_level = (at[2] - z).abs() < 1e-6;
        // Staying down (or just over the material) only where the straight way is known clear.
        let from = [at[0], at[1]];
        let way_clear = c.direct_ok
            && (c
                .free
                .is_some_and(|f| crate::region::segment_inside(f, from, xy))
                || (c.clear.is_some() && self.swept_clear(&[from, xy], c.clear)));
        if action == LinkAction::Direct && way_clear && same_level {
            self.feed([xy[0], xy[1], z], FeedKind::Link);
            return true;
        }
        let action = if action == LinkAction::Direct {
            LinkAction::RetractToFeed
        } else {
            action
        };
        // Retracts to the feed distance only stay down inside cleared areas; elsewhere go to the
        // rapid plane.
        let action = if action == LinkAction::RetractToFeed && !way_clear {
            LinkAction::RetractToRapid
        } else {
            action
        };
        let rz = self.retract_z(action, c.material_top).max(at[2]);
        self.up_to(rz);
        self.rapid([xy[0], xy[1], rz]);
        let start = c.material_top + l.feed_distance;
        if start < rz {
            self.rapid([xy[0], xy[1], start]);
        }
        false
    }

    /// Cuts a contour at depth `z`, with its entry, leads, overlap and tabs.
    pub fn cut(&mut self, c: &Contour, z: f64, cut: &Cut<'_>) {
        if c.pts.is_empty() {
            return;
        }
        let p = self.input.params;
        let l = &p.links;
        let walk = c.walk();
        let tabs_here = cut.tabs.filter(|(top, _, _)| *top > z + 1e-9 && c.closed);
        // Wear compensation (G41/G42) on a profile: it starts on a straight move into the lead
        // and ends after the lead-out.
        let comp_wanted = cut
            .free_side
            .filter(|_| p.compensation == Compensation::Wear && p.pattern == Pattern::Profile2D);
        let comp = comp_wanted.filter(|_| !self.comp_off.contains(&self.contour));
        // Leads (in the free side only).
        let lead_in = cut
            .free_side
            .filter(|_| l.lead_in != LeadKind::None && l.lead_radius > 0.0)
            .and_then(|side| {
                let (pt, t) = c.start_tangent()?;
                let lead = match l.lead_in {
                    LeadKind::Arc => {
                        let (s, ctr, cw) = lead_arc(pt, t, side, l.lead_radius, 90.0);
                        (s, Some((ctr, cw)))
                    }
                    _ => {
                        let n = [-t[1] * side, t[0] * side];
                        (
                            [pt[0] + n[0] * l.lead_radius, pt[1] + n[1] * l.lead_radius],
                            None,
                        )
                    }
                };
                // Compensated, an arc has a straight run into it, along its start's tangent.
                let run_in = match lead.1 {
                    Some((ctr, cw)) if comp.is_some() => {
                        let v = [lead.0[0] - ctr[0], lead.0[1] - ctr[1]];
                        let n = v[0].hypot(v[1]).max(1e-9);
                        let t = if cw { [v[1], -v[0]] } else { [-v[1], v[0]] };
                        let k = l.lead_radius / n;
                        Some([lead.0[0] - t[0] * k, lead.0[1] - t[1] * k])
                    }
                    _ => None,
                };
                // Only where the tool can come down and run to the contour: what it sweeps along
                // the lead clear (else it ramps in along the contour).
                let mut path = match lead.1 {
                    Some((ctr, cw)) => arc_path(lead.0, pt, ctr, cw),
                    None => vec![lead.0, pt],
                };
                if let Some(q) = run_in {
                    path.insert(0, q);
                }
                self.lead_clear(&path, cut)
                    .then_some((lead.0, lead.1, run_in))
            });
        let start = walk[0];
        let entry_xy = lead_in.map_or(start, |(s, _, run_in)| run_in.unwrap_or(s));
        let at_depth = self.travel_to(entry_xy, z, cut);
        if !at_depth {
            let use_ramp =
                cut.ramp && lead_in.is_none() && tabs_here.is_none() && l.entry == Entry::Ramp;
            if use_ramp {
                let from = self.at.map_or(cut.material_top, |a| a[2]);
                let pts = ramp_along(c, from, z, l.ramp_angle);
                // Compensated from the ramp's first straight leg along a closed contour: above
                // the depth, and cut again at it as the pass goes round. (An open one's ramp
                // goes back and forth: no compensation through its turns.)
                let comp = comp.filter(|_| c.closed);
                if comp_wanted.is_some() && !c.closed {
                    self.comp_missed.insert(self.contour);
                }
                if let Some(side) = comp {
                    self.steps.push(Step::Compensation(Some(side > 0.0)));
                }
                let mut deep = false;
                for q in pts.iter().skip(1) {
                    if !deep && (q[2] - z).abs() < 1e-9 {
                        deep = true;
                        self.feed(*q, FeedKind::Ramp);
                        continue;
                    }
                    self.feed(*q, if deep { FeedKind::Cut } else { FeedKind::Ramp });
                }
                self.finish_contour(c, z, cut, true);
                if comp.is_some() {
                    self.end_compensation(z, cut);
                }
                return;
            }
            // Tabs on this contour: a ramp back and forth along its start, which is clear of
            // them (they're centred between the start and the end of each stretch).
            match tabs_here {
                Some((_, count, width))
                    if cut.ramp && lead_in.is_none() && l.entry == Entry::Ramp =>
                {
                    let total: f64 = walk.windows(2).map(|w| dist2(w[0], w[1])).sum();
                    let room = total / count.max(1) as f64 / 2.0 - width / 2.0;
                    let run = room.min(self.input.tool.diameter * 2.0);
                    if run > self.input.tool.radius() * 0.5 {
                        self.zigzag_ramp(&sub_walk(&walk, 0.0, run), z);
                    } else {
                        self.feed([entry_xy[0], entry_xy[1], z], FeedKind::Plunge);
                    }
                }
                _ => self.feed([entry_xy[0], entry_xy[1], z], FeedKind::Plunge),
            }
        }
        // A plunge or a zigzag ramp in has no straight run for compensation to start on.
        if comp_wanted.is_some() && lead_in.is_none() {
            self.comp_missed.insert(self.contour);
        }
        let compensated = match (comp, lead_in) {
            (Some(side), Some((s, _, run_in))) => {
                self.steps.push(Step::Compensation(Some(side > 0.0)));
                if run_in.is_some() {
                    self.feed([s[0], s[1], z], FeedKind::LeadIn);
                }
                true
            }
            _ => false,
        };
        if let Some((_, arc, _)) = lead_in {
            match arc {
                Some((ctr, cw)) => self.arc([start[0], start[1], z], ctr, cw, FeedKind::LeadIn),
                None => self.feed([start[0], start[1], z], FeedKind::LeadIn),
            }
        }
        match tabs_here {
            Some((top, count, width)) => {
                let total: f64 = walk.windows(2).map(|w| dist2(w[0], w[1])).sum();
                if width * count as f64 >= total * 0.8 {
                    let w =
                        "A contour too short for its tabs was cut through without them".to_string();
                    if !self.warnings.contains(&w) {
                        self.warnings.push(w);
                    }
                }
                for q in with_tabs(&walk, z, top, count, width).iter().skip(1) {
                    self.feed(*q, FeedKind::Cut);
                }
            }
            None => {
                for q in walk.iter().skip(1) {
                    self.feed([q[0], q[1], z], FeedKind::Cut);
                }
            }
        }
        self.finish_contour(c, z, cut, false);
        if compensated {
            self.end_compensation(z, cut);
        }
    }

    /// Compensation off: cancelled on a straight run on out of the lead-out (if there's room;
    /// else with the retract).
    fn end_compensation(&mut self, z: f64, cut: &Cut<'_>) {
        let l = &self.input.params.links;
        self.steps.push(Step::Compensation(None));
        let Some((a, h)) = self.at.zip(self.heading) else {
            return;
        };
        let on = [a[0] + h[0] * l.lead_radius, a[1] + h[1] * l.lead_radius];
        let e = if l.lead_out != LeadKind::None && self.lead_clear(&[[a[0], a[1]], on], cut) {
            on
        } else {
            // No room to run on: a hair back along the way just cut (cleared), so the cancel
            // is still a straight move across, never with the retract.
            [a[0] - h[0] * 0.2, a[1] - h[1] * 0.2]
        };
        self.feed([e[0], e[1], z], FeedKind::LeadOut);
    }

    /// Down to `z` from where the tool is, back and forth along `stretch` (from its start) at
    /// the ramp angle, ending back at its start at depth.
    fn zigzag_ramp(&mut self, stretch: &[P2], z: f64) {
        let Some(from) = self.at else { return };
        if stretch.len() < 2 {
            self.feed([stretch[0][0], stretch[0][1], z], FeedKind::Plunge);
            return;
        }
        let len: f64 = stretch.windows(2).map(|w| dist2(w[0], w[1])).sum();
        let drop = len
            * self
                .input
                .params
                .links
                .ramp_angle
                .clamp(0.5, 45.0)
                .to_radians()
                .tan();
        let back: Vec<P2> = stretch.iter().rev().copied().collect();
        let mut h = from[2];
        let mut forward = true;
        while h > z + 1e-9 || !forward {
            let leg = if forward { stretch } else { &back[..] };
            let target = (h - drop).max(z);
            // Down evenly along the leg.
            let mut s = 0.0;
            for w in leg.windows(2) {
                s += dist2(w[0], w[1]);
                let zz = h + (target - h) * (s / len).min(1.0);
                self.feed([w[1][0], w[1][1], zz], FeedKind::Ramp);
            }
            h = target;
            forward = !forward;
        }
    }

    /// The overlap past the start of a closed contour and the lead-out.
    fn finish_contour(&mut self, c: &Contour, z: f64, cut: &Cut<'_>, ramped: bool) {
        let l = &self.input.params.links;
        let mut end_contour = c.clone();
        if c.closed && l.overlap > 0.0 && !ramped {
            // Along the start again.
            let mut left = l.overlap;
            let walk = c.walk();
            for w in walk.windows(2) {
                let d = dist2(w[0], w[1]);
                if d >= left {
                    let q = lerp2(w[0], w[1], left / d);
                    self.feed([q[0], q[1], z], FeedKind::Cut);
                    end_contour = Contour::open(vec![w[0], q]);
                    break;
                }
                self.feed([w[1][0], w[1][1], z], FeedKind::Cut);
                left -= d;
            }
        }
        let Some(side) = cut.free_side else {
            return;
        };
        if l.lead_out == LeadKind::None || l.lead_radius <= 0.0 {
            return;
        }
        let Some((pt, t)) = end_contour.end_tangent() else {
            return;
        };
        // Leave from where the tool is, the way it's going (a ramp ends where it reached depth).
        let pt = self.at.map_or(pt, |a| [a[0], a[1]]);
        let t = self.heading.unwrap_or(t);
        // Off the wall only where what the tool sweeps is clear (else it retracts straight up).
        match l.lead_out {
            LeadKind::Arc => {
                let (e, ctr, cw) = lead_out_arc(pt, t, side, l.lead_radius, 90.0);
                if self.lead_clear(&arc_path(pt, e, ctr, cw), cut) {
                    self.arc([e[0], e[1], z], ctr, cw, FeedKind::LeadOut);
                }
            }
            _ => {
                let n = [-t[1] * side, t[0] * side];
                let e = [pt[0] + n[0] * l.lead_radius, pt[1] + n[1] * l.lead_radius];
                if self.lead_clear(&[pt, e], cut) {
                    self.feed([e[0], e[1], z], FeedKind::LeadOut);
                }
            }
        }
    }

    /// Whether a lead along `path` is clear: its centre stays in the cut's `free` region, or
    /// what it sweeps stays in its `clear` region (with neither, anywhere is).
    fn lead_clear(&self, path: &[P2], cut: &Cut<'_>) -> bool {
        match cut.free {
            Some(f) => path
                .windows(2)
                .all(|w| crate::region::segment_inside(f, w[0], w[1])),
            None => self.swept_clear(path, cut.clear),
        }
    }

    /// Whether the tool moving along `path` stays inside `clear` (`None`: anywhere is).
    fn swept_clear(&self, path: &[P2], clear: Option<&crate::region::Region>) -> bool {
        let Some(clear) = clear else {
            return true;
        };
        let r = self.input.tool.radius();
        let swept = crate::region::stroke(path, 2.0 * r * 0.98);
        crate::region::area(&crate::region::difference(&swept, clear)) < 1e-3
    }

    /// Ends at the clearance height.
    pub fn finish(mut self) -> Toolpath {
        let cl = self.input.clearance();
        self.up_to(cl);
        Toolpath {
            steps: self.steps,
            warnings: self.warnings,
        }
    }

    /// Whether anything was cut.
    pub fn moved(&self) -> bool {
        self.at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::MachineKind;
    use crate::region;
    use crate::strategy::testing::{pocket_block, stock_for};
    use crate::strategy::{Params, Pattern};
    use crate::tool::{Tool, ToolKind};

    /// Cuts two short lines at z = 5, `free` round them, and returns the steps after the first.
    fn link(free: Option<&region::Region>, lead: bool) -> Vec<Step> {
        let m = pocket_block();
        let stock = stock_for(&m, 5.0, 2.0);
        let tool = Tool::new(ToolKind::EndMill, 4.0);
        let mut p = Params::new(Pattern::Rough2D, &tool, MachineKind::Mill);
        p.links.small_gap = 100.0;
        p.links.small_gaps = LinkAction::Direct;
        p.links.lead_in = if lead { LeadKind::Arc } else { LeadKind::None };
        p.links.lead_out = LeadKind::None;
        p.links.lead_radius = 3.0;
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let mut b = Builder::new(&input);
        let cut = |direct_ok| Cut {
            material_top: 10.0,
            direct_ok,
            free,
            free_side: lead.then_some(1.0),
            tabs: None,
            ramp: false,
            clear: None,
        };
        b.cut(
            &Contour::open(vec![[0.0, 0.0], [10.0, 0.0]]),
            5.0,
            &cut(false),
        );
        let n = b.steps.len();
        b.cut(
            &Contour::open(vec![[30.0, 0.0], [40.0, 0.0]]),
            5.0,
            &cut(true),
        );
        b.steps[n..].to_vec()
    }

    fn stays_down(steps: &[Step]) -> bool {
        matches!(
            steps.first(),
            Some(Step::Feed {
                kind: FeedKind::Link,
                ..
            })
        )
    }

    #[test]
    fn stays_down_only_where_the_way_is_free() {
        let open = vec![region::rect([-10.0, -10.0], [50.0, 10.0])];
        assert!(stays_down(&link(Some(&open), false)));
        // A wall across the way (the free area split in two): up and over.
        let split = region::difference(&open, &[region::rect([18.0, -20.0], [22.0, 20.0])]);
        let steps = link(Some(&split), false);
        assert!(!stays_down(&steps));
        assert!(matches!(steps[0], Step::Rapid(p) if p[2] > 5.0));
        // Unknown: never stays down.
        assert!(!stays_down(&link(None, false)));
    }

    #[test]
    fn leads_keep_to_the_free_area() {
        let open = vec![region::rect([-10.0, -10.0], [50.0, 10.0])];
        let has_lead = |s: &[Step]| {
            s.iter().any(|s| {
                matches!(
                    s,
                    Step::Arc {
                        kind: FeedKind::LeadIn,
                        ..
                    }
                )
            })
        };
        assert!(has_lead(&link(Some(&open), true)));
        // Free only along the line's band: no room for a 3 mm lead beside it.
        let band = vec![region::rect([-10.0, -0.5], [50.0, 0.5])];
        assert!(!has_lead(&link(Some(&band), true)));
    }
}
