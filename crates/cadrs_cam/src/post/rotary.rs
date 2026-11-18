//! Posting through the machine's axes: operations whose tool leans as it cuts (4- and 5-axis
//! work), and tilted work planes on a control without a tilted-working-plane code.
//!
//! Such an operation's moves are cutter locations: where the tip is and which way the tool
//! points, relative to the work. The machine's [`Chain`] turns each into axis values
//! ([`Chain::solve`]), and what is written depends on who keeps the tip on its line:
//!
//! - **The control** (tool centre point control: Fanuc `G43.4`, Haas `G234`, LinuxCNC's
//!   switchable kinematics): X Y Z are the tip in the work's coordinates, with the rotary
//!   angles beside them. The control moves its slides to suit, whatever the tool's length and
//!   wherever the work sits on the table.
//! - **The post** (every other control): X Y Z are the slides themselves, measured from where
//!   they hold the tip on the work's origin with the rotary axes at zero, so with the rotaries
//!   at zero the program reads as a 3-axis one. Between two blocks the control runs every axis
//!   straight, which bends the tip's path wherever an axis turns, so a move is cut up until it
//!   stays within tolerance ([`Chain::linearize`]). Such a program is good only for the work
//!   set where it was programmed from the rotary axes.
//!
//! Feeds are inverse time (`G93`, each block's `F` the reciprocal of its minutes) unless set
//! otherwise: the one feed whose meaning does not depend on how a control weighs a degree
//! against a millimetre. Blocks that lie on one line in the written words are merged (a helix
//! round a rotary axis is one block). Where an axis runs out of travel mid-cut the tool lifts
//! clear, the axis comes round, and the cut goes on.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::{Dialect, NcOperation, PostError, PostSettings, Tilt, Writer, decimal};
use crate::geom::{Frame, P3, add, dist, scale};
use crate::kinematics::{Chain, Cl, IkError, Tolerance, angle_between};
use crate::toolpath::{Step, UPRIGHT};

/// What `F` means on a block that turns a rotary axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RotaryFeed {
    /// `G93`: `F` is one over the block's minutes.
    #[default]
    InverseTime,
    /// `G94`, the feed counted along X Y Z; a block that only turns is in degrees a minute
    /// (LinuxCNC's and Mach3's rule).
    LinearThenDegrees,
    /// `G94`, the feed counted over every axis, a degree as a millimetre.
    AllAxes,
}

impl RotaryFeed {
    pub const ALL: [RotaryFeed; 3] = [
        RotaryFeed::InverseTime,
        RotaryFeed::LinearThenDegrees,
        RotaryFeed::AllAxes,
    ];
    pub fn label(self) -> &'static str {
        match self {
            RotaryFeed::InverseTime => "Inverse time (G93)",
            RotaryFeed::LinearThenDegrees => "Along X Y Z; degrees/min when only turning",
            RotaryFeed::AllAxes => "Over every axis",
        }
    }
}

/// How a tilted work plane (3+2) is posted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TiltedPlane {
    /// The control's own tilted working plane where it has one (Fanuc `G68.2`), else the
    /// rotary axes' positions.
    #[default]
    Control,
    /// Always the rotary axes' positions, the moves in the machine's axes.
    Positions,
}

/// A machine's post options for its rotary axes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RotaryPost {
    /// The control keeps the tool's tip on its path as the rotary axes turn (tool centre point
    /// control).
    pub tcp: bool,
    /// The blocks that switch it on and off (`;` between blocks, `{h}` the tool's number);
    /// empty: the controller family's own.
    pub tcp_on: String,
    pub tcp_off: String,
    pub tilted_plane: TiltedPlane,
    pub feed: RotaryFeed,
    /// How far the tip may stray from a straight cut between two blocks (mm).
    pub tolerance: f64,
    /// How far the tool's axis may (degrees).
    pub axis_tolerance: f64,
    /// How far the tool lifts along its axis where a rotary axis has to come round (mm).
    pub retract: f64,
    /// Decimals of the angles.
    pub angle_decimals: u32,
}

