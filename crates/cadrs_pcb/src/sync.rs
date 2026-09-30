//! **Sync a Part Studio or assembly with PCB Studio** (P3H.5, PCB5.2–5.6, PCB9.6, X8): gathers
//! a tab's parts ([`plan`]), turns them into a board on the kernel thread ([`run`]: each board
//! and keep part is its studio's exact body moved to where the tab has it, then
//! [`crate::board_from_mcad`]) and gives the command that puts the board into the PCB Studio
//! ([`cadrs_core::pcb::SyncBoard`]: a new board, or the board synced from this tab before,
//! updated in place).
//!
//! **What is translated** (PCB5.1, PCB5.4, PCB5.5): every part of a Part Studio (in its
//! coordinates), or every part of an assembly at any depth (in assembly coordinates, suppressed
//! instances left out), by its part name:
//! - **component instances**: first the instances Create assembly made (P3H.6), at any depth:
//!   each is tied to its placement by designator, with its package frame
//!   ([`cadrs_core::pcb::GeneratedAssembly`]), so it is read back whatever its part is called;
//!   otherwise (a part of a board built into a Part Studio, [`crate::sample`]) a part named
//!   "`<designator> <package>`" (or just the designator) where a board of this PCB Studio has a
//!   placement with that designator (and package). For those the package frame in the part's own
//!   coordinates is that placement's (the first sync that sees it records it on the board,
//!   [`cadrs_core::pcb::McadSource::frames`], so later syncs read a moved instance from the same
//!   frame). The placement is where the instance puts the package frame;
//! - the **board**, **keep-ins** and **keep-outs** by [`crate::names`];
//! - everything else is **not translated** and listed ("Not translated: Enclosure, …").

use cadrs_core::Document;
use std::collections::HashMap;

use cadrs_core::assembly::structure::{derive, occurrences};
use cadrs_core::assembly::{InstanceId, InstanceSource, Pose as AsmPose, source_part_name};
use cadrs_core::document::Feature;
use cadrs_core::ids::{ElementId, PartId};
use cadrs_core::pcb::{BoardId, BoardSource, PcbStudio, SyncBoard, SyncPlaneChoice};
use cadrs_core::rebuild::PendingJob;
use cadrs_idf::{IdfVersion, Library, Package, Placement};
use cadrs_kernel::Motion;
use nalgebra::{Matrix3, Vector3};

use crate::mcad::{McadInstance, McadPart, SyncPlane, board_from_mcad};
use crate::names::role_of;
use crate::placement::placement_motion;

/// A tab the Sync dialog offers: its id and its label ("Cell phone (Assembly)").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncSourceTab {
    pub element: ElementId,
    pub name: String,
    pub label: String,
}

/// The document's Part Studio and Assembly tabs, in tab order (PCB5.2's first dropdown).
pub fn sources(doc: &Document) -> Vec<SyncSourceTab> {
    doc.elements
        .iter()
        .filter_map(|e| {
            let kind = if e.assembly_model().is_some() {
                "Assembly"
            } else if matches!(e.kind, cadrs_core::document::ElementKind::PartStudio { .. }) {
                "Part Studio"
            } else {
                return None;
            };
            Some(SyncSourceTab { element: e.id, name: e.name.clone(), label: format!("{} ({kind})", e.name) })
        })
        .collect()
}

/// The plane of a choice.
pub fn sync_plane(c: SyncPlaneChoice) -> SyncPlane {
    match c {
        SyncPlaneChoice::Top => SyncPlane::Top,
        SyncPlaneChoice::Front => SyncPlane::Front,
        SyncPlaneChoice::Right => SyncPlane::Right,
    }
}

/// A part to translate: its studio's features, the part, and where the tab has it.
#[derive(Clone, Debug)]
pub struct SyncPart {
    pub name: String,
    pub features: Vec<Feature>,
    pub part: PartId,
    pub pose: AsmPose,
}

/// A component instance found by name.
#[derive(Clone, Debug)]
pub struct SyncComponent {
    pub refdes: String,
    pub package: String,
    pub part_number: String,
    /// The package frame in the part's own coordinates.
    pub frame: AsmPose,
    /// Where the tab has the part.
    pub pose: AsmPose,
}

/// What a sync will do (made on the main thread from the document, run on the kernel thread).
#[derive(Clone, Debug)]
pub struct SyncPlan {
    pub pcb: ElementId,
    pub target: Option<BoardId>,
    pub source: ElementId,
    pub source_name: String,
    pub plane: SyncPlaneChoice,
    pub parts: Vec<SyncPart>,
    pub components: Vec<SyncComponent>,
    pub library: Library,
    pub unrecognised: Vec<String>,
}

