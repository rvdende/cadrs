//! Cutting tools and the tool library.
//!
//! A [`Tool`] is either a **cutter** (end, ball, bull, chamfer and taper mills, drills and taps,
//! for mills and routers) or a **beam** (a laser, a plasma torch or a waterjet nozzle) whose
//! "diameter" is its kerf. Its cutting data ([`CuttingData`]) seeds the speeds of the toolpaths
//! that use it. Mills can carry a holder and an arbor ([`Profile`], a revolved outline of X/Z
//! points) for collision checks and the simulation.
//!
//! The [`ToolLibrary`] is the studio's collection of tools, holders and arbors: tools are added
//! to it as they are created, and it exports to and imports from JSON.

use serde::{Deserialize, Serialize};

use crate::machine::MachineKind;

/// The family a tool belongs to, as the tool dialog's "Machining type" filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MachiningType {
    Milling,
    HoleMaking,
    Cutting,
}

impl MachiningType {
    pub const ALL: [MachiningType; 3] = [
        MachiningType::Milling,
        MachiningType::HoleMaking,
        MachiningType::Cutting,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MachiningType::Milling => "Milling",
            MachiningType::HoleMaking => "Hole making",
            MachiningType::Cutting => "Thermal and waterjet cutting",
        }
    }
}

/// What kind of cutter or beam a tool is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolKind {
    EndMill,
    BallMill,
    BullMill,
    ChamferMill,
    TaperMill,
    FaceMill,
    Drill,
    Tap,
    /// A single-form thread mill: cuts a thread with a helix, a pitch per turn.
    ThreadMill,
    Laser,
    Plasma,
    Waterjet,
}

impl ToolKind {
    pub const ALL: [ToolKind; 12] = [
        ToolKind::EndMill,
        ToolKind::BallMill,
        ToolKind::BullMill,
        ToolKind::ChamferMill,
        ToolKind::TaperMill,
        ToolKind::FaceMill,
        ToolKind::Drill,
        ToolKind::Tap,
        ToolKind::ThreadMill,
        ToolKind::Laser,
        ToolKind::Plasma,
        ToolKind::Waterjet,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::EndMill => "End mill",
            ToolKind::BallMill => "Ball mill",
            ToolKind::BullMill => "Bull mill",
            ToolKind::ChamferMill => "Chamfer mill / V-bit",
            ToolKind::TaperMill => "Taper mill",
            ToolKind::FaceMill => "Face mill",
            ToolKind::Drill => "Drill",
            ToolKind::Tap => "Thread tap",
            ToolKind::ThreadMill => "Thread mill",
            ToolKind::Laser => "Laser",
            ToolKind::Plasma => "Plasma torch",
            ToolKind::Waterjet => "Waterjet nozzle",
        }
    }

    /// The icon of the kind (icon-rs).
    pub fn icon(self) -> &'static str {
        match self {
            ToolKind::Drill | ToolKind::Tap | ToolKind::ThreadMill => "cam-drill",
            ToolKind::Laser => "cam-laser",
            ToolKind::Plasma => "cam-plasma",
            ToolKind::Waterjet => "cam-waterjet",
            _ => "cam-tool",
        }
    }

    pub fn machining(self) -> MachiningType {
        match self {
            ToolKind::Drill | ToolKind::Tap | ToolKind::ThreadMill => MachiningType::HoleMaking,
            ToolKind::Laser | ToolKind::Plasma | ToolKind::Waterjet => MachiningType::Cutting,
            _ => MachiningType::Milling,
        }
    }

    /// A beam: cuts through along a line with a kerf, no depth passes.
    pub fn is_beam(self) -> bool {
        self.machining() == MachiningType::Cutting
    }

    /// The machines that can hold it.
    pub fn fits(self, machine: MachineKind) -> bool {
        match self {
            ToolKind::Laser => machine == MachineKind::Laser,
            ToolKind::Plasma => machine == MachineKind::Plasma,
            ToolKind::Waterjet => machine == MachineKind::Waterjet,
            _ => matches!(machine, MachineKind::Mill | MachineKind::Router),
        }
    }

    /// The kinds a machine of `machine` takes, in list order.
    pub fn for_machine(machine: MachineKind) -> Vec<ToolKind> {
        ToolKind::ALL
            .into_iter()
            .filter(|k| k.fits(machine))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Hand {
    #[default]
    Right,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Unit {
    #[default]
    Mm,
    Inch,
}

impl Unit {
    pub fn label(self) -> &'static str {
        match self {
            Unit::Mm => "mm",
            Unit::Inch => "in",
        }
    }

    /// mm per unit.
    pub fn mm(self) -> f64 {
        match self {
            Unit::Mm => 1.0,
            Unit::Inch => 25.4,
        }
    }
}

/// Speeds and feeds a tool suggests for the toolpaths that use it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CuttingData {
    /// Spindle speed (rpm); unused for beams.
    pub spindle_rpm: f64,
    /// Cutting feed (mm/min).
    pub feed: f64,
    /// Plunge feed (mm/min).
    pub plunge_feed: f64,
    /// Depth of cut per pass (mm).
    pub step_down: f64,
    /// Width of cut (mm).
    pub step_over: f64,
    /// The work material these were worked out for ([`crate::feeds::cutting_data`]); `None`:
    /// set by hand.
    #[serde(default)]
    pub material: Option<crate::feeds::WorkMaterial>,
}

