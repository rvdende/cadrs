//! **Create an assembly from this ECAD data** (P3H.6; PCB7.1–PCB7.6, X7): builds, through the
//! command layer, the tabs PCB Studio makes from a board and returns them as one command
//! ([`cadrs_core::pcb::CreatePcbAssembly`], one undo step):
//!
//! - a **Part Studio** named after the board: "Sketch 1" and the extrude "Board [<board>]" whose
//!   part is "Board [<board>]" (PCB7.2, `ex3-step6-keepout-sketch.png`), and with **Keep-In and
//!   Keep-Out Areas** checked one part per place keep area (named so Sync recognises them);
//! - the **components** (PCB7.3, PCB11.1): one part per package (the `.emp` outline extruded by
//!   its height), named after the package, with its **Part number** and a **Description** made
//!   from the `.emp` PROP records ([`description`]). Where these parts live is the
//!   [`ComponentProvider`] seam: in the app, **component documents**
//!   ([`crate::component_docs::ComponentDocuments`], P3H.7: one stored document per package in
//!   the component folder, versioned, the instances version-pinned references to it,
//!   [`ComponentSource::External`], [`generate_linked`]); with no document store (pure-core
//!   tests) [`InDocumentComponents`] (P3H.6: a Part Studio "<board> components" in this
//!   document, the packages side by side along x);
//! - an **Assembly** named after the board: the board and keep instances where the Part Studio
//!   has them, and one instance per placement at its IDF position, rotation and side, **no
//!   mates** (PCB7.4), named from the package with the usual `<n>` (PCB7.5). Each component
//!   instance is tied to its placement by designator ([`cadrs_core::pcb::GeneratedAssembly`])
//!   so Sync reads it back without matching part names.
//!
//! [`generate`] works on a copy of the document (it can run off the main thread); the app puts
//! the command into the open document when it is done.

use std::collections::HashMap;

use cadrs_core::assembly::commands::InsertInstance;
use cadrs_core::assembly::{Instance, InstanceId, InstanceSource, Pose as AsmPose};
use cadrs_core::command::{Command, CommandError};
use cadrs_core::commands::{AddElement, CreateFolder, NewElementKind};
use cadrs_core::document::Document;
use cadrs_core::ids::{ElementId, FeatureId, PartId};
use cadrs_core::external::{LinkedElement, SourceRef};
use cadrs_core::pcb::component_docs::ComponentDocument;
use cadrs_core::pcb::{BoardId, CreatePcbAssembly, GeneratedAssembly, GeneratedPackage, LinkedComponent};
use cadrs_core::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};
use cadrs_core::studio::Studio;
use cadrs_idf::{Package, Placement};
use cadrs_kernel::Motion;

use crate::board::{KeepKind, PcbBoard};
use crate::colors::{BodyClass, component_kind};
use crate::geometry::{BodyPlan, MARKER, all_keep_plans, board_plan, find_package, placeholder_package};
use crate::placement::placement_motion;
use crate::sample::{build_plan, fid, name_hash};

/// "Select features to include in the assembly" (PCB7.1): Board and Components on, Keep-In and
/// Keep-Out Areas off by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateOptions {
    pub board: bool,
    pub components: bool,
    pub keep_areas: bool,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self { board: true, components: true, keep_areas: false }
    }
}

/// Where a component's part comes from: the seam between in-document parts (P3H.6, the
/// fallback without a document store) and component documents referenced by version (P3H.7,
/// [`crate::component_docs`]).
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentSource {
    /// A part of a Part Studio of this document.
    InDocument { studio: ElementId, part: PartId },
    /// A part of a component document at a version: the reference, and the id of its frozen copy
    /// in this document (`Document::linked`).
    External { reference: SourceRef, copy: ElementId, part: PartId },
}

impl ComponentSource {
    /// The assembly instance source for it.
    pub fn instance_source(&self) -> InstanceSource {
        match self {
            ComponentSource::InDocument { studio, part } => InstanceSource::Part { element: *studio, part: *part },
            ComponentSource::External { copy, part, .. } => InstanceSource::Part { element: *copy, part: *part },
        }
    }

    /// The version reference an instance of it carries (`Instance::link`).
    pub fn link(&self) -> Option<SourceRef> {
        match self {
            ComponentSource::InDocument { .. } => None,
            ComponentSource::External { reference, .. } => Some(*reference),
        }
    }
}

/// A package's part and where the package frame (the `.emp` outline's coordinates, the body from
/// z = 0 up) is in the part's own coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PackageComponent {
    pub source: ComponentSource,
    pub frame: AsmPose,
}

