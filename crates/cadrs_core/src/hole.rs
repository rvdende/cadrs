//! Holes (P3.6, PS15): the standard tables, a hole's section and its callout.
//!
//! A hole is a revolution of its half section about the hole's axis, subtracted from the parts
//! (see `rebuild`). The section runs from a little above the start (so the tool leaves no skin
//! at the part's surface) down through the counterbore or countersink and the full diameter, to
//! the drill point (a cone with the tip angle, 118° by default) unless the hole goes through.
//!
//! **Tables** (values from public standards, not from Onshape's library):
//! - Metric clearance holes: ISO 273, fine / medium / coarse series as Close / Normal / Loose
//!   (M5: 5.3 / 5.5 / 5.8; M45: 46 / 48 / 52).
//! - Metric tap drills: nominal diameter less the pitch (ISO 2306's 100 % rule of thumb; M10×1.5:
//!   8.5), coarse pitches from ISO 261 and a few fine ones.
//! - Metric counterbores: for ISO 4762 socket head cap screws, the head diameter `dk` (ISO 4762
//!   max.) plus 1.25 mm, as deep as the head is high (`k = d`): M5 → Ø9.75 × 5, the course's value
//!   (PS17.5). Countersinks: 90°, Ø 2.24·d (ISO 10642 `dk` theoretical: M5 11.2).
//! - Inch (ANSI) clearance holes: ASME B18.2.8 close / normal / loose; tap drills for 75 % thread
//!   (UNC and UNF, from the common drill charts, e.g. 1/4-20: #7 = 0.201"); counterbores for ASME
//!   B18.3 socket heads: head diameter + 1/32", as deep as the nominal diameter; countersinks 82°,
//!   ASME B18.6.3 flat head diameter.
//! - PEM® self-clinching fasteners (P3.10, PS15.5; PennEngineering's published mounting-hole
//!   sizes, bulletins CL and FH): CLS/S nuts M2.5–M8 (M3 Ø4.22, M4 Ø5.41, M5 Ø6.35, M6 Ø8.75,
//!   M8 Ø10.5), FH studs M2.5–M8 (the hole is the thread's nominal Ø); holes +0.08/−0 mm.
//! - Thread classes (P3.10): ISO 965 internal 6H (also 5H, 7H), ASME B1.1 internal 2B (1B, 3B).
//!
//! **Tolerances** (P3.10, PS15.8): the diameter and depth each take a [`Tolerance`] (None,
//! Symmetrical, Deviation, Limits, Min, Max, Basic) written into the callout after the value.

use serde::{Deserialize, Serialize};

/// Inch (ANSI) or Metric (ISO): the dialog's first tabs (PS15.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HoleStandard {
    Ansi,
    #[default]
    Iso,
}

impl HoleStandard {
    pub const ALL: [HoleStandard; 2] = [HoleStandard::Ansi, HoleStandard::Iso];

    /// The tab label.
    pub fn label(self) -> &'static str {
        match self {
            HoleStandard::Ansi => "Inch",
            HoleStandard::Iso => "Metric",
        }
    }

    /// The length unit of its table values (mm).
    pub fn unit_mm(self) -> f64 {
        match self {
            HoleStandard::Ansi => 25.4,
            HoleStandard::Iso => 1.0,
        }
    }

    pub fn unit_name(self) -> &'static str {
        match self {
            HoleStandard::Ansi => "in",
            HoleStandard::Iso => "mm",
        }
    }
}

/// Simple, Counterbore or Countersink: the second row of tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HoleStyle {
    #[default]
    Simple,
    Counterbore,
    Countersink,
}

impl HoleStyle {
    pub const ALL: [HoleStyle; 3] = [HoleStyle::Simple, HoleStyle::Counterbore, HoleStyle::Countersink];

    pub fn label(self) -> &'static str {
        match self {
            HoleStyle::Simple => "Simple",
            HoleStyle::Counterbore => "Counterbore",
            HoleStyle::Countersink => "Countersink",
        }
    }
}

/// Hole type (PS15.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HoleType {
    /// A drill size; the diameter can be edited.
    #[default]
    Drilled,
    /// A fastener size and fit (Close, Normal, Loose).
    Clearance,
    /// A thread (size and pitch): the tap drill, with a tapped depth for the callout.
    Tapped,
    /// (P3.10) The mounting hole of a PEM® self-clinching fastener (Metric only).
    Pem,
}

impl HoleType {
    pub const ALL: [HoleType; 4] = [HoleType::Drilled, HoleType::Clearance, HoleType::Tapped, HoleType::Pem];

    pub fn label(self) -> &'static str {
        match self {
            HoleType::Drilled => "Drilled",
            HoleType::Clearance => "Clearance",
            HoleType::Tapped => "Tapped",
            HoleType::Pem => "PEM®",
        }
    }

    /// Whether the standard offers it (PEM® sizes here are metric).
    pub fn available(self, standard: HoleStandard) -> bool {
        self != HoleType::Pem || standard == HoleStandard::Iso
    }
}

/// A tapped hole's Tap type (P3.10, PS15.4, PS27.8: "Straight tap").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TapType {
    #[default]
    Straight,
    /// Pipe threads; not built (listed disabled).
    Tapered,
}

impl TapType {
    pub const ALL: [TapType; 2] = [TapType::Straight, TapType::Tapered];

    pub fn label(self) -> &'static str {
        match self {
            TapType::Straight => "Straight tap",
            TapType::Tapered => "Tapered tap",
        }
    }
}

/// A diameter's or depth's tolerance in the callout (P3.10, PS15.8), as Onshape's types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ToleranceType {
    #[default]
    None,
    Symmetrical,
    Deviation,
    Limits,
    Min,
    Max,
    Basic,
}

impl ToleranceType {
    pub const ALL: [ToleranceType; 7] = [
        ToleranceType::None,
        ToleranceType::Symmetrical,
        ToleranceType::Deviation,
        ToleranceType::Limits,
        ToleranceType::Min,
        ToleranceType::Max,
        ToleranceType::Basic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToleranceType::None => "None",
            ToleranceType::Symmetrical => "Symmetrical",
            ToleranceType::Deviation => "Deviation",
            ToleranceType::Limits => "Limits",
            ToleranceType::Min => "Min",
            ToleranceType::Max => "Max",
            ToleranceType::Basic => "Basic",
        }
    }
}

/// A tolerance: its type, the upper and lower deviations (in the standard's unit, lower as a
/// positive number below the value) and the decimals shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    pub kind: ToleranceType,
    pub upper: f64,
    pub lower: f64,
    pub precision: usize,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self { kind: ToleranceType::None, upper: 0.1, lower: 0.1, precision: 2 }
    }
}

