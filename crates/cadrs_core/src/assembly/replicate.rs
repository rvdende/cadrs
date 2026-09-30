//! **Replicate** (P3B.8, `intro-to-assemblies.md` X16; "Replicate 1" in `ex4-step11.png`): an
//! instance pattern driven by matching geometry. Pick a **seed** instance and its mate (a screw
//! and its Fastened mate into a hole), then the matching places (the other holes' rims, or
//! "all matching" on the part): Replicate puts a copy of the seed at each, held by a copy of the
//! seed's mate on that place.
//!
//! It is one feature of the Mate Features list ([`super::mate::MateKind::Replicate`]) that can
//! be edited (its targets added or removed). The copies are ordinary instances
//! ([`super::Instance::replicate`] names the feature, and the Instances list shows them under a
//! "Replicate 1" row); the copied mates are not stored: [`expand`] makes them for the solver.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::commands::assembly_mut;
use super::connector::{ConnectorAnchor, EntityRef, ImplicitPoint, MateConnector, resolve_implicit};
use super::mate::{Mate, MateFeature, MateId, MateKind};
use super::structure::derive;
use super::{Assembly, Instance, InstanceId, Pose};
use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;
use crate::solid::Solid;

/// A Replicate feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Replicate {
    /// The instance copied.
    pub seed: InstanceId,
    /// Its mate, copied onto each target.
    pub seed_mate: MateId,
    /// The places: connectors like the seed mate's other one.
    pub targets: Vec<MateConnector>,
    /// The copy at each target (same order).
    pub instances: Vec<InstanceId>,
}

impl Replicate {
    /// Which side of the seed mate is the seed's (0 or 1).
    pub fn seed_side(&self, m: &Mate) -> Option<usize> {
        m.connectors.iter().position(|c| c.instance == self.seed)
    }
}

/// The mate id of the `k`th copy of a Replicate `id`'s mate.
pub fn copy_mate_id(id: MateId, k: usize) -> MateId {
    MateId(derive(InstanceId(id.0), InstanceId::from_u128(0x7e9_0000 + k as u128)).0)
}

/// The copy of the seed mate `m` for the `k`th target: the seed's connector moved onto the copy,
/// the other one onto the target (keeping the seed mate's flip, reorient and edits there).
pub fn copy_mate(r: &Replicate, m: &Mate, k: usize) -> Option<Mate> {
    let side = r.seed_side(m)?;
    let (inst, target) = (*r.instances.get(k)?, *r.targets.get(k)?);
    let mut out = m.clone();
    out.connectors[side].instance = inst;
    let other = &m.connectors[1 - side];
    out.connectors[1 - side] = MateConnector { flip: other.flip ^ target.flip, reorient: other.reorient, edit: other.edit, ..target };
    Some(out)
}

/// The assembly with each (not suppressed) Replicate's copied mates added as mates and the
/// Replicate features themselves left out: what the solver sees.
pub fn expand(asm: &Assembly) -> Assembly {
    if !asm.mates.iter().any(|f| matches!(f.kind, MateKind::Replicate(_))) {
        return asm.clone();
    }
    let mut out = asm.clone();
    let mut mates = Vec::new();
    for f in &asm.mates {
        let MateKind::Replicate(r) = &f.kind else {
            mates.push(f.clone());
            continue;
        };
        if f.suppressed {
            continue;
        }
        let Some(seed) = asm.mate(r.seed_mate) else { continue };
        let Some(m) = seed.mate() else { continue };
        for k in 0..r.instances.len() {
            if let Some(c) = copy_mate(r, m, k) {
                let mut g = MateFeature::new(copy_mate_id(f.id, k), format!("{} {}", f.name, k + 1), MateKind::Mate(c));
                g.suppressed = seed.suppressed;
                mates.push(g);
            }
        }
    }
    out.mates = mates;
    out
}

