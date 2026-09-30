//! **Check interference** (P3B.9, `intro-to-assemblies.md` X15): which parts of an assembly
//! overlap, and by how much. Every part at any depth ([`super::structure::occurrences`]) is its
//! Part Studio's exact kernel body moved to its placement; each pair whose bounding boxes meet is
//! intersected (the kernel's boolean **Intersect**) and the pair interferes when the shared solid
//! has a volume (touching faces share none). Runs on the kernel thread
//! ([`crate::rebuild::run_on_worker`]).

use serde::{Deserialize, Serialize};

use super::structure::occurrences;
use super::{Assembly, InstanceId, Pose};
use crate::document::{Document, Feature};
use crate::ids::PartId;
use crate::rebuild::PendingJob;

/// Smaller shared volumes (mm³) count as touching.
pub const MIN_VOLUME: f64 = 1e-6;

/// One part to check: its Part Studio's features, the part, and where it is.
#[derive(Debug, Clone)]
pub struct Item {
    /// Its occurrence id (the solver's), its top-level instance and its part in the view.
    pub occurrence: InstanceId,
    pub top: InstanceId,
    pub view_part: PartId,
    pub features: Vec<Feature>,
    pub part: PartId,
    pub pose: Pose,
}

/// Two parts that overlap, and the volume they share (mm³).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clash {
    pub a: PartId,
    pub b: PartId,
    pub volume: f64,
    /// The shared volume's triangles (assembly coordinates), to draw it (Final part 3).
    #[serde(default, skip)]
    pub triangles: Vec<[[f32; 3]; 3]>,
}

/// The parts of `asm` to check: every part at any depth (suppressed instances left out). With
/// `among` given, only pairs with at least one part of those top-level instances are checked.
pub fn items(doc: &Document, asm: &Assembly) -> Vec<Item> {
    occurrences(doc, asm)
        .into_iter()
        .filter_map(|o| {
            let features = doc.element(o.element)?.active_features();
            Some(Item { occurrence: o.id, top: o.top, view_part: o.view_part, features, part: o.part, pose: o.pose })
        })
        .collect()
}

/// The pairs to check: parts of different top-level instances (a rigid subassembly's own parts
/// are checked against each other too), at least one of them among `among` (all when empty).
pub fn pairs(items: &[Item], among: &[InstanceId]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 0..items.len() {
        for j in i + 1..items.len() {
            let (a, b) = (&items[i], &items[j]);
            if a.occurrence == b.occurrence {
                continue;
            }
            if !among.is_empty() && !among.contains(&a.top) && !among.contains(&b.top) {
                continue;
            }
            out.push((i, j));
        }
    }
    out
}

/// Checks `items` on the kernel thread: the clashes, largest first.
pub fn check(items: Vec<Item>, among: Vec<InstanceId>) -> PendingJob<Result<Vec<Clash>, String>> {
    crate::rebuild::run_on_worker(move |r| run(r, &items, &among))
}

/// [`check`], waiting for the result.
pub fn check_now(items: Vec<Item>, among: Vec<InstanceId>) -> Result<Vec<Clash>, String> {
    check(items, among).wait().unwrap_or_else(|| Err("The kernel thread stopped".into()))
}

#[cfg(feature = "occt")]
fn run(r: &mut crate::rebuild::Rebuilder, items: &[Item], among: &[InstanceId]) -> Result<Vec<Clash>, String> {
    let pairs = pairs(items, among);
    let mut needed: Vec<usize> = pairs.iter().flat_map(|(a, b)| [*a, *b]).collect();
    needed.sort_unstable();
    needed.dedup();
    let mut bodies: std::collections::HashMap<usize, cadrs_kernel::BodyId> = std::collections::HashMap::new();
    let mut boxes = std::collections::HashMap::new();
    let mut result = Ok(());
    for k in needed {
        let it = &items[k];
        match r.placed_body(&it.features, it.part, it.pose.rotation, it.pose.translation) {
            Ok(b) => {
                if let Some(bx) = r.body_box(b) {
                    boxes.insert(k, bx);
                }
                bodies.insert(k, b);
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    let mut out = Vec::new();
    if result.is_ok() {
        let meet = |a: &([f64; 3], [f64; 3]), b: &([f64; 3], [f64; 3])| (0..3).all(|i| a.0[i] <= b.1[i] + 1e-6 && b.0[i] <= a.1[i] + 1e-6);
        for (i, j) in pairs {
            if let (Some(a), Some(b)) = (boxes.get(&i), boxes.get(&j))
                && !meet(a, b)
            {
                continue;
            }
            let (Some(a), Some(b)) = (bodies.get(&i), bodies.get(&j)) else { continue };
            match r.common_volume_mesh(*a, *b) {
                Ok((v, triangles)) if v > MIN_VOLUME => out.push(Clash { a: items[i].view_part, b: items[j].view_part, volume: v, triangles }),
                Ok(_) => {}
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
    }
    for b in bodies.into_values() {
        r.release_body(b);
    }
    result?;
    out.sort_by(|a, b| b.volume.total_cmp(&a.volume));
    Ok(out)
}

#[cfg(not(feature = "occt"))]
fn run(_r: &mut crate::rebuild::Rebuilder, _items: &[Item], _among: &[InstanceId]) -> Result<Vec<Clash>, String> {
    Err("Check interference needs the solid-modelling kernel".into())
}

/// P3B.9 (X15 Export…): STEP files of assembly parts (`items`, each with its product name), as
/// they are placed in the assembly (turned −90° about X first with `y_up`); one file per part
/// (`individual`) or one with them all. Runs on the kernel thread.
pub fn export_step(items: Vec<(Item, String)>, y_up: bool, individual: bool) -> PendingJob<Result<Vec<crate::export::StepFile>, String>> {
    crate::rebuild::run_on_worker(move |r| export_run(r, &items, y_up, individual))
}

#[cfg(feature = "occt")]
fn export_run(r: &mut crate::rebuild::Rebuilder, items: &[(Item, String)], y_up: bool, individual: bool) -> Result<Vec<crate::export::StepFile>, String> {
    if items.is_empty() {
        return Err("No parts to export".into());
    }
    let turn = if y_up { Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], -std::f64::consts::FRAC_PI_2) } else { Pose::IDENTITY };
    let mut bodies = Vec::new();
    let mut result = Ok(());
    for (it, _) in items {
        let pose = it.pose.then(&turn);
        match r.placed_body(&it.features, it.part, pose.rotation, pose.translation) {
            Ok(b) => bodies.push(b),
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    let files = result.and_then(|_| {
        let groups: Vec<Vec<usize>> = if individual { (0..items.len()).map(|k| vec![k]).collect() } else { vec![(0..items.len()).collect()] };
        groups
            .into_iter()
            .map(|g| {
                let ids: Vec<cadrs_kernel::BodyId> = g.iter().map(|k| bodies[*k]).collect();
                let names: Vec<&str> = g.iter().map(|k| items[*k].1.as_str()).collect();
                let bytes = r.step_of(&ids, &names)?;
                Ok(crate::export::StepFile { part: (g.len() == 1).then(|| names[0].to_string()), bytes })
            })
            .collect::<Result<Vec<_>, String>>()
    });
    for b in bodies {
        r.release_body(b);
    }
    files
}

#[cfg(not(feature = "occt"))]
fn export_run(_r: &mut crate::rebuild::Rebuilder, _items: &[(Item, String)], _y_up: bool, _individual: bool) -> Result<Vec<crate::export::StepFile>, String> {
    Err("STEP export needs the solid-modelling kernel".into())
}
