//! Post processors: toolpaths into the G-code a machine's controller runs.
//!
//! [`PostSettings`] are the machine's post options (as the Create machine dialog's Post
//! settings): output unit, datums, sequence numbers, rapid positioning, arcs and helices,
//! decimals, forced retracts and canned cycles. A [`Dialect`] is the controller family: Fanuc,
//! Haas-style, LinuxCNC, GRBL and Mach3. Beam machines differ in how the beam switches:
//!
//! - **Laser (GRBL)**: `M4` dynamic power (`M3` constant when chosen) with the power as `S`;
//!   `$32` laser mode makes GRBL stop the beam on rapids.
//! - **Plasma, LinuxCNC (QtPlasmaC)**: QtPlasmaC runs the torch's heights and the pierce
//!   delay from its material table and strips Z from programs, so the program has no Z moves
//!   and no pierce dwell (the torch's own values are written as a comment). `M190 P…` then
//!   `M66 P3 L3 Q1` select the torch's material when it has a number; `M3 $0 S1` and `M5 $0`
//!   fire and stop it; a torch without height control turns QtPlasmaC's off with `M62 P2`
//!   (synchronised with motion) and on again after with `M63 P2`.
//! - **Plasma, Mach3**: `M3`/`M5` fire the torch, the pierce is a dwell at the pierce height,
//!   then the torch drops to its cut height; the screen's THC does the rest.
//! - **Waterjet**: `M3`/`M5` open and close the jet, with the pierce dwell.
//!
//! [`post`] writes a whole program: header comments, units, the work offset, each operation
//! (tool change, spindle, coolant, its moves), and the end.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::geom::{P2, P3, dist};
use crate::machine::{Machine, MachineKind};
use crate::tool::{Hand, Tool, Unit};
use crate::toolpath::{Coolant, CycleKind, DrillCycle, Step, Toolpath};

/// The controller family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dialect {
    Fanuc,
    Haas,
    LinuxCnc,
    Grbl,
    Mach3,
}

impl Dialect {
    pub const ALL: [Dialect; 5] = [
        Dialect::Fanuc,
        Dialect::Haas,
        Dialect::LinuxCnc,
        Dialect::Grbl,
        Dialect::Mach3,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Dialect::Fanuc => "Fanuc",
            Dialect::Haas => "Haas-style",
            Dialect::LinuxCnc => "LinuxCNC",
            Dialect::Grbl => "GRBL",
            Dialect::Mach3 => "Mach3/Mach4",
        }
    }

    /// The file extension programs for it usually have.
    pub fn extension(self) -> &'static str {
        match self {
            Dialect::Fanuc | Dialect::Haas => "nc",
            Dialect::LinuxCnc => "ngc",
            Dialect::Grbl => "gcode",
            Dialect::Mach3 => "tap",
        }
    }

    /// Supports canned drilling cycles.
    pub fn has_cycles(self) -> bool {
        !matches!(self, Dialect::Grbl)
    }

    /// Wraps the program in `%` and numbers it `O1001`.
    fn program_number(self) -> bool {
        matches!(self, Dialect::Fanuc | Dialect::Haas)
    }

    fn comment(self, text: &str) -> String {
        // Parentheses can't nest in a comment, and controls alarm on characters outside ASCII.
        let text: String = text
            .chars()
            .filter_map(|c| match c {
                '(' | ')' => None,
                '\u{2014}' | '\u{2013}' => Some('-'),
                '\u{00d7}' => Some('x'),
                '\u{00b0}' => Some('d'),
                '\u{00d8}' | '\u{2300}' => Some('D'),
                c if c.is_ascii() && !c.is_ascii_control() => Some(c),
                _ => Some('?'),
            })
            .collect();
        format!("({text})")
    }

    /// A canned cycle's dwell `P` word for `sec` seconds: milliseconds on Fanuc-style controls
    /// (no point), seconds on LinuxCNC and Mach3.
    fn cycle_dwell(self, sec: f64) -> String {
        match self {
            Dialect::Fanuc | Dialect::Haas => format!("{}", (sec * 1000.0).round()),
            _ => decimal(sec, 3, false),
        }
    }

    /// A dwell of `sec` seconds.
    fn dwell(self, sec: f64) -> String {
        match self {
            // Fanuc's P is in milliseconds; X takes seconds with a point.
            Dialect::Fanuc => format!("G4 X{}", decimal(sec, 3, true)),
            // Haas: P with a point is seconds.
            Dialect::Haas => format!("G4 P{}", decimal(sec, 3, true)),
            _ => format!("G4 P{}", decimal(sec, 3, false)),
        }
    }
}

/// How a laser's power follows its speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LaserMode {
    /// `M4`: the power scales with the actual speed (cleaner corners).
    #[default]
    Dynamic,
    /// `M3`: the power stays constant.
    Constant,
}

