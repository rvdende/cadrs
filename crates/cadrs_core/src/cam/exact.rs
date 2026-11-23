//! Simulate's exact stock: the stock as a solid of the kernel's, cut by the tool's own solid
//! (its profile revolved: lines and arcs, not facets) along the motion the machine makes,
//! each cut the kernel's exact cut by a swept solid (`Kernel::cut_swept`), and drawn from the
//! solid's own tessellation.
//!
//! **The motion.** Each move takes the tool from the pose the machine holds it in at the
//! move's start to the one at its end (its axes' values from [`super::program::axis_track`],
//! the tool's frame from the machine's chain). Between two poses the tool is taken by the
//! screw that joins them (any rigid motion between two poses is one: a slide, a turn, or a
//! turn and a slide along its axis at once), which the kernel cuts exactly. Where the machine
//! moves its axes so that the tool strays from that screw (two rotary axes turning at once,
//! or a slide across a turning axis) by more than [`STRAY`], the move is taken in pieces, each
//! its own screw. A worm's thread (X along the A axis while A turns) is one screw exactly.
//!
//! **Cuts.** Moves one after another on the same screw (a thread's chords, a pass turned about
//! one axis, a straight run of collinear moves) are one cut: one boolean, however many moves.
//!
//! **As it plays** ([`ExactStock::cut_to`]): the stock after every finished cut is kept, and
//! only the cut the tool is on is made, from the stock before it, as far as the tool has gone:
//! nothing is cut twice (going back starts again from the uncut stock).

use std::collections::HashMap;
use std::f64::consts::TAU;

use cadrs_cam::cutter::{Cutter, Piece};
use cadrs_cam::geom::{Frame, P3};
use cadrs_cam::kinematics::Chain;
use cadrs_cam::toolpath::Segment;
use cadrs_cam::{Machine, Tool};
use cadrs_kernel::{
    Axis, BodyId, Curve2, DefaultKernel, Kernel, Loop, Motion, Plane, Primitive, Profile,
    RevolveSpec, SolidMotion, Tessellation, TriMesh,
};
use nalgebra::{Matrix3, Point3, Rotation3, Vector3};

use super::program::{SetupRun, StockShape};

type V3 = Vector3<f64>;

/// How far the tool may stray from the screw a move is cut by before the move is taken in
/// pieces (mm, at the tool's tip and at its top).
pub const STRAY: f64 = 0.002;

/// How far the tool may be, at the end of any move of a cut, from where the cut's one screw
/// has it then (mm, at its tip and its top): moves on one screw within this are one cut.
pub const MERGE: f64 = 0.002;

/// [`MERGE`], or `CADRS_EXACT_MERGE` (mm) to try others.
fn merge() -> f64 {
    std::env::var("CADRS_EXACT_MERGE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(MERGE)
}

/// Whether two screws are one going on: the same axis (the very same line, as the machine's
/// own screws about one of its axes are) and the same lead.
fn on_same_screw(a: &Screw, b: &Screw) -> bool {
    match (*a, *b) {
        (
            Screw::Turn {
                at: a0,
                dir: d0,
                angle: t0,
                rise: r0,
            },
            Screw::Turn {
                at: a1,
                dir: d1,
                angle: t1,
                rise: r1,
            },
        ) => {
            (a0 - a1).norm() <= EXACT_POSE
                && (d0 - d1).norm() <= 1e-12
                && (r0 / t0 - r1 / t1).abs() <= 1e-12 * (1.0 + (r0 / t0).abs())
        }
        _ => false,
    }
}

/// The longest turn a cut's screw is fitted over between its two ends (radians); a longer
/// one goes on with the screw it has.
const LONGEST_FIT: f64 = 2.5;

/// The most moves one cut takes (so extending one stays cheap).
const MOST_MOVES: usize = 100_000;

/// The tool's pose: its tip, and its frame's axes (its own axis the third column).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub rot: Matrix3<f64>,
    pub tip: V3,
}

impl Pose {
    fn motion(&self) -> Motion {
        Motion {
            linear: self.rot,
            translation: self.tip,
        }
    }

    fn apply(&self, p: V3) -> V3 {
        self.rot * p + self.tip
    }
}

/// The motion taking the tool from one pose to the next: a slide, or a screw (a turn about an
/// axis and a slide along it).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Screw {
    Slide(V3),
    Turn {
        /// A point of the axis, its direction.
        at: V3,
        dir: V3,
        /// The turn (radians, right-handed about `dir`) and the slide along `dir` with it.
        angle: f64,
        rise: f64,
    },
}

impl Screw {
    /// The screw from `a` to `b`.
    fn between(a: &Pose, b: &Pose) -> Screw {
        let r = b.rot * a.rot.transpose();
        let t = b.tip - r * a.tip;
        let rot = Rotation3::from_matrix_unchecked(r);
        match rot.axis_angle() {
            Some((u, angle)) if angle > 1e-9 => {
                let u = u.into_inner();
                let rise = t.dot(&u);
                let w = t - u * rise;
                // (I − R) c = w for the point c of the axis square to it.
                let at = (w + u.cross(&w) / (angle / 2.0).tan()) / 2.0;
                Screw::Turn {
                    at,
                    dir: u,
                    angle,
                    rise,
                }
            }
            _ => Screw::Slide(t),
        }
    }

    /// The motion's place `f` of the way along it.
    fn at(&self, f: f64) -> Motion {
        match *self {
            Screw::Slide(v) => Motion::translation(v * f),
            Screw::Turn {
                at,
                dir,
                angle,
                rise,
            } => Motion::rotation(&Axis::new(Point3::from(at), dir), angle * f)
                .then(&Motion::translation(dir * (rise * f))),
        }
    }

    /// Where it takes the pose `a` to.
    fn moved(&self, a: &Pose) -> Pose {
        let m = self.at(1.0);
        Pose {
            rot: m.linear * a.rot,
            tip: m.linear * a.tip + m.translation,
        }
    }

    /// The amount of the motion (to share a cut out among its moves): its length, or its turn.
    fn amount(&self) -> f64 {
        match *self {
            Screw::Slide(v) => v.norm(),
            Screw::Turn { angle, .. } => angle,
        }
    }

    /// The kernel's motion of the tool, drawn where it starts, `f` of the way along.
    fn solid_motion(&self, f: f64) -> SolidMotion {
        match *self {
            Screw::Slide(v) => SolidMotion::slide(v * f),
            Screw::Turn {
                at,
                dir,
                angle,
                rise,
            } => {
                let axis = Axis::new(Point3::from(at), dir);
                if rise.abs() < 1e-9 {
                    SolidMotion::swing(axis, 0.0, angle * f)
                } else {
                    SolidMotion::helical(axis, rise / angle * TAU, 0.0, angle * f)
                }
            }
        }
    }
}

/// One cut: a stretch of the run (by places along it: a move's index and how far along it,
/// `n + f`) of the operation `op`, made by one motion from the pose `start`.
#[derive(Debug, Clone)]
struct Cut {
    op: usize,
    start: Pose,
    screw: Screw,
    /// Places along the run, and how much of the cut's motion is done there (its length or
    /// turn so far): from its start to its end, a knot at each move's end within it.
    knots: Vec<(f64, f64)>,
    /// The tool's pose at each knot, and how far along the moves are there (their amounts
    /// summed), for the cut to be extended.
    poses: Vec<(Pose, f64)>,
    /// The tool's length to the top of its flutes.
    top: f64,
    /// Its motion is the machine's own screw about one of its axes ([`machine_screw`]) or an
    /// arc's ([`arc_screw`]), not one fitted to its poses.
    machine: bool,
}

/// A tool's motion: the operation's tool from a pose by a screw.
#[derive(Debug, Clone, Copy)]
struct Moving {
    op: usize,
    start: Pose,
    screw: Screw,
}

/// How far two cuts joined as one slide (where one failed) may take the tool from where the
/// moves have it: a fifth of the shown stock's deflection.
const JOINED: f64 = 0.002 * 5.0;

