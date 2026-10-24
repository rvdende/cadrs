//! CAM Studio (`ElementKind::CamStudio`): a tab that machines parts of this document's Part
//! Studios, modelled on Onshape's CAM Studio and extended to lasers, plasma tables and
//! waterjets. The machining itself is `cadrs_cam`'s; this module is the tab's document model,
//! its commands and the kernel work behind it.
//!
//! - **Components** ([`Component`]): the parts to machine, the **workholding** (vices,
//!   fixtures) and **stock** bodies. Each is a Derived reference ([`crate::derived`]) to a
//!   part of a Part Studio, at its workspace (followed live, [`refresh`]) or at a version (a
//!   frozen copy in the document's linked elements). The studio is a Part Studio of those
//!   Derived features for the rebuild ([`CamStudio::part_features`]), so the viewport shows
//!   and picks them like any parts.
//! - **Jobs** ([`Job`]): the bodies to machine, their stock (a box or cylinder round them with
//!   offsets, or a stock component) and the workholding; the job's **machine** (a copy of a
//!   catalogue machine with its post settings) and its **setups**.
//! - **Setups** ([`Setup`]): a program number, how the parts sit on the machine table
//!   (machine mating) and the work coordinate system ([`WorkPlane`]: origin and orientation,
//!   datum); then the **operations** in order: **tools** ([`ToolOp`], with their T numbers),
//!   **toolpaths** ([`ToolpathOp`], each using the tool listed above it) and **work planes**
//!   (each toolpath below one is computed and posted in it).
//!
//! Every change is an [`EditCam`] command, so it undoes. Computing toolpaths, the stock
//! simulation and posting are [`program`]'s; the kernel work (the parts' exact faces, holes and
//! sections in a setup's coordinates) is [`geometry`]'s.

pub mod geometry;
pub mod program;
pub mod sheet;

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::derived::DerivedFeature;
use crate::document::{Document, Element, ElementKind, Feature, FeatureKind};
use crate::ids::{ElementId, FeatureId, PartId};
pub use cadrs_cam;
use cadrs_cam::geom::{Aabb, Frame, P3};
use cadrs_cam::{Machine, Params, Tool, ToolLibrary};

/// An id within a CAM Studio (components, jobs, setups, operations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CamId(pub u64);

impl CamId {
    /// A new random id.
    pub fn new() -> CamId {
        CamId(uuid::Uuid::new_v4().as_u64_pair().0)
    }
}

impl Default for CamId {
    fn default() -> Self {
        CamId::new()
    }
}

/// What a component is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    /// The part to machine.
    Part,
    /// What holds the part on the machine (a vice, soft jaws, clamps).
    Workholding,
    /// The material to machine away (a casting, a plate).
    Stock,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Part, Role::Workholding, Role::Stock];

    pub fn label(self) -> &'static str {
        match self {
            Role::Part => "Part",
            Role::Workholding => "Workholding",
            Role::Stock => "Stock",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Role::Part => "part",
            Role::Workholding => "cam-workholding",
            Role::Stock => "cam-stock",
        }
    }
}

/// A part, workholding or stock body brought into the studio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: CamId,
    pub role: Role,
    /// Its name in the Components list (the part's).
    pub name: String,
    /// The reference: its source Part Studio (workspace or version) and the part it takes.
    pub derived: DerivedFeature,
    #[serde(default)]
    pub hidden: bool,
}

impl Component {
    /// The Derived feature standing for it in [`CamStudio::part_features`].
    pub fn feature_id(&self) -> FeatureId {
        component_feature(self.id)
    }

    /// "Block (V2)", "Block (workspace)".
    pub fn reference_label(&self) -> String {
        let d = &self.derived;
        if d.version_name.is_empty() {
            format!("{} (workspace)", d.source_name)
        } else {
            format!("{} ({})", d.source_name, d.version_name)
        }
    }
}

/// The feature id of a component's Derived feature.
pub fn component_feature(id: CamId) -> FeatureId {
    FeatureId::from_u128(0xca3d_0000_0000_4000_8000_0000_0000_0000 ^ u128::from(id.0))
}

/// How the job's stock is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StockKind {
    /// A box round the bodies, grown by the offsets.
    Box,
    /// A cylinder round the bodies about the stock's Z, grown by the radial offset.
    Cylinder,
    /// A stock component's body.
    Component(CamId),
}

impl StockKind {
    pub fn label(self) -> &'static str {
        match self {
            StockKind::Box => "Box",
            StockKind::Cylinder => "Cylinder",
            StockKind::Component(_) => "User defined",
        }
    }
}