/// A machine's post options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostSettings {
    /// Program: the unit the NC code is written in, whatever the job's unit.
    pub output_unit: Unit,
    /// Datums: allow work planes with their own datum (G54.1 P…/G55…); off writes every
    /// operation in the setup's datum.
    pub advanced_work_planes: bool,
    /// NC general: line numbers.
    pub sequence_numbers: bool,
    pub block_start: u32,
    pub block_increment: u32,
    /// Line numbers only at tool changes.
    pub numbers_at_tools_only: bool,
    /// Movement: rapids as G0 (off: G1 at `rapid_rate`).
    pub rapid_positioning: bool,
    /// The feed rapids get without rapid positioning (mm/min).
    pub rapid_rate: f64,
    /// Arcs as G2/G3 (off: short lines).
    pub output_arcs: bool,
    /// Arcs fitted in the XY plane (also from lines that follow a circle).
    pub arcs_xy: bool,
    /// Helical arcs (an arc that changes Z).
    pub output_helices: bool,
    /// Decimals of coordinates.
    pub decimals: u32,
    /// Always write a decimal point (`X30.`, not `X30`): a Fanuc-style control without
    /// decimal point programming reads `X30` as 30 least increments (0.030 mm).
    #[serde(default)]
    pub decimal_point: bool,
    /// Retracts: to the machine's Z home (G28/G53) at each tool change.
    pub force_retract_on_tool_change: bool,
    /// At each work plane change.
    pub force_retract_on_plane_change: bool,
    /// At the program's end.
    pub force_retract_on_end: bool,
    /// Fixed cycles: canned cycles for drilling (G81/G82), peck drilling (G83), tapping (G84)
    /// and chip breaking (G73). A cycle that's off is written as moves.
    pub canned_drill: bool,
    pub canned_peck: bool,
    pub canned_tap: bool,
    pub canned_chip_break: bool,
    /// Write the operations' names and the tools as comments.
    pub comments: bool,
    /// Laser power: dynamic (M4) or constant (M3).
    pub laser_mode: LaserMode,
}

impl PostSettings {
    /// The defaults for a controller and machine kind.
    pub fn for_dialect(d: Dialect, kind: MachineKind) -> PostSettings {
        let cycles = d.has_cycles() && kind.is_milling();
        PostSettings {
            output_unit: Unit::Mm,
            advanced_work_planes: matches!(d, Dialect::Fanuc | Dialect::Haas),
            sequence_numbers: matches!(d, Dialect::Fanuc | Dialect::Haas),
            block_start: 10,
            block_increment: 5,
            numbers_at_tools_only: false,
            rapid_positioning: true,
            rapid_rate: 5_000.0,
            output_arcs: true,
            arcs_xy: true,
            output_helices: d != Dialect::Mach3,
            decimals: if d == Dialect::Grbl { 3 } else { 4 },
            decimal_point: matches!(d, Dialect::Fanuc | Dialect::Haas),
            force_retract_on_tool_change: kind.is_milling() && d != Dialect::Grbl,
            force_retract_on_plane_change: false,
            force_retract_on_end: kind.is_milling() && d != Dialect::Grbl,
            canned_drill: cycles,
            canned_peck: cycles,
            canned_tap: cycles,
            canned_chip_break: cycles,
            comments: true,
            laser_mode: LaserMode::Dynamic,
        }
    }

    fn canned(&self, k: CycleKind) -> bool {
        match k {
            CycleKind::Drill => self.canned_drill,
            CycleKind::Peck => self.canned_peck,
            CycleKind::Tap => self.canned_tap,
            CycleKind::ChipBreak => self.canned_chip_break,
        }
    }
}

/// One operation of a program.
#[derive(Debug, Clone, PartialEq)]
pub struct NcOperation {
    pub name: String,
    pub tool: Tool,
    /// The tool number (T) the job gave it.
    pub tool_number: u32,
    pub toolpath: Toolpath,
    pub coolant: Coolant,
    /// The work offset it's written in: 1 = G54, 2 = G55, … (7+: G54.1 P1…).
    pub datum: u32,
    /// Pause for the operator before it ("M0").
    pub stop_before: bool,
    /// Make the tool change even when the same tool is in.
    pub force_tool_change: bool,
    /// Cut in a tilted working plane (3+2): its moves are in that plane's coordinates.
    pub tilt: Option<Tilt>,
}

/// A tilted working plane (3+2): where an operation's coordinates are, in the setup's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tilt {
    /// The plane's origin in the setup's coordinates.
    pub origin: P3,
    /// Its orientation as Euler angles (degrees): about Z, then the new X, then the new Z,
    /// as `G68.2 … I J K` takes them.
    pub euler: [f64; 3],
    /// Its Z axis in the setup's coordinates (the tool's axis).
    pub z: P3,
}

impl Tilt {
    /// The tilt of a frame given in the setup's coordinates.
    pub fn of(f: &crate::Frame) -> Tilt {
        // Columns of the rotation: the frame's axes. R = Rz(a) Rx(b) Rz(c).
        let r = |i: usize, j: usize| [f.x, f.y, f.z][j][i];
        let b = r(2, 2).clamp(-1.0, 1.0).acos();
        let (a, c) = if b.sin().abs() > 1e-9 {
            (r(0, 2).atan2(-r(1, 2)), r(2, 0).atan2(r(2, 1)))
        } else if r(2, 2) > 0.0 {
            // About Z only: a + c.
            (0.0, r(1, 0).atan2(r(0, 0)))
        } else {
            (0.0, (-r(0, 1)).atan2(r(0, 0)))
        };
        Tilt {
            origin: f.origin,
            euler: [a.to_degrees(), b.to_degrees(), c.to_degrees()],
            z: f.z,
        }
    }
}

/// What to post.
#[derive(Debug, Clone, PartialEq)]
pub struct NcProgram {
    pub name: String,
    pub number: u32,
    /// The work offsets it uses (listed in its header for the operator to set).
    pub offsets: Vec<WorkOffset>,
    pub operations: Vec<NcOperation>,
}

/// A work offset a program uses and where its origin is.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkOffset {
    /// 1 = G54, 2 = G55, …
    pub number: u32,
    /// What its origin is ("Setup 1's work coordinate system", "Work plane 1").
    pub origin: String,
    /// Its origin from the first one's (mm, the first one's axes).
    pub at: P3,
    /// The operations cut in it.
    pub operations: Vec<String>,
}

/// Why a program couldn't be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostError {
    Empty,
    /// An operation's tool doesn't fit the machine (a laser on a mill).
    WrongTool(String),
    /// An operation tilts the tool where the machine can't.
    CantTilt(String),
    /// A work offset the control doesn't have.
    NoDatum(String),
    /// Cutter compensation on a control without it.
    NoCompensation(String),
}

