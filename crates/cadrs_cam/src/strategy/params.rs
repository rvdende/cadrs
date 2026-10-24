//! A toolpath's settings, as its dialog's tabs set them: the type and pattern, the sides,
//! heights, links, tabs, speeds and the rest ([`Params`]).

use serde::{Deserialize, Serialize};

use crate::machine::MachineKind;
use crate::tool::{Tool, ToolKind};
use crate::toolpath::{Coolant, CycleKind, Step, Toolpath};

use super::collision;

/// What a toolpath machines (the dialog's Type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Type {
    /// Wireframe drive edges (no machining faces needed).
    Edges,
    /// Selected faces.
    Faces,
    /// Whole bodies.
    Bodies,
    /// Holes.
    Holes,
}

impl Type {
    pub const ALL: [Type; 4] = [Type::Edges, Type::Faces, Type::Bodies, Type::Holes];

    pub fn label(self) -> &'static str {
        match self {
            Type::Edges => "Edges",
            Type::Faces => "Faces",
            Type::Bodies => "Bodies",
            Type::Holes => "Holes",
        }
    }
}

/// How a toolpath cuts (the dialog's Pattern).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pattern {
    /// Flat passes over the stock's top (or selected flat faces).
    Face,
    /// Clears pockets and around bosses level by level (2.5 axis).
    Rough2D,
    /// Follows drive edges or the part's outline, with depth passes, tabs and leads.
    Profile2D,
    /// Breaks edges with a chamfer mill.
    Chamfer2D,
    /// Follows drive edges at a depth.
    Engrave,
    /// Finishes horizontal floors.
    Flatlands,
    /// Removes the bulk of the stock around any shape, level by level.
    Rough3D,
    /// Roughing at a constant radial engagement: deep, light, fast.
    Adaptive,
    /// Raster passes over the surfaces (drop cutter).
    Parallel,
    /// Waterline passes round the steep walls.
    ConstantZ,
    /// Rings offset across the shallow surfaces: an even step over along them.
    Scallop,
    /// One spiral out from the middle over the surfaces.
    Spiral,
    /// Along the concave corners and fillets tighter than the tool.
    Pencil,
    /// Drilling, peck drilling and tapping cycles.
    Drill,
    /// Helical milling of holes with an end mill.
    Bore,
    /// Threads cut with a thread mill on a helix.
    ThreadMill,
    /// Through-cutting with a beam, kerf compensated.
    Cut,
    /// Marking along edges with a beam (no compensation).
    Etch,
    /// Laser hatching of faces.
    Fill,
}