impl Cut {
    /// Extended by a move to `pose` at the run's place `at`, the motion `screw` from the last:
    /// when one screw from the cut's start still holds the tool at every knot within
    /// [`MERGE`].
    fn extend(&mut self, pose: Pose, screw: &Screw, at: f64, machine: bool) -> bool {
        if self.poses.len() > MOST_MOVES || (machine && !self.machine) {
            return false;
        }

        // The machine's own screw: one cut while it is the very same screw going on, or a move
        // the same screw turned on by its turn takes the tool to within [`MERGE`] of where
        // the machine has it.
        if self.machine {
            let on = machine && on_same_screw(&self.screw, screw);
            let near = !on
                && matches!((self.screw, *screw), (
                    Screw::Turn { dir: d0, .. },
                    Screw::Turn { dir: d1, .. },
                ) if d0.dot(&d1) > 0.0)
                && {
                    let Screw::Turn {
                        angle: a1,
                        rise: r1,
                        ..
                    } = *screw
                    else {
                        unreachable!()
                    };
                    let Screw::Turn {
                        at,
                        dir,
                        angle,
                        rise,
                    } = self.screw
                    else {
                        unreachable!()
                    };
                    let end = Screw::Turn {
                        at,
                        dir,
                        angle: angle + a1,
                        rise: rise + r1,
                    }
                    .moved(&self.start);
                    [V3::zeros(), V3::new(0.0, 0.0, self.top)]
                        .iter()
                        .all(|q| (end.apply(*q) - pose.apply(*q)).norm() <= merge())
                };
            if !on && !near {
                return false;
            }
            let total = self.poses.last().map_or(0.0, |p| p.1) + screw.amount();
            self.screw = match (self.screw, *screw) {
                (
                    Screw::Turn {
                        at,
                        dir,
                        angle,
                        rise,
                    },
                    Screw::Turn {
                        angle: a1,
                        rise: r1,
                        ..
                    },
                ) => Screw::Turn {
                    at,
                    dir,
                    angle: angle + a1,
                    rise: rise + r1,
                },
                (s, _) => s,
            };
            self.poses.push((pose, total));
            self.knots.push((at, total));
            return true;
        }
        let total = self.poses.last().map_or(0.0, |p| p.1) + screw.amount();
        // The screw from the cut's start to `pose`: fitted between the two while the turn is
        // short of half one (beyond, a rotation is the shorter one the other way), then the
        // same screw turned on by the move's turn and rise.
        let joined = match (self.screw, *screw) {
            (
                Screw::Turn {
                    at,
                    dir,
                    angle,
                    rise,
                },
                Screw::Turn {
                    dir: d1,
                    angle: a1,
                    rise: r1,
                    ..
                },
            ) if angle + a1 > LONGEST_FIT && d1.dot(&dir) > 0.0 => Screw::Turn {
                at,
                dir,
                angle: angle + a1,
                rise: rise + r1,
            },
            _ => Screw::between(&self.start, &pose),
        };
        // (Turned on by the move: the moves before end where they did, and only the new end
        // is checked.)
        let carried = matches!((self.screw, joined), (
            Screw::Turn { at: a0, dir: d0, .. },
            Screw::Turn { at: a1, dir: d1, .. },
        ) if a0 == a1 && d0 == d1);
        if std::mem::discriminant(&joined) != std::mem::discriminant(&self.screw)
            || joined.amount() < 1e-12
        {
            return false;
        }
        let tops = [V3::zeros(), V3::new(0.0, 0.0, self.top)];
        if std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "3") {
            let worst = self
                .poses
                .iter()
                .map(|(p, amount)| {
                    let m = joined.at(amount / total);
                    tops.iter()
                        .map(|q| {
                            (m.linear * self.start.apply(*q) + m.translation - p.apply(*q)).norm()
                        })
                        .fold(0.0, f64::max)
                })
                .fold(0.0, f64::max);
            if worst > merge() {
                eprintln!(
                    "exact merge: a cut of {} moves ends, the next move {:?} strays {worst:.2e}",
                    self.poses.len(),
                    screw
                );
            }
        }
        let holds = carried
            || self.poses.iter().all(|(p, amount)| {
                let m = joined.at(amount / total);
                tops.iter().all(|q| {
                    (m.linear * self.start.apply(*q) + m.translation - p.apply(*q)).norm()
                        <= merge()
                })
            }) && {
                // (And the new end, where the joined screw takes the tool.)
                let end = joined.moved(&self.start);
                tops.iter()
                    .all(|q| (end.apply(*q) - pose.apply(*q)).norm() <= merge())
            };
        if !holds {
            return false;
        }
        self.screw = joined;
        self.poses.push((pose, total));
        self.knots.push((at, total));
        true
    }

    fn from(&self) -> f64 {
        self.knots[0].0
    }

    fn to(&self) -> f64 {
        self.knots[self.knots.len() - 1].0
    }

    /// How far along the cut the run's place `at` is.
    fn along(&self, at: f64) -> f64 {
        let i = self
            .knots
            .partition_point(|k| k.0 < at)
            .clamp(1, self.knots.len() - 1);
        let (a, b) = (self.knots[i - 1], self.knots[i]);
        let total = self.knots[self.knots.len() - 1].1.max(1e-300);
        let amount = if b.0 - a.0 < 1e-12 {
            b.1
        } else {
            a.1 + (b.1 - a.1) * ((at - a.0) / (b.0 - a.0)).clamp(0.0, 1.0)
        };
        amount / total
    }
}

/// The tool's pose at the axis values `q` in the setup's coordinates (`work`: the setup's frame
/// in the table mount's), the tool `length` from its tip to the spindle's gauge line.
fn pose_at(chain: &Chain, work: &Frame, q: &[f64], length: f64) -> Pose {
    let (tool, table) = chain.mounts(q);
    let f = work.relative(&table.relative(&tool));
    let v = |p: P3| V3::new(p[0], p[1], p[2]);
    let (x, y, z) = (v(f.x), v(f.y), v(f.z));
    Pose {
        rot: Matrix3::from_columns(&[x, y, z]),
        tip: v(f.origin) - z * length,
    }
}

/// The machine's rotary axes in the setup's coordinates (each a point and a direction) at the
/// axis values `q`.
fn rotary_lines(chain: &Chain, work: &Frame, q: &[f64]) -> Vec<(V3, V3)> {
    use cadrs_cam::kinematics::Xf;
    let (_, table) = chain.mounts(q);
    let v = |p: P3| V3::new(p[0], p[1], p[2]);
    let split = chain.tool.len().min(q.len());
    let mut out = Vec::new();
    for (joints, values) in [(&chain.tool, &q[..split]), (&chain.table, &q[split..])] {
        let mut before = Xf::IDENTITY;
        for (j, value) in joints.iter().zip(values) {
            if j.is_rotary() {
                let (p, d) = (before.point(j.origin), before.dir(j.dir));
                out.push((
                    v(work.to_local(table.to_local(p))),
                    v(work.dir_to_local(table.dir_to_local(d))).normalize(),
                ));
            }
            before = before.then(&j.motion(*value));
        }
    }
    out
}

/// The screw about one of the machine's rotary axes (`lines`, from [`rotary_lines`]) that takes
/// the tool between two of its poses `a` and `b`, where the machine's own places on the move
/// (`mid`, half way, and `b`) are within [`MERGE`] of it: the move turning about that axis
/// with the slides along it (exactly so, to rounding, where only those axes move; within the
/// toolpath's own scatter where the move also follows the part a hair). Cut on the machine's
/// own axis, the passes about it share it exactly: their walls, which lie on one another
/// level after level, are one surface and not two a few microns apart. `None`: the move
/// isn't one.
fn machine_screw(
    a: &Pose,
    b: &Pose,
    mid: &Pose,
    fitted: &Screw,
    lines: &[(V3, V3)],
    top: f64,
) -> Option<Screw> {
    let Screw::Turn {
        at,
        dir,
        angle,
        rise,
    } = *fitted
    else {
        return None;
    };
    let tops = [V3::zeros(), V3::new(0.0, 0.0, top)];
    lines.iter().find_map(|(p, d)| {
        let cos = d.dot(&dir);
        let off = (at - p) - d * (at - p).dot(d);
        if cos.abs() < 1.0 - 1e-9 || off.norm() > 1e-3 {
            return None;
        }
        let s = Screw::Turn {
            at: *p,
            dir: d * cos.signum(),
            angle,
            rise,
        };
        let worst = [(0.5, mid), (1.0, b)]
            .iter()
            .flat_map(|(f, want)| {
                let m = s.at(*f);
                tops.map(|q| (m.linear * a.apply(q) + m.translation - want.apply(q)).norm())
            })
            .fold(0.0, f64::max);
        if std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "5") && worst > merge() {
            eprintln!("machine screw: misses by {worst:.3e}, axis {off:.3e} off");
        }
        let holds = worst <= merge();
        holds.then_some(s)
    })
}

/// The turn about the upright line through `centre` (an arc's, in the setup's XY) that takes
/// the tool from `a` to `b`, rising with a helix's: where the tool is upright and both its
/// tips are as far from the line (to a micron). The machine moves it so for a G2/G3; the
/// tool turns about its own axis on the way, which leaves the space it sweeps the same.
fn arc_screw(a: &Pose, b: &Pose, centre: [f64; 2]) -> Option<Screw> {
    let up = V3::z();
    if a.rot.column(2).dot(&up) < 1.0 - 1e-12 || b.rot.column(2).dot(&up) < 1.0 - 1e-12 {
        return None;
    }
    let c = V3::new(centre[0], centre[1], 0.0);
    let (ra, rb) = ((a.tip - c).xy(), (b.tip - c).xy());
    if ra.norm() < 1e-6 || (ra.norm() - rb.norm()).abs() > 1e-3 {
        return None;
    }
    let turn = ra.perp(&rb).atan2(ra.dot(&rb));
    if turn.abs() < 1e-12 {
        return None;
    }
    let dir = up * turn.signum();
    Some(Screw::Turn {
        at: c,
        dir,
        angle: turn.abs(),
        rise: (b.tip.z - a.tip.z) * dir.z,
    })
}

/// The pose `held` with the tool's turn about its own axis on an arc ([`arc_screw`]) undone:
/// as the machine holds it at `machine`, the tip where `held` has it. (A tool is round about
/// its axis: only the moves after it would turn it back.)
fn unspun(held: Pose, machine: &Pose) -> Pose {
    Pose {
        rot: machine.rot,
        tip: held.tip,
    }
}

/// How near the machine's own places a screw about one of its axes must take the tool to be
/// that motion (mm): rounding.
const EXACT_POSE: f64 = 1e-9;

/// The run's cuts: each operation's moves through the machine's axes (`rows`: the axis values
/// before the first move and at the end of each, through the run).
fn cuts_of(
    machine: &Machine,
    run: &SetupRun,
    motions: &[Vec<Segment>],
    rows: &[Vec<f64>],
) -> Vec<Cut> {
    let chain = machine.kinematics();
    let mut cuts: Vec<Cut> = Vec::new();
    let mut g = 0;
    // Where the tool is as the cuts leave it: each cut starts where the one before ended (on
    // the same faces), and a rapid puts it back where the machine has it.
    let mut held: Option<Pose> = None;
    for (k, (moves, o)) in motions.iter().zip(&run.ops).enumerate() {
        let length = o.tool.gauge_length();
        let pose = |q: &[f64]| pose_at(&chain, &run.work, q, length);
        for s in moves {
            let (Some(q0), Some(q1)) = (rows.get(g), rows.get(g + 1)) else {
                break;
            };
            let here = g;
            g += 1;
            if s.is_rapid() {
                held = None;
                continue;
            }
            let lines = rotary_lines(&chain, &run.work, q0);
            // The move in as many pieces as keep the tool on their screws.
            let lerp =
                |f: f64| -> Vec<f64> { q0.iter().zip(q1).map(|(a, b)| a + (b - a) * f).collect() };
            let mut pieces = 1usize;
            while pieces < 256 {
                let strays = (0..pieces).any(|i| {
                    let (f0, f1) = (i as f64 / pieces as f64, (i + 1) as f64 / pieces as f64);
                    let (a, b) = (pose(&lerp(f0)), pose(&lerp(f1)));
                    let mid = pose(&lerp((f0 + f1) / 2.0));
                    let screw = Screw::between(&a, &b).at(0.5);
                    let top = V3::new(0.0, 0.0, o.tool.cutting_length);
                    [V3::zeros(), top].iter().any(|p| {
                        let m = screw.linear * a.apply(*p) + screw.translation;
                        (m - mid.apply(*p)).norm() > STRAY
                    })
                });
                if !strays {
                    break;
                }
                pieces *= 2;
            }
            for i in 0..pieces {
                let (f0, f1) = (i as f64 / pieces as f64, (i + 1) as f64 / pieces as f64);
                let a = held.unwrap_or_else(|| pose(&lerp(f0)));
                let b = pose(&lerp(f1));
                let fitted = Screw::between(&a, &b);
                // The machine's own screw where the move is one (an arc's chord: the arc),
                // else the one fitted.
                let mid = pose(&lerp((f0 + f1) / 2.0));
                let arc = s
                    .about
                    .filter(|_| pieces == 1)
                    .and_then(|c| arc_screw(&a, &b, c));
                let exact = arc.or_else(|| {
                    machine_screw(&a, &b, &mid, &fitted, &lines, o.tool.cutting_length)
                });
                let machine = exact.is_some();
                let screw = exact.unwrap_or(fitted);
                if screw.amount() < 1e-9 {
                    continue;
                }
                let (p0, p1) = (here as f64 + f0, here as f64 + f1);
                // On with the cut before when one screw still does both.
                if let Some(c) = cuts.last_mut()
                    && c.op == k
                    && (c.to() - p0).abs() < 1e-9
                    && c.extend(b, &screw, p1, machine)
                {
                    held = Some(c.screw.moved(&c.start));
                    if arc.is_some() {
                        held = held.map(|h| unspun(h, &b));
                    }
                    continue;
                }
                held = Some(screw.moved(&a));
                if arc.is_some() {
                    held = held.map(|h| unspun(h, &b));
                }
                cuts.push(Cut {
                    op: k,
                    start: a,
                    screw,
                    knots: vec![(p0, 0.0), (p1, screw.amount())],
                    poses: vec![(b, screw.amount())],
                    top: o.tool.cutting_length,
                    machine,
                });
            }
        }
    }
    cuts
}