impl Default for CuttingData {
    fn default() -> Self {
        CuttingData {
            spindle_rpm: 12_000.0,
            feed: 1_000.0,
            plunge_feed: 300.0,
            step_down: 1.0,
            step_over: 2.4,
            material: None,
        }
    }
}

/// What a beam tool (laser, plasma, waterjet) needs beyond its kerf.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BeamData {
    /// Power while cutting (0–100 % of the machine's maximum; for plasma, the amperage dial is
    /// set on the power source and this is ignored).
    pub power: f64,
    /// Power at standstill with dynamic (M4) laser power, as a percentage.
    pub min_power: f64,
    /// Pierce: the time the beam dwells to break through (s).
    pub pierce_delay: f64,
    /// The torch's height while piercing (mm above the material; plasma).
    pub pierce_height: f64,
    /// The torch's height while cutting (mm above the material; plasma and waterjet).
    pub cut_height: f64,
    /// Torch height control on during cuts (plasma).
    pub thc: bool,
    /// The material number in the control's cut table (QtPlasmaC `M190 P…`); 0 leaves the
    /// material the operator picked.
    #[serde(default)]
    pub material: u32,
    /// Air assist on while cutting (laser).
    pub air_assist: bool,
    /// Passes per cut (a thin laser pass at a time).
    pub passes: u32,
}

impl Default for BeamData {
    fn default() -> Self {
        BeamData {
            power: 80.0,
            min_power: 0.0,
            pierce_delay: 0.0,
            pierce_height: 3.8,
            cut_height: 1.5,
            thc: true,
            material: 0,
            air_assist: true,
            passes: 1,
        }
    }
}

/// A revolved outline of points `[radius, height]` from the tool's gauge line up (a holder or
/// an arbor).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub points: Vec<[f64; 2]>,
}

impl Profile {
    /// A plain cylinder of `diameter` and `length`.
    pub fn cylinder(name: impl Into<String>, diameter: f64, length: f64) -> Self {
        Profile {
            name: name.into(),
            points: vec![[diameter / 2.0, 0.0], [diameter / 2.0, length]],
        }
    }

    /// Its largest radius.
    pub fn max_radius(&self) -> f64 {
        self.points.iter().map(|p| p[0]).fold(0.0, f64::max)
    }

    /// Its length along the tool axis.
    pub fn length(&self) -> f64 {
        self.points.iter().map(|p| p[1]).fold(0.0, f64::max)
    }
}

/// A cutting tool or beam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub kind: ToolKind,
    /// The unit its sizes are shown in (they are stored in mm).
    #[serde(default)]
    pub unit: Unit,
    #[serde(default)]
    pub hand: Hand,
    /// Cutting diameter (mm); a beam's kerf width.
    pub diameter: f64,
    /// Corner radius of a bull mill (mm).
    #[serde(default)]
    pub corner_radius: f64,
    /// Included angle (degrees): a chamfer mill's or V-bit's, a drill's point, twice a taper
    /// mill's side angle.
    #[serde(default = "default_angle")]
    pub angle: f64,
    /// Diameter of a chamfer mill's or taper mill's flat tip (mm).
    #[serde(default)]
    pub tip_diameter: f64,
    /// Length of the flutes (mm).
    #[serde(default = "default_flute")]
    pub cutting_length: f64,
    /// Stick-out from the holder (mm).
    #[serde(default = "default_overall")]
    pub overall_length: f64,
    #[serde(default = "default_flutes")]
    pub flutes: u32,
    /// Cuts at its centre, so it can plunge straight down.
    #[serde(default = "yes")]
    pub center_cutting: bool,
    /// Thread pitch (mm per turn) of a tap.
    #[serde(default)]
    pub pitch: f64,
    #[serde(default)]
    pub holder: Option<Profile>,
    #[serde(default)]
    pub arbor: Option<Profile>,
    #[serde(default)]
    pub cutting: CuttingData,
    #[serde(default)]
    pub beam: BeamData,
}