/// Makes (or reuses) the part of each package.
pub trait ComponentProvider {
    /// The part for the package of `placement` (`package` is its library entry; a
    /// [`placeholder_package`] when the library lacks it).
    fn component(&mut self, s: &mut dyn Studio, package: &Package) -> Result<PackageComponent, CommandError>;
    /// The part for a package the component library maps to a **custom part** (PCB11.4, X10),
    /// placed on the footprint by the mapping's transform; `None` when this provider can't use it
    /// (the package's own part is used then).
    fn custom(&mut self, _s: &mut dyn Studio, _package: &Package, _custom: &cadrs_core::pcb::CustomPart) -> Option<Result<PackageComponent, CommandError>> {
        None
    }
    /// Tidies up once every component is made (folders, …).
    fn finish(&mut self, _s: &mut dyn Studio) -> Result<(), CommandError> {
        Ok(())
    }
    /// The frozen copies the components need in this document (component documents).
    fn links(&mut self) -> Vec<LinkedElement> {
        Vec::new()
    }
    /// The component documents used (P3H.7).
    fn documents(&self) -> Vec<ComponentDocument> {
        Vec::new()
    }
    /// The packages' parts made in this document (P3H.6's components Part Studio).
    fn packages(&mut self) -> Vec<GeneratedPackage> {
        Vec::new()
    }
}

/// Gap between packages laid side by side in the components Part Studio (mm).
const GAP: f64 = 4.0;

/// The P3H.6 provider: one part per package in the Part Studio `studio` of this document, laid
/// out along +x in the order the packages are first placed (so they don't overlap there). Seeded
/// with the packages an earlier Create put in the same studio, it reuses their parts and adds
/// only the new packages after them.
pub struct InDocumentComponents {
    pub studio: ElementId,
    salt: u64,
    next_x: f64,
    made: HashMap<(String, String), PackageComponent>,
    /// Every package's part, in the order made (the seeded ones first).
    pub packages: Vec<GeneratedPackage>,
    features: Vec<FeatureId>,
}

impl InDocumentComponents {
    pub fn new(studio: ElementId, salt: u64) -> Self {
        Self::seeded(studio, salt, &[])
    }

    /// A provider that reuses `packages` (made earlier in `studio`).
    pub fn seeded(studio: ElementId, salt: u64, packages: &[GeneratedPackage]) -> Self {
        let mut me = Self { studio, salt, next_x: 0.0, made: HashMap::new(), packages: Vec::new(), features: Vec::new() };
        for g in packages {
            let key = (g.package.clone(), g.part_number.clone());
            if me.made.contains_key(&key) {
                continue;
            }
            me.made.insert(key, PackageComponent { source: ComponentSource::InDocument { studio, part: g.part }, frame: g.frame });
            me.next_x = me.next_x.max(g.x_range[1] + GAP);
            me.packages.push(g.clone());
        }
        me
    }
}

fn translated(l: &cadrs_idf::Loop, dx: f64) -> cadrs_idf::Loop {
    let mut l = l.clone();
    for p in &mut l.points {
        p.x += dx;
    }
    l
}

fn loop_x_range(loops: &[cadrs_idf::Loop]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for l in loops {
        for s in l.segments() {
            let pts: Vec<[f64; 2]> = match s {
                cadrs_idf::Segment::Line { start, end } => vec![start, end],
                cadrs_idf::Segment::Arc { start, end, center, radius, .. } => vec![start, end, [center[0] - radius, center[1]], [center[0] + radius, center[1]]],
                cadrs_idf::Segment::Circle { center, radius } => vec![[center[0] - radius, center[1]], [center[0] + radius, center[1]]],
            };
            for p in pts {
                lo = lo.min(p[0]);
                hi = hi.max(p[0]);
            }
        }
    }
    if lo > hi { (0.0, 0.0) } else { (lo, hi) }
}