/// Whether a tool's cutting part never narrows from its tip up (an end, ball, bull-nose or
/// taper mill, a drill): slid along its own axis it then covers no more than it does where the
/// slide is deepest, within the length it reaches.
fn widens_upward(tool: &Tool) -> bool {
    let c = Cutter::of(tool);
    !c.undercut
        && c.pieces.iter().all(|p| {
            let (s, e) = (p.start(), p.end());
            e[0] >= s[0] - 1e-12 && e[1] >= s[1] - 1e-12
        })
}

/// Whether the point (setup coordinates) is clear of the uncut stock by `margin`.
fn clear_of_stock(shape: &StockShape, setup: &Frame, p: V3, margin: f64) -> bool {
    let w = setup.to_world([p.x, p.y, p.z]);
    match shape {
        StockShape::Box { frame, b } => {
            let q = frame.to_local(w);
            (0..3).any(|i| q[i] < b.min[i] - margin || q[i] > b.max[i] + margin)
        }
        StockShape::Cylinder {
            frame,
            center,
            radius,
            z,
        } => {
            let q = frame.to_local(w);
            (q[0] - center[0]).hypot(q[1] - center[1]) > radius + margin
                || q[2] < z[0] - margin
                || q[2] > z[1] + margin
        }
        StockShape::Mesh(_) => false,
    }
}

/// Leaves out the slides along the tool's own axis that cut nothing the cut next to them
/// doesn't: a plunge to where the next cut starts, a rise from where the last one ended, with a
/// tool that never narrows from its tip up ([`widens_upward`]) and reaches clear of the stock
/// there. What such a slide sweeps within the stock is the tool where it is deepest, which that
/// cut takes away anyway; cut on its own it meets the walls that cut left tangentially, along
/// the contacts of the very tool, the hardest case a boolean has.
fn drop_axial_slides(cuts: &mut Vec<Cut>, run: &SetupRun, shape: &StockShape) {
    let n = cuts.len();
    let same =
        |a: &Pose, b: &Pose| (a.tip - b.tip).norm() <= 1e-9 && (a.rot - b.rot).norm() <= 1e-12;
    let mut keep = vec![true; n];
    for i in 0..n {
        let c = &cuts[i];
        let Screw::Slide(v) = c.screw else { continue };
        let z = c.start.rot.column(2).into_owned();
        if v.norm() < 1e-12 || v.cross(&z).norm() > 1e-9 * v.norm() {
            continue;
        }
        let Some(o) = run.ops.get(c.op) else { continue };
        if !widens_upward(o.tool) {
            continue;
        }
        let end = c.screw.moved(&c.start);
        // The deepest pose, and the cut beside the slide that holds the tool there.
        let (deep, beside) = if v.dot(&z) < 0.0 {
            (
                end,
                cuts.get(i + 1)
                    .filter(|d| d.op == c.op && same(&d.start, &end)),
            )
        } else {
            (
                c.start,
                i.checked_sub(1)
                    .and_then(|j| cuts.get(j))
                    .filter(|d| d.op == c.op && same(&d.screw.moved(&d.start), &c.start)),
            )
        };
        let top = deep.apply(V3::new(0.0, 0.0, o.tool.cutting_length));
        let clear = clear_of_stock(shape, &run.frame, top, Cutter::of(o.tool).radius);
        if std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "6") {
            eprintln!(
                "axial slide {i}: plunge {} beside {} clear {clear} top {top:?}",
                v.dot(&z) < 0.0,
                beside.is_some()
            );
        }
        if beside.is_some() && clear {
            keep[i] = false;
        }
    }
    let mut k = 0;
    cuts.retain(|_| {
        let kept = keep[k];
        k += 1;
        kept
    });
}

/// The cutting part of a tool as a solid: its profile's pieces revolved about its axis, its
/// tip at the origin and its axis along +Z, up to the top of its flutes.
fn tool_solid(k: &mut DefaultKernel, tool: &Tool) -> Result<BodyId, String> {
    let cutter = Cutter::of(tool);
    let mut curves: Vec<Curve2> = Vec::new();
    let mut at = [0.0, 0.0];
    let far = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) > 1e-9;
    for p in &cutter.pieces {
        let (s, e) = (p.start(), p.end());
        if far(s, at) {
            curves.push(Curve2::line(at, s));
        }
        match *p {
            Piece::Cone { .. } => {
                if far(s, e) {
                    curves.push(Curve2::line(s, e));
                }
            }
            Piece::Arc {
                rc,
                hc,
                rho,
                a0,
                a1,
                ..
            } => curves.push(Curve2::arc([rc, hc], rho, a0, a1 - a0)),
        }
        at = e;
    }
    let top = cutter.length.max(at[1]);
    if (at[1] - top).abs() > 1e-9 {
        curves.push(Curve2::line(at, [at[0], top]));
        at = [at[0], top];
    }
    curves.push(Curve2::line(at, [0.0, top]));
    curves.push(Curve2::line([0.0, top], [0.0, 0.0]));
    // (The Front plane: sketch x the model's x, sketch y its z.)
    let half = Profile::of(Plane::xz(0.0), vec![Loop::new(curves)]);
    let spec = RevolveSpec::solid(Axis::new(Point3::origin(), V3::z()), None);
    k.revolve_with(&half, &spec)
        .and_then(|r| r.first())
        .map_err(|e| format!("the tool's solid: {e}"))
}

/// The stock as a solid in the setup's coordinates (`frame`, in the model's): a box or a
/// bar. `None`: a stock of another shape (a body's mesh), which has no exact solid here.
fn stock_solid(k: &mut DefaultKernel, shape: &StockShape, frame: &Frame) -> Option<BodyId> {
    let (body, in_frame) = match shape {
        StockShape::Box { frame: f, b } => (
            k.primitive(&Primitive::block(
                Point3::from(V3::from(b.min)),
                Point3::from(V3::from(b.max)),
            ))
            .ok()?
            .first()
            .ok()?,
            *f,
        ),
        StockShape::Cylinder {
            frame: f,
            center,
            radius,
            z,
        } => (
            k.primitive(&Primitive::cylinder(
                Point3::new(center[0], center[1], z[0]),
                V3::z(),
                *radius,
                z[1] - z[0],
            ))
            .ok()?
            .first()
            .ok()?,
            *f,
        ),
        StockShape::Mesh(_) => return None,
    };
    // From the stock's own frame to the setup's.
    let f = frame.relative(&in_frame);
    let v = |p: P3| V3::new(p[0], p[1], p[2]);
    let m = Motion {
        linear: Matrix3::from_columns(&[v(f.x), v(f.y), v(f.z)]),
        translation: v(f.origin),
    };
    k.transform_motion(body, &m).ok()?.first().ok()
}

/// A rigid motion as text, a step a line (`turn ox oy oz dx dy dz from to`, `slide x0 y0 z0
/// x1 y1 z1`): the motion of a cut dumped for the kernel's tests to replay.
fn motion_text(m: &SolidMotion) -> String {
    let SolidMotion::Rigid { steps } = m else {
        return format!("# {m:?}\n");
    };
    let mut out = String::new();
    for s in steps {
        match s {
            cadrs_kernel::RigidStep::Turn { axis, from, to } => {
                let (o, d) = (axis.origin, axis.dir);
                out += &format!(
                    "turn {:e} {:e} {:e} {:e} {:e} {:e} {from:e} {to:e}\n",
                    o.x, o.y, o.z, d.x, d.y, d.z
                );
            }
            cadrs_kernel::RigidStep::Slide { from, to } => {
                out += &format!(
                    "slide {:e} {:e} {:e} {:e} {:e} {:e}\n",
                    from.x, from.y, from.z, to.x, to.y, to.z
                );
            }
        }
    }
    out
}

/// A solid stock cut exactly as a run plays (see the module docs).
pub struct ExactStock {
    k: DefaultKernel,
    /// The uncut stock, each operation's tool (at the origin, upright).
    start: BodyId,
    tools: Vec<BodyId>,
    /// Each operation's tool's profile, for [`Tween`]s.
    cutters: Vec<Cutter>,
    cuts: Vec<Cut>,
    /// The stock after the first `done` cuts.
    kept: BodyId,
    done: usize,
    /// What is shown: `kept` with the cut the tool is on made as far as `at` says.
    shown: BodyId,
    at: (usize, f64),
    /// Which operation (from 1) made each face of each stock body made (by its place in the
    /// body's faces; 0: the stock's own).
    tags: HashMap<BodyId, Vec<u8>>,
    /// Cuts the kernel could not make (left out), with why.
    pub failed: Vec<(usize, String)>,
    /// A cut the kernel couldn't make whole, put off to be joined with the next
    /// ([`Self::joined`]).
    deferred: Option<usize>,
    /// How far along the cut the tool is on (`done`) the stock shown is cut; 0 where it is the
    /// stock kept.
    shown_along: f64,
    /// A cut whose part-way slice failed: not sliced again.
    unsliced: Option<usize>,
    /// A cut made whole ahead of the tool for a tween: its index, the kept stock it was cut
    /// from, and the stock after it.
    ahead: Option<(usize, BodyId, BodyId)>,
}