impl std::fmt::Display for PostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PostError::Empty => write!(f, "There is nothing to post"),
            PostError::WrongTool(s) => write!(f, "{s}"),
            PostError::CantTilt(s) => write!(f, "{s}"),
            PostError::NoDatum(s) => write!(f, "{s}"),
            PostError::NoCompensation(s) => write!(f, "{s}"),
        }
    }
}

/// The datum code for a work offset number.
pub fn datum_code(n: u32) -> String {
    match n {
        0 | 1 => "G54".into(),
        2..=6 => format!("G{}", 53 + n),
        n => format!("G54.1 P{}", n - 6),
    }
}

impl Dialect {
    /// Its code for work offset `n` (1 = G54 … 6 = G59, then the extended ones), or `None`
    /// past what it has.
    pub fn datum(self, n: u32) -> Option<String> {
        let n = n.max(1);
        if n <= 6 {
            return Some(datum_code(n));
        }
        let e = n - 6;
        match self {
            Dialect::Fanuc if e <= 48 => Some(format!("G54.1 P{e}")),
            Dialect::Haas if e <= 99 => Some(format!("G154 P{e}")),
            Dialect::LinuxCnc if e <= 3 => Some(format!("G59.{e}")),
            Dialect::Mach3 if n <= 254 => Some(format!("G59 P{n}")),
            _ => None,
        }
    }
}

/// `v` with at most `decimals` decimals and no trailing zeros; with `point`, integers keep
/// their point (`30.`).
fn decimal(v: f64, decimals: u32, point: bool) -> String {
    let s = format!("{:.*}", decimals.min(6) as usize, v);
    let mut s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    if s == "-0" {
        s = "0".into();
    }
    if point && !s.contains('.') {
        s.push('.');
    }
    s
}

struct Writer<'a> {
    s: &'a PostSettings,
    d: Dialect,
    out: String,
    line: u32,
    /// The scale from mm to the output unit.
    k: f64,
    /// The last position written (mm), for modal output.
    at: [Option<f64>; 3],
    feed: Option<f64>,
    /// The motion mode last written (0, 1, 2, 3).
    mode: Option<u8>,
    /// Number the next line even with numbers at tools only.
    number_next: bool,
    /// Leave Z out of moves (the control runs the torch's heights).
    no_z: bool,
    /// The beam is on.
    beam_on: bool,
    /// The spindle speed last set (rpm).
    rpm: Option<f64>,
    /// Cutter compensation words (G41 D1, G40) for the next straight move.
    comp: Option<String>,
    /// The operator has been told what the compensation register holds.
    comp_noted: bool,
}

impl<'a> Writer<'a> {
    fn new(s: &'a PostSettings, d: Dialect) -> Self {
        Writer {
            s,
            d,
            out: String::new(),
            line: s.block_start,
            k: 1.0 / s.output_unit.mm(),
            at: [None; 3],
            feed: None,
            mode: None,
            number_next: false,
            no_z: false,
            beam_on: false,
            rpm: None,
            comp: None,
            comp_noted: false,
        }
    }

    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn block(&mut self, text: &str) {
        let numbered =
            self.s.sequence_numbers && (!self.s.numbers_at_tools_only || self.number_next);
        if numbered {
            // Controls read five-digit N words: past N99999 the numbers start again.
            if self.line > 99_999 {
                self.line = self.s.block_start.min(99_999);
            }
            let n = self.line;
            self.line += self.s.block_increment.max(1);
            let _ = writeln!(self.out, "N{n} {text}");
        } else {
            self.raw(text);
        }
        self.number_next = false;
    }

    fn comment(&mut self, text: &str) {
        if self.s.comments {
            let c = self.d.comment(text);
            self.raw(&c);
        }
    }

    fn num(&self, v: f64) -> String {
        decimal(v * self.k, self.s.decimals, self.s.decimal_point)
    }

    fn feed_word(&mut self, f: f64) -> String {
        if self.feed.is_some_and(|x| (x - f).abs() < 1e-9) {
            String::new()
        } else {
            self.feed = Some(f);
            let d = if self.s.output_unit == Unit::Inch {
                2
            } else {
                0
            };
            let v = f * self.k;
            format!(
                " F{}",
                if d == 0 {
                    format!("{}", v.round())
                } else {
                    format!("{v:.2}")
                }
            )
        }
    }

    /// The axis words of a move to `p` (only the axes that change).
    fn axes(&mut self, p: P3) -> String {
        let mut w = String::new();
        let n = if self.no_z { 2 } else { 3 };
        for (i, a) in ["X", "Y", "Z"].iter().enumerate().take(n) {
            let v = self.num(p[i]);
            let old = self.at[i].map(|o| self.num(o));
            if old.as_deref() != Some(v.as_str()) {
                let _ = write!(w, " {a}{v}");
            }
            self.at[i] = Some(p[i]);
        }
        w
    }

    fn motion(&mut self, mode: u8) -> String {
        if self.mode == Some(mode) && mode <= 1 {
            String::new()
        } else {
            self.mode = Some(mode);
            format!("G{mode}")
        }
    }

    fn rapid(&mut self, p: P3) {
        let axes = self.axes(p);
        if axes.is_empty() {
            return;
        }
        if self.s.rapid_positioning {
            let m = self.motion(0) + &self.comp_words();
            let line = format!("{m}{axes}").trim().to_string();
            self.block(&line);
        } else {
            let m = self.motion(1) + &self.comp_words();
            let f = self.feed_word(self.s.rapid_rate);
            let line = format!("{m}{axes}{f}").trim().to_string();
            self.block(&line);
        }
    }

    fn linear(&mut self, p: P3, feed: f64) {
        let axes = self.axes(p);
        if axes.is_empty() {
            return;
        }
        let m = self.motion(1) + &self.comp_words();
        let f = self.feed_word(feed);
        let line = format!("{m}{axes}{f}").trim().to_string();
        self.block(&line);
    }