impl Default for RotaryPost {
    fn default() -> Self {
        RotaryPost {
            tcp: false,
            tcp_on: String::new(),
            tcp_off: String::new(),
            tilted_plane: TiltedPlane::Control,
            feed: RotaryFeed::InverseTime,
            tolerance: 0.01,
            axis_tolerance: 0.1,
            retract: 25.0,
            angle_decimals: 3,
        }
    }
}

impl RotaryPost {
    /// The defaults for a controller family: tool centre point control where it is usual.
    pub fn for_dialect(d: Dialect) -> RotaryPost {
        RotaryPost {
            tcp: matches!(d, Dialect::Fanuc | Dialect::Haas),
            ..RotaryPost::default()
        }
    }

    /// The blocks that switch tool centre point control on and off for tool `h`, and whether
    /// switching it off takes the tool's length compensation with it; `None` where it is off
    /// or the control has none.
    pub fn tcp_codes(&self, d: Dialect, h: u32) -> Option<(Vec<String>, Vec<String>, bool)> {
        if !self.tcp {
            return None;
        }
        let own = |text: &str| -> Vec<String> {
            text.split(';')
                .map(|b| b.trim().replace("{h}", &h.to_string()))
                .filter(|b| !b.is_empty())
                .collect()
        };
        let (on, off, drops) = match d {
            Dialect::Fanuc => (vec![format!("G43.4 H{h}")], vec!["G49".to_string()], true),
            Dialect::Haas => (vec![format!("G234 H{h}")], vec!["G49".to_string()], true),
            // LinuxCNC switches its kinematics (the 5-axis sample configurations' M428 and
            // M429); the tool's length stays on.
            Dialect::LinuxCnc => (
                vec![format!("G43 H{h}"), "M428".to_string()],
                vec!["M429".to_string()],
                false,
            ),
            Dialect::Grbl | Dialect::Mach3 if self.tcp_on.trim().is_empty() => return None,
            Dialect::Grbl | Dialect::Mach3 => (vec![], vec![], false),
        };
        let on = if self.tcp_on.trim().is_empty() {
            on
        } else {
            own(&self.tcp_on)
        };
        let off = if self.tcp_off.trim().is_empty() {
            off
        } else {
            own(&self.tcp_off)
        };
        Some((on, off, drops))
    }
}

impl RotaryPost {
    /// [`RotaryPost::tcp_codes`] for a machine with `rotaries` rotary axes: tool centre point
    /// control is a five-axis control's (two rotary axes or more); a machine with one posts
    /// its slides and its angle plainly, where a helix is one block.
    pub fn tcp_for(
        &self,
        d: Dialect,
        h: u32,
        rotaries: usize,
    ) -> Option<(Vec<String>, Vec<String>, bool)> {
        if rotaries < 2 {
            return None;
        }
        self.tcp_codes(d, h)
    }

    /// The block that tells the control a rotary axis now reads `value` where it stands (a
    /// continuous axis wound many turns is counted from nearer zero again, with no unwinding).
    pub fn reset_code(d: Dialect, letter: char, value: &str) -> String {
        match d {
            Dialect::LinuxCnc | Dialect::Grbl => format!("G10 L20 P0 {letter}{value}"),
            Dialect::Fanuc | Dialect::Haas | Dialect::Mach3 => format!("G92 {letter}{value}"),
        }
    }
}

/// How an operation is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// In the work offset's own axes, the rotary axes at zero.
    Plain,
    /// In a tilted working plane the control keeps (`G68.2`).
    Plane,
    /// Through the machine's axes.
    Axes,
}

pub(super) fn route(op: &NcOperation, d: Dialect, s: &PostSettings, rotaries: usize) -> Route {
    if op.toolpath.is_multi_axis() {
        Route::Axes
    } else if op.tilt.is_none() {
        Route::Plain
    } else if d == Dialect::Fanuc && s.rotary.tilted_plane == TiltedPlane::Control && rotaries >= 2
    {
        // (A tilted working plane is a five-axis control's: one rotary axis is indexed.)
        Route::Plane
    } else {
        Route::Axes
    }
}

/// The slides' values that hold the tip of a tool `length` long on the origin of `work` (the
/// work's coordinate system, in the table mount's coordinates) with every rotary axis at zero:
/// what a program's X Y Z are measured from without tool centre point control.
pub fn program_zero(chain: &Chain, work: &Frame, length: f64) -> Option<Vec<f64>> {
    chain.slide_to(work.origin, length, &chain.rest())
}