/// The directions the stock's box or cylinder is aligned with.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StockDirection {
    /// The model's axes.
    World,
    /// Its Z and X (world directions); Y follows.
    Axes { z: P3, x: P3 },
}

/// The job's stock.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StockDef {
    pub kind: StockKind,
    pub direction: StockDirection,
    /// Growth beyond the bodies' box: −X, +X, −Y, +Y, −Z, +Z (mm).
    pub offsets: [f64; 6],
    /// A cylinder's growth beyond the bodies' radius (mm).
    pub radial: f64,
}

impl Default for StockDef {
    fn default() -> Self {
        StockDef {
            kind: StockKind::Box,
            direction: StockDirection::World,
            offsets: [2.0, 2.0, 2.0, 2.0, 0.0, 1.0],
            radial: 2.0,
        }
    }
}

impl StockDef {
    /// The frame the stock is aligned with (world coordinates).
    pub fn frame(&self) -> Frame {
        match self.direction {
            StockDirection::World => Frame::WORLD,
            StockDirection::Axes { z, x } => Frame::from_zx([0.0; 3], z, x),
        }
    }
}

/// A point of a box: each coordinate at its minimum (−1), middle (0) or maximum (+1).
pub type BoxPoint = [i8; 3];

/// The point of `b` a [`BoxPoint`] names.
pub fn box_point(b: &Aabb, p: BoxPoint) -> P3 {
    let mut q = [0.0; 3];
    for i in 0..3 {
        q[i] = match p[i] {
            i8::MIN..=-1 => b.min[i],
            0 => (b.min[i] + b.max[i]) / 2.0,
            _ => b.max[i],
        };
    }
    q
}

/// Where a work plane's origin is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Origin {
    /// The model's origin.
    World,
    /// A point of the job's bodies' box (in the work plane's orientation).
    ComponentBox(BoxPoint),
    /// A point of the stock's box.
    StockBox(BoxPoint),
    /// A point (model coordinates).
    Manual(P3),
}

impl Origin {
    pub fn label(&self) -> &'static str {
        match self {
            Origin::World => "World",
            Origin::ComponentBox(_) => "Component box point",
            Origin::StockBox(_) => "Stock box point",
            Origin::Manual(_) => "Manual",
        }
    }
}

/// How a work plane's coordinates are written in the NC code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum WorkPlaneType {
    /// A fixed datum set up on the control (G54…).
    #[default]
    Absolute,
    /// A temporary frame the control defines (G68.2): for 3+2 machines.
    Dynamic,
    /// The setup's coordinates: toolpaths follow the work plane, the output doesn't move.
    None,
}

impl WorkPlaneType {
    pub const ALL: [WorkPlaneType; 3] = [
        WorkPlaneType::Absolute,
        WorkPlaneType::Dynamic,
        WorkPlaneType::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WorkPlaneType::Absolute => "Absolute",
            WorkPlaneType::Dynamic => "Dynamic",
            WorkPlaneType::None => "None",
        }
    }
}

/// A coordinate system for machining: a setup's work coordinate system or a work plane.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorkPlane {
    pub origin: Origin,
    /// Its Z (the tool's axis, pointing out of the material) and X, in model coordinates.
    pub z: P3,
    pub x: P3,
    pub kind: WorkPlaneType,
    /// The datum number written (1 = G54, 2 = G55, …); `None`: the next free one.
    pub datum: Option<u32>,
    /// Show its triad in the view.
    #[serde(default)]
    pub show_triad: bool,
}

impl Default for WorkPlane {
    fn default() -> Self {
        WorkPlane {
            origin: Origin::StockBox([0, 0, 1]),
            z: [0.0, 0.0, 1.0],
            x: [1.0, 0.0, 0.0],
            kind: WorkPlaneType::Absolute,
            datum: None,
            show_triad: true,
        }
    }
}

/// How the parts are placed on the machine's table for the simulation and the travel check.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Mating {
    /// The stock's bottom centred on the table.
    Automatic,
    /// Moved from the table's centre by this (mm).
    Manual(P3),
}

/// A setup: one way of holding the parts, with its coordinate system and operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub id: CamId,
    pub name: String,
    pub program_number: u32,
    pub mating: Mating,
    pub wcs: WorkPlane,
    #[serde(default)]
    pub operations: Vec<Operation>,
    #[serde(default)]
    pub suppressed: bool,
}

/// A tool in a setup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOp {
    pub tool: Tool,
    /// Its number in the machine (T): the assembly number.
    pub number: u32,
}