    /// The pending compensation words, once (on the straight move they start or end on).
    fn comp_words(&mut self) -> String {
        self.comp.take().map_or(String::new(), |c| format!(" {c}"))
    }

    fn arc(&mut self, from: P3, to: P3, center: P2, cw: bool, feed: f64) {
        // Compensation never starts or ends on an arc: on its own line first.
        if let Some(c) = self.comp.take() {
            self.block(&c);
        }
        let mode = if cw { 2 } else { 3 };
        self.mode = None;
        let i = self.num(center[0] - from[0]);
        let j = self.num(center[1] - from[1]);
        let mut axes = String::new();
        for (k, a) in ["X", "Y", "Z"].iter().enumerate() {
            // An arc writes X and Y always (a full circle ends where it starts).
            if k < 2 || (!self.no_z && self.at[k].map(|o| self.num(o)) != Some(self.num(to[k]))) {
                let _ = write!(axes, " {a}{}", self.num(to[k]));
            }
            self.at[k] = Some(to[k]);
        }
        let f = self.feed_word(feed);
        self.block(&format!("G{mode}{axes} I{i} J{j}{f}"));
        self.mode = Some(mode);
    }
}

/// The program for `machine`, or why it can't be written.
pub fn post(program: &NcProgram, machine: &Machine, s: &PostSettings) -> Result<String, PostError> {
    if program.operations.iter().all(|o| o.toolpath.is_empty()) {
        return Err(PostError::Empty);
    }
    for op in &program.operations {
        if !op.tool.kind.fits(machine.kind) {
            return Err(PostError::WrongTool(format!(
                "{}: a {} can't run on a {}",
                op.name,
                op.tool.kind.label().to_lowercase(),
                machine.kind.label().to_lowercase()
            )));
        }
    }
    let d = machine.dialect;
    for op in &program.operations {
        let compensated = op
            .toolpath
            .steps
            .iter()
            .any(|s| matches!(s, Step::Compensation(Some(_))));
        if compensated && d == Dialect::Grbl {
            return Err(PostError::NoCompensation(format!(
                "{}: GRBL has no cutter compensation; set the profile's compensation to \
                 In computer",
                op.name
            )));
        }
        if let Some(t) = op.tilt {
            if d != Dialect::Fanuc {
                return Err(PostError::CantTilt(format!(
                    "{}: tilted work planes are posted as Fanuc's tilted working plane \
                     (G68.2) only",
                    op.name
                )));
            }
            if !machine.can_tilt_to(t.z) {
                return Err(PostError::CantTilt(format!(
                    "{}: the work plane tilts the tool further than the machine can",
                    op.name
                )));
            }
        }
    }
    let mut w = Writer::new(s, d);
    if d.program_number() {
        w.raw("%");
        let name = program.name.to_uppercase().replace(['(', ')'], "");
        w.raw(&format!("O{:04} ({name})", program.number.min(9999)));
    }
    w.comment(&format!("{} for {}", program.name, machine.title()));
    let mut listed: Vec<(u32, &str)> = Vec::new();
    for op in &program.operations {
        if !listed.contains(&(op.tool_number, op.tool.name.as_str())) {
            listed.push((op.tool_number, &op.tool.name));
            w.comment(&format!("T{} {}", op.tool_number, op.tool.name));
        }
    }
    // Every work offset beyond the first, and where to set it (an offset left at machine zero
    // crashes the tool).
    if program.offsets.len() > 1 {
        for (k, o) in program.offsets.iter().enumerate() {
            let Some(code) = d.datum(o.number) else {
                continue;
            };
            let at = if k == 0 {
                String::new()
            } else {
                let first = program
                    .offsets
                    .first()
                    .and_then(|f| d.datum(f.number))
                    .unwrap_or_default();
                format!(
                    " at X{} Y{} Z{} from {first}",
                    w.num(o.at[0]),
                    w.num(o.at[1]),
                    w.num(o.at[2])
                )
            };
            let ops = if o.operations.is_empty() {
                String::new()
            } else {
                format!(": {}", o.operations.join(", "))
            };
            w.comment(&format!("{code}: {}{at}{ops}", o.origin));
        }
    }
    let unit = match s.output_unit {
        Unit::Mm => "G21",
        Unit::Inch => "G20",
    };
    let plane = if machine.kind.is_milling() {
        " G17"
    } else {
        ""
    };
    match d {
        Dialect::Grbl => w.block(&format!("G90 G94{plane} {unit}")),
        // QtPlasmaC's own preamble (its path blending tolerance).
        Dialect::LinuxCnc if qtplasmac(machine) => w.block(&format!(
            "{unit} G40 G49 G64 P{} G80 G90 G92.1 G94 G97",
            if s.output_unit == Unit::Inch {
                "0.004"
            } else {
                "0.1"
            }
        )),
        Dialect::LinuxCnc => w.block(&format!("G90 G94{plane} G40 G49 {unit} G64 P0.01")),
        _ => w.block(&format!("G90 G94{plane} G40 G49 G80 {unit}")),
    }
    if qtplasmac(machine) {
        w.comment("QtPlasmaC: torch heights and pierce delay from the material table");
        w.no_z = true;
    }
    let mut tool_in: Option<u32> = None;
    let mut tilted: Option<Tilt> = None;
    let mut datum: Option<u32> = None;
    let mut coolant = Coolant::Off;
    let beam = machine.kind.is_cutting();
    for op in &program.operations {
        if op.toolpath.is_empty() {
            continue;
        }
        w.comment(&op.name);
        let changing =
            machine.kind.is_milling() && (tool_in != Some(op.tool_number) || op.force_tool_change);
        // Out of a tilted plane that isn't this one's, or for a tool change: up clear, cancel
        // it, the rotary axes home.
        if tilted.is_some() && (tilted != op.tilt || changing) {
            cancel_tilt(&mut w, machine);
            tilted = None;
        }
        // A stop for the operator: the spindle and coolant off first (M0 doesn't stop them on
        // every control), both written again as the operation starts.
        if op.stop_before {
            if !beam && tool_in.is_some() {
                w.block("M5");
                if coolant != Coolant::Off {
                    w.block("M9");
                    coolant = Coolant::Off;
                }
            }
            w.block("M0");
        }
        // Tool change (its length comes in with the first move, after the work offset).
        let mut length_comp = false;
        if changing {
            if tool_in.is_some() {
                w.block("M5");
                if coolant != Coolant::Off {
                    w.block("M9");
                    coolant = Coolant::Off;
                }
            }
            if s.force_retract_on_tool_change {
                retract_home(&mut w, d);
            }
            w.number_next = true;
            // (GRBL has no M6: a manual change whatever the machine says.)
            if machine.tool_changer && d != Dialect::Grbl {
                w.block(&format!("T{} M6", op.tool_number));
            } else {
                w.comment(&format!("Change to T{}: {}", op.tool_number, op.tool.name));
                match d {
                    Dialect::Grbl => w.block("M0"),
                    _ => w.block(&format!("T{} M6", op.tool_number)),
                }
            }
            length_comp = d != Dialect::Grbl || machine.kind.is_milling();
            tool_in = Some(op.tool_number);
            w.at = [None; 3];
            w.mode = None;
        }
        // Work offset.
        let want = if s.advanced_work_planes {
            op.datum.max(1)
        } else {
            1
        };
        // A new work offset: up to the first height before across, as after a tool change.
        let mut z_first = false;
        if datum != Some(want) {
            if datum.is_some() && s.force_retract_on_plane_change {
                retract_home(&mut w, d);
            }
            z_first = machine.kind.is_milling();
            let code = d.datum(want).ok_or_else(|| {
                PostError::NoDatum(format!(
                    "{}: {} has no work offset {want} (G54 is 1)",
                    op.name,
                    d.label()
                ))
            })?;
            w.block(&code);
            datum = Some(want);
            w.at = [None; 3];
        }
        // Into the tilted plane: up clear first, the plane, then the rotary axes to it.
        if let Some(t) = op.tilt
            && tilted.is_none()
        {
            retract_home(&mut w, d);
            let line = format!(
                "G68.2 X{} Y{} Z{} I{} J{} K{}",
                w.num(t.origin[0]),
                w.num(t.origin[1]),
                w.num(t.origin[2]),
                decimal(t.euler[0], 4, s.decimal_point),
                decimal(t.euler[1], 4, s.decimal_point),
                decimal(t.euler[2], 4, s.decimal_point)
            );
            w.block(&line);
            w.block("G53.1");
            // The tool's length again, along the tilted axis.
            length_comp = machine.kind.is_milling();
            tilted = Some(t);
            w.at = [None; 3];
            w.mode = None;
        }
        // The tool's length on with a move to the operation's first height, after the work
        // offset (and the tilted plane): `G0 G43 H1 Z…`.
        if length_comp {
            let z = op.toolpath.steps.iter().find_map(|st| match st {
                Step::Rapid(p) => Some(p[2]),
                _ => None,
            });
            match z {
                // GRBL has no tool table: still up to the first height on its own first.
                Some(z) if d == Dialect::Grbl => {
                    let line = format!("G0 Z{}", w.num(z));
                    w.block(&line);
                    w.at[2] = Some(z);
                    w.mode = Some(0);
                }
                None if d == Dialect::Grbl => {}
                Some(z) => {
                    let line = format!("G0 G43 H{} Z{}", op.tool_number, w.num(z));
                    w.block(&line);
                    w.at[2] = Some(z);
                    w.mode = Some(0);
                }
                None => w.block(&format!("G43 H{}", op.tool_number)),
            }
        } else if z_first
            && let Some(z) = op.toolpath.steps.iter().find_map(|st| match st {
                Step::Rapid(p) => Some(p[2]),
                _ => None,
            })
        {
            let line = format!("G0 Z{}", w.num(z));
            w.block(&line);
            w.at[2] = Some(z);
            w.mode = Some(0);
        }
        // On from where the last operation left the tool: up to the higher of there and this
        // one's first height, across, then down (a control's G0 isn't a straight line, and the
        // way across must clear what's between them).
        let first = op.toolpath.steps.iter().find_map(|st| match st {
            Step::Rapid(p) => Some(*p),
            Step::Feed { .. } | Step::Arc { .. } | Step::Drill(_) => Some([f64::NAN; 3]),
            _ => None,
        });
        if machine.kind.is_milling()
            && let (Some(f), [Some(x), Some(y), Some(z)]) = (first, w.at)
            && !f[0].is_nan()
            && ((f[0] - x).abs() > 1e-9 || (f[1] - y).abs() > 1e-9)
        {
            let h = z.max(f[2]);
            w.rapid([x, y, h]);
            w.rapid([f[0], f[1], h]);
        }
        post_moves(&mut w, op, machine, s, beam, &mut coolant);
    }
    // End.
    if tilted.is_some() {
        cancel_tilt(&mut w, machine);
    }
    if beam {
        // The beam off if an operation left it on.
        if w.beam_on {
            w.block(if qtplasmac(machine) { "M5 $0" } else { "M5" });
        }
    } else {
        w.block("M5");
        if coolant != Coolant::Off {
            w.block("M9");
        }
        if s.force_retract_on_end {
            retract_home(&mut w, d);
        }
    }
    w.block(if d == Dialect::Grbl { "M2" } else { "M30" });
    if d.program_number() {
        w.raw("%");
    }
    Ok(w.out)
}