impl Pattern {
    pub const ALL: [Pattern; 19] = [
        Pattern::Face,
        Pattern::Rough2D,
        Pattern::Profile2D,
        Pattern::Chamfer2D,
        Pattern::Engrave,
        Pattern::Flatlands,
        Pattern::Rough3D,
        Pattern::Adaptive,
        Pattern::Parallel,
        Pattern::ConstantZ,
        Pattern::Scallop,
        Pattern::Spiral,
        Pattern::Pencil,
        Pattern::Drill,
        Pattern::Bore,
        Pattern::ThreadMill,
        Pattern::Cut,
        Pattern::Etch,
        Pattern::Fill,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Pattern::Face => "Face",
            Pattern::Rough2D => "2-axis rough",
            Pattern::Profile2D => "2-axis profile",
            Pattern::Chamfer2D => "2-axis chamfer",
            Pattern::Engrave => "Engrave",
            Pattern::Flatlands => "Flatlands",
            Pattern::Rough3D => "Rough",
            Pattern::Adaptive => "Adaptive clearing",
            Pattern::Parallel => "Parallel",
            Pattern::ConstantZ => "Constant Z",
            Pattern::Scallop => "Scallop",
            Pattern::Spiral => "Spiral",
            Pattern::Pencil => "Pencil",
            Pattern::ThreadMill => "Thread mill",
            Pattern::Drill => "Drill",
            Pattern::Bore => "Bore",
            Pattern::Cut => "Cut",
            Pattern::Etch => "Etch",
            Pattern::Fill => "Fill",
        }
    }

    /// The toolpath's default name ("2-axis profile 1" is the caller's job).
    pub fn type_of(self) -> Type {
        match self {
            Pattern::Profile2D
            | Pattern::Chamfer2D
            | Pattern::Engrave
            | Pattern::Cut
            | Pattern::Etch => Type::Edges,
            Pattern::Face | Pattern::Rough2D | Pattern::Flatlands | Pattern::Fill => Type::Faces,
            Pattern::Rough3D
            | Pattern::Adaptive
            | Pattern::Parallel
            | Pattern::ConstantZ
            | Pattern::Scallop
            | Pattern::Spiral
            | Pattern::Pencil => Type::Bodies,
            Pattern::Drill | Pattern::Bore | Pattern::ThreadMill => Type::Holes,
        }
    }

    /// The icon for the tree (icon-rs).
    pub fn icon(self) -> &'static str {
        match self {
            Pattern::Face => "cam-face",
            Pattern::Rough2D | Pattern::Rough3D => "cam-pocket",
            Pattern::Adaptive => "cam-adaptive",
            Pattern::Profile2D => "cam-profile",
            Pattern::Chamfer2D => "cam-chamfer",
            Pattern::Engrave => "cam-engrave",
            Pattern::Flatlands => "cam-flatlands",
            Pattern::Parallel => "cam-parallel",
            Pattern::ConstantZ => "cam-waterline",
            Pattern::Scallop => "cam-scallop",
            Pattern::Spiral => "cam-spiral",
            Pattern::Pencil => "cam-pencil",
            Pattern::Drill => "cam-drill-cycle",
            Pattern::Bore => "cam-bore",
            Pattern::ThreadMill => "cam-thread",
            Pattern::Cut => "cam-cut-path",
            Pattern::Etch => "cam-etch",
            Pattern::Fill => "cam-fill",
        }
    }

    /// Whether `tool` on a `machine` can run it.
    pub fn allowed(self, tool: ToolKind, machine: MachineKind) -> bool {
        if !tool.fits(machine) {
            return false;
        }
        use ToolKind::*;
        match self {
            Pattern::Cut | Pattern::Etch => tool.is_beam(),
            Pattern::Fill => tool == Laser,
            Pattern::Drill => matches!(tool, Drill | Tap | ChamferMill),
            Pattern::Bore => matches!(tool, EndMill | BullMill | BallMill),
            Pattern::Chamfer2D => matches!(tool, ChamferMill),
            Pattern::Engrave => matches!(tool, ChamferMill | BallMill | EndMill | TaperMill),
            Pattern::Face => matches!(tool, FaceMill | EndMill | BullMill),
            Pattern::Rough2D
            | Pattern::Rough3D
            | Pattern::Adaptive
            | Pattern::Profile2D
            | Pattern::Flatlands => {
                matches!(tool, EndMill | BullMill | BallMill | FaceMill | TaperMill)
            }
            Pattern::Parallel | Pattern::ConstantZ | Pattern::Scallop | Pattern::Spiral => {
                matches!(tool, EndMill | BullMill | BallMill | TaperMill)
            }
            Pattern::Pencil => matches!(tool, BallMill | BullMill),
            Pattern::ThreadMill => tool == ThreadMill,
        }
    }

    /// The patterns of `kind` a tool and machine can run, in list order.
    pub fn available(kind: Type, tool: ToolKind, machine: MachineKind) -> Vec<Pattern> {
        Pattern::ALL
            .into_iter()
            .filter(|p| p.type_of() == kind && p.allowed(tool, machine))
            .collect()
    }
}

/// Which side of a contour the tool runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Side {
    /// Outside outer outlines, inside holes.
    #[default]
    Auto,
    Outside,
    Inside,
    /// On the line (no compensation).
    On,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Auto, Side::Outside, Side::Inside, Side::On];
    pub fn label(self) -> &'static str {
        match self {
            Side::Auto => "Automatic",
            Side::Outside => "Outside",
            Side::Inside => "Inside",
            Side::On => "On the line",
        }
    }
}

/// Where a profile's tool radius is taken off the part's outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Compensation {
    /// The toolpath is the tool's centre; the control moves it as written.
    #[default]
    Computer,
    /// The toolpath is the tool's centre, written with G41/G42: the operator trims the size
    /// on the machine with the tool's wear offset (D = its tool number).
    Wear,
}

