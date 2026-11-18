//! A machine ready to drive ([`resolve`]): the controller's record with its
//! [`cadrs_cam::kinematics::Chain`], and the placements of every instance for a list of axis
//! values.
//!
//! **Axis values** are in the chain's order (the tool side's joints from the base outward,
//! then the table side's), in mm and degrees. An axis reads `q` where its mate's value is
//! `zero ± q` (minus when reversed), so each joint of the tree has its mate's change from now
//! as `k·q + c` of one axis, of one actuator, or not at all ([`Source`]); a joint that follows
//! another through a relation takes its leader's with the ratio. Placing the machine is then
//! one pass down the tree ([`Tree::place`]) with nothing looked up by id.
//!
//! **The chain** describes the machine at the pose where every axis reads its
//! [`MachineAxis::home`], as [`cadrs_cam::kinematics::Joint`] asks: each joint's line is the
//! tree's, carried there by the joints nearer the base, and turned round for a reversed axis;
//! the mounts are the connectors' frames there. The tool mount's Z is turned too: the
//! connector's points out of the spindle, the chain's runs from the tool's tip into it.

use cadrs_cam::Frame;
use cadrs_cam::kinematics::{Chain, Joint};

use super::super::connector::ConnectorFrame;
use super::super::mate::{Dof, MateId};
use super::super::{Assembly, InstanceId, Pose};
use super::derive::{Frames, Motion, Role, Tree, derive};
use super::detect::detect;
use super::{ActuatorId, Issue, MachineAxis, MachineDef, from_mate_units, to_mate_units};
use crate::document::Document;
use crate::ids::ElementId;

/// What sets a joint's mate value: its change from the document's value is `k·v + c`, `v`
/// the value of an axis or an actuator (mm or degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Source {
    /// Nothing moves it: it stays this far from where it is.
    Fixed(f64),
    /// The axis at `index` of the axis values.
    Axis { index: usize, k: f64, c: f64 },
    /// The actuator at `index` of the definition's.
    Actuator { index: usize, k: f64, c: f64 },
}

/// An assembly resolved as a machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The controller with the chain, and its travel and rates summed up from the axes.
    pub machine: cadrs_cam::Machine,
    pub tree: Tree,
    /// The axes, in the order of the axis values: the definition's rows where it has them,
    /// [`detect`]'s proposals for the joints it has none for.
    pub axes: Vec<MachineAxis>,
    /// Warnings: what the machine leaves out.
    pub issues: Vec<Issue>,
    /// Each joint's source, as [`Tree::joints`].
    sources: Vec<Source>,
    /// The definition's actuators: their ids and the value each has when none is given.
    actuators: Vec<(ActuatorId, f64)>,
}

impl Resolved {
    /// The machine's axes as CAM sees them.
    pub fn chain(&self) -> &Chain {
        self.machine
            .chain
            .as_ref()
            .expect("a resolved machine has a chain")
    }

    /// Every axis at home (mm, degrees).
    pub fn home(&self) -> Vec<f64> {
        self.axes.iter().map(|a| a.home).collect()
    }

    /// What every axis reads where the document has the machine now (mm, degrees).
    pub fn present(&self) -> Vec<f64> {
        self.axes
            .iter()
            .map(|a| {
                let v = self
                    .tree
                    .joint_of(a.mate, a.dof)
                    .map_or(a.zero, |j| self.tree.joints[j].present);
                a.axis_value(v)
            })
            .collect()
    }

    /// The axis values `from` with the axes named in `named` at the values given there (a
    /// tool change position, [`super::ToolChange::position`]: the axes it leaves out stay).
    pub fn at_named(&self, from: &[f64], named: &[(String, f64)]) -> Vec<f64> {
        self.axes
            .iter()
            .enumerate()
            .map(|(i, a)| {
                named
                    .iter()
                    .find(|(n, _)| *n == a.name)
                    .map(|(_, v)| *v)
                    .or(from.get(i).copied())
                    .unwrap_or(a.home)
            })
            .collect()
    }

    /// The mate values (mm, radians) for the axis values `q` (mm, degrees; an axis `q` is too
    /// short for is at home): each axis's mate, and every mate a relation makes follow one.
    pub fn mate_values(&self, q: &[f64]) -> Vec<(MateId, Dof, f64)> {
        self.tree
            .joints
            .iter()
            .zip(&self.sources)
            .filter(|(_, s)| matches!(s, Source::Axis { .. }))
            .map(|(j, s)| (j.mate, j.dof, j.present + self.change(s, q, &[])))
            .collect()
    }

