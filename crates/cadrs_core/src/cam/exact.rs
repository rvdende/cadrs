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
    /// Its motion is the machine's own screw about one of its axes ([`machine_screw`]), not
    /// one fitted to its poses.
    machine: bool,
}

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
                        .all(|q| (end.apply(*q) - pose.apply(*q)).norm() <= MERGE)
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
            if worst > MERGE {
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
                    (m.linear * self.start.apply(*q) + m.translation - p.apply(*q)).norm() <= MERGE
                })
            }) && {
                // (And the new end, where the joined screw takes the tool.)
                let end = joined.moved(&self.start);
                tops.iter()
                    .all(|q| (end.apply(*q) - pose.apply(*q)).norm() <= MERGE)
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
        if std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "5") && worst > MERGE {
            eprintln!("machine screw: misses by {worst:.3e}, axis {off:.3e} off");
        }
        let holds = worst <= MERGE;
        holds.then_some(s)
    })
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
                // The machine's own screw where the move is one, else the one fitted.
                let mid = pose(&lerp((f0 + f1) / 2.0));
                let exact = machine_screw(&a, &b, &mid, &fitted, &lines, o.tool.cutting_length);
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
                    continue;
                }
                held = Some(screw.moved(&a));
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
        let start = stock_solid(&mut k, shape, &run.frame)
            .ok_or("the stock has no exact solid (only boxes and bars do)")?;
        let tools = run
            .ops
            .iter()
            .map(|o| tool_solid(&mut k, o.tool))
            .collect::<Result<Vec<_>, _>>()?;
        let cuts = cuts_of(machine, run, motions, rows);
        if std::env::var_os("CADRS_EXACT_DEBUG").is_some_and(|v| v == "4") {
            for (i, c) in cuts.iter().enumerate().filter(|(_, c)| c.poses.len() > 100) {
                eprintln!("exact cut {i}: {} moves, {:?}", c.poses.len(), c.screw);
            }
        }
        Ok(ExactStock {
            k,
            start,
            tools,
            cuts,
            kept: start,
            done: 0,
            shown: start,
            at: (0, 0.0),
            tags: HashMap::new(),
            failed: Vec::new(),
        })
    }

    /// How many moves each cut takes.
    pub fn cut_sizes(&self) -> Vec<usize> {
        self.cuts.iter().map(|c| c.poses.len()).collect()
    }

    /// How many cuts the run is made in (its moves on one motion each one).
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
    fn cut(&mut self, body: BodyId, c: usize, f0: f64, f1: f64) -> BodyId {
        self.cut_in(body, c, f0, f1, 0)
    }

    fn cut_in(&mut self, body: BodyId, c: usize, f0: f64, f1: f64, depth: u32) -> BodyId {
        match self.cut_once(body, c, f0, f1) {
            Ok(b) => b,
            Err(e) if depth >= 5 => {
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
        Ok(new)
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
        }
        let place = to.0 as f64 + to.1;
        if self.shown != self.kept {
            self.drop_body(self.shown);
        }
        // The cuts the tool is through.
        while self.done < self.cuts.len() && self.cuts[self.done].to() <= place + 1e-12 {
            let kept = self.kept;
            let b = self.cut(kept, self.done, 0.0, 1.0);
            if b != kept {
                self.drop_body(kept);
            }
            self.kept = b;
            self.done += 1;
        }
        self.shown = self.kept;
        // The cut the tool is on, as far as it has gone.
        if let Some(c) = self.cuts.get(self.done)
            && c.from() < place
        {
            let along = c.along(place);
            if along > 1e-9 {
                let kept = self.kept;
                self.shown = self.cut(kept, self.done, 0.0, along);
            }
        }
        self.at = to;
    }

    /// The stock as shown, tessellated (`deflection` mm), with each triangle's operation (from
    /// 1; 0 the stock's own surface).
    pub fn mesh(&self, deflection: f64) -> Option<(TriMesh, Vec<u8>)> {
        let m = self
            .k
            .tessellate(
                self.shown,
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
                    .get(&self.shown)
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
        let (m, tags) = self.mesh(deflection)?;
        let mut vertex_tags = vec![0u8; m.positions.len()];
        for (t, tag) in m.indices.iter().zip(&tags) {
            for i in t {
                vertex_tags[*i as usize] = *tag;
            }
        }
        let f = |v: &V3| [v.x as f32, v.y as f32, v.z as f32];
        Some(cadrs_cam::dexel::SimMesh {
            positions: m.positions.iter().map(|p| f(&p.coords)).collect(),
            normals: m.normals.iter().map(f).collect(),
            indices: m.indices.iter().flatten().copied().collect(),
            colors: vertex_tags
                .iter()
                .map(|t| cadrs_cam::dexel::tag_color(*t))
                .collect(),
            tags: vertex_tags,
        })
    }

    /// The stock's volume as shown (mm³).
    pub fn volume(&self) -> f64 {
        self.k
            .mass_properties(self.shown)
            .map(|m| m.volume)
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
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