/// Where a chain's X, Y and Z slides are among its axis values, and its rotary axes with their
/// letters.
pub type AxisWords = ([usize; 3], Vec<(usize, char)>);

/// The axes of a chain as a program names them; why not, for a chain a G-code program cannot
/// drive.
pub fn words_of(chain: &Chain) -> Result<AxisWords, String> {
    if !chain.is_mill() {
        return Err(
            "the machine isn't three slides with up to two rotary axes (a robot arm has no \
             G-code post yet)"
                .into(),
        );
    }
    let slide = |n: &str| {
        chain
            .index(n)
            .filter(|i| !chain.joint(*i).is_rotary())
            .ok_or(format!("the machine has no linear axis named {n}"))
    };
    let lin = [slide("X")?, slide("Y")?, slide("Z")?];
    let mut rot = Vec::new();
    for i in chain.rotaries() {
        let name = &chain.joint(i).name;
        let mut letters = name.chars();
        match (letters.next(), letters.next()) {
            (Some(c), None) if "ABCUVW".contains(c.to_ascii_uppercase()) => {
                rot.push((i, c.to_ascii_uppercase()))
            }
            _ => {
                return Err(format!(
                    "the rotary axis \"{name}\" needs a letter (A, B or C)"
                ));
            }
        }
    }
    Ok((lin, rot))
}

/// A feed move waiting to be written (moves on one line are merged first).
struct Pending {
    /// The words: X Y Z, then the rotary axes.
    v: Vec<f64>,
    minutes: f64,
    /// The feed it was programmed at (mm/min along the tip's path).
    feed: f64,
}

/// An operation being written through the machine's axes.
pub(super) struct Axes {
    chain: Chain,
    /// The work's coordinate system in the table mount's coordinates.
    work: Frame,
    /// The operation's own frame in the work's (a tilted work plane).
    local: Option<Frame>,
    /// The tool's length from its tip to the gauge line.
    length: f64,
    lin: [usize; 3],
    rot: Vec<(usize, char)>,
    /// See [`program_zero`].
    zero: Vec<f64>,
    /// The axis values now.
    q: Vec<f64>,
    /// The cutter location now (`None` before the first move).
    cl: Option<Cl>,
    /// The tool's axis now (the operation's frame) and the lean the next move ends with.
    axis: P3,
    lean: Option<P3>,
    /// The rotary words last written.
    written: Vec<Option<String>>,
    /// The words of the last block (X Y Z, then the rotary axes).
    last: Option<Vec<f64>>,
    pending: Vec<Pending>,
    tcp: bool,
    /// The tool never leans within the operation (a tilted work plane): nothing turns as it
    /// cuts, so its feeds are plain ones along X Y Z.
    fixed: bool,
    /// The blocks that end tool centre point control.
    tcp_off: Vec<String>,
    /// `G93` is on.
    inverse: bool,
    feed_mode: RotaryFeed,
    tol: Tolerance,
    retract: f64,
    angle_decimals: u32,
    op: String,
    /// The first thing that could not be written.
    pub error: Option<PostError>,
    /// Ending tool centre point control took the tool's length compensation with it.
    pub length_dropped: bool,
    /// Rewinds made (a rotary axis come round with the tool lifted).
    pub rewinds: usize,
    /// The next lean the toolpath makes after the move in hand (the table mount's
    /// coordinates): what a rotary axis the tool's lean leaves free is turned to.
    ahead: Option<P3>,
    dialect: Dialect,
}