impl Tolerance {
    /// `v` (in the standard's unit, named `unit`) with the tolerance, as the callout writes it:
    /// "5.3 mm ±0.10", "5.3 mm +0.10/−0.05", "5.40/5.20 mm" (limits), "5.3 mm MIN",
    /// "5.3 mm MAX", "[5.3 mm]" (basic).
    pub fn apply(&self, v: f64, unit: &str) -> String {
        let p = self.precision;
        let value = format!("{} {unit}", fmt(round3(v)));
        let d = |x: f64| format!("{x:.p$}");
        match self.kind {
            ToleranceType::None => value,
            ToleranceType::Symmetrical => format!("{value} ±{}", d(self.upper)),
            ToleranceType::Deviation => format!("{value} +{}/−{}", d(self.upper), d(self.lower)),
            ToleranceType::Limits => format!("{}/{} {unit}", d(v + self.upper), d(v - self.lower)),
            ToleranceType::Min => format!("{value} MIN"),
            ToleranceType::Max => format!("{value} MAX"),
            ToleranceType::Basic => format!("[{value}]"),
        }
    }

    /// An angle `v` (degrees) with the tolerance (P3.11, the countersink's angle): "90° ±1.0",
    /// "90° +1.0/−0.5", "91.0°/89.0°", "90° MIN", "90° MAX", "[90°]".
    pub fn apply_angle(&self, v: f64) -> String {
        let p = self.precision;
        let value = format!("{}°", fmt(round3(v)));
        let d = |x: f64| format!("{x:.p$}");
        match self.kind {
            ToleranceType::None => value,
            ToleranceType::Symmetrical => format!("{value} ±{}", d(self.upper)),
            ToleranceType::Deviation => format!("{value} +{}/−{}", d(self.upper), d(self.lower)),
            ToleranceType::Limits => format!("{}°/{}°", d(v + self.upper), d(v - self.lower)),
            ToleranceType::Min => format!("{value} MIN"),
            ToleranceType::Max => format!("{value} MAX"),
            ToleranceType::Basic => format!("[{value}]"),
        }
    }
}

/// P3.11 (PS15.8): which of a hole's counterbore and countersink sizes a tolerance is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleTolerance {
    CboreDiameter,
    CboreDepth,
    CsinkDiameter,
    CsinkAngle,
}

impl StyleTolerance {
    pub const ALL: [StyleTolerance; 4] =
        [StyleTolerance::CboreDiameter, StyleTolerance::CboreDepth, StyleTolerance::CsinkDiameter, StyleTolerance::CsinkAngle];

    /// The dialog's section title.
    pub fn label(self) -> &'static str {
        match self {
            StyleTolerance::CboreDiameter => "Counterbore Ø tolerance",
            StyleTolerance::CboreDepth => "Counterbore depth tolerance",
            StyleTolerance::CsinkDiameter => "Countersink Ø tolerance",
            StyleTolerance::CsinkAngle => "Countersink angle tolerance",
        }
    }

    /// The dialog's name for its rows ("hole-cbore-diameter-tolerance").
    pub fn name(self) -> &'static str {
        match self {
            StyleTolerance::CboreDiameter => "hole-cbore-diameter-tolerance",
            StyleTolerance::CboreDepth => "hole-cbore-depth-tolerance",
            StyleTolerance::CsinkDiameter => "hole-csink-diameter-tolerance",
            StyleTolerance::CsinkAngle => "hole-csink-angle-tolerance",
        }
    }
}

/// A PEM® fastener type (P3.10, PS15.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PemType {
    /// CLS / S self-clinching nuts.
    #[default]
    Nut,
    /// FH self-clinching studs.
    Stud,
}

impl PemType {
    pub const ALL: [PemType; 2] = [PemType::Nut, PemType::Stud];

    pub fn label(self) -> &'static str {
        match self {
            PemType::Nut => "Self-clinching nut (CLS)",
            PemType::Stud => "Self-clinching stud (FH)",
        }
    }
}

/// PennEngineering's mounting-hole diameters (mm) for metric CLS/S nuts and FH studs, by thread.
pub const PEM: &[(&str, f64, f64)] = &[
    ("M2.5", 4.22, 2.5),
    ("M3", 4.22, 3.0),
    ("M3.5", 4.75, 3.5),
    ("M4", 5.41, 4.0),
    ("M5", 6.35, 5.0),
    ("M6", 8.75, 6.0),
    ("M8", 10.5, 8.0),
];

/// The mounting hole of a PEM® fastener of `size`.
pub fn pem_hole(kind: PemType, size: &str) -> Option<f64> {
    PEM.iter().find(|(n, ..)| *n == size).map(|(_, nut, stud)| match kind {
        PemType::Nut => *nut,
        PemType::Stud => *stud,
    })
}

/// Fastener fit of a clearance hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Fit {
    Close,
    #[default]
    Normal,
    Loose,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Close, Fit::Normal, Fit::Loose];

    pub fn label(self) -> &'static str {
        match self {
            Fit::Close => "Close",
            Fit::Normal => "Normal",
            Fit::Loose => "Loose",
        }
    }

    fn index(self) -> usize {
        match self {
            Fit::Close => 0,
            Fit::Normal => 1,
            Fit::Loose => 2,
        }
    }
}

/// Where the hole starts (PS15.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HoleStart {
    /// Where the part begins along the hole direction (from the sketch plane on).
    #[default]
    Part,
    /// At the sketch plane.
    SketchPlane,
    /// (P3.10) At a picked plane or flat face (the Hole start plane).
    SelectedPlane,
}

impl HoleStart {
    pub const ALL: [HoleStart; 3] = [HoleStart::Part, HoleStart::SketchPlane, HoleStart::SelectedPlane];

    pub fn label(self) -> &'static str {
        match self {
            HoleStart::Part => "Start from part",
            HoleStart::SketchPlane => "Start from sketch plane",
            HoleStart::SelectedPlane => "Start from selected plane",
        }
    }
}

/// Termination (PS15.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HoleEnd {
    /// Through every part in the merge scope.
    ThroughAll,
    /// To a depth (of the full diameter); the drill point goes past it.
    #[default]
    Blind,
    /// Until the hole leaves the part it entered (the full diameter reaches that face).
    UpToNext,
    /// (P3.10) Until the full diameter reaches a picked face or plane (the drill point goes
    /// past it).
    UpToEntity,
}

impl HoleEnd {
    pub const ALL: [HoleEnd; 4] = [HoleEnd::ThroughAll, HoleEnd::Blind, HoleEnd::UpToNext, HoleEnd::UpToEntity];

    pub fn label(self) -> &'static str {
        match self {
            HoleEnd::ThroughAll => "Through",
            HoleEnd::Blind => "Blind",
            HoleEnd::UpToNext => "Up to next",
            HoleEnd::UpToEntity => "Up to entity",
        }
    }
}

