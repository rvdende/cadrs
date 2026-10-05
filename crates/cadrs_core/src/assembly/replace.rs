//! **Replace instances** (P3B.9, `intro-to-assemblies.md` X15): one or more part instances take
//! another part as their source, keeping their placements, numbers' order and mates. Each mate
//! connector on a replaced instance is found again on the new part ([`match_connector`]): by its
//! persistent names when the new part has them (the same Part Studio's part), else by geometry —
//! the implicit point of the same kind with the same origin and axis (the new part's own
//! coordinates, where the instance's placement puts it), the surface of the same kind in the
//! same place, the explicit connector at the same frame. A mate whose connector has no match
//! is removed (and a relation on it), as the dialog reports. One undo step ([`ReplaceInstances`]).

use std::collections::HashMap;
use std::sync::Arc;

use super::commands::assembly_mut;
use super::connector::{ConnectorAnchor, ConnectorFrame, EntityRef, ImplicitConnector, ImplicitPoint, MateConnector, implicit_points, resolve_implicit, surface_of};
use super::mate::{MateFeature, MateId, MateKind};
use super::{Assembly, InstanceId, InstanceSource};
use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;
use crate::solid::Solid;

/// Distance (mm) and direction tolerances of a geometric match.
const TOL: f64 = 1e-4;
const DIR_TOL: f64 = 1e-6;

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < TOL)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Every implicit point of every face and edge of `s` (not merged where they coincide: a circle's
/// centre and a face's centroid at the same place are both kept).
fn every_point(s: &Solid) -> Vec<ImplicitConnector> {
    let faces = s.faces.iter().map(|f| EntityRef::Face(f.name));
    let edges = s.edges.iter().map(|e| EntityRef::Edge(e.name));
    faces.chain(edges).flat_map(|e| implicit_points(s, &e)).collect()
}

fn same_kind(a: &ImplicitPoint, b: &ImplicitPoint) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// `c` with its anchor moved from the part `old` to the part `new` (both in the instance's own
/// coordinates), keeping its frame: by name, then by geometry. `None` when nothing matches.
pub fn match_connector(c: &MateConnector, old: &Solid, new: &Solid) -> Option<MateConnector> {
    let before = c.base_frame(Some(old));
    // The same axis, or the opposite one (the flip then toggles); X brought back by reorient.
    let fit = |mut out: MateConnector, f: ConnectorFrame| -> Option<MateConnector> {
        if !close(f.origin, before.origin) {
            return None;
        }
        let d = dot(f.z, before.z);
        if (d.abs() - 1.0).abs() > DIR_TOL {
            return None;
        }
        let raw = f;
        let f = if d < 0.0 {
            out.flip = !out.flip;
            f.flipped()
        } else {
            f
        };
        // Quarter turns about Z that bring X back (the flip and reorient apply after).
        let q = (0..4u8).max_by(|a, b| dot(f.reoriented(*a).x, before.x).total_cmp(&dot(f.reoriented(*b).x, before.x))).unwrap_or(0);
        out.reorient = (out.reorient + q) % 4;
        out.frame = raw;
        Some(out)
    };
    match c.anchor {
        ConnectorAnchor::Implicit { point, owner } => {
            // By name (the same Part Studio's part, or one sharing its history).
            if let Some(f) = resolve_implicit(new, &point, &owner)
                && close(f.origin, before.origin)
                && (dot(f.z, before.z) - 1.0).abs() < DIR_TOL
            {
                return Some(MateConnector { frame: f, ..*c });
            }
            // By geometry: the same kind of point, where it was, on a circle or cylinder of the
            // same size.
            let olds = every_point(old);
            let size = olds.iter().find(|p| p.point == point && p.owner == owner).or_else(|| olds.iter().find(|p| p.point == point)).and_then(|p| p.diameter);
            let same_size = |d: Option<f64>| match (d, size) {
                (Some(a), Some(b)) => (a - b).abs() < TOL,
                (None, None) => true,
                _ => false,
            };
            let mut cands: Vec<_> = every_point(new).into_iter().filter(|p| same_kind(&p.point, &point) && same_size(p.diameter)).collect();
            // A point owned by the same kind of entity first (a face's centroid over an edge's).
            let owner_kind = std::mem::discriminant(&owner);
            cands.sort_by_key(|p| std::mem::discriminant(&p.owner) != owner_kind);
            cands.into_iter().find_map(|p| fit(MateConnector { anchor: ConnectorAnchor::Implicit { point: p.point, owner: p.owner }, ..*c }, p.frame))
        }
        ConnectorAnchor::Surface { entity, kind } => {
            if let Some((f, k)) = surface_of(new, &entity)
                && k == kind
                && close(f.origin, before.origin)
            {
                return Some(MateConnector { frame: f, ..*c });
            }
            let entities = new
                .faces
                .iter()
                .map(|f| EntityRef::Face(f.name))
                .chain(new.edges.iter().map(|e| EntityRef::Edge(e.name)))
                .chain(new.vertices.iter().map(|v| EntityRef::Vertex(v.name)));
            for e in entities {
                let Some((f, k)) = surface_of(new, &e) else { continue };
                if k != kind || (dot(f.z, before.z).abs() - 1.0).abs() > DIR_TOL {
                    continue;
                }
                // The same plane / axis / centre: the old origin lies on it.
                let d = [before.origin[0] - f.origin[0], before.origin[1] - f.origin[1], before.origin[2] - f.origin[2]];
                let on = match kind {
                    super::connector::SurfaceKind::Plane => dot(d, f.z).abs() < TOL,
                    super::connector::SurfaceKind::Cylinder { .. } | super::connector::SurfaceKind::Line => {
                        let t = dot(d, f.z);
                        close([f.z[0] * t, f.z[1] * t, f.z[2] * t], d)
                    }
                    _ => close(f.origin, before.origin),
                };
                if on {
                    return Some(MateConnector { anchor: ConnectorAnchor::Surface { entity: e, kind }, frame: before, ..*c });
                }
            }
            None
        }
        ConnectorAnchor::Explicit { .. } => new.connectors.iter().find_map(|sc| {
            let f = ConnectorFrame::new(sc.frame.origin, sc.frame.normal(), sc.frame.u);
            fit(MateConnector { anchor: ConnectorAnchor::Explicit { feature: sc.feature }, ..*c }, f)
        }),
        // A fixed frame, or an assembly connector: it stays where it is on the instance.
        ConnectorAnchor::Frame | ConnectorAnchor::Local { .. } => Some(*c),
    }
}

/// What replacing `instances` of `asm` with parts `new` (its solid, per instance) does to the
/// mates: the mates whose connectors all match, rewritten, and the ones to remove.
/// `old` gives each instance's current source solid.
pub fn plan(
    asm: &Assembly,
    instances: &[InstanceId],
    old: &HashMap<InstanceId, Arc<Solid>>,
    new: &Solid,
) -> (Vec<MateFeature>, Vec<MateId>) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for f in &asm.mates {
        let MateKind::Mate(m) = &f.kind else { continue };
        if !m.all_connectors().any(|c| instances.contains(&c.instance)) {
            continue;
        }
        let mut m2 = m.clone();
        let mut ok = true;
        for c in m2.connectors.iter_mut().chain(m2.tabs.iter_mut()) {
            if !instances.contains(&c.instance) {
                continue;
            }
            let Some(o) = old.get(&c.instance) else {
                ok = false;
                break;
            };
            match match_connector(c, o, new) {
                Some(n) => *c = n,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            kept.push(MateFeature { kind: MateKind::Mate(m2), ..f.clone() });
        } else {
            dropped.push(f.id);
        }
    }
    (kept, dropped)
}

/// **Replace instances** (X15): `instances` take `source` (a part of a Part Studio), numbered
/// after its other instances; `mates` replace the mates of the same ids (their connectors on the
/// new part, see [`plan`]) and `dropped` are removed, with relations on them. One undo step.
#[derive(Debug, Clone)]
pub struct ReplaceInstances {
    pub element: ElementId,
    pub instances: Vec<InstanceId>,
    pub source: InstanceSource,
    pub mates: Vec<MateFeature>,
    pub dropped: Vec<MateId>,
}

impl Command for ReplaceInstances {
    fn label(&self) -> String {
        if self.instances.len() == 1 { "Replace instance".into() } else { "Replace instances".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let InstanceSource::Part { element: studio, .. } = self.source else {
            return Err(CommandError::Invalid("instances are replaced by a part".into()));
        };
        if !doc.element(studio).is_some_and(|e| matches!(e.kind, crate::document::ElementKind::PartStudio { .. })) {
            return Err(CommandError::Invalid("the part is not in a Part Studio of this document".into()));
        }
        let asm = assembly_mut(doc, self.element)?;
        if self.instances.is_empty() {
            return Err(CommandError::Invalid("no instances".into()));
        }
        for id in &self.instances {
            let Some(i) = asm.instance(*id) else {
                return Err(CommandError::Invalid(format!("instance {id} not found")));
            };
            if i.source.part().is_none() {
                return Err(CommandError::Invalid("only part instances are replaced".into()));
            }
        }
        for id in &self.instances {
            if asm.instance(*id).is_some_and(|i| i.source == self.source) {
                continue;
            }
            let n = asm.next_index(&self.source);
            let i = asm.instance_mut(*id).expect("checked");
            i.source = self.source;
            i.index = n;
        }
        for m in &self.mates {
            if let Some(slot) = asm.mates.iter_mut().find(|f| f.id == m.id) {
                *slot = m.clone();
            }
        }
        asm.retain_mates(|f| !self.dropped.contains(&f.id));
        asm.drop_orphan_relations();
        super::folders::tidy(asm);
        Ok(())
    }
}