fn default_angle() -> f64 {
    90.0
}
fn default_flute() -> f64 {
    15.0
}
fn default_overall() -> f64 {
    40.0
}
fn default_flutes() -> u32 {
    2
}
fn yes() -> bool {
    true
}

impl Tool {
    /// A tool of `kind` with sizes and cutting data that suit it.
    pub fn new(kind: ToolKind, diameter: f64) -> Tool {
        let d = diameter.max(0.01);
        let mut t = Tool {
            name: String::new(),
            kind,
            unit: Unit::Mm,
            hand: Hand::Right,
            diameter: d,
            corner_radius: 0.0,
            angle: 90.0,
            tip_diameter: 0.0,
            cutting_length: (d * 3.0).max(5.0),
            overall_length: (d * 6.0).max(30.0),
            flutes: 2,
            center_cutting: true,
            pitch: 0.0,
            holder: None,
            arbor: None,
            cutting: CuttingData {
                spindle_rpm: 10_000.0,
                feed: 1_200.0,
                plunge_feed: 300.0,
                step_down: (d * 0.5).max(0.1),
                step_over: d * 0.4,
                material: None,
            },
            beam: BeamData::default(),
        };
        match kind {
            ToolKind::BullMill => t.corner_radius = d * 0.1,
            ToolKind::ChamferMill => t.angle = 90.0,
            ToolKind::TaperMill => {
                t.angle = 10.0;
                t.tip_diameter = d * 0.5;
            }
            ToolKind::FaceMill => {
                t.flutes = 4;
                t.center_cutting = false;
                t.cutting.step_over = d * 0.7;
                t.cutting.spindle_rpm = 8_000.0;
            }
            ToolKind::Drill => {
                t.angle = 118.0;
                t.cutting.spindle_rpm = 3_000.0;
                t.cutting.feed = 150.0;
                t.cutting.plunge_feed = 150.0;
                t.cutting.step_down = d;
            }
            ToolKind::Tap => {
                t.pitch = metric_coarse_pitch(d);
                t.cutting.spindle_rpm = 500.0;
                t.cutting.feed = t.pitch * 500.0;
                t.cutting.plunge_feed = t.cutting.feed;
            }
            ToolKind::ThreadMill => {
                // A cutter for the coarse thread a size up (a D6 cuts M8 and bigger).
                t.pitch = metric_coarse_pitch(d + 2.0);
                t.flutes = 3;
                t.cutting.spindle_rpm = 8_000.0;
                t.cutting.feed = 400.0;
                t.cutting.plunge_feed = 200.0;
            }
            ToolKind::Laser => {
                t.cutting.feed = 600.0;
                t.cutting.plunge_feed = 600.0;
                t.cutting.spindle_rpm = 0.0;
                t.beam.thc = false;
                t.beam.pierce_delay = 0.0;
                t.beam.cut_height = 0.0;
                t.beam.pierce_height = 0.0;
            }
            ToolKind::Plasma => {
                t.cutting.feed = 2_500.0;
                t.cutting.plunge_feed = 1_000.0;
                t.cutting.spindle_rpm = 0.0;
                t.beam.power = 100.0;
                t.beam.pierce_delay = 0.5;
                t.beam.air_assist = false;
            }
            ToolKind::Waterjet => {
                t.cutting.feed = 300.0;
                t.cutting.plunge_feed = 300.0;
                t.cutting.spindle_rpm = 0.0;
                t.beam.power = 100.0;
                t.beam.pierce_delay = 2.0;
                t.beam.thc = false;
                t.beam.air_assist = false;
            }
            _ => {}
        }
        t.name = t.default_name();
        t
    }