/// A length typed in a field: its value (mm) and the text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Length {
    pub value: f64,
    pub expr: String,
}

impl Length {
    /// A value in the standard's unit, written the way the dialog shows it.
    pub fn of(standard: HoleStandard, v: f64) -> Self {
        Self {
            value: v * standard.unit_mm(),
            expr: format!("{} {}", fmt(v), standard.unit_name()),
        }
    }

    pub fn mm(v: f64) -> Self {
        Self {
            value: v,
            expr: format!("{} mm", fmt(v)),
        }
    }

    pub fn deg(v: f64) -> Self {
        Self {
            value: v,
            expr: format!("{} deg", fmt(v)),
        }
    }
}

/// A number without trailing zeros (at most 4 decimals).
pub fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// Which table entry a hole uses. Kept as text so a document names its size the way the table
/// does ("M5", "#10", "1/4", "M10x1.5", "1/4-20 UNC", "5 mm", "#7").
pub type HoleSize = String;

/// A hole's shape and size (the dialog's fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleSpec {
    pub standard: HoleStandard,
    pub style: HoleStyle,
    pub hole_type: HoleType,
    /// Drill size (Drilled) or fastener size (Clearance, Tapped).
    pub size: HoleSize,
    pub fit: Fit,
    /// The thread (Tapped): "M10x1.5", "1/4-20 UNC".
    pub pitch: String,
    /// The hole's diameter (the drill, clearance or tap drill diameter; editable).
    pub diameter: Length,
    pub start: HoleStart,
    pub end: HoleEnd,
    /// The full diameter's depth (Blind) from the start.
    pub depth: Length,
    /// The drill point's angle (degrees).
    pub tip_angle: Length,
    pub cbore_diameter: Length,
    pub cbore_depth: Length,
    pub csink_diameter: Length,
    pub csink_angle: Length,
    /// The thread's depth (Tapped; shown in the callout).
    pub tapped_depth: Length,
    /// (P3.10) A tapped hole's Tap type.
    #[serde(default)]
    pub tap_type: TapType,
    /// (P3.10) The Thread class checkbox and its class ("6H", "2B").
    #[serde(default)]
    pub thread_class: bool,
    #[serde(default = "six_h")]
    pub class: String,
    /// (P3.10) A PEM® hole's fastener type.
    #[serde(default)]
    pub pem: PemType,
    /// (P3.10, PS15.8) The diameter's and the depth's tolerances in the callout.
    #[serde(default)]
    pub diameter_tol: Tolerance,
    #[serde(default)]
    pub depth_tol: Tolerance,
    /// (P3.11, PS15.8) The counterbore's diameter and depth tolerances and the countersink's
    /// diameter and angle tolerances (see [`StyleTolerance`]).
    #[serde(default)]
    pub cbore_diameter_tol: Tolerance,
    #[serde(default)]
    pub cbore_depth_tol: Tolerance,
    #[serde(default)]
    pub csink_diameter_tol: Tolerance,
    #[serde(default)]
    pub csink_angle_tol: Tolerance,
    /// (P3.10, PS15.7) Up to next / Up to entity: the full diameter stops this far short of the
    /// target (negative: past it); `None` is no offset.
    #[serde(default)]
    pub end_offset: Option<Length>,
}

fn six_h() -> String {
    "6H".into()
}

impl Default for HoleSpec {
    fn default() -> Self {
        Self::new(String::new())
    }
}

impl HoleSpec {
    /// A metric simple drilled hole of `size` (a drill size; "5 mm" if empty), Blind 10 mm.
    pub fn new(size: HoleSize) -> Self {
        let size = if size.is_empty() { "5 mm".to_string() } else { size };
        let mut s = Self {
            standard: HoleStandard::Iso,
            style: HoleStyle::Simple,
            hole_type: HoleType::Drilled,
            size,
            fit: Fit::Normal,
            pitch: String::new(),
            diameter: Length::mm(5.0),
            start: HoleStart::Part,
            end: HoleEnd::Blind,
            depth: Length::mm(10.0),
            tip_angle: Length::deg(118.0),
            cbore_diameter: Length::mm(10.0),
            cbore_depth: Length::mm(5.0),
            csink_diameter: Length::mm(10.0),
            csink_angle: Length::deg(90.0),
            tapped_depth: Length::mm(10.0),
            tap_type: TapType::Straight,
            thread_class: false,
            class: six_h(),
            pem: PemType::Nut,
            diameter_tol: Tolerance::default(),
            depth_tol: Tolerance::default(),
            cbore_diameter_tol: Tolerance::default(),
            cbore_depth_tol: Tolerance::default(),
            csink_diameter_tol: Tolerance::default(),
            csink_angle_tol: Tolerance::default(),
            end_offset: None,
        };
        s.apply_table();
        s
    }

    /// The sizes the Size dropdown offers for the standard and hole type.
    pub fn sizes(standard: HoleStandard, hole_type: HoleType) -> Vec<String> {
        match (standard, hole_type) {
            (HoleStandard::Iso, HoleType::Drilled) => METRIC_DRILLS.iter().map(|d| format!("{} mm", fmt(*d))).collect(),
            (_, HoleType::Pem) => PEM.iter().map(|(n, ..)| n.to_string()).collect(),
            (HoleStandard::Ansi, HoleType::Drilled) => INCH_DRILLS.iter().map(|(n, _)| n.to_string()).collect(),
            (HoleStandard::Iso, _) => ISO.iter().map(|m| m.name.to_string()).collect(),
            (HoleStandard::Ansi, _) => ANSI.iter().map(|m| m.name.to_string()).collect(),
        }
    }

    /// The threads the Pitch dropdown offers for a tapped hole's size.
    pub fn pitches(standard: HoleStandard, size: &str) -> Vec<String> {
        match standard {
            HoleStandard::Iso => iso(size)
                .map(|m| {
                    std::iter::once(m.coarse)
                        .chain(m.fine.iter().copied())
                        .map(|p| format!("{}x{}", m.name, fmt(p)))
                        .collect()
                })
                .unwrap_or_default(),
            HoleStandard::Ansi => ansi(size)
                .map(|a| {
                    let mut v = vec![format!("{}-{} UNC", a.name, a.unc)];
                    if let Some(f) = a.unf {
                        v.push(format!("{}-{} UNF", a.name, f));
                    }
                    v
                })
                .unwrap_or_default(),
        }
    }