impl ExactStock {
    /// The run's stock to be cut exactly: `motions` each operation's moves (as
    /// [`super::program::run_motions`] gives them), `rows` the machine's axes through them
    /// ([`super::program::axis_track`]). `Err`: why it can't be (a stock with no exact solid,
    /// a tool the kernel cannot make).
    pub fn new(
        machine: &Machine,
        run: &SetupRun,
        shape: &StockShape,
        motions: &[Vec<Segment>],
        rows: &[Vec<f64>],
    ) -> Result<ExactStock, String> {
        let mut k = DefaultKernel::new();
        let clock = web_time::Instant::now();
        let lap = |what: &str| {
            if std::env::var_os("CADRS_EXACT_DEBUG").is_some() {
                eprintln!(
                    "exact stock: {what} by {:.2} s",
                    clock.elapsed().as_secs_f64()
                );
            }
        };
        let start = stock_solid(&mut k, shape, &run.frame)
            .ok_or("the stock has no exact solid (only boxes and bars do)")?;
        lap("the stock made");
        let tools = run
            .ops
            .iter()
            .map(|o| tool_solid(&mut k, o.tool))
            .collect::<Result<Vec<_>, _>>()?;
        let cutters = run.ops.iter().map(|o| Cutter::of(o.tool)).collect();
        lap("the tools made");
        let mut cuts = cuts_of(machine, run, motions, rows);
        lap("the cuts found");
        drop_axial_slides(&mut cuts, run, shape);
        lap("axial slides dropped");
        if std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "7") {
            let mut kinds: std::collections::BTreeMap<String, (usize, f64)> = Default::default();
            for c in &cuts {
                let k = match c.screw {
                    Screw::Slide(v) => {
                        let z = c.start.rot.column(2).into_owned();
                        format!(
                            "slide {:.0}° off the axis",
                            v.normalize().dot(&z).abs().acos().to_degrees()
                        )
                    }
                    Screw::Turn { .. } => format!("turn machine={}", c.machine),
                };
                let e = kinds.entry(k).or_default();
                e.0 += 1;
                e.1 += c.screw.amount();
            }
            for (k, (n, amount)) in kinds {
                eprintln!("exact cuts: {n} {k}, amount {amount:.1}");
            }
        }
        if std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "4") {
            for (i, c) in cuts.iter().enumerate().filter(|(_, c)| c.poses.len() > 100) {
                eprintln!("exact cut {i}: {} moves, {:?}", c.poses.len(), c.screw);
            }
        }
        Ok(ExactStock {
            k,
            start,
            tools,
            cutters,
            cuts,
            kept: start,
            done: 0,
            shown: start,
            at: (0, 0.0),
            tags: HashMap::new(),
            failed: Vec::new(),
            deferred: None,
            shown_along: 0.0,
            unsliced: None,
            ahead: None,
        })
    }

    /// How many moves each cut takes.
    pub fn cut_sizes(&self) -> Vec<usize> {
        self.cuts.iter().map(|c| c.poses.len()).collect()
    }

    /// How many cuts the run is made in (its moves on one motion each one).
    /// How many cuts are made so far.
    /// The cut the tool is on at the run's place `n + f`, its index and how far along it that is
    /// (0 to 1); `None` past the last cut or between cuts.
    pub fn cut_at(&self, n: usize, f: f64) -> Option<(usize, f64)> {
        let place = n as f64 + f.clamp(0.0, 1.0);
        let c = self.cuts.partition_point(|c| c.to() <= place);
        let cut = self.cuts.get(c)?;
        (cut.from() <= place).then(|| (c, cut.along(place)))
    }

    /// For a tween at the run's place `n + f`: the stock cut through every cut before the one
    /// the tool is on (kept), that cut made whole after it (made once, ahead of the tool), and
    /// the two meshed with each vertex's time ([`Tween::reached`]) along it. `None` where the
    /// tool is on no cut (the kept stock is then the whole stock to there: [`Self::sim_mesh`]).
    pub fn tweened(&mut self, n: usize, f: f64, deflection: f64) -> Option<Tweened> {
        let (c, _) = self.cut_at(n, f)?;
        self.tweened_cut(c, deflection)
    }

    /// Each cut's stretch of the run: the places (`n + f`) it starts and ends at.
    pub fn cut_spans(&self) -> Vec<(f64, f64)> {
        self.cuts.iter().map(|c| (c.from(), c.to())).collect()
    }

    /// The last cut ended by the run's place `n + f` (the stock there is the stock after it,
    /// where the tool is between cuts).
    pub fn cut_before(&self, n: usize, f: f64) -> Option<usize> {
        let place = n as f64 + f.clamp(0.0, 1.0);
        self.cuts
            .partition_point(|c| c.to() <= place)
            .checked_sub(1)
    }

    /// Whether cut `c` is one the kernel couldn't make exactly (cut as best it could: joined
    /// with the next, in halves).
    pub fn cut_failed(&self, c: usize) -> bool {
        self.failed.iter().any(|(i, _)| *i == c)
    }

    /// [`Self::tweened`] for cut `c`: the stock before it and after it, each point timed.
    pub fn tweened_cut(&mut self, c: usize, deflection: f64) -> Option<Tweened> {
        // The kept stock through the cuts before (the part-way slices dropped).
        let before = self.cuts.get(c)?.from();
        let (bn, bf) = (before.floor() as usize, before - before.floor());
        self.cut_to(bn, bf);
        if self.done != c {
            if std::env::var_os("CADRS_EXACT_DEBUG").is_some() {
                eprintln!("tween: cut {c} not reached (done {})", self.done);
            }
            return None;
        }
        let kept = self.kept;
        let after = match &self.ahead {
            Some((k, a, b)) if *k == c && *a == kept => *b,
            _ => {
                if let Some((_, _, b)) = self.ahead.take() {
                    self.drop_body(b);
                }
                // (Not made whole: in halves, as the stock kept would be.)
                let b = match self.cut_once(kept, c, 0.0, 1.0) {
                    Ok(b) => b,
                    Err(_) => self.halves(kept, c),
                };
                self.ahead = Some((c, kept, b));
                b
            }
        };
        let tween = self.tween(c)?;
        let reach = self.tween_reach(c);
        let (mut before, before_edges) = self.sim_mesh_of(kept, deflection)?;
        let (mut after, after_edges) = self.sim_mesh_of(after, deflection)?;
        let time = |p: &[f32; 3], grow: f64, never: f32| -> f32 {
            let q = [p[0] as f64, p[1] as f64, p[2] as f64];
            if !reach.contains(&Point3::new(q[0], q[1], q[2])) {
                return never;
            }
            tween.reached(q, grow).map_or(never, |t| t as f32)
        };
        // (Before: hidden once reached. After: its new faces shown once made, its old ones always.)
        // (Triangles within the cut's reach split till a fraction of the tool across first: a
        // face whose corners the tool never reaches, a block's top, is still crossed by it.)
        let coarse = (0.5 * self.cutters[self.cuts[c].op].radius, reach);
        let before_times = refine_by_times(&mut before, coarse, &|p| time(p, 0.0, f32::INFINITY));
        // (The stock after the cut's new faces lie on the tool's sweep, their tessellation's
        // points within its deflection of it.)
        let on_sweep = ON_SWEEP.max(deflection);
        let after_times = refine_by_times(&mut after, coarse, &|p| {
            time(p, on_sweep, f32::NEG_INFINITY)
        });
        let (before_edges, before_edge_times) =
            edges_by_times(before_edges, &|p| time(p, 0.0, f32::INFINITY));
        let (after_edges, after_edge_times) =
            edges_by_times(after_edges, &|p| time(p, on_sweep, f32::NEG_INFINITY));
        let cut = &self.cuts[c];
        Some(Tweened {
            cut: c,
            knots: cut.knots.clone(),
            before,
            before_times,
            after,
            after_times,
            before_edges,
            before_edge_times,
            after_edges,
            after_edge_times,
        })
    }

    /// Where cut `c` can reach: the box of its tool at its start, its end and between.
    fn tween_reach(&self, c: usize) -> Bbox3 {
        let cut = &self.cuts[c];
        let cutter = &self.cutters[cut.op];
        let r = cutter.radius.hypot(cutter.length) + 1.0;
        let mut b = Bbox3::empty();
        for i in 0..=64 {
            let m = cut.screw.at(i as f64 / 64.0);
            let mid =
                m.linear * cut.start.apply(V3::new(0.0, 0.0, cutter.length / 2.0)) + m.translation;
            b.add(&Point3::from(mid));
        }
        b.grow(r)
    }

    /// The motion of cut `c`, to tween the stock along it ([`Tween`]).
    pub fn tween(&self, c: usize) -> Option<Tween> {
        let cut = self.cuts.get(c)?;
        Some(Tween {
            cutter: self.cutters[cut.op].clone(),
            start: cut.start,
            screw: cut.screw,
        })
    }

    pub fn cut_count_done(&self) -> usize {
        self.done
    }

    /// The share of the cuts made so far that the kernel couldn't make (in whole or part).
    pub fn failed_share(&self) -> f64 {
        let mut cuts: Vec<usize> = self.failed.iter().map(|f| f.0).collect();
        cuts.sort_unstable();
        cuts.dedup();
        cuts.len() as f64 / self.done.max(1) as f64
    }

    pub fn cut_count(&self) -> usize {
        self.cuts.len()
    }

    /// How far the stock is cut: the run's move, and how far along it.
    pub fn at(&self) -> (usize, f64) {
        self.at
    }

    /// `body` less cut `c` from `f0` to `f1` along it, the faces it makes tagged with its
    /// operation; in halves (down to a 32nd of it) where the kernel can't make it exactly as
    /// one. What it couldn't cut at all is left (and listed in [`ExactStock::failed`]).
    /// The cuts `a` and `a + 1` as one slide, from `a`'s start to `a + 1`'s end, where both
    /// are slides of one tool turned alike and none of their knots is further off it than
    /// [`JOINED`]: a cut the kernel can't make (the 55 µm last stretch of a ramp, its start and
    /// end cylinders all but one) done with the one after, which goes on the same way.
    fn joined(&self, a: usize) -> Option<Moving> {
        let (c0, c1) = (self.cuts.get(a)?, self.cuts.get(a + 1)?);
        let (Screw::Slide(_), Screw::Slide(_)) = (c0.screw, c1.screw) else {
            return None;
        };
        let same = |r: &Matrix3<f64>| (r - c0.start.rot).abs().max() < 1e-12;
        if c0.op != c1.op || !same(&c1.start.rot) || (c0.to() - c1.from()).abs() > 1e-9 {
            return None;
        }
        let end = c1.screw.moved(&c1.start);
        let span = end.tip - c0.start.tip;
        let len = span.norm();
        if len < 1e-12 {
            return None;
        }
        let off = |p: &V3| {
            let w = p - c0.start.tip;
            (w - span * (w.dot(&span) / (len * len))).norm()
        };
        let knots = c0.poses.iter().chain(&c1.poses).map(|(p, _)| p.tip);
        (knots.chain([c1.start.tip]).all(|p| off(&p) <= JOINED)
            && std::env::var_os("CADRS_EXACT_NO_JOIN").is_none())
        .then_some(Moving {
            op: c0.op,
            start: c0.start,
            screw: Screw::Slide(span),
        })
    }

    /// The cut `c` in halves (it failed whole), a half that fails counted.
    fn halves(&mut self, body: BodyId, c: usize) -> BodyId {
        let half = self.cut_in(body, c, 0.0, 0.5, 2);
        let whole = self.cut_in(half, c, 0.5, 1.0, 2);
        if half != body && half != whole {
            self.drop_body(half);
        }
        whole
    }

    fn cut_in(&mut self, body: BodyId, c: usize, f0: f64, f1: f64, depth: u32) -> BodyId {
        match self.cut_once(body, c, f0, f1) {
            Ok(b) => b,
            Err(e) if depth >= 2 => {
                self.failed.push((c, e));
                body
            }
            Err(_) => {
                let mid = (f0 + f1) / 2.0;
                let half = self.cut_in(body, c, f0, mid, depth + 1);
                let whole = self.cut_in(half, c, mid, f1, depth + 1);
                if half != body && half != whole {
                    self.drop_body(half);
                }
                whole
            }
        }
    }

    /// [`Self::cut`] in one cut by the kernel, or why it can't be.
    fn cut_once(&mut self, body: BodyId, c: usize, f0: f64, f1: f64) -> Result<BodyId, String> {
        let cut = &self.cuts[c];
        let motion = Moving {
            op: cut.op,
            start: cut.start,
            screw: cut.screw,
        };
        self.cut_moving(body, c, motion, f0, f1)
    }

    /// [`Self::cut_once`] by the tool of the operation `op` from `start` by `screw` (the cut
    /// `c`'s, or two cuts joined: [`Self::joined`]).
    fn cut_moving(
        &mut self,
        body: BodyId,
        c: usize,
        cut: Moving,
        f0: f64,
        f1: f64,
    ) -> Result<BodyId, String> {
        let tool = self.tools[cut.op];
        // The tool where the stretch starts, and its motion from there.
        let at = cut.screw.at(f0);
        let start = Pose {
            rot: at.linear * cut.start.rot,
            tip: at.linear * cut.start.tip + at.translation,
        };
        let motion = cut.screw.solid_motion(f1 - f0);
        let tag = (cut.op + 1).min(254) as u8;
        let placed = self
            .k
            .transform_motion(tool, &start.motion())
            .and_then(|r| r.first())
            .map_err(|e| e.to_string())?;
        let started = web_time::Instant::now();
        let result = self.k.cut_swept_exact(body, placed, &motion);
        // (`CADRS_EXACT_DUMP=<dir>`: a failing cut's target and placed tool, as the kernel
        // writes bodies, and its motion as text (`turn ox oy oz dx dy dz from to` and
        // `slide x0 y0 z0 x1 y1 z1`, a step a line), replayed by the kernel's tests.)
        if let Err(e) = &result
            && let Some(dir) = std::env::var_os("CADRS_EXACT_DUMP")
        {
            let dir = std::path::PathBuf::from(dir);
            let name = format!("cut{c}-{f0:.4}-{f1:.4}");
            if let (Ok(t), Ok(p)) = (self.k.write_body(body), self.k.write_body(placed)) {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join(format!("{name}.target")), t);
                let _ = std::fs::write(dir.join(format!("{name}.tool")), p);
                let _ = std::fs::write(
                    dir.join(format!("{name}.motion")),
                    format!("{}# {e}\n", motion_text(&motion)),
                );
            }
        }
        self.k.release(placed);
        if std::env::var_os("CADRS_EXACT_DEBUG").is_some() {
            let faces = result
                .as_ref()
                .ok()
                .and_then(|r| r.first().ok())
                .and_then(|b| self.k.faces(b).ok())
                .map_or(0, |f| f.len());
            if std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "2") {
                let t = &self.tools[cut.op];
                eprintln!(
                    "exact cut {c}: start {start:?} screw {:?} tool bbox {:?} target bbox {:?}",
                    cut.screw,
                    self.k.bounding_box(*t),
                    self.k.bounding_box(body)
                );
            }
            eprintln!(
                "exact cut {c} ({f0:.3} to {f1:.3}): {:.3} s, {faces} faces{}",
                started.elapsed().as_secs_f64(),
                match &result {
                    Ok(_) => String::new(),
                    Err(e) => format!(", failed: {e}"),
                }
            );
        }
        let made = result.map_err(|e| e.to_string())?;
        let new = made.first().map_err(|e| e.to_string())?;
        // Each face keeps the operation of the stock's face it continues; the rest the tool
        // made.
        let mut tags = vec![tag; self.k.faces(new).map_or(0, |f| f.len())];
        let old = self.tags.get(&body);
        for (face, from) in &made.history.modified {
            if from.body == body
                && let Some(t) = tags.get_mut(face.0 as usize)
            {
                *t = old
                    .and_then(|o| o.get(from.face.0 as usize))
                    .copied()
                    .unwrap_or(0);
            }
        }
        self.tags.insert(new, tags);
        Ok(self.without_slivers(new))
    }

    /// `body` with its faces no wider than the merge tolerance collapsed
    /// ([`cadrs_kernel::HealOp::Slivers`]): strips that cuts a hair apart leave (a cut merged
    /// within [`MERGE`] beside one on the machine's exact screw), which every later cut would
    /// have to split, slowly and badly. Faces that wide are the merging's, not the toolpath's.
    /// `body` as it is when the heal doesn't leave it sound.
    fn without_slivers(&mut self, body: BodyId) -> BodyId {
        // (`CADRS_EXACT_DEBUG=8`: degenerate edges of length, by the cut or by the heal.)
        let long_points = |k: &DefaultKernel, b: BodyId| -> Vec<String> {
            k.get(b).map_or(Vec::new(), |x| {
                x.edges()
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        e.degenerate
                            && (e.curve.point(e.t[0]) - e.curve.point(e.t[1])).norm() > 1e-6
                    })
                    .map(|(i, e)| format!("{i} at {:?}", e.curve.point(e.t[0])))
                    .collect()
            })
        };
        let debug = std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "8");
        if debug {
            let l = long_points(&self.k, body);
            if !l.is_empty() {
                eprintln!(
                    "exact cut {}: the cut left degenerate edges of length {l:?}",
                    self.done
                );
            }
        }
        if std::env::var_os("CADRS_EXACT_NO_HEAL").is_some() {
            return body;
        }
        // (By the trait: with the kernel's test kit an inherent `heal` of its own shadows it.)
        let Ok(r) = cadrs_kernel::Kernel::heal(
            &mut self.k,
            body,
            &[cadrs_kernel::HealOp::Slivers { width: merge() }],
        ) else {
            return body;
        };
        let Ok(healed) = r.first() else { return body };
        let faces = |k: &DefaultKernel, b| k.faces(b).map_or(0, |f| f.len());
        if faces(&self.k, healed) == faces(&self.k, body)
            || !self.k.get(healed).is_ok_and(|b| b.check().is_empty())
        {
            self.k.release(healed);
            return body;
        }
        let old = self.tags.get(&body).cloned().unwrap_or_default();
        let mut tags = vec![0u8; faces(&self.k, healed)];
        for (face, from) in &r.history.modified {
            if from.body == body
                && let Some(t) = tags.get_mut(face.0 as usize)
            {
                *t = old.get(from.face.0 as usize).copied().unwrap_or(0);
            }
        }
        self.tags.insert(healed, tags);
        if debug {
            let l = long_points(&self.k, healed);
            if !l.is_empty() {
                eprintln!(
                    "exact cut {}: the heal left degenerate edges of length {l:?}",
                    self.done
                );
            }
        }
        self.drop_body(body);
        healed
    }

    /// The stock kept from now `b`, the one kept before let go.
    fn keep(&mut self, b: BodyId) {
        if b != self.kept {
            self.drop_body(self.kept);
        }
        self.kept = b;
    }

    /// Lets go of a stock body no longer wanted (never the uncut stock).
    fn drop_body(&mut self, b: BodyId) {
        if b != self.start {
            self.k.release(b);
            self.tags.remove(&b);
        }
    }

    /// The stock cut up to `f` of the way along the run's move `n` (see the module docs).
    pub fn cut_to(&mut self, n: usize, f: f64) {
        let to = (n, f.clamp(0.0, 1.0));
        if to.0 < self.at.0 || (to.0 == self.at.0 && to.1 < self.at.1) {
            // Back: from the uncut stock again.
            if self.shown != self.kept {
                self.drop_body(self.shown);
            }
            self.drop_body(self.kept);
            (self.kept, self.done, self.shown) = (self.start, 0, self.start);
            self.deferred = None;
            self.shown_along = 0.0;
            self.unsliced = None;
            if let Some((_, _, b)) = self.ahead.take() {
                self.drop_body(b);
            }
        }
        let place = to.0 as f64 + to.1;
        // (The stock shown part-way along the cut the tool was on, cut on from where it was:
        // each place asked for costs the stretch since the last, where cut afresh from the
        // stock kept a 45 rad screw's slices grew to seconds each, the stock far behind.)
        let incremental = std::env::var_os("CADRS_EXACT_AFRESH").is_none();
        if self.shown != self.kept
            && !(incremental
                && self.shown_along > 0.0
                && self.cuts.get(self.done).is_some_and(|c| c.from() < place))
        {
            self.drop_body(self.shown);
            (self.shown, self.shown_along) = (self.kept, 0.0);
        }
        // The cuts the tool is through. (One the kernel can't make whole is tried again joined
        // with the next, as one slide, once the tool is through that too: [`Self::joined`].)
        while self.done < self.cuts.len() && self.cuts[self.done].to() <= place + 1e-12 {
            let kept = self.kept;
            let c = self.done;
            // Made whole ahead of the tool for a tween: that.
            if let Some((k, from, b)) = self.ahead
                && k == c
                && from == kept
            {
                self.ahead = None;
                if self.shown != kept {
                    self.drop_body(self.shown);
                }
                (self.shown, self.shown_along) = (kept, 0.0);
                self.keep(b);
                self.done += 1;
                continue;
            }
            // Shown part-way along: finished on from there.
            if self.shown != kept && self.shown_along > 0.0 {
                let (part, along) = (self.shown, self.shown_along);
                (self.shown, self.shown_along) = (kept, 0.0);
                if self.deferred.is_none()
                    && let Ok(n) = self.cut_once(part, c, along, 1.0)
                {
                    if n != part {
                        self.drop_body(part);
                    }
                    self.keep(n);
                    self.done += 1;
                    continue;
                }
                self.drop_body(part);
            }
            // A cut put off (whole, it failed): joined with this one, else in halves and this
            // one as ever.
            let mut b = kept;
            if let Some(p) = self.deferred.take() {
                if let Some(m) = self.joined(p)
                    && let Ok(j) = self.cut_moving(kept, c, m, 0.0, 1.0)
                {
                    self.keep(j);
                    self.done += 1;
                    continue;
                }
                b = self.halves(kept, p);
            }
            let next = match self.cut_once(b, c, 0.0, 1.0) {
                Ok(n) => n,
                // (Whole, put off for the next where it can be joined with it: its halves,
                // each a sliver of a sweep the whole doesn't meet, the stock cut in pieces
                // the next cuts fail on.)
                Err(_) if self.joined(c).is_some() => {
                    self.deferred = Some(c);
                    b
                }
                Err(_) => self.halves(b, c),
            };
            if b != kept && b != next {
                self.drop_body(b);
            }
            self.keep(next);
            self.done += 1;
        }
        if self.shown_along == 0.0 {
            self.shown = self.kept;
        }
        // The cut the tool is on, as far as it has gone: on from where it was shown, else from
        // the kept stock. (Shown only, in one try, the stock as it was where it fails: a
        // fraction of a move is a sliver of a sweep, at angles the whole one doesn't meet, and
        // halving it took ten minutes to show what the next whole cut shows anyway.)
        if let Some(c) = self.cuts.get(self.done)
            && c.from() < place
        {
            let along = c.along(place);
            let from = self.shown_along;
            // (Not again for a cut once a slice of it failed: a seven-turn screw's slices
            // failed at 20 s each, the worker stuck on them; the stock as kept is shown till
            // the tool is through it, the whole cut then made.)
            if along > from + 1e-9 && self.deferred.is_none() && self.unsliced != Some(self.done) {
                let base = self.shown;
                match self.cut_once(base, self.done, from, along) {
                    Ok(b) => {
                        if base != self.kept && base != b {
                            self.drop_body(base);
                        }
                        (self.shown, self.shown_along) = (b, along);
                    }
                    Err(_) => self.unsliced = Some(self.done),
                }
            }
        }
        self.at = to;
    }

    /// The stock as shown, tessellated (`deflection` mm), with each triangle's operation (from
    /// 1; 0 the stock's own surface).
    pub fn mesh(&self, deflection: f64) -> Option<(TriMesh, Vec<u8>)> {
        self.mesh_of(self.shown, deflection)
    }

    /// [`Self::mesh`] of a stock body.
    fn mesh_of(&self, body: BodyId, deflection: f64) -> Option<(TriMesh, Vec<u8>)> {
        let m = self
            .k
            .tessellate(
                body,
                Tessellation {
                    deflection,
                    angle: 5f64.to_radians(),
                },
            )
            .ok()?;
        let tags = m
            .triangle_faces
            .iter()
            .map(|f| {
                self.tags
                    .get(&body)
                    .and_then(|t| t.get(f.0 as usize))
                    .copied()
                    .unwrap_or(0)
            })
            .collect();
        Some((m, tags))
    }

    /// The stock as shown as the simulation's mesh ([`cadrs_cam::dexel::SimMesh`]): each
    /// vertex coloured by the operation that made its face.
    pub fn sim_mesh(&self, deflection: f64) -> Option<cadrs_cam::dexel::SimMesh> {
        self.sim_mesh_of(self.shown, deflection).map(|(m, _)| m)
    }

    /// [`Self::sim_mesh`] and the stock's edges, each as a line through its points.
    pub fn sim_mesh_and_edges(
        &self,
        deflection: f64,
    ) -> Option<(cadrs_cam::dexel::SimMesh, Vec<Vec<[f32; 3]>>)> {
        self.sim_mesh_of(self.shown, deflection)
    }

    /// [`Self::sim_mesh_and_edges`] of a stock body.
    fn sim_mesh_of(
        &self,
        body: BodyId,
        deflection: f64,
    ) -> Option<(cadrs_cam::dexel::SimMesh, Vec<Vec<[f32; 3]>>)> {
        let (m, tags) = self.mesh_of(body, deflection)?;
        let mut vertex_tags = vec![0u8; m.positions.len()];
        for (t, tag) in m.indices.iter().zip(&tags) {
            for i in t {
                vertex_tags[*i as usize] = *tag;
            }
        }
        let f = |v: &V3| [v.x as f32, v.y as f32, v.z as f32];
        let edges = m
            .edges
            .iter()
            .filter(|(_, pts)| pts.len() >= 2)
            .map(|(_, pts)| pts.iter().map(|p| f(&p.coords)).collect())
            .collect();
        Some((
            cadrs_cam::dexel::SimMesh {
                positions: m.positions.iter().map(|p| f(&p.coords)).collect(),
                normals: m.normals.iter().map(f).collect(),
                indices: m.indices.iter().flatten().copied().collect(),
                colors: vertex_tags
                    .iter()
                    .map(|t| cadrs_cam::dexel::tag_color(*t))
                    .collect(),
                tags: vertex_tags,
            },
            edges,
        ))
    }

    /// The stock's volume as shown (mm³).
    pub fn volume(&self) -> f64 {
        self.k
            .mass_properties(self.shown)
            .map(|m| m.volume)
            .unwrap_or(0.0)
    }

    /// The stock's surface area as shown (mm²).
    pub fn area(&self) -> f64 {
        self.k
            .mass_properties(self.shown)
            .map(|m| m.surface_area)
            .unwrap_or(0.0)
    }
}