impl ComponentProvider for InDocumentComponents {
    fn component(&mut self, s: &mut dyn Studio, pkg: &Package) -> Result<PackageComponent, CommandError> {
        let key = (pkg.name.clone(), pkg.part_number.clone());
        if let Some(c) = self.made.get(&key) {
            return Ok(c.clone());
        }
        let (lo, hi) = loop_x_range(&pkg.loops);
        let dx = if self.made.is_empty() { 0.0 } else { self.next_x - lo };
        let x_range = [lo + dx, hi + dx];
        self.next_x = hi + dx + GAP;
        let n = self.made.len() as u64;
        let plan = BodyPlan {
            name: pkg.name.clone(),
            class: BodyClass::Component(component_kind(&pkg.name)),
            item: None,
            loops: pkg.loops.iter().map(|l| translated(l, dx)).collect(),
            z0: 0.0,
            depth: pkg.height.max(MARKER),
        };
        let (sketch, extrude) = (fid(self.salt, 0x1000 + 2 * n), fid(self.salt, 0x1001 + 2 * n));
        let part = build_plan(s, self.studio, &plan, sketch, extrude)?;
        self.features.extend([sketch, extrude]);
        let mut values = Vec::new();
        if !pkg.part_number.is_empty() {
            values.push((PropertyKey::PartNumber, PropertyValue::Text(pkg.part_number.clone())));
        }
        if let Some(d) = description(pkg) {
            values.push((PropertyKey::Description, PropertyValue::Text(d)));
        }
        if !values.is_empty() {
            s.run(&SetProperties { owners: vec![PropertyOwner::Part { element: self.studio, part }], values, label: "Component properties".into() })?;
        }
        let c = PackageComponent { source: ComponentSource::InDocument { studio: self.studio, part }, frame: AsmPose::translation([dx, 0.0, 0.0]) };
        self.packages.push(GeneratedPackage { package: pkg.name.clone(), part_number: pkg.part_number.clone(), part, frame: c.frame, x_range });
        self.made.insert(key, c.clone());
        Ok(c)
    }

    fn packages(&mut self) -> Vec<GeneratedPackage> {
        self.packages.clone()
    }
}

/// A Description from a package's `.emp` PROP records (PCB7.6: "Resistor 113K OHM"): a
/// `DESCRIPTION` record as it is; else from the electrical value: a resistor ("RESISTANCE" in
/// ohms, with its "TOLERANCE" in %), a capacitor ("CAPACITANCE" in µF), an inductor
/// ("INDUCTANCE" in µH) or a crystal ("FREQUENCY" in Hz). `None` without any of them (thermal
/// and other records don't describe the part).
pub fn description(pkg: &Package) -> Option<String> {
    let get = |k: &str| pkg.props.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_string());
    let num = |k: &str| get(k).and_then(|v| v.parse::<f64>().ok());
    if let Some(d) = get("DESCRIPTION").filter(|d| !d.is_empty()) {
        return Some(d.trim_matches('"').to_string());
    }
    let tol = get("TOLERANCE").map(|t| format!(" ±{t}%")).unwrap_or_default();
    if let Some(r) = num("RESISTANCE") {
        return Some(format!("Resistor {} OHM{tol}", engineering(r)));
    }
    if let Some(c) = num("CAPACITANCE") {
        return Some(format!("Capacitor {}uF{tol}", cadrs_idf::fmt_num(c)));
    }
    if let Some(l) = num("INDUCTANCE") {
        return Some(format!("Inductor {}uH{tol}", cadrs_idf::fmt_num(l)));
    }
    if let Some(f) = num("FREQUENCY") {
        return Some(format!("Crystal {}Hz", engineering(f)));
    }
    None
}

/// The board part's Part number and Description: from the IDF data when a `.NOTES` record
/// carries them ("PART NUMBER: …", "P/N …", "DESCRIPTION: …"), else no Part number and the
/// Description "Board, <name>".
pub fn board_properties(b: &PcbBoard) -> (Option<String>, String) {
    let field = |keys: &[&str]| {
        b.board.notes.iter().find_map(|n| {
            let t = n.text.trim().trim_matches('"').trim();
            let up = t.to_ascii_uppercase();
            keys.iter().find_map(|k| up.strip_prefix(k).map(|_| t[k.len()..].trim_start_matches([':', '=', ' ']).trim().to_string())).filter(|v| !v.is_empty())
        })
    };
    let pn = field(&["PART NUMBER", "PART_NUMBER", "P/N"]);
    let desc = field(&["DESCRIPTION"]).unwrap_or_else(|| format!("Board, {}", b.name()));
    (pn, desc)
}

/// 113000 → "113K", 4700 → "4.7K", 2200000 → "2.2M", 16000000 → "16M", 47 → "47".
fn engineering(v: f64) -> String {
    let (x, suffix) = if v.abs() >= 1e9 {
        (v / 1e9, "G")
    } else if v.abs() >= 1e6 {
        (v / 1e6, "M")
    } else if v.abs() >= 1e3 {
        (v / 1e3, "K")
    } else {
        (v, "")
    };
    let r = (x * 1e6).round() / 1e6;
    format!("{}{suffix}", cadrs_idf::fmt_num(r))
}