    /// The Pitch dropdown's label for a thread: "1.50 mm (Coarse)", "1.25 mm (Fine)" (metric,
    /// as Onshape writes it), "20 tpi (UNC)" (inch).
    pub fn pitch_label(standard: HoleStandard, thread: &str) -> String {
        match standard {
            HoleStandard::Iso => {
                let Some((m, p)) = thread.split_once('x') else { return thread.to_string() };
                let Ok(v) = p.parse::<f64>() else { return thread.to_string() };
                let coarse = iso(m).is_some_and(|s| (s.coarse - v).abs() < 1e-9);
                format!("{v:.2} mm ({})", if coarse { "Coarse" } else { "Fine" })
            }
            HoleStandard::Ansi => match thread.split_once('-') {
                Some((_, rest)) => match rest.split_once(' ') {
                    Some((tpi, series)) => format!("{tpi} tpi ({series})"),
                    None => thread.to_string(),
                },
                None => thread.to_string(),
            },
        }
    }

    /// The thread's pitch (mm): a metric thread's P, an inch thread's 25.4 / tpi.
    pub fn pitch_mm(&self) -> Option<f64> {
        match self.standard {
            HoleStandard::Iso => self.pitch.split_once('x')?.1.parse().ok(),
            HoleStandard::Ansi => {
                let tpi: f64 = self.pitch.split_once('-')?.1.split(' ').next()?.parse().ok()?;
                Some(25.4 / tpi)
            }
        }
    }

    /// The tapped clearance (PS27.8): the threads' worth of plain hole below the thread,
    /// (hole depth − tapped depth) / pitch (Blind 20, tapped 10.02, P 1.5: 6.653).
    pub fn tap_clearance(&self) -> Option<f64> {
        let p = self.pitch_mm()?;
        (p > 0.0).then(|| (self.depth.value - self.tapped_depth.value) / p)
    }