impl Axes {
    pub fn new(
        chain: &Chain,
        work: &Frame,
        op: &NcOperation,
        d: Dialect,
        s: &PostSettings,
        from: Option<&[f64]>,
    ) -> Result<Axes, PostError> {
        let name = &op.name;
        let (lin, rot) = words_of(chain).map_err(|e| PostError::NoPost(format!("{name}: {e}")))?;
        let length = op.tool.gauge_length();
        let zero = program_zero(chain, work, length).ok_or_else(|| {
            PostError::NoPost(format!(
                "{name}: the machine's slides don't reach every direction"
            ))
        })?;
        let codes = s.rotary.tcp_for(d, op.tool_number, rot.len());
        let tcp = codes.is_some();
        Ok(Axes {
            q: from.map_or_else(|| zero.clone(), <[f64]>::to_vec),
            chain: chain.clone(),
            work: *work,
            local: op.tilt.map(|t| t.frame()),
            length,
            lin,
            written: vec![None; rot.len()],
            rot,
            zero,
            cl: None,
            axis: UPRIGHT,
            lean: None,
            last: None,
            pending: Vec::new(),
            tcp,
            fixed: !op.toolpath.is_multi_axis(),
            tcp_off: codes.map(|c| c.1).unwrap_or_default(),
            inverse: false,
            feed_mode: s.rotary.feed,
            tol: Tolerance {
                tip: s.rotary.tolerance.max(1e-4),
                axis: s.rotary.axis_tolerance.max(1e-3),
                tcp,
            },
            retract: s.rotary.retract.max(0.0),
            angle_decimals: s.rotary.angle_decimals,
            op: name.clone(),
            error: None,
            length_dropped: false,
            rewinds: 0,
            ahead: None,
            dialect: d,
        })
    }

    /// The axis values where the operation leaves the machine.
    pub fn position(&self) -> &[f64] {
        &self.q
    }

    /// Notes the next lean the toolpath makes (the operation's frame), for looking ahead.
    pub fn look_ahead(&mut self, axis: Option<P3>) {
        self.ahead = axis.map(|a| self.cl_of([0.0; 3], a).axis);
    }

    /// The axis values for `cl`, on from where the machine is, a free rotary turned to what
    /// comes next.
    fn solve(&self, cl: &Cl) -> Result<Vec<f64>, IkError> {
        self.chain
            .solve_toward(cl, self.length, &self.q, self.ahead)
    }

    /// A continuous rotary axis wound a turn or more is counted from nearer zero again: the
    /// control is told what it reads where it stands, and nothing moves. (Not under tool
    /// centre point control, which such a code would upset.)
    fn unwind(&mut self, w: &mut Writer, past: f64) {
        for (k, (i, letter)) in self.rot.clone().into_iter().enumerate() {
            let j = self.chain.joint(i);
            if !j.continuous || self.q[i].abs() < past {
                continue;
            }
            let reduced = self.q[i] - 360.0 * (self.q[i] / 360.0).round();
            let text = decimal(reduced, self.angle_decimals, w.s.decimal_point);
            w.block(&RotaryPost::reset_code(self.dialect, letter, &text));
            self.q[i] = reduced;
            self.written[k] = Some(text);
            if let Some(last) = &mut self.last {
                last[3 + k] = reduced;
            }
        }
    }

    /// The cutter location (the table mount's coordinates) of a point and an axis given in
    /// the operation's frame.
    fn cl_of(&self, p: P3, axis: P3) -> Cl {
        let (p, a) = match &self.local {
            Some(f) => (f.to_world(p), f.dir_to_world(axis)),
            None => (p, axis),
        };
        Cl::new(self.work.to_world(p), self.work.dir_to_world(a))
    }

    /// The words of a position: X Y Z, then the rotary axes.
    fn words(&self, cl: &Cl, q: &[f64]) -> Vec<f64> {
        let xyz = if self.tcp {
            self.work.to_local(cl.tip)
        } else {
            [
                q[self.lin[0]] - self.zero[self.lin[0]],
                q[self.lin[1]] - self.zero[self.lin[1]],
                q[self.lin[2]] - self.zero[self.lin[2]],
            ]
        };
        let mut v = xyz.to_vec();
        v.extend(self.rot.iter().map(|(i, _)| q[*i]));
        v
    }

    fn fail(&mut self, e: IkError) {
        if self.error.is_none() {
            let text = format!("{}: {e}", self.op);
            self.error = Some(match e {
                IkError::Limit { .. } => PostError::Travel(text),
                _ => PostError::CantTilt(text),
            });
        }
    }

    /// The rotary words that changed, and the angles noted as written.
    fn rotary_words(&mut self, w: &Writer, v: &[f64]) -> String {
        let mut out = String::new();
        for (k, (_, letter)) in self.rot.iter().enumerate() {
            let text = decimal(v[3 + k], self.angle_decimals, w.s.decimal_point);
            if self.written[k].as_deref() != Some(text.as_str()) {
                let _ = write!(out, " {letter}{text}");
                self.written[k] = Some(text);
            }
        }
        out
    }

