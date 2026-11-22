//! The Machine Builder's draft: a [`MachineDef`] being edited, what the assembly makes of it
//! ([`Draft::refresh`]: the axis rows, the joints left for actuators, the issues, the machine
//! when there is one) and where the machine is jogged to. Nothing here touches the document
//! or the view: the panel shows it, [`Draft::poses`] is what a jog previews, and
//! [`Draft::command`] is the one command ✓ runs.
//!
//! **Rows.** With both mounts set, the definition's axes are exactly [`detect`]'s rows: the
//! rows the user has edited kept, the rest proposed. An edit changes the row in the
//! definition, and the next refresh works the rest out again (a reversed axis's travel, the
//! machine's chain).
//!
//! **Jog values** are axis values (mm, degrees) by the axis's mate, so they stay with their
//! axis as rows come and go; an axis nobody has jogged is where the document has it. They are
//! kept on the same machine pose through an edit that changes what the axis reads there
//! (reversing it, setting its zero).

use cadrs_core::ElementId;
use cadrs_core::assembly::connector::MateConnector;
use cadrs_core::assembly::machine::commands::SetMachineDef;
use cadrs_core::assembly::machine::detect::{FreeJoint, UNLIMITED};
use cadrs_core::assembly::machine::{
    self, Actuator, ActuatorId, Frames, Issue, MachineAxis, MachineDef, Pocket, Resolved,
    from_mate_units,
};
use cadrs_core::assembly::mate::{Dof, MateId};
use cadrs_core::assembly::{Assembly, InstanceId, Pose};

/// Which of the two mounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mount {
    Tool,
    Table,
}

/// What a row shows of its mate.
#[derive(Debug, Clone, PartialEq)]
pub struct JointInfo {
    /// The mate's name.
    pub name: String,
    pub rotary: bool,
    /// The mate's value in the document (mm or radians).
    pub present: f64,
    /// The instances the mate joins (for the hover highlight).
    pub instances: Vec<InstanceId>,
}

/// The track of a jog slider: its ends, and the travel beyond which a value is an error
/// (`None`: none, a turn without stops).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Track {
    pub min: f64,
    pub max: f64,
    pub limits: Option<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub struct Draft {
    pub element: ElementId,
    pub def: MachineDef,
    /// The definition the assembly had when the panel opened (`None`: none).
    pub stored: Option<MachineDef>,
    /// The joints on neither way to a mount that are not actuators yet.
    pub free: Vec<FreeJoint>,
    pub issues: Vec<Issue>,
    /// The machine, when the assembly makes one.
    pub resolved: Option<Resolved>,
    /// Each axis row's mate, as [`MachineDef::axes`].
    pub axis_info: Vec<JointInfo>,
    /// Each actuator's mate, as [`MachineDef::actuators`] (`None`: its mate cannot be moved).
    pub actuator_info: Vec<Option<JointInfo>>,
    /// The jogged axes: their values by mate.
    jog: Vec<((MateId, Dof), f64)>,
    /// The jogged actuators: their values (mm or degrees).
    jog_actuators: Vec<(ActuatorId, f64)>,
}

impl Draft {
    /// A draft of the assembly's definition (a new one on the catalogue's first mill when it
    /// has none). [`Draft::refresh`] fills in the rest.
    pub fn new(element: ElementId, stored: Option<MachineDef>) -> Self {
        Draft {
            element,
            def: stored.clone().unwrap_or_default(),
            stored,
            free: Vec::new(),
            issues: Vec::new(),
            resolved: None,
            axis_info: Vec::new(),
            actuator_info: Vec::new(),
            jog: Vec::new(),
            jog_actuators: Vec::new(),
        }
    }

    /// Whether both mounts are set (the axes are known only then).
    pub fn mounted(&self) -> bool {
        self.def.tool_mount.is_some() && self.def.table_mount.is_some()
    }

    /// Whether the draft differs from what the assembly has.
    pub fn changed(&self) -> bool {
        self.stored.as_ref() != Some(&self.def)
    }

