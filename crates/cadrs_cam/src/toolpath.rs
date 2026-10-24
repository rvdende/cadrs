//! Toolpaths: what a strategy makes and a post turns into G-code.
//!
//! A [`Toolpath`] is a list of [`Step`]s in its frame's coordinates (mm, mm/min): rapid and
//! feed moves, arcs, drilling cycles (kept as cycles so a post can write canned cycles),
//! process switches (spindle, beam, coolant), dwells and comments. [`Toolpath::motion`] expands
//! it into straight [`Segment`]s with their times, which the statistics, the back plot and the
//! stock simulation use.

use serde::{Deserialize, Serialize};

use crate::geom::{Aabb, P2, P3, dist, lerp};

/// Why a feed move is made (the back plot colours them; the post doesn't care).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeedKind {
    /// Cutting.
    Cut,
    /// Straight down into the material.
    Plunge,
    /// A ramp or helix down into the material.
    Ramp,
    /// Toward the cut (lead-in).
    LeadIn,
    /// Away from the cut (lead-out).
    LeadOut,
    /// A link at feed between cuts.
    Link,
    /// Up out of the material at feed.
    Retract,
}

/// A drilling cycle at one hole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DrillCycle {
    /// The hole's position.
    pub at: P2,
    /// Where feeding starts (the R plane).
    pub r_plane: f64,
    /// The hole's bottom (the tip's lowest z).
    pub bottom: f64,
    /// Where the tool goes after the hole (the clearance or the R plane).
    pub retract: f64,
    pub feed: f64,
    pub kind: CycleKind,
    /// Depth per peck (`Peck` and `ChipBreak`).
    pub peck: f64,
    /// Dwell at the bottom (s).
    pub dwell: f64,
    /// Thread pitch (`Tap`), mm per turn.
    pub pitch: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CycleKind {
    /// Feed to the bottom, rapid out (G81, G82 with a dwell).
    Drill,
    /// Peck, retracting fully between pecks (G83).
    Peck,
    /// Peck, backing off a little between pecks (G73).
    ChipBreak,
    /// Rigid tap (G84).
    Tap,
}

impl CycleKind {
    pub fn label(self) -> &'static str {
        match self {
            CycleKind::Drill => "Drill",
            CycleKind::Peck => "Peck drill",
            CycleKind::ChipBreak => "Chip break",
            CycleKind::Tap => "Tap",
        }
    }
}

/// How the coolant runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Coolant {
    #[default]
    Off,
    Flood,
    Mist,
    Air,
}

impl Coolant {
    pub const ALL: [Coolant; 4] = [Coolant::Off, Coolant::Flood, Coolant::Mist, Coolant::Air];
    pub fn label(self) -> &'static str {
        match self {
            Coolant::Off => "Off",
            Coolant::Flood => "Flood",
            Coolant::Mist => "Mist",
            Coolant::Air => "Air blast",
        }
    }
}

/// One instruction of a toolpath.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Step {
    /// At the rapid rate.
    Rapid(P3),
    Feed {
        to: P3,
        feed: f64,
        kind: FeedKind,
    },
    /// An arc in the XY plane about `center` (z may change: a helix).
    Arc {
        to: P3,
        center: P2,
        /// Clockwise seen from +Z.
        cw: bool,
        feed: f64,
        kind: FeedKind,
    },
    Drill(DrillCycle),
    /// Spindle on at `rpm` (clockwise for a right-hand tool).
    Spindle {
        rpm: f64,
        cw: bool,
    },
    SpindleOff,
    /// Beam or torch on (power as a fraction 0–1).
    BeamOn {
        power: f64,
    },
    BeamOff,
    Coolant(Coolant),
    Dwell(f64),
    Comment(String),
    /// Cutter compensation from the next straight move on: the tool left of the path (G41) or
    /// right of it (G42); `None` cancels it (G40). For wear offsets on a path that is already the
    /// tool's centre.
    Compensation(Option<bool>),
}

/// A straight piece of the expanded motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub from: P3,
    pub to: P3,
    /// `None` for a rapid.
    pub feed: Option<f64>,
    pub kind: Option<FeedKind>,
    /// Whether the beam is on (beam machines) while moving.
    pub beam: bool,
    /// Its time (min), including any dwell before it.
    pub minutes: f64,
}

impl Segment {
    pub fn is_rapid(&self) -> bool {
        self.feed.is_none()
    }
    /// Whether it removes material: a feed move (spindle machines), or a move with the beam on.
    pub fn cuts(&self, beam_machine: bool) -> bool {
        if beam_machine {
            // (A laser in laser mode is off during rapids.)
            self.beam && !self.is_rapid()
        } else {
            self.feed.is_some()
        }
    }
}

