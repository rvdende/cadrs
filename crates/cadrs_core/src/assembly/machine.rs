//! The **machine definition** of an assembly (issue #278): what turns a CNC mill or a robot
//! arm modelled with mates into a machine CAM can drive.
//!
//! The assembly stays an ordinary one. Its definition ([`MachineDef`], on
//! [`super::Assembly::machine`]) only names things in it:
//!
//! - the two **mounts**: a connector on the spindle's nose (Z out of the spindle, toward the
//!   work) and one on the table (Z out of the table);
//! - the **axes**: the mates between the base and each mount, each with the letter it is
//!   programmed by, its sign, the mate value it reads 0 at, its travel and its rates;
//! - the **actuators**: mates that move something else (a door, the tool carousel, a vise);
//! - what is the enclosure, where the tools are changed, and the controller's settings
//!   ([`cadrs_cam::Machine`]: dialect, post, spindle).
//!
//! The machine's coordinates are the assembly's own: there is no separate machine frame.
//!
//! Everything else is worked out from the mates, in three steps:
//!
//! 1. [`derive`] reads the solver's model of the assembly ([`super::structure::solver_model`])
//!    as a **kinematic tree**: rigid bodies (instances held together by groups and Fastened
//!    mates), the base they hang from, and the joints between them. It notes what it cannot
//!    follow: a mate that is not a slide or a turn, a closed loop, a relation between two
//!    axes, a mount that has lost its face.
//! 2. [`detect`] proposes the axis rows for the joints between the base and the mounts
//!    (letters by direction, signs so that a rising value moves the tool plus relative to the
//!    work, travel from the mates' limits), keeping the rows the user already has.
//! 3. [`resolve`] puts the two together as a [`Resolved`] machine: the controller with its
//!    [`cadrs_cam::kinematics::Chain`], and the forward kinematics that place every instance
//!    for a list of axis values ([`Resolved::poses`]), which is what an animation of a
//!    program calls every frame. The solver is not run again: the tree's joints are exact.
//!
//! **Units.** A mate's value is in mm or radians ([`super::mate::dof_value`]); an axis's and an
//! actuator's are in mm or degrees, as a program and a person give them.
//!
//! **Ids.** The definition names mates and instances by their ids in the solver's model, so an
//! axis may be a mate of a flexible subassembly and a mount may be on a part inside one
//! ([`super::structure::derive`]).

pub mod commands;
pub mod derive;
pub mod detect;
pub mod resolve;
pub mod samples;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::connector::MateConnector;
use super::mate::{Dof, MateId};
use super::{Assembly, InstanceId};

pub use cadrs_cam::kinematics::{JointKind, Side};
pub use derive::{Frames, Tree, derive, frames_on, stored_frames};
pub use detect::{Detection, detect};
pub use resolve::{Resolved, resolve, resolve_model};

/// Identifies an actuator of a machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ActuatorId(pub Uuid);

impl ActuatorId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for ActuatorId {
    fn default() -> Self {
        Self::new()
    }
}

/// An assembly as a machine (see the module).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineDef {
    /// Where a tool holder seats: a connector on the spindle's nose, its Z out of the spindle,
    /// toward the work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_mount: Option<MateConnector>,
    /// Where work is put: a connector on the table, its Z out of the table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_mount: Option<MateConnector>,
    /// The axes: the tool side's from the base out to the tool mount, then the table side's
    /// from the base out to the table mount.
    #[serde(default)]
    pub axes: Vec<MachineAxis>,
    /// What else moves: mates on neither way to a mount.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actuators: Vec<Actuator>,
    /// The instances drawn see-through in the CAM view (guards, doors, covers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enclosure: Vec<InstanceId>,
    #[serde(default, skip_serializing_if = "ToolChange::is_default")]
    pub tool_change: ToolChange,
    /// The controller: its dialect, post settings, spindle and tool changer. Its travel, rates
    /// and chain are not kept here: [`resolve`] works them out from the axes.
    pub controller: cadrs_cam::Machine,
}

impl MachineDef {
    /// An empty definition for `controller`: nothing picked yet.
    pub fn new(controller: cadrs_cam::Machine) -> Self {
        Self {
            tool_mount: None,
            table_mount: None,
            axes: Vec::new(),
            actuators: Vec::new(),
            enclosure: Vec::new(),
            tool_change: ToolChange::default(),
            controller: cadrs_cam::Machine {
                chain: None,
                ..controller
            },
        }
    }

    pub fn axis(&self, mate: MateId, dof: Dof) -> Option<&MachineAxis> {
        self.axes.iter().find(|a| a.mate == mate && a.dof == dof)
    }

    pub fn axis_named(&self, name: &str) -> Option<&MachineAxis> {
        self.axes.iter().find(|a| a.name == name)
    }

    pub fn actuator(&self, id: ActuatorId) -> Option<&Actuator> {
        self.actuators.iter().find(|a| a.id == id)
    }
}