/// A [`Studio`] over a bare document: each command applied directly (the whole generation is
/// one undo step of its own, [`CreatePcbAssembly`]).
pub(crate) struct Direct<'a>(pub(crate) &'a mut Document);

impl Studio for Direct<'_> {
    fn run(&mut self, c: &dyn Command) -> Result<(), CommandError> {
        c.apply(self.0)
    }
    fn document(&self) -> &Document {
        self.0
    }
}

pub(crate) fn pose_of(m: &Motion) -> AsmPose {
    let l = &m.linear;
    AsmPose {
        rotation: [[l[(0, 0)], l[(0, 1)], l[(0, 2)]], [l[(1, 0)], l[(1, 1)], l[(1, 2)]], [l[(2, 0)], l[(2, 1)], l[(2, 2)]]],
        translation: [m.translation.x, m.translation.y, m.translation.z],
    }
}

/// Where the instance of a placement goes: the part's package frame moved onto the board.
pub fn instance_pose(p: &Placement, thickness: f64, frame: &AsmPose) -> AsmPose {
    frame.inverse().then(&pose_of(&placement_motion(p, thickness)))
}

/// The name of the components Part Studio.
pub fn components_studio_name(board: &str) -> String {
    format!("{board} components")
}

/// Builds the tabs for the board `board` of the PCB Studio `pcb` (see the module docs) on a copy
/// of `doc`, with the components as in-document parts (the fallback with no document store),
/// and returns the command that adds them.
pub fn generate(doc: &Document, pcb: ElementId, board: BoardId, opts: &CreateOptions) -> Result<CreatePcbAssembly, CommandError> {
    generate_with(doc, pcb, board, opts, None)
}

/// [`generate`] with the components in **component documents** (P3H.7): `docs` makes or reuses
/// one stored document per package and the instances reference their versions.
pub fn generate_linked(doc: &Document, pcb: ElementId, board: BoardId, opts: &CreateOptions, docs: &mut crate::component_docs::ComponentDocuments) -> Result<CreatePcbAssembly, CommandError> {
    generate_with(doc, pcb, board, opts, Some(docs))
}

