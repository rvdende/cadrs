//! The machine definition's commands: [`SetMachineDef`] stores, replaces or removes it (one
//! undo step), and [`prune`] keeps it true to the assembly when mates and instances are
//! deleted (called by the commands that delete them, so it is part of their undo step).

use std::collections::HashSet;

use super::super::commands::assembly_mut;
use super::super::connector::{ConnectorAnchor, MateConnector};
use super::super::mate::{Dof, MateId};
use super::super::{Assembly, InstanceId};
use super::{MachineDef, flat_model};
use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;

/// Sets an assembly's machine definition (`None`: the assembly is a machine no more).
#[derive(Debug, Clone)]
pub struct SetMachineDef {
    pub element: ElementId,
    pub def: Option<Box<MachineDef>>,
    /// "Set up machine", "Edit axis X", …
    pub label: String,
}

impl Command for SetMachineDef {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if let Some(def) = &self.def {
            let asm = doc
                .element(self.element)
                .ok_or(CommandError::ElementNotFound(self.element))?
                .assembly_model()
                .ok_or_else(|| CommandError::Invalid("not an assembly".into()))?;
            check(def, &Names::of(doc, asm)).map_err(CommandError::Invalid)?;
        }
        let asm = assembly_mut(doc, self.element)?;
        asm.machine = self.def.clone().map(|mut d| {
            // The chain is worked out from the axes each time: none is kept.
            d.controller.chain = None;
            d
        });
        Ok(())
    }
}

/// What a definition may name in an assembly.
struct Names {
    /// The mates of the solver's model with the motions each has.
    mates: Vec<(MateId, &'static [Dof])>,
    /// Its instances (parts inside subassemblies too) and the assembly's own.
    instances: HashSet<InstanceId>,
    /// The assembly's own connectors.
    connectors: Vec<super::super::connector::LocalConnectorId>,
}

impl Names {
    fn of(doc: &Document, asm: &Assembly) -> Self {
        let flat = flat_model(doc, asm);
        Names {
            mates: flat
                .mates
                .iter()
                .filter_map(|f| f.mate().map(|m| (f.id, m.mate_type.dof())))
                .collect(),
            instances: flat
                .instances
                .iter()
                .map(|i| i.id)
                .chain(asm.instances.iter().map(|i| i.id))
                .collect(),
            connectors: asm.connectors.iter().map(|c| c.id).collect(),
        }
    }

    fn has_motion(&self, mate: MateId, dof: Dof) -> bool {
        self.mates
            .iter()
            .any(|(m, dofs)| *m == mate && dofs.contains(&dof))
    }