impl Default for MachineDef {
    /// An empty definition on the catalogue's first milling machine's controller.
    fn default() -> Self {
        let controller = cadrs_cam::machine::catalog()
            .into_iter()
            .find(|m| m.kind == cadrs_cam::MachineKind::Mill)
            .expect("the catalogue has a mill");
        Self::new(controller)
    }
}

/// One axis of a machine: a mate's degree of freedom as the controller sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineAxis {
    pub mate: MateId,
    pub dof: Dof,
    /// The letter or name it is programmed by: "X", "A", "J3".
    pub name: String,
    /// Whether it carries the tool or the work.
    pub side: Side,
    /// The axis value rises as the mate's value falls.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reversed: bool,
    /// The mate's value (mm or radians) where the axis reads 0.
    #[serde(default)]
    pub zero: f64,
    /// The axis value the machine parks at (mm or degrees).
    #[serde(default)]
    pub home: f64,
    /// Its travel: the least and greatest axis value (mm or degrees).
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
    /// `min` and `max` follow the mate's limits; off once the user types them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub limits_from_mate: bool,
    /// A turn without stops.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub continuous: bool,
    /// Its rapid rate and fastest feed (mm/min or degrees/min).
    pub rapid: f64,
    pub max_feed: f64,
}

impl MachineAxis {
    pub fn is_rotary(&self) -> bool {
        self.dof.is_angle()
    }

    /// The mate's value (mm or radians) where the axis reads `q` (mm or degrees).
    pub fn mate_value(&self, q: f64) -> f64 {
        self.zero + self.sign() * to_mate_units(self.dof, q)
    }

    /// What the axis reads (mm or degrees) where the mate's value is `v` (mm or radians).
    pub fn axis_value(&self, v: f64) -> f64 {
        self.sign() * from_mate_units(self.dof, v - self.zero)
    }

    /// −1 when reversed, else 1.
    pub fn sign(&self) -> f64 {
        if self.reversed { -1.0 } else { 1.0 }
    }
}

/// An axis or actuator value (mm or degrees) as a mate's (mm or radians).
pub fn to_mate_units(dof: Dof, v: f64) -> f64 {
    if dof.is_angle() { v.to_radians() } else { v }
}

/// A mate's value (mm or radians) as an axis's or actuator's (mm or degrees).
pub fn from_mate_units(dof: Dof, v: f64) -> f64 {
    if dof.is_angle() { v.to_degrees() } else { v }
}

/// A mate that moves something other than the tool or the work, with its named positions
/// ("Open", "Closed"; a carousel's pockets are in [`ToolChange::pockets`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actuator {
    pub id: ActuatorId,
    /// "Door", "Magazine".
    pub name: String,
    pub mate: MateId,
    pub dof: Dof,
    /// Its positions by name: the mate's value at each, in mm or degrees.
    #[serde(default)]
    pub positions: Vec<(String, f64)>,
    /// Which of `positions` it is at when nothing has moved it.
    #[serde(default)]
    pub rest: usize,
}

impl Actuator {
    /// The mate's value (mm or degrees) it rests at, when it has positions.
    pub fn rest_value(&self) -> Option<f64> {
        self.positions
            .get(self.rest)
            .or(self.positions.first())
            .map(|p| p.1)
    }
}

/// Where and how the machine changes tools.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ToolChange {
    /// Where the axes go for a change: axis names with their values (mm or degrees); an axis
    /// not listed stays where it is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub position: Vec<(String, f64)>,
    /// The actuator that brings a pocket to the spindle (a carousel, a chain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magazine: Option<ActuatorId>,
    /// The magazine's pockets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pockets: Vec<Pocket>,
}

impl ToolChange {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A pocket of the tool magazine: its number and the magazine's value (mm or degrees) that
/// brings it to the spindle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pocket {
    pub number: u32,
    pub value: f64,
}

/// How bad an [`Issue`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// The assembly cannot be driven as a machine until it is put right.
    Error,
    /// It can, with something left out or guessed.
    Warning,
}

/// Something about the assembly that keeps it from being a machine, or that the machine
/// leaves out, with the mates and instances to point at.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub severity: Severity,
    /// A sentence for the user.
    pub text: String,
    pub mates: Vec<MateId>,
    pub instances: Vec<InstanceId>,
}

impl Issue {
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            text: text.into(),
            mates: Vec::new(),
            instances: Vec::new(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(text)
        }
    }

    pub fn mates(mut self, mates: impl IntoIterator<Item = MateId>) -> Self {
        self.mates.extend(mates);
        self
    }

    pub fn instances(mut self, instances: impl IntoIterator<Item = InstanceId>) -> Self {
        self.instances.extend(instances);
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// The assembly as [`derive`] reads it: the solver's model
/// ([`super::structure::solver_model`]: subassemblies flattened) with each Replicate's copied
/// mates as mates.
pub fn flat_model(doc: &crate::document::Document, asm: &Assembly) -> Assembly {
    super::replicate::expand(&super::structure::solver_model(doc, asm))
}