    /// The thread classes the Class dropdown offers.
    pub fn classes(standard: HoleStandard) -> &'static [&'static str] {
        match standard {
            HoleStandard::Iso => &["6H", "5H", "7H"],
            HoleStandard::Ansi => &["2B", "1B", "3B"],
        }
    }

    /// Resets the size-dependent fields from the tables for the current standard, type, size,
    /// fit and pitch: the diameter, the counterbore, the countersink, the tip angle's default
    /// countersink angle (82° inch, 90° metric). An unknown size keeps the values.
    pub fn apply_table(&mut self) {
        if !self.hole_type.available(self.standard) {
            self.hole_type = HoleType::Drilled;
        }
        if !Self::classes(self.standard).contains(&self.class.as_str()) {
            self.class = Self::classes(self.standard)[0].to_string();
        }
        let st = self.standard;
        // A size from another list (the type or standard changed): the list's default.
        let sizes = Self::sizes(st, self.hole_type);
        if !sizes.contains(&self.size) {
            self.size = match (st, self.hole_type) {
                (HoleStandard::Iso, HoleType::Drilled) => "5 mm".into(),
                (HoleStandard::Ansi, HoleType::Drilled) => "#7".into(),
                (_, HoleType::Pem) => "M4".into(),
                (HoleStandard::Iso, _) => "M5".into(),
                (HoleStandard::Ansi, _) => "1/4".into(),
            };
        }
        if self.hole_type == HoleType::Pem {
            // PEM® mounting holes: the table's diameter, +0.08/−0 mm.
            if let Some(d) = pem_hole(self.pem, &self.size) {
                self.diameter = Length::mm(d);
                self.diameter_tol = Tolerance { kind: ToleranceType::Deviation, upper: 0.08, lower: 0.0, precision: 2 };
            }
            return;
        }
        if self.hole_type == HoleType::Tapped {
            let pitches = Self::pitches(st, &self.size);
            if !pitches.contains(&self.pitch) {
                self.pitch = pitches.first().cloned().unwrap_or_default();
            }
        }
        self.csink_angle = Length::deg(if st == HoleStandard::Ansi { 82.0 } else { 90.0 });
        // The depths follow the standard's unit (the Inch tab shows inches, to 0.001 in; the
        // Metric tab mm, to 0.01 mm).
        let (unit, step) = (st.unit_name(), if st == HoleStandard::Ansi { 1000.0 } else { 100.0 });
        for l in [&mut self.depth, &mut self.tapped_depth] {
            if !l.expr.trim_end().ends_with(unit) {
                *l = Length::of(st, (l.value / st.unit_mm() * step).round() / step);
            }
        }
        match st {
            HoleStandard::Iso => {
                let d = match self.hole_type {
                    HoleType::Drilled => self.size.trim_end_matches(" mm").parse::<f64>().ok(),
                    HoleType::Clearance => iso(&self.size).map(|m| m.clearance[self.fit.index()]),
                    HoleType::Tapped => iso_tap(&self.pitch),
                    HoleType::Pem => None,
                };
                if let Some(d) = d {
                    self.diameter = Length::mm(d);
                }
                // The head a fastener of this size has (the drill's nearest fastener for Drilled).
                let m = match self.hole_type {
                    HoleType::Drilled => ISO.iter().rev().find(|m| m.d <= self.diameter.value + 1e-9).or(ISO.first()),
                    _ => iso(&self.size),
                };
                if let Some(m) = m {
                    self.cbore_diameter = Length::mm(m.dk + 1.25);
                    self.cbore_depth = Length::mm(m.d);
                    self.csink_diameter = Length::mm((2.24 * m.d * 100.0).round() / 100.0);
                }
            }
            HoleStandard::Ansi => {
                let d = match self.hole_type {
                    HoleType::Drilled => INCH_DRILLS.iter().find(|(n, _)| *n == self.size).map(|(_, d)| *d),
                    HoleType::Clearance => ansi(&self.size).map(|a| a.clearance[self.fit.index()]),
                    HoleType::Tapped => ansi(&self.size).map(|a| if self.pitch.ends_with("UNF") { a.tap_unf.unwrap_or(a.tap_unc) } else { a.tap_unc }),
                    HoleType::Pem => None,
                };
                if let Some(d) = d {
                    self.diameter = Length::of(st, d);
                }
                let a = match self.hole_type {
                    HoleType::Drilled => ANSI.iter().rev().find(|a| a.d <= self.diameter.value / 25.4 + 1e-9).or(ANSI.first()),
                    _ => ansi(&self.size),
                };
                if let Some(a) = a {
                    self.cbore_diameter = Length::of(st, a.head + 1.0 / 32.0);
                    self.cbore_depth = Length::of(st, a.d);
                    self.csink_diameter = Length::of(st, a.flat_head);
                }
            }
        }
    }

    /// Why the fields can't be built, if they can't.
    pub fn problem(&self) -> Option<&'static str> {
        let pos = |l: &Length| l.value > 0.0 && l.value.is_finite();
        if !pos(&self.diameter) {
            return Some("The hole diameter must be greater than zero");
        }
        if self.end == HoleEnd::Blind && !pos(&self.depth) {
            return Some("The hole depth must be greater than zero");
        }
        if !(self.tip_angle.value > 0.0 && self.tip_angle.value < 180.0) {
            return Some("The tip angle must be between 0 and 180°");
        }
        match self.style {
            HoleStyle::Counterbore if self.cbore_diameter.value <= self.diameter.value => {
                Some("The counterbore must be wider than the hole")
            }
            HoleStyle::Counterbore if !pos(&self.cbore_depth) => Some("The counterbore depth must be greater than zero"),
            HoleStyle::Countersink if self.csink_diameter.value <= self.diameter.value => {
                Some("The countersink must be wider than the hole")
            }
            HoleStyle::Countersink if !(self.csink_angle.value > 0.0 && self.csink_angle.value < 180.0) => {
                Some("The countersink angle must be between 0 and 180°")
            }
            _ => None,
        }
    }

    /// The drill point's height below the full diameter: `r / tan(tip/2)`.
    pub fn tip_height(&self) -> f64 {
        self.diameter.value / 2.0 / (self.tip_angle.value.to_radians() / 2.0).tan()
    }

    /// The countersink cone's depth from the start down to the hole's diameter.
    pub fn csink_depth(&self) -> f64 {
        (self.csink_diameter.value - self.diameter.value) / 2.0 / (self.csink_angle.value.to_radians() / 2.0).tan()
    }

    /// The half section, as (radius, depth below the start) corners in order from the axis
    /// above the start, for a full diameter `depth` deep (with a drill point unless `through`)
    /// and `above` mm of tool above the start.
    pub fn section(&self, depth: f64, through: bool, above: f64) -> Vec<(f64, f64)> {
        let r = self.diameter.value / 2.0;
        let mut pts = vec![(0.0, -above)];
        match self.style {
            HoleStyle::Simple => {
                pts.push((r, -above));
            }
            HoleStyle::Counterbore => {
                let (rc, hc) = (self.cbore_diameter.value / 2.0, self.cbore_depth.value.min(depth));
                pts.extend([(rc, -above), (rc, hc), (r, hc)]);
            }
            HoleStyle::Countersink => {
                let rs = self.csink_diameter.value / 2.0;
                pts.extend([(rs, -above), (rs, 0.0), (r, self.csink_depth().min(depth))]);
            }
        }
        pts.push((r, depth));
        if through {
            pts.push((0.0, depth));
        } else {
            pts.push((0.0, depth + self.tip_height()));
        }
        // No zero-length sides (a buried countersink starts at its own rim, P3.10).
        pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-12 && (a.1 - b.1).abs() < 1e-12);
        pts
    }

    /// The live callout (PS15.10), e.g. `Ø 5.3 mm THRU | ⌴Ø 9.75 mm ↧ 5 mm` or
    /// `M10x1.50 ↧ 20 mm`. `⌴` is the counterbore symbol, `⌵` the countersink, `↧` depth. A tapped
    /// blind hole gives its hole depth (P3.8 judge: the course's `M10x1.50 ↧ 20 mm`); the thread
    /// class follows the thread ("M10x1.50 - 6H"). Tolerances follow their values (PS15.8).
    /// A counterbore or countersink tolerance (P3.11).
    pub fn style_tol(&self, which: StyleTolerance) -> &Tolerance {
        match which {
            StyleTolerance::CboreDiameter => &self.cbore_diameter_tol,
            StyleTolerance::CboreDepth => &self.cbore_depth_tol,
            StyleTolerance::CsinkDiameter => &self.csink_diameter_tol,
            StyleTolerance::CsinkAngle => &self.csink_angle_tol,
        }
    }

    pub fn style_tol_mut(&mut self, which: StyleTolerance) -> &mut Tolerance {
        match which {
            StyleTolerance::CboreDiameter => &mut self.cbore_diameter_tol,
            StyleTolerance::CboreDepth => &mut self.cbore_depth_tol,
            StyleTolerance::CsinkDiameter => &mut self.csink_diameter_tol,
            StyleTolerance::CsinkAngle => &mut self.csink_angle_tol,
        }
    }

    pub fn callout(&self) -> String {
        let st = self.standard;
        let len = |l: &Length| format!("{} {}", fmt(round3(l.value / st.unit_mm())), st.unit_name());
        let tol = |l: &Length, t: &Tolerance| t.apply(l.value / st.unit_mm(), st.unit_name());
        let depth = tol(&self.depth, &self.depth_tol);
        let end = match self.end {
            HoleEnd::ThroughAll => "THRU".to_string(),
            HoleEnd::Blind => format!("↧ {depth}"),
            HoleEnd::UpToNext => "UP TO NEXT".to_string(),
            HoleEnd::UpToEntity => "UP TO ENTITY".to_string(),
        };
        let main = if self.hole_type == HoleType::Tapped && !self.pitch.is_empty() {
            let mut thread = thread_label(&self.pitch);
            if self.thread_class {
                thread = format!("{thread} - {}", self.class);
            }
            match self.end {
                HoleEnd::ThroughAll => format!("{thread} THRU"),
                HoleEnd::Blind => format!("{thread} ↧ {depth}"),
                _ => format!("{thread} ↧ {} {end}", len(&self.tapped_depth)),
            }
        } else {
            format!("Ø {} {end}", tol(&self.diameter, &self.diameter_tol))
        };
        match self.style {
            HoleStyle::Simple => main,
            HoleStyle::Counterbore => format!(
                "{main} | ⌴Ø {} ↧ {}",
                tol(&self.cbore_diameter, &self.cbore_diameter_tol),
                tol(&self.cbore_depth, &self.cbore_depth_tol)
            ),
            HoleStyle::Countersink => format!(
                "{main} | ⌵Ø {} X {}",
                tol(&self.csink_diameter, &self.csink_diameter_tol),
                self.csink_angle_tol.apply_angle(self.csink_angle.value)
            ),
        }
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// "M10x1.5" → "M10x1.50" (the pitch with two decimals, as Onshape writes it); inch threads as
/// they are.
pub fn thread_label(pitch: &str) -> String {
    match pitch.split_once('x') {
        Some((m, p)) if m.starts_with('M') => match p.parse::<f64>() {
            Ok(v) => format!("{m}x{v:.2}"),
            Err(_) => pitch.to_string(),
        },
        _ => pitch.to_string(),
    }
}

/// A metric fastener size.
#[derive(Debug, Clone, Copy)]
pub struct IsoSize {
    pub name: &'static str,
    /// Nominal diameter (mm).
    pub d: f64,
    /// Coarse pitch (ISO 261).
    pub coarse: f64,
    /// Some fine pitches.
    pub fine: &'static [f64],
    /// ISO 273 clearance holes: fine (Close), medium (Normal), coarse (Loose).
    pub clearance: [f64; 3],
    /// ISO 4762 socket head diameter (max.).
    pub dk: f64,
}

const fn m(name: &'static str, d: f64, coarse: f64, fine: &'static [f64], clearance: [f64; 3], dk: f64) -> IsoSize {
    IsoSize { name, d, coarse, fine, clearance, dk }
}

pub const ISO: &[IsoSize] = &[
    m("M1.6", 1.6, 0.35, &[], [1.7, 1.8, 2.0], 3.0),
    m("M2", 2.0, 0.4, &[], [2.2, 2.4, 2.6], 3.8),
    m("M2.5", 2.5, 0.45, &[], [2.7, 2.9, 3.1], 4.5),
    m("M3", 3.0, 0.5, &[], [3.2, 3.4, 3.6], 5.5),
    m("M4", 4.0, 0.7, &[], [4.3, 4.5, 4.8], 7.0),
    m("M5", 5.0, 0.8, &[], [5.3, 5.5, 5.8], 8.5),
    m("M6", 6.0, 1.0, &[], [6.4, 6.6, 7.0], 10.0),
    m("M8", 8.0, 1.25, &[1.0], [8.4, 9.0, 10.0], 13.0),
    m("M10", 10.0, 1.5, &[1.25, 1.0], [10.5, 11.0, 12.0], 16.0),
    m("M12", 12.0, 1.75, &[1.5, 1.25], [13.0, 13.5, 14.5], 18.0),
    m("M14", 14.0, 2.0, &[1.5], [15.0, 15.5, 16.5], 21.0),
    m("M16", 16.0, 2.0, &[1.5], [17.0, 17.5, 18.5], 24.0),
    m("M20", 20.0, 2.5, &[1.5], [21.0, 22.0, 24.0], 30.0),
    m("M24", 24.0, 3.0, &[2.0], [25.0, 26.0, 28.0], 36.0),
    m("M30", 30.0, 3.5, &[2.0], [31.0, 33.0, 35.0], 45.0),
    m("M36", 36.0, 4.0, &[3.0], [37.0, 39.0, 42.0], 54.0),
    m("M42", 42.0, 4.5, &[3.0], [43.0, 45.0, 48.0], 63.0),
    m("M45", 45.0, 4.5, &[3.0], [46.0, 48.0, 52.0], 68.0),
    m("M48", 48.0, 5.0, &[3.0], [50.0, 52.0, 56.0], 72.0),
];

pub fn iso(size: &str) -> Option<&'static IsoSize> {
    ISO.iter().find(|m| m.name == size)
}