    /// "D6 end mill", "D1.5 plasma torch".
    pub fn default_name(&self) -> String {
        let size = if self.kind.is_beam() {
            format!("Kerf {}", fmt_mm(self.diameter))
        } else if self.kind == ToolKind::BullMill {
            format!("D{} R{}", fmt_mm(self.diameter), fmt_mm(self.corner_radius))
        } else if self.kind == ToolKind::Tap {
            format!("M{}×{}", fmt_mm(self.diameter), fmt_mm(self.pitch))
        } else if self.kind == ToolKind::ThreadMill {
            format!("D{} P{}", fmt_mm(self.diameter), fmt_mm(self.pitch))
        } else {
            format!("D{}", fmt_mm(self.diameter))
        };
        format!("{size} {}", self.kind.label().to_lowercase())
    }

    /// The radius the tool's centre stays from a wall it cuts: half its diameter (its kerf).
    pub fn radius(&self) -> f64 {
        self.diameter / 2.0
    }

    pub fn is_beam(&self) -> bool {
        self.kind.is_beam()
    }

    /// How high above its tip the cutter's surface is at distance `r` from its axis (`None`
    /// outside the cutter). This is the shape the drop-cutter and the stock simulation use.
    pub fn height_at(&self, r: f64) -> Option<f64> {
        let rad = self.radius();
        if r > rad + 1e-12 {
            return None;
        }
        let r = r.min(rad);
        Some(match self.kind {
            ToolKind::EndMill | ToolKind::FaceMill => 0.0,
            ToolKind::BallMill => rad - (rad * rad - r * r).max(0.0).sqrt(),
            ToolKind::BullMill => {
                let rc = self.corner_radius.clamp(0.0, rad);
                let flat = rad - rc;
                if r <= flat {
                    0.0
                } else {
                    let d = r - flat;
                    rc - (rc * rc - d * d).max(0.0).sqrt()
                }
            }
            ToolKind::ChamferMill | ToolKind::TaperMill | ToolKind::Drill => {
                let tip = self.tip_diameter / 2.0;
                if r <= tip {
                    0.0
                } else {
                    let half = (self.angle.clamp(1.0, 179.0) / 2.0).to_radians();
                    (r - tip) / half.tan()
                }
            }
            ToolKind::Tap | ToolKind::ThreadMill => 0.0,
            ToolKind::Laser | ToolKind::Plasma | ToolKind::Waterjet => 0.0,
        })
    }

    /// The depth a chamfer mill or V-bit must go to cut a chamfer `width` wide (measured
    /// horizontally from the edge), touching with the middle of its flank.
    pub fn chamfer_depth(&self, width: f64) -> f64 {
        let half = (self.angle.clamp(1.0, 179.0) / 2.0).to_radians();
        width / half.tan()
    }

    /// The horizontal distance of the flank point at `depth` above the tip from the axis.
    pub fn radius_at_height(&self, h: f64) -> f64 {
        match self.kind {
            ToolKind::ChamferMill | ToolKind::TaperMill | ToolKind::Drill => {
                let half = (self.angle.clamp(1.0, 179.0) / 2.0).to_radians();
                (self.tip_diameter / 2.0 + h * half.tan()).min(self.radius())
            }
            ToolKind::BallMill => {
                let r = self.radius();
                if h >= r {
                    r
                } else {
                    (r * r - (r - h) * (r - h)).max(0.0).sqrt()
                }
            }
            _ => self.radius(),
        }
    }

    /// Problems that make the tool unusable.
    pub fn check(&self) -> Result<(), String> {
        if self.diameter.is_nan() || self.diameter <= 0.0 {
            return Err("The diameter must be more than 0".into());
        }
        if self.kind == ToolKind::BullMill && self.corner_radius * 2.0 > self.diameter + 1e-9 {
            return Err("The corner radius must be at most half the diameter".into());
        }
        if matches!(self.kind, ToolKind::ChamferMill | ToolKind::TaperMill)
            && self.tip_diameter >= self.diameter
        {
            return Err("The tip must be narrower than the diameter".into());
        }
        if !self.is_beam() && self.cutting_length <= 0.0 {
            return Err("The cutting length must be more than 0".into());
        }
        if self.cutting.feed <= 0.0 {
            return Err("The feed rate must be more than 0".into());
        }
        Ok(())
    }
}

/// ISO metric coarse pitch for a tap diameter (the nearest standard size).
pub fn metric_coarse_pitch(d: f64) -> f64 {
    const TABLE: [(f64, f64); 12] = [
        (2.0, 0.4),
        (2.5, 0.45),
        (3.0, 0.5),
        (4.0, 0.7),
        (5.0, 0.8),
        (6.0, 1.0),
        (8.0, 1.25),
        (10.0, 1.5),
        (12.0, 1.75),
        (16.0, 2.0),
        (20.0, 2.5),
        (24.0, 3.0),
    ];
    TABLE
        .iter()
        .min_by(|a, b| (a.0 - d).abs().total_cmp(&(b.0 - d).abs()))
        .map(|t| t.1)
        .unwrap_or(1.0)
}