/// The stock either side of the cut the tool is on, for a tween ([`ExactStock::tweened`]).
#[derive(Debug, Clone)]
pub struct Tweened {
    /// The cut.
    pub cut: usize,
    /// The run's places within it, and how much of its motion is done there: what the tool's
    /// place is along it ([`Tweened::along`]).
    pub knots: Vec<(f64, f64)>,
    /// The stock before it, each vertex's time along it (when the tool reaches it; infinite
    /// where never: shown till then).
    pub before: cadrs_cam::dexel::SimMesh,
    pub before_times: Vec<f32>,
    /// The stock after it, each vertex's time (when the cut makes its face there; minus
    /// infinite on the faces it leaves as they were: shown always).
    pub after: cadrs_cam::dexel::SimMesh,
    pub after_times: Vec<f32>,
    /// The two stocks' edges, each a line through its points, and each point's time as their
    /// vertices' are (split where the times differ, so a line is cut where the tool is).
    pub before_edges: Vec<Vec<[f32; 3]>>,
    pub before_edge_times: Vec<Vec<f32>>,
    pub after_edges: Vec<Vec<[f32; 3]>>,
    pub after_edge_times: Vec<Vec<f32>>,
}

impl Tweened {
    /// How far along the cut (0 to 1) the run's place `n + f` is.
    pub fn along(&self, n: usize, f: f64) -> f64 {
        along_knots(&self.knots, n as f64 + f.clamp(0.0, 1.0))
    }
}