    /// The operation's start: the rotary axes to where its first move needs them (the tool is
    /// up clear), then the tool's length or tool centre point control on.
    pub fn begin(
        &mut self,
        w: &mut Writer,
        op: &NcOperation,
        d: Dialect,
        s: &PostSettings,
        length_comp: bool,
    ) {
        let mut axis = UPRIGHT;
        let first = op.toolpath.steps.iter().find_map(|st| match st {
            Step::Axis(a) => {
                axis = *a;
                None
            }
            Step::Rapid(p) | Step::Feed { to: p, .. } | Step::Arc { to: p, .. } => Some(*p),
            Step::Drill(c) => Some([c.at[0], c.at[1], c.retract]),
            _ => None,
        });
        if let Some(p) = first {
            let cl = self.cl_of(p, axis);
            // (What the first move's lean leaves free is turned to the first lean after it.)
            let mut leans = op.toolpath.steps.iter().filter_map(|st| match st {
                Step::Axis(a) => Some(*a),
                _ => None,
            });
            let next = leans.find(|a| dist(*a, axis) > 1e-9);
            self.look_ahead(next);
            self.unwind(w, 180.0);
            match self.solve(&cl) {
                Ok(q) => {
                    let v = self.words(&cl, &q);
                    let turn = self.rotary_words(w, &v);
                    if !turn.is_empty() {
                        w.block(&format!("G0{turn}"));
                    }
                    w.mode = None;
                    // The slides' words are written by the first move itself.
                    for (i, _) in &self.rot {
                        self.q[*i] = q[*i];
                    }
                }
                Err(e) => self.fail(e),
            }
        }
        // Without tool centre point control a rotary axis on the tool's side swings the tip
        // by the tool's length: the program holds for the length it was posted with.
        if !self.tcp && self.chain.tool.iter().any(|j| j.is_rotary()) {
            w.comment(&format!(
                "T{} is posted {} mm long from the spindle's gauge line: set the tool to it",
                op.tool_number,
                decimal(self.length, 3, false)
            ));
        }
        match s.rotary.tcp_for(d, op.tool_number, self.rot.len()) {
            Some((on, _, _)) => {
                for b in on {
                    w.block(&b);
                }
            }
            None if length_comp && d != Dialect::Grbl => {
                w.block(&format!("G43 H{}", op.tool_number))
            }
            None => {}
        }
        w.at = [None; 3];
    }

    /// The operation's end: the feed mode back, tool centre point control off.
    pub fn end(&mut self, w: &mut Writer, d: Dialect, s: &PostSettings, tool: u32) {
        self.flush(w);
        if self.inverse {
            w.block("G94");
            self.inverse = false;
            w.feed = None;
        }
        if let Some((_, _, drops)) = s.rotary.tcp_for(d, tool, self.rot.len()) {
            for b in std::mem::take(&mut self.tcp_off) {
                w.block(&b);
            }
            self.length_dropped = drops;
        }
        self.unwind(w, 180.0);
        w.at = [None; 3];
        w.mode = None;
    }

    /// The lean the next move ends with ([`Step::Axis`]).
    pub fn lean(&mut self, axis: P3) {
        self.lean = Some(crate::geom::normalize(axis));
    }

    pub fn rapid(&mut self, w: &mut Writer, p: P3) {
        self.flush(w);
        let axis = self.lean.take().unwrap_or(self.axis);
        self.axis = axis;
        let cl = self.cl_of(p, axis);
        if !self.tcp {
            self.unwind(w, 360.0);
        }
        let q = match self.solve(&cl) {
            Ok(q) => q,
            Err(e) => return self.fail(e),
        };
        let first = self.cl.is_none();
        // A rapid that turns a rotary axis without tool centre point control would swing the
        // tip off its way: it is made in pieces that keep the tip near the straight line.
        let turns = self
            .rot
            .iter()
            .any(|(i, _)| (q[*i] - self.q[*i]).abs() > 1.0);
        if let (Some(from), true, false, false) = (self.cl, turns, self.tcp, first) {
            let loose = Tolerance {
                tip: (self.retract / 8.0).clamp(0.1, 2.0),
                axis: 5.0,
                tcp: false,
            };
            if let Ok(pieces) =
                self.chain
                    .linearize_to(&from, &self.q, &cl, &q, self.length, &loose)
            {
                for wp in pieces {
                    let at = from.toward(&cl, wp.t);
                    let v = self.words(&at, &wp.q);
                    self.go_rapid(w, &v, false);
                }
                self.q = q;
                self.cl = Some(cl);
                return;
            }
        }
        let v = self.words(&cl, &q);
        self.go_rapid(w, &v, first);
        self.q = q;
        self.cl = Some(cl);
    }