/// The result: the command, and what to tell the user.
#[derive(Clone, Debug)]
pub struct SyncOutcome {
    pub command: SyncBoard,
    /// Not translated (PCB5.4), each name once.
    pub unrecognised: Vec<String>,
    pub warnings: Vec<String>,
}

fn motion_of(p: &AsmPose) -> Motion {
    Motion { linear: Matrix3::from_row_slice(&p.rotation.concat()), translation: Vector3::from(p.translation) }
}

fn pose_of(m: &Motion) -> AsmPose {
    let l = &m.linear;
    AsmPose { rotation: [[l[(0, 0)], l[(0, 1)], l[(0, 2)]], [l[(1, 0)], l[(1, 1)], l[(1, 2)]], [l[(2, 0)], l[(2, 1)], l[(2, 2)]]], translation: [m.translation.x, m.translation.y, m.translation.z] }
}

/// The placement a part name stands for: the board synced before first, then the others.
fn component_of<'a>(name: &str, studio: &'a PcbStudio, target: Option<BoardId>) -> Option<(&'a Placement, f64, &'a Library)> {
    let mut words = name.split_whitespace();
    let refdes = words.next()?;
    let package: String = words.collect::<Vec<_>>().join(" ");
    let order = target.into_iter().chain(studio.boards.iter().map(|b| b.id).filter(|b| Some(*b) != target));
    for id in order {
        let Some(b) = studio.board(id) else { continue };
        if let Some((_, p)) = b.board.components().find(|(_, p)| p.refdes.eq_ignore_ascii_case(refdes) && (package.is_empty() || p.package == package)) {
            return Some((p, b.board.thickness(), &b.board.library));
        }
    }
    None
}

/// A component instance Create assembly made (P3H.6).
#[derive(Clone, Debug)]
struct Link {
    board: BoardId,
    refdes: String,
    frame: AsmPose,
}

/// The component instances of the Assembly tab `asm` that Create assembly tied to placements
/// of a board of `studio`, by their occurrence id in `asm` (as
/// [`cadrs_core::assembly::structure::occurrences`] derives it), at any depth.
fn linked_components(doc: &Document, studio: &PcbStudio, asm: ElementId, depth: usize) -> HashMap<InstanceId, Link> {
    let mut out = HashMap::new();
    let Some(model) = doc.element(asm).and_then(|e| e.assembly_model()) else { return out };
    if depth > 16 {
        return out;
    }
    if let Some(g) = studio.generated.iter().rev().find(|g| g.assembly == asm) {
        for c in &g.components {
            if model.instance(c.instance).is_some() {
                out.insert(c.instance, Link { board: g.board, refdes: c.refdes.clone(), frame: c.frame });
            }
        }
    }
    for inst in model.instances.iter().filter(|i| !i.suppressed) {
        if let InstanceSource::Assembly { element } = inst.source {
            for (id, l) in linked_components(doc, studio, element, depth + 1) {
                out.insert(derive(inst.id, id), l);
            }
        }
    }
    out
}

/// Gathers the parts of the tab `source` for a sync into the PCB Studio `pcb` (see the module
/// docs). The target is the board synced from `source` before (the shown one first).
pub fn plan(doc: &Document, pcb: ElementId, source: ElementId, plane: SyncPlaneChoice, shown: Option<BoardId>) -> Result<SyncPlan, String> {
    let studio = doc.element(pcb).and_then(|e| e.pcb()).ok_or("not a PCB Studio")?;
    let src = doc.element(source).ok_or("The Part Studio or assembly is gone")?;
    let target = studio.synced_from(source, shown);
    // (element, part, pose, occurrence) of every part.
    let items: Vec<(ElementId, PartId, AsmPose, Option<InstanceId>)> = if let Some(asm) = src.assembly_model() {
        occurrences(doc, asm).into_iter().map(|o| (o.element, o.part, o.pose, Some(o.id))).collect()
    } else {
        let build = cadrs_core::rebuild::build(&src.active_features());
        build.parts.iter().map(|p| (source, p.id, AsmPose::IDENTITY, None)).collect()
    };
    let linked = linked_components(doc, studio, source, 0);
    if items.is_empty() {
        return Err(format!("{} has no parts", src.name));
    }
    let frames = target.and_then(|t| studio.board(t)).and_then(|b| match &b.source {
        BoardSource::Mcad(m) => Some(m.clone()),
        _ => None,
    });
    let mut plan = SyncPlan {
        pcb,
        target,
        source,
        source_name: src.name.clone(),
        plane,
        parts: Vec::new(),
        components: Vec::new(),
        library: Library::new(IdfVersion::V3),
        unrecognised: Vec::new(),
    };
    for (element, part, pose, occurrence) in items {
        let name = source_part_name(doc, &InstanceSource::Part { element, part }, None);
        // P3H.6: an instance Create assembly made stands for its placement, by designator.
        let link = occurrence.and_then(|o| linked.get(&o));
        let found = match link {
            Some(l) => component_of(&l.refdes, studio, Some(l.board).or(target)).map(|(p, _, lib)| (p, l.frame, lib)),
            None => component_of(&name, studio, target).map(|(p, t, lib)| (p, frames.as_ref().and_then(|m| m.frame(&p.refdes)).unwrap_or_else(|| pose_of(&placement_motion(p, t))), lib)),
        };
        if let Some((p, frame, lib)) = found {
            plan.components.push(SyncComponent { refdes: p.refdes.clone(), package: p.package.clone(), part_number: p.part_number.clone(), frame, pose });
            if let Some(pk) = lib.package(&p.package, &p.part_number)
                && plan.library.package(&pk.name, &pk.part_number).is_none()
            {
                plan.library.packages.push(Package::clone(pk));
            }
            continue;
        }
        if role_of(&name).is_some() {
            let features = doc.element(element).map(|e| e.active_features()).unwrap_or_default();
            plan.parts.push(SyncPart { name, features, part, pose });
        } else if !plan.unrecognised.contains(&name) {
            plan.unrecognised.push(name);
        }
    }
    Ok(plan)
}