/// How far along a cut with `knots` (places and amounts done) the place `x` is, 0 to 1.
fn along_knots(knots: &[(f64, f64)], x: f64) -> f64 {
    let total = knots.last().map_or(0.0, |k| k.1);
    if total <= 0.0 || knots.len() < 2 {
        return if knots.first().is_some_and(|k| x >= k.0) {
            1.0
        } else {
            0.0
        };
    }
    let i = knots.partition_point(|k| k.0 <= x);
    if i == 0 {
        return 0.0;
    }
    if i >= knots.len() {
        return 1.0;
    }
    let (a, b) = (knots[i - 1], knots[i]);
    let s = if b.0 > a.0 {
        (x - a.0) / (b.0 - a.0)
    } else {
        1.0
    };
    ((a.1 + (b.1 - a.1) * s) / total).clamp(0.0, 1.0)
}

/// The longest edge a tweened mesh keeps within a cut's reach (mm): a vertex's time is
/// interpolated across its triangles, and a bar's faces are strips its whole length.
const TWEEN_EDGE: f64 = 0.1;

/// An axis-aligned box.
#[derive(Debug, Clone, Copy)]
struct Bbox3 {
    min: Point3<f64>,
    max: Point3<f64>,
}

impl Bbox3 {
    fn empty() -> Self {
        Bbox3 {
            min: Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
    fn add(&mut self, p: &Point3<f64>) {
        self.min = self.min.inf(p);
        self.max = self.max.sup(p);
    }
    fn grow(self, r: f64) -> Self {
        let d = V3::new(r, r, r);
        Bbox3 {
            min: self.min - d,
            max: self.max + d,
        }
    }
    fn overlaps(&self, o: &Bbox3) -> bool {
        (0..3).all(|k| self.min[k] <= o.max[k] && o.min[k] <= self.max[k])
    }

    fn contains(&self, p: &Point3<f64>) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }
}

/// Each vertex's time (`time`), `m`'s triangles split (at their longest edges' middles) where
/// the times at their corners differ by more than [`TWEEN_STEP`] (or one is reached and another
/// never), down to edges [`TWEEN_EDGE`] long: where an interpolated time would put the cut's
/// front wrong. A triangle all one time stays as it is.
fn refine_by_times(
    m: &mut cadrs_cam::dexel::SimMesh,
    (coarse, reach): (f64, Bbox3),
    time: &(dyn Fn(&[f32; 3]) -> f32 + Sync),
) -> Vec<f32> {
    use rayon::prelude::*;
    use std::collections::HashMap;
    let mut times: Vec<f32> = m.positions.par_iter().map(time).collect();
    let mut todo: Vec<[u32; 3]> = m.indices.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();
    let mut done: Vec<u32> = Vec::with_capacity(m.indices.len());
    // Edges are known by where their ends are, not by their vertices: the tessellation repeats
    // vertices (each face has its own along an edge), and an edge split on one side but not on
    // the other cracks the mesh (pinholes along the cut's outline). Each split point is kept
    // for each normal it is made with (each face its own, so the faces shade as they did).
    let key = |p: &[f32; 3]| p.map(f32::to_bits);
    let mut at: HashMap<[u32; 3], u32> = HashMap::new();
    for (i, p) in m.positions.iter().enumerate() {
        at.entry(key(p)).or_insert(i as u32);
    }
    let canon = |at: &HashMap<[u32; 3], u32>, m: &cadrs_cam::dexel::SimMesh, i: u32| {
        at[&key(&m.positions[i as usize])]
    };
    // An edge's (canonical ends) split points, by normal.
    let mut middles: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    // The split point of `i`–`j` with their normal (made, timed later, if new: `fresh`).
    let middle = |m: &mut cadrs_cam::dexel::SimMesh,
                  at: &mut HashMap<[u32; 3], u32>,
                  middles: &mut HashMap<(u32, u32), Vec<u32>>,
                  times: &mut Vec<f32>,
                  fresh: &mut Vec<u32>,
                  i: u32,
                  j: u32,
                  make: bool|
     -> Option<u32> {
        let (ci, cj) = (canon(at, m, i), canon(at, m, j));
        let k = (ci.min(cj), ci.max(cj));
        let (ni, nj) = (m.normals[i as usize], m.normals[j as usize]);
        let n = [0, 1, 2].map(|a| 0.5 * (ni[a] + nj[a]));
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        let n = n.map(|x| x / l);
        let have = middles.get(&k);
        if let Some(&v) = have.and_then(|v| v.iter().find(|&&v| m.normals[v as usize] == n)) {
            return Some(v);
        }
        let other = have.and_then(|v| v.first().copied());
        if other.is_none() && !make {
            return None;
        }
        // (From the canonical ends, in their order: every face's copy at the very same place.)
        let (pa, pb) = (m.positions[k.0 as usize], m.positions[k.1 as usize]);
        m.positions.push([0, 1, 2].map(|a| 0.5 * (pa[a] + pb[a])));
        m.normals.push(n);
        m.colors.push(m.colors[i as usize]);
        m.tags.push(m.tags[i as usize]);
        let v = (m.positions.len() - 1) as u32;
        match other {
            Some(o) => times.push(times[o as usize]),
            None => {
                times.push(f32::NAN);
                fresh.push(v);
            }
        }
        at.entry(key(&m.positions[v as usize])).or_insert(v);
        middles.entry(k).or_default().push(v);
        Some(v)
    };
    let differ = |a: f32, b: f32| {
        if a.is_finite() && b.is_finite() {
            (a - b).abs() > TWEEN_STEP
        } else {
            a != b
        }
    };
    // In rounds, each round's new vertices timed together.
    while !todo.is_empty() {
        let mut next: Vec<[u32; 3]> = Vec::new();
        let mut fresh: Vec<u32> = Vec::new();
        for t in std::mem::take(&mut todo) {
            let q = |i: u32| {
                let p = m.positions[i as usize];
                V3::new(p[0] as f64, p[1] as f64, p[2] as f64)
            };
            let ts = [
                times[t[0] as usize],
                times[t[1] as usize],
                times[t[2] as usize],
            ];
            let split = differ(ts[0], ts[1]) || differ(ts[1], ts[2]) || differ(ts[2], ts[0]);
            let lens = [
                (q(t[1]) - q(t[0])).norm(),
                (q(t[2]) - q(t[1])).norm(),
                (q(t[0]) - q(t[2])).norm(),
            ];
            let (k, longest) = lens
                .iter()
                .copied()
                .enumerate()
                .fold((0, 0.0), |a, (i, l)| if l > a.1 { (i, l) } else { a });
            // (Or within the cut's reach and still wide: its corners may all miss the tool.)
            let wide = longest > coarse && {
                let mut b = Bbox3::empty();
                for i in t {
                    b.add(&Point3::from(q(i)));
                }
                b.overlaps(&reach)
            };
            let split = split || wide;
            if !split || longest <= TWEEN_EDGE || m.positions.len() > 4_000_000 {
                done.extend(t);
                continue;
            }
            let (i, j) = (t[k], t[(k + 1) % 3]);
            let mid =
                middle(m, &mut at, &mut middles, &mut times, &mut fresh, i, j, true).expect("made");
            let o = t[(k + 2) % 3];
            next.push([i, mid, o]);
            next.push([mid, j, o]);
        }
        let new: Vec<f32> = fresh
            .par_iter()
            .map(|&v| time(&m.positions[v as usize]))
            .collect();
        for (v, t) in fresh.into_iter().zip(new) {
            times[v as usize] = t;
        }
        // (A copy made for another face before its first was timed: timed now too.)
        for v in 0..times.len() {
            if times[v].is_nan() {
                times[v] = time(&m.positions[v]);
            }
        }
        todo = next;
    }
    // No T-junctions: a triangle beside a split edge is split there too (else the mesh cracks
    // along it, the stock behind showing through in dots).
    let mut todo: Vec<[u32; 3]> = done.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();
    let mut done: Vec<u32> = Vec::with_capacity(todo.len() * 3);
    let mut fresh = Vec::new();
    while let Some(t) = todo.pop() {
        let hanging = (0..3).find_map(|k| {
            let (i, j) = (t[k], t[(k + 1) % 3]);
            middle(
                m,
                &mut at,
                &mut middles,
                &mut times,
                &mut fresh,
                i,
                j,
                false,
            )
            .map(|mid| (k, mid))
        });
        match hanging {
            Some((k, mid)) => {
                let (i, j, o) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                todo.push([i, mid, o]);
                todo.push([mid, j, o]);
            }
            None => done.extend(t),
        }
    }
    m.indices = done;
    times
}

/// How near the tool a point of the stock after a cut is reached (mm), at least: its new faces
/// lie on the tool's sweep, their tessellation's points (and their f32 positions) off it by up to
/// the tessellation's deflection.
const ON_SWEEP: f64 = 2e-3;

/// How far apart (of the cut, 0 to 1) the times at a tweened triangle's corners may be.
const TWEEN_STEP: f32 = 0.02;

/// The longest piece of an edge a tween keeps between points whose times differ (mm): the
/// line's end is found between them by their times.
const TWEEN_EDGE_LINE: f64 = 0.02;

/// Lines (a stock's edges) with each point's time ([`refine_by_times`]'s, for lines): each
/// piece between points whose times differ halved till it is short.
fn edges_by_times(
    lines: Vec<Vec<[f32; 3]>>,
    time: &(dyn Fn(&[f32; 3]) -> f32 + Sync),
) -> (Vec<Vec<[f32; 3]>>, Vec<Vec<f32>>) {
    use rayon::prelude::*;
    let differ = |a: f32, b: f32| {
        if a.is_finite() && b.is_finite() {
            (a - b).abs() > TWEEN_STEP
        } else {
            a != b
        }
    };
    let len = |a: &[f32; 3], b: &[f32; 3]| {
        ((0..3).map(|k| ((a[k] - b[k]) as f64).powi(2)).sum::<f64>()).sqrt()
    };
    lines
        .into_par_iter()
        .map(|line| {
            let mut pts: Vec<[f32; 3]> = Vec::with_capacity(line.len());
            let mut ts: Vec<f32> = Vec::with_capacity(line.len());
            for w in line.windows(2) {
                // The piece from w[0] to w[1], halved where its ends' times differ.
                let (a, b) = (w[0], w[1]);
                let (ta, tb) = (time(&a), time(&b));
                if pts.is_empty() {
                    pts.push(a);
                    ts.push(ta);
                }
                let mut stack = vec![(b, tb)];
                let (mut at, mut t_at) = (a, ta);
                while let Some((q, tq)) = stack.pop() {
                    if differ(t_at, tq) && len(&at, &q) > TWEEN_EDGE_LINE {
                        let m = [0, 1, 2].map(|k| 0.5 * (at[k] + q[k]));
                        stack.push((q, tq));
                        stack.push((m, time(&m)));
                        continue;
                    }
                    pts.push(q);
                    ts.push(tq);
                    (at, t_at) = (q, tq);
                }
            }
            (pts, ts)
        })
        .unzip()
}

/// `m`'s triangles within `reach` split (at their longest edges' middles) till no edge is
/// longer than `edge`.
#[allow(dead_code)]
fn refine(m: &mut cadrs_cam::dexel::SimMesh, edge: f64, reach: &Bbox3) {
    let p = |m: &cadrs_cam::dexel::SimMesh, i: u32| {
        let q = m.positions[i as usize];
        Point3::new(q[0] as f64, q[1] as f64, q[2] as f64)
    };
    let mut middles: std::collections::HashMap<(u32, u32), u32> = Default::default();
    let mut todo: Vec<[u32; 3]> = m.indices.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();
    let mut done: Vec<u32> = Vec::with_capacity(m.indices.len());
    while let Some(t) = todo.pop() {
        let (a, b, c) = (p(m, t[0]), p(m, t[1]), p(m, t[2]));
        let near = [a, b, c].iter().any(|q| reach.contains(q))
            || reach.contains(&Point3::from((a.coords + b.coords + c.coords) / 3.0));
        let lens = [(b - a).norm(), (c - b).norm(), (a - c).norm()];
        let (k, longest) = lens
            .iter()
            .copied()
            .enumerate()
            .fold((0, 0.0), |m, (i, l)| if l > m.1 { (i, l) } else { m });
        if !near || longest <= edge || done.len() + todo.len() * 3 > 30_000_000 {
            done.extend(t);
            continue;
        }
        let (i, j) = (t[k], t[(k + 1) % 3]);
        let key = (i.min(j), i.max(j));
        let mid = *middles.entry(key).or_insert_with(|| {
            let (pi, pj) = (m.positions[i as usize], m.positions[j as usize]);
            let (ni, nj) = (m.normals[i as usize], m.normals[j as usize]);
            m.positions.push([0, 1, 2].map(|a| 0.5 * (pi[a] + pj[a])));
            let n = [0, 1, 2].map(|a| 0.5 * (ni[a] + nj[a]));
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            m.normals.push(n.map(|x| x / l));
            m.colors.push(m.colors[i as usize]);
            m.tags.push(m.tags[i as usize]);
            (m.positions.len() - 1) as u32
        });
        let o = t[(k + 2) % 3];
        todo.push([i, mid, o]);
        todo.push([mid, j, o]);
    }
    m.indices = done;
}

/// A cut's motion, to show the stock part-way along it without cutting it there: for each point
/// of the stock, how far along the cut the tool first reaches it. Drawn between the stock before
/// the cut (where the tool hasn't reached yet) and after it (its new faces where it has), the cut
/// sweeps on smoothly with the tool, each whole cut exact.
#[derive(Debug, Clone)]
pub struct Tween {
    cutter: Cutter,
    start: Pose,
    screw: Screw,
}

impl Tween {
    /// `p` in the tool's own frame (its tip at the origin, its axis up z) at `t` along.
    fn local(&self, p: &V3, t: f64) -> V3 {
        let m = self.screw.at(t);
        let w = m.linear.transpose() * (p - m.translation);
        self.start.rot.transpose() * (w - self.start.tip)
    }

