//! **Exploded views** (P3B.8, `intro-to-assemblies.md` A1.8, X16): named view states of an
//! assembly in which instances are moved apart in **steps**. Each step moves some instances by a
//! translation along an axis or a turn about one ([`ExplodeMotion`]); steps apply in order, each
//! starting where the earlier ones left its instances, so they compose.
//!
//! An exploded view never changes the assembly: its mates and placements stay as they are, and
//! [`exploded_poses`] gives the placements to *show* at a slider position from 0 (assembled) to
//! 1 (fully exploded), the steps playing one after another. [`trails`] gives the trail lines
//! from where each moved instance was to where it is shown.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::commands::assembly_mut;
use super::{Assembly, InstanceId, Pose};
use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;
use cadrs_sketch::Vec3;

/// Identifies an exploded view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExplodedViewId(pub Uuid);

impl ExplodedViewId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for ExplodedViewId {
    fn default() -> Self {
        Self::new()
    }
}

/// How a step moves its instances (assembly coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExplodeMotion {
    /// Along `direction` (a unit vector) by `distance` (mm).
    Translate { direction: Vec3, distance: f64 },
    /// About the axis through `point` along `axis` by `angle` (radians).
    Rotate { point: Vec3, axis: Vec3, angle: f64 },
}

impl ExplodeMotion {
    /// The motion done to the fraction `t` (0…1).
    pub fn pose(&self, t: f64) -> Pose {
        match *self {
            ExplodeMotion::Translate { direction, distance } => Pose::translation(direction.map(|c| c * distance * t)),
            ExplodeMotion::Rotate { point, axis, angle } => Pose::rotation_about(point, axis, angle * t),
        }
    }

    /// Its size: the distance (mm) or the angle (radians).
    pub fn amount(&self) -> f64 {
        match *self {
            ExplodeMotion::Translate { distance, .. } => distance,
            ExplodeMotion::Rotate { angle, .. } => angle,
        }
    }

    /// The same motion with another size.
    pub fn with_amount(&self, v: f64) -> Self {
        match *self {
            ExplodeMotion::Translate { direction, .. } => ExplodeMotion::Translate { direction, distance: v },
            ExplodeMotion::Rotate { point, axis, .. } => ExplodeMotion::Rotate { point, axis, angle: v },
        }
    }
}

/// A step: some instances and how they move.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExplodeStep {
    pub instances: Vec<InstanceId>,
    pub motion: ExplodeMotion,
}

/// An exploded view ("Exploded view 1").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExplodedView {
    pub id: ExplodedViewId,
    pub name: String,
    #[serde(default)]
    pub steps: Vec<ExplodeStep>,
    /// Trail lines shown.
    #[serde(default = "yes")]
    pub trails: bool,
}

fn yes() -> bool {
    true
}

impl ExplodedView {
    pub fn new(id: ExplodedViewId, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), steps: Vec::new(), trails: true }
    }
}

impl Assembly {
    pub fn exploded_view(&self, id: ExplodedViewId) -> Option<&ExplodedView> {
        self.exploded_views.iter().find(|v| v.id == id)
    }
}

/// The next free "Exploded view n".
pub fn next_name(asm: &Assembly) -> String {
    let n = asm
        .exploded_views
        .iter()
        .filter_map(|p| p.name.strip_prefix("Exploded view ")?.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("Exploded view {}", n + 1)
}

/// How far each step has played at the slider position `t` (0…1): the steps one after another.
pub fn step_fractions(n: usize, t: f64) -> Vec<f64> {
    let p = t.clamp(0.0, 1.0) * n as f64;
    (0..n).map(|k| (p - k as f64).clamp(0.0, 1.0)).collect()
}

/// The placements to show for the top-level instances of `asm` in `view` at the slider position
/// `t` (0: assembled, 1: exploded). Only the instances moved are listed; the assembly itself is
/// not changed.
pub fn exploded_poses(asm: &Assembly, view: &ExplodedView, t: f64) -> Vec<(InstanceId, Pose)> {
    let mut poses: Vec<(InstanceId, Pose)> = Vec::new();
    let fr = step_fractions(view.steps.len(), t);
    for (step, f) in view.steps.iter().zip(fr) {
        if f <= 0.0 {
            continue;
        }
        let m = step.motion.pose(f);
        for id in &step.instances {
            let Some(inst) = asm.instance(*id) else { continue };
            match poses.iter_mut().find(|(i, _)| i == id) {
                Some(slot) => slot.1 = slot.1.then(&m),
                None => poses.push((*id, inst.pose.then(&m))),
            }
        }
    }
    poses
}

/// The trail lines of `view` at `t`: for each step and instance, the path of the instance's
/// point `anchor(id)` (in its own coordinates) during the step, as a polyline (assembly
/// coordinates; a turn is drawn as an arc).
pub fn trails(asm: &Assembly, view: &ExplodedView, t: f64, anchor: impl Fn(InstanceId) -> Vec3) -> Vec<Vec<Vec3>> {
    let mut out = Vec::new();
    let mut poses: Vec<(InstanceId, Pose)> = asm.instances.iter().map(|i| (i.id, i.pose)).collect();
    let fr = step_fractions(view.steps.len(), t);
    for (step, f) in view.steps.iter().zip(fr) {
        if f <= 0.0 {
            break;
        }
        let samples = match step.motion {
            ExplodeMotion::Translate { .. } => 1,
            ExplodeMotion::Rotate { .. } => 16,
        };
        for id in &step.instances {
            let Some(slot) = poses.iter_mut().find(|(i, _)| i == id) else { continue };
            let a = anchor(*id);
            let line: Vec<Vec3> = (0..=samples).map(|k| slot.1.then(&step.motion.pose(f * k as f64 / samples as f64)).apply(a)).collect();
            out.push(line);
            slot.1 = slot.1.then(&step.motion.pose(f));
        }
    }
    out
}

/// Adds or replaces an exploded view (its steps, name, trails): one undo step.
#[derive(Debug, Clone)]
pub struct SetExplodedView {
    pub element: ElementId,
    pub view: ExplodedView,
}

impl Command for SetExplodedView {
    fn label(&self) -> String {
        format!("Exploded view: {}", self.view.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.view.name.trim().is_empty() {
            return Err(CommandError::Invalid("an exploded view needs a name".into()));
        }
        let asm = assembly_mut(doc, self.element)?;
        if let Some(i) = self.view.steps.iter().flat_map(|s| s.instances.iter()).find(|i| asm.instance(**i).is_none()) {
            return Err(CommandError::Invalid(format!("instance {i} not found")));
        }
        match asm.exploded_views.iter_mut().find(|v| v.id == self.view.id) {
            Some(slot) => *slot = self.view.clone(),
            None => asm.exploded_views.push(self.view.clone()),
        }
        Ok(())
    }
}

/// Deletes an exploded view.
#[derive(Debug, Clone)]
pub struct DeleteExplodedView {
    pub element: ElementId,
    pub id: ExplodedViewId,
}

impl Command for DeleteExplodedView {
    fn label(&self) -> String {
        "Delete exploded view".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        if asm.exploded_view(self.id).is_none() {
            return Err(CommandError::Invalid("exploded view not found".into()));
        }
        asm.exploded_views.retain(|v| v.id != self.id);
        Ok(())
    }
}