/// Out of a tilted working plane: up clear, cancel it, the rotary axes back to zero (G69 leaves
/// them where they are).
fn cancel_tilt(w: &mut Writer, machine: &Machine) {
    retract_home(w, machine.dialect);
    w.block("G69");
    let [a, b] = machine.indexing.map_or(['A', 'C'], |i| i.rotary);
    let zero = decimal(0.0, 0, w.s.decimal_point);
    w.block(&format!("G0 {a}{zero} {b}{zero}"));
    w.at = [None; 3];
    w.mode = None;
}

/// A LinuxCNC plasma table: QtPlasmaC.
fn qtplasmac(machine: &Machine) -> bool {
    machine.kind == MachineKind::Plasma && machine.dialect == Dialect::LinuxCnc
}

fn retract_home(w: &mut Writer, d: Dialect) {
    match d {
        Dialect::Fanuc | Dialect::Haas => w.block("G91 G28 Z0."),
        Dialect::LinuxCnc | Dialect::Mach3 => w.block("G53 G0 Z0"),
        // GRBL's machine Z is negative down from home: just under it.
        Dialect::Grbl => {
            let z = w.num(-1.0);
            w.block(&format!("G53 G0 Z{z}"))
        }
    }
    if matches!(d, Dialect::Fanuc | Dialect::Haas) {
        w.block("G90");
    }
    w.at[2] = None;
    w.mode = None;
}