    /// How far inside the tool a point of its frame is (negative outside): its radius short of
    /// the profile's at its height, within the tool's length.
    fn depth(&self, q: &V3) -> f64 {
        let c = &self.cutter;
        let r = q.x.hypot(q.y);
        // (The radius at the height on the tool: a point in its tip's plane a rounding below
        // it is beside the tip, not under nothing.)
        let radial = c.radius_at_height(q.z.clamp(0.0, c.length)) - r;
        radial.min(q.z).min(c.length - q.z)
    }

    /// The stretches of the cut (0 to 1) where the tool may reach `p`: where its path in the
    /// tool's frame passes the tool's bounding cylinder.
    fn windows(&self, p: &V3) -> Vec<(f64, f64)> {
        let c = &self.cutter;
        let reach = c.radius.hypot(c.length) + 1e-6;
        match self.screw {
            Screw::Slide(v) => {
                // The path is a line in the tool's frame: where it is within `reach` of the
                // tool's middle.
                let q0 = self.local(p, 0.0);
                let w = self.start.rot.transpose() * v;
                let mid = V3::new(0.0, 0.0, c.length / 2.0);
                let (a, b) = (w.norm_squared(), (q0 - mid).dot(&w));
                let cc = (q0 - mid).norm_squared() - reach * reach;
                if a < 1e-24 {
                    return if cc <= 0.0 {
                        vec![(0.0, 1.0)]
                    } else {
                        Vec::new()
                    };
                }
                // |q0 − w t − mid|² = reach²: a t² − 2 b t + cc = 0.
                let disc = b * b - a * cc;
                if disc < 0.0 {
                    return Vec::new();
                }
                let (t0, t1) = ((b - disc.sqrt()) / a, (b + disc.sqrt()) / a);
                let (t0, t1) = (t0.max(0.0), t1.min(1.0));
                if t0 <= t1 { vec![(t0, t1)] } else { Vec::new() }
            }
            Screw::Turn {
                at,
                dir,
                angle,
                rise,
            } => {
                // About the axis the point turns back against the tool: where its angle comes
                // within the tool's reach of the tool's middle, each turn.
                let mid = self.start.apply(V3::new(0.0, 0.0, c.length / 2.0));
                let flat = |x: &V3| {
                    let w = x - at;
                    w - dir * w.dot(&dir)
                };
                let (pm, pp) = (flat(&mid), flat(p));
                let (rm, rp) = (pm.norm(), pp.norm());
                // (Near the axis, or the tool across it: every angle.)
                if rm <= reach || rp <= 1e-9 || (rp - rm).abs() > reach {
                    if (rp - rm).abs() > reach && rm > reach {
                        return Vec::new();
                    }
                    return vec![(0.0, 1.0)];
                }
                let half = (reach / rm.min(rp)).min(1.0).asin() + 1e-3;
                // The angle the tool must turn for its middle to come round to the point.
                let x = pm / rm;
                let y = dir.cross(&x);
                let to = pp.dot(&y).atan2(pp.dot(&x)).rem_euclid(TAU);
                let _ = rise;
                let total = angle.abs();
                let mut out = Vec::new();
                let mut k = 0.0;
                while (to + TAU * k - half) / total <= 1.0 {
                    let (a, b) = ((to + TAU * k - half) / total, (to + TAU * k + half) / total);
                    let (a, b) = (a.max(0.0), b.min(1.0));
                    if a <= b {
                        out.push((a, b));
                    }
                    k += 1.0;
                }
                // (And the window round the start, the tool already there or past it.)
                let lead = ((half - to) / total).min(1.0);
                if lead > 0.0 {
                    out.insert(0, (0.0, lead));
                }
                out
            }
        }
    }