/// The world frame of a connector at the placements of `asm`.
fn world(asm: &Assembly, solids: &HashMap<InstanceId, Arc<Solid>>, c: &MateConnector) -> super::connector::ConnectorFrame {
    let pose = asm.instance(c.instance).map(|i| i.pose).unwrap_or_default();
    c.local_frame(solids.get(&c.instance).map(|s| &**s)).moved(&pose)
}

/// Where the copy on `target` goes: the seed moved as the seed mate's other connector would move
/// onto the target.
pub fn copy_pose(asm: &Assembly, solids: &HashMap<InstanceId, Arc<Solid>>, seed: &Instance, m: &Mate, side: usize, target: &MateConnector) -> Pose {
    let other = &m.connectors[1 - side];
    let from = world(asm, solids, other);
    let adjusted = MateConnector { flip: other.flip ^ target.flip, reorient: other.reorient, edit: other.edit, ..*target };
    let to = world(asm, solids, &adjusted);
    seed.pose.then(&from.pose().inverse().then(&to.pose()))
}

/// The places on the target part that match the seed mate's other connector `c` (its instance's
/// part `s`): for a circle's centre, every circle of the same size about a parallel axis at the
/// same height along it (the rims of a row of holes on one face); for a cylinder's axis middle,
/// every cylinder of the same radius about a parallel axis. The connector's own place is left
/// out; each keeps `c`'s owner kind and gets a flip when its axis points the other way.
pub fn matching_targets(s: &Solid, c: &MateConnector) -> Vec<MateConnector> {
    let ConnectorAnchor::Implicit { point, owner } = c.anchor else { return Vec::new() };
    let Some(base) = resolve_implicit(s, &point, &owner) else { return Vec::new() };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6);
    let mut out: Vec<MateConnector> = Vec::new();
    let mut push = |p: ImplicitPoint, owner: EntityRef| {
        let Some(f) = resolve_implicit(s, &p, &owner) else { return };
        if near(f.origin, base.origin) || out.iter().any(|q| near(q.frame.origin, f.origin)) {
            return;
        }
        let mut m = MateConnector { instance: c.instance, anchor: ConnectorAnchor::Implicit { point: p, owner }, frame: f, flip: false, reorient: 0, edit: Default::default() };
        m.flip = dot(f.z, base.z) < 0.0;
        out.push(m);
    };
    match point {
        ImplicitPoint::CircleCenter(e) => {
            let Some(r) = s.edge(&e).and_then(|x| x.circle).map(|k| k.radius) else { return Vec::new() };
            let n = base.z;
            let h = dot(base.origin, n);
            for x in &s.edges {
                let Some(k) = x.circle else { continue };
                if (k.radius - r).abs() > 1e-6 || dot(k.normal, n).abs() < 1.0 - 1e-9 || (dot(k.center, n) - h).abs() > 1e-6 {
                    continue;
                }
                // The same owner kind: the rim itself, or the face of the seed's owner kind.
                let o = match owner {
                    EntityRef::Edge(_) => EntityRef::Edge(x.name),
                    _ => x.name.faces.iter().copied().find(|f| s.face(f).is_some_and(|ff| ff.plane.is_some())).map(EntityRef::Face).unwrap_or(EntityRef::Edge(x.name)),
                };
                push(ImplicitPoint::CircleCenter(x.name), o);
            }
        }
        ImplicitPoint::AxisMiddle(face) => {
            let Some((_, super::connector::SurfaceKind::Cylinder { radius })) = super::connector::surface_of(s, &EntityRef::Face(face)) else { return Vec::new() };
            for f in &s.faces {
                if let Some((fr, super::connector::SurfaceKind::Cylinder { radius: r2 })) = super::connector::surface_of(s, &EntityRef::Face(f.name))
                    && (r2 - radius).abs() < 1e-6
                    && dot(fr.z, base.z).abs() > 1.0 - 1e-9
                {
                    push(ImplicitPoint::AxisMiddle(f.name), EntityRef::Face(f.name));
                }
            }
        }
        _ => {}
    }
    // In a stable order: along the part's axes.
    out.sort_by(|a, b| {
        let (p, q) = (a.frame.origin, b.frame.origin);
        p[0].total_cmp(&q[0]).then(p[1].total_cmp(&q[1])).then(p[2].total_cmp(&q[2]))
    });
    out
}