/// What the operator sets for wear compensation: the path is already the tool's centre, so the
/// control's register holds the wear only.
pub fn compensation_note(d: Dialect, tool: u32) -> String {
    match d {
        Dialect::LinuxCnc | Dialect::Mach3 => format!(
            "Wear compensation: the tool table diameter of T{tool} holds twice the wear only \
             (0 to start), not the tool's diameter"
        ),
        _ => format!("Wear compensation: D{tool} holds the wear only (0 to start), not the radius"),
    }
}

fn spindle_word(rpm: f64, cw: bool) -> String {
    format!("S{} {}", rpm.round(), if cw { "M3" } else { "M4" })
}

fn coolant_code(c: Coolant) -> Option<&'static str> {
    match c {
        Coolant::Off => None,
        Coolant::Flood => Some("M8"),
        Coolant::Mist => Some("M7"),
        Coolant::Air => Some("M7"),
    }
}

fn post_moves(
    w: &mut Writer,
    op: &NcOperation,
    machine: &Machine,
    s: &PostSettings,
    beam: bool,
    coolant: &mut Coolant,
) {
    let d = machine.dialect;
    let tool = &op.tool;
    let mut at: P3 = [f64::NAN; 3];
    // (Told once for each operation that compensates: its tool's register.)
    w.comp_noted = false;
    let max_s = machine.max_spindle.max(1.0);
    // Lines that follow a circle become arcs (when arcs are on); collect feed runs first.
    let steps = if s.output_arcs && s.arcs_xy {
        fit_arcs(&op.toolpath.steps, 0.005)
    } else {
        op.toolpath.steps.clone()
    };
    let qtp = qtplasmac(machine);
    if qtp {
        let b = &tool.beam;
        w.comment(&format!(
            "Torch: pierce {} s at {} mm, cut at {} mm",
            decimal(b.pierce_delay, 2, false),
            decimal(b.pierce_height, 2, false),
            decimal(b.cut_height, 2, false)
        ));
        if b.material > 0 {
            w.block(&format!("M190 P{}", b.material));
            w.block("M66 P3 L3 Q1");
        } else {
            w.comment("No material number: the cut uses the material loaded in QtPlasmaC");
        }
        if !b.thc {
            w.block("M62 P2");
        }
    }
    for st in &steps {
        match st {
            Step::Comment(c) => w.comment(c),
            Step::Spindle { rpm, cw } => {
                if !beam {
                    let cw = *cw == (tool.hand == Hand::Right);
                    w.rpm = Some(rpm.min(machine.max_spindle));
                    let word = spindle_word(rpm.min(machine.max_spindle), cw);
                    w.block(&word);
                    if op.coolant != *coolant {
                        if let Some(c) = coolant_code(op.coolant) {
                            w.block(c);
                        } else {
                            w.block("M9");
                        }
                        *coolant = op.coolant;
                    }
                }
            }
            Step::SpindleOff => {
                if !beam {
                    w.block("M5");
                }
            }
            Step::Coolant(c) => {
                if c != coolant {
                    match coolant_code(*c) {
                        Some(code) => w.block(code),
                        None => w.block("M9"),
                    }
                    *coolant = *c;
                }
            }
            Step::BeamOn { power } => {
                w.beam_on = true;
                match machine.kind {
                    MachineKind::Laser => {
                        let m = match s.laser_mode {
                            LaserMode::Dynamic => "M4",
                            LaserMode::Constant => "M3",
                        };
                        let sv = (power.clamp(0.0, 1.0) * max_s).round();
                        w.block(&format!("{m} S{sv}"));
                        if tool.beam.air_assist {
                            w.block("M8");
                        }
                    }
                    _ => {
                        if d == Dialect::LinuxCnc && machine.kind == MachineKind::Plasma {
                            w.block("M3 $0 S1");
                        } else {
                            w.block("M3");
                        }
                    }
                }
            }
            Step::BeamOff => {
                w.beam_on = false;
                if machine.kind == MachineKind::Plasma && d == Dialect::LinuxCnc {
                    w.block("M5 $0");
                } else {
                    w.block("M5");
                }
                if machine.kind == MachineKind::Laser && tool.beam.air_assist {
                    w.block("M9");
                }
            }
            // QtPlasmaC pierces for the material's delay itself.
            Step::Dwell(_) if qtp => {}
            Step::Dwell(sec) => w.block(&d.dwell(*sec)),
            Step::Rapid(p) => {
                // A laser in GRBL laser mode turns off on G0 itself.
                w.rapid(*p);
                at = *p;
            }
            Step::Feed { to, feed, .. } => {
                w.linear(*to, feed.min(machine.max_feed));
                at = *to;
            }
            Step::Arc {
                to,
                center,
                cw,
                feed,
                ..
            } => {
                let f = feed.min(machine.max_feed);
                let helix = (to[2] - at[2]).abs() > 1e-9;
                let r_ok = !at[0].is_nan();
                if s.output_arcs && s.arcs_xy && r_ok && (!helix || s.output_helices) {
                    if dist(at, *to) < 1e-9 && !helix {
                        // A full circle: two halves (some controls refuse one full circle).
                        let mid = [2.0 * center[0] - at[0], 2.0 * center[1] - at[1], at[2]];
                        w.arc(at, mid, *center, *cw, f);
                        w.arc(mid, *to, *center, *cw, f);
                    } else {
                        w.arc(at, *to, *center, *cw, f);
                    }
                } else {
                    let start = if r_ok { at } else { *to };
                    for p in crate::toolpath::arc_points(start, *to, *center, *cw, 0.005) {
                        w.linear(p, f);
                    }
                }
                at = *to;
            }
            Step::Drill(c) => {
                post_drill(w, c, s, d, tool, &mut at);
            }
            Step::Compensation(side) => {
                if side.is_some() && !w.comp_noted {
                    w.comment(&compensation_note(d, op.tool_number));
                    w.comp_noted = true;
                }
                w.comp = Some(match side {
                    Some(true) => format!("G41 D{}", op.tool_number),
                    Some(false) => format!("G42 D{}", op.tool_number),
                    None => "G40".to_string(),
                });
                // With the motion code written again on the move that takes it.
                w.mode = None;
            }
        }
    }
    // Never left on past the operation.
    if let Some(c) = w.comp.take()
        && c == "G40"
    {
        w.block("G40");
    }
    if qtp && !tool.beam.thc {
        w.block("M63 P2");
    }
}