/// What a toolpath does: lengths, time, pierces.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Stats {
    /// Length at feed (mm).
    pub feed_length: f64,
    /// Length at rapid (mm).
    pub rapid_length: f64,
    /// Estimated time (min).
    pub minutes: f64,
    /// Beam starts (pierces) or plunges into material.
    pub pierces: usize,
}

/// The output of a strategy.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Toolpath {
    pub steps: Vec<Step>,
    /// Messages from the strategy (areas it couldn't reach, holes too small for the tool).
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Chord tolerance arcs are expanded to (mm).
const ARC_CHORD: f64 = 0.005;

impl Toolpath {
    /// Brings its spindle speeds down to `max` rpm, slowing every feed by the same ratio so
    /// each tooth still takes the chip it was set for; the ratio when it had to (else `None`).
    pub fn limit_spindle(&mut self, max: f64) -> Option<f64> {
        let fastest = self
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Spindle { rpm, .. } => Some(*rpm),
                _ => None,
            })
            .fold(0.0, f64::max);
        if max <= 0.0 || fastest <= max + 1e-9 {
            return None;
        }
        let k = max / fastest;
        for s in &mut self.steps {
            match s {
                Step::Spindle { rpm, .. } => *rpm *= k,
                Step::Feed { feed, .. } | Step::Arc { feed, .. } => *feed *= k,
                // A tap's feed is its pitch a turn: it follows the spindle the same way.
                Step::Drill(c) => c.feed *= k,
                _ => {}
            }
        }
        Some(k)
    }

    pub fn is_empty(&self) -> bool {
        !self.steps.iter().any(|s| {
            matches!(
                s,
                Step::Rapid(_) | Step::Feed { .. } | Step::Arc { .. } | Step::Drill(_)
            )
        })
    }

    /// The first position it moves to.
    pub fn start(&self) -> Option<P3> {
        self.steps.iter().find_map(|s| match s {
            Step::Rapid(p) => Some(*p),
            Step::Feed { to, .. } | Step::Arc { to, .. } => Some(*to),
            Step::Drill(c) => Some([c.at[0], c.at[1], c.retract]),
            _ => None,
        })
    }

    /// The motion as straight segments, from `from` (the position before the toolpath; its
    /// first move starts there), at `rapid_rate` mm/min for rapids.
    pub fn motion(&self, from: P3, rapid_rate: f64) -> Vec<Segment> {
        let mut m = Mover {
            out: Vec::new(),
            at: from,
            beam: false,
            dwell: 0.0,
            rapid_rate: rapid_rate.max(1.0),
        };
        for s in &self.steps {
            match s {
                Step::Rapid(p) => m.go(*p, None, None),
                Step::Feed { to, feed, kind } => m.go(*to, Some(*feed), Some(*kind)),
                Step::Arc {
                    to,
                    center,
                    cw,
                    feed,
                    kind,
                } => {
                    for p in arc_points(m.at, *to, *center, *cw, ARC_CHORD) {
                        m.go(p, Some(*feed), Some(*kind));
                    }
                }
                Step::Drill(c) => m.drill(c),
                Step::BeamOn { .. } => m.beam = true,
                Step::BeamOff => m.beam = false,
                Step::Dwell(s) => m.dwell += s,
                _ => {}
            }
        }
        if m.dwell > 0.0 {
            let at = m.at;
            m.out.push(Segment {
                from: at,
                to: at,
                feed: None,
                kind: None,
                beam: m.beam,
                minutes: m.dwell / 60.0,
            });
        }
        m.out
    }

    /// Lengths, time and pierces, starting at `from`.
    pub fn stats(&self, from: P3, rapid_rate: f64) -> Stats {
        let segs = self.motion(from, rapid_rate);
        let mut s = Stats::default();
        for g in &segs {
            let l = dist(g.from, g.to);
            if g.is_rapid() {
                s.rapid_length += l;
            } else {
                s.feed_length += l;
            }
            s.minutes += g.minutes;
        }
        // A ramp is several steps; count each run of plunges once.
        let mut runs = 0;
        let mut prev = false;
        for st in &self.steps {
            let p = matches!(
                st,
                Step::Feed {
                    kind: FeedKind::Plunge | FeedKind::Ramp,
                    ..
                } | Step::Arc {
                    kind: FeedKind::Ramp,
                    ..
                }
            );
            if p && !prev {
                runs += 1;
            }
            prev = p;
        }
        let beams = self
            .steps
            .iter()
            .filter(|st| matches!(st, Step::BeamOn { .. }))
            .count();
        let drills = self
            .steps
            .iter()
            .filter(|st| matches!(st, Step::Drill(_)))
            .count();
        s.pierces = if beams > 0 { beams } else { runs + drills };
        s
    }

    /// The box of every position it reaches.
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for s in &self.steps {
            match s {
                Step::Rapid(p) | Step::Feed { to: p, .. } | Step::Arc { to: p, .. } => {
                    b.include(*p)
                }
                Step::Drill(c) => {
                    b.include([c.at[0], c.at[1], c.bottom]);
                    b.include([c.at[0], c.at[1], c.retract.max(c.r_plane)]);
                }
                _ => {}
            }
        }
        b
    }
}