/// The tap drill of a metric thread "M10x1.5": d − P.
pub fn iso_tap(pitch: &str) -> Option<f64> {
    let (m, p) = pitch.split_once('x')?;
    let size = iso(m)?;
    let p: f64 = p.parse().ok()?;
    Some(((size.d - p) * 100.0).round() / 100.0)
}

/// An inch fastener size (values in inches).
#[derive(Debug, Clone, Copy)]
pub struct AnsiSize {
    pub name: &'static str,
    pub d: f64,
    /// Threads per inch, coarse (UNC) and fine (UNF).
    pub unc: u32,
    pub unf: Option<u32>,
    /// ASME B18.2.8 clearance: close, normal, loose.
    pub clearance: [f64; 3],
    /// Tap drills for 75 % thread.
    pub tap_unc: f64,
    pub tap_unf: Option<f64>,
    /// ASME B18.3 socket head diameter.
    pub head: f64,
    /// ASME B18.6.3 82° flat head diameter.
    pub flat_head: f64,
}

#[allow(clippy::too_many_arguments)]
const fn a(
    name: &'static str,
    d: f64,
    unc: u32,
    unf: Option<u32>,
    clearance: [f64; 3],
    tap_unc: f64,
    tap_unf: Option<f64>,
    head: f64,
    flat_head: f64,
) -> AnsiSize {
    AnsiSize { name, d, unc, unf, clearance, tap_unc, tap_unf, head, flat_head }
}

pub const ANSI: &[AnsiSize] = &[
    a("#0", 0.060, 80, None, [0.0635, 0.073, 0.081], 0.0469, None, 0.096, 0.119),
    a("#1", 0.073, 64, Some(72), [0.076, 0.089, 0.104], 0.0595, Some(0.0595), 0.118, 0.146),
    a("#2", 0.086, 56, Some(64), [0.089, 0.1015, 0.116], 0.070, Some(0.070), 0.140, 0.172),
    a("#3", 0.099, 48, Some(56), [0.104, 0.110, 0.1285], 0.0785, Some(0.082), 0.161, 0.199),
    a("#4", 0.112, 40, Some(48), [0.116, 0.1285, 0.144], 0.089, Some(0.0935), 0.183, 0.225),
    a("#5", 0.125, 40, Some(44), [0.1285, 0.136, 0.1495], 0.1015, Some(0.104), 0.205, 0.252),
    a("#6", 0.138, 32, Some(40), [0.144, 0.1495, 0.1695], 0.1065, Some(0.113), 0.226, 0.279),
    a("#8", 0.164, 32, Some(36), [0.1695, 0.177, 0.196], 0.136, Some(0.136), 0.270, 0.332),
    a("#10", 0.190, 24, Some(32), [0.196, 0.201, 0.221], 0.1495, Some(0.159), 0.312, 0.385),
    a("#12", 0.216, 24, Some(28), [0.221, 0.228, 0.246], 0.177, Some(0.180), 0.354, 0.438),
    a("1/4", 0.250, 20, Some(28), [0.257, 0.266, 0.281], 0.201, Some(0.213), 0.375, 0.507),
    a("5/16", 0.3125, 18, Some(24), [0.323, 0.332, 0.344], 0.257, Some(0.272), 0.469, 0.635),
    a("3/8", 0.375, 16, Some(24), [0.386, 0.397, 0.406], 0.3125, Some(0.332), 0.562, 0.762),
    a("7/16", 0.4375, 14, Some(20), [0.453, 0.469, 0.484], 0.368, Some(0.3906), 0.656, 0.812),
    a("1/2", 0.500, 13, Some(20), [0.516, 0.531, 0.547], 0.4219, Some(0.4531), 0.750, 0.875),
    a("5/8", 0.625, 11, Some(18), [0.641, 0.656, 0.672], 0.5312, Some(0.5781), 0.938, 1.125),
    a("3/4", 0.750, 10, Some(16), [0.766, 0.781, 0.797], 0.6562, Some(0.6875), 1.125, 1.375),
    a("7/8", 0.875, 9, Some(14), [0.891, 0.906, 0.922], 0.7656, Some(0.8125), 1.312, 1.625),
    a("1", 1.000, 8, Some(12), [1.016, 1.031, 1.047], 0.875, Some(0.9219), 1.500, 1.875),
];

pub fn ansi(size: &str) -> Option<&'static AnsiSize> {
    ANSI.iter().find(|a| a.name == size)
}

