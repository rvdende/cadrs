//! **Named positions** (P3B.8, `intro-to-assemblies.md` A1.8, A16.2, X16): an assembly's
//! saved states of motion. A named position holds the value of every driven degree of freedom of
//! the assembly's own mates when it was captured ([`MateValue`]: a Revolute's angle, a Slider's
//! travel, …) and, for a parent assembly that **follows** it (A16.2), the placements of the
//! instances then.
//!
//! **Apply** solves the assembly to the stored values ([`solve_to`]): each mate is snapped to
//! its values in turn with all of them held, so the assembly moves there even after edits (a
//! mate deleted since is skipped). Capture, update, rename and delete are commands
//! ([`SetNamedPosition`], [`DeleteNamedPosition`]); applying one is a
//! [`super::commands::MoveInstances`].
//!
//! A rigid subassembly instance can **Lock / follow** a Named position of its tab
//! ([`SetFollowPosition`]): its parts are placed as the position placed them
//! ([`super::structure::occurrences`]).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::commands::assembly_mut;
use super::connector::MateConnector;
use super::mate::{Dof, MateId, MateKind};
use super::solver::{Drive, SolveOptions, Solution};
use super::{Assembly, InstanceId, Pose};
use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;
use crate::solid::Solid;

/// Identifies a named position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NamedPositionId(pub Uuid);

impl NamedPositionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for NamedPositionId {
    fn default() -> Self {
        Self::new()
    }
}

/// A mate's position along one of its degrees of freedom (mm or radians).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MateValue {
    pub mate: MateId,
    pub dof: Dof,
    pub value: f64,
}

/// A named position ("Open", "Closed").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedPosition {
    pub id: NamedPositionId,
    pub name: String,
    /// The mates' values when captured.
    #[serde(default)]
    pub values: Vec<MateValue>,
    /// The top-level instances' placements when captured (what a following parent uses).
    #[serde(default)]
    pub poses: Vec<(InstanceId, Pose)>,
}

impl NamedPosition {
    /// Where it placed the instance `id`.
    pub fn pose_of(&self, id: InstanceId) -> Option<Pose> {
        self.poses.iter().find(|(i, _)| *i == id).map(|(_, p)| *p)
    }

    /// Its value of a mate's DOF.
    pub fn value(&self, mate: MateId, dof: Dof) -> Option<f64> {
        self.values.iter().find(|v| v.mate == mate && v.dof == dof).map(|v| v.value)
    }
}

impl Assembly {
    pub fn named_position(&self, id: NamedPositionId) -> Option<&NamedPosition> {
        self.named_positions.iter().find(|p| p.id == id)
    }
}

/// The next free "Position n".
pub fn next_name(asm: &Assembly) -> String {
    let n = asm
        .named_positions
        .iter()
        .filter_map(|p| p.name.strip_prefix("Position ")?.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("Position {}", n + 1)
}

/// The value of every DOF of the assembly's own mates (not suppressed) at the placements of
/// `flat` (the solver's model, [`super::structure::solver_model`]; its connectors resolved on
/// `solids`). `own` limits them to the mates of the assembly itself.
pub fn mate_values(flat: &Assembly, solids: &HashMap<InstanceId, Arc<Solid>>, own: &[MateId]) -> Vec<MateValue> {
    let model = super::resolve_local_connectors(flat, solids);
    let world = |c: &MateConnector| {
        let pose = model.instance(c.instance).map(|i| i.pose).unwrap_or_default();
        c.local_frame(solids.get(&c.instance).map(|s| &**s)).moved(&pose)
    };
    let mut out = Vec::new();
    for f in model.mates.iter().filter(|f| !f.suppressed && own.contains(&f.id)) {
        let MateKind::Mate(m) = &f.kind else { continue };
        let (a, g) = (world(&m.connectors[0]), m.target(&world(&m.connectors[1])));
        for d in m.mate_type.dof() {
            out.push(MateValue { mate: f.id, dof: *d, value: super::mate::dof_value(&a, &g, *d) });
        }
    }
    out
}

/// A named position of the assembly `element` at its current placements (named `name`).
pub fn capture(doc: &Document, element: ElementId, id: NamedPositionId, name: &str, solids: &HashMap<InstanceId, Arc<Solid>>) -> Result<NamedPosition, CommandError> {
    let asm = doc.element(element).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(element))?;
    let flat = super::structure::solver_model(doc, asm);
    let own: Vec<MateId> = asm.mates.iter().map(|m| m.id).collect();
    Ok(NamedPosition {
        id,
        name: name.to_string(),
        values: mate_values(&flat, solids, &own),
        poses: asm.instances.iter().map(|i| (i.id, i.pose)).collect(),
    })
}