/// Expands steps into segments (see [`Toolpath::motion`]).
struct Mover {
    out: Vec<Segment>,
    at: P3,
    beam: bool,
    /// Dwell waiting to be added to the next segment's time (s).
    dwell: f64,
    rapid_rate: f64,
}

impl Mover {
    fn go(&mut self, to: P3, feed: Option<f64>, kind: Option<FeedKind>) {
        let rate = feed.unwrap_or(self.rapid_rate).max(1e-6);
        self.out.push(Segment {
            from: self.at,
            to,
            feed,
            kind,
            beam: self.beam,
            minutes: dist(self.at, to) / rate + self.dwell / 60.0,
        });
        self.dwell = 0.0;
        self.at = to;
    }

    fn drill(&mut self, c: &DrillCycle) {
        let (x, y) = (c.at[0], c.at[1]);
        let top = [x, y, c.r_plane];
        let bottom = [x, y, c.bottom];
        // Over the hole (never lower than the R plane), down to R.
        let over = self.at[2].max(c.r_plane);
        self.go([x, y, over], None, None);
        self.go(top, None, None);
        match c.kind {
            CycleKind::Drill | CycleKind::Tap => {
                self.go(bottom, Some(c.feed), Some(FeedKind::Plunge));
                self.dwell += c.dwell;
                // A tap feeds out (the spindle reverses); a drill rapids.
                if c.kind == CycleKind::Tap {
                    self.go(top, Some(c.feed), Some(FeedKind::Retract));
                } else {
                    self.go(top, None, None);
                }
            }
            CycleKind::Peck | CycleKind::ChipBreak => {
                let peck = if c.peck > 0.0 { c.peck } else { f64::INFINITY };
                let mut depth = c.r_plane;
                while depth > c.bottom + 1e-9 {
                    let next = (depth - peck).max(c.bottom);
                    // After a full retract, back down to just above the last depth.
                    let start = if c.kind == CycleKind::Peck {
                        [x, y, (depth + 0.5).min(c.r_plane)]
                    } else {
                        [x, y, depth]
                    };
                    if dist(self.at, start) > 1e-9 {
                        self.go(start, None, None);
                    }
                    self.go([x, y, next], Some(c.feed), Some(FeedKind::Plunge));
                    depth = next;
                    if depth > c.bottom + 1e-9 {
                        let back = if c.kind == CycleKind::Peck {
                            top
                        } else {
                            [x, y, depth + 0.5]
                        };
                        self.go(back, None, None);
                    }
                }
                self.dwell += c.dwell;
                self.go(top, None, None);
            }
        }
        self.go([x, y, c.retract], None, None);
    }
}

/// The points along an XY arc from `from` to `to` about `center` (z interpolated), excluding
/// `from`, within `chord` of the true arc. A full circle when `to` equals `from` in XY.
pub fn arc_points(from: P3, to: P3, center: P2, cw: bool, chord: f64) -> Vec<P3> {
    let r = ((from[0] - center[0]).powi(2) + (from[1] - center[1]).powi(2)).sqrt();
    let a0 = (from[1] - center[1]).atan2(from[0] - center[0]);
    let a1 = (to[1] - center[1]).atan2(to[0] - center[0]);
    let tau = std::f64::consts::TAU;
    let mut sweep = if cw { a0 - a1 } else { a1 - a0 };
    while sweep <= 1e-9 {
        sweep += tau;
    }
    if sweep > tau + 1e-9 {
        sweep -= tau;
    }
    let n = crate::region::segments_for(r.max(chord), sweep)
        .max((sweep / (2.0 * (1.0 - chord / r.max(chord)).acos().max(1e-3))).ceil() as usize)
        .max(1);
    (1..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let a = if cw { a0 - sweep * t } else { a0 + sweep * t };
            let z = from[2] + (to[2] - from[2]) * t;
            if i == n {
                to
            } else {
                [center[0] + r * a.cos(), center[1] + r * a.sin(), z]
            }
        })
        .collect()
}