impl Compensation {
    pub const ALL: [Compensation; 2] = [Compensation::Computer, Compensation::Wear];
    pub fn label(self) -> &'static str {
        match self {
            Compensation::Computer => "In computer",
            Compensation::Wear => "Wear (G41/G42)",
        }
    }
}

/// Sub-pattern of the roughing patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SubPattern {
    /// Rings offset from the walls.
    #[default]
    Offset,
    /// Parallel passes, then a ring round the walls.
    Parallel,
}

impl SubPattern {
    pub const ALL: [SubPattern; 2] = [SubPattern::Offset, SubPattern::Parallel];
    pub fn label(self) -> &'static str {
        match self {
            SubPattern::Offset => "Offset",
            SubPattern::Parallel => "Parallel",
        }
    }
}

/// Cutting direction: climb (down) milling keeps the material on the tool's right with a
/// clockwise spindle; conventional on its left. For a beam, "climb" keeps the part on the right
/// (a plasma torch's good side with a clockwise swirl).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Direction {
    #[default]
    Climb,
    Conventional,
}

impl Direction {
    pub const ALL: [Direction; 2] = [Direction::Climb, Direction::Conventional];
    pub fn label(self) -> &'static str {
        match self {
            Direction::Climb => "Climb",
            Direction::Conventional => "Conventional",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CuttingMethod {
    #[default]
    OneWay,
    Zigzag,
}

impl CuttingMethod {
    pub const ALL: [CuttingMethod; 2] = [CuttingMethod::OneWay, CuttingMethod::Zigzag];
    pub fn label(self) -> &'static str {
        match self {
            CuttingMethod::OneWay => "One way",
            CuttingMethod::Zigzag => "Zigzag",
        }
    }
}

/// Machine by levels (every region at one depth before the next) or by regions (each region to
/// the bottom before the next).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MachineBy {
    #[default]
    Levels,
    Regions,
}

impl MachineBy {
    pub const ALL: [MachineBy; 2] = [MachineBy::Levels, MachineBy::Regions];
    pub fn label(self) -> &'static str {
        match self {
            MachineBy::Levels => "Levels",
            MachineBy::Regions => "Regions",
        }
    }
}

/// What a height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HeightRef {
    StockTop,
    StockBottom,
    ModelTop,
    ModelBottom,
    /// The drive geometry's (selected edges' or faces') height; the model's when none.
    Selection,
    /// Absolute, in the setup's coordinates.
    Origin,
}

impl HeightRef {
    pub const ALL: [HeightRef; 6] = [
        HeightRef::StockTop,
        HeightRef::StockBottom,
        HeightRef::ModelTop,
        HeightRef::ModelBottom,
        HeightRef::Selection,
        HeightRef::Origin,
    ];
    pub fn label(self) -> &'static str {
        match self {
            HeightRef::StockTop => "Stock top",
            HeightRef::StockBottom => "Stock bottom",
            HeightRef::ModelTop => "Model top",
            HeightRef::ModelBottom => "Model bottom",
            HeightRef::Selection => "Selection",
            HeightRef::Origin => "Origin",
        }
    }
}

/// A height: a reference and an offset above it (mm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Height {
    pub from: HeightRef,
    pub offset: f64,
}

impl Height {
    pub fn new(from: HeightRef, offset: f64) -> Height {
        Height { from, offset }
    }
}

/// The Heights group.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Heights {
    /// Where cutting starts.
    pub top: Height,
    /// Where it ends.
    pub bottom: Height,
}

/// How the tool gets between cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkAction {
    /// The shortest move at feed, staying down.
    Direct,
    /// Up to the feed distance above the material, across, down.
    RetractToFeed,
    /// Up to the retract (rapid) distance above the stock.
    RetractToRapid,
    /// Up to the clearance height.
    RetractToClearance,
}

impl LinkAction {
    pub const ALL: [LinkAction; 4] = [
        LinkAction::Direct,
        LinkAction::RetractToFeed,
        LinkAction::RetractToRapid,
        LinkAction::RetractToClearance,
    ];
    pub fn label(self) -> &'static str {
        match self {
            LinkAction::Direct => "Direct",
            LinkAction::RetractToFeed => "Retract to feed distance",
            LinkAction::RetractToRapid => "Retract to rapid distance",
            LinkAction::RetractToClearance => "Retract to clearance area",
        }
    }
}