/// Metric drill sizes (mm).
pub const METRIC_DRILLS: &[f64] = &[
    1.0, 1.5, 2.0, 2.5, 3.0, 3.2, 3.3, 3.5, 4.0, 4.2, 4.5, 5.0, 5.3, 5.5, 6.0, 6.4, 6.6, 6.8, 7.0, 8.0, 8.4, 8.5,
    9.0, 10.0, 10.2, 10.5, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.5, 18.0, 20.0,
];

/// Inch drill sizes: numbers, letters and fractions (inches).
pub const INCH_DRILLS: &[(&str, f64)] = &[
    ("#50", 0.070),
    ("#43", 0.089),
    ("#36", 0.1065),
    ("#29", 0.136),
    ("#25", 0.1495),
    ("#21", 0.159),
    ("#16", 0.177),
    ("#7", 0.201),
    ("#3", 0.213),
    ("F", 0.257),
    ("I", 0.272),
    ("Q", 0.332),
    ("U", 0.368),
    ("1/8", 0.125),
    ("3/16", 0.1875),
    ("1/4", 0.250),
    ("5/16", 0.3125),
    ("3/8", 0.375),
    ("7/16", 0.4375),
    ("1/2", 0.500),
    ("5/8", 0.625),
    ("3/4", 0.750),
];