    /// Works the rows, the free joints, the issues and the machine out again from `flat`
    /// (the assembly as [`machine::flat_model`] gives it) with its connectors found by
    /// `frames`: after every edit of the definition, and when the document changes.
    pub fn refresh(&mut self, flat: &Assembly, frames: &Frames) {
        let tree = machine::derive(flat, &self.def, frames);
        let found = machine::detect(&tree, &self.def);
        let info = |mate: MateId, dof: Dof| {
            let j = &tree.joints[tree.joint_of(mate, dof)?];
            Some(JointInfo {
                name: j.name.clone(),
                rotary: j.is_rotary(),
                present: j.present,
                instances: flat
                    .mates
                    .iter()
                    .find(|f| f.id == mate)
                    .map(|f| f.instances())
                    .unwrap_or_default(),
            })
        };
        if self.mounted() {
            self.def.axes = found.axes;
        }
        self.axis_info = self
            .def
            .axes
            .iter()
            .map(|a| {
                info(a.mate, a.dof).unwrap_or(JointInfo {
                    name: String::new(),
                    rotary: a.is_rotary(),
                    present: a.zero,
                    instances: Vec::new(),
                })
            })
            .collect();
        self.actuator_info = self
            .def
            .actuators
            .iter()
            .map(|a| info(a.mate, a.dof))
            .collect();
        self.free = found
            .off_path
            .into_iter()
            .filter(|f| {
                !self
                    .def
                    .actuators
                    .iter()
                    .any(|a| a.mate == f.mate && a.dof == f.dof)
            })
            .collect();
        self.issues = found.issues;
        self.resolved = match machine::resolve_model(flat, &self.def, frames) {
            Ok(r) => Some(r),
            Err(issues) => {
                self.issues = issues;
                None
            }
        };
        // Jog values of axes and actuators that are gone go with them.
        let axes = &self.def.axes;
        self.jog
            .retain(|(k, _)| axes.iter().any(|a| (a.mate, a.dof) == *k));
        let actuators = &self.def.actuators;
        self.jog_actuators
            .retain(|(id, _)| actuators.iter().any(|a| a.id == *id));
    }

    // -----------------------------------------------------------------------------------------
    // Where the machine is

    /// What axis `i` reads where the document has the machine (mm or degrees).
    pub fn present(&self, i: usize) -> f64 {
        match (self.def.axes.get(i), self.axis_info.get(i)) {
            (Some(a), Some(info)) => a.axis_value(info.present),
            _ => 0.0,
        }
    }

    /// What axis `i` reads where the machine is jogged to.
    pub fn value(&self, i: usize) -> f64 {
        let Some(a) = self.def.axes.get(i) else {
            return 0.0;
        };
        self.jog
            .iter()
            .find(|(k, _)| *k == (a.mate, a.dof))
            .map_or_else(|| self.present(i), |(_, v)| *v)
    }

    /// Every axis's value, as [`Resolved::poses`] takes them.
    pub fn values(&self) -> Vec<f64> {
        (0..self.def.axes.len()).map(|i| self.value(i)).collect()
    }

    /// Jogs axis `i` to `v` (mm or degrees; beyond its travel too: the panel shows that).
    pub fn jog(&mut self, i: usize, v: f64) {
        let Some(a) = self.def.axes.get(i) else {
            return;
        };
        if !v.is_finite() {
            return;
        }
        let key = (a.mate, a.dof);
        match self.jog.iter_mut().find(|(k, _)| *k == key) {
            Some(j) => j.1 = v,
            None => self.jog.push((key, v)),
        }
    }

    /// The axis named `name`.
    pub fn axis_named(&self, name: &str) -> Option<usize> {
        self.def.axes.iter().position(|a| a.name == name)
    }