/// **Apply** a named position: the solver's placements with every stored mate value held
/// (`flat` is the solver's model). Each mate with values is snapped in turn, then the whole
/// assembly is solved with all of them.
pub fn solve_to(flat: &Assembly, solids: &HashMap<InstanceId, Arc<Solid>>, position: &NamedPosition) -> Solution {
    let drives: Vec<Drive> = position
        .values
        .iter()
        .filter(|v| flat.mate(v.mate).is_some_and(|f| !f.suppressed && f.mate().is_some_and(|m| m.mate_type.dof().contains(&v.dof))))
        .map(|v| Drive { mate: v.mate, dof: v.dof, value: v.value })
        .collect();
    let movers: Vec<InstanceId> = flat.instances.iter().filter(|i| !i.fixed).map(|i| i.id).collect();
    let mut model = flat.clone();
    let mut mates: Vec<MateId> = Vec::new();
    for d in &drives {
        if !mates.contains(&d.mate) {
            mates.push(d.mate);
        }
    }
    let mut last = None;
    for m in mates {
        let sol = super::solve(&model, solids, &SolveOptions { movers: movers.clone(), snap: Some(m), drives: drives.clone(), ..Default::default() });
        for (id, p) in &sol.poses {
            if let Some(i) = model.instance_mut(*id) {
                i.pose = *p;
            }
        }
        last = Some(sol);
    }
    match last {
        Some(_) => super::solve(&model, solids, &SolveOptions { movers, drives, ..Default::default() }),
        None => super::solve(&model, solids, &SolveOptions { movers, ..Default::default() }),
    }
}

/// Adds or replaces a named position (Capture, Update, Rename): one undo step.
#[derive(Debug, Clone)]
pub struct SetNamedPosition {
    pub element: ElementId,
    pub position: NamedPosition,
}

impl Command for SetNamedPosition {
    fn label(&self) -> String {
        format!("Named position: {}", self.position.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.position.name.trim().is_empty() {
            return Err(CommandError::Invalid("a named position needs a name".into()));
        }
        let asm = assembly_mut(doc, self.element)?;
        match asm.named_positions.iter_mut().find(|p| p.id == self.position.id) {
            Some(slot) => *slot = self.position.clone(),
            None => asm.named_positions.push(self.position.clone()),
        }
        Ok(())
    }
}

/// Deletes a named position.
#[derive(Debug, Clone)]
pub struct DeleteNamedPosition {
    pub element: ElementId,
    pub id: NamedPositionId,
}

impl Command for DeleteNamedPosition {
    fn label(&self) -> String {
        "Delete named position".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        if asm.named_position(self.id).is_none() {
            return Err(CommandError::Invalid("named position not found".into()));
        }
        asm.named_positions.retain(|p| p.id != self.id);
        Ok(())
    }
}

/// **Lock / follow position to** (A16.2): a rigid subassembly instance follows a Named position
/// of its tab (`None`: its tab's current placements). A flexible one is made rigid.
#[derive(Debug, Clone)]
pub struct SetFollowPosition {
    pub element: ElementId,
    pub instance: InstanceId,
    pub follow: Option<NamedPositionId>,
}

impl Command for SetFollowPosition {
    fn label(&self) -> String {
        "Lock / follow position".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let src = doc
            .element(self.element)
            .and_then(|e| e.assembly_model())
            .and_then(|a| a.instance(self.instance))
            .map(|i| i.source)
            .ok_or_else(|| CommandError::Invalid(format!("instance {} not found", self.instance)))?;
        let super::InstanceSource::Assembly { element: child } = src else {
            return Err(CommandError::Invalid("only a subassembly follows a named position".into()));
        };
        if let Some(f) = self.follow
            && doc.element(child).and_then(|e| e.assembly_model()).and_then(|a| a.named_position(f)).is_none()
        {
            return Err(CommandError::Invalid("the subassembly has no such named position".into()));
        }
        let asm = assembly_mut(doc, self.element)?;
        let i = asm.instance_mut(self.instance).ok_or_else(|| CommandError::Invalid("instance not found".into()))?;
        i.follow = self.follow;
        if self.follow.is_some() {
            i.flexible = false;
            i.overrides.clear();
        }
        Ok(())
    }
}