/// How the tool enters the material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Entry {
    /// Straight down at the plunge feed.
    Plunge,
    /// Down along the path at the ramp angle.
    #[default]
    Ramp,
}

impl Entry {
    pub const ALL: [Entry; 2] = [Entry::Plunge, Entry::Ramp];
    pub fn label(self) -> &'static str {
        match self {
            Entry::Plunge => "Plunge",
            Entry::Ramp => "Ramp",
        }
    }
}

/// A lead onto or off a contour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LeadKind {
    None,
    /// A straight line along the normal.
    Line,
    /// A tangent arc.
    #[default]
    Arc,
}

impl LeadKind {
    pub const ALL: [LeadKind; 3] = [LeadKind::None, LeadKind::Line, LeadKind::Arc];
    pub fn label(self) -> &'static str {
        match self {
            LeadKind::None => "None",
            LeadKind::Line => "Line",
            LeadKind::Arc => "Arc",
        }
    }
}

/// The Link tab.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Links {
    /// Clearance height above the stock top (mm): where the tool travels between features and
    /// starts and ends.
    pub clearance: f64,
    /// Rapid distance above the stock top (mm).
    pub rapid_distance: f64,
    /// Feed distance (mm): rapids stop this far above the material, then feed.
    pub feed_distance: f64,
    /// Gaps up to this long are small (mm).
    pub small_gap: f64,
    pub small_gaps: LinkAction,
    pub large_gaps: LinkAction,
    pub entry: Entry,
    /// Ramp angle (degrees from horizontal).
    pub ramp_angle: f64,
    pub lead_in: LeadKind,
    pub lead_out: LeadKind,
    /// Lead length or arc radius (mm).
    pub lead_radius: f64,
    /// Extra cut past the start of a closed contour (mm).
    pub overlap: f64,
}

impl Default for Links {
    fn default() -> Self {
        Links {
            clearance: 10.0,
            rapid_distance: 3.0,
            feed_distance: 1.0,
            small_gap: 5.0,
            small_gaps: LinkAction::Direct,
            large_gaps: LinkAction::RetractToRapid,
            entry: Entry::Ramp,
            ramp_angle: 3.0,
            lead_in: LeadKind::Arc,
            lead_out: LeadKind::Arc,
            lead_radius: 2.0,
            overlap: 0.0,
        }
    }
}

/// Tabs left to hold a part cut out of its stock.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tabs {
    pub enabled: bool,
    /// Per contour.
    pub count: usize,
    /// Along the path (mm).
    pub width: f64,
    /// Above the bottom (mm).
    pub height: f64,
}

impl Default for Tabs {
    fn default() -> Self {
        Tabs {
            enabled: false,
            count: 4,
            width: 5.0,
            height: 1.5,
        }
    }
}

/// Which holes a hole toolpath takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum HoleSelect {
    /// The holes the tool's diameter suits (equal for a drill or tap, larger for Bore).
    #[default]
    MatchingTool,
    /// Every hole.
    All,
    /// Only blind holes.
    Blind,
    /// The selected holes' diameters.
    SameDiameter,
}

impl HoleSelect {
    pub const ALL: [HoleSelect; 4] = [
        HoleSelect::MatchingTool,
        HoleSelect::All,
        HoleSelect::Blind,
        HoleSelect::SameDiameter,
    ];
    pub fn label(self) -> &'static str {
        match self {
            HoleSelect::MatchingTool => "Holes the tool fits",
            HoleSelect::All => "All holes",
            HoleSelect::Blind => "Blind holes",
            HoleSelect::SameDiameter => "Same diameter as selected",
        }
    }
}

/// The drive geometry: what was picked. Empty is automatic (the job's bodies).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Drive {
    /// Edge names (chained into contours).
    #[serde(default)]
    pub edges: Vec<String>,
    /// Face names.
    #[serde(default)]
    pub faces: Vec<String>,
    /// Hole centres picked (for Same diameter, and to limit the holes).
    #[serde(default)]
    pub holes: Vec<[f64; 2]>,
    /// Reverse the picked edges' direction.
    #[serde(default)]
    pub reverse: bool,
}