/// "6", "1.5", "0.25": a length without trailing zeros.
pub fn fmt_mm(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// The studio's tools, holders, arbors and toolpath templates.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolLibrary {
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub holders: Vec<Profile>,
    #[serde(default)]
    pub arbors: Vec<Profile>,
    #[serde(default)]
    pub templates: Vec<Template>,
}

/// A toolpath's settings saved to start others from: everything but what belongs to its part
/// (its drive geometry, its repeat pattern's places) and its tool (its speeds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub params: crate::strategy::Params,
    /// The diameter of the tool it was saved with (mm): its step over, step down and lead
    /// radius scale to another tool's.
    #[serde(default)]
    pub diameter: f64,
}

impl Template {
    pub fn new(name: &str, params: &crate::strategy::Params, tool: &Tool) -> Template {
        let mut params = params.clone();
        params.drive = Default::default();
        params.repeat = Default::default();
        params.posting.comment.clear();
        Template {
            name: name.to_string(),
            params,
            diameter: tool.diameter,
        }
    }

    /// `params` with this template's settings for `tool`, keeping what they machine: the
    /// steps and leads scaled to its diameter, its own speeds.
    pub fn apply_to(
        &self,
        params: &mut crate::strategy::Params,
        tool: &Tool,
        machine: crate::machine::MachineKind,
    ) {
        let drive = std::mem::take(&mut params.drive);
        let repeat = std::mem::take(&mut params.repeat);
        *params = self.params.clone();
        params.drive = drive;
        params.repeat = repeat;
        if self.diameter > 0.0 && tool.diameter > 0.0 {
            let k = tool.diameter / self.diameter;
            params.step_over *= k;
            params.step_down *= k;
            params.first_step_down *= k;
            params.links.lead_radius *= k;
        }
        params.speeds = crate::strategy::Params::new(params.pattern, tool, machine).speeds;
    }
}

/// The file format's name and version, checked on import.
pub const LIBRARY_FORMAT: &str = "cadrs-cam-tool-library";
pub const LIBRARY_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    library: ToolLibrary,
}

impl ToolLibrary {
    /// A starter library: common end mills, a ball mill, a V-bit, drills, and a laser, plasma
    /// and waterjet process each.
    pub fn starter() -> ToolLibrary {
        let mut tools = Vec::new();
        for d in [3.0, 6.0, 10.0] {
            tools.push(Tool::new(ToolKind::EndMill, d));
        }
        tools.push(Tool::new(ToolKind::BallMill, 6.0));
        let mut bull = Tool::new(ToolKind::BullMill, 10.0);
        bull.corner_radius = 1.0;
        bull.name = bull.default_name();
        tools.push(bull);
        tools.push(Tool::new(ToolKind::ChamferMill, 6.0));
        tools.push(Tool::new(ToolKind::FaceMill, 50.0));
        for d in [3.0, 5.0, 6.8, 8.5] {
            tools.push(Tool::new(ToolKind::Drill, d));
        }
        tools.push(Tool::new(ToolKind::Tap, 8.0));
        tools.push(Tool::new(ToolKind::ThreadMill, 6.0));
        tools.push(Tool::new(ToolKind::Laser, 0.15));
        tools.push(Tool::new(ToolKind::Plasma, 1.5));
        tools.push(Tool::new(ToolKind::Waterjet, 0.9));
        ToolLibrary {
            tools,
            holders: vec![Profile {
                name: "ER20 collet chuck".into(),
                points: vec![[16.0, 0.0], [16.0, 30.0], [25.0, 40.0], [25.0, 60.0]],
            }],
            arbors: Vec::new(),
            templates: Vec::new(),
        }
    }

    /// Saves `t`, over a template of the same name.
    pub fn save_template(&mut self, t: Template) {
        match self.templates.iter_mut().find(|x| x.name == t.name) {
            Some(x) => *x = t,
            None => self.templates.push(t),
        }
    }

    /// Adds the tool unless an equal one is in the library.
    pub fn add(&mut self, tool: &Tool) {
        if !self.tools.iter().any(|t| t == tool) {
            self.tools.push(tool.clone());
        }
    }