/// A toolpath in a setup: it uses the tool listed above it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolpathOp {
    pub params: Params,
    /// The name was typed (it stays when the pattern changes).
    #[serde(default)]
    pub name_edited: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpKind {
    Tool(Box<ToolOp>),
    Toolpath(Box<ToolpathOp>),
    WorkPlane(Box<WorkPlane>),
}

/// An operation in a setup's list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub id: CamId,
    pub name: String,
    pub kind: OpKind,
    #[serde(default)]
    pub suppressed: bool,
    /// Hidden in the view (its toolpath or triad).
    #[serde(default)]
    pub hidden: bool,
}

impl Operation {
    pub fn tool(&self) -> Option<&ToolOp> {
        match &self.kind {
            OpKind::Tool(t) => Some(t),
            _ => None,
        }
    }

    pub fn toolpath(&self) -> Option<&ToolpathOp> {
        match &self.kind {
            OpKind::Toolpath(t) => Some(t),
            _ => None,
        }
    }

    pub fn work_plane(&self) -> Option<&WorkPlane> {
        match &self.kind {
            OpKind::WorkPlane(w) => Some(w),
            _ => None,
        }
    }

    pub fn icon(&self) -> &'static str {
        match &self.kind {
            OpKind::Tool(t) => t.tool.kind.icon(),
            OpKind::Toolpath(t) => t.params.pattern.icon(),
            OpKind::WorkPlane(_) => "cam-work-plane",
        }
    }
}

/// A job: what is machined, from what, on which machine, in which setups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: CamId,
    pub name: String,
    /// The parts to machine (their ids in [`CamStudio::part_features`]'s build); empty: every
    /// part component's.
    #[serde(default)]
    pub bodies: Vec<PartId>,
    pub stock: StockDef,
    /// The workholding bodies.
    #[serde(default)]
    pub workholdings: Vec<PartId>,
    #[serde(default)]
    pub machine: Option<Machine>,
    #[serde(default)]
    pub setups: Vec<Setup>,
    #[serde(default)]
    pub suppressed: bool,
}

/// A CAM Studio tab's contents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CamStudio {
    #[serde(default)]
    pub components: Vec<Component>,
    #[serde(default)]
    pub jobs: Vec<Job>,
    /// The job the toolbar's Create buttons add to (bold in the Jobs list).
    #[serde(default)]
    pub active_job: Option<CamId>,
    /// The tools, holders and arbors made or imported here.
    #[serde(default)]
    pub library: ToolLibrary,
    /// [`CamStudio::part_features`], made on first use.
    #[serde(skip)]
    features: std::sync::OnceLock<Vec<Feature>>,
}

impl PartialEq for CamStudio {
    fn eq(&self, o: &Self) -> bool {
        self.components == o.components
            && self.jobs == o.jobs
            && self.active_job == o.active_job
            && self.library == o.library
    }
}

/// Where something is in a studio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRef {
    Component(usize),
    Job(usize),
    Setup(usize, usize),
    Operation(usize, usize, usize),
}

impl CamStudio {
    /// The Derived reference of the component standing for `feature` (for updating it in
    /// place); the features worked out from the components are dropped to be made again.
    pub fn component_derived_mut(
        &mut self,
        feature: FeatureId,
    ) -> Option<&mut crate::derived::DerivedFeature> {
        self.features = std::sync::OnceLock::new();
        self.components
            .iter_mut()
            .find(|c| c.feature_id() == feature)
            .map(|c| &mut c.derived)
    }

    /// The components as Derived features (hidden ones too; the app hides their parts).
    pub fn part_features(&self) -> &[Feature] {
        self.features.get_or_init(|| {
            self.components
                .iter()
                .map(|c| {
                    Feature::new(
                        c.feature_id(),
                        c.name.clone(),
                        FeatureKind::Derived(Box::new(c.derived.clone())),
                    )
                })
                .collect()
        })
    }

    /// The component a part of the build belongs to.
    pub fn component_of(&self, part: PartId) -> Option<&Component> {
        self.components
            .iter()
            .find(|c| c.feature_id() == part.feature)
    }

    pub fn component(&self, id: CamId) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    pub fn job(&self, id: CamId) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// The active job: the chosen one, else the first.
    pub fn active(&self) -> Option<&Job> {
        self.active_job
            .and_then(|id| self.job(id))
            .or_else(|| self.jobs.first())
    }