fn post_drill(
    w: &mut Writer,
    c: &DrillCycle,
    s: &PostSettings,
    d: Dialect,
    tool: &Tool,
    at: &mut P3,
) {
    if c.kind == CycleKind::Tap && s.canned_tap && d == Dialect::LinuxCnc {
        // LinuxCNC taps with spindle-synchronised motion: in at the pitch a turn, then back
        // out the same way with the spindle reversed, to where it started.
        if !at[0].is_nan() && at[2] < c.retract {
            w.rapid([at[0], at[1], c.retract]);
        }
        w.rapid([c.at[0], c.at[1], c.retract]);
        w.rapid([c.at[0], c.at[1], c.r_plane]);
        let line = format!("G33.1 Z{} K{}", w.num(c.bottom), w.num(c.pitch));
        w.block(&line);
        w.mode = None;
        w.rapid([c.at[0], c.at[1], c.retract]);
        *at = [c.at[0], c.at[1], c.retract];
        return;
    }
    if s.canned(c.kind) && d.has_cycles() {
        let code = match c.kind {
            CycleKind::Drill if c.dwell > 0.0 => "G82",
            CycleKind::Drill => "G81",
            CycleKind::Peck => "G83",
            CycleKind::ChipBreak => "G73",
            CycleKind::Tap => {
                if tool.hand == Hand::Left {
                    "G74"
                } else {
                    "G84"
                }
            }
        };
        // Over the hole at the retract height first.
        if !at[0].is_nan() && at[2] < c.retract {
            w.rapid([at[0], at[1], c.retract]);
        }
        w.rapid([c.at[0], c.at[1], c.retract]);
        let mut words = format!(
            "G98 {code} X{} Y{} Z{} R{}",
            w.num(c.at[0]),
            w.num(c.at[1]),
            w.num(c.bottom),
            w.num(c.r_plane)
        );
        if matches!(c.kind, CycleKind::Peck | CycleKind::ChipBreak) && c.peck > 0.0 {
            let _ = write!(words, " Q{}", w.num(c.peck));
        }
        if c.dwell > 0.0 {
            let _ = write!(words, " P{}", d.cycle_dwell(c.dwell));
        }
        // Fanuc rigid tapping: the spindle synchronised first (Haas taps rigidly with G84
        // alone).
        if c.kind == CycleKind::Tap && d == Dialect::Fanuc {
            let rpm = w.rpm.unwrap_or(c.feed / c.pitch.max(1e-6));
            w.block(&format!("M29 S{}", rpm.round()));
        }
        // A tap's feed is its pitch a turn of the speed the program set (whatever the cycle
        // says: a spindle limited since keeps them in step).
        let feed = match (c.kind, w.rpm) {
            (CycleKind::Tap, Some(rpm)) if c.pitch > 0.0 => rpm * c.pitch,
            _ => c.feed,
        };
        w.feed = None;
        let fw = w.feed_word(feed);
        words.push_str(&fw);
        w.block(&words);
        w.block("G80");
        w.mode = None;
        w.at = [Some(c.at[0]), Some(c.at[1]), Some(c.retract)];
        *at = [c.at[0], c.at[1], c.retract];
    } else {
        // As moves.
        let tp = Toolpath {
            steps: vec![Step::Drill(*c)],
            warnings: vec![],
        };
        let start = if at[0].is_nan() {
            [c.at[0], c.at[1], c.retract]
        } else {
            *at
        };
        // A tap backs out with the spindle reversed (stopped first: a floating holder or a
        // control without rigid tapping), then turns the right way again.
        let tap = c.kind == CycleKind::Tap;
        let rpm = w.rpm.unwrap_or(c.feed / c.pitch.max(1e-6)).round();
        let (fwd, rev) = if tool.hand == Hand::Left {
            ("M4", "M3")
        } else {
            ("M3", "M4")
        };
        let mut reversed = false;
        for seg in tp.motion(start, 1.0) {
            match seg.feed {
                None => {
                    if reversed && seg.to[2] > seg.from[2] {
                        w.block("M5");
                        w.block(&format!("S{rpm} {fwd}"));
                        reversed = false;
                    }
                    w.rapid(seg.to)
                }
                Some(f) => {
                    let out_of_bottom =
                        (seg.from[2] - c.bottom).abs() < 1e-9 && seg.to[2] > seg.from[2];
                    if c.dwell > 0.0 && out_of_bottom {
                        w.block(&d.dwell(c.dwell));
                    }
                    if tap && out_of_bottom {
                        w.block("M5");
                        w.block(&format!("S{rpm} {rev}"));
                        reversed = true;
                    }
                    w.linear(seg.to, f)
                }
            }
        }
        if reversed {
            w.block("M5");
            w.block(&format!("S{rpm} {fwd}"));
        }
        *at = [c.at[0], c.at[1], c.retract];
    }
}