    /// How far from its value now a joint's mate is at the axis values `q` and the actuator
    /// values given.
    fn change(&self, s: &Source, q: &[f64], actuators: &[(ActuatorId, f64)]) -> f64 {
        match *s {
            Source::Fixed(d) => d,
            Source::Axis { index, k, c } => {
                k * q.get(index).copied().unwrap_or(self.axes[index].home) + c
            }
            Source::Actuator { index, k, c } => {
                let (id, rest) = self.actuators[index];
                let v = actuators
                    .iter()
                    .find(|(a, _)| *a == id)
                    .map_or(rest, |(_, v)| *v);
                k * v + c
            }
        }
    }

    /// Every instance's placement at the axis values `q` (mm, degrees) with the actuators at
    /// `actuators` (mm or degrees; one not listed is at rest): the instances of the solver's
    /// model, by their ids there ([`super::super::structure::occurrences`]).
    pub fn poses(&self, q: &[f64], actuators: &[(ActuatorId, f64)]) -> Vec<(InstanceId, Pose)> {
        let mut out = Vec::with_capacity(self.tree.instances.len());
        self.poses_into(q, actuators, &mut out);
        out
    }

    /// [`Resolved::poses`] written over `out`, which an animation keeps from frame to frame:
    /// nothing is allocated.
    pub fn poses_into(
        &self,
        q: &[f64],
        actuators: &[(ActuatorId, f64)],
        out: &mut Vec<(InstanceId, Pose)>,
    ) {
        let delta = |j: usize| self.change(&self.sources[j], q, actuators);
        self.tree.place(&delta, out);
    }

    /// The two mounts' connector frames (the assembly's coordinates) at the axis values `q`:
    /// the tool mount's Z out of the spindle, the table mount's out of the table.
    pub fn mounts(&self, q: &[f64]) -> (Option<ConnectorFrame>, Option<ConnectorFrame>) {
        let delta = |j: usize| self.change(&self.sources[j], q, &[]);
        let mut motions = vec![Pose::IDENTITY; self.tree.bodies.len()];
        self.tree.motions(&delta, &mut motions);
        let at =
            |m: &Option<super::derive::Mount>| m.as_ref().map(|m| m.frame.moved(&motions[m.body]));
        (at(&self.tree.tool), at(&self.tree.table))
    }
}

/// The chain's frame of a connector's: the same origin and X, and Z turned round when
/// `into` (the tool mount: from the tool's tip into the spindle).
fn mount_frame(f: &ConnectorFrame, into: bool) -> Frame {
    let z = if into {
        [-f.z[0], -f.z[1], -f.z[2]]
    } else {
        f.z
    };
    Frame::from_zx(f.origin, z, f.x)
}

/// The machine of the assembly `element` of `doc`, its connectors found by `frames`
/// ([`super::derive::frames_on`] the parts as built; [`super::stored_frames`] without them).
/// `Err`: what stands in the way, the warnings with it.
pub fn resolve(
    doc: &Document,
    element: ElementId,
    frames: &Frames,
) -> Result<Resolved, Vec<Issue>> {
    let asm = doc
        .element(element)
        .and_then(|e| e.assembly_model())
        .ok_or_else(|| vec![Issue::error("Not an assembly")])?;
    let def = asm
        .machine
        .as_deref()
        .ok_or_else(|| vec![Issue::error("The assembly is not set up as a machine")])?;
    resolve_model(&super::flat_model(doc, asm), def, frames)
}