    /// How far along the cut the tool first reaches `p` (within `grow` of its surface): `None`
    /// where it never does.
    pub fn reached(&self, p: [f64; 3], grow: f64) -> Option<f64> {
        let p = V3::new(p[0], p[1], p[2]);
        // (Samples so close the tool moves no more than a twentieth of a millimetre between.)
        let speed = match self.screw {
            Screw::Slide(v) => v.norm(),
            Screw::Turn {
                at,
                dir,
                angle,
                rise,
            } => {
                // (The point's own arc about the axis, its radius out to the tool's reach.)
                let w = p - at;
                let rho = (w - dir * w.dot(&dir)).norm();
                angle.abs() * (rho + self.cutter.radius.hypot(self.cutter.length)) + rise.abs()
            }
        };
        for (a, b) in self.windows(&p) {
            let n = (((b - a) * speed / 0.05).ceil() as usize).clamp(4, 4096);
            let inside = |t: f64| self.depth(&self.local(&p, t)) >= -grow;
            if inside(a) {
                return Some(a);
            }
            let mut prev = a;
            for i in 1..=n {
                let t = a + (b - a) * i as f64 / n as f64;
                if inside(t) {
                    // The first contact, bisected.
                    let (mut lo, mut hi) = (prev, t);
                    for _ in 0..40 {
                        let m = 0.5 * (lo + hi);
                        if inside(m) {
                            hi = m;
                        } else {
                            lo = m;
                        }
                    }
                    return Some(hi);
                }
                prev = t;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    /// A tween's first contact, against the motion sampled finely by brute force: a slide and a
    /// half turn round a far axis (a worm's roughing), points the tool reaches and one it doesn't.
    #[test]
    fn a_tween_finds_when_the_tool_first_reaches_a_point() {
        use cadrs_cam::{Tool, ToolKind};
        let cutter = Cutter::of(&Tool::new(ToolKind::EndMill, 6.0));
        let up = Pose {
            rot: Matrix3::identity(),
            tip: V3::new(0.0, 0.0, 0.0),
        };
        let brute = |tw: &Tween, p: [f64; 3]| {
            let q = V3::new(p[0], p[1], p[2]);
            (0..=200_000)
                .map(|i| i as f64 / 200_000.0)
                .find(|&t| tw.depth(&tw.local(&q, t)) >= 0.0)
        };
        let slide = Tween {
            cutter: cutter.clone(),
            start: up,
            screw: Screw::Slide(V3::new(10.0, 0.0, 0.0)),
        };
        let turn = Tween {
            cutter,
            start: Pose {
                rot: Matrix3::identity(),
                tip: V3::new(0.0, 0.0, 20.0),
            },
            screw: Screw::Turn {
                at: V3::zeros(),
                dir: V3::new(1.0, 0.0, 0.0),
                angle: std::f64::consts::PI,
                rise: 0.0,
            },
        };
        for (tw, p) in [
            (&slide, [5.0, 0.0, 1.0]),
            (&slide, [8.0, 2.0, 4.0]),
            (&slide, [5.0, 4.0, 1.0]),
            (&turn, [0.0, -20.0, 1.0]),
            (&turn, [0.5, 14.0, -14.0]),
            (&turn, [0.0, 0.0, -21.0]),
            (&turn, [9.0, 0.0, -21.0]),
        ] {
            let (got, want) = (tw.reached(p, 0.0), brute(tw, p));
            match (got, want) {
                (Some(g), Some(w)) => assert!((g - w).abs() < 1e-4, "{p:?}: {g} against {w}"),
                (None, None) => {}
                _ => panic!("{p:?}: {got:?} against {want:?}"),
            }
        }
        // (The front of a radius 3 tool at x = 5 when its axis is at 2: a fifth of the way.)
        assert!((slide.reached([5.0, 0.0, 1.0], 0.0).unwrap() - 0.2).abs() < 1e-6);
    }

    use super::*;

    fn pose(rot: Matrix3<f64>, tip: V3) -> Pose {
        Pose { rot, tip }
    }

    #[test]
    fn the_screw_between_two_poses_takes_one_to_the_other() {
        // A turn of 30° about the x axis through (0, 5, 0) and 2 along it.
        let axis = Axis::new(Point3::new(0.0, 5.0, 0.0), V3::x());
        let m = Motion::rotation(&axis, 0.5).then(&Motion::translation(V3::x() * 2.0));
        let a = pose(Matrix3::identity(), V3::new(1.0, 2.0, 3.0));
        let b = pose(m.linear * a.rot, m.linear * a.tip + m.translation);
        let s = Screw::between(&a, &b);
        let Screw::Turn {
            at,
            dir,
            angle,
            rise,
        } = s
        else {
            panic!("{s:?}");
        };
        assert!((angle - 0.5).abs() < 1e-12 && (rise - 2.0).abs() < 1e-12);
        assert!((dir - V3::x()).norm() < 1e-12);
        assert!((at - V3::new(at.x, 5.0, 0.0)).norm() < 1e-9, "{at:?}");
        // Its end is `b`, and half way the tool has turned 15° and slid 1.
        let end = s.at(1.0);
        assert!((end.linear * a.tip + end.translation - b.tip).norm() < 1e-12);
        let half = s.at(0.5);
        let m = Motion::rotation(&axis, 0.25).then(&Motion::translation(V3::x()));
        assert!(
            ((half.linear * a.tip + half.translation) - (m.linear * a.tip + m.translation)).norm()
                < 1e-12
        );
        // Two halves are one cut, the first ending half way along it.
        let h = pose(half.linear * a.rot, half.linear * a.tip + half.translation);
        let cut = |from: Pose, to: Pose| {
            let screw = Screw::between(&from, &to);
            Cut {
                op: 0,
                start: from,
                screw,
                knots: vec![(0.0, 0.0), (1.0, screw.amount())],
                poses: vec![(to, screw.amount())],
                top: 10.0,
                machine: false,
            }
        };
        let mut c0 = cut(a, h);
        assert!(c0.extend(b, &Screw::between(&h, &b), 2.0, false));
        assert!((c0.screw.amount() - 0.5).abs() < 1e-12);
        assert!((c0.along(1.0) - 0.5).abs() < 1e-12 && (c0.along(2.0) - 1.0).abs() < 1e-12);
        assert!((c0.along(1.5) - 0.75).abs() < 1e-12);
        // A slide is a slide; collinear ones are one, others not.
        let c = pose(Matrix3::identity(), V3::new(4.0, 2.0, 3.0));
        let d = pose(Matrix3::identity(), V3::new(7.0, 2.0, 3.0));
        let e = pose(Matrix3::identity(), V3::new(7.0, 5.0, 3.0));
        assert_eq!(Screw::between(&a, &c), Screw::Slide(V3::new(3.0, 0.0, 0.0)));
        assert!(cut(a, c).extend(d, &Screw::between(&c, &d), 2.0, false));
        assert!(!cut(c, d).extend(e, &Screw::between(&d, &e), 2.0, false));
    }
}