    /// Finds a node by id.
    pub fn find(&self, id: CamId) -> Option<NodeRef> {
        if let Some(i) = self.components.iter().position(|c| c.id == id) {
            return Some(NodeRef::Component(i));
        }
        for (j, job) in self.jobs.iter().enumerate() {
            if job.id == id {
                return Some(NodeRef::Job(j));
            }
            for (s, setup) in job.setups.iter().enumerate() {
                if setup.id == id {
                    return Some(NodeRef::Setup(j, s));
                }
                if let Some(o) = setup.operations.iter().position(|o| o.id == id) {
                    return Some(NodeRef::Operation(j, s, o));
                }
            }
        }
        None
    }

    /// The setup and operation of an operation id.
    pub fn operation(&self, id: CamId) -> Option<(&Job, &Setup, &Operation)> {
        match self.find(id)? {
            NodeRef::Operation(j, s, o) => {
                let job = &self.jobs[j];
                let setup = &job.setups[s];
                Some((job, setup, &setup.operations[o]))
            }
            _ => None,
        }
    }

    pub fn setup(&self, id: CamId) -> Option<(&Job, &Setup)> {
        match self.find(id)? {
            NodeRef::Setup(j, s) => Some((&self.jobs[j], &self.jobs[j].setups[s])),
            _ => None,
        }
    }

    /// A name not used by another job, setup or operation: "Rough 1", "Rough 2", …
    pub fn next_name(&self, base: &str) -> String {
        let used = |n: &str| {
            self.jobs.iter().any(|j| {
                j.name == n
                    || j.setups
                        .iter()
                        .any(|s| s.name == n || s.operations.iter().any(|o| o.name == n))
            })
        };
        (1..)
            .map(|k| format!("{base} {k}"))
            .find(|n| !used(n))
            .unwrap()
    }

    /// The next free tool number in a job.
    pub fn next_tool_number(&self, job: CamId) -> u32 {
        self.job(job)
            .map(|j| {
                j.setups
                    .iter()
                    .flat_map(|s| s.operations.iter())
                    .filter_map(|o| o.tool().map(|t| t.number))
                    .max()
                    .unwrap_or(0)
                    + 1
            })
            .unwrap_or(1)
    }

    /// The tool a toolpath uses: the last tool listed above it in its setup.
    pub fn tool_for(&self, op: CamId) -> Option<&ToolOp> {
        let (_, setup, _) = self.operation(op)?;
        let i = setup.operations.iter().position(|o| o.id == op)?;
        setup.operations[..i]
            .iter()
            .rev()
            .filter(|o| !o.suppressed)
            .find_map(|o| o.tool())
    }

    /// The work plane a toolpath is computed in: the last listed above it, else the setup's.
    pub fn work_plane_for(&self, op: CamId) -> Option<(Option<CamId>, &WorkPlane)> {
        let (_, setup, _) = self.operation(op)?;
        let i = setup.operations.iter().position(|o| o.id == op)?;
        Some(
            setup.operations[..i]
                .iter()
                .rev()
                .filter(|o| !o.suppressed)
                .find_map(|o| o.work_plane().map(|w| (Some(o.id), w)))
                .unwrap_or((None, &setup.wcs)),
        )
    }

    fn reset_cache(&mut self) {
        self.features = std::sync::OnceLock::new();
    }
}

impl Element {
    /// A CAM Studio tab with nothing in it.
    pub fn cam_studio(name: impl Into<String>) -> Self {
        Self {
            id: ElementId::new(),
            name: name.into(),
            kind: ElementKind::CamStudio(Box::new(CamStudio {
                library: ToolLibrary::starter(),
                ..CamStudio::default()
            })),
            assembly: Default::default(),
            contexts: Vec::new(),
            open_context: None,
            simulation: Default::default(),
            named_views: Vec::new(),
        }
    }

    /// A CAM Studio tab's contents.
    pub fn cam(&self) -> Option<&CamStudio> {
        match &self.kind {
            ElementKind::CamStudio(c) => Some(c),
            _ => None,
        }
    }

    /// (Its components may change: the features made from them are made again when next asked
    /// for.)
    pub fn cam_mut(&mut self) -> Option<&mut CamStudio> {
        match &mut self.kind {
            ElementKind::CamStudio(c) => {
                c.reset_cache();
                Some(c)
            }
            _ => None,
        }
    }
}