/// A Replicate feature ready to add: the copies' instances (new ids from `ids`, placed with
/// [`copy_pose`]) for `targets`. `flat` is the solver's model and `solids` its source parts.
#[allow(clippy::too_many_arguments)]
pub fn plan(
    asm: &Assembly,
    flat: &Assembly,
    solids: &HashMap<InstanceId, Arc<Solid>>,
    seed: InstanceId,
    seed_mate: MateId,
    targets: Vec<MateConnector>,
    mut ids: impl FnMut(usize) -> InstanceId,
    feature: MateId,
) -> Result<(Replicate, Vec<Instance>), CommandError> {
    let seed_inst = asm.instance(seed).ok_or_else(|| CommandError::Invalid("seed instance not found".into()))?;
    if seed_inst.source.is_composite() {
        return Err(CommandError::Invalid("the seed must be a part instance".into()));
    }
    let resolved = super::resolve_local_connectors(flat, solids);
    let m = resolved.mate(seed_mate).and_then(|f| f.mate()).cloned().ok_or_else(|| CommandError::Invalid("seed mate not found".into()))?;
    let side = m.connectors.iter().position(|c| c.instance == seed).ok_or_else(|| CommandError::Invalid("the mate doesn't hold the seed".into()))?;
    let mut instances = Vec::new();
    for (k, t) in targets.iter().enumerate() {
        let mut i = Instance::new(ids(k), seed_inst.source, copy_pose(flat, solids, seed_inst, &m, side, t));
        i.replicate = Some(feature);
        instances.push(i);
    }
    let r = Replicate { seed, seed_mate, targets, instances: instances.iter().map(|i| i.id).collect() };
    Ok((r, instances))
}

/// Adds a Replicate feature (`feature`, a [`MateKind::Replicate`]) with its copies, or replaces
/// one (its old copies removed, the new ones added): one undo step.
#[derive(Debug, Clone)]
pub struct SetReplicate {
    pub element: ElementId,
    pub feature: MateFeature,
    /// The copies (see [`plan`]).
    pub instances: Vec<Instance>,
}

impl Command for SetReplicate {
    fn label(&self) -> String {
        format!("Replicate: {}", self.feature.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let MateKind::Replicate(r) = &self.feature.kind else {
            return Err(CommandError::Invalid("not a Replicate feature".into()));
        };
        let asm = assembly_mut(doc, self.element)?;
        if asm.instance(r.seed).is_none() || asm.mate(r.seed_mate).is_none() {
            return Err(CommandError::Invalid("the seed or its mate is gone".into()));
        }
        if r.targets.len() != r.instances.len() || self.instances.iter().map(|i| i.id).ne(r.instances.iter().copied()) {
            return Err(CommandError::Invalid("a copy per target".into()));
        }
        let id = self.feature.id;
        // The old copies go (kept where they are when they stay).
        let old: Vec<Instance> = asm.instances.iter().filter(|i| i.replicate == Some(id)).cloned().collect();
        asm.instances.retain(|i| i.replicate != Some(id));
        for mut i in self.instances.iter().cloned() {
            if asm.instance(i.id).is_some() {
                return Err(CommandError::Invalid("instance id already in use".into()));
            }
            i.replicate = Some(id);
            match old.iter().find(|o| o.id == i.id) {
                Some(o) => i.index = o.index,
                None => i.index = asm.next_index(&i.source),
            }
            asm.instances.push(i);
        }
        match asm.mates.iter_mut().find(|m| m.id == id) {
            Some(slot) => *slot = self.feature.clone(),
            None => asm.mates.push(self.feature.clone()),
        }
        super::folders::tidy(asm);
        Ok(())
    }
}
