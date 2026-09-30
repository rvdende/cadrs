//! Reading an Import's file with its assembly structure (P3F.2, [`crate::import::structure`]):
//! IGES, and STEP when [`ImportFeature::structure`] is set. The kernel's XDE reader gives each
//! distinct part once and its occurrences; this makes each distinct part where its first
//! occurrence is ([`ImportMode::Parts`]) or every occurrence where the file puts it
//! ([`ImportMode::Flatten`]), then one body per solid, in order, named as
//! [`crate::import::ImportPlan::part_names`] says. The Import feature's rebuild
//! (`linked.rs`) makes the parts of them. A child of `rebuild::kernel_ops`, so it shares its
//! helpers.

use super::*;
use cadrs_kernel::{Kernel, Motion};

use crate::import::{ImportFeature, ImportMode};

impl Rebuilder {
    /// The bodies (one per solid) and part names of `x`'s file (`bytes`), read with its
    /// structure in `mode`.
    pub(in crate::rebuild) fn read_structured(&mut self, x: &ImportFeature, bytes: &[u8], mode: ImportMode) -> Result<Vec<(BodyId, String)>, String> {
        let format = x.format.kernel().ok_or_else(|| format!("{} files have no assembly structure", x.format.label()))?;
        let model = self.kernel.import_model(format, bytes).map_err(|e| format!("Import failed: {e}"))?;
        let plan = self.plan_of(x.format, &model);
        let originals: Vec<BodyId> = model.parts.iter().map(|p| p.body).collect();
        // Where each body goes: a part at its first occurrence, or every occurrence.
        let wanted: Vec<(usize, Motion, String)> = match mode {
            ImportMode::Parts => (0..originals.len())
                .map(|i| {
                    let first = model.occurrences.iter().find(|o| o.part == i).map_or(Motion::identity(), |o| o.placement);
                    (i, first, format!("part {}", i + 1))
                })
                .collect(),
            ImportMode::Flatten => model.occurrences.iter().map(|o| (o.part, o.placement, o.name.clone())).collect(),
        };
        let mut placed: Vec<BodyId> = Vec::new();
        let mut failed = None;
        for (part, placement, what) in &wanted {
            let body = originals[*part];
            // A copy even where nothing moves, so every placed body is its own.
            let moved = if *placement == Motion::identity() {
                self.kernel.transform(body, &cadrs_kernel::Transform::identity())
            } else {
                self.kernel.transform_motion(body, placement)
            };
            match moved {
                Ok(r) => placed.push(r.bodies[0]),
                Err(e) => {
                    failed = Some(format!("Placing {what} failed: {e}"));
                    break;
                }
            }
        }
        originals.iter().for_each(|b| self.kernel.release(*b));
        if let Some(e) = failed {
            placed.iter().for_each(|b| self.kernel.release(*b));
            return Err(e);
        }
        // One body per solid.
        let mut bodies: Vec<BodyId> = Vec::new();
        let mut failed = None;
        for b in &placed {
            if failed.is_none() {
                match self.kernel.split_solids(*b) {
                    Ok(rs) => bodies.extend(rs.into_iter().flat_map(|r| r.bodies)),
                    Err(e) => failed = Some(format!("Import failed: {e}")),
                }
            }
            self.kernel.release(*b);
        }
        if let Some(e) = failed {
            bodies.iter().for_each(|b| self.kernel.release(*b));
            return Err(e);
        }
        if bodies.is_empty() {
            return Err("The file holds no parts".into());
        }
        let names = plan.part_names(mode);
        let stem = x.stem();
        Ok(bodies
            .into_iter()
            .enumerate()
            .map(|(k, b)| (b, names.get(k).cloned().unwrap_or_else(|| if k == 0 { stem.clone() } else { format!("{stem} ({})", k + 1) })))
            .collect())
    }
}