/// A position along a motion at a time: for the back plot.
#[derive(Debug, Clone)]
pub struct Timeline {
    pub segments: Vec<Segment>,
    /// Cumulative minutes at the end of each segment.
    ends: Vec<f64>,
}

impl Timeline {
    pub fn new(segments: Vec<Segment>) -> Timeline {
        let mut t = 0.0;
        let ends = segments
            .iter()
            .map(|s| {
                t += s.minutes.max(0.0);
                t
            })
            .collect();
        Timeline { segments, ends }
    }

    pub fn total(&self) -> f64 {
        self.ends.last().copied().unwrap_or(0.0)
    }

    /// Where the tool is at `t` minutes, and the index of the segment it is on.
    pub fn at(&self, t: f64) -> Option<(P3, usize)> {
        if self.segments.is_empty() {
            return None;
        }
        let i = self
            .ends
            .partition_point(|e| *e < t)
            .min(self.segments.len() - 1);
        let s = &self.segments[i];
        let start = if i == 0 { 0.0 } else { self.ends[i - 1] };
        let f = if s.minutes > 0.0 {
            ((t - start) / s.minutes).clamp(0.0, 1.0)
        } else {
            1.0
        };
        Some((lerp(s.from, s.to, f), i))
    }

    /// The time at the end of segment `i`.
    pub fn end_of(&self, i: usize) -> f64 {
        self.ends.get(i).copied().unwrap_or_else(|| self.total())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_count_lengths_and_time() {
        let tp = Toolpath {
            steps: vec![
                Step::Rapid([0.0, 0.0, 10.0]),
                Step::Feed {
                    to: [0.0, 0.0, 0.0],
                    feed: 100.0,
                    kind: FeedKind::Plunge,
                },
                Step::Feed {
                    to: [100.0, 0.0, 0.0],
                    feed: 1000.0,
                    kind: FeedKind::Cut,
                },
                Step::Rapid([100.0, 0.0, 10.0]),
            ],
            warnings: vec![],
        };
        let s = tp.stats([0.0, 0.0, 10.0], 5000.0);
        assert!((s.feed_length - 110.0).abs() < 1e-9);
        assert!((s.rapid_length - 10.0).abs() < 1e-9);
        assert!((s.minutes - (0.1 + 0.1 + 10.0 / 5000.0)).abs() < 1e-9);
        assert_eq!(s.pierces, 1);
    }

    #[test]
    fn arc_expands_within_chord() {
        let pts = arc_points([10.0, 0.0, 0.0], [10.0, 0.0, -1.0], [0.0, 0.0], false, 0.01);
        assert!(pts.len() > 20);
        for p in &pts {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!((r - 10.0).abs() < 1e-9);
        }
        assert_eq!(*pts.last().unwrap(), [10.0, 0.0, -1.0]);
        // Half way round, the helix is half way down.
        let mid = pts[pts.len() / 2];
        assert!(mid[0] < -9.0 && (mid[2] + 0.5).abs() < 0.1);
    }

    #[test]
    fn peck_cycle_expands() {
        let tp = Toolpath {
            steps: vec![Step::Drill(DrillCycle {
                at: [5.0, 5.0],
                r_plane: 2.0,
                bottom: -10.0,
                retract: 10.0,
                feed: 100.0,
                kind: CycleKind::Peck,
                peck: 4.0,
                dwell: 0.0,
                pitch: 0.0,
            })],
            warnings: vec![],
        };
        let segs = tp.motion([0.0, 0.0, 10.0], 1000.0);
        let lowest = segs.iter().map(|s| s.to[2]).fold(f64::INFINITY, f64::min);
        assert!((lowest + 10.0).abs() < 1e-9);
        let plunges = segs
            .iter()
            .filter(|s| s.kind == Some(FeedKind::Plunge))
            .count();
        assert_eq!(plunges, 3);
        assert_eq!(segs.last().unwrap().to, [5.0, 5.0, 10.0]);
    }

    #[test]
    fn timeline_interpolates() {
        let tp = Toolpath {
            steps: vec![Step::Feed {
                to: [10.0, 0.0, 0.0],
                feed: 10.0,
                kind: FeedKind::Cut,
            }],
            warnings: vec![],
        };
        let tl = Timeline::new(tp.motion([0.0; 3], 100.0));
        assert!((tl.total() - 1.0).abs() < 1e-12);
        let (p, i) = tl.at(0.5).unwrap();
        assert_eq!(i, 0);
        assert!((p[0] - 5.0).abs() < 1e-9);
    }
}