impl Drive {
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty() && self.faces.is_empty() && self.holes.is_empty()
    }
}

/// Speeds and feeds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Speeds {
    pub spindle_rpm: f64,
    pub feed: f64,
    pub plunge: f64,
    /// Ramp feed (also helices).
    pub ramp: f64,
    /// Lead-in and lead-out feed.
    pub lead: f64,
}

/// The Posting group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Posting {
    pub coolant: Coolant,
    /// Change tools even when the same tool is in.
    pub force_tool_change: bool,
    /// Stop (M0) before the operation.
    pub stop_before: bool,
    /// A note written into the program.
    pub comment: String,
}

impl Default for Posting {
    fn default() -> Self {
        Posting {
            coolant: Coolant::Flood,
            force_tool_change: false,
            stop_before: false,
            comment: String::new(),
        }
    }
}

/// A hole cycle's settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Cycle {
    pub kind: CycleKind,
    pub peck: f64,
    pub dwell: f64,
    /// How far a drill's full diameter goes past a through hole's bottom (mm).
    pub break_through: f64,
    /// Retract to the clearance height between holes (else to the R plane).
    pub retract_full: bool,
    pub select: HoleSelect,
}

impl Default for Cycle {
    fn default() -> Self {
        Cycle {
            kind: CycleKind::Drill,
            peck: 2.0,
            dwell: 0.0,
            break_through: 1.0,
            retract_full: false,
            select: HoleSelect::MatchingTool,
        }
    }
}

/// A beam cut's settings beyond the tool's.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BeamCut {
    /// Power (0–100 %).
    pub power: f64,
    pub passes: u32,
    /// How far each pass goes down (laser cutting thick material in passes; mm).
    pub pass_depth: f64,
    /// Small gaps left uncut to hold parts in the sheet.
    pub microjoints: usize,
    pub microjoint_width: f64,
    /// Holes smaller than this get a slower feed (plasma; mm; 0 = off).
    pub small_hole: f64,
    /// Feed factor in small holes.
    pub small_hole_feed: f64,
}

impl Default for BeamCut {
    fn default() -> Self {
        BeamCut {
            power: 80.0,
            passes: 1,
            pass_depth: 0.0,
            microjoints: 0,
            microjoint_width: 0.5,
            small_hole: 0.0,
            small_hole_feed: 0.6,
        }
    }
}

/// The Axis tab: the direction the tool cuts along.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum ToolAxis {
    /// The work plane's Z (the setup's, or the work plane above the toolpath's).
    #[default]
    WorkPlane,
    /// A direction of its own in model coordinates (a picked face's normal): the toolpath is
    /// worked out in the work plane turned to it, cut on a 3+2 machine.
    Direction(crate::geom::P3),
}

/// A toolpath repeated (the Misc tab's pattern): the same moves at more places.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Repeat {
    #[default]
    None,
    /// `count` copies in all, each `step` (mm) on from the one before.
    Linear { count: u32, step: [f64; 2] },
    /// `count` copies in all round `center`, each turned `angle` (degrees) on.
    Circular {
        count: u32,
        center: [f64; 2],
        angle: f64,
    },
}

impl Repeat {
    /// The copies' placements (the first is the toolpath itself): a point's image under each.
    fn placements(&self) -> Vec<Box<dyn Fn(crate::geom::P2) -> crate::geom::P2>> {
        match *self {
            Repeat::None => vec![Box::new(|p| p)],
            Repeat::Linear { count, step } => (0..count.max(1))
                .map(|k| {
                    let d = [step[0] * k as f64, step[1] * k as f64];
                    Box::new(move |p: crate::geom::P2| [p[0] + d[0], p[1] + d[1]])
                        as Box<dyn Fn(crate::geom::P2) -> crate::geom::P2>
                })
                .collect(),
            Repeat::Circular {
                count,
                center: c,
                angle,
            } => (0..count.max(1))
                .map(|k| {
                    let (sn, cs) = (angle * k as f64).to_radians().sin_cos();
                    Box::new(move |p: crate::geom::P2| {
                        let (x, y) = (p[0] - c[0], p[1] - c[1]);
                        [c[0] + x * cs - y * sn, c[1] + x * sn + y * cs]
                    }) as Box<dyn Fn(crate::geom::P2) -> crate::geom::P2>
                })
                .collect(),
        }
    }