/// Brings every workspace component of this document up to date with its source Part Studio
/// (its features, part settings and name), as Derived features in Part Studios are
/// ([`crate::derived::refresh`]). Called after every command, undo and redo.
pub fn refresh(doc: &mut Document) {
    use crate::external::RefAt;
    let this = doc.id;
    let mut updates = Vec::new();
    for (i, el) in doc.elements.iter().enumerate() {
        let Some(cam) = el.cam() else { continue };
        for (k, c) in cam.components.iter().enumerate() {
            let Some(r) = c.derived.source else { continue };
            if r.at != RefAt::Workspace || r.document_or(this) != this {
                continue;
            }
            let Some(src) = doc
                .elements
                .iter()
                .find(|e| e.id == r.element && matches!(e.kind, ElementKind::PartStudio { .. }))
            else {
                continue;
            };
            let features = src.active_features();
            if c.derived.studio != features
                || c.derived.props != src.part_props()
                || c.derived.source_name != src.name
            {
                updates.push((i, k, features, src.part_props().to_vec(), src.name.clone()));
            }
        }
    }
    for (i, k, features, props, name) in updates {
        if let Some(cam) = doc.elements[i].cam_mut() {
            let d = &mut cam.components[k].derived;
            d.studio = features;
            d.props = props;
            d.source_name = name;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

/// A change to a CAM Studio.
#[derive(Debug, Clone)]
pub enum Edit {
    /// Insert part, workholding or stock (with the copies a version reference needs).
    AddComponents {
        components: Vec<Component>,
        links: Vec<crate::external::LinkedElement>,
    },
    /// Replace a component (rename, role, a new reference).
    SetComponent {
        component: Box<Component>,
        links: Vec<crate::external::LinkedElement>,
    },
    /// Create job (it becomes the active one).
    AddJob(Box<Job>),
    /// Edit job: its name, bodies, stock and workholding (its machine and setups stay).
    SetJob(Box<Job>),
    /// Create or edit the job's machine.
    SetMachine {
        job: CamId,
        machine: Option<Box<Machine>>,
    },
    AddSetup {
        job: CamId,
        setup: Box<Setup>,
    },
    /// Edit setup: its name, program, mating and work coordinate system (its operations stay).
    SetSetup {
        job: CamId,
        setup: Box<Setup>,
    },
    /// Adds an operation at the end of a setup (or right after `after`).
    AddOperation {
        setup: CamId,
        after: Option<CamId>,
        op: Box<Operation>,
    },
    SetOperation(Box<Operation>),
    /// Deletes a component, job, setup or operation.
    Delete(CamId),
    Suppress(CamId, bool),
    Hide(CamId, bool),
    Rename(CamId, String),
    /// Copies a job, setup or operation (new ids from `seed`), right after it.
    Duplicate {
        id: CamId,
        seed: u64,
    },
    /// Moves an operation to `index` in `setup`.
    Move {
        op: CamId,
        setup: CamId,
        index: usize,
    },
    SetActiveJob(CamId),
    /// Replaces the tool library (an import merges first and passes the result).
    SetLibrary(Box<ToolLibrary>),
    /// Copies an operation to the end of a setup, in any job (a new id from `seed`).
    CopyTo {
        op: CamId,
        setup: CamId,
        seed: u64,
    },
    /// Several edits as one undo step (a Delete of something already gone with its parent is
    /// skipped).
    Batch(Vec<Edit>),
}

/// A change to the CAM Studio `element` (one undo step).
#[derive(Debug, Clone)]
pub struct EditCam {
    pub element: ElementId,
    pub edit: Edit,
}

impl EditCam {
    pub fn new(element: ElementId, edit: Edit) -> EditCam {
        EditCam { element, edit }
    }
}

fn studio_mut(doc: &mut Document, element: ElementId) -> Result<&mut CamStudio, CommandError> {
    doc.element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?
        .cam_mut()
        .ok_or_else(|| CommandError::Invalid("not a CAM Studio".into()))
}

fn invalid(s: &str) -> CommandError {
    CommandError::Invalid(s.into())
}

/// New ids for a copy, from a seed (so redo makes the same ids).
fn reseed(seed: &mut u64) -> CamId {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    CamId(*seed)
}

impl Command for EditCam {
    fn label(&self) -> String {
        match &self.edit {
            Edit::AddComponents { components, .. } => match components.first().map(|c| c.role) {
                Some(Role::Workholding) => "Insert workholding".into(),
                Some(Role::Stock) => "Insert stock".into(),
                _ => "Insert part".into(),
            },
            Edit::SetComponent { .. } => "Edit component".into(),
            Edit::AddJob(_) => "Create job".into(),
            Edit::SetJob(_) => "Edit job".into(),
            Edit::SetMachine { .. } => "Create machine".into(),
            Edit::AddSetup { .. } => "Create setup".into(),
            Edit::SetSetup { .. } => "Edit setup".into(),
            Edit::AddOperation { op, .. } => match op.kind {
                OpKind::Tool(_) => "Create tool".into(),
                OpKind::Toolpath(_) => "Create toolpath".into(),
                OpKind::WorkPlane(_) => "Create work plane".into(),
            },
            Edit::SetOperation(op) => format!("Edit {}", op.name),
            Edit::Delete(_) => "Delete".into(),
            Edit::Suppress(_, true) => "Suppress".into(),
            Edit::Suppress(_, false) => "Unsuppress".into(),
            Edit::Hide(_, true) => "Hide".into(),
            Edit::Hide(_, false) => "Show".into(),
            Edit::Rename(_, _) => "Rename".into(),
            Edit::Duplicate { .. } => "Duplicate".into(),
            Edit::Move { .. } => "Reorder operations".into(),
            Edit::SetActiveJob(_) => "Activate job".into(),
            Edit::SetLibrary(_) => "Update tool library".into(),
            Edit::CopyTo { .. } => "Copy operation".into(),
            Edit::Batch(v) => {
                if v.iter().all(|e| matches!(e, Edit::Delete(_))) {
                    format!("Delete {} items", v.len())
                } else {
                    format!("{} edits", v.len())
                }
            }
        }
    }

    fn scope(&self) -> Scope {
        match &self.edit {
            // Version references add frozen copies to the document.
            Edit::AddComponents { .. } | Edit::SetComponent { .. } | Edit::Delete(_) => {
                Scope::Whole
            }
            Edit::Batch(v)
                if v.iter()
                    .any(|e| EditCam::new(self.element, e.clone()).scope() == Scope::Whole) =>
            {
                Scope::Whole
            }
            _ => Scope::Element(self.element),
        }
    }

    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        match &self.edit {
            Edit::AddComponents { components, links } => {
                if components.is_empty() {
                    return Err(invalid("nothing to insert"));
                }
                add_links(doc, links);
                let s = studio_mut(doc, self.element)?;
                for c in components {
                    if s.component(c.id).is_some() {
                        return Err(invalid("component id already in use"));
                    }
                    s.components.push(c.clone());
                }
            }
            Edit::SetComponent { component, links } => {
                add_links(doc, links);
                let s = studio_mut(doc, self.element)?;
                let old = s
                    .components
                    .iter_mut()
                    .find(|c| c.id == component.id)
                    .ok_or_else(|| invalid("no such component"))?;
                let copy = old.derived.copy;
                *old = (**component).clone();
                if let Some(c) = copy.filter(|c| Some(*c) != component.derived.copy) {
                    crate::link_update::prune(doc, &[c]);
                }
            }
            Edit::AddJob(job) => {
                let s = studio_mut(doc, self.element)?;
                if s.find(job.id).is_some() {
                    return Err(invalid("id already in use"));
                }
                s.jobs.push((**job).clone());
                s.active_job = Some(job.id);
            }
            Edit::SetJob(job) => {
                let s = studio_mut(doc, self.element)?;
                let j = s
                    .jobs
                    .iter_mut()
                    .find(|j| j.id == job.id)
                    .ok_or_else(|| invalid("no such job"))?;
                j.name = job.name.clone();
                j.bodies = job.bodies.clone();
                j.stock = job.stock;
                j.workholdings = job.workholdings.clone();
            }
            Edit::SetMachine { job, machine } => {
                let s = studio_mut(doc, self.element)?;
                let j = s
                    .jobs
                    .iter_mut()
                    .find(|j| j.id == *job)
                    .ok_or_else(|| invalid("no such job"))?;
                if let Some(m) = machine {
                    m.check().map_err(CommandError::Invalid)?;
                }
                j.machine = machine.as_deref().cloned();
            }
            Edit::AddSetup { job, setup } => {
                let s = studio_mut(doc, self.element)?;
                if s.find(setup.id).is_some() {
                    return Err(invalid("id already in use"));
                }
                let j = s
                    .jobs
                    .iter_mut()
                    .find(|j| j.id == *job)
                    .ok_or_else(|| invalid("no such job"))?;
                if j.machine.is_none() {
                    return Err(invalid("create the job's machine first"));
                }
                j.setups.push((**setup).clone());
            }
            Edit::SetSetup { job, setup } => {
                let s = studio_mut(doc, self.element)?;
                let st = s
                    .jobs
                    .iter_mut()
                    .find(|j| j.id == *job)
                    .and_then(|j| j.setups.iter_mut().find(|x| x.id == setup.id))
                    .ok_or_else(|| invalid("no such setup"))?;
                st.name = setup.name.clone();
                st.program_number = setup.program_number;
                st.mating = setup.mating;
                st.wcs = setup.wcs;
            }
            Edit::AddOperation { setup, after, op } => {
                let s = studio_mut(doc, self.element)?;
                if s.find(op.id).is_some() {
                    return Err(invalid("id already in use"));
                }
                let Some(NodeRef::Setup(j, k)) = s.find(*setup) else {
                    return Err(invalid("no such setup"));
                };
                let ops = &mut s.jobs[j].setups[k].operations;
                let at = after
                    .and_then(|a| ops.iter().position(|o| o.id == a))
                    .map_or(ops.len(), |i| i + 1);
                ops.insert(at, (**op).clone());
                if let OpKind::Tool(t) = &op.kind {
                    s.library.add(&t.tool);
                    if let Some(h) = &t.tool.holder {
                        s.library.add_holder(h);
                    }
                    if let Some(a) = &t.tool.arbor {
                        s.library.add_arbor(a);
                    }
                }
            }
            Edit::SetOperation(op) => {
                let s = studio_mut(doc, self.element)?;
                let Some(NodeRef::Operation(j, k, o)) = s.find(op.id) else {
                    return Err(invalid("no such operation"));
                };
                s.jobs[j].setups[k].operations[o] = (**op).clone();
                if let OpKind::Tool(t) = &op.kind {
                    s.library.add(&t.tool);
                }
            }
            Edit::Delete(id) => {
                let s = studio_mut(doc, self.element)?;
                let mut pruned = None;
                match s.find(*id).ok_or_else(|| invalid("not found"))? {
                    NodeRef::Component(i) => {
                        let c = s.components.remove(i);
                        let f = c.feature_id();
                        for j in &mut s.jobs {
                            j.bodies.retain(|p| p.feature != f);
                            j.workholdings.retain(|p| p.feature != f);
                            if j.stock.kind == StockKind::Component(c.id) {
                                j.stock.kind = StockKind::Box;
                            }
                        }
                        pruned = c.derived.copy;
                    }
                    NodeRef::Job(j) => {
                        s.jobs.remove(j);
                        if s.active_job == Some(*id) {
                            s.active_job = s.jobs.first().map(|j| j.id);
                        }
                    }
                    NodeRef::Setup(j, k) => {
                        s.jobs[j].setups.remove(k);
                    }
                    NodeRef::Operation(j, k, o) => {
                        s.jobs[j].setups[k].operations.remove(o);
                    }
                }
                if let Some(c) = pruned {
                    crate::link_update::prune(doc, &[c]);
                }
            }
            Edit::Suppress(id, on) => {
                let s = studio_mut(doc, self.element)?;
                match s.find(*id).ok_or_else(|| invalid("not found"))? {
                    NodeRef::Job(j) => s.jobs[j].suppressed = *on,
                    NodeRef::Setup(j, k) => s.jobs[j].setups[k].suppressed = *on,
                    NodeRef::Operation(j, k, o) => {
                        s.jobs[j].setups[k].operations[o].suppressed = *on
                    }
                    NodeRef::Component(_) => return Err(invalid("components can't be suppressed")),
                }
            }
            Edit::Hide(id, on) => {
                let s = studio_mut(doc, self.element)?;
                match s.find(*id).ok_or_else(|| invalid("not found"))? {
                    NodeRef::Component(i) => s.components[i].hidden = *on,
                    NodeRef::Operation(j, k, o) => s.jobs[j].setups[k].operations[o].hidden = *on,
                    _ => return Err(invalid("only components and operations hide")),
                }
            }
            Edit::Rename(id, name) => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(invalid("a name can't be empty"));
                }
                let s = studio_mut(doc, self.element)?;
                match s.find(*id).ok_or_else(|| invalid("not found"))? {
                    NodeRef::Component(i) => s.components[i].name = name.into(),
                    NodeRef::Job(j) => s.jobs[j].name = name.into(),
                    NodeRef::Setup(j, k) => s.jobs[j].setups[k].name = name.into(),
                    NodeRef::Operation(j, k, o) => {
                        let op = &mut s.jobs[j].setups[k].operations[o];
                        op.name = name.into();
                        if let OpKind::Toolpath(t) = &mut op.kind {
                            t.name_edited = true;
                        }
                    }
                }
            }
            Edit::Duplicate { id, seed } => {
                let mut seed = *seed;
                let s = studio_mut(doc, self.element)?;
                match s.find(*id).ok_or_else(|| invalid("not found"))? {
                    NodeRef::Job(j) => {
                        let mut c = s.jobs[j].clone();
                        c.id = reseed(&mut seed);
                        for st in &mut c.setups {
                            st.id = reseed(&mut seed);
                            for o in &mut st.operations {
                                o.id = reseed(&mut seed);
                            }
                        }
                        c.name = s.next_name(&strip_number(&c.name));
                        s.jobs.insert(j + 1, c);
                    }
                    NodeRef::Setup(j, k) => {
                        let mut c = s.jobs[j].setups[k].clone();
                        c.id = reseed(&mut seed);
                        for o in &mut c.operations {
                            o.id = reseed(&mut seed);
                        }
                        c.name = s.next_name(&strip_number(&c.name));
                        s.jobs[j].setups.insert(k + 1, c);
                    }
                    NodeRef::Operation(j, k, o) => {
                        let mut c = s.jobs[j].setups[k].operations[o].clone();
                        c.id = reseed(&mut seed);
                        c.name = s.next_name(&strip_number(&c.name));
                        s.jobs[j].setups[k].operations.insert(o + 1, c);
                    }
                    NodeRef::Component(_) => return Err(invalid("insert the part again instead")),
                }
            }
            Edit::Move { op, setup, index } => {
                let s = studio_mut(doc, self.element)?;
                let Some(NodeRef::Operation(j, k, o)) = s.find(*op) else {
                    return Err(invalid("no such operation"));
                };
                let Some(NodeRef::Setup(j2, k2)) = s.find(*setup) else {
                    return Err(invalid("no such setup"));
                };
                if j2 != j {
                    return Err(invalid("operations move within their job"));
                }
                let moved = s.jobs[j].setups[k].operations.remove(o);
                let ops = &mut s.jobs[j].setups[k2].operations;
                ops.insert((*index).min(ops.len()), moved);
            }
            Edit::SetActiveJob(id) => {
                let s = studio_mut(doc, self.element)?;
                if s.job(*id).is_none() {
                    return Err(invalid("no such job"));
                }
                s.active_job = Some(*id);
            }
            Edit::SetLibrary(lib) => {
                let s = studio_mut(doc, self.element)?;
                s.library = (**lib).clone();
            }
            Edit::CopyTo { op, setup, seed } => {
                let mut seed = *seed;
                let s = studio_mut(doc, self.element)?;
                let Some(NodeRef::Operation(j, k, o)) = s.find(*op) else {
                    return Err(invalid("no such operation"));
                };
                let Some(NodeRef::Setup(j2, k2)) = s.find(*setup) else {
                    return Err(invalid("no such setup"));
                };
                let mut c = s.jobs[j].setups[k].operations[o].clone();
                c.id = reseed(&mut seed);
                c.name = s.next_name(&strip_number(&c.name));
                s.jobs[j2].setups[k2].operations.push(c);
            }
            Edit::Batch(edits) => {
                for e in edits {
                    if let Edit::Delete(id) = e
                        && studio_mut(doc, self.element)?.find(*id).is_none()
                    {
                        continue;
                    }
                    EditCam::new(self.element, e.clone()).apply(doc)?;
                }
            }
        }
        Ok(())
    }
}