    pub fn add_holder(&mut self, p: &Profile) {
        if !self.holders.iter().any(|h| h == p) {
            self.holders.push(p.clone());
        }
    }

    pub fn add_arbor(&mut self, p: &Profile) {
        if !self.arbors.iter().any(|h| h == p) {
            self.arbors.push(p.clone());
        }
    }

    /// The tools of a machining type and kind (`None`: any).
    pub fn filtered(
        &self,
        machining: Option<MachiningType>,
        kind: Option<ToolKind>,
    ) -> Vec<(usize, &Tool)> {
        self.tools
            .iter()
            .enumerate()
            .filter(|(_, t)| machining.is_none_or(|m| t.kind.machining() == m))
            .filter(|(_, t)| kind.is_none_or(|k| t.kind == k))
            .collect()
    }

    /// The library as a JSON file.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&LibraryFile {
            format: LIBRARY_FORMAT.into(),
            version: LIBRARY_VERSION,
            library: self.clone(),
        })
        .expect("a tool library serialises")
    }

    /// Reads a library written by [`ToolLibrary::to_json`].
    pub fn from_json(s: &str) -> Result<ToolLibrary, String> {
        let f: LibraryFile =
            serde_json::from_str(s).map_err(|e| format!("Not a tool library: {e}"))?;
        if f.format != LIBRARY_FORMAT {
            return Err(format!("Not a tool library ({})", f.format));
        }
        if f.version > LIBRARY_VERSION {
            return Err(format!(
                "The tool library is version {}, newer than this cadrs reads ({LIBRARY_VERSION})",
                f.version
            ));
        }
        Ok(f.library)
    }

    /// Adds every tool, holder and arbor of `other` that isn't here yet; returns how many tools
    /// were new.
    pub fn merge(&mut self, other: &ToolLibrary) -> usize {
        let before = self.tools.len();
        for t in &other.tools {
            self.add(t);
        }
        for h in &other.holders {
            self.add_holder(h);
        }
        for a in &other.arbors {
            self.add_arbor(a);
        }
        for t in &other.templates {
            if !self.templates.iter().any(|x| x.name == t.name) {
                self.templates.push(t.clone());
            }
        }
        self.tools.len() - before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        let ball = Tool::new(ToolKind::BallMill, 10.0);
        assert_eq!(ball.height_at(0.0), Some(0.0));
        assert!((ball.height_at(5.0).unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(ball.height_at(5.1), None);
        let mut bull = Tool::new(ToolKind::BullMill, 10.0);
        bull.corner_radius = 2.0;
        assert_eq!(bull.height_at(3.0), Some(0.0));
        assert!((bull.height_at(5.0).unwrap() - 2.0).abs() < 1e-9);
        let v = Tool::new(ToolKind::ChamferMill, 10.0);
        // 90° included: 45° flanks rise 1:1.
        assert!((v.height_at(4.0).unwrap() - 4.0).abs() < 1e-9);
        assert!((v.chamfer_depth(1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn names() {
        assert_eq!(Tool::new(ToolKind::EndMill, 6.0).name, "D6 end mill");
        assert_eq!(
            Tool::new(ToolKind::Plasma, 1.5).name,
            "Kerf 1.5 plasma torch"
        );
        assert_eq!(Tool::new(ToolKind::Tap, 8.0).name, "M8×1.25 thread tap");
        assert_eq!(fmt_mm(0.25), "0.25");
        assert_eq!(fmt_mm(10.0), "10");
    }

    #[test]
    fn library_round_trip() {
        let lib = ToolLibrary::starter();
        let back = ToolLibrary::from_json(&lib.to_json()).unwrap();
        assert_eq!(lib, back);
        assert!(ToolLibrary::from_json("{\"format\":\"x\",\"version\":1}").is_err());
        let mut a = ToolLibrary::default();
        assert_eq!(a.merge(&lib), lib.tools.len());
        assert_eq!(a.merge(&lib), 0);
    }

    #[test]
    fn kinds_fit_machines() {
        assert!(ToolKind::for_machine(MachineKind::Laser) == vec![ToolKind::Laser]);
        assert!(ToolKind::for_machine(MachineKind::Router).contains(&ToolKind::Drill));
        assert!(Tool::new(ToolKind::EndMill, 6.0).check().is_ok());
        let mut bad = Tool::new(ToolKind::BullMill, 6.0);
        bad.corner_radius = 4.0;
        assert!(bad.check().is_err());
    }
}