/// The points of a whole sketch a hole goes at (PS15.1): standalone points, the ends of its
/// (non-construction) lines and arcs, and the centres of its circles and arcs.
pub fn hole_vertices(g: &cadrs_sketch::Sketch) -> Vec<cadrs_sketch::PointId> {
    use cadrs_sketch::CurveKind;
    let mut used: Vec<cadrs_sketch::PointId> = Vec::new();
    let mut out: Vec<cadrs_sketch::PointId> = Vec::new();
    for (id, c) in &g.curves {
        let pts: Vec<cadrs_sketch::PointId> = match c.kind {
            CurveKind::Spline { .. } => g.kind_points(id, &c.kind),
            CurveKind::Line { a, b } => vec![a, b],
            CurveKind::Circle { center, .. } => vec![center],
            CurveKind::Arc { center, start, end } => vec![center, start, end],
            CurveKind::Ellipse { center, major, .. } | CurveKind::EllipseOffset { center, major, .. } => vec![center, major],
            // Its ends (its control points are handles, not places for holes).
            CurveKind::Bezier { a, b, c1, c2 } => {
                used.extend([c1, c2]);
                vec![a, b]
            }
        };
        used.extend(&pts);
        if c.construction || matches!(c.kind, CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } | CurveKind::Spline { .. }) {
            continue;
        }
        for p in pts {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    // Text boxes' corners are construction lines, so they are only "used".
    for (id, _) in &g.points {
        if !used.contains(&id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// The points a hole can be placed at one by one (picked in the view): [`hole_vertices`], and
/// the ends and centres of construction lines, arcs and circles too (holes laid out on
/// construction geometry). Text boxes' corners and curve handles are not.
pub fn pickable_points(g: &cadrs_sketch::Sketch) -> Vec<cadrs_sketch::PointId> {
    use cadrs_sketch::CurveKind;
    let mut out = hole_vertices(g);
    let corners: Vec<cadrs_sketch::PointId> = g.texts.values().flat_map(|t| t.corners).collect();
    for c in g.curves.values().filter(|c| c.construction) {
        let pts = match c.kind {
            CurveKind::Line { a, b } => vec![a, b],
            CurveKind::Circle { center, .. } => vec![center],
            CurveKind::Arc { center, start, end } => vec![center, start, end],
            _ => Vec::new(),
        };
        for p in pts {
            if !corners.contains(&p) && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_points_are_pickable_but_not_in_a_whole_sketch() {
        use cadrs_sketch::{SketchOp, Vec2};
        let mut g = cadrs_sketch::Sketch::default();
        SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)],
            closed: false,
            construction: true,
            label: "Add line",
        }
        .apply(&mut g)
        .unwrap();
        assert!(hole_vertices(&g).is_empty());
        assert_eq!(pickable_points(&g).len(), 2);
    }

    #[test]
    fn course_sizes() {
        // PS17.5: M5 Close clearance is Ø5.3, with a Ø9.75 × 5 counterbore.
        let mut s = HoleSpec::new(String::new());
        s.style = HoleStyle::Counterbore;
        s.hole_type = HoleType::Clearance;
        s.size = "M5".into();
        s.fit = Fit::Close;
        s.end = HoleEnd::ThroughAll;
        s.apply_table();
        assert_eq!(s.diameter.value, 5.3);
        assert_eq!(s.cbore_diameter.value, 9.75);
        assert_eq!(s.cbore_depth.value, 5.0);
        assert_eq!(s.callout(), "Ø 5.3 mm THRU | ⌴Ø 9.75 mm ↧ 5 mm");
        // M45 Close: Ø46 (ISO 273 fine).
        s.size = "M45".into();
        s.apply_table();
        assert_eq!(s.diameter.value, 46.0);
        // Tapped M10×1.5: tap drill 8.5, callout "M10x1.50 ↧ 20 mm".
        let mut t = HoleSpec::new(String::new());
        t.hole_type = HoleType::Tapped;
        t.size = "M10".into();
        t.apply_table();
        assert_eq!(t.pitch, "M10x1.5");
        assert_eq!(t.diameter.value, 8.5);
        // The hole depth names a tapped blind hole (the course's Blind 20 with 10.02 tapped).
        t.depth = Length::mm(20.0);
        t.tapped_depth = Length::mm(10.02);
        assert_eq!(t.callout(), "M10x1.50 ↧ 20 mm");
        // PS27.8's last row: (20 − 10.02) / 1.5 = 6.653 threads of tap clearance.
        assert!((t.tap_clearance().unwrap() - 6.653_333).abs() < 1e-5);
        assert_eq!(HoleSpec::pitch_label(HoleStandard::Iso, "M10x1.5"), "1.50 mm (Coarse)");
        assert_eq!(HoleSpec::pitch_label(HoleStandard::Iso, "M10x1.25"), "1.25 mm (Fine)");
        assert_eq!(HoleSpec::pitch_label(HoleStandard::Ansi, "1/4-20 UNC"), "20 tpi (UNC)");
        t.thread_class = true;
        assert_eq!(t.callout(), "M10x1.50 - 6H ↧ 20 mm");
        assert_eq!(HoleSpec::pitches(HoleStandard::Iso, "M10"), ["M10x1.5", "M10x1.25", "M10x1"]);
        t.pitch = "M10x1.25".into();
        t.apply_table();
        assert_eq!(t.diameter.value, 8.75);
    }

    #[test]
    fn inch_sizes() {
        let mut s = HoleSpec::new(String::new());
        s.standard = HoleStandard::Ansi;
        s.hole_type = HoleType::Clearance;
        s.apply_table();
        // A size from the metric list falls back to the inch default.
        assert_eq!(s.size, "1/4");
        assert!((s.diameter.value - 0.266 * 25.4).abs() < 1e-9);
        s.fit = Fit::Close;
        s.size = "#10".into();
        s.apply_table();
        assert!((s.diameter.value - 0.196 * 25.4).abs() < 1e-9);
        assert_eq!(s.csink_angle.value, 82.0);
        // The depths are in inches too (a new spec's 10 mm is 0.394 in).
        assert_eq!(s.depth.expr, "0.394 in");
        assert!((s.depth.value - 0.394 * 25.4).abs() < 1e-9);
        assert!(s.tapped_depth.expr.ends_with(" in"), "{}", s.tapped_depth.expr);
        s.end = HoleEnd::ThroughAll;
        assert_eq!(s.callout(), "Ø 0.196 in THRU");
        let mut t = HoleSpec::new(String::new());
        t.standard = HoleStandard::Ansi;
        t.hole_type = HoleType::Tapped;
        t.apply_table();
        assert_eq!(t.pitch, "1/4-20 UNC");
        assert!((t.diameter.value - 0.201 * 25.4).abs() < 1e-9);
        t.pitch = "1/4-28 UNF".into();
        t.apply_table();
        assert!((t.diameter.value - 0.213 * 25.4).abs() < 1e-9);
    }

    #[test]
    fn tolerances_and_pem() {
        // PS15.8: tolerances in the callout.
        let mut s = HoleSpec::new(String::new());
        s.diameter = Length::mm(5.3);
        s.end = HoleEnd::ThroughAll;
        s.diameter_tol = Tolerance { kind: ToleranceType::Symmetrical, upper: 0.1, lower: 0.1, precision: 2 };
        assert_eq!(s.callout(), "Ø 5.3 mm ±0.10 THRU");
        s.diameter_tol = Tolerance { kind: ToleranceType::Deviation, upper: 0.1, lower: 0.05, precision: 2 };
        assert_eq!(s.callout(), "Ø 5.3 mm +0.10/−0.05 THRU");
        s.diameter_tol = Tolerance { kind: ToleranceType::Limits, upper: 0.1, lower: 0.1, precision: 2 };
        assert_eq!(s.callout(), "Ø 5.40/5.20 mm THRU");
        s.end = HoleEnd::Blind;
        s.diameter_tol = Tolerance::default();
        s.depth = Length::mm(12.0);
        s.depth_tol = Tolerance { kind: ToleranceType::Max, ..Tolerance::default() };
        assert_eq!(s.callout(), "Ø 5.3 mm ↧ 12 mm MAX");
        // P3.11: the counterbore's and the countersink's tolerances follow their values.
        let mut c = HoleSpec::new(String::new());
        c.end = HoleEnd::ThroughAll;
        c.style = HoleStyle::Counterbore;
        c.cbore_diameter = Length::mm(10.0);
        c.cbore_depth = Length::mm(4.0);
        assert_eq!(c.callout(), "Ø 5 mm THRU | ⌴Ø 10 mm ↧ 4 mm");
        c.cbore_diameter_tol = Tolerance { kind: ToleranceType::Deviation, upper: 0.2, lower: 0.0, precision: 1 };
        c.cbore_depth_tol = Tolerance { kind: ToleranceType::Symmetrical, upper: 0.1, lower: 0.1, precision: 2 };
        assert_eq!(c.callout(), "Ø 5 mm THRU | ⌴Ø 10 mm +0.2/−0.0 ↧ 4 mm ±0.10");
        c.style = HoleStyle::Countersink;
        c.csink_diameter = Length::mm(10.0);
        c.csink_angle = Length::deg(90.0);
        c.csink_diameter_tol = Tolerance { kind: ToleranceType::Limits, upper: 0.1, lower: 0.1, precision: 2 };
        c.csink_angle_tol = Tolerance { kind: ToleranceType::Symmetrical, upper: 1.0, lower: 1.0, precision: 1 };
        assert_eq!(c.callout(), "Ø 5 mm THRU | ⌵Ø 10.10/9.90 mm X 90° ±1.0");
        c.csink_angle_tol.kind = ToleranceType::Limits;
        assert_eq!(c.callout(), "Ø 5 mm THRU | ⌵Ø 10.10/9.90 mm X 91.0°/89.0°");
        for w in StyleTolerance::ALL {
            c.style_tol_mut(w).kind = ToleranceType::None;
            assert_eq!(c.style_tol(w).kind, ToleranceType::None);
        }
        assert_eq!(c.callout(), "Ø 5 mm THRU | ⌵Ø 10 mm X 90°");
        // PS15.5: a PEM® CLS-M4 nut's mounting hole is Ø5.41 +0.08/−0 (PennEngineering bulletin
        // CL); an FH-M4 stud's Ø4.
        let mut p = HoleSpec::new(String::new());
        p.hole_type = HoleType::Pem;
        p.size = "M4".into();
        p.apply_table();
        assert_eq!(p.diameter.value, 5.41);
        assert_eq!(p.diameter_tol.kind, ToleranceType::Deviation);
        p.end = HoleEnd::ThroughAll;
        assert_eq!(p.callout(), "Ø 5.41 mm +0.08/−0.00 THRU");
        p.pem = PemType::Stud;
        p.apply_table();
        assert_eq!(p.diameter.value, 4.0);
        // Not on the Inch tab.
        p.standard = HoleStandard::Ansi;
        p.apply_table();
        assert_eq!(p.hole_type, HoleType::Drilled);
    }

    #[test]
    fn sections() {
        // A counterbore Ø10 × 4 over Ø5, 20 deep with a 118° point, 1 mm above the start.
        let mut s = HoleSpec::new(String::new());
        s.style = HoleStyle::Counterbore;
        s.diameter = Length::mm(5.0);
        s.cbore_diameter = Length::mm(10.0);
        s.cbore_depth = Length::mm(4.0);
        let tip = 2.5 / (59.0f64.to_radians()).tan();
        assert_eq!(
            s.section(20.0, false, 1.0),
            vec![(0.0, -1.0), (5.0, -1.0), (5.0, 4.0), (2.5, 4.0), (2.5, 20.0), (0.0, 20.0 + tip)]
        );
        assert_eq!(s.section(20.0, true, 1.0).last(), Some(&(0.0, 20.0)));
        s.style = HoleStyle::Countersink;
        s.csink_angle = Length::deg(90.0);
        s.csink_diameter = Length::mm(10.0);
        // A 90° countersink from Ø10 to Ø5 is 2.5 deep.
        assert!((s.csink_depth() - 2.5).abs() < 1e-12);
    }
}