/// Runs of feed moves at one feed and height that follow a circle within `tol`, replaced by
/// arcs (the strategies make polylines; G2/G3 are shorter and smoother on the machine).
pub fn fit_arcs(steps: &[Step], tol: f64) -> Vec<Step> {
    let mut out: Vec<Step> = Vec::with_capacity(steps.len());
    let mut i = 0;
    // The position before step i.
    let mut at: Option<P3> = None;
    while i < steps.len() {
        if let (Some(start), Step::Feed { feed, kind, .. }) = (at, &steps[i]) {
            // The longest run from i that fits one arc (at least 4 segments).
            let mut pts = vec![start];
            let mut j = i;
            while j < steps.len() {
                match &steps[j] {
                    Step::Feed {
                        to,
                        feed: f,
                        kind: k,
                    } if (f - feed).abs() < 1e-9
                        && k == kind
                        && (to[2] - start[2]).abs() < 1e-9 =>
                    {
                        pts.push(*to);
                        j += 1;
                    }
                    _ => break,
                }
            }
            let mut best: Option<(usize, P2, bool)> = None;
            let mut n = pts.len();
            while n >= 5 {
                if let Some((c, cw)) = fits_circle(&pts[..n], tol) {
                    best = Some((n, c, cw));
                    break;
                }
                n -= 1;
            }
            if let Some((n, c, cw)) = best {
                out.push(Step::Arc {
                    to: pts[n - 1],
                    center: c,
                    cw,
                    feed: *feed,
                    kind: *kind,
                });
                at = Some(pts[n - 1]);
                i += n - 1;
                continue;
            }
        }
        match &steps[i] {
            Step::Rapid(p) | Step::Feed { to: p, .. } | Step::Arc { to: p, .. } => at = Some(*p),
            Step::Drill(c) => at = Some([c.at[0], c.at[1], c.retract]),
            _ => {}
        }
        out.push(steps[i].clone());
        i += 1;
    }
    out
}

/// Whether the points lie on one circle turning one way (within `tol`, sweeping less than a
/// full turn, chords short against the radius); its centre and direction.
fn fits_circle(pts: &[P3], tol: f64) -> Option<(P2, bool)> {
    let n = pts.len();
    let (a, b, c) = (pts[0], pts[n / 2], pts[n - 1]);
    let center = circumcenter([a[0], a[1]], [b[0], b[1]], [c[0], c[1]])?;
    let r = (a[0] - center[0]).hypot(a[1] - center[1]);
    if r.is_nan() || r <= tol * 10.0 || r > 5_000.0 {
        return None;
    }
    let mut turn = 0.0;
    let mut sign = 0.0;
    for w in pts.windows(2) {
        let (p, q) = (w[0], w[1]);
        let rq = (q[0] - center[0]).hypot(q[1] - center[1]);
        if (rq - r).abs() > tol {
            return None;
        }
        // The chord's middle stays within tol of the circle too.
        let m = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
        if r - (m[0] - center[0]).hypot(m[1] - center[1]) > tol {
            return None;
        }
        let a0 = (p[1] - center[1]).atan2(p[0] - center[0]);
        let a1 = (q[1] - center[1]).atan2(q[0] - center[0]);
        let mut da = a1 - a0;
        while da > std::f64::consts::PI {
            da -= std::f64::consts::TAU;
        }
        while da < -std::f64::consts::PI {
            da += std::f64::consts::TAU;
        }
        if da.abs() < 1e-9 {
            return None;
        }
        if sign == 0.0 {
            sign = da.signum();
        } else if da.signum() != sign {
            return None;
        }
        turn += da;
    }
    (turn.abs() < std::f64::consts::TAU - 0.05).then_some((center, sign < 0.0))
}

fn circumcenter(a: P2, b: P2, c: P2) -> Option<P2> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (
        a[0] * a[0] + a[1] * a[1],
        b[0] * b[0] + b[1] * b[1],
        c[0] * c[0] + c[1] * c[1],
    );
    Some([
        (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d,
        (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d,
    ])
}

/// A file name for a program: its name with characters files can't have replaced, and the
/// dialect's extension.
pub fn file_name(name: &str, d: Dialect) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = stem.trim_matches('_');
    format!(
        "{}.{}",
        if stem.is_empty() { "program" } else { stem },
        d.extension()
    )
}

/// A short summary line of a program for the export dialog: lines and unit.
pub fn summary(nc: &str, s: &PostSettings) -> String {
    format!(
        "{} lines, {}",
        nc.lines().count(),
        match s.output_unit {
            Unit::Mm => "millimetres",
            Unit::Inch => "inches",
        }
    )
}

#[cfg(test)]
mod tests;