    /// Whether a mount's connector is on something that is there (the Origin always is).
    fn has_connector(&self, c: &MateConnector) -> bool {
        match c.anchor {
            ConnectorAnchor::Local { id } => self.connectors.contains(&id),
            _ => c.is_origin() || self.instances.contains(&c.instance),
        }
    }
}

/// Why `def` cannot be stored, or `Ok`: every mate and instance it names is there, names are
/// given and differ, travel runs from less to more, and every number is one (an infinity or
/// a NaN never reaches the document).
fn check(def: &MachineDef, names: &Names) -> Result<(), String> {
    let finite = |what: &str, vs: &[f64]| -> Result<(), String> {
        if vs.iter().all(|v| v.is_finite()) {
            Ok(())
        } else {
            Err(format!("{what} has a value that is not a number"))
        }
    };
    for (what, mount) in [("tool", &def.tool_mount), ("table", &def.table_mount)] {
        let Some(c) = mount else { continue };
        if !names.has_connector(c) {
            return Err(format!(
                "the {what} mount is on an instance that is not there"
            ));
        }
        let f = &c.frame;
        finite(&format!("the {what} mount"), &[f.origin, f.z, f.x].concat())?;
    }
    let mut moved: Vec<(MateId, Dof)> = Vec::new();
    let mut axis_names: Vec<&str> = Vec::new();
    for a in &def.axes {
        let name = a.name.trim();
        if name.is_empty() {
            return Err("an axis needs a name".into());
        }
        if axis_names.contains(&name) {
            return Err(format!("two axes are named {name}"));
        }
        axis_names.push(name);
        if !names.has_motion(a.mate, a.dof) {
            return Err(format!("the mate of axis {name} is not there"));
        }
        if moved.contains(&(a.mate, a.dof)) {
            return Err(format!("axis {name}'s mate is already an axis"));
        }
        moved.push((a.mate, a.dof));
        finite(
            &format!("axis {name}"),
            &[a.zero, a.home, a.min, a.max, a.rapid, a.max_feed],
        )?;
        if a.continuous && !a.is_rotary() {
            return Err(format!(
                "axis {name} is a slide: it cannot be without stops"
            ));
        }
        if !a.continuous && a.min >= a.max {
            return Err(format!("axis {name} needs a travel from less to more"));
        }
        if a.rapid < 0.0 || a.max_feed < 0.0 {
            return Err(format!("axis {name}'s rates cannot be less than 0"));
        }
    }
    let mut actuator_names: Vec<&str> = Vec::new();
    for (k, a) in def.actuators.iter().enumerate() {
        let name = a.name.trim();
        if name.is_empty() {
            return Err("an actuator needs a name".into());
        }
        if actuator_names.contains(&name) {
            return Err(format!("two actuators are named {name}"));
        }
        actuator_names.push(name);
        if def.actuators[..k].iter().any(|b| b.id == a.id) {
            return Err(format!("the actuator {name} is listed twice"));
        }
        if !names.has_motion(a.mate, a.dof) {
            return Err(format!("the mate of the actuator {name} is not there"));
        }
        if moved.contains(&(a.mate, a.dof)) {
            return Err(format!(
                "the actuator {name}'s mate is already an axis or an actuator"
            ));
        }
        moved.push((a.mate, a.dof));
        let values: Vec<f64> = a.positions.iter().map(|p| p.1).collect();
        finite(&format!("the actuator {name}"), &values)?;
        if a.positions.iter().any(|p| p.0.trim().is_empty()) {
            return Err(format!("a position of the actuator {name} needs a name"));
        }
        if a.rest >= a.positions.len().max(1) {
            return Err(format!(
                "the actuator {name} rests at a position it has not"
            ));
        }
    }
    let change = &def.tool_change;
    for (axis, value) in &change.position {
        if !axis_names.contains(&axis.trim()) {
            return Err(format!(
                "the tool change position names an axis {axis} that is not there"
            ));
        }
        finite("the tool change position", &[*value])?;
    }
    if let Some(m) = change.magazine
        && def.actuator(m).is_none()
    {
        return Err("the tool magazine is not one of the actuators".into());
    }
    for (k, p) in change.pockets.iter().enumerate() {
        finite("a tool pocket", &[p.value])?;
        if change.pockets[..k].iter().any(|q| q.number == p.number) {
            return Err(format!("two tool pockets are numbered {}", p.number));
        }
    }
    if let Some(i) = def.enclosure.iter().find(|i| !names.instances.contains(i)) {
        return Err(format!(
            "the enclosure names an instance {i} that is not there"
        ));
    }
    Ok(())
}

/// Drops from the machine definition of the assembly `element` what names a mate or an
/// instance that is gone: its axes and actuators (and what the tool change says of them), its
/// mounts, its enclosure's instances. For the commands that delete mates and instances.
pub(crate) fn prune(doc: &mut Document, element: ElementId) {
    let Some(asm) = doc.element(element).and_then(|e| e.assembly_model()) else {
        return;
    };
    if asm.machine.is_none() {
        return;
    }
    let names = Names::of(doc, asm);
    let Some(def) = doc
        .element_mut(element)
        .and_then(|e| e.assembly_model_mut())
        .and_then(|a| a.machine.as_deref_mut())
    else {
        return;
    };
    for mount in [&mut def.tool_mount, &mut def.table_mount] {
        if mount.as_ref().is_some_and(|c| !names.has_connector(c)) {
            *mount = None;
        }
    }
    def.axes.retain(|a| names.has_motion(a.mate, a.dof));
    def.actuators.retain(|a| names.has_motion(a.mate, a.dof));
    def.enclosure.retain(|i| names.instances.contains(i));
    let axes: Vec<String> = def.axes.iter().map(|a| a.name.clone()).collect();
    def.tool_change.position.retain(|(n, _)| axes.contains(n));
    if let Some(m) = def.tool_change.magazine
        && def.actuator(m).is_none()
    {
        def.tool_change.magazine = None;
        def.tool_change.pockets.clear();
    }
}