/// Runs a plan on the kernel thread.
pub fn run(plan: SyncPlan) -> PendingJob<Result<SyncOutcome, String>> {
    cadrs_core::rebuild::run_on_worker(move |r| run_on(r, &plan))
}

/// [`run`], waiting for the result.
pub fn run_now(plan: SyncPlan) -> Result<SyncOutcome, String> {
    run(plan).wait().unwrap_or_else(|| Err("The kernel thread stopped".into()))
}

#[cfg(feature = "occt")]
fn run_on(r: &mut cadrs_core::rebuild::Rebuilder, plan: &SyncPlan) -> Result<SyncOutcome, String> {
    let mut parts = Vec::new();
    let mut result = Ok(());
    for p in &plan.parts {
        match r.placed_body(&p.features, p.part, p.pose.rotation, p.pose.translation) {
            Ok(body) => parts.push(McadPart { name: p.name.clone(), body }),
            Err(e) => {
                result = Err(format!("{}: {e}", p.name));
                break;
            }
        }
    }
    let instances: Vec<McadInstance> = plan
        .components
        .iter()
        .map(|c| McadInstance { refdes: c.refdes.clone(), package: c.package.clone(), part_number: c.part_number.clone(), motion: motion_of(&c.frame).then(&motion_of(&c.pose)) })
        .collect();
    let out = result.and_then(|_| board_from_mcad(r.kernel(), &plan.source_name, &parts, &instances, &sync_plane(plan.plane).plane()));
    for p in parts {
        r.release_body(p.body);
    }
    let m = out?;
    let mut unrecognised = plan.unrecognised.clone();
    for n in m.unrecognised {
        if !unrecognised.contains(&n) {
            unrecognised.push(n);
        }
    }
    let mut library = plan.library.clone();
    library.header.version = IdfVersion::V3;
    let command = SyncBoard {
        element: plan.pcb,
        target: plan.target,
        name: plan.source_name.clone(),
        board: Box::new(m.board),
        library,
        keepout_parts: m.keepout_parts,
        keepin_parts: m.keepin_parts,
        source: plan.source,
        plane: plan.plane,
        frames: plan.components.iter().map(|c| (c.refdes.clone(), c.frame)).collect(),
    };
    Ok(SyncOutcome { command, unrecognised, warnings: m.warnings })
}

#[cfg(not(feature = "occt"))]
fn run_on(_r: &mut cadrs_core::rebuild::Rebuilder, _plan: &SyncPlan) -> Result<SyncOutcome, String> {
    Err("Sync needs the solid-modelling kernel".into())
}

/// The toast after a sync: the board and what wasn't translated (PCB5.4).
pub fn message(o: &SyncOutcome, board: &str) -> String {
    let what = if o.command.target.is_some() { format!("Updated {board}") } else { format!("Added {board}") };
    let mut s = what;
    if !o.unrecognised.is_empty() {
        s.push_str(&format!(". Not translated: {}", o.unrecognised.join(", ")));
    }
    if let Some(w) = o.warnings.first() {
        s.push_str(&format!(". {w}"));
    }
    s
}