fn generate_with(doc: &Document, pcb: ElementId, board: BoardId, opts: &CreateOptions, docs: Option<&mut crate::component_docs::ComponentDocuments>) -> Result<CreatePcbAssembly, CommandError> {
    let sb = doc.element(pcb).and_then(|e| e.pcb()).and_then(|s| s.board(board)).ok_or_else(|| CommandError::Invalid("the board is gone".into()))?;
    let pcb_board: PcbBoard = sb.board.clone();
    let name = pcb_board.name().to_string();
    // A components Part Studio an earlier Create made (still there) is reused, with the packages
    // it already has (PCB7.3: the component documents are reused).
    let external = docs.is_some();
    let reused: Option<(ElementId, Vec<GeneratedPackage>)> = if external { None } else { doc.element(pcb).and_then(|e| e.pcb()).and_then(|st| {
        let c = st.generated.iter().rev().find_map(|g| g.components_studio.filter(|c| doc.element(*c).is_some_and(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }))))?;
        let mut packages: Vec<GeneratedPackage> = Vec::new();
        for g in st.generated.iter().filter(|g| g.components_studio == Some(c)) {
            for p in &g.packages {
                if !packages.iter().any(|q| q.package == p.package && q.part_number == p.part_number) {
                    packages.push(p.clone());
                }
            }
        }
        Some((c, packages))
    }) };
    let mut scratch = doc.clone();
    let mut s = Direct(&mut scratch);
    let (studio, assembly) = (ElementId::new(), ElementId::new());
    let components = reused.as_ref().map_or_else(ElementId::new, |(c, _)| *c);
    let salt = name_hash(&format!("{name}\u{0}{}", studio.0));
    let with_components = opts.components && !pcb_board.board.placements.is_empty();
    let in_document = with_components && !external;
    s.run(&AddElement { id: studio, kind: NewElementKind::PartStudio, name: Some(name.clone()), after: None })?;
    if in_document && reused.is_none() {
        s.run(&AddElement { id: components, kind: NewElementKind::PartStudio, name: Some(components_studio_name(&name)), after: None })?;
    }
    s.run(&AddElement { id: assembly, kind: NewElementKind::Assembly, name: Some(name.clone()), after: None })?;

    // The board and keep parts, in the board frame.
    let mut studio_parts: Vec<PartId> = Vec::new();
    let mut n = 0u64;
    let mut next = || {
        n += 1;
        (fid(salt, 2 * n), fid(salt, 2 * n + 1))
    };
    if opts.board
        && let Some(plan) = board_plan(&pcb_board)
    {
        let (sk, ex) = next();
        let part = build_plan(&mut s, studio, &plan, sk, ex)?;
        // The board's BOM row: its Part number and Description (P3H.6 judge).
        let (pn, desc) = board_properties(&pcb_board);
        let mut values = vec![(PropertyKey::Description, PropertyValue::Text(desc))];
        if let Some(pn) = pn {
            values.insert(0, (PropertyKey::PartNumber, PropertyValue::Text(pn)));
        }
        s.run(&SetProperties { owners: vec![PropertyOwner::Part { element: studio, part }], values, label: "Board properties".into() })?;
        studio_parts.push(part);
    }
    if opts.keep_areas {
        let mut keep_features = Vec::new();
        for plan in all_keep_plans(&pcb_board, |k| matches!(k.kind, KeepKind::PlaceKeepout | KeepKind::PlaceRegion | KeepKind::PlaceOutline)) {
            let (sk, ex) = next();
            studio_parts.push(build_plan(&mut s, studio, &plan, sk, ex)?);
            keep_features.extend([sk, ex]);
        }
        if !keep_features.is_empty() {
            s.run(&CreateFolder { element: studio, folder: fid(salt, 1), name: Some("Keep areas".into()), features: keep_features })?;
        }
    }
    let at = |source: InstanceSource, pose: AsmPose| Instance::new(InstanceId::new(), source, pose);
    for part in &studio_parts {
        s.run(&InsertInstance { element: assembly, instance: at(InstanceSource::Part { element: studio, part: *part }, AsmPose::IDENTITY) })?;
    }

    // The components (PCB7.3–7.5).
    let mut linked = Vec::new();
    let mut packages = Vec::new();
    let mut links: Vec<LinkedElement> = Vec::new();
    let mut documents = Vec::new();
    if with_components {
        let mut in_doc: InDocumentComponents;
        let provider: &mut dyn ComponentProvider = match docs {
            Some(d) => d,
            None => {
                in_doc = InDocumentComponents::seeded(components, salt, reused.as_ref().map_or(&[][..], |(_, p)| &p[..]));
                &mut in_doc
            }
        };
        let t = pcb_board.thickness();
        // P3H stage close (PCB11.4): a package the library maps to a custom part is inserted as
        // that part (where the provider can reference it), so a mapping change reaches the next
        // Create; None and From ECAD data use the package's own part.
        let mappings = doc.element(pcb).and_then(|e| e.pcb()).map(|st| st.library.clone()).unwrap_or_default();
        for (_, p) in pcb_board.components() {
            let pkg = find_package(&pcb_board, p).cloned().unwrap_or_else(|| placeholder_package(p));
            let custom = mappings.get(&pkg.name).custom().and_then(|c| provider.custom(&mut s, &pkg, c));
            let c = match custom {
                Some(c) => c?,
                None => provider.component(&mut s, &pkg)?,
            };
            let mut inst = at(c.source.instance_source(), instance_pose(p, t, &c.frame));
            linked.push(LinkedComponent { instance: inst.id, refdes: p.refdes.clone(), frame: c.frame });
            if let Some(r) = c.source.link() {
                // The copies first (the instance's name comes from its part there).
                for l in provider.links() {
                    if s.0.linked_element(l.id()).is_none() {
                        s.0.linked.push(l.clone());
                    }
                    if links.iter().all(|x: &LinkedElement| x.id() != l.id()) {
                        links.push(l);
                    }
                }
                inst.link = Some(r);
            }
            s.run(&InsertInstance { element: assembly, instance: inst })?;
        }
        provider.finish(&mut s)?;
        documents = provider.documents();
        packages = provider.packages();
    }

    let fresh_components = in_document && reused.is_none();
    let replaced = reused.filter(|_| in_document).and_then(|(c, _)| scratch.element(c).cloned()).into_iter().collect();
    let elements = [Some(studio), fresh_components.then_some(components), Some(assembly)]
        .into_iter()
        .flatten()
        .filter_map(|id| scratch.element(id).cloned())
        .collect();
    Ok(CreatePcbAssembly {
        element: pcb,
        board_name: name,
        elements,
        replaced,
        generated: GeneratedAssembly { board, studio, components_studio: in_document.then_some(components), assembly, components: linked, packages, documents },
        links,
    })
}