    /// A rapid to the words `v`; the operation's first goes across before it goes down (the
    /// tool comes from its home height).
    fn go_rapid(&mut self, w: &mut Writer, v: &[f64], across_first: bool) {
        // (A rapid takes no feed: inverse time stays on through it.)
        let turn = self.rotary_words(w, v);
        let p = [v[0], v[1], v[2]];
        if across_first && w.at[2].is_none() {
            let z = w.at[2];
            // (X and Y alone: Z is left for the block after.)
            w.at[2] = Some(p[2]);
            let across = w.axes(p);
            w.at[2] = z;
            if !across.is_empty() || !turn.is_empty() {
                let m = w.motion(0);
                w.block(format!("{m}{across}{turn}").trim());
            }
            let down = w.axes(p);
            if !down.is_empty() {
                let m = w.motion(0);
                w.block(format!("{m}{down}").trim());
            }
        } else {
            let axes = w.axes(p);
            if !axes.is_empty() || !turn.is_empty() {
                let m = w.motion(0);
                w.block(format!("{m}{axes}{turn}").trim());
            }
        }
        self.last = Some(v.to_vec());
    }

    pub fn linear(&mut self, w: &mut Writer, p: P3, feed: f64) {
        let axis = self.lean.take().unwrap_or(self.axis);
        self.axis = axis;
        let to = self.cl_of(p, axis);
        let Some(from) = self.cl else {
            // A feed with no position before it: straight there.
            match self.solve(&to) {
                Ok(q) => {
                    let v = self.words(&to, &q);
                    let minutes = self.chain.least_minutes(&self.q, &q, false).max(1e-6);
                    self.pending.push(Pending { v, minutes, feed });
                    self.q = q;
                    self.cl = Some(to);
                }
                Err(e) => self.fail(e),
            }
            return;
        };
        // Where the tool's lean leaves a rotary axis free here (upright over a platter) and the
        // move leans it, that axis is turned first, the tool standing where it is: the lean
        // fixes the axis the moment it starts, and it can't turn there in no time.
        self.turn_in_place(&from, &to, feed);
        // Where the move ends: followed from here, the machine keeping to its side.
        let cut = self
            .chain
            .follow(&from, &self.q, &to, self.length)
            .and_then(|q1| {
                self.chain
                    .linearize_to(&from, &self.q, &to, &q1, self.length, &self.tol)
            });
        let cut = match cut {
            Ok(c) => c,
            Err(e) => return self.fail(e),
        };
        let length = dist(from.tip, to.tip);
        let mut t0 = 0.0;
        for wp in cut {
            let cl = from.toward(&to, wp.t);
            if wp.leap {
                self.rewind(w, &cl, &wp.q, feed);
            } else {
                // The block's time: the tip's way at the feed, or longer where an axis cannot
                // keep up.
                let minutes = ((wp.t - t0) * length / feed.max(1e-6))
                    .max(self.chain.least_minutes(&self.q, &wp.q, false))
                    .max(1e-7);
                let v = self.words(&cl, &wp.q);
                self.pending.push(Pending { v, minutes, feed });
            }
            t0 = wp.t;
            self.q = wp.q;
            self.cl = Some(cl);
        }
    }