    /// The toolpath with its copies, one after another (each starts and ends at the clearance
    /// height, as a toolpath does).
    pub fn apply(&self, tp: Toolpath) -> Toolpath {
        let places = self.placements();
        if places.len() <= 1 {
            return tp;
        }
        let mut steps = Vec::with_capacity(tp.steps.len() * places.len());
        for (k, f) in places.iter().enumerate() {
            let p3 = |q: crate::geom::P3| {
                let xy = f([q[0], q[1]]);
                [xy[0], xy[1], q[2]]
            };
            for s in &tp.steps {
                steps.push(match s {
                    // The spindle and the comment once.
                    Step::Spindle { .. } | Step::Comment(_) if k > 0 => continue,
                    Step::Rapid(q) => Step::Rapid(p3(*q)),
                    Step::Feed { to, feed, kind } => Step::Feed {
                        to: p3(*to),
                        feed: *feed,
                        kind: *kind,
                    },
                    Step::Arc {
                        to,
                        center,
                        cw,
                        feed,
                        kind,
                    } => Step::Arc {
                        to: p3(*to),
                        center: f(*center),
                        cw: *cw,
                        feed: *feed,
                        kind: *kind,
                    },
                    Step::Drill(c) => {
                        let mut c = *c;
                        c.at = f(c.at);
                        Step::Drill(c)
                    }
                    other => other.clone(),
                });
            }
        }
        Toolpath {
            steps,
            warnings: tp.warnings,
        }
    }
}

/// Everything a toolpath's dialog sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub pattern: Pattern,
    #[serde(default)]
    pub sub_pattern: SubPattern,
    #[serde(default)]
    pub drive: Drive,
    /// Use the job's bodies (off: only the drive geometry).
    #[serde(default = "yes")]
    pub use_job_bodies: bool,
    #[serde(default)]
    pub side: Side,
    /// Where the tool radius is taken off (2-axis profile).
    #[serde(default)]
    pub compensation: Compensation,
    /// Material left on walls (mm).
    pub wall_stock: f64,
    /// Material left on floors (mm).
    pub floor_stock: f64,
    pub step_down: f64,
    pub step_over: f64,
    /// A first, deeper or shallower, step down (0 = like the others).
    #[serde(default)]
    pub first_step_down: f64,
    /// Finishing passes on the walls (profiles).
    #[serde(default = "one")]
    pub passes: u32,
    pub direction: Direction,
    pub cutting_method: CuttingMethod,
    pub machine_by: MachineBy,
    /// Raster angle (degrees from X) of parallel passes.
    pub angle: f64,
    pub heights: Heights,
    /// Depth of an engraving or chamfer width (mm).
    pub depth: f64,
    /// A chamfer mill's tip goes this far below the chamfer (mm).
    pub tip_offset: f64,
    /// Steepness limit of Constant Z (degrees from horizontal; walls flatter are left).
    pub steep_angle: f64,
    pub tabs: Tabs,
    pub links: Links,
    pub speeds: Speeds,
    pub posting: Posting,
    pub cycle: Cycle,
    pub beam: BeamCut,
    /// Rest machining: only where the operations before left material.
    #[serde(default)]
    pub rest: bool,
    /// Round sharp corners of the toolpath by this radius (mm; 0 = off).
    #[serde(default)]
    pub smooth_corners: f64,
    /// Chordal accuracy (mm).
    pub tolerance: f64,
    /// The Collision tab.
    #[serde(default)]
    pub collision: collision::Collision,
    /// The Axis tab.
    #[serde(default)]
    pub axis: ToolAxis,
    /// Thread mill: the thread's major diameter (mm; 0 = from the hole).
    #[serde(default)]
    pub thread_diameter: f64,
    /// Thread mill: a left-hand thread (else right-hand).
    #[serde(default)]
    pub left_hand_thread: bool,
    /// Adaptive clearing: the fine levels' spacing between the deep ones (mm; 0 = none).
    #[serde(default)]
    pub fine_step_down: f64,
    /// The Misc tab's pattern: the toolpath repeated.
    #[serde(default)]
    pub repeat: Repeat,
}

fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}

impl Params {
    /// The settings a pattern starts with for a tool on a machine.
    pub fn new(pattern: Pattern, tool: &Tool, machine: MachineKind) -> Params {
        let d = tool.diameter;
        let c = &tool.cutting;
        let mut p = Params {
            pattern,
            sub_pattern: SubPattern::Offset,
            drive: Drive::default(),
            use_job_bodies: true,
            side: Side::Auto,
            compensation: Compensation::Computer,
            wall_stock: 0.0,
            floor_stock: 0.0,
            step_down: c.step_down.max(0.05),
            step_over: c.step_over.clamp(d * 0.05, d * 0.95).max(0.01),
            first_step_down: 0.0,
            passes: 1,
            direction: Direction::Climb,
            cutting_method: CuttingMethod::OneWay,
            machine_by: MachineBy::Levels,
            angle: 0.0,
            heights: Heights {
                top: Height::new(HeightRef::StockTop, 0.0),
                bottom: Height::new(HeightRef::ModelBottom, 0.0),
            },
            depth: 0.5,
            tip_offset: 0.5,
            steep_angle: 30.0,
            tabs: Tabs::default(),
            links: Links::default(),
            collision: collision::Collision::default(),
            axis: ToolAxis::WorkPlane,
            thread_diameter: 0.0,
            left_hand_thread: false,
            fine_step_down: 0.0,
            repeat: Repeat::None,
            speeds: Speeds {
                spindle_rpm: c.spindle_rpm,
                feed: c.feed,
                plunge: c.plunge_feed,
                ramp: c.plunge_feed.max(c.feed * 0.5),
                lead: c.feed,
            },
            posting: Posting::default(),
            cycle: Cycle::default(),
            beam: BeamCut {
                power: tool.beam.power,
                passes: tool.beam.passes.max(1),
                ..BeamCut::default()
            },
            rest: false,
            smooth_corners: 0.0,
            tolerance: 0.01,
        };
        match pattern {
            Pattern::Face => {
                p.step_over = d * 0.7;
                p.cutting_method = CuttingMethod::Zigzag;
                p.heights.bottom = Height::new(HeightRef::ModelTop, 0.0);
            }
            Pattern::Rough2D | Pattern::Rough3D => {
                p.wall_stock = if d >= 3.0 { 0.3 } else { 0.1 };
                p.floor_stock = p.wall_stock;
            }
            Pattern::Adaptive => {
                p.wall_stock = if d >= 3.0 { 0.3 } else { 0.1 };
                p.floor_stock = p.wall_stock;
                // Light and deep: a tenth of the diameter across, the flutes down (at most
                // twice the diameter).
                p.step_over = (d * 0.1).max(0.05);
                p.step_down = tool.cutting_length.min(d * 2.0).max(0.1);
                p.fine_step_down = (d * 0.2).max(0.1);
                // At a tenth of the diameter across the chips thin: feed faster.
                p.speeds.feed = c.feed * 2.0;
            }
            Pattern::Profile2D => {
                p.links.lead_radius = (d * 0.5).max(1.0);
                // Down to the picked contour (the model's bottom with nothing picked).
                p.heights.bottom = Height::new(HeightRef::Selection, 0.0);
            }
            Pattern::Chamfer2D => {
                p.depth = 0.5;
                p.heights.top = Height::new(HeightRef::ModelTop, 0.0);
                p.heights.bottom = Height::new(HeightRef::ModelTop, 0.0);
                p.links.lead_in = LeadKind::None;
                p.links.lead_out = LeadKind::None;
            }
            Pattern::Engrave => {
                p.side = Side::On;
                p.depth = 0.3;
                p.heights.bottom = Height::new(HeightRef::Selection, 0.0);
                p.links.lead_in = LeadKind::None;
                p.links.lead_out = LeadKind::None;
                p.links.entry = Entry::Plunge;
            }
            Pattern::Flatlands => {
                p.step_over = d * 0.5;
            }
            Pattern::Parallel => {
                p.step_over = if tool.kind == ToolKind::BallMill {
                    d * 0.1
                } else {
                    d * 0.2
                };
                p.cutting_method = CuttingMethod::Zigzag;
                p.angle = 45.0;
                p.links.lead_in = LeadKind::None;
                p.links.lead_out = LeadKind::None;
            }
            Pattern::ConstantZ => {
                p.step_down = (d * 0.1).max(0.1);
                p.links.lead_in = LeadKind::Arc;
            }
            Pattern::Scallop | Pattern::Spiral | Pattern::Pencil => {
                p.step_over = if tool.kind == ToolKind::BallMill {
                    d * 0.1
                } else {
                    d * 0.2
                };
                p.links.lead_in = LeadKind::None;
                p.links.lead_out = LeadKind::None;
            }
            Pattern::ThreadMill => {
                p.posting.coolant = Coolant::Flood;
                p.passes = 1;
            }
            Pattern::Drill => {
                p.cycle.kind = if tool.kind == ToolKind::Tap {
                    CycleKind::Tap
                } else {
                    CycleKind::Drill
                };
                p.cycle.peck = d.max(0.5);
                p.posting.coolant = Coolant::Flood;
                p.speeds.feed = c.feed;
                if tool.kind == ToolKind::Tap {
                    p.speeds.feed = tool.pitch * c.spindle_rpm;
                }
            }
            Pattern::Bore => {
                p.step_down = (d * 0.1).max(0.2);
            }
            Pattern::Cut | Pattern::Etch | Pattern::Fill => {
                p.posting.coolant = Coolant::Off;
                p.heights.top = Height::new(HeightRef::StockTop, 0.0);
                p.heights.bottom = Height::new(HeightRef::StockTop, 0.0);
                p.links.lead_in = if pattern == Pattern::Cut {
                    LeadKind::Arc
                } else {
                    LeadKind::None
                };
                p.links.lead_out = LeadKind::None;
                p.links.lead_radius = match machine {
                    MachineKind::Plasma => 4.0,
                    MachineKind::Waterjet => 3.0,
                    _ => 1.0,
                };
                p.links.overlap = if machine == MachineKind::Plasma {
                    1.0
                } else {
                    0.2
                };
                p.links.clearance = match machine {
                    MachineKind::Laser => 0.0,
                    _ => 15.0,
                };
                p.links.small_gaps = LinkAction::RetractToClearance;
                p.links.large_gaps = LinkAction::RetractToClearance;
                p.step_over = d.max(0.05);
                if pattern == Pattern::Etch {
                    p.beam.power = (tool.beam.power * 0.3).max(5.0);
                    p.side = Side::On;
                }
                if pattern == Pattern::Fill {
                    p.beam.power = (tool.beam.power * 0.5).max(5.0);
                    p.step_over = d.max(0.08);
                    p.cutting_method = CuttingMethod::Zigzag;
                }
            }
        }
        if machine == MachineKind::Router || machine == MachineKind::Laser {
            p.posting.coolant = Coolant::Off;
        }
        if !tool.center_cutting {
            p.links.entry = Entry::Ramp;
        }
        p
    }

    /// Problems with the settings themselves.
    pub fn check(&self, tool: &Tool) -> Result<(), String> {
        if self.step_down.is_nan() || self.step_down <= 0.0 {
            return Err("The step down must be more than 0".into());
        }
        if self.step_over.is_nan() || self.step_over <= 0.0 {
            return Err("The step over must be more than 0".into());
        }
        if self.step_over > tool.diameter * 1.0001 && !tool.is_beam() {
            return Err("The step over can't be more than the tool's diameter".into());
        }
        if self.speeds.feed.is_nan() || self.speeds.feed <= 0.0 {
            return Err("The feed rate must be more than 0".into());
        }
        if self.tolerance.is_nan() || self.tolerance <= 0.0 {
            return Err("The tolerance must be more than 0".into());
        }
        if self.tabs.enabled && (self.tabs.width <= 0.0 || self.tabs.height <= 0.0) {
            return Err("Tabs need a width and a height".into());
        }
        Ok(())
    }
}