/// "Rough 3" → "Rough".
fn strip_number(name: &str) -> String {
    match name.rsplit_once(' ') {
        Some((head, n)) if n.chars().all(|c| c.is_ascii_digit()) && !head.is_empty() => {
            head.to_string()
        }
        _ => name.to_string(),
    }
}

fn add_links(doc: &mut Document, links: &[crate::external::LinkedElement]) {
    for l in links {
        if doc.linked_element(l.id()).is_none() {
            doc.linked.push(l.clone());
        }
    }
}

/// A component of the part `part` of the Part Studio `element` of this document, at its
/// workspace (followed live).
pub fn workspace_component(
    doc: &Document,
    element: ElementId,
    part: PartId,
    role: Role,
) -> Result<Component, String> {
    let el = doc
        .element(element)
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .ok_or("not a Part Studio of this document")?;
    let mut d = DerivedFeature::new(None, element);
    d.fill_from(el, "", "");
    d.selection = crate::derived::DerivedSelection {
        all: false,
        parts: vec![part],
        ..Default::default()
    };
    let build = crate::rebuild::build(&d.studio);
    let p = build.part(part).ok_or("the Part Studio has no such part")?;
    Ok(Component {
        id: CamId::new(),
        role,
        name: crate::parts::display_name(p, &d.props).to_string(),
        derived: d,
        hidden: false,
    })
}

#[cfg(test)]
mod tests;