/// [`resolve`] for the definition `def` on `flat`, the solver's model of its assembly
/// ([`super::flat_model`]): a definition being edited, or an assembly built in code.
pub fn resolve_model(
    flat: &Assembly,
    def: &MachineDef,
    frames: &Frames,
) -> Result<Resolved, Vec<Issue>> {
    let tree = derive(flat, def, frames);
    let found = detect(&tree, def);
    if found.issues.iter().any(|i| i.is_error()) {
        return Err(found.issues);
    }
    if tree.tool.is_none() || tree.table.is_none() {
        return Err(found.issues);
    }
    let axes = found.axes;

    // Each joint's source: an axis's, an actuator's, then the followers' from their leaders'.
    let mut sources = vec![Source::Fixed(0.0); tree.joints.len()];
    for (index, a) in axes.iter().enumerate() {
        if let Some(j) = tree.joint_of(a.mate, a.dof) {
            sources[j] = Source::Axis {
                index,
                k: a.sign() * to_mate_units(a.dof, 1.0),
                c: a.zero - tree.joints[j].present,
            };
        }
    }
    let mut actuators = Vec::with_capacity(def.actuators.len());
    for (index, a) in def.actuators.iter().enumerate() {
        let joint = tree.joint_of(a.mate, a.dof).filter(|j| {
            let joint = &tree.joints[*j];
            joint.motion == Motion::Free && matches!(joint.role, Role::Off | Role::Both)
        });
        // One that cannot be moved (detect has said so) rests wherever it is told to.
        let present = joint.map_or(0.0, |j| tree.joints[j].present);
        actuators.push((
            a.id,
            a.rest_value()
                .unwrap_or_else(|| from_mate_units(a.dof, present)),
        ));
        if let Some(j) = joint {
            sources[j] = Source::Actuator {
                index,
                k: to_mate_units(a.dof, 1.0),
                c: -present,
            };
        }
    }
    for j in 0..tree.joints.len() {
        if !matches!(tree.joints[j].motion, Motion::Follows { .. }) {
            continue;
        }
        let (lead, ratio) = tree.leader(j);
        sources[j] = match (tree.joints[lead].motion, sources[lead]) {
            (Motion::Frozen, _) => Source::Fixed(0.0),
            (_, Source::Fixed(d)) => Source::Fixed(ratio * d),
            (_, Source::Axis { index, k, c }) => Source::Axis {
                index,
                k: ratio * k,
                c: ratio * c,
            },
            (_, Source::Actuator { index, k, c }) => Source::Actuator {
                index,
                k: ratio * k,
                c: ratio * c,
            },
        };
    }

    let mut resolved = Resolved {
        machine: def.controller.clone(),
        tree,
        axes,
        issues: found.issues,
        sources,
        actuators,
    };
    let tree = &resolved.tree;
    let (Some(tool), Some(table)) = (&tree.tool, &tree.table) else {
        return Err(resolved.issues);
    };

    // The chain, at the pose where every axis is at home.
    let home = resolved.home();
    let at_home = |j: usize| resolved.change(&resolved.sources[j], &home, &[]);
    let joint_at = |a: &MachineAxis| -> Option<Joint> {
        let j = tree.joint_of(a.mate, a.dof)?;
        let joint = &tree.joints[j];
        let carried = tree.motion_before(j, &at_home);
        let d = carried.rotate(joint.dir);
        let s = a.sign();
        Some(Joint {
            name: a.name.clone(),
            kind: joint.kind,
            origin: carried.apply(joint.origin),
            dir: [s * d[0], s * d[1], s * d[2]],
            min: a.min,
            max: a.max,
            continuous: a.continuous,
            home: a.home,
            rapid: a.rapid,
            max_feed: a.max_feed,
        })
    };
    let side = |side: super::Side| -> Vec<Joint> {
        resolved
            .axes
            .iter()
            .filter(|a| a.side == side)
            .filter_map(joint_at)
            .collect()
    };
    let mut motions = vec![Pose::IDENTITY; tree.bodies.len()];
    tree.motions(&at_home, &mut motions);
    let chain = Chain {
        tool: side(super::Side::Tool),
        table: side(super::Side::Table),
        tool_mount: mount_frame(&tool.frame.moved(&motions[tool.body]), true),
        table_mount: mount_frame(&table.frame.moved(&motions[table.body]), false),
    };

    // The controller's summary of its axes: the travel of X, Y and Z, and the slowest slide's
    // rates (the record's own where the machine has no such axis: an arm).
    let machine = &mut resolved.machine;
    for (k, letter) in ["X", "Y", "Z"].iter().enumerate() {
        if let Some(j) = chain.joints().find(|j| j.name == *letter && !j.is_rotary()) {
            machine.travel[k] = j.max - j.min;
        }
    }
    let least = |f: &dyn Fn(&Joint) -> f64| {
        chain
            .joints()
            .filter(|j| !j.is_rotary())
            .map(f)
            .min_by(f64::total_cmp)
    };
    if let Some(r) = least(&|j| j.rapid) {
        machine.rapid_rate = r;
    }
    if let Some(f) = least(&|j| j.max_feed) {
        machine.max_feed = f;
    }
    machine.chain = Some(chain);
    Ok(resolved)
}