    /// Turns the rotary axes the tool's lean at `from` leaves free to where the move to `to`
    /// needs them the moment it leaves, the tool held at `from` (under tool centre point control
    /// the turn alone; without it, the slides keep the tip where it is as it turns).
    fn turn_in_place(&mut self, from: &Cl, to: &Cl, feed: f64) {
        let free = self.chain.free_rotaries(from, self.length, &self.q);
        if free.is_empty() || angle_between(from.axis, to.axis) < 1e-9 {
            return;
        }
        // Just after the start the lean has its way: the free axes there.
        let n = (angle_between(from.axis, to.axis) / 0.05).ceil().max(1.0);
        let start = from.toward(to, 1.0 / n);
        let Ok(q) = self.chain.solve(&start, self.length, &self.q) else {
            return;
        };
        let mut turned = self.q.clone();
        for i in &free {
            turned[*i] = q[*i];
        }
        if free.iter().all(|i| (turned[*i] - self.q[*i]).abs() < 1e-6) {
            return;
        }
        let Ok(turned) = self.chain.solve(from, self.length, &turned) else {
            return;
        };
        let pieces =
            match self
                .chain
                .linearize_to(from, &self.q, from, &turned, self.length, &self.tol)
            {
                Ok(p) => p,
                Err(e) => return self.fail(e),
            };
        for wp in pieces {
            // (A turn in place: as long as the slowest axis takes.)
            let minutes = self.chain.least_minutes(&self.q, &wp.q, false).max(1e-7);
            let v = self.words(from, &wp.q);
            self.pending.push(Pending { v, minutes, feed });
            self.q = wp.q;
        }
    }

    /// A rotary axis has run out mid-cut: out along the tool's axis, round with the tool
    /// clear, and back in to `cl` at the axis values `q`.
    fn rewind(&mut self, w: &mut Writer, cl: &Cl, q: &[f64], feed: f64) {
        self.flush(w);
        self.rewinds += 1;
        w.comment("Rewind: the tool lifts clear while a rotary axis comes round");
        let Some(here) = self.cl else {
            return;
        };
        let lifted = |c: &Cl| Cl {
            tip: add(c.tip, scale(c.axis, self.retract)),
            axis: c.axis,
        };
        let (out, back) = (lifted(&here), lifted(cl));
        let (q_out, q_back) = match (
            self.chain.solve(&out, self.length, &self.q),
            self.chain.solve(&back, self.length, q),
        ) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return self.fail(e),
        };
        let v = self.words(&out, &q_out);
        let minutes = (self.retract / feed.max(1e-6)).max(1e-7);
        self.pending.push(Pending { v, minutes, feed });
        self.flush(w);
        let v = self.words(&back, &q_back);
        self.go_rapid(w, &v, false);
        let v = self.words(cl, q);
        self.pending.push(Pending { v, minutes, feed });
        self.flush(w);
    }

    /// Writes the feed moves waiting, those on one line as one block.
    pub fn flush(&mut self, w: &mut Writer) {
        let pending = std::mem::take(&mut self.pending);
        let mut i = 0;
        while i < pending.len() {
            let mut j = i;
            if let Some(base) = &self.last {
                // The furthest move the ones before it lie on the way to, at times in step:
                // tried twice as far each time, then narrowed down.
                let within = 0.5 * 10f64.powi(-(w.s.decimals.min(6) as i32));
                let fits = |to: usize| on_one_line(base, &pending[i..=to], within);
                let mut step = 1;
                loop {
                    let next = (j + step).min(pending.len() - 1);
                    if next == j {
                        break;
                    }
                    if fits(next) {
                        j = next;
                        step *= 2;
                    } else {
                        let (mut good, mut bad) = (j, next);
                        while bad - good > 1 {
                            let mid = (good + bad) / 2;
                            if fits(mid) {
                                good = mid;
                            } else {
                                bad = mid;
                            }
                        }
                        j = good;
                        break;
                    }
                }
            }
            let minutes: f64 = pending[i..=j].iter().map(|p| p.minutes).sum();
            self.feed_block(w, &pending[j].v, minutes, pending[j].feed);
            i = j + 1;
        }
    }

    fn feed_block(&mut self, w: &mut Writer, v: &[f64], minutes: f64, feed: f64) {
        let from = self.last.clone();
        let (lin, turn) = match &from {
            Some(f) => (
                dist([f[0], f[1], f[2]], [v[0], v[1], v[2]]),
                f[3..]
                    .iter()
                    .zip(&v[3..])
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f64>()
                    .sqrt(),
            ),
            None => (0.0, 0.0),
        };
        let inverse = !self.tcp && !self.fixed && self.feed_mode == RotaryFeed::InverseTime;
        if inverse != self.inverse {
            w.block(if inverse { "G93" } else { "G94" });
            self.inverse = inverse;
            w.feed = None;
        }
        let turn_words = self.rotary_words(w, v);
        let axes = w.axes([v[0], v[1], v[2]]);
        if axes.is_empty() && turn_words.is_empty() {
            return;
        }
        let m = w.motion(1) + &w.comp_words();
        let f = if inverse {
            // Written on every block: one over its minutes.
            let f = (1.0 / minutes.max(1e-9)).clamp(0.001, 99_999.0);
            format!(" F{}", decimal(f, 3, w.s.decimal_point))
        } else {
            let rate = if self.tcp {
                // The control feeds the tip; a block that only turns is in degrees a minute.
                if lin > 1e-6 {
                    feed
                } else {
                    turn / minutes.max(1e-9)
                }
            } else {
                match self.feed_mode {
                    RotaryFeed::AllAxes => lin.hypot(turn) / minutes.max(1e-9),
                    _ if lin > 1e-6 => lin / minutes.max(1e-9),
                    _ => turn / minutes.max(1e-9),
                }
            };
            w.feed_word(rate)
        };
        w.block(format!("{m}{axes}{turn_words}{f}").trim());
        self.last = Some(v.to_vec());
    }
}