    /// Where actuator `k`'s mate is in the document (mm or degrees).
    pub fn actuator_present(&self, k: usize) -> f64 {
        match (self.def.actuators.get(k), self.actuator_info.get(k)) {
            (Some(a), Some(Some(info))) => from_mate_units(a.dof, info.present),
            (Some(a), _) => a.rest_value().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// Where actuator `k` is jogged to: an actuator nobody has moved is where the document
    /// has it.
    pub fn actuator_value(&self, k: usize) -> f64 {
        let Some(a) = self.def.actuators.get(k) else {
            return 0.0;
        };
        self.jog_actuators
            .iter()
            .find(|(id, _)| *id == a.id)
            .map_or_else(|| self.actuator_present(k), |(_, v)| *v)
    }

    pub fn jog_actuator(&mut self, k: usize, v: f64) {
        let Some(a) = self.def.actuators.get(k) else {
            return;
        };
        if !v.is_finite() {
            return;
        }
        match self.jog_actuators.iter_mut().find(|(id, _)| *id == a.id) {
            Some(j) => j.1 = v,
            None => self.jog_actuators.push((a.id, v)),
        }
    }

    /// Every axis to its home, the actuators left where they are.
    pub fn go_home(&mut self) {
        for i in 0..self.def.axes.len() {
            let home = self.def.axes[i].home;
            self.jog(i, home);
        }
    }

    /// Back to where the document has the machine.
    pub fn reset_pose(&mut self) {
        self.jog.clear();
        self.jog_actuators.clear();
    }

    /// Whether anything is jogged away from where the document has it.
    pub fn jogged(&self) -> bool {
        (0..self.def.axes.len()).any(|i| (self.value(i) - self.present(i)).abs() > 1e-9)
            || (0..self.def.actuators.len())
                .any(|k| (self.actuator_value(k) - self.actuator_present(k)).abs() > 1e-9)
    }

    /// The placements of the instances the jog moves (by their ids in the solver's model):
    /// what the view previews. Empty when nothing is jogged or the assembly makes no machine.
    pub fn poses(&self) -> Vec<(InstanceId, Pose)> {
        let Some(r) = &self.resolved else {
            return Vec::new();
        };
        if !self.jogged() {
            return Vec::new();
        }
        let actuators: Vec<(ActuatorId, f64)> = (0..self.def.actuators.len())
            .map(|k| (self.def.actuators[k].id, self.actuator_value(k)))
            .collect();
        r.poses(&self.values(), &actuators)
            .into_iter()
            .zip(&r.tree.instances)
            .filter(|((_, to), from)| *to != from.pose)
            .map(|(p, _)| p)
            .collect()
    }

    /// The axes jogged beyond their travel: their names.
    pub fn beyond_travel(&self) -> Vec<String> {
        (0..self.def.axes.len())
            .filter(|i| {
                self.axis_track(*i)
                    .limits
                    .is_some_and(|(lo, hi)| !(lo - 1e-9..=hi + 1e-9).contains(&self.value(*i)))
            })
            .map(|i| self.def.axes[i].name.clone())
            .collect()
    }

    /// Axis `i`'s slider: its travel, or a turn either way for a turn without stops, or a
    /// stretch round where it is for a slide whose travel nobody has given.
    pub fn axis_track(&self, i: usize) -> Track {
        let Some(a) = self.def.axes.get(i) else {
            return Track {
                min: 0.0,
                max: 1.0,
                limits: None,
            };
        };
        if a.continuous {
            return Track {
                min: -360.0,
                max: 360.0,
                limits: None,
            };
        }
        if a.max - a.min >= UNLIMITED {
            let at = self.present(i);
            return Track {
                min: at - 500.0,
                max: at + 500.0,
                limits: None,
            };
        }
        Track {
            min: a.min,
            max: a.max,
            limits: Some((a.min, a.max)),
        }
    }

    /// Actuator `k`'s slider: over its positions and where it is, with room either side (a
    /// turn either way for one that turns).
    pub fn actuator_track(&self, k: usize) -> Track {
        let Some(a) = self.def.actuators.get(k) else {
            return Track {
                min: 0.0,
                max: 1.0,
                limits: None,
            };
        };
        let (mut lo, mut hi) = if a.dof.is_angle() {
            (-360.0, 360.0)
        } else {
            let at = self.actuator_present(k);
            (at - 100.0, at + 100.0)
        };
        for v in a
            .positions
            .iter()
            .map(|p| p.1)
            .chain([self.actuator_value(k)])
        {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        Track {
            min: lo,
            max: hi,
            limits: None,
        }
    }

    // -----------------------------------------------------------------------------------------
    // Edits (the caller refreshes after)

    fn mount_mut(&mut self, which: Mount) -> &mut Option<MateConnector> {
        match which {
            Mount::Tool => &mut self.def.tool_mount,
            Mount::Table => &mut self.def.table_mount,
        }
    }

    pub fn mount(&self, which: Mount) -> Option<&MateConnector> {
        match which {
            Mount::Tool => self.def.tool_mount.as_ref(),
            Mount::Table => self.def.table_mount.as_ref(),
        }
    }

    pub fn set_mount(&mut self, which: Mount, connector: Option<MateConnector>) {
        *self.mount_mut(which) = connector;
    }

    /// Turns a mount's Z round (it was picked on the wrong side of its face).
    pub fn flip_mount(&mut self, which: Mount) {
        if let Some(c) = self.mount_mut(which) {
            c.flip = !c.flip;
        }
    }

    /// Runs `f` on axis `i`, the jog kept on the same machine pose.
    fn edit_axis(&mut self, i: usize, f: impl FnOnce(&mut MachineAxis)) {
        let Some(a) = self.def.axes.get(i) else {
            return;
        };
        let at = a.mate_value(self.value(i));
        let jogged = self.jog.iter().any(|(k, _)| *k == (a.mate, a.dof));
        let a = &mut self.def.axes[i];
        f(a);
        if jogged {
            // (Through radians and back: 30° stays 30°.)
            let v = (a.axis_value(at) * 1e9).round() / 1e9;
            self.jog(i, v);
        }
    }

    /// Names axis `i`; refused (false) for an empty name or another axis's.
    pub fn rename_axis(&mut self, i: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty()
            || i >= self.def.axes.len()
            || self
                .def
                .axes
                .iter()
                .enumerate()
                .any(|(k, a)| k != i && a.name == name)
        {
            return false;
        }
        let old = std::mem::replace(&mut self.def.axes[i].name, name.to_string());
        // The tool change position names its axes.
        for p in &mut self.def.tool_change.position {
            if p.0 == old {
                p.0 = name.to_string();
            }
        }
        true
    }

    /// Reverses axis `i`: it reads the other way from its zero, with its home and typed
    /// travel on the same places of the machine.
    pub fn reverse_axis(&mut self, i: usize) {
        self.edit_axis(i, |a| {
            a.reversed = !a.reversed;
            a.home = -a.home;
            (a.min, a.max) = (-a.max, -a.min);
        });
    }

    /// The axis reads 0 where the machine is jogged to; its home and typed travel stay on the
    /// same places of the machine.
    pub fn zero_here(&mut self, i: usize) {
        let v = self.value(i);
        self.edit_axis(i, |a| {
            a.zero = a.mate_value(v);
            a.home -= v;
            a.min -= v;
            a.max -= v;
        });
    }

    /// The axis parks where the machine is jogged to.
    pub fn home_here(&mut self, i: usize) {
        let v = self.value(i);
        self.edit_axis(i, |a| a.home = v);
    }

    /// A typed travel end (`max`: the greater one). The travel no longer follows the mate.
    pub fn set_travel(&mut self, i: usize, max: bool, v: f64) {
        if !v.is_finite() {
            return;
        }
        self.edit_axis(i, |a| {
            // A travel nobody gave yet starts as none either side of the typed end.
            if a.max - a.min >= UNLIMITED {
                (a.min, a.max) = (v.min(0.0), v.max(0.0));
            }
            if max {
                a.max = v;
            } else {
                a.min = v;
            }
            a.limits_from_mate = false;
            a.continuous = false;
        });
    }

    /// The travel follows the mate's limits again.
    pub fn travel_from_mate(&mut self, i: usize) {
        self.edit_axis(i, |a| a.limits_from_mate = true);
    }

    /// A turn without stops, or with (a full turn either way until its travel is typed).
    pub fn set_continuous(&mut self, i: usize, on: bool) {
        self.edit_axis(i, |a| {
            if !a.is_rotary() {
                return;
            }
            a.continuous = on;
            a.limits_from_mate = false;
            if !on && a.min >= a.max {
                (a.min, a.max) = (-360.0, 360.0);
            }
        });
    }

    pub fn set_home(&mut self, i: usize, v: f64) {
        if v.is_finite() {
            self.edit_axis(i, |a| a.home = v);
        }
    }

    pub fn set_rapid(&mut self, i: usize, v: f64) {
        if v.is_finite() {
            self.edit_axis(i, |a| a.rapid = v.max(0.0));
        }
    }

    pub fn set_max_feed(&mut self, i: usize, v: f64) {
        if v.is_finite() {
            self.edit_axis(i, |a| a.max_feed = v.max(0.0));
        }
    }

    /// Makes the free joint `k` (of [`Draft::free`]) an actuator named after its mate, with
    /// where it is now as its first position.
    pub fn add_actuator(&mut self, k: usize) -> Option<ActuatorId> {
        let f = self.free.get(k)?.clone();
        let mut name = f.name.clone();
        let mut n = 2;
        while self.def.actuators.iter().any(|a| a.name == name) {
            name = format!("{} {n}", f.name);
            n += 1;
        }
        let id = ActuatorId::new();
        self.def.actuators.push(Actuator {
            id,
            name,
            mate: f.mate,
            dof: f.dof,
            positions: Vec::new(),
            rest: 0,
        });
        Some(id)
    }

    pub fn remove_actuator(&mut self, k: usize) {
        if k >= self.def.actuators.len() {
            return;
        }
        let a = self.def.actuators.remove(k);
        if self.def.tool_change.magazine == Some(a.id) {
            self.def.tool_change.magazine = None;
            self.def.tool_change.pockets.clear();
        }
    }

    pub fn rename_actuator(&mut self, k: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty()
            || k >= self.def.actuators.len()
            || self
                .def
                .actuators
                .iter()
                .enumerate()
                .any(|(j, a)| j != k && a.name == name)
        {
            return false;
        }
        self.def.actuators[k].name = name.to_string();
        true
    }

    /// A named position of actuator `k` where it is jogged to: "Position n".
    pub fn add_position_here(&mut self, k: usize) {
        let v = self.actuator_value(k);
        let Some(a) = self.def.actuators.get_mut(k) else {
            return;
        };
        let mut n = a.positions.len() + 1;
        while a.positions.iter().any(|p| p.0 == format!("Position {n}")) {
            n += 1;
        }
        a.positions.push((format!("Position {n}"), v));
    }

    pub fn rename_position(&mut self, k: usize, p: usize, name: &str) -> bool {
        let name = name.trim();
        let Some(pos) = self
            .def
            .actuators
            .get_mut(k)
            .and_then(|a| a.positions.get_mut(p))
        else {
            return false;
        };
        if name.is_empty() {
            return false;
        }
        pos.0 = name.to_string();
        true
    }

    pub fn remove_position(&mut self, k: usize, p: usize) {
        let Some(a) = self.def.actuators.get_mut(k) else {
            return;
        };
        if p < a.positions.len() {
            a.positions.remove(p);
            if a.rest > p || a.rest >= a.positions.len() {
                a.rest = a.rest.saturating_sub(1);
            }
        }
    }

    /// Actuator `k` to its position `p`.
    pub fn go_to_position(&mut self, k: usize, p: usize) {
        if let Some(v) = self
            .def
            .actuators
            .get(k)
            .and_then(|a| a.positions.get(p))
            .map(|p| p.1)
        {
            self.jog_actuator(k, v);
        }
    }

    /// The tool change position: every axis where the machine is jogged to.
    pub fn tool_change_here(&mut self) {
        self.def.tool_change.position = self
            .def
            .axes
            .iter()
            .enumerate()
            .map(|(i, a)| (a.name.clone(), self.value(i)))
            .collect();
    }

    pub fn clear_tool_change(&mut self) {
        self.def.tool_change.position.clear();
    }

    /// The machine to its tool change position (the axes it leaves out stay).
    pub fn go_to_tool_change(&mut self) {
        for (name, v) in self.def.tool_change.position.clone() {
            if let Some(i) = self.axis_named(&name) {
                self.jog(i, v);
            }
        }
    }

    /// The actuator that brings a pocket to the spindle (`None`: no magazine). Another
    /// magazine's pockets go with it.
    pub fn set_magazine(&mut self, k: Option<usize>) {
        let id = k.and_then(|k| self.def.actuators.get(k)).map(|a| a.id);
        if self.def.tool_change.magazine != id {
            self.def.tool_change.magazine = id;
            self.def.tool_change.pockets.clear();
        }
    }

    /// The magazine's place among the actuators.
    pub fn magazine(&self) -> Option<usize> {
        let id = self.def.tool_change.magazine?;
        self.def.actuators.iter().position(|a| a.id == id)
    }

    /// A pocket where the magazine is jogged to, numbered one more than the greatest.
    pub fn add_pocket_here(&mut self) {
        let Some(k) = self.magazine() else { return };
        let value = self.actuator_value(k);
        let pockets = &mut self.def.tool_change.pockets;
        let number = pockets.iter().map(|p| p.number).max().unwrap_or(0) + 1;
        pockets.push(Pocket { number, value });
    }

    /// The magazine's named positions as its pockets, numbered from 1 in their order.
    pub fn pockets_from_positions(&mut self) {
        let Some(k) = self.magazine() else { return };
        self.def.tool_change.pockets = self.def.actuators[k]
            .positions
            .iter()
            .enumerate()
            .map(|(n, p)| Pocket {
                number: n as u32 + 1,
                value: p.1,
            })
            .collect();
    }

    pub fn remove_pocket(&mut self, p: usize) {
        if p < self.def.tool_change.pockets.len() {
            self.def.tool_change.pockets.remove(p);
        }
    }

    /// The instances drawn see-through in the CAM view: `instances` added, or all dropped.
    pub fn set_enclosure(&mut self, instances: Option<&[InstanceId]>) {
        match instances {
            None => self.def.enclosure.clear(),
            Some(list) => {
                for i in list {
                    if !self.def.enclosure.contains(i) {
                        self.def.enclosure.push(*i);
                    }
                }
            }
        }
    }

    /// The command ✓ runs: the draft stored as the assembly's definition, one undo step.
    pub fn command(&self) -> SetMachineDef {
        SetMachineDef {
            element: self.element,
            def: Some(Box::new(self.def.clone())),
            label: if self.stored.is_some() {
                "Edit machine".into()
            } else {
                "Set up machine".into()
            },
        }
    }

    /// The command of **Remove machine**.
    pub fn remove_command(&self) -> SetMachineDef {
        SetMachineDef {
            element: self.element,
            def: None,
            label: "Remove machine".into(),
        }
    }
}