/// Whether every move but the last lies on the straight way from `base` to the last, one
/// after another along it (within half of the last decimal a word is written to: a faceted
/// surface's hair's breadths don't break a helix into a block a facet).
fn on_one_line(base: &[f64], moves: &[Pending], within: f64) -> bool {
    let end = &moves[moves.len() - 1];
    if moves.iter().any(|m| (m.feed - end.feed).abs() > 1e-9) {
        return false;
    }
    let along: Vec<f64> = end.v.iter().zip(base).map(|(b, a)| b - a).collect();
    let l2: f64 = along.iter().map(|d| d * d).sum();
    if l2 <= 0.0 {
        return false;
    }
    let mut last = 0.0;
    for m in &moves[..moves.len() - 1] {
        // Where along the line it is, and how far off it.
        let f =
            m.v.iter()
                .zip(base)
                .zip(&along)
                .map(|((x, a), d)| (x - a) * d)
                .sum::<f64>()
                / l2;
        let off =
            m.v.iter()
                .zip(base.iter().zip(&along))
                .any(|(x, (a, d))| (x - (a + d * f)).abs() > within);
        if off || f < last - 1e-9 || f > 1.0 + 1e-9 {
            return false;
        }
        last = f;
    }
    true
}

/// The rotary axes back to zero with the tool up clear, before work in the work offset's own
/// axes or at the program's end.
pub(super) fn unturn(w: &mut Writer, chain: &Chain, d: Dialect) {
    let Ok((_, rot)) = words_of(chain) else {
        return;
    };
    if rot.is_empty() {
        return;
    }
    // (A wound axis was counted from near zero again as its operation ended: this is half a
    // turn at the most.)
    super::retract_home(w, d);
    let zero = decimal(0.0, 0, w.s.decimal_point);
    let words: Vec<String> = rot.iter().map(|(_, c)| format!("{c}{zero}")).collect();
    w.block(&format!("G0 {}", words.join(" ")));
    w.at = [None; 3];
    w.mode = None;
}

impl Tilt {
    /// The plane's frame in the setup's coordinates, rebuilt from its angles.
    pub fn frame(&self) -> Frame {
        let [a, b, c] = self.euler.map(f64::to_radians);
        let (sa, ca) = a.sin_cos();
        let (sb, cb) = b.sin_cos();
        let (sc, cc) = c.sin_cos();
        // R = Rz(a)·Rx(b)·Rz(c); its columns are the frame's axes.
        Frame {
            origin: self.origin,
            x: [ca * cc - sa * cb * sc, sa * cc + ca * cb * sc, sb * sc],
            y: [-ca * sc - sa * cb * cc, -sa * sc + ca * cb * cc, sb * cc],
            z: [sa * sb, -ca * sb, cb],
        }
    }
}
